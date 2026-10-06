use super::{
    aggregate, batch_insert_commitments, fts_digest, leaves, root, seal, ALGORITHM, DOMAINS, FTS,
};
use anyhow::Result;
use rayon::prelude::*;
use rusqlite::{params, Connection, OpenFlags};
use std::collections::BTreeMap;

/// Seal a committed, private build candidate; published and delta databases use `seal`.
pub(crate) fn seal_snapshot(conn: &Connection) -> Result<String> {
    anyhow::ensure!(
        conn.is_autocommit(),
        "snapshot sealing requires committed ingestion"
    );
    let nodes: usize = conn.query_row("SELECT count(*) FROM nodes", [], |row| row.get(0))?;
    let Some(path) = conn.path().filter(|path| !path.is_empty()) else {
        return seal(conn);
    };
    if nodes < 8192 || rayon::current_num_threads() == 1 {
        return seal(conn);
    }
    install_snapshot(conn, hash_snapshot(path)?)
}

struct DomainCommitment {
    table: &'static str,
    entries: Option<BTreeMap<String, String>>,
    digest: String,
}

struct SnapshotCommitments {
    domains: Vec<DomainCommitment>,
}

/// Hash committed candidate rows while its logical domains remain immutable.
fn hash_snapshot(path: &str) -> Result<SnapshotCommitments> {
    let domains = (0..DOMAINS.len() + FTS.len())
        .into_par_iter()
        .map(|index| -> Result<_> {
            let reader = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            reader.busy_timeout(std::time::Duration::from_secs(5))?;
            reader.execute_batch(
                "PRAGMA query_only=ON; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-4096; PRAGMA mmap_size=268435456;",
            )?;
            if index < DOMAINS.len() {
                let table = DOMAINS[index];
                let entries = leaves(&reader, table, None)?;
                let digest = aggregate(table, &entries);
                Ok(DomainCommitment {
                    table,
                    entries: Some(entries),
                    digest,
                })
            } else {
                let (table, order_by) = FTS[index - DOMAINS.len()];
                Ok(DomainCommitment {
                    table,
                    entries: None,
                    digest: fts_digest(&reader, table, order_by)?,
                })
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SnapshotCommitments { domains })
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
