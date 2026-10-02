use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use rusqlite::{params, types::ValueRef, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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

fn leaves_nodes(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    let path_map: HashMap<i64, String> = conn
        .prepare_cached("SELECT path_id,path FROM path_dictionary")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let filter = if owners.is_some() {
        "WHERE owner_path_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let sql = format!(
        "SELECT id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id,path FROM nodes {filter} ORDER BY owner_path_id,id"
    );
    let mut statement = conn.prepare(&sql)?;
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let mut rows = if let Some(encoded) = &encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    let mut hashers: HashMap<i64, Sha256> = HashMap::new();
    while let Some(row) = rows.next()? {
        let owner_id: i64 = row.get(12)?;
        let owner = path_map.get(&owner_id).context("missing node owner path")?;
        let path_id: i64 = row.get(4)?;
        let path = path_map.get(&path_id).context("missing node path")?;
        if row.get_ref(13)?.as_str()? != path {
            bail!("node path differs from its path dictionary entry");
        }
        let hasher = hashers
            .entry(owner_id)
            .or_insert_with(|| new_leaf_hasher("nodes", owner));
        hasher.update([0xff]);
        for index in 0..4 {
            encode_text(hasher, row.get_ref(index)?.as_str()?);
        }
        encode_text(hasher, path);
        for index in 5..=7 {
            encode_int(hasher, row.get(index)?);
        }
        encode_text(hasher, row.get_ref(8)?.as_str()?);
        encode_int(hasher, row.get(9)?);
        encode_text(hasher, row.get_ref(10)?.as_str()?);
        encode_int(hasher, row.get(11)?);
    }
    Ok(hashers
        .into_iter()
        .map(|(owner_id, hasher)| {
            let owner = &path_map[&owner_id];
            (key("nodes", owner), hex::encode(hasher.finalize()))
        })
        .collect())
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

fn graph_evidence_dictionary(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut statement =
        conn.prepare_cached("SELECT evidence_id,evidence FROM coverage_evidence")?;
    let mut rows = statement.query([])?;
    let mut dictionary = HashMap::new();
    while let Some(row) = rows.next()? {
        if let ValueRef::Text(bytes) = row.get_ref(1)? {
            if let Ok(text) = std::str::from_utf8(bytes) {
                dictionary.insert(row.get(0)?, text.to_owned());
            }
        }
    }
    Ok(dictionary)
}

fn graph_evidence<'a>(
    value: ValueRef<'a>,
    dictionary: &'a HashMap<i64, String>,
) -> Result<&'a str> {
    match value {
        ValueRef::Integer(id) => dictionary
            .get(&id)
            .map(String::as_str)
            .with_context(|| format!("graph commitment references missing evidence {id}")),
        _ => Ok(value.as_str()?),
    }
}

fn leaves_edge_occurrences(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    let evidence_map = graph_evidence_dictionary(conn)?;
    let mut stmt_p = conn.prepare_cached("SELECT path_id, path FROM path_dictionary")?;
    let path_map: HashMap<i64, String> = stmt_p
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut stmt_n = conn.prepare_cached("SELECT node_hash, id FROM nodes")?;
    let node_map: HashMap<i64, String> = stmt_n
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let filter = if owners.is_some() {
        "WHERE owner_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let sql = format!(
        "SELECT owner_id, ordinal, src_hash, dst_hash, kind, line, evidence_id, confidence_id FROM edge_occurrences {filter} ORDER BY owner_id, ordinal"
    );
    let mut stmt = conn.prepare(&sql)?;
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let mut rows = if let Some(encoded) = &encoded {
        stmt.query([encoded])?
    } else {
        stmt.query([])?
    };

    let mut result = BTreeMap::new();
    let mut current_owner_id: Option<i64> = None;
    let mut current_hasher = Sha256::new();
    let mut current_owner_path = "";

    while let Some(row) = rows.next()? {
        let owner_id: i64 = row.get(0)?;
        let ordinal: i64 = row.get(1)?;
        let src_hash: i64 = row.get(2)?;
        let dst_hash: i64 = row.get(3)?;
        let kind = row.get_ref(4)?.as_str()?;
        let line: i64 = row.get(5)?;
        let evidence = graph_evidence(row.get_ref(6)?, &evidence_map)?;
        let confidence = graph_evidence(row.get_ref(7)?, &evidence_map)?;

        let owner_path = path_map
            .get(&owner_id)
            .with_context(|| {
                format!("edge occurrence commitment references missing owner path {owner_id}")
            })?
            .as_str();
        let src = node_map
            .get(&src_hash)
            .with_context(|| {
                format!("edge occurrence commitment references missing source node {src_hash}")
            })?
            .as_str();
        let dst = node_map
            .get(&dst_hash)
            .with_context(|| {
                format!("edge occurrence commitment references missing destination node {dst_hash}")
            })?
            .as_str();

        if current_owner_id != Some(owner_id) {
            if current_owner_id.is_some() {
                result.insert(
                    key("edge_occurrences", current_owner_path),
                    hex::encode(current_hasher.finalize()),
                );
            }
            current_hasher = new_leaf_hasher("edge_occurrences", owner_path);
            current_owner_path = owner_path;
            current_owner_id = Some(owner_id);
        }

        sha2::digest::Update::update(&mut current_hasher, &[0xff]);
        encode_text(&mut current_hasher, current_owner_path);
        encode_int(&mut current_hasher, ordinal);
        encode_text(&mut current_hasher, src);
        encode_text(&mut current_hasher, dst);
        encode_text(&mut current_hasher, kind);
        encode_text(&mut current_hasher, current_owner_path);
        encode_int(&mut current_hasher, line);
        encode_text(&mut current_hasher, evidence);
        encode_text(&mut current_hasher, confidence);
    }

    if current_owner_id.is_some() {
        result.insert(
            key("edge_occurrences", current_owner_path),
            hex::encode(current_hasher.finalize()),
        );
    }
    Ok(result)
}

