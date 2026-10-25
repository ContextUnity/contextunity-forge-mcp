//! Incremental delta persistence operations.

use anyhow::{bail, Result};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

use super::{
    batch::{BorrowedSqlValue, MultiValueBatch},
    dictionary::PathDictionaryCache,
    ingest::{persist_files, persist_graph, CachedNodePath},
};
use crate::core::{
    commitments,
    models::{compact_graph::Graph as CompactGraph, stable_hash64, FileEntry},
    typed_facts::TypedFacts,
};

fn cached(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> rusqlite::Result<usize> {
    conn.prepare_cached(sql)?.execute(params)
}

fn delete_file(tx: &Connection, path_cache: &mut PathDictionaryCache, path: &str) -> Result<()> {
    let owner_id = path_cache.get_or_insert(tx, path)?;
    cached(
        tx,
        "DELETE FROM node_search WHERE rowid IN(SELECT node_hash FROM nodes WHERE owner_path_id=?1)",
        [owner_id],
    )?;
    cached(
        tx,
        "INSERT INTO doc_search(doc_search,rowid,section_title,content,invariants)SELECT 'delete',rowid,section_title,content,invariants FROM doc_sections WHERE path=?1",
        [path],
    )?;
    cached(
        tx,
        "UPDATE shared_keys SET key=(SELECT qualname FROM nodes WHERE node_id=shared_keys.key) WHERE typeof(key)='integer' AND key IN(SELECT node_id FROM nodes WHERE owner_path_id=?1)",
        [owner_id],
    )?;
    cached(tx, "DELETE FROM nodes WHERE owner_path_id=?1", [owner_id])?;
    for (table, column) in [
        ("source_inventory", "path"),
        ("files", "path"),
        ("local_facts", "path"),
        ("shared_owners", "owner_id"),
        ("errors", "path"),
        ("doc_sections", "path"),
    ] {
        let value = if column == "owner_id" {
            rusqlite::types::Value::Integer(owner_id)
        } else {
            rusqlite::types::Value::Text(path.to_owned())
        };
        cached(
            tx,
            &format!("DELETE FROM {table} WHERE {column}=?1"),
            [value],
        )?;
    }
    Ok(())
}

/// Inputs for delta SQLite database updates.
pub(crate) struct DeltaPersistenceInputs<'a> {
    pub db: &'a Path,
    pub root: &'a Path,
    pub expected: &'a str,
    pub admitted_identity: crate::db::cache::Identity,
    pub entries: &'a [FileEntry],
    pub affected: &'a BTreeSet<String>,
    pub replace_files: &'a BTreeSet<String>,
    pub facts: &'a BTreeMap<String, TypedFacts>,
    pub graph: &'a CompactGraph,
    pub manifest_digest: &'a str,
}

/// Timings and output metadata resulting from delta persistence.
pub(crate) struct DeltaPersistResult {
    pub delete_ms: f64,
    pub files_ms: f64,
    pub graph_ms: f64,
    pub aggregate_ms: f64,
    pub commitments_ms: f64,
    pub seal_ms: f64,
    pub verify_ms: f64,
    pub commit_ms: f64,
    pub persist_ms: f64,
    pub output_root: String,
    pub verification_cache: bool,
    pub rewritten_files: usize,
}

