use crate::core::models::compact_graph::Graph as CompactGraph;
use crate::core::typed_facts::{decode_facts, encode_facts, TypedFacts};
use crate::engine::languages::module_name;
use crate::{
    core::{commitments, models::*, schema::SCHEMA_DDL},
    engine::{
        ast, docs, linker,
        scanner::{self, FileEntry},
    },
};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use rusqlite::{
    limits::Limit, params, params_from_iter, types::Value as SqlValue, Connection, OpenFlags,
    OptionalExtension, ToSql,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::Instant,
};
#[path = "batch.rs"]
mod batch;
#[path = "delta.rs"]
mod delta;
#[path = "dictionary.rs"]
mod dictionary;
#[path = "ingest.rs"]
mod ingest;

use batch::*;
pub use delta::delta;
use dictionary::*;
use ingest::*;

static NONCE: AtomicU64 = AtomicU64::new(0);
// Bound source, parser-tree, and extracted-fact working memory per active file.
const EXTRACTION_MEMORY_MULTIPLIER: u64 = 8;
const MAX_EXTRACTION_BATCH_FILES: usize = 25_000;
const MAX_MULTI_VALUE_BATCH_ROWS: usize = 250;

fn sqlite_cache_size_kib(memory_budget: u64) -> u64 {
    (memory_budget / 8 / 1024).clamp(4_096, 128_000)
}

#[derive(Debug)]
/// Reports that a source file changed after its inventory digest was scanned.
pub struct SourceSnapshotMismatch(pub String);

impl std::fmt::Display for SourceSnapshotMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SourceSnapshotMismatch {}

/// Extracts one scanned file and rejects changes to its source snapshot.
///
/// # Errors
///
/// Returns [`SourceSnapshotMismatch`] if the file no longer matches its digest.
pub fn extract(root: &Path, file: &FileEntry, adapter: &scanner::Adapter) -> Result<Facts> {
    Ok(extract_typed(root, file, adapter)?.into_public())
}

