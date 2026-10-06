use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use rusqlite::{params, types::ValueRef, Connection};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod coverage_leaves;
mod domain_leaves;
mod snapshot;

pub(crate) use snapshot::seal_snapshot;
/// Domain-separated owner commitment algorithm persisted in index metadata.
pub const ALGORITHM: &str = "forge-owner-sha256-v1";
/// Logical table domains included in the deterministic output root.
pub const DOMAINS: &[&str] = &[
    "source_inventory",
    "files",
    "nodes",
    "edges",
    "local_facts",
    "edge_occurrences",
    "dependencies",
    "shared_owners",
    "errors",
    "resolution_coverage",
    "coverage_owner_language",
    "coverage_language_counts",
    "doc_sections",
];
const FTS: &[(&str, &str)] = &[
    ("node_search_data", "id"),
    ("node_search_idx", "segid,term"),
    ("node_search_docsize", "id"),
    ("node_search_config", "k"),
    ("doc_search_data", "id"),
    ("doc_search_idx", "segid,term"),
    ("doc_search_docsize", "id"),
    ("doc_search_config", "k"),
];
/// Returns a lowercase SHA-256 digest of the supplied bytes.
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn encode_row(
    hasher: &mut impl sha2::digest::Update,
    row: &rusqlite::Row<'_>,
    columns: usize,
) -> Result<()> {
    hasher.update(&[0xff]);
    for i in 0..columns {
        match row.get_ref(i)? {
            ValueRef::Null => hasher.update(b"n"),
            ValueRef::Integer(n) => {
                hasher.update(b"i");
                hasher.update(&n.to_le_bytes());
            }
            ValueRef::Real(n) => {
                hasher.update(b"r");
                hasher.update(&n.to_bits().to_le_bytes());
            }
            ValueRef::Text(s) => {
                hasher.update(b"t");
                hasher.update(&(s.len() as u64).to_le_bytes());
                hasher.update(s);
            }
            ValueRef::Blob(s) => {
                hasher.update(b"b");
                hasher.update(&(s.len() as u64).to_le_bytes());
                hasher.update(s);
            }
        }
    }
    Ok(())
}
fn new_leaf_hasher(table: &str, owner: &str) -> Sha256 {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    h.update(table);
    h.update((owner.len() as u64).to_le_bytes());
    h.update(owner.as_bytes());
    h
}
fn key(table: &str, owner: &str) -> String {
    format!("leaf:{}", serde_json::json!([table, owner]))
}
fn shape(table: &str) -> (&'static str, &'static str, &'static str) {
    match table {
        "errors" => ("t.path", "", "t.line,t.message"),
        "doc_sections" => ("t.path", "", "t.doc_id"),
        _ => ("t.path", "", "t.path"),
    }
}
#[inline]
fn encode_text(h: &mut impl sha2::digest::Update, s: &str) {
    h.update(b"t");
    h.update(&(s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

#[inline]
fn encode_int(h: &mut impl sha2::digest::Update, n: i64) {
    h.update(b"i");
    h.update(&n.to_le_bytes());
}

#[inline]
fn encode_null(h: &mut impl sha2::digest::Update) {
    h.update(b"n");
}

#[derive(Clone, Copy)]
struct TextSpan {
    start: usize,
    end: usize,
}

#[derive(Default)]
struct TextArena(String);

impl TextArena {
    fn push(&mut self, text: &str) -> TextSpan {
        let start = self.0.len();
        self.0.push_str(text);
        TextSpan {
            start,
            end: self.0.len(),
        }
    }

    fn get(&self, span: TextSpan) -> &str {
        &self.0[span.start..span.end]
    }
}

fn leaves(
    conn: &Connection,
    table: &str,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    match table {
        "nodes" => return domain_leaves::leaves_nodes(conn, owners),
        "edge_occurrences" => return domain_leaves::leaves_edge_occurrences(conn, owners),
        "edges" => return domain_leaves::leaves_edges(conn, owners),
        "shared_owners" => return domain_leaves::leaves_shared_owners(conn, owners),
        "dependencies" => return domain_leaves::leaves_dependencies(conn, owners),
        "resolution_coverage" => {
            if let Some(leaves) = coverage_leaves::canonical_coverage_leaves(conn, owners)? {
                return Ok(leaves);
            }
        }
        "coverage_owner_language" => {
            let leaves = coverage_leaves::canonical_owner_language_leaves(conn, owners)?
                .context("coverage_owner_language rows require canonical text")?;
            return Ok(leaves);
        }
        "coverage_language_counts" => {
            let leaves = coverage_leaves::canonical_language_count_leaves(conn, owners)?
                .context("coverage_language_counts rows require canonical text")?;
            return Ok(leaves);
        }
        _ => {}
    }
    domain_leaves::leaves_generic(conn, table, owners)
}

fn fts_digest(conn: &Connection, table: &str, order_by: &str) -> Result<String> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table} ORDER BY {order_by}"))?;
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
    let mut stmt=conn.prepare("SELECT domain,CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments WHERE owner_id=0 ORDER BY domain")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    let mut stmt=conn.prepare("SELECT key,value FROM metadata WHERE key IN('schema_version','index_semantics_version','indexer_engine','workspace_root','adapter','corpus_hash','commitment_algorithm','inventory_snapshot','manifest_digest')ORDER BY key")?;
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

fn leaf_owner<'a>(key: &'a str, prefix: &str) -> Result<std::borrow::Cow<'a, str>> {
    let encoded = key
        .strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(']'))
        .context("invalid commitment leaf key")?;
    Ok(match serde_json::from_str::<&str>(encoded) {
        Ok(owner) => std::borrow::Cow::Borrowed(owner),
        Err(_) => std::borrow::Cow::Owned(serde_json::from_str::<String>(encoded)?),
    })
}