fn leaves_edges(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    let evidence_map = graph_evidence_dictionary(conn)?;
    let mut stmt_p = conn.prepare_cached("SELECT path_id, path FROM path_dictionary")?;
    let path_map: HashMap<i64, String> = stmt_p
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut stmt_n = conn.prepare_cached("SELECT node_hash, id FROM nodes")?;
    let node_map: HashMap<i64, String> = stmt_n
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let filter = if owners.is_some() {
        "WHERE path_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let sql = format!(
        "SELECT src_hash, kind, dst_hash, path_id, line, evidence_id, confidence_id, occurrence_count FROM edges {filter}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let mut rows = if let Some(encoded) = &encoded {
        stmt.query([encoded])?
    } else {
        stmt.query([])?
    };

    struct EdgeRow<'a> {
        src: &'a str,
        dst: &'a str,
        kind: TextSpan,
        line: i64,
        evidence: TextSpan,
        confidence: TextSpan,
        count: i64,
    }

    let mut by_owner: HashMap<&str, Vec<EdgeRow<'_>>> = HashMap::new();
    let mut text = TextArena::default();

    while let Some(row) = rows.next()? {
        let src_hash: i64 = row.get(0)?;
        let kind = text.push(row.get_ref(1)?.as_str()?);
        let dst_hash: i64 = row.get(2)?;
        let path_id: i64 = row.get(3)?;
        let line: i64 = row.get(4)?;
        let evidence = text.push(graph_evidence(row.get_ref(5)?, &evidence_map)?);
        let confidence = text.push(graph_evidence(row.get_ref(6)?, &evidence_map)?);
        let count: i64 = row.get(7)?;

        let path = path_map
            .get(&path_id)
            .with_context(|| format!("edge commitment references missing owner path {path_id}"))?
            .as_str();
        let src = node_map
            .get(&src_hash)
            .with_context(|| format!("edge commitment references missing source node {src_hash}"))?
            .as_str();
        let dst = node_map
            .get(&dst_hash)
            .with_context(|| {
                format!("edge commitment references missing destination node {dst_hash}")
            })?
            .as_str();

        by_owner.entry(path).or_default().push(EdgeRow {
            src,
            dst,
            kind,
            line,
            evidence,
            confidence,
            count,
        });
    }

    let parallel = by_owner.values().map(Vec::len).sum::<usize>() >= 8192;
    let hash_owner = |(owner, mut edge_rows): (&str, Vec<EdgeRow<'_>>)| {
        edge_rows.sort_unstable_by(|a, b| {
            a.src
                .cmp(b.src)
                .then_with(|| a.dst.cmp(b.dst))
                .then_with(|| text.get(a.kind).cmp(text.get(b.kind)))
        });
        let mut hasher = new_leaf_hasher("edges", owner);
        for e in edge_rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, e.src);
            encode_text(&mut hasher, e.dst);
            encode_text(&mut hasher, text.get(e.kind));
            encode_text(&mut hasher, owner);
            encode_int(&mut hasher, e.line);
            encode_text(&mut hasher, text.get(e.evidence));
            encode_text(&mut hasher, text.get(e.confidence));
            encode_int(&mut hasher, e.count);
        }
        (key("edges", owner), hex::encode(hasher.finalize()))
    };
    let result = if parallel {
        by_owner.into_par_iter().map(hash_owner).collect()
    } else {
        by_owner.into_iter().map(hash_owner).collect()
    };
    Ok(result)
}