fn extract_typed(root: &Path, file: &FileEntry, adapter: &scanner::Adapter) -> Result<TypedFacts> {
    let path = scanner::resolve_file_path(root, adapter, &file.path)?;
    let source = fs::read_to_string(&path)?;
    if commitments::hash(source.as_bytes()) != file.digest {
        return Err(SourceSnapshotMismatch(format!(
            "source changed during extraction: {}",
            file.path
        ))
        .into());
    }
    if file.is_doc {
        let sections = docs::extract(&file.path, &source, file.mtime_ns as f64 / 1e9)?;
        let nodes = sections
            .iter()
            .map(|d| Node {
                id: d.doc_id.clone(),
                kind: "document".into(),
                name: d.section_title.clone(),
                qualname: d.doc_id.clone(),
                path: d.path.clone(),
                line: 1,
                end_line: source.lines().count().max(1),
                is_test: false,
                language: "markdown".into(),
                generated: false,
                details: json!({"doc_type":d.doc_type,"is_invariant":d.is_invariant}),
            })
            .collect();
        Ok(TypedFacts::from_public(Facts {
            docs: sections,
            nodes,
            ..Facts::default()
        }))
    } else {
        ast::extract_typed(&file.path, &file.language, &source)
    }
}
fn policy(root: &Path, adapter: &scanner::Adapter) -> Value {
    json!({
        "adapter_path": adapter.adapter_path.as_ref().map(|path| path.strip_prefix(root).unwrap_or(path).to_string_lossy()),
        "roots": adapter.roots.iter().map(|p| p.strip_prefix(root).unwrap_or(p).to_string_lossy()).collect::<Vec<_>>(),
        "ignored_names": adapter.ignored_names,
        "adapter_version": adapter.adapter_version,
        "digest": adapter.digest,
        "linked_workspaces": adapter.linked_workspaces,
        "owners": adapter.owners,
        "aliases": adapter.aliases,
    })
}
fn read_policy(root: &Path, conn: &Connection) -> Result<scanner::Adapter> {
    let raw: String =
        conn.query_row("SELECT value FROM metadata WHERE key='adapter'", [], |r| {
            r.get(0)
        })?;
    let value: Value = serde_json::from_str(&raw)?;
    let roots = value["roots"]
        .as_array()
        .context("invalid persisted roots")?
        .iter()
        .map(|v| {
            let p = v.as_str().context("invalid root")?;
            if p.is_empty() {
                Ok(root.to_path_buf())
            } else {
                Ok(scanner::checked_child(root, Path::new(p))?)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let ignored_names = value["ignored_names"]
        .as_array()
        .context("invalid ignore policy")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).context("invalid ignore name"))
        .collect::<Result<BTreeSet<_>>>()?;
    let adapter_version = value["adapter_version"].as_str().map(str::to_owned);
    let adapter_path = match value.get("adapter_path") {
        None | Some(Value::Null) => None,
        Some(path) => Some(scanner::checked_child(
            root,
            Path::new(path.as_str().context("invalid persisted adapter path")?),
        )?),
    };
    let digest = value["digest"].as_str().unwrap_or_default().to_owned();
    let linked_workspaces: Vec<scanner::LinkedWorkspace> = value
        .get("linked_workspaces")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let owners: std::collections::BTreeMap<String, String> = value
        .get("owners")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let aliases: std::collections::BTreeMap<String, String> = value
        .get("aliases")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    Ok(scanner::Adapter {
        adapter_path,
        response: Default::default(),
        limits: Default::default(),
        roots,
        ignored_names,
        adapter_version,
        digest,
        linked_workspaces,
        owners,
        aliases,
        debug: false,
    })
}
fn populate(
    conn: &mut Connection,
    root: &Path,
    adapter: &scanner::Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
) -> Result<Value> {
    let budget = scanner::memory_budget_bytes();
    let cache_kb = sqlite_cache_size_kib(budget);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=OFF;"
    ))?;
    conn.set_prepared_statement_cache_capacity(128);
    let (tables, indexes) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    conn.execute_batch(tables)?;
    let linking = Instant::now();
    let dependencies =
        crate::engine::languages::manifests::DependencyRegistry::collect_with_adapter(
            root,
            Some(adapter),
        );
    let graph = linker::link_compact_with_typed_registry(facts, None, Some(root), &dependencies);
    let link_ms = linking.elapsed().as_secs_f64() * 1000.;
    let writing = Instant::now();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    cached(
        &tx,
        "INSERT INTO node_search(node_search) VALUES('delete-all')",
        [],
    )?;
    cached(
        &tx,
        "INSERT INTO doc_search(doc_search) VALUES('delete-all')",
        [],
    )
    .ok();
    cached(&tx, "DELETE FROM shared_keys", [])?;
    for table in commitments::DOMAINS {
        cached(&tx, &format!("DELETE FROM {table}"), [])?;
    }
    cached(&tx, "DELETE FROM path_dictionary", [])?;
    cached(&tx, "DELETE FROM domain_commitments", [])?;
    cached(&tx, "DELETE FROM coverage_evidence", [])?;
    cached(&tx, "DELETE FROM coverage_expressions", [])?;
    for (k, v) in [
        ("schema_version", scanner::ENGINE_SCHEMA_VERSION.to_owned()),
        (
            "index_semantics_version",
            scanner::INDEX_SEMANTICS_VERSION.to_owned(),
        ),
        ("indexer_engine", "contextunity-forge-mcp-rust".to_owned()),
        ("workspace_root", root.to_string_lossy().into_owned()),
        ("adapter", policy(root, adapter).to_string()),
        (
            "adapter_version",
            adapter.adapter_version.clone().unwrap_or_default(),
        ),
        ("adapter_digest", adapter.digest.clone()),
        ("manifest_digest", dependencies.digest().to_owned()),
        (
            "adapter_linked_workspaces",
            serde_json::to_string(&adapter.linked_workspaces)?,
        ),
        ("inventory_snapshot", serde_json::to_string(entries)?),
        (
            "corpus_hash",
            commitments::hash(&serde_json::to_vec(entries)?),
        ),
    ] {
        cached(
            &tx,
            "INSERT OR REPLACE INTO metadata VALUES(?1,?2)",
            params![k, v],
        )?;
    }
    let mut node_paths: hashbrown::HashMap<String, Option<CachedNodePath>> =
        hashbrown::HashMap::with_capacity(facts.values().map(|f| f.nodes.len()).sum());
    for f in facts.values() {
        for n in &f.nodes {
            node_paths.insert(
                n.id.clone(),
                Some(CachedNodePath::new(&n.id, n.path.clone())),
            );
        }
    }
    let mut path_cache = PathDictionaryCache::new();
    let all_paths = entries
        .iter()
        .map(|e| e.path.as_str())
        .chain(graph.edges.iter().map(|e| e.path.as_str()))
        .chain(graph.coverage.iter().map(|c| c.path.as_str()));
    let t_bulk = Instant::now();
    path_cache.bulk_insert(&tx, all_paths)?;
    let bulk_paths_ms = t_bulk.elapsed().as_secs_f64() * 1000.;

    let t_files = Instant::now();
    let (node_id, docs_count, fts_insert_ms) =
        persist_files(&tx, &mut path_cache, entries, facts, encoded_facts, true)?;
    let persist_files_ms = t_files.elapsed().as_secs_f64() * 1000. - fts_insert_ms;

    let t_doc_fts = Instant::now();
    if docs_count > 0 {
        tx.execute(
            "INSERT INTO doc_search(rowid,section_title,content,invariants) SELECT rowid,section_title,content,invariants FROM doc_sections",
            [],
        )?;
    }
    let doc_fts_insert_ms = t_doc_fts.elapsed().as_secs_f64() * 1000.;

    let t_graph = Instant::now();
    let graph_timings = persist_graph(&tx, &mut path_cache, &graph, &mut node_paths, true)?;
    let persist_graph_ms = t_graph.elapsed().as_secs_f64() * 1000.;

    let rows_ms = writing.elapsed().as_secs_f64() * 1000.;
    tx.commit()?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let index_and_seal = Instant::now();
    let index_sql = format!("CREATE INDEX{indexes}");
    let index_candidate = |conn: &mut Connection| -> Result<f64> {
        let started = Instant::now();
        conn.execute_batch("PRAGMA cache_size=-262144; PRAGMA temp_store=MEMORY;")?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(&index_sql)?;
        tx.commit()?;
        conn.execute_batch(&format!("PRAGMA cache_size=-{cache_kb};"))?;
        Ok(started.elapsed().as_secs_f64() * 1000.)
    };
    let indexes_ms = index_candidate(conn)?;
    let started = Instant::now();
    let seal = commitments::seal_snapshot(conn)?;
    let seal_ms = started.elapsed().as_secs_f64() * 1000.;
    let index_and_seal_ms = index_and_seal.elapsed().as_secs_f64() * 1000.;
    let persist_ms = writing.elapsed().as_secs_f64() * 1000.;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    let mut report = json!({"files":entries.len(),"nodes":node_id,"edges":graph.edges.len(),"doc_sections":docs_count,"output_root":seal,"schema_version":scanner::ENGINE_SCHEMA_VERSION,"link_ms":link_ms,"persist_ms":persist_ms,"rows_ms":rows_ms,"bulk_paths_ms":bulk_paths_ms,"persist_files_ms":persist_files_ms,"fts_insert_ms":fts_insert_ms,"doc_fts_insert_ms":doc_fts_insert_ms,"persist_graph_ms":persist_graph_ms,"indexes_ms":indexes_ms,"seal_ms":seal_ms});
    report["graph_phases_ms"] = json!({"occurrences_and_dependencies":graph_timings.occurrences,"unique_edges":graph_timings.edges,"coverage_dictionaries":graph_timings.dictionaries,"coverage_and_unresolved":graph_timings.coverage});
    report["index_and_seal_ms"] = json!(index_and_seal_ms);
    Ok(report)
}
fn components(facts: &mut BTreeMap<String, TypedFacts>) {
    let owners = component_owners(facts.iter().filter_map(|(path, f)| {
        f.nodes
            .iter()
            .any(|n| n.kind == "module")
            .then_some(path.as_str())
    }));
    assign_components(facts, &owners);
}
fn component_owners<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, String> {
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    for path in paths {
        let root = path.split_once('/').map_or(".", |(root, _)| root);
        owners
            .entry(root.to_owned())
            .and_modify(|owner| {
                if path < owner.as_str() {
                    *owner = path.to_owned();
                }
            })
            .or_insert_with(|| path.to_owned());
    }
    owners
}
fn assign_components(facts: &mut BTreeMap<String, TypedFacts>, owners: &BTreeMap<String, String>) {
    for (path, f) in facts {
        f.nodes.retain(|n| n.kind != "component");
        f.edges.retain(|e| !e.src.starts_with("component:"));
        if !f.nodes.iter().any(|n| n.kind == "module") {
            continue;
        }
        let root = path.split_once('/').map_or(".", |(root, _)| root);
        let id = format!("component:{root}");
        if owners.get(root) == Some(path) {
            f.nodes.push(Node {
                id: id.clone(),
                kind: "component".into(),
                name: root.into(),
                qualname: root.into(),
                path: root.into(),
                line: 1,
                end_line: 1,
                is_test: false,
                language: "workspace".into(),
                generated: false,
                details: json!({}),
            });
        }
        f.edges.push(Edge {
            src: id,
            dst: format!("module:{path}"),
            kind: "contains".into(),
            path: path.clone(),
            line: 1,
            evidence: "component ownership".into(),
            confidence: "exact".into(),
        });
    }
}
fn atomic_build(
    root: &Path,
    output: &Path,
    adapter: &scanner::Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
) -> Result<Value> {
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        std::env::current_dir()?.join(output)
    };
    let parent = output.parent().context("output needs a parent")?;
    let mut ancestor = PathBuf::new();
    for part in parent.components() {
        ancestor.push(part);
        if fs::symlink_metadata(&ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("output parent must not traverse a symlink");
        }
    }
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    if fs::symlink_metadata(&output).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("output must not be a symlink");
    }
    let stage = parent.join(format!(
        ".forge-stage-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&stage)?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(&stage)?;
    }
    let temporary = stage.join("generation.sqlite");
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&temporary)?;
        let mut conn = Connection::open_with_flags(
            &temporary,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.execute_batch("PRAGMA page_size=32768;")?;
        let mut report = populate(&mut conn, root, adapter, entries, facts, encoded_facts)?;
        let verifying = Instant::now();
        let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            bail!("candidate database failed integrity check: {integrity}");
        }
        report["verify_ms"] = json!(verifying.elapsed().as_secs_f64() * 1000.);
        conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        conn.close().map_err(|(_, e)| e)?;
        let _generation_lock = super::cache::exclusive_lock(&output)?;
        let wal = PathBuf::from(format!("{}-wal", output.display()));
        let shm = PathBuf::from(format!("{}-shm", output.display()));
        let _ = fs::remove_file(&wal);
        let _ = fs::remove_file(&shm);
        fs::rename(&temporary, &output)?;
        report["verification_cache"] = json!(super::cache::publish_verified(
            &output,
            root,
            report["output_root"]
                .as_str()
                .context("missing output root")?
        )
        .unwrap_or(false));
        Ok(report)
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

