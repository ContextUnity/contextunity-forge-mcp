#[cfg(test)]
use super::seal;
use super::{
    aggregate, batch_insert_commitments, ensure_fts_vocab, fts_digest, root, ALGORITHM, DOMAINS,
    FTS,
};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct SnapshotTimings {
    pub(crate) logical_domains_ms: f64,
    pub(crate) install_ms: f64,
}

/// Retained as the SQL-backed test oracle for the in-memory snapshot path.
#[cfg(test)]
pub(crate) fn seal_snapshot(conn: &Connection) -> Result<String> {
    seal(conn)
}

/// Computes the FTS commitment digests independently from the logical table leaves.
pub(crate) fn fts_commitment_digests(conn: &Connection) -> Result<BTreeMap<&'static str, String>> {
    let mut digests = BTreeMap::new();
    for &(table, order_by) in FTS {
        if let Some(index) = table.strip_suffix("_postings") {
            ensure_fts_vocab(conn, index)?;
        }
        digests.insert(table, fts_digest(conn, table, order_by)?);
    }
    Ok(digests)
}

/// Seal logical owner leaves produced from the post-linking in-memory projection.
/// FTS commitments remain sourced from their SQLite virtual tables.
pub(crate) fn seal_snapshot_from_projection(
    conn: &Connection,
    mut logical: BTreeMap<&'static str, BTreeMap<String, String>>,
    mut fts_digests: BTreeMap<&'static str, String>,
) -> Result<(String, SnapshotTimings)> {
    anyhow::ensure!(
        conn.is_autocommit(),
        "snapshot sealing requires committed ingestion"
    );
    let mut domains = Vec::with_capacity(DOMAINS.len() + FTS.len());
    let logical_started = Instant::now();
    for &table in DOMAINS {
        let entries = logical
            .remove(table)
            .with_context(|| format!("in-memory projection is missing domain {table}"))?;
        domains.push(DomainCommitment {
            table,
            digest: aggregate(table, &entries),
            entries: Some(entries),
        });
    }
    let logical_domains_ms = logical_started.elapsed().as_secs_f64() * 1000.;
    anyhow::ensure!(
        logical.is_empty(),
        "in-memory projection contains an unknown logical domain"
    );
    for &(table, _) in FTS {
        let digest = fts_digests
            .remove(table)
            .with_context(|| format!("precomputed FTS commitments are missing domain {table}"))?;
        domains.push(DomainCommitment {
            table,
            entries: None,
            digest,
        });
    }
    anyhow::ensure!(
        fts_digests.is_empty(),
        "precomputed FTS commitments contain an unknown domain"
    );
    let install_started = Instant::now();
    let root = install_snapshot(conn, SnapshotCommitments { domains })?;
    let install_ms = install_started.elapsed().as_secs_f64() * 1000.;
    Ok((
        root,
        SnapshotTimings {
            logical_domains_ms,
            install_ms,
        },
    ))
}

struct DomainCommitment {
    table: &'static str,
    entries: Option<BTreeMap<String, String>>,
    digest: String,
}

struct SnapshotCommitments {
    domains: Vec<DomainCommitment>,
}

fn install_snapshot(conn: &Connection, snapshot: SnapshotCommitments) -> Result<String> {
    anyhow::ensure!(
        conn.is_autocommit(),
        "snapshot installation requires committed indexes"
    );
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO metadata VALUES('commitment_algorithm',?1)",
        [ALGORITHM],
    )?;
    tx.execute("DELETE FROM domain_commitments", [])?;
    for DomainCommitment {
        table,
        entries,
        digest,
    } in snapshot.domains
    {
        if let Some(entries) = entries {
            batch_insert_commitments(&tx, table, &entries)?;
        }
        tx.execute(
            "INSERT INTO domain_commitments VALUES(?1,0,unhex(?2))",
            params![table, digest],
        )?;
    }
    let root = root(&tx)?;
    tx.execute(
        "INSERT OR REPLACE INTO metadata VALUES('output_root',?1)",
        [&root],
    )?;
    tx.commit()?;
    Ok(root)
}
