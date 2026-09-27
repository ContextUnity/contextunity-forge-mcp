use anyhow::{bail, Result};
use rusqlite::{params, types::ValueRef, Connection};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub const ALGORITHM: &str = "forge-owner-sha256-v1";
pub const DOMAINS: &[&str] = &[
    "source_inventory",
    "files",
    "nodes",
    "edges",
    "local_facts",
    "owned_nodes",
    "edge_occurrences",
    "owned_search",
    "dependencies",
    "reverse_dependencies",
    "shared_owners",
    "file_commitments",
    "search_text",
    "errors",
    "resolution_coverage",
    "doc_sections",
];
const FTS: &[&str] = &[
    "node_search_data",
    "node_search_idx",
    "node_search_config",
    "doc_search_data",
    "doc_search_idx",
    "doc_search_docsize",
    "doc_search_config",
];
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn encode_row(hasher: &mut Sha256, row: &rusqlite::Row<'_>, columns: usize) -> Result<()> {
    hasher.update([0xff]);
    for i in 0..columns {
        match row.get_ref(i)? {
            ValueRef::Null => hasher.update(b"n"),
            ValueRef::Integer(n) => {
                hasher.update(b"i");
                hasher.update(n.to_le_bytes());
            }
            ValueRef::Real(n) => {
                hasher.update(b"r");
                hasher.update(n.to_bits().to_le_bytes());
            }
            ValueRef::Text(s) => {
                hasher.update(b"t");
                hasher.update((s.len() as u64).to_le_bytes());
                hasher.update(s);
            }
            ValueRef::Blob(s) => {
                hasher.update(b"b");
                hasher.update((s.len() as u64).to_le_bytes());
                hasher.update(s);
            }
        }
    }
    Ok(())
}
fn key(table: &str, owner: &str) -> String {
    format!("leaf:{}", serde_json::json!([table, owner]))
}
fn shape(table: &str) -> (&'static str, &'static str, &'static str) {
    match table {
        "nodes" => (
            "o.owner",
            "LEFT JOIN owned_nodes o ON o.public_id=t.id",
            "t.id",
        ),
        "search_text" => (
            "o.owner",
            "LEFT JOIN owned_nodes o ON o.public_id=t.public_id",
            "t.public_id",
        ),
        "edges" => ("t.path", "", "t.src_public_id,t.dst_public_id,t.kind"),
        "owned_nodes" => ("t.owner", "", "t.public_id"),
        "edge_occurrences" => ("t.owner", "", "t.ordinal"),
        "owned_search" => ("t.owner", "", "t.public_id,t.ordinal"),
        "dependencies" => ("t.owner", "", "t.target,t.kind,t.symbol,t.resolution"),
        "reverse_dependencies" => ("t.owner", "", "t.target,t.kind,t.symbol,t.resolution"),
        "shared_owners" => ("t.owner", "", "t.kind,t.key,t.ordinal"),
        "errors" => ("t.path", "", "t.line,t.message"),
        "resolution_coverage" => ("t.path", "", "t.line,t.expression,t.status,t.evidence"),
        "doc_sections" => ("t.path", "", "t.doc_id"),
        _ => ("t.path", "", "t.path"),
    }
}
fn leaves(
    conn: &Connection,
    table: &str,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    let (ownership, join, order) = shape(table);
    let filter = if owners.is_some() {
        format!("WHERE {ownership} IN(SELECT value FROM json_each(?1))")
    } else {
        String::new()
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT t.*,{ownership} AS __owner FROM {table} t {join} {filter} ORDER BY __owner,{order}"
    ))?;
    let columns = stmt.column_count() - 1;
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let mut rows = if let Some(encoded) = &encoded {
        stmt.query([encoded])?
    } else {
        stmt.query([])?
    };
    let mut result = BTreeMap::new();
    let mut current: Option<(String, Sha256)> = None;
    while let Some(row) = rows.next()? {
        let owner: String = row.get(columns)?;
        if current.as_ref().is_none_or(|(old, _)| *old != owner) {
            if let Some((old, h)) = current.take() {
                result.insert(key(table, &old), hex::encode(h.finalize()));
            }
            let mut h = Sha256::new();
            h.update(ALGORITHM);
            h.update(table);
            h.update((owner.len() as u64).to_le_bytes());
            h.update(&owner);
            current = Some((owner, h));
        }
        encode_row(
            &mut current.as_mut().expect("owner initialized").1,
            row,
            columns,
        )?;
    }
    if let Some((owner, h)) = current {
        result.insert(key(table, &owner), hex::encode(h.finalize()));
    }
    Ok(result)
}
fn fts_digest(conn: &Connection, table: &str) -> Result<String> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table}"))?;
    let columns = stmt.column_count();
    let mut rows = stmt.query([])?;
    let mut h = Sha256::new();
    h.update(table);
    while let Some(row) = rows.next()? {
        encode_row(&mut h, row, columns)?;
    }
    Ok(hex::encode(h.finalize()))
}
fn root(conn: &Connection) -> Result<String> {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    let mut stmt=conn.prepare("SELECT domain,digest FROM domain_commitments WHERE domain NOT LIKE 'leaf:%' ORDER BY domain")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    let mut stmt=conn.prepare("SELECT key,value FROM metadata WHERE key IN('schema_version','index_semantics_version','indexer_engine','workspace_root','adapter','corpus_hash','commitment_algorithm','inventory_snapshot')ORDER BY key")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update((v.len() as u64).to_le_bytes());
        h.update(v);
    }
    Ok(hex::encode(h.finalize()))
}
fn aggregate(table: &str, leaves: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    h.update(table);
    for (k, v) in leaves {
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    hex::encode(h.finalize())
}
fn stored(conn: &Connection, table: &str) -> Result<BTreeMap<String, String>> {
    let prefix = format!("leaf:[\"{table}\",");
    let mut stmt = conn.prepare_cached(
        "SELECT domain,digest FROM domain_commitments WHERE domain>=?1 AND domain<?2 ORDER BY domain",
    )?;
    let rows = stmt
        .query_map(params![prefix, format!("{prefix}\u{10ffff}")], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok(rows)
}
pub fn seal(conn: &Connection) -> Result<String> {
    seal_owners(conn, None)
}
pub fn seal_owners(conn: &Connection, owners: Option<&BTreeSet<String>>) -> Result<String> {
    conn.execute(
        "INSERT OR REPLACE INTO metadata VALUES('commitment_algorithm',?1)",
        [ALGORITHM],
    )?;
    for table in DOMAINS {
        let changed = leaves(conn, table, owners)?;
        if let Some(owners) = owners {
            let mut st = conn.prepare_cached("DELETE FROM domain_commitments WHERE domain=?1")?;
            for owner in owners {
                st.execute([key(table, owner)])?;
            }
        } else {
            let prefix = format!("leaf:[\"{table}\",");
            conn.execute(
                "DELETE FROM domain_commitments WHERE domain>=?1 AND domain<?2",
                params![prefix, format!("{prefix}\u{10ffff}")],
            )?;
        }
        let mut st =
            conn.prepare_cached("INSERT OR REPLACE INTO domain_commitments VALUES(?1,?2)")?;
        for (k, v) in &changed {
            st.execute(params![k, v])?;
        }
        let digest = if owners.is_none() {
            aggregate(table, &changed)
        } else {
            aggregate(table, &stored(conn, table)?)
        };
        st.execute(params![table, digest])?;
    }
    for table in FTS {
        conn.execute(
            "INSERT OR REPLACE INTO domain_commitments VALUES(?1,?2)",
            params![table, fts_digest(conn, table)?],
        )?;
    }
    let root = root(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata VALUES('output_root',?1)",
        [&root],
    )?;
    Ok(root)
}
pub fn verify(conn: &Connection) -> Result<()> {
    verify_owners(conn, None)
}
pub fn verify_owners(conn: &Connection, owners: Option<&BTreeSet<String>>) -> Result<()> {
    let algorithm: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='commitment_algorithm'",
        [],
        |r| r.get(0),
    )?;
    if algorithm != ALGORITHM {
        bail!("incompatible commitment algorithm; rebuild index");
    }
    for table in DOMAINS {
        let actual = leaves(conn, table, owners)?;
        let all = stored(conn, table)?;
        let expected = if let Some(owners) = owners {
            let keys: BTreeSet<_> = owners.iter().map(|o| key(table, o)).collect();
            all.iter()
                .filter(|(k, _)| keys.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            all.clone()
        };
        if actual != expected {
            bail!("commitment mismatch: {table}");
        }
        let stored: String = conn.query_row(
            "SELECT digest FROM domain_commitments WHERE domain=?1",
            [table],
            |r| r.get(0),
        )?;
        if aggregate(table, &all) != stored {
            bail!("domain root mismatch: {table}");
        }
    }
    for table in FTS {
        let stored: String = conn.query_row(
            "SELECT digest FROM domain_commitments WHERE domain=?1",
            [table],
            |r| r.get(0),
        )?;
        if fts_digest(conn, table)? != stored {
            bail!("FTS commitment mismatch: {table}");
        }
    }
    let stored: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    if root(conn)? != stored {
        bail!("output root mismatch");
    }
    Ok(())
}