fn leaves_shared_owners(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    let mut stmt_p = conn.prepare_cached("SELECT path_id, path FROM path_dictionary")?;
    let path_map: HashMap<i64, String> = stmt_p
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let node_keys: HashMap<i64, (String, String)> = conn
        .prepare_cached("SELECT node_id,qualname,id FROM nodes")?
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt_k = conn.prepare_cached("SELECT key_hash,key FROM shared_keys")?;
    let mut key_rows = stmt_k.query([])?;
    let mut key_map = HashMap::new();
    while let Some(row) = key_rows.next()? {
        let hash: i64 = row.get(0)?;
        let key = match row.get_ref(1)? {
            ValueRef::Integer(node_id) => node_keys
                .get(&node_id)
                .context("missing node for shared key")?
                .0
                .clone(),
            ValueRef::Text(key) => std::str::from_utf8(key)?.to_owned(),
            _ => bail!("invalid shared key storage"),
        };
        key_map.insert(hash, key);
    }

    let filter = if owners.is_some() {
        "WHERE owner_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let sql = format!("SELECT kind_id, key_hash, owner_id, ordinal FROM shared_owners {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let mut rows = if let Some(encoded) = &encoded {
        stmt.query([encoded])?
    } else {
        stmt.query([])?
    };

    struct SharedRow<'a> {
        kind: &'static str,
        key: &'a str,
        ordinal: i64,
        node_id: Option<&'a str>,
    }

    let mut by_owner: HashMap<&str, Vec<SharedRow<'_>>> = HashMap::new();

    while let Some(row) = rows.next()? {
        let kind_id: i64 = row.get(0)?;
        let key_hash: i64 = row.get(1)?;
        let owner_id: i64 = row.get(2)?;
        let ordinal: i64 = row.get(3)?;
        let owner = path_map
            .get(&owner_id)
            .with_context(|| {
                format!("shared-owner commitment references missing owner path {owner_id}")
            })?
            .as_str();
        let key = key_map
            .get(&key_hash)
            .with_context(|| format!("shared-owner commitment references missing key {key_hash}"))?
            .as_str();
        let kind = match kind_id {
            0 => "symbol",
            1 => "reference",
            2 => "default_export",
            _ => bail!("shared-owner commitment contains invalid kind {kind_id}"),
        };
        let node_id = if kind_id == 0 {
            Some(
                node_keys
                    .get(&ordinal)
                    .context("shared symbol owner references missing node")?
                    .1
                    .as_str(),
            )
        } else {
            None
        };
        by_owner.entry(owner).or_default().push(SharedRow {
            kind,
            key,
            ordinal,
            node_id,
        });
    }

    let mut result = BTreeMap::new();
    for (owner, mut shared_rows) in by_owner {
        shared_rows.sort_unstable_by(|a, b| {
            a.kind
                .cmp(b.kind)
                .then_with(|| a.key.cmp(b.key))
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.ordinal.cmp(&b.ordinal))
        });
        let mut hasher = new_leaf_hasher("shared_owners", owner);
        for s in shared_rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, s.kind);
            encode_text(&mut hasher, s.key);
            encode_text(&mut hasher, owner);
            if let Some(node_id) = s.node_id {
                encode_text(&mut hasher, node_id);
            } else {
                encode_int(&mut hasher, s.ordinal);
            }
        }
        result.insert(key("shared_owners", owner), hex::encode(hasher.finalize()));
    }
    Ok(result)
}