/// Builds and publishes a sealed index for the workspace.
///
/// Scanning and extraction use bounded batches. Graph rows are persisted with
/// multi-value SQL inserts before the final Merkle seal and publication.
///
/// # Errors
///
/// Rejects unsafe roots, changed source snapshots, storage failures, and seal failures.
pub fn build(root: &Path, output: &Path, adapter_path: Option<&Path>) -> Result<Value> {
    let started = Instant::now();
    let root = scanner::canonical_root(root)?;
    let adapter = scanner::load_adapter(&root, adapter_path)?;
    scanner::check_root_scope(&root, adapter.limits.allow_broad_root)?;
    let scanning = Instant::now();
    let scan = scanner::scan_with_adapter(&root, &adapter)?;
    let scan_ms = scanning.elapsed().as_secs_f64() * 1000.;
    let extracting = Instant::now();
    let budget = scanner::memory_budget_bytes();
    let batch_size = scanner::bounded_batch_size(
        budget,
        adapter.limits.max_file_bytes,
        EXTRACTION_MEMORY_MULTIPLIER,
        MAX_EXTRACTION_BATCH_FILES,
    );
    let mut facts: BTreeMap<_, _> = BTreeMap::new();
    for chunk in scan.entries.chunks(batch_size) {
        let chunk_facts: Vec<(String, TypedFacts)> = chunk
            .par_iter()
            .map(|file| Ok((file.path.clone(), extract_typed(&root, file, &adapter)?)))
            .collect::<Result<_>>()?;
        facts.extend(chunk_facts);
    }
    let extract_ms = extracting.elapsed().as_secs_f64() * 1000.;
    let preparing_components = Instant::now();
    components(&mut facts);
    let components_ms = preparing_components.elapsed().as_secs_f64() * 1000.;
    let encoding = Instant::now();
    let encoded_facts: hashbrown::HashMap<String, Vec<u8>> = facts
        .par_iter()
        .map(|(path, f)| Ok((path.clone(), encode_facts(f)?)))
        .collect::<Result<_>>()?;
    let encode_facts_ms = encoding.elapsed().as_secs_f64() * 1000.;
    let mut report = atomic_build(
        &root,
        output,
        &adapter,
        &scan.entries,
        &facts,
        Some(&encoded_facts),
    )?;
    report["scan_ms"] = json!(scan_ms);
    report["extract_ms"] = json!(extract_ms);
    report["components_ms"] = json!(components_ms);
    report["encode_facts_ms"] = json!(encode_facts_ms);
    report["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    report["output"] = json!(output);
    Ok(report)
}
fn cached(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> rusqlite::Result<usize> {
    conn.prepare_cached(sql)?.execute(params)
}

fn reference_keys<'a>(facts: &'a TypedFacts) -> Vec<&'a str> {
    let mut keys = Vec::new();
    let mut insert = |expression: &'a str| {
        keys.push(expression);
        for token in expression.split(['.', ':', '/']) {
            if !token.is_empty() && token.chars().all(|c| c.is_alphanumeric() || c == '_') {
                keys.push(token);
            }
        }
    };
    for expression in facts
        .references
        .iter()
        .filter(|r| !r.dynamic)
        .flat_map(|r| std::iter::once(r.expression.as_str()).chain(r.module.as_deref()))
        .chain(
            facts
                .nodes
                .iter()
                .filter(|n| n.kind == "impl")
                .map(|n| n.name.split('<').next().unwrap_or(&n.name).trim()),
        )
        .chain(
            facts
                .docs
                .iter()
                .flat_map(|d| d.referenced_symbols.iter().map(String::as_str)),
        )
    {
        insert(expression);
    }
    for reference in &facts.references {
        if let Some(
            ReceiverHint::CallResult { callee, .. }
            | ReceiverHint::ConstructorResult { callee, .. },
        ) = &reference.receiver_hint
        {
            insert(callee);
        }
    }
    for node in &facts.nodes {
        if let Some(flow) = facts.flows.get(node) {
            flow.reference_keys(&mut insert);
        } else if let Some(raw) = facts.flows.raw(node) {
            crate::core::semantic::ValueFlowFacts::reference_keys_from_value(raw, &mut insert);
        } else {
            crate::core::semantic::ValueFlowFacts::reference_keys_from_details(
                &node.details,
                &mut insert,
            );
        }
        if let Some(exports) = node.details.get("exports").and_then(Value::as_array) {
            for export in exports {
                for key in ["local", "module"] {
                    if let Some(name) = export.get(key).and_then(Value::as_str) {
                        insert(name);
                    }
                }
            }
        }
        if let Some(types) = node.details.get("param_types").and_then(Value::as_object) {
            for ty in types.values().filter_map(Value::as_str) {
                insert(ty);
            }
        }
    }
    if keys.len() >= 8192 {
        keys.par_sort_unstable();
    } else {
        keys.sort_unstable();
    }
    keys.dedup();
    keys
}

#[cfg(test)]
#[path = "writer/memory_budget_tests.rs"]
mod memory_budget_tests;
#[cfg(test)]
#[path = "writer/multi_value_batch_tests.rs"]
mod multi_value_batch_tests;