fn batch_insert_commitments(
    conn: &Connection,
    table: &str,
    entries: &BTreeMap<String, String>,
) -> Result<()> {
    let chunk_size =
        (usize::try_from(conn.limit(rusqlite::limits::Limit::SQLITE_LIMIT_VARIABLE_NUMBER))? / 3)
            .min(250);
    anyhow::ensure!(
        chunk_size > 0,
        "SQLite variable limit cannot fit a commitment row"
    );
    let prefix = format!("leaf:[\"{table}\",");
    let mut paths: HashMap<String, i64> = conn
        .prepare_cached("SELECT path,path_id FROM path_dictionary")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let missing: BTreeSet<_> = entries
        .keys()
        .map(|key| leaf_owner(key, &prefix))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|owner| !paths.contains_key(owner.as_ref()))
        .collect();
    if !missing.is_empty() {
        let missing: Vec<_> = missing.into_iter().collect();
        for chunk in missing.chunks(chunk_size) {
            let sql = format!(
                "INSERT OR IGNORE INTO path_dictionary(path) VALUES{}",
                std::iter::repeat_n("(?)", chunk.len())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            conn.prepare_cached(&sql)?
                .execute(rusqlite::params_from_iter(
                    chunk.iter().map(|path| path.as_ref()),
                ))?;
        }
        paths = conn
            .prepare_cached("SELECT path,path_id FROM path_dictionary")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
    }
    let mut chunk = Vec::with_capacity(chunk_size);
    for (k, v) in entries {
        let path = leaf_owner(k, &prefix)?;
        let owner_id = *paths
            .get(path.as_ref())
            .context("missing commitment owner path")?;
        let mut digest = [0u8; 32];
        hex::decode_to_slice(v, &mut digest)?;
        chunk.push((owner_id, digest));
        if chunk.len() == chunk_size {
            insert_commitment_chunk(conn, table, &chunk)?;
            chunk.clear();
        }
    }
    if !chunk.is_empty() {
        insert_commitment_chunk(conn, table, &chunk)?;
    }
    Ok(())
}