fn leaves_dependencies(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    let mut stmt_p = conn.prepare_cached("SELECT path_id, path FROM path_dictionary")?;
    let path_map: HashMap<i64, String> = stmt_p
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut stmt_f = conn.prepare_cached("SELECT path_hash, path FROM files")?;
    let file_map: HashMap<i64, String> = stmt_f
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let filter = if owners.is_some() {
        "WHERE owner_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let sql = format!(
        "SELECT owner_id, ordinal, kind, symbol, resolution, target_hash FROM dependencies {filter}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let mut rows = if let Some(encoded) = &encoded {
        stmt.query([encoded])?
    } else {
        stmt.query([])?
    };

    struct DepRow<'a> {
        target: Option<&'a str>,
        kind: TextSpan,
        symbol: TextSpan,
        resolution: TextSpan,
    }

    let mut by_owner: HashMap<&str, Vec<DepRow<'_>>> = HashMap::new();
    let mut text = TextArena::default();

    while let Some(row) = rows.next()? {
        let owner_id: i64 = row.get(0)?;
        let _ordinal: i64 = row.get(1)?;
        let kind = text.push(row.get_ref(2)?.as_str()?);
        let symbol = text.push(row.get_ref(3)?.as_str()?);
        let resolution = text.push(row.get_ref(4)?.as_str()?);
        let target_hash: Option<i64> = row.get(5)?;
        let owner = path_map
            .get(&owner_id)
            .with_context(|| {
                format!("dependency commitment references missing owner path {owner_id}")
            })?
            .as_str();
        let target = target_hash.and_then(|h| file_map.get(&h).map(|s| s.as_str()));
        by_owner.entry(owner).or_default().push(DepRow {
            target,
            kind,
            symbol,
            resolution,
        });
    }

    let parallel = by_owner.values().map(Vec::len).sum::<usize>() >= 8192;
    let hash_owner = |(owner, mut dep_rows): (&str, Vec<DepRow<'_>>)| {
        dep_rows.sort_unstable_by(|a, b| {
            a.target
                .cmp(&b.target)
                .then_with(|| text.get(a.kind).cmp(text.get(b.kind)))
                .then_with(|| text.get(a.symbol).cmp(text.get(b.symbol)))
                .then_with(|| text.get(a.resolution).cmp(text.get(b.resolution)))
        });
        let mut hasher = new_leaf_hasher("dependencies", owner);
        for d in dep_rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, owner);
            if let Some(target) = d.target {
                encode_text(&mut hasher, target);
            } else {
                encode_null(&mut hasher);
            }
            encode_text(&mut hasher, text.get(d.kind));
            encode_text(&mut hasher, text.get(d.symbol));
            encode_text(&mut hasher, text.get(d.resolution));
        }
        (key("dependencies", owner), hex::encode(hasher.finalize()))
    };
    let result = if parallel {
        by_owner.into_par_iter().map(hash_owner).collect()
    } else {
        by_owner.into_iter().map(hash_owner).collect()
    };
    Ok(result)
}

fn leaves(
    conn: &Connection,
    table: &str,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    match table {
        "nodes" => return leaves_nodes(conn, owners),
        "edge_occurrences" => return leaves_edge_occurrences(conn, owners),
        "edges" => return leaves_edges(conn, owners),
        "shared_owners" => return leaves_shared_owners(conn, owners),
        "dependencies" => return leaves_dependencies(conn, owners),
        "resolution_coverage" => {
            if let Some(leaves) = canonical_coverage_leaves(conn, owners)? {
                return Ok(leaves);
            }
        }
        _ => {}
    }
    leaves_generic(conn, table, owners)
}

fn coverage_dictionary(
    conn: &Connection,
    sql: &str,
    encoded: Option<&str>,
) -> Result<Option<HashMap<i64, String>>> {
    let mut statement = conn.prepare_cached(sql)?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    let mut dictionary = HashMap::new();
    while let Some(row) = rows.next()? {
        let (ValueRef::Integer(id), ValueRef::Text(text)) = (row.get_ref(0)?, row.get_ref(1)?)
        else {
            return Ok(None);
        };
        let Ok(text) = std::str::from_utf8(text) else {
            return Ok(None);
        };
        dictionary.insert(id, text.to_owned());
    }
    Ok(Some(dictionary))
}

