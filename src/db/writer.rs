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
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static NONCE: AtomicU64 = AtomicU64::new(0);
#[derive(Debug)]
pub struct SourceSnapshotMismatch(pub String);

impl std::fmt::Display for SourceSnapshotMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SourceSnapshotMismatch {}

pub fn extract(root: &Path, file: &FileEntry, adapter: &scanner::Adapter) -> Result<Facts> {
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
        Ok(Facts {
            docs: sections,
            nodes,
            ..Facts::default()
        })
    } else {
        ast::extract(&file.path, &file.language, &source)
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
    Ok(scanner::Adapter {
        adapter_path,
        roots,
        ignored_names,
        adapter_version,
        digest,
        linked_workspaces,
        response: Default::default(),
    })
}
fn populate(
    conn: &mut Connection,
    root: &Path,
    adapter: &scanner::Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, Facts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
) -> Result<Value> {
    conn.execute_batch("PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-128000; PRAGMA cache_spill=OFF;")?;
    conn.set_prepared_statement_cache_capacity(128);
    let (tables, indexes) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    conn.execute_batch(tables)?;
    let linking = Instant::now();
    let graph = linker::link_with_root(facts, None, Some(root));
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
    for table in commitments::DOMAINS {
        let storage_table = if *table == "resolution_coverage" {
            "resolution_coverage_data"
        } else {
            table
        };
        cached(&tx, &format!("DELETE FROM {storage_table}"), [])?;
    }
    cached(&tx, "DELETE FROM domain_commitments", [])?;
    cached(&tx, "DELETE FROM coverage_evidence", [])?;
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
    let mut node_paths: hashbrown::HashMap<String, Option<String>> =
        hashbrown::HashMap::with_capacity(facts.values().map(|f| f.nodes.len()).sum());
    for f in facts.values() {
        for n in &f.nodes {
            node_paths.insert(n.id.clone(), Some(n.path.clone()));
        }
    }
    let (node_id, docs_count) = persist_files(&tx, entries, facts, encoded_facts)?;
    persist_graph(&tx, &graph, &mut node_paths, true)?;
    persist_file_commitments(&tx, entries, facts, &graph)?;
    let rows_ms = writing.elapsed().as_secs_f64() * 1000.;
    let indexing = Instant::now();
    tx.execute_batch(&format!("CREATE INDEX{indexes}"))?;
    let indexes_ms = indexing.elapsed().as_secs_f64() * 1000.;
    let persist_ms = writing.elapsed().as_secs_f64() * 1000.;
    let sealing = Instant::now();
    let seal = commitments::seal(&tx)?;
    let seal_ms = sealing.elapsed().as_secs_f64() * 1000.;
    tx.commit()?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(
        json!({"files":entries.len(),"nodes":node_id,"edges":graph.edges.len(),"doc_sections":docs_count,"output_root":seal,"schema_version":scanner::ENGINE_SCHEMA_VERSION,"link_ms":link_ms,"persist_ms":persist_ms,"rows_ms":rows_ms,"indexes_ms":indexes_ms,"seal_ms":seal_ms}),
    )
}
fn components(facts: &mut BTreeMap<String, Facts>) {
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
fn assign_components(facts: &mut BTreeMap<String, Facts>, owners: &BTreeMap<String, String>) {
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
fn delta_components(
    conn: &Connection,
    modified: &BTreeSet<String>,
    facts: &BTreeMap<String, Facts>,
    replace_files: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let mut st = conn.prepare("SELECT path,owner FROM owned_nodes WHERE kind='component'")?;
    let previous = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let mut st = conn.prepare("SELECT owner FROM owned_nodes WHERE kind='module' AND owner NOT IN(SELECT value FROM json_each(?1))")?;
    let paths = st
        .query_map([serde_json::to_string(modified)?], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let current = component_owners(paths.iter().map(String::as_str).chain(
        facts.iter().filter_map(|(path, f)| {
            f.nodes
                .iter()
                .any(|n| n.kind == "module")
                .then_some(path.as_str())
        }),
    ));
    for (root, owner) in &previous {
        if current.get(root) != Some(owner) {
            replace_files.insert(owner.clone());
        }
    }
    for (root, owner) in &current {
        if previous.get(root) != Some(owner) {
            replace_files.insert(owner.clone());
        }
    }
    Ok(current)
}
fn atomic_build(
    root: &Path,
    output: &Path,
    adapter: &scanner::Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, Facts>,
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

pub fn build(root: &Path, output: &Path, adapter_path: Option<&Path>) -> Result<Value> {
    let started = Instant::now();
    let root = scanner::canonical_root(root)?;
    let adapter = scanner::load_adapter(&root, adapter_path)?;
    let scanning = Instant::now();
    let scan = scanner::scan_with_adapter(&root, &adapter)?;
    let scan_ms = scanning.elapsed().as_secs_f64() * 1000.;
    let extracting = Instant::now();
    let mut facts: BTreeMap<_, _> = scan
        .entries
        .par_iter()
        .map(|file| Ok((file.path.clone(), extract(&root, file, &adapter)?)))
        .collect::<Result<_>>()?;
    let extract_ms = extracting.elapsed().as_secs_f64() * 1000.;
    components(&mut facts);
    let encoded_facts: hashbrown::HashMap<String, Vec<u8>> = facts
        .par_iter()
        .map(|(path, f)| Ok((path.clone(), encode_facts(f)?)))
        .collect::<Result<_>>()?;
    let mut report = atomic_build(&root, output, &adapter, &scan.entries, &facts, Some(&encoded_facts))?;
    report["scan_ms"] = json!(scan_ms);
    report["extract_ms"] = json!(extract_ms);
    report["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    report["output"] = json!(output);
    Ok(report)
}
pub fn delta(root: &Path, db: &Path, modified: &[PathBuf]) -> Result<Value> {
    let started = Instant::now();
    if modified.is_empty() {
        bail!("delta requires at least one modified path");
    }
    let root = scanner::canonical_root(root)?;
    let generation_lock = super::cache::exclusive_lock(db)?;
    let schema_compatible = {
        let probe = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        probe
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some(scanner::ENGINE_SCHEMA_VERSION)
    };
    if !schema_compatible {
        drop(generation_lock);
        return build(&root, db, None);
    }
    let admitted = super::reader::open(db, &root)?;
    let admitted_identity = super::cache::identity(db)?;
    let expected: String = admitted.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    let adapter = read_policy(&root, &admitted)?;
    let current_adapter = scanner::load_adapter(&root, adapter.adapter_path.as_deref())?;
    if current_adapter.digest != adapter.digest
        || current_adapter.linked_workspaces != adapter.linked_workspaces
    {
        drop(admitted);
        return build(&root, db, adapter.adapter_path.as_deref());
    }
    let previous = super::reader::inventory_snapshot(&admitted)?;
    let scanning = Instant::now();
    let scan = scanner::scan_reusing(&root, &adapter, &previous)?;
    let scan_ms = scanning.elapsed().as_secs_f64() * 1000.;
    let modified: BTreeSet<String> = modified
        .iter()
        .map(|p| {
            scanner::checked_child(&root, p)?;
            Ok(p.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Result<_>>()?;
    let inventory: BTreeMap<_, _> = scan.entries.iter().map(|f| (f.path.as_str(), f)).collect();
    let previous: BTreeMap<_, _> = previous.iter().map(|f| (f.path.as_str(), f)).collect();
    for (path, old) in &previous {
        if !modified.contains(*path) && inventory.get(path).is_none_or(|f| f.digest != old.digest) {
            return Err(SourceSnapshotMismatch(format!("unlisted source change: {path}")).into());
        }
    }
    for path in inventory.keys() {
        if !previous.contains_key(path) && !modified.contains(*path) {
            return Err(SourceSnapshotMismatch(format!("unlisted source addition: {path}")).into());
        }
    }
    for path in &modified {
        if !inventory.contains_key(path.as_str()) {
            if root.join(path).exists() {
                bail!("modified path outside admitted inventory: {path}");
            }
            if !previous.contains_key(path.as_str()) {
                bail!("deletion was not in previous inventory: {path}");
            }
        }
    }
    let extracting = Instant::now();
    let mut facts = BTreeMap::new();
    let mut changed_resolution = BTreeSet::new();
    for path in &modified {
        if let Some(file) = inventory.get(path.as_str()) {
            let extracted = extract(&root, file, &adapter)?;
            if !previous.contains_key(path.as_str())
                || resolution_identity_changed(&admitted, path, &extracted)?
                || (linker::needs_reference_identity(path, &extracted)
                    && reference_identity_changed(&admitted, path, &extracted)?)
            {
                changed_resolution.insert(path.clone());
            }
            facts.insert(path.clone(), extracted);
        } else {
            changed_resolution.insert(path.clone());
        }
    }
    let mut names = BTreeSet::new();
    let encoded = serde_json::to_string(&changed_resolution)?;
    {
        let mut st = admitted.prepare(
            "SELECT owner,name,qualname,kind FROM owned_nodes WHERE owner IN(SELECT value FROM json_each(?1))",
        )?;
        for row in st.query_map([&encoded], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (owner, name, qual, kind) = row?;
            names.insert(qual.clone());
            if !matches!(kind.as_str(), "field" | "method" | "route" | "document") {
                let mod_name = module_name(&owner);
                let (_, local_path) = crate::engine::languages::workspace_path(&owner);
                let local_module = module_name(local_path);
                let src_module = local_path
                    .split_once("/src/")
                    .map(|(_, r)| module_name(r))
                    .or_else(|| local_path.strip_prefix("src/").map(module_name));
                let is_top = qual == name
                    || qual == format!("{mod_name}.{name}")
                    || qual == format!("{local_module}.{name}")
                    || src_module
                        .as_ref()
                        .is_some_and(|m| qual == format!("{m}.{name}"));
                if is_top {
                    names.insert(name);
                }
            }
        }
    }
    for path in &changed_resolution {
        let mod_name = module_name(path);
        names.insert(mod_name.clone());
        let (_, local_path) = crate::engine::languages::workspace_path(path);
        let local_module = module_name(local_path);
        names.insert(local_module.clone());
        let src_module = local_path
            .split_once("/src/")
            .map(|(_, r)| module_name(r))
            .or_else(|| local_path.strip_prefix("src/").map(module_name));
        if let Some(src_mod) = &src_module {
            names.insert(src_mod.clone());
        }
        let mut stable_module = false;
        if let Some(f) = facts.get(path) {
            let previous_module: Option<(String, String, String)> = admitted
                .prepare_cached("SELECT public_id,qualname,language FROM owned_nodes WHERE owner=?1 AND kind='module'")?
                .query_row([path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            stable_module = previous_module
                .as_ref()
                .is_some_and(|(id, qualname, language)| {
                    f.nodes.iter().any(|n| {
                        n.kind == "module"
                            && &n.id == id
                            && &n.qualname == qualname
                            && &n.language == language
                    })
                });
            for n in &f.nodes {
                names.insert(n.qualname.clone());
                if !matches!(n.kind.as_str(), "field" | "method" | "route" | "document") {
                    let is_top = n.qualname == n.name
                        || n.qualname == format!("{mod_name}.{}", n.name)
                        || n.qualname == format!("{local_module}.{}", n.name)
                        || src_module
                            .as_ref()
                            .is_some_and(|m| n.qualname == format!("{m}.{}", n.name));
                    if is_top {
                        names.insert(n.name.clone());
                    }
                }
            }
        }
        if !stable_module {
            if let Some((_, last)) = local_module.rsplit_once('.') {
                names.insert(last.to_owned());
            }
        }
    }
    let extract_ms = extracting.elapsed().as_secs_f64() * 1000.;
    let hydrating = Instant::now();
    let mut affected = modified.clone();
    {
        let mut st = admitted.prepare(
            "SELECT owner FROM dependencies WHERE target IN(SELECT value FROM json_each(?1))",
        )?;
        for owner in st.query_map([&encoded], |r| r.get::<_, String>(0))? {
            affected.insert(owner?);
        }
        let mut st=admitted.prepare("SELECT owner FROM shared_owners WHERE kind='reference' AND key IN(SELECT value FROM json_each(?1))")?;
        for owner in st.query_map([serde_json::to_string(&names)?], |r| r.get::<_, String>(0))? {
            affected.insert(owner?);
        }
    }
    let mut replace_files = modified.clone();
    let component_owners = delta_components(&admitted, &modified, &facts, &mut replace_files)?;
    affected.extend(replace_files.iter().cloned());
    let load_owners: Vec<_> = affected
        .difference(&modified)
        .filter(|path| inventory.contains_key(path.as_str()))
        .collect();
    let mut loaded_fact_files = 0;
    {
        let mut st=admitted.prepare("SELECT path,facts_blob FROM local_facts WHERE path IN(SELECT value FROM json_each(?1))")?;
        for row in st.query_map([serde_json::to_string(&load_owners)?], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })? {
            let (path, blob) = row?;
            let f = decode_facts(&blob)?;
            facts.insert(path, f);
            loaded_fact_files += 1;
        }
    }
    {
        let catalog: Vec<_> = scan
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.language.as_str()))
            .collect();
        let mut st = admitted.prepare("SELECT facts_blob FROM local_facts WHERE path=?1")?;
        for path in linker::required_full_facts(&facts, &affected, &catalog) {
            if facts.contains_key(&path) {
                continue;
            }
            if let Some(raw) = st
                .query_row([&path], |row| row.get::<_, Vec<u8>>(0))
                .optional()?
            {
                facts.insert(path, decode_facts(&raw)?);
                loaded_fact_files += 1;
            }
        }
    }
    assign_components(&mut facts, &component_owners);
    let mut tokens = BTreeSet::new();
    let mut doc_symbols = BTreeSet::new();
    for f in facts.values() {
        tokens.extend(reference_keys(f));
        doc_symbols.extend(
            f.docs
                .iter()
                .flat_map(|doc| doc.referenced_symbols.iter())
                .map(|symbol| symbol.replace("::", "."))
                .filter(|symbol| !symbol.is_empty()),
        );
    }
    // Cold linking discovers documentation targets by any qualified-name suffix.
    // Reuse the existing FTS index to narrow delta candidates before checking
    // the exact dot-boundary suffix. Symbols outside its ASCII token contract
    // retain the full scan so their links cannot silently disappear.
    let mut suffix_ids = BTreeSet::new();
    if !doc_symbols.is_empty() {
        let mut terms = BTreeSet::new();
        let mut fallback_symbols = BTreeSet::new();
        for symbol in &doc_symbols {
            let token = symbol
                .is_ascii()
                .then(|| symbol.rsplit(|c: char| !c.is_ascii_alphanumeric()).next())
                .flatten()
                .filter(|token| !token.is_empty() && token.len() <= 128);
            if let Some(token) = token {
                terms.insert(token.to_owned());
            } else {
                fallback_symbols.insert(symbol.as_str());
            }
        }
        let mut st = admitted.prepare("SELECT n.id,n.qualname FROM node_search s JOIN nodes n ON n.node_id=s.rowid WHERE node_search MATCH ?1")?;
        for chunk in terms.into_iter().collect::<Vec<_>>().chunks(32) {
            let query = chunk
                .iter()
                .map(|term| format!("\"{term}\""))
                .collect::<Vec<_>>()
                .join(" OR ");
            for row in st.query_map([query], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (id, qualname) = row?;
                if qualname
                    .match_indices('.')
                    .any(|(at, _)| doc_symbols.contains(&qualname[at + 1..]))
                {
                    suffix_ids.insert(id);
                }
            }
        }
        if !fallback_symbols.is_empty() {
            let mut st =
                admitted.prepare("SELECT id,qualname FROM nodes WHERE instr(qualname,'.')>0")?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (id, qualname) = row?;
                if qualname
                    .match_indices('.')
                    .any(|(at, _)| fallback_symbols.contains(&qualname[at + 1..]))
                {
                    suffix_ids.insert(id);
                }
            }
        }
    }
    let encoded = serde_json::to_string(&tokens)?;
    let suffix_ids = serde_json::to_string(&suffix_ids)?;
    {
        let sql="SELECT n.id,n.kind,n.name,n.qualname,n.path,n.line,n.end_line,n.is_test,n.language,n.generated,EXISTS(SELECT 1 FROM shared_owners s WHERE s.kind='default_export' AND s.key=n.id) FROM nodes n WHERE n.id IN(SELECT id FROM nodes WHERE kind='module' UNION SELECT id FROM nodes WHERE name IN(SELECT value FROM json_each(?1)) UNION SELECT id FROM nodes WHERE qualname IN(SELECT value FROM json_each(?1)) UNION SELECT key FROM shared_owners WHERE kind='default_export' UNION SELECT value FROM json_each(?2))";
        let mut st = admitted.prepare(sql)?;
        let nodes = st.query_map(params![encoded, suffix_ids], |r| {
            Ok(Node {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                qualname: r.get(3)?,
                path: r.get(4)?,
                line: r.get(5)?,
                end_line: r.get(6)?,
                is_test: r.get(7)?,
                language: r.get(8)?,
                generated: r.get(9)?,
                details: json!({"default_export":r.get::<_,bool>(10)?}),
            })
        })?;
        let complete: BTreeSet<_> = facts.keys().cloned().collect();
        for node in nodes {
            let node = node?;
            if complete.contains(&node.path) || modified.contains(&node.path) {
                continue;
            }
            facts
                .entry(node.path.clone())
                .or_insert_with(Facts::default)
                .nodes
                .push(node);
        }
    }
    let hydrate_ms = hydrating.elapsed().as_secs_f64() * 1000.;
    let linking = Instant::now();
    let graph = linker::link_with_root(&facts, Some(&affected), Some(&root));
    let link_ms = linking.elapsed().as_secs_f64() * 1000.;
    let persisting = Instant::now();
    drop(admitted);
    if super::cache::identity(db)? != admitted_identity {
        bail!("database changed during delta admission");
    }
    let mut conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.set_prepared_statement_cache_capacity(128);
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-64000; PRAGMA mmap_size=268435456;",
    )?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: String = tx.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    if current != expected {
        bail!("generation changed during delta admission");
    }
    let deleting = Instant::now();
    tx.execute_batch("CREATE TEMP TABLE changed_edge_keys(src TEXT,dst TEXT,kind TEXT,PRIMARY KEY(src,dst,kind));")?;
    for path in &affected {
        cached(&tx,"INSERT OR IGNORE INTO changed_edge_keys SELECT src,dst,kind FROM edge_occurrences WHERE owner=?1",[path])?;
    }
    for e in &graph.edges {
        cached(
            &tx,
            "INSERT OR IGNORE INTO changed_edge_keys VALUES(?1,?2,?3)",
            params![e.src, e.dst, e.kind],
        )?;
    }
    let mut commitment_owners = affected.clone();
    commitment_owners.extend(
        tx.prepare_cached("SELECT DISTINCT path FROM edges WHERE (src_public_id,dst_public_id,kind)IN(SELECT src,dst,kind FROM changed_edge_keys)")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    tx.execute("DELETE FROM edges WHERE (src_public_id,dst_public_id,kind)IN(SELECT src,dst,kind FROM changed_edge_keys)",[])?;
    for path in &affected {
        for (table, column) in [
            ("edge_occurrences", "owner"),
            ("dependencies", "owner"),
            ("resolution_coverage_data", "path"),
            ("file_commitments", "path"),
        ] {
            cached(
                &tx,
                &format!("DELETE FROM {table} WHERE {column}=?1"),
                [path],
            )?;
        }
    }
    tx.execute_batch("CREATE TEMP TABLE replaced_nodes(id TEXT PRIMARY KEY)")?;
    tx.execute("INSERT OR IGNORE INTO replaced_nodes SELECT public_id FROM owned_nodes WHERE owner IN(SELECT value FROM json_each(?1))", [serde_json::to_string(&replace_files)?])?;
    for path in &replace_files {
        delete_file(&tx, path)?;
    }
    let delete_ms = deleting.elapsed().as_secs_f64() * 1000.;
    let changed_entries: Vec<_> = scan
        .entries
        .iter()
        .filter(|f| replace_files.contains(&f.path))
        .cloned()
        .collect();
    let mut node_paths: hashbrown::HashMap<String, Option<String>> =
        hashbrown::HashMap::with_capacity(facts.values().map(|f| f.nodes.len()).sum());
    for f in facts.values() {
        for n in &f.nodes {
            node_paths.insert(n.id.clone(), Some(n.path.clone()));
        }
    }
    let files_writing = Instant::now();
    persist_files(&tx, &changed_entries, &facts, None)?;
    let files_ms = files_writing.elapsed().as_secs_f64() * 1000.;
    let graph_writing = Instant::now();
    persist_graph(&tx, &graph, &mut node_paths, false)?;
    tx.execute(
        "DELETE FROM coverage_evidence WHERE evidence_id NOT IN(SELECT evidence_id FROM resolution_coverage_data)",
        [],
    )?;
    let graph_ms = graph_writing.elapsed().as_secs_f64() * 1000.;
    let aggregating = Instant::now();
    let dangling: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM replaced_nodes r WHERE NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=r.id) AND (EXISTS(SELECT 1 FROM edges e WHERE e.src_public_id=r.id) OR EXISTS(SELECT 1 FROM edges e WHERE e.dst_public_id=r.id)))", [], |r| r.get(0))?;
    if dangling {
        bail!("delta leaves an edge with a missing endpoint; rebuild index");
    }
    tx.execute(
        "WITH ranked AS (
            SELECT e.*,row_number() OVER edge_group AS representative,
                   count(*) OVER edge_group AS occurrences
            FROM changed_edge_keys k CROSS JOIN edge_occurrences e
              ON k.src=e.src AND k.dst=e.dst AND k.kind=e.kind
            WINDOW edge_group AS (
                PARTITION BY e.src,e.dst,e.kind ORDER BY e.owner,e.ordinal
                ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
            )
        )
        INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count)
        SELECT src,dst,kind,path,line,evidence,confidence,occurrences
        FROM ranked WHERE representative=1",
        [],
    )?;
    commitment_owners.extend(
        tx.prepare_cached("SELECT DISTINCT path FROM edges WHERE (src_public_id,dst_public_id,kind)IN(SELECT src,dst,kind FROM changed_edge_keys)")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    let aggregate_ms = aggregating.elapsed().as_secs_f64() * 1000.;
    let affected_entries: Vec<_> = scan
        .entries
        .iter()
        .filter(|f| affected.contains(&f.path))
        .cloned()
        .collect();
    let commitments_writing = Instant::now();
    persist_file_commitments(&tx, &affected_entries, &facts, &graph)?;
    let commitments_ms = commitments_writing.elapsed().as_secs_f64() * 1000.;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('corpus_hash',?1)",
        [commitments::hash(&serde_json::to_vec(&scan.entries)?)],
    )?;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('inventory_snapshot',?1)",
        [serde_json::to_string(&scan.entries)?],
    )?;
    let persist_ms = persisting.elapsed().as_secs_f64() * 1000.;
    let sealing = Instant::now();
    let seal = commitments::seal_owners(&tx, Some(&commitment_owners))?;
    let seal_ms = sealing.elapsed().as_secs_f64() * 1000.;
    let verifying = Instant::now();
    commitments::verify_owners(&tx, Some(&commitment_owners))?;
    let latest = scanner::scan_reusing(&root, &adapter, &scan.entries)?;
    if latest.entries != scan.entries {
        return Err(SourceSnapshotMismatch(format!(
            "workspace changed during delta; transaction rolled back (scan={scan_ms:.0}ms extract={extract_ms:.0}ms hydrate={hydrate_ms:.0}ms link={link_ms:.0}ms persist={persist_ms:.0}ms seal={seal_ms:.0}ms verify={:.0}ms)",
            verifying.elapsed().as_secs_f64() * 1000.
        ))
        .into());
    }
    let verify_ms = verifying.elapsed().as_secs_f64() * 1000.;
    let committing = Instant::now();
    tx.commit()?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    let commit_ms = committing.elapsed().as_secs_f64() * 1000.;
    drop(conn);
    let cached = super::cache::publish_verified(db, &root, &seal).unwrap_or(false);
    Ok(
        json!({"files":scan.files,"changed_files":modified.len(),"affected_owners":affected.len(),"reparsed_files":modified.iter().filter(|p|inventory.contains_key(p.as_str())).count(),"loaded_fact_files":loaded_fact_files,"rewritten_files":changed_entries.len(),"verification_cache":cached,"output_root":seal,"scan_ms":scan_ms,"extract_ms":extract_ms,"hydrate_ms":hydrate_ms,"link_ms":link_ms,"persist_ms":persist_ms,"delete_ms":delete_ms,"files_ms":files_ms,"graph_ms":graph_ms,"aggregate_ms":aggregate_ms,"commitments_ms":commitments_ms,"seal_ms":seal_ms,"verify_ms":verify_ms,"commit_ms":commit_ms,"elapsed_ms":started.elapsed().as_secs_f64()*1000.}),
    )
}

fn resolution_identity_changed(conn: &Connection, path: &str, facts: &Facts) -> Result<bool> {
    let identity = |node: &Node| -> Result<String> {
        let mut value = serde_json::to_value(node)?;
        let object = value
            .as_object_mut()
            .context("node identity must be an object")?;
        object.remove("line");
        object.remove("end_line");
        Ok(serde_json::to_string(&value)?)
    };
    let mut statement = conn.prepare_cached(
        "SELECT public_id,kind,name,qualname,path,line,end_line,is_test,language,generated,details_json FROM owned_nodes WHERE owner=?1",
    )?;
    let rows = statement.query_map([path], |row| {
        Ok((
            Node {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                qualname: row.get(3)?,
                path: row.get(4)?,
                line: row.get(5)?,
                end_line: row.get(6)?,
                is_test: row.get(7)?,
                language: row.get(8)?,
                generated: row.get(9)?,
                details: Value::Null,
            },
            row.get::<_, String>(10)?,
        ))
    })?;
    let mut old = BTreeSet::new();
    for row in rows {
        let (mut node, details) = row?;
        if node.kind == "component" {
            continue;
        }
        node.details = serde_json::from_str(&details)?;
        old.insert(identity(&node)?);
    }
    let new = facts
        .nodes
        .iter()
        .filter(|node| node.kind != "component")
        .map(identity)
        .collect::<Result<BTreeSet<_>>>()?;
    Ok(old != new)
}
fn reference_identity_changed(conn: &Connection, path: &str, facts: &Facts) -> Result<bool> {
    let old: Option<Vec<u8>> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    let Some(old) = old else {
        return Ok(true);
    };
    let old = decode_facts(&old)?;
    Ok(linker::reference_identity_changed(path, &old, facts))
}
fn delete_file(tx: &Connection, path: &str) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "SELECT n.node_id,s.search_text FROM nodes n JOIN owned_search s ON s.public_id=n.id WHERE s.owner=?1"
    )?;
    let old: Vec<(i64, String)> = stmt
        .query_map([path], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, text) in old {
        cached(
            tx,
            "INSERT INTO node_search(node_search,rowid,search_text)VALUES('delete',?1,?2)",
            params![id, text],
        )?;
    }
    cached(tx,"INSERT INTO doc_search(doc_search,rowid,section_title,content,invariants)SELECT 'delete',rowid,section_title,content,invariants FROM doc_sections WHERE path=?1",[path])?;
    cached(
        tx,
        "DELETE FROM nodes WHERE id IN(SELECT public_id FROM owned_nodes WHERE owner=?1)",
        [path],
    )?;
    for (table, column) in [
        ("source_inventory", "path"),
        ("files", "path"),
        ("local_facts", "path"),
        ("owned_nodes", "owner"),
        ("owned_search", "owner"),
        ("shared_owners", "owner"),
        ("errors", "path"),
        ("doc_sections", "path"),
    ] {
        cached(
            tx,
            &format!("DELETE FROM {table} WHERE {column}=?1"),
            [path],
        )?;
    }
    Ok(())
}

