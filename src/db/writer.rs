//! SQLite persistence and indexing operations for Forge.

use crate::core::models::compact_graph::Graph as CompactGraph;
use crate::core::semantic::CatalogStoragePayload;
use crate::core::typed_facts::{encode_facts, TypedFacts};
use crate::core::{
    commitments,
    config::{self, Adapter},
    models::*,
    schema::SCHEMA_DDL,
};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use rusqlite::{
    limits::Limit, params, params_from_iter, types::Value as SqlValue, Connection,
    OptionalExtension, ToSql,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fmt::Write as _, path::Path, sync::Arc, time::Instant};

#[path = "batch.rs"]
mod batch;
/// Database extraction cache facilities.
pub mod cache;
#[path = "delta.rs"]
pub(crate) mod delta_persist;
#[path = "dictionary.rs"]
mod dictionary;
#[path = "ingest.rs"]
mod ingest;
mod seal;

use batch::*;
use dictionary::*;
use ingest::*;

const MAX_MULTI_VALUE_BATCH_ROWS: usize = 500;

#[derive(serde::Serialize, serde::Deserialize)]
struct PreparedCatalogStorage {
    facts: TypedFacts,
    details_json: Vec<String>,
    search_texts: Vec<String>,
    reference_keys: Vec<String>,
    leaves: BTreeMap<String, BTreeMap<String, String>>,
}

pub(crate) struct StagedFilePayload {
    pub(crate) encoded_facts: Vec<u8>,
    pub(crate) storage_rows: Option<Vec<u8>>,
}

pub(crate) fn prepare_catalog_storage(
    file: &FileEntry,
    single: &BTreeMap<String, TypedFacts>,
) -> Result<Vec<u8>> {
    let prepared = prepare_catalog_rows(file, single)?;
    let mut bytes = zstd::stream::encode_all(serde_json::to_vec(&prepared)?.as_slice(), -3)?;
    bytes.shrink_to_fit();
    Ok(bytes)
}

fn prepare_catalog_rows(
    file: &FileEntry,
    single: &BTreeMap<String, TypedFacts>,
) -> Result<PreparedCatalogStorage> {
    let facts = single
        .get(&file.path)
        .context("missing prepared storage file")?;
    let details_json = facts
        .nodes
        .iter()
        .map(|node| serde_json::to_string(&node.navigation_details()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let search_texts = facts
        .nodes
        .iter()
        .map(|node| {
            let mut text = String::new();
            write_node_search_text(&mut text, node, facts);
            text
        })
        .collect();
    let reference_keys = reference_keys(facts)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let leaves = seal::file_domain_leaves(
        std::slice::from_ref(file),
        single,
        &details_json,
        &reference_keys,
    )?;
    let nodes = facts
        .nodes
        .iter()
        .map(|node| {
            let details = if node.details["default_export"] == true {
                json!({"default_export": true})
            } else {
                Value::Null
            };
            let storage = Node {
                id: node.id.clone(),
                kind: node.kind.clone(),
                name: node.name.clone(),
                qualname: node.qualname.clone(),
                path: node.path.clone(),
                line: node.line,
                end_line: node.end_line,
                is_test: node.is_test,
                language: node.language.clone(),
                generated: node.generated,
                details,
            };
            Ok(storage)
        })
        .collect::<Result<Vec<_>>>()?;
    let storage = TypedFacts::from_public(Facts {
        nodes,
        docs: facts.docs.clone(),
        errors: facts.errors.clone(),
        ..Default::default()
    });
    let prepared = PreparedCatalogStorage {
        facts: storage,
        details_json,
        search_texts,
        reference_keys,
        leaves: leaves
            .into_iter()
            .map(|(domain, leaves)| (domain.to_owned(), leaves))
            .collect(),
    };
    Ok(prepared)
}

/// Inputs supplied to pipeline index persistence.
#[derive(Clone, Copy)]
pub struct BuildInputs<'a> {
    /// Manifest digest across workspace dependencies.
    pub manifest_digest: &'a str,
    /// Allocated memory budget in bytes.
    pub memory_budget: u64,
    /// Whether schema DDL tables have already been prepared on the connection.
    pub schema_ready: bool,
}

pub use crate::core::config::{policy, read_policy};
pub use crate::engine::pipeline::{build, delta, extract, SourceSnapshotMismatch};

pub(crate) fn prepare_pipeline_schema(
    conn: &mut Connection,
    memory_budget: u64,
    test_consumer_failure: bool,
) -> Result<()> {
    let cache_kb = sqlite_cache_size_kib(memory_budget);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=ON;"
    ))?;
    let (tables, _indexes) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    conn.execute_batch(tables)?;
    if test_consumer_failure {
        conn.execute_batch(
            "CREATE TEMP TRIGGER pipeline_test_consumer_failure
             BEFORE INSERT ON main.source_inventory
             WHEN NEW.path='src/unit_00.rs'
             BEGIN SELECT RAISE(ABORT,'injected pipeline SQLite consumer failure'); END;",
        )?;
    }
    Ok(())
}

