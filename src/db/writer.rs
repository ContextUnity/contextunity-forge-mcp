//! SQLite persistence and indexing operations for Forge.

use crate::core::models::compact_graph::Graph as CompactGraph;
use crate::core::typed_facts::TypedFacts;
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
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::Path,
    sync::Arc,
    time::Instant,
};

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
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=OFF;"
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
    raw.clamp(4_096, 128_000) as usize
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate(
    conn: &mut Connection,
    root: &Path,
    adapter: &Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
    graph: &CompactGraph,
    build_inputs: BuildInputs<'_>,
) -> Result<Value> {
    let cache_kb = sqlite_cache_size_kib(build_inputs.memory_budget);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=OFF;"
    ))?;
    conn.set_prepared_statement_cache_capacity(128);
    let (tables, indexes) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    if !build_inputs.schema_ready {
        conn.execute_batch(tables)?;
    }
    std::thread::scope(|scope| -> Result<Value> {
        let graph_ref = graph;
        let projection_started = Instant::now();
        let projection_thread = scope.spawn(move || {
            let mut timings = seal::ProjectionTimings::default();
            let leaves = seal::logical_domain_leaves(
                entries,
                facts,
                graph_ref,
                encoded_facts,
                &mut timings,
            )?;
            Ok::<_, anyhow::Error>((
                leaves,
                timings,
                projection_started.elapsed().as_secs_f64() * 1000.,
            ))
        });
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
        let file_timings =
            persist_files(&tx, &mut path_cache, entries, facts, encoded_facts, true)?;
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

        let t_graph = Instant::now();
        let graph_timings = persist_graph(
            &tx,
            &mut path_cache,
            graph,
            facts,
            None,
            &mut node_paths,
            true,
        )?;
        let persist_graph_ms = t_graph.elapsed().as_secs_f64() * 1000.;

        let rows_ms = writing.elapsed().as_secs_f64() * 1000.;
        tx.commit()?;
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
        let (logical_commitments, projection_timings, seal_projection_ms) = projection_thread
            .join()
            .map_err(|_| anyhow::anyhow!("in-memory seal projection thread panicked"))??;
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
        report["commitment_seal_mode"] = json!("in-memory-projection");
        Ok(report)
    })
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