fn cached(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> rusqlite::Result<usize> {
    conn.prepare_cached(sql)?.execute(params)
}

fn encode_facts(facts: &Facts) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(facts)?;
    Ok(zstd::stream::encode_all(json.as_slice(), 1)?)
}

fn decode_facts(blob: &[u8]) -> Result<Facts> {
    let json = zstd::stream::decode_all(blob)?;
    Ok(serde_json::from_slice(&json)?)
}

fn persist_files(
    tx: &Connection,
    entries: &[FileEntry],
    facts: &BTreeMap<String, Facts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
) -> Result<(i64, usize)> {
    let mut node_id: i64 = tx.query_row("SELECT coalesce(max(node_id),0)FROM nodes", [], |r| {
        r.get(0)
    })?;
    let mut docs_count = 0;

    let mut stmt_source_inv =
        tx.prepare("INSERT INTO source_inventory VALUES(?1,'indexed',?2,?3)")?;
    let mut stmt_files = tx.prepare("INSERT INTO files VALUES(?1,'indexed',?2,?3,?4,?5,0)")?;
    let mut stmt_local_facts = tx.prepare("INSERT INTO local_facts VALUES(?1,?2,?3,?4,0,?5)")?;
    let mut stmt_nodes =
        tx.prepare("INSERT INTO nodes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)")?;
    let mut stmt_owned_nodes =
        tx.prepare("INSERT INTO owned_nodes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)")?;
    let mut stmt_owned_search = tx.prepare("INSERT INTO owned_search VALUES(?1,?2,0,?3)")?;
    let mut stmt_node_search =
        tx.prepare("INSERT INTO node_search(rowid,search_text)VALUES(?1,?2)")?;
    let mut stmt_shared_symbol =
        tx.prepare("INSERT INTO shared_owners VALUES('symbol',?1,?2,?3)")?;
    let mut stmt_shared_ref =
        tx.prepare("INSERT OR IGNORE INTO shared_owners VALUES('reference',?1,?2,0)")?;
    let mut stmt_shared_def =
        tx.prepare("INSERT OR IGNORE INTO shared_owners VALUES('default_export',?1,?2,0)")?;
    let mut stmt_doc_sec =
        tx.prepare("INSERT INTO doc_sections VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?;
    let mut stmt_doc_search = tx.prepare("INSERT INTO doc_search(rowid,section_title,content,invariants)SELECT rowid,section_title,content,invariants FROM doc_sections WHERE path=?1")?;
    let mut stmt_errors = tx.prepare("INSERT INTO errors VALUES(?1,?2,?3)")?;

    for file in entries {
        let f = facts.get(&file.path).context("missing extracted facts")?;
        stmt_source_inv.execute(params![file.path, file.digest, file.bytes])?;
        stmt_files.execute(params![
            file.path,
            file.digest,
            file.bytes,
            file.language,
            is_test(&file.path)
        ])?;
        let facts_blob = match encoded_facts.and_then(|m| m.get(&file.path)) {
            Some(blob) => blob.clone(),
            None => encode_facts(f)?,
        };
        stmt_local_facts.execute(params![
            file.path,
            file.digest,
            file.language,
            is_test(&file.path),
            facts_blob
        ])?;
        for n in &f.nodes {
            node_id += 1;
            let details = n.details.to_string();
            stmt_nodes.execute(params![
                node_id,
                n.id,
                n.kind,
                n.name,
                n.qualname,
                n.path,
                n.line,
                n.end_line,
                n.is_test,
                n.language,
                n.generated,
                details
            ])?;
            stmt_owned_nodes.execute(params![
                file.path,
                n.id,
                n.kind,
                n.name,
                n.qualname,
                n.path,
                n.line,
                n.end_line,
                n.is_test,
                n.language,
                n.generated,
                details
            ])?;
            let doc = n.details.get("doc").and_then(|d| d.as_str()).unwrap_or("");
            let mut search = if doc.is_empty() {
                format!("{} {} {}", n.name, n.qualname, n.path)
            } else {
                format!("{} {} {} {}", n.name, n.qualname, n.path, doc)
            };
            if n.kind == "module" {
                for reference in &f.references {
                    search.push(' ');
                    search.push_str(&reference.expression);
                }
            }
            stmt_owned_search.execute(params![file.path, n.id, search])?;
            stmt_node_search.execute(params![node_id, search])?;
            stmt_shared_symbol.execute(params![n.qualname, file.path, node_id])?;
        }
        for key in reference_keys(f) {
            stmt_shared_ref.execute(params![key, file.path])?;
        }
        for n in &f.nodes {
            if n.details["default_export"] == true {
                stmt_shared_def.execute(params![n.id, file.path])?;
            }
        }
        for d in &f.docs {
            docs_count += 1;
            stmt_doc_sec.execute(params![
                d.doc_id,
                d.path,
                d.section_title,
                d.doc_type,
                d.content,
                serde_json::to_string(&d.invariants)?,
                serde_json::to_string(&d.referenced_symbols)?,
                d.mtime,
                d.size,
                d.is_invariant
            ])?;
        }
        stmt_doc_search.execute([&file.path])?;
        for e in &f.errors {
            stmt_errors.execute(params![e.path, e.line, e.message])?;
        }
    }

    Ok((node_id, docs_count))
}

fn persist_graph(
    tx: &Connection,
    graph: &Graph,
    node_paths: &mut hashbrown::HashMap<String, Option<String>>,
    materialize_edges: bool,
) -> Result<()> {
    let mut ordinals: BTreeMap<&str, usize> = BTreeMap::new();
    let mut stmt_occ =
        tx.prepare("INSERT INTO edge_occurrences VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)")?;
    let mut stmt_edges = materialize_edges
        .then(|| tx.prepare("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count)VALUES(?1,?2,?3,?4,?5,?6,?7,1)ON CONFLICT(src_public_id,dst_public_id,kind)DO UPDATE SET occurrence_count=occurrence_count+1"))
        .transpose()?;
    let mut stmt_deps = tx.prepare("INSERT INTO dependencies VALUES(?1,?2,?3,?4,'resolved')")?;
    let mut stmt_select_path = tx.prepare_cached("SELECT path FROM nodes WHERE id=?1")?;

    for e in &graph.edges {
        let ordinal = ordinals.entry(&e.path).or_default();
        stmt_occ.execute(params![
            e.path,
            *ordinal,
            e.src,
            e.dst,
            e.kind,
            e.path,
            e.line,
            e.evidence,
            e.confidence
        ])?;
        *ordinal += 1;
        if let Some(stmt_edges) = stmt_edges.as_mut() {
            stmt_edges.execute(params![
                e.src,
                e.dst,
                e.kind,
                e.path,
                e.line,
                e.evidence,
                e.confidence
            ])?;
        }
        let mut targets = BTreeSet::new();
        for endpoint in [&e.src, &e.dst] {
            let target: Option<String> = if let Some(p) = node_paths.get(endpoint) {
                p.clone()
            } else {
                let p: Option<String> = stmt_select_path
                    .query_row([endpoint], |r| r.get(0))
                    .optional()?;
                node_paths.insert(endpoint.clone(), p.clone());
                p
            };
            let target = target
                .with_context(|| format!("edge {} has missing endpoint {endpoint}", e.kind))?;
            if target != e.path {
                targets.insert(target);
            }
        }
        for target in targets {
            stmt_deps.execute(params![e.path, target, e.kind, e.evidence])?;
        }
    }
    let mut stmt_cov = tx.prepare("INSERT INTO resolution_coverage VALUES(?1,?2,?3,?4,?5)")?;
    let mut stmt_unres =
        tx.prepare("INSERT INTO dependencies VALUES(?1,NULL,'unresolved',?2,?3)")?;
    for c in &graph.coverage {
        stmt_cov.execute(params![c.path, c.line, c.expression, c.status, c.evidence])?;
        if c.status != "resolved" {
            stmt_unres.execute(params![c.path, c.expression, c.status])?;
        }
    }

    Ok(())
}

fn persist_file_commitments(
    tx: &Connection,
    entries: &[FileEntry],
    facts: &BTreeMap<String, Facts>,
    graph: &Graph,
) -> Result<()> {
    let mut owned_edges: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    for e in &graph.edges {
        owned_edges.entry(&e.path).or_default().push(e);
    }
    let mut owned_coverage: BTreeMap<&str, Vec<&Coverage>> = BTreeMap::new();
    for c in &graph.coverage {
        owned_coverage.entry(&c.path).or_default().push(c);
    }
    let rows: Vec<Result<[String; 8]>> = entries
        .par_iter()
        .map(|file| {
            let f = &facts[&file.path];
            let h = |v: &Value| commitments::hash(v.to_string().as_bytes());
            let inventory = h(&json!([file.path, file.digest, file.bytes]));
            let facts_hash = commitments::hash(&serde_json::to_vec(f)?);
            let nodes = h(&json!(f.nodes));
            let edges = h(&json!(owned_edges
                .get(file.path.as_str())
                .cloned()
                .unwrap_or_default()));
            let search = h(&json!(f
                .nodes
                .iter()
                .map(|n| format!("{} {} {} {}", n.name, n.qualname, n.path, n.details))
                .collect::<Vec<_>>()));
            let deps = h(&json!(owned_coverage
                .get(file.path.as_str())
                .cloned()
                .unwrap_or_default()));
            Ok([
                file.path.clone(),
                file.digest.clone(),
                inventory,
                facts_hash,
                nodes,
                edges,
                search,
                deps,
            ])
        })
        .collect();

    let mut stmt = tx.prepare("INSERT INTO file_commitments VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?;
    for row in rows {
        let [p, d, inv, fh, n, e, s, dep] = row?;
        stmt.execute(params![p, d, inv, fh, n, e, s, dep])?;
    }

    Ok(())
}

fn reference_keys(facts: &Facts) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
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
        keys.insert(expression.into());
        for token in expression.split(['.', ':', '/']) {
            if !token.is_empty() && token.chars().all(|c| c.is_alphanumeric() || c == '_') {
                keys.insert(token.into());
            }
        }
    }
    keys
}
