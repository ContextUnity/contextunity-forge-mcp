use super::{paging, reader};
use crate::core::models::stable_hash64;
use crate::core::response::QueryOptions;
use crate::db::traversal;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;

fn admit_test_mapping(conn: &Connection, id: &str, inbound: bool) -> Result<()> {
    let seeds = paging::count(
        conn,
        "SELECT count(*) FROM edges WHERE src_hash=?1 AND kind='contains'",
        &[&stable_hash64(id)],
    )?;
    let dependencies = traversal::immediate_links(conn, id, inbound, false)?;
    let immediate = seeds.saturating_add(dependencies);
    if immediate > 1000 {
        bail!("test mapping from {id} starts with {immediate} direct graph links before its unbounded dependency walk. Select a narrower module or symbol; reducing limit alone does not reduce traversal work");
    }
    Ok(())
}

/// Performs tests paged.
pub fn tests_paged(
    conn: &Connection,
    selector: &str,
    direction: &str,
    options: &QueryOptions,
) -> Result<Value> {
    tests_paged_with_path(conn, selector, None, direction, options)
}

pub(crate) fn tests_paged_with_path(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    direction: &str,
    options: &QueryOptions,
) -> Result<Value> {
    let (inbound, is_test) = match direction {
        "inbound" => (true, 1),
        "outbound" => (false, 0),
        _ => bail!("direction must be inbound or outbound"),
    };
    paging::generation(conn, options)?;
    let node = reader::select_detail_with_path(conn, selector, path, options.detail)?;
    let id = node["id"].as_str().context("invalid node id")?;
    let id_hash = stable_hash64(id);
    admit_test_mapping(conn, id, inbound)?;
    let path = node["path"].as_str().unwrap_or("");
    let start_line = node["line"].as_i64().unwrap_or(0);
    let end_line = node["end_line"].as_i64().unwrap_or(i64::MAX);

    let unresolved_count: usize = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND line BETWEEN ?2 AND ?3",
            rusqlite::params![path, start_line, end_line],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let steps = traversal::dependency_steps(inbound, true, false);
    let (forward_candidate, forward_selected, reverse_candidate, reverse_selected) = if inbound {
        ("src_hash", "dst_hash", "dst_hash", "src_hash")
    } else {
        ("dst_hash", "src_hash", "src_hash", "dst_hash")
    };
    let max_depth = 4;
    let sql = format!("WITH RECURSIVE seeds(id,depth) AS (SELECT ?1,0 UNION SELECT e.dst_hash,s.depth+1 FROM seeds s JOIN edges e ON e.src_hash=s.id WHERE e.kind='contains' AND s.depth<?2), walk(id,depth) AS (SELECT id,depth FROM seeds UNION {steps}), reached(id) AS (SELECT id FROM walk GROUP BY id) SELECT {} FROM reached w JOIN nodes n ON n.node_hash=w.id JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.is_test=?3 AND n.kind IN ('function','method','class','struct') AND n.node_hash!=?1 ORDER BY CASE WHEN EXISTS(SELECT 1 FROM edges e WHERE e.{forward_selected}=?1 AND e.{forward_candidate}=n.node_hash AND e.kind IN({})) OR EXISTS(SELECT 1 FROM edges e WHERE e.{reverse_selected}=?1 AND e.{reverse_candidate}=n.node_hash AND e.kind IN({})) THEN 0 ELSE 1 END,p.path,n.line,n.id", paging::nodes_with_path("n", options.detail, "p.path"), traversal::FORWARD_DEPENDENCIES, traversal::REVERSE_DEPENDENCIES);
    let mut nodes = paging::query(conn, &sql, &[&id_hash, &max_depth, &is_test], options)?;
    let mut discovery_method = "graph";
    if inbound && nodes["total"].as_u64() == Some(0) {
        let name = node["name"].as_str().unwrap_or_default();
        let mut words = Vec::new();
        let mut current = String::new();
        for ch in name.chars() {
            if ch.is_uppercase() && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            if ch.is_alphanumeric() {
                current.push(ch.to_ascii_lowercase());
            } else if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            words.push(current);
        }
        let snake = words.join("_");
        let lexical_query = if words.len() > 1 {
            format!(
                "\"{}\" OR ({})",
                name,
                words
                    .iter()
                    .map(|w| format!("\"{w}\""))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            )
        } else {
            format!("\"{name}\"")
        };
        if !lexical_query.is_empty() {
            nodes = paging::query(
                conn,
                &format!("SELECT {} FROM node_search JOIN nodes n ON n.node_id=node_search.rowid JOIN path_dictionary p ON p.path_id=n.path_id JOIN files f ON f.path=p.path WHERE node_search MATCH ?1 AND n.is_test=1 AND f.is_test=1 AND n.kind IN('function','method','class','struct') AND n.id!=?2 AND (n.name=?3 COLLATE NOCASE OR n.name=('test_'||?3) COLLATE NOCASE OR n.name LIKE ('%'||?4||'%') OR f.path LIKE ('%'||?4||'%')) ORDER BY bm25(node_search),p.path,n.line,n.id", paging::nodes_with_path("n", options.detail, "p.path")),
                &[&lexical_query, &id, &name, &snake],
                options,
            )?;
            if nodes["total"].as_u64().is_some_and(|total| total > 0) {
                discovery_method = "lexical_fallback";
            }
        }
    }
    annotate_test_connections(
        conn,
        &mut nodes,
        id,
        path,
        inbound,
        options.detail == crate::core::response::Detail::Compact,
    )?;
    let scope_note = if unresolved_count > 0 {
        format!("indexed static dependencies; {unresolved_count} unresolved reference(s) within symbol scope may conceal dynamic test callers")
    } else {
        "indexed static dependencies; no unresolved references in symbol scope".to_string()
    };
    Ok(
        json!({"selector":node,"direction":direction,"nodes":nodes,"discovery_method":discovery_method,"unresolved_references":unresolved_count,"scope":scope_note}),
    )
}

