use super::{
    encode_int, encode_null, encode_row, encode_text, key, new_leaf_hasher, shape, TextArena,
    TextSpan,
};
use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use rayon::prelude::*;
use rusqlite::{types::ValueRef, Connection};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn leaves_nodes(
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
        "SELECT id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id FROM nodes {filter} ORDER BY owner_path_id,id"
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

pub(super) fn leaves_edge_occurrences(
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

pub(super) fn leaves_edges(
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

pub(super) fn leaves_shared_owners(
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

pub(super) fn leaves_dependencies(
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

pub(super) fn leaves_generic(
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