fn canonical_coverage_leaves(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<Option<BTreeMap<String, String>>> {
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let encoded = encoded.as_deref();
    let filter = if owners.is_some() {
        "WHERE path_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let path_sql = if owners.is_some() {
        "SELECT path_id,path FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1))"
    } else {
        "SELECT path_id,path FROM path_dictionary"
    };
    let expression_sql = if owners.is_some() {
        format!("SELECT expression_id,expression FROM coverage_expressions WHERE expression_id IN(SELECT expression_id FROM resolution_coverage {filter})")
    } else {
        "SELECT expression_id,expression FROM coverage_expressions".to_owned()
    };
    let evidence_sql = if owners.is_some() {
        format!("SELECT evidence_id,evidence FROM coverage_evidence WHERE evidence_id IN(SELECT evidence_id FROM resolution_coverage {filter})")
    } else {
        "SELECT evidence_id,evidence FROM coverage_evidence".to_owned()
    };
    let Some(paths) = coverage_dictionary(conn, path_sql, encoded)? else {
        return Ok(None);
    };
    let Some(expressions) = coverage_dictionary(conn, &expression_sql, encoded)? else {
        return Ok(None);
    };
    let Some(evidence) = coverage_dictionary(conn, &evidence_sql, encoded)? else {
        return Ok(None);
    };
    let mut statuses = HashSet::new();
    {
        let mut statement = conn.prepare_cached(&format!(
            "SELECT DISTINCT status FROM resolution_coverage {filter}"
        ))?;
        let mut rows = if let Some(encoded) = encoded {
            statement.query([encoded])?
        } else {
            statement.query([])?
        };
        while let Some(row) = rows.next()? {
            let ValueRef::Text(status) = row.get_ref(0)? else {
                return Ok(None);
            };
            let Ok(status) = std::str::from_utf8(status) else {
                return Ok(None);
            };
            statuses.insert(status.to_owned());
        }
    }
    let mut statement = conn.prepare(&format!(
        "SELECT path_id,line,expression_id,status,evidence_id FROM resolution_coverage {filter}"
    ))?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    struct CoverageRow<'a> {
        line: i64,
        expression: &'a str,
        status: &'a str,
        evidence: &'a str,
    }
    let mut by_owner: HashMap<&str, Vec<CoverageRow<'_>>> = HashMap::new();
    let mut accepted = 0;
    while let Some(row) = rows.next()? {
        let (
            ValueRef::Integer(path),
            ValueRef::Integer(line),
            ValueRef::Integer(expression),
            ValueRef::Text(status),
            ValueRef::Integer(proof),
        ) = (
            row.get_ref(0)?,
            row.get_ref(1)?,
            row.get_ref(2)?,
            row.get_ref(3)?,
            row.get_ref(4)?,
        )
        else {
            return Ok(None);
        };
        let (Some(owner), Some(expression), Some(proof)) = (
            paths.get(&path),
            expressions.get(&expression),
            evidence.get(&proof),
        ) else {
            continue;
        };
        let Ok(status) = std::str::from_utf8(status) else {
            return Ok(None);
        };
        let Some(status) = statuses.get(status) else {
            return Ok(None);
        };
        by_owner
            .entry(owner.as_str())
            .or_default()
            .push(CoverageRow {
                line,
                expression: expression.as_str(),
                status: status.as_str(),
                evidence: proof.as_str(),
            });
        accepted += 1;
    }
    let hash_owner = |(owner, mut rows): (&str, Vec<CoverageRow<'_>>)| {
        rows.sort_unstable_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.expression.as_bytes().cmp(right.expression.as_bytes()))
                .then_with(|| left.status.as_bytes().cmp(right.status.as_bytes()))
                .then_with(|| left.evidence.as_bytes().cmp(right.evidence.as_bytes()))
        });
        let mut hasher = new_leaf_hasher("resolution_coverage", owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, owner);
            encode_int(&mut hasher, row.line);
            encode_text(&mut hasher, row.expression);
            encode_text(&mut hasher, row.status);
            encode_text(&mut hasher, row.evidence);
        }
        (
            key("resolution_coverage", owner),
            hex::encode(hasher.finalize()),
        )
    };
    let leaves = if accepted >= 8192 {
        by_owner.into_par_iter().map(hash_owner).collect()
    } else {
        by_owner.into_iter().map(hash_owner).collect()
    };
    Ok(Some(leaves))
}

fn leaves_generic(
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
    let mut current_owner: Option<String> = None;
    let mut current_hasher = Sha256::new();

    while let Some(row) = rows.next()? {
        let owner = row.get_ref(columns)?.as_str()?;
        if current_owner.as_deref() != Some(owner) {
            if let Some(old_owner) = current_owner.take() {
                result.insert(
                    key(table, &old_owner),
                    hex::encode(current_hasher.finalize()),
                );
            }
            current_hasher = new_leaf_hasher(table, owner);
            current_owner = Some(owner.to_owned());
        }
        encode_row(&mut current_hasher, row, columns)?;
    }
    if let Some(old_owner) = current_owner {
        result.insert(
            key(table, &old_owner),
            hex::encode(current_hasher.finalize()),
        );
    }
    Ok(result)
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

pub(crate) struct SnapshotCommitments {
    domains: Vec<DomainCommitment>,
}

/// Hash committed candidate rows while its logical domains remain immutable.
pub(crate) fn hash_snapshot(path: &str) -> Result<SnapshotCommitments> {
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

pub(crate) fn install_snapshot(conn: &Connection, snapshot: SnapshotCommitments) -> Result<String> {
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