fn annotate_test_connections(
    conn: &Connection,
    page: &mut Value,
    selected_id: &str,
    selected_path: &str,
    inbound: bool,
    compact: bool,
) -> Result<()> {
    let items = page["items"]
        .as_array_mut()
        .context("missing test result items")?;
    if items.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = items
        .iter()
        .map(|item| {
            item["id"]
                .as_str()
                .context("invalid test result id")
                .map(str::to_owned)
        })
        .collect::<Result<_>>()?;
    let placeholders = (2..=ids.len() + 1)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let (forward_candidate, forward_selected, reverse_candidate, reverse_selected) = if inbound {
        ("src_hash", "dst_hash", "dst_hash", "src_hash")
    } else {
        ("dst_hash", "src_hash", "src_hash", "dst_hash")
    };
    let sql = format!(
        "SELECT e.{forward_candidate} related_hash,e.kind,p.path,e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.{forward_selected}=?1 AND e.{forward_candidate} IN ({placeholders}) AND e.kind IN({}) UNION ALL SELECT e.{reverse_candidate} related_hash,e.kind,p.path,e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.{reverse_selected}=?1 AND e.{reverse_candidate} IN ({placeholders}) AND e.kind IN({}) ORDER BY related_hash,kind,path,line",
        traversal::FORWARD_DEPENDENCIES,
        traversal::REVERSE_DEPENDENCIES,
    );
    let selected_hash = stable_hash64(selected_id);
    let candidate_hashes: Vec<i64> = ids.iter().map(|id| stable_hash64(id)).collect();
    let bindings: Vec<&dyn rusqlite::ToSql> =
        std::iter::once(&selected_hash as &dyn rusqlite::ToSql)
            .chain(
                candidate_hashes
                    .iter()
                    .map(|hash| hash as &dyn rusqlite::ToSql),
            )
            .collect();
    // At most seven dependency kinds are persisted per selected/candidate pair.
    let direct = reader::rows(conn, &sql, &bindings, ids.len() * 7)?;
    let mut reasons = HashMap::new();
    for edge in direct {
        let related_hash = edge["related_hash"]
            .as_i64()
            .context("invalid related node hash")?;
        reasons.entry(related_hash).or_insert_with(|| {
            json!({
                "relation": "direct",
                "edge_kind": edge["kind"],
                "path": edge["path"],
                "line": edge["line"],
            })
        });
    }
    for item in items {
        let id = item["id"].as_str().context("invalid test result id")?;
        let mut connection = reasons
            .remove(&stable_hash64(id))
            .unwrap_or_else(|| json!({"relation":"scope_or_transitive"}));
        if compact && connection["relation"] == "direct" {
            let evidence_path = connection["path"].as_str().unwrap_or("");
            if evidence_path == selected_path
                || evidence_path == item["path"].as_str().unwrap_or("")
            {
                connection
                    .as_object_mut()
                    .context("invalid direct connection")?
                    .remove("path");
            }
        }
        item["connection"] = connection;
    }
    Ok(())
}