pub(crate) fn sqlite_cache_size_kib(memory_budget: u64) -> usize {
    let raw = memory_budget / 8 / 1024;
    raw.clamp(4_096, 32_768) as usize
}

#[cfg(test)]
pub(crate) fn populate(
    conn: &mut Connection,
    root: &Path,
    adapter: &Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    graph: &CompactGraph,
    build_inputs: BuildInputs<'_>,
) -> Result<Value> {
    populate_owned(
        conn,
        root,
        adapter,
        entries,
        facts.clone(),
        graph,
        build_inputs,
        &mut BTreeMap::new(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_owned(
    conn: &mut Connection,
    root: &Path,
    adapter: &Adapter,
    entries: &[FileEntry],
    mut facts: BTreeMap<String, TypedFacts>,
    graph: &CompactGraph,
    build_inputs: BuildInputs<'_>,
    staged_payloads: &mut BTreeMap<String, StagedFilePayload>,
) -> Result<Value> {
    let streamed_payloads = !staged_payloads.is_empty();
    let cache_kb = sqlite_cache_size_kib(build_inputs.memory_budget);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=ON;"
    ))?;
    conn.set_prepared_statement_cache_capacity(128);
    let (tables, indexes) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    if !build_inputs.schema_ready {
        conn.execute_batch(tables)?;
    }
    (|| -> Result<Value> {
        let mut projection_timings = seal::ProjectionTimings::default();
        let mut logical_commitments = BTreeMap::from([
            ("nodes", BTreeMap::new()),
            ("doc_sections", BTreeMap::new()),
            ("shared_owners", BTreeMap::new()),
        ]);
        let mut seal_projection_ms = 0.0;
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
            ("schema_version", config::ENGINE_SCHEMA_VERSION.to_owned()),
            (
                "index_semantics_version",
                config::INDEX_SEMANTICS_VERSION.to_owned(),
            ),
            ("indexer_engine", "contextunity-forge-mcp-rust".to_owned()),
            ("workspace_root", root.to_string_lossy().into_owned()),
            ("adapter", config::policy(root, adapter).to_string()),
            (
                "adapter_version",
                adapter.adapter_version.clone().unwrap_or_default(),
            ),
            ("adapter_digest", adapter.digest.clone()),
            ("manifest_digest", build_inputs.manifest_digest.to_owned()),
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
        let mut path_cache = PathDictionaryCache::new();
        let all_paths = entries
            .iter()
            .map(|e| e.path.as_str())
            .chain(graph.edges.iter().map(|e| e.path.as_str()))
            .chain(graph.coverage.iter().map(|c| c.path.as_str()))
            .chain(
                entries
                    .iter()
                    .filter_map(|entry| facts.get(&entry.path))
                    .flat_map(|file_facts| file_facts.nodes.iter().map(|node| node.path.as_str())),
            );
        let t_bulk = Instant::now();
        path_cache.bulk_insert(&tx, all_paths)?;
        let bulk_paths_ms = t_bulk.elapsed().as_secs_f64() * 1000.;
        let mut node_paths: hashbrown::HashMap<String, Option<CachedNodePath>> =
            hashbrown::HashMap::with_capacity(facts.values().map(|f| f.nodes.len()).sum());
        let mut shared_paths = hashbrown::HashMap::<&str, Arc<str>>::new();
        for f in facts.values() {
            for n in &f.nodes {
                let shared_path = shared_paths
                    .entry(n.path.as_str())
                    .or_insert_with(|| Arc::<str>::from(n.path.as_str()));
                node_paths.insert(
                    n.id.clone(),
                    Some(CachedNodePath::from_shared(&n.id, Arc::clone(shared_path))),
                );
            }
        }

        let t_files = Instant::now();
        let mut restore_payload_ms = 0.0;
        let composed_sources = facts
            .values()
            .filter(|_| streamed_payloads)
            .flat_map(|facts| &facts.nodes)
            .filter(|node| node.details.get("included_from").is_some())
            .filter_map(|node| node.details["source_path"].as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|path| {
                let staged = staged_payloads
                    .get(path)
                    .context("missing composed source payload")?;
                Ok((
                    path.to_owned(),
                    catalog_payload(path, &staged.encoded_facts)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let file_timings = if streamed_payloads {
            persist_catalog_files(
                &tx,
                &mut path_cache,
                entries,
                &facts,
                &mut |chunk, facts| {
                    let staged = chunk
                        .iter()
                        .map(|file| {
                            Ok((
                                file,
                                facts.get(&file.path).context("missing catalog file")?,
                                staged_payloads
                                    .remove(&file.path)
                                    .context("missing staged file payload")?,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let prepared = staged
                        .into_par_iter()
                        .map(|(file, lean, staged)| {
                            let restoring = Instant::now();
                            let (storage, encoded) = if let Some(storage) = staged.storage_rows {
                                let bytes = zstd::stream::decode_all(storage.as_slice())?;
                                let storage: PreparedCatalogStorage =
                                    serde_json::from_slice(&bytes)?;
                                (storage, staged.encoded_facts)
                            } else {
                                let full = restore_catalog_file(
                                    file,
                                    lean,
                                    &staged.encoded_facts,
                                    &composed_sources,
                                )?;
                                let encoded = encode_facts(&full)?;
                                let single = BTreeMap::from([(file.path.clone(), full)]);
                                (prepare_catalog_rows(file, &single)?, encoded)
                            };
                            let local_facts_leaf = seal::local_facts_leaf(
                                &file.path,
                                &file.digest,
                                &file.language,
                                is_test(&file.path),
                                &encoded,
                            )?;
                            Ok((
                                LoadedCatalogFile {
                                    facts: storage.facts,
                                    encoded,
                                    local_facts_leaf,
                                    details_json: storage.details_json,
                                    search_texts: storage.search_texts,
                                    reference_keys: storage.reference_keys,
                                },
                                storage.leaves,
                                restoring.elapsed().as_secs_f64() * 1000.,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let mut loaded = Vec::with_capacity(prepared.len());
                    for (file, mut leaves, elapsed_ms) in prepared {
                        for domain in ["nodes", "doc_sections", "shared_owners"] {
                            logical_commitments
                                .get_mut(domain)
                                .context("missing storage commitment domain")?
                                .extend(
                                    leaves
                                        .remove(domain)
                                        .context("missing prepared commitment domain")?,
                                );
                        }
                        restore_payload_ms += elapsed_ms;
                        loaded.push(file);
                    }
                    Ok(loaded)
                },
            )?
        } else {
            persist_files(&tx, &mut path_cache, entries, &facts, None, true)?
        };
        drop(composed_sources);
        let node_id = file_timings.node_id;
        let node_row_count = file_timings.node_row_count;
        let node_row_insert_ms = file_timings
            .node_row_insert_ms
            .context("cold file persistence must report node row insertion time")?;
        let docs_count = file_timings.docs_count;
        let fts_insert_ms = file_timings.fts_insert_ms;
        let encode_facts_ms = file_timings.encode_facts_ms;
        let local_facts_batch_rows = file_timings.local_facts_batch_rows;
        let persist_files_ms = t_files.elapsed().as_secs_f64() * 1000. - fts_insert_ms;
        let t_doc_fts = Instant::now();
        if docs_count > 0 {
            tx.execute(
            "INSERT INTO doc_search(rowid,section_title,content,invariants) SELECT rowid,section_title,content,invariants FROM doc_sections",
            [],
        )?;
        }
        let doc_fts_insert_ms = t_doc_fts.elapsed().as_secs_f64() * 1000.;

        drop(shared_paths);
        if streamed_payloads {
            for file_facts in facts.values_mut() {
                retain_graph_storage_facts(file_facts);
            }
        }
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            // SAFETY: glibc synchronizes allocator arenas; no pointers are passed.
            libc::malloc_trim(0);
        }
        let (mut domains, graph_timings, persist_graph_ms, graph_projection_ms) =
            std::thread::scope(|scope| -> Result<_> {
                let projection = scope.spawn(|| {
                    let started = Instant::now();
                    let domains = seal::logical_domain_leaves(
                        entries,
                        &facts,
                        graph,
                        &mut projection_timings,
                        !streamed_payloads,
                    );
                    #[cfg(all(target_os = "linux", target_env = "gnu"))]
                    unsafe {
                        // SAFETY: projection scratch buffers have dropped; arenas are synchronized.
                        libc::malloc_trim(0);
                    }
                    (domains, started.elapsed().as_secs_f64() * 1000.)
                });
                let started = Instant::now();
                let stored = persist_graph(
                    &tx,
                    &mut path_cache,
                    graph,
                    &facts,
                    None,
                    &mut node_paths,
                    true,
                );
                let persist_ms = started.elapsed().as_secs_f64() * 1000.;
                let (domains, projection_ms) = projection
                    .join()
                    .map_err(|_| anyhow::anyhow!("graph commitment projection panicked"))?;
                Ok((domains?, stored?, persist_ms, projection_ms))
            })?;
        seal_projection_ms += graph_projection_ms;
        if streamed_payloads {
            for (domain, leaves) in logical_commitments {
                domains.insert(domain, leaves);
            }
        }
        let mut logical_commitments = domains;

        let rows_ms = writing.elapsed().as_secs_f64() * 1000.;
        tx.commit()?;
        // Rows and their logical commitment leaves are now independent of the
        // extracted facts. Release those facts before index and FTS construction.
        drop(facts);
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            // SAFETY: glibc synchronizes allocator arenas; no pointers are passed.
            libc::malloc_trim(0);
        }
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let index_and_seal = Instant::now();
        let index_sql = format!("CREATE INDEX{indexes}");
        let index_candidate = |conn: &mut Connection| -> Result<f64> {
            let started = Instant::now();
            conn.execute_batch(&format!(
                "PRAGMA cache_size=-{cache_kb}; PRAGMA temp_store=MEMORY;"
            ))?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(&index_sql)?;
            tx.commit()?;
            Ok(started.elapsed().as_secs_f64() * 1000.)
        };
        let indexes_ms = index_candidate(conn)?;
        let t_fts = Instant::now();
        let fts_digests = commitments::fts_commitment_digests(conn)?;
        let seal_fts_ms = t_fts.elapsed().as_secs_f64() * 1000.;
        logical_commitments.insert("local_facts", file_timings.local_facts_leaves);
        let seal_started = Instant::now();
        let snapshot_started = Instant::now();
        let (seal, snapshot_timings): (String, commitments::SnapshotTimings) =
            commitments::seal_snapshot_from_projection(conn, logical_commitments, fts_digests)?;
        let seal_snapshot_ms = snapshot_started.elapsed().as_secs_f64() * 1000.;
        let seal_ms = seal_started.elapsed().as_secs_f64() * 1000.;
        conn.execute_batch(&format!("PRAGMA cache_size=-{cache_kb};"))?;
        let index_and_seal_ms = index_and_seal.elapsed().as_secs_f64() * 1000.;
        let persist_ms = writing.elapsed().as_secs_f64() * 1000.;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let mut report = json!({"files":entries.len(),"nodes":node_id,"node_row_count":node_row_count,"node_row_insert_ms":node_row_insert_ms,"edges":graph.edges.len(),"doc_sections":docs_count,"output_root":seal,"schema_version":config::ENGINE_SCHEMA_VERSION,"link_ms":0.0,"persist_ms":persist_ms,"rows_ms":rows_ms,"bulk_paths_ms":bulk_paths_ms,"persist_files_ms":persist_files_ms,"fts_insert_ms":fts_insert_ms,"doc_fts_insert_ms":doc_fts_insert_ms,"persist_graph_ms":persist_graph_ms,"indexes_ms":indexes_ms,"seal_ms":seal_ms,"seal_projection_ms":seal_projection_ms,"seal_fts_ms":seal_fts_ms,"seal_file_domains_ms":projection_timings.file_domains_ms,"seal_node_domain_ms":projection_timings.node_domain_ms,"seal_graph_edges_ms":projection_timings.graph_edges_ms,"seal_graph_occurrences_ms":projection_timings.graph_occurrences_ms,"seal_graph_unique_edges_ms":projection_timings.graph_unique_edges_ms,"seal_graph_shared_owners_ms":projection_timings.graph_shared_owners_ms,"seal_graph_coverage_ms":projection_timings.graph_coverage_ms,"seal_graph_dependencies_ms":projection_timings.graph_dependencies_ms,"seal_graph_domains_ms":projection_timings.graph_domains_ms,"seal_snapshot_ms":seal_snapshot_ms,"seal_snapshot_logical_domains_ms":snapshot_timings.logical_domains_ms,"seal_snapshot_install_ms":snapshot_timings.install_ms,"encode_facts_ms":encode_facts_ms,"sqlite_ingestion":{"mode":"deferred-index-batched","secondary_indexes_deferred":true,"batch_rows":local_facts_batch_rows}});
        report["graph_phases_ms"] = json!({"occurrences_and_dependencies":graph_timings.occurrences,"unique_edges":graph_timings.edges,"coverage_dictionaries":graph_timings.dictionaries,"coverage_and_unresolved":graph_timings.coverage});
        report["index_and_seal_ms"] = json!(index_and_seal_ms);
        report["restore_payload_ms"] = json!(restore_payload_ms);
        report["commitment_seal_mode"] = json!("in-memory-projection");
        Ok(report)
    })()
}

fn restore_catalog_file(
    file: &FileEntry,
    lean: &TypedFacts,
    encoded: &[u8],
    composed_sources: &BTreeMap<String, CatalogStoragePayload>,
) -> Result<TypedFacts> {
    let mut full = lean.clone();
    catalog_payload(&file.path, encoded)?.restore(&mut full);
    for node in &mut full.nodes {
        if node.details.get("included_from").is_some() {
            if let Some(source) = node.details["source_path"].as_str() {
                let payload = composed_sources
                    .get(source)
                    .context("missing composed source")?;
                payload.restore_composed_node(node);
            }
        }
    }
    Ok(full)
}

fn catalog_payload(path: &str, encoded: &[u8]) -> Result<CatalogStoragePayload> {
    let facts = crate::core::typed_facts::decode_facts(encoded)?;
    Ok(crate::core::semantic::SymbolCatalog::default().insert_typed(path.to_owned(), facts, true))
}

fn retain_graph_storage_facts(facts: &mut TypedFacts) {
    let html = facts
        .nodes
        .iter()
        .any(|node| node.kind == "module" && node.language == "html");
    let mut flows = crate::core::typed_facts::FlowStore::default();
    for node in &mut facts.facts.nodes {
        if let Some(flow) = facts.flows.get_mut(&node.id) {
            if !flow.template_contexts.is_empty() {
                flows.insert(
                    node.id.clone(),
                    crate::core::semantic::ValueFlowFacts {
                        template_contexts: std::mem::take(&mut flow.template_contexts),
                        ..Default::default()
                    },
                );
            }
        }
        node.details = Value::Null;
        node.name = String::new();
        node.qualname = String::new();
    }
    facts.flows = flows;
    facts.facts.edges = Vec::new();
    facts.facts.docs = Vec::new();
    if !html {
        facts.facts.references = Vec::new();
    }
}

fn cached(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> rusqlite::Result<usize> {
    conn.prepare_cached(sql)?.execute(params)
}

fn reference_keys(facts: &TypedFacts) -> Vec<&str> {
    facts.reference_keys()
}

#[cfg(test)]
#[path = "writer/memory_budget_tests.rs"]
mod memory_budget_tests;
#[cfg(test)]
#[path = "writer/multi_value_batch_tests.rs"]
mod multi_value_batch_tests;