fn insert_commitment_chunk(
    conn: &Connection,
    table: &str,
    chunk: &[(i64, [u8; 32])],
) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let mut sql = String::from("INSERT OR REPLACE INTO domain_commitments VALUES");
    for i in 0..chunk.len() {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str("(?,?,?)");
    }
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 3);
    for (k, v) in chunk {
        params_vec.push(&table);
        params_vec.push(k);
        params_vec.push(v);
    }
    stmt.execute(rusqlite::params_from_iter(params_vec))?;
    Ok(())
}
fn stored(conn: &Connection, table: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.path,CASE WHEN typeof(d.digest)='blob' THEN lower(hex(d.digest)) ELSE d.digest END FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.domain=?1 AND d.owner_id!=0",
    )?;
    let rows = stmt
        .query_map([table], |r| {
            let owner: String = r.get(0)?;
            Ok((key(table, &owner), r.get(1)?))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok(rows)
}
/// Recomputes every owner leaf and stores the deterministic output root.
///
/// # Errors
///
/// Fails if indexed rows or dictionary references are malformed, or SQLite fails.
pub fn seal(conn: &Connection) -> Result<String> {
    seal_owners(conn, None)
}

/// Reseals selected owner leaves and recomputes their domain and output roots.
///
/// `None` seals the complete index. The caller owns the SQLite transaction and
/// must keep the source rows stable until the seal has completed.
///
/// # Errors
///
/// Fails on malformed graph references, hashing errors, or SQLite failures.
pub fn seal_owners(conn: &Connection, owners: Option<&BTreeSet<String>>) -> Result<String> {
    conn.execute(
        "INSERT OR REPLACE INTO metadata VALUES('commitment_algorithm',?1)",
        [ALGORITHM],
    )?;
    let encoded_owners = owners.map(serde_json::to_string).transpose()?;
    for table in DOMAINS {
        let changed = leaves(conn, table, owners)?;
        if let Some(encoded) = &encoded_owners {
            conn.execute(
                "DELETE FROM domain_commitments WHERE domain=?1 AND owner_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?2)))",
                params![table, encoded],
            )?;
        } else {
            conn.execute(
                "DELETE FROM domain_commitments WHERE domain=?1 AND owner_id!=0",
                [table],
            )?;
        }
        batch_insert_commitments(conn, table, &changed)?;
        let digest = if owners.is_none() {
            aggregate(table, &changed)
        } else {
            aggregate(table, &stored(conn, table)?)
        };
        conn.prepare_cached("INSERT OR REPLACE INTO domain_commitments VALUES(?1,0,unhex(?2))")?
            .execute(params![table, digest])?;
    }
    for &(table, order_by) in FTS {
        conn.execute(
            "INSERT OR REPLACE INTO domain_commitments VALUES(?1,0,unhex(?2))",
            params![table, fts_digest(conn, table, order_by)?],
        )?;
    }
    let root = root(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata VALUES('output_root',?1)",
        [&root],
    )?;
    Ok(root)
}
/// Verifies every stored owner leaf, domain root, and output root.
///
/// # Errors
///
/// Returns a mismatch error when the committed index differs from its seal.
pub fn verify(conn: &Connection) -> Result<()> {
    verify_owners(conn, None)
}
/// Verifies selected owner leaves and all affected domain and output roots.
///
/// `None` verifies the complete index.
///
/// # Errors
///
/// Returns a mismatch error when the stored commitment differs from source rows.
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
            "SELECT CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments WHERE domain=?1 AND owner_id=0",
            [table],
            |r| r.get(0),
        )?;
        if aggregate(table, &all) != stored {
            bail!("domain root mismatch: {table}");
        }
    }
    for &(table, order_by) in FTS {
        let stored: String = conn.query_row(
            "SELECT CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments WHERE domain=?1 AND owner_id=0",
            [table],
            |r| r.get(0),
        )?;
        if fts_digest(conn, table, order_by)? != stored {
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