pub(crate) fn persist_delta(
    inputs: DeltaPersistenceInputs<'_>,
    check_snapshot: impl FnOnce(f64, f64, f64, f64) -> Result<()>,
) -> Result<DeltaPersistResult> {
    let DeltaPersistenceInputs {
        db,
        root,
        expected,
        admitted_identity,
        entries,
        affected,
        replace_files,
        facts,
        graph,
        manifest_digest,
    } = inputs;

    let persisting = Instant::now();
    if crate::db::cache::identity(db)? != admitted_identity {
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
    let mut path_cache = PathDictionaryCache::new();
    path_cache.load_from_db(&tx)?;
    tx.execute_batch("CREATE TEMP TABLE changed_edge_keys(src_hash INTEGER,dst_hash INTEGER,kind TEXT,PRIMARY KEY(src_hash,dst_hash,kind));")?;
    for path in affected {
        let owner_id = path_cache.get_or_insert(&tx, path)?;
        cached(
            &tx,
            "INSERT OR IGNORE INTO changed_edge_keys SELECT src_hash,dst_hash,kind FROM edge_occurrences WHERE owner_id=?1",
            [owner_id],
        )?;
    }
    let mut changed_edge_batch = MultiValueBatch::<3, BorrowedSqlValue<'_>>::new(
        &tx,
        "INSERT OR IGNORE INTO changed_edge_keys VALUES",
    )?;
    for e in &graph.edges {
        changed_edge_batch.push([
            BorrowedSqlValue::Integer(stable_hash64(&e.src)),
            BorrowedSqlValue::Integer(stable_hash64(&e.dst)),
            BorrowedSqlValue::Text(e.kind.as_str()),
        ])?;
    }
    changed_edge_batch.flush()?;
    drop(changed_edge_batch);
    let mut commitment_owners = affected.clone();
    commitment_owners.extend(
        tx.prepare_cached("SELECT DISTINCT p.path FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE (e.src_hash,e.dst_hash,e.kind)IN(SELECT src_hash,dst_hash,kind FROM changed_edge_keys)")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    tx.execute("DELETE FROM edges WHERE (src_hash,dst_hash,kind)IN(SELECT src_hash,dst_hash,kind FROM changed_edge_keys)", [])?;
    for path in affected {
        let owner_id = path_cache.get_or_insert(&tx, path)?;
        for (table, column) in [
            ("edge_occurrences", "owner_id"),
            ("dependencies", "owner_id"),
            ("resolution_coverage", "path_id"),
            ("coverage_owner_language", "path_id"),
            ("coverage_language_counts", "path_id"),
        ] {
            cached(
                &tx,
                &format!("DELETE FROM {table} WHERE {column}=?1"),
                [owner_id],
            )?;
        }
    }
    tx.execute_batch(
        "CREATE TEMP TABLE replaced_nodes(id TEXT PRIMARY KEY,node_hash INTEGER NOT NULL)",
    )?;
    tx.execute("INSERT OR IGNORE INTO replaced_nodes SELECT n.id,n.node_hash FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE owner.path IN(SELECT value FROM json_each(?1))", [serde_json::to_string(&replace_files)?])?;
    for path in replace_files {
        delete_file(&tx, &mut path_cache, path)?;
    }
    let delete_ms = deleting.elapsed().as_secs_f64() * 1000.;
    let changed_entries: Vec<_> = entries
        .iter()
        .filter(|f| replace_files.contains(&f.path))
        .cloned()
        .collect();
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
    let files_writing = Instant::now();
    persist_files(&tx, &mut path_cache, &changed_entries, facts, false)?;
    let files_ms = files_writing.elapsed().as_secs_f64() * 1000.;
    let graph_writing = Instant::now();
    persist_graph(
        &tx,
        &mut path_cache,
        graph,
        facts,
        Some(affected),
        &mut node_paths,
        false,
    )?;
    tx.execute(
        "DELETE FROM coverage_evidence WHERE evidence_id NOT IN(SELECT evidence_id FROM resolution_coverage UNION SELECT evidence_id FROM edges UNION SELECT confidence_id FROM edges UNION SELECT evidence_id FROM edge_occurrences UNION SELECT confidence_id FROM edge_occurrences)",
        [],
    )?;
    let graph_ms = graph_writing.elapsed().as_secs_f64() * 1000.;
    let aggregating = Instant::now();
    let dangling: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM replaced_nodes r WHERE NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=r.id AND n.node_hash=r.node_hash) AND EXISTS(SELECT 1 FROM edges e WHERE e.src_hash=r.node_hash OR e.dst_hash=r.node_hash))", [], |r| r.get(0))?;
    if dangling {
        bail!("delta leaves an edge with a missing endpoint; rebuild index");
    }
    tx.execute(
        "WITH ranked AS (
            SELECT e.*,e.owner_id AS path_id,p.path AS owner_path,
                   row_number() OVER edge_group AS representative,
                   count(*) OVER edge_group AS occurrences
            FROM changed_edge_keys k JOIN edge_occurrences e
              ON k.src_hash=e.src_hash AND k.dst_hash=e.dst_hash AND k.kind=e.kind
            JOIN path_dictionary p ON p.path_id=e.owner_id
            WINDOW edge_group AS (
                PARTITION BY e.src_hash,e.dst_hash,e.kind ORDER BY p.path,e.ordinal
                ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
            )
        )
        INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count)
        SELECT src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrences
        FROM ranked WHERE representative=1",
        [],
    )?;
    commitment_owners.extend(
        tx.prepare_cached("SELECT DISTINCT p.path FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE (e.src_hash,e.dst_hash,e.kind)IN(SELECT src_hash,dst_hash,kind FROM changed_edge_keys)")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    let aggregate_ms = aggregating.elapsed().as_secs_f64() * 1000.;
    let commitments_writing = Instant::now();
    let commitments_ms = commitments_writing.elapsed().as_secs_f64() * 1000.;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('corpus_hash',?1)",
        [commitments::hash(&serde_json::to_vec(entries)?)],
    )?;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('inventory_snapshot',?1)",
        [serde_json::to_string(entries)?],
    )?;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('manifest_digest',?1)",
        [manifest_digest],
    )?;
    let persist_ms = persisting.elapsed().as_secs_f64() * 1000.;
    let sealing = Instant::now();
    let seal = commitments::seal_owners(&tx, Some(&commitment_owners))?;
    let seal_ms = sealing.elapsed().as_secs_f64() * 1000.;
    let verifying = Instant::now();
    commitments::verify_owners(&tx, Some(&commitment_owners))?;

    let verify_ms = verifying.elapsed().as_secs_f64() * 1000.;
    let verify_elapsed = verifying.elapsed().as_secs_f64() * 1000.;
    check_snapshot(persist_ms, seal_ms, verify_ms, verify_elapsed)?;

    let committing = Instant::now();
    tx.commit()?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    let commit_ms = committing.elapsed().as_secs_f64() * 1000.;
    drop(conn);
    let verification_cache = crate::db::cache::publish_verified(db, root, &seal).unwrap_or(false);

    Ok(DeltaPersistResult {
        delete_ms,
        files_ms,
        graph_ms,
        aggregate_ms,
        commitments_ms,
        seal_ms,
        verify_ms,
        commit_ms,
        persist_ms,
        output_root: seal,
        verification_cache,
        rewritten_files: changed_entries.len(),
    })
}
