use super::{paging, reader};
use crate::core::models::stable_hash64;
use crate::core::response::QueryOptions;
use crate::db::traversal;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const MAX_CALL_GRAPH_NODES: usize = 20_000;
const MAX_CALL_GRAPH_EDGES_PER_FRONTIER: usize = 20_000;
const MAX_CALL_GRAPH_DEPTH: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct NodeKey {
    path: String,
    line: i64,
    id: String,
}
#[derive(Clone, Debug)]
struct CallNode {
    hash: i64,
    id: String,
    path: String,
    line: i64,
    is_test: bool,
    test_role: Option<String>,
}

impl CallNode {
    fn key(&self) -> NodeKey {
        NodeKey {
            path: self.path.clone(),
            line: self.line,
            id: self.id.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct CallPath {
    hashes: Vec<i64>,
    nodes: Vec<NodeKey>,
    edges: Vec<(String, i64)>,
}

impl CallPath {
    fn precedes(&self, other: &Self) -> bool {
        (&self.nodes, &self.edges) < (&other.nodes, &other.edges)
    }
}

fn call_node(row: &Value) -> Result<CallNode> {
    Ok(CallNode {
        hash: row["hash"]
            .as_i64()
            .context("invalid call graph node hash")?,
        id: row["id"]
            .as_str()
            .context("invalid call graph node id")?
            .to_owned(),
        path: row["path"]
            .as_str()
            .context("invalid call graph node path")?
            .to_owned(),
        line: row["line"]
            .as_i64()
            .context("invalid call graph node line")?,
        is_test: row["is_test"].as_i64() == Some(1),
        test_role: row["test_role"].as_str().map(str::to_owned),
    })
}

fn selected_call_node(conn: &Connection, hash: i64) -> Result<CallNode> {
    let rows = reader::rows(
        conn,
        "SELECT n.node_hash AS hash,n.id,p.path,n.line,n.is_test,json_extract(n.details,'$.test_role') AS test_role FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.node_hash=?1",
        &[&hash],
        1,
    )?;
    call_node(
        rows.first()
            .context("selected symbol disappeared from the index")?,
    )
}

fn call_paths(
    conn: &Connection,
    selected_hash: i64,
    inbound: bool,
) -> Result<(HashMap<i64, CallNode>, HashMap<i64, CallPath>)> {
    let selected = selected_call_node(conn, selected_hash)?;
    let root_path = CallPath {
        hashes: vec![selected.hash],
        nodes: vec![selected.key()],
        edges: Vec::new(),
    };
    let mut nodes = HashMap::from([(selected.hash, selected)]);
    let mut paths = HashMap::from([(selected_hash, root_path)]);
    let mut visited = HashSet::from([selected_hash]);
    let mut frontier = vec![selected_hash];

    for depth in 0..MAX_CALL_GRAPH_DEPTH {
        if frontier.is_empty() {
            return Ok((nodes, paths));
        }
        let frontier_json = serde_json::to_string(&frontier)?;
        let (next_hash, current_hash) = if inbound {
            ("e.src_hash", "e.dst_hash")
        } else {
            ("e.dst_hash", "e.src_hash")
        };
        let sql = format!(
            "SELECT {next_hash} AS hash,{current_hash} AS parent_hash,min(e.line) AS edge_line,ep.path AS edge_path,n.id,p.path,n.line,n.is_test,json_extract(n.details,'$.test_role') AS test_role FROM edges e JOIN json_each(?1) frontier ON {current_hash}=CAST(frontier.value AS INTEGER) JOIN nodes n ON n.node_hash={next_hash} JOIN path_dictionary p ON p.path_id=n.path_id JOIN path_dictionary ep ON ep.path_id=e.path_id WHERE e.kind='calls' GROUP BY {next_hash},{current_hash},ep.path ORDER BY p.path,n.line,n.id,edge_line,edge_path LIMIT {}",
            MAX_CALL_GRAPH_EDGES_PER_FRONTIER + 1
        );
        let expanded = reader::rows(
            conn,
            &sql,
            &[&frontier_json],
            MAX_CALL_GRAPH_EDGES_PER_FRONTIER + 1,
        )?;
        if expanded.len() > MAX_CALL_GRAPH_EDGES_PER_FRONTIER {
            bail!("verified test call graph exceeds the per-frontier budget; narrow the selected symbol or path");
        }
        let mut next_paths: HashMap<i64, (CallNode, CallPath)> = HashMap::new();
        for row in expanded {
            let candidate_node = call_node(&row)?;
            if visited.contains(&candidate_node.hash) {
                continue;
            }
            let parent_hash = row["parent_hash"]
                .as_i64()
                .context("invalid call graph parent hash")?;
            let parent_path = paths
                .get(&parent_hash)
                .context("call graph frontier lost its path")?;
            let edge = (
                row["edge_path"]
                    .as_str()
                    .context("invalid call edge path")?
                    .to_owned(),
                row["edge_line"]
                    .as_i64()
                    .context("invalid call edge line")?,
            );
            let candidate = if inbound {
                let mut hashes = Vec::with_capacity(parent_path.hashes.len() + 1);
                hashes.push(candidate_node.hash);
                hashes.extend(parent_path.hashes.iter().copied());
                let mut node_keys = Vec::with_capacity(parent_path.nodes.len() + 1);
                node_keys.push(candidate_node.key());
                node_keys.extend(parent_path.nodes.iter().cloned());
                let mut edges = Vec::with_capacity(parent_path.edges.len() + 1);
                edges.push(edge);
                edges.extend(parent_path.edges.iter().cloned());
                CallPath {
                    hashes,
                    nodes: node_keys,
                    edges,
                }
            } else {
                let mut hashes = parent_path.hashes.clone();
                hashes.push(candidate_node.hash);
                let mut node_keys = parent_path.nodes.clone();
                node_keys.push(candidate_node.key());
                let mut edges = parent_path.edges.clone();
                edges.push(edge);
                CallPath {
                    hashes,
                    nodes: node_keys,
                    edges,
                }
            };
            let entry = next_paths.entry(candidate_node.hash);
            match entry {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert((candidate_node, candidate));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if candidate.precedes(&entry.get().1) {
                        entry.insert((candidate_node, candidate));
                    }
                }
            }
        }
        if paths.len().saturating_add(next_paths.len()) > MAX_CALL_GRAPH_NODES {
            bail!("verified test call graph exceeds the node budget; narrow the selected symbol or path");
        }
        frontier = next_paths.keys().copied().collect();
        frontier.sort_by_key(|hash| {
            next_paths
                .get(hash)
                .map(|(node, _)| node.key())
                .unwrap_or_else(|| NodeKey {
                    path: String::new(),
                    line: 0,
                    id: String::new(),
                })
        });
        for (hash, (node, path)) in next_paths {
            visited.insert(hash);
            nodes.insert(hash, node);
            paths.insert(hash, path);
        }
        if depth + 1 == MAX_CALL_GRAPH_DEPTH && !frontier.is_empty() {
            bail!("verified test call graph exceeds the depth budget; narrow the selected symbol or path");
        }
    }
    Ok((nodes, paths))
}

fn page_call_nodes(
    conn: &Connection,
    candidates: &[(i64, CallPath)],
    options: &QueryOptions,
    generation: &str,
    selected_path: &str,
    compact: bool,
) -> Result<Value> {
    let total = candidates.len();
    let page = candidates
        .iter()
        .skip(options.offset)
        .take(options.limit)
        .collect::<Vec<_>>();
    let hashes = page.iter().map(|(hash, _)| *hash).collect::<Vec<_>>();
    let serialized = serde_json::to_string(&hashes)?;
    let columns = paging::nodes_with_path("n", options.detail, "p.path");
    let sql = format!(
        "SELECT {columns},json_extract(n.details,'$.test_role') AS test_role FROM json_each(?1) requested JOIN nodes n ON n.node_hash=CAST(requested.value AS INTEGER) JOIN path_dictionary p ON p.path_id=n.path_id ORDER BY CAST(requested.key AS INTEGER)"
    );
    let mut items = reader::rows(conn, &sql, &[&serialized], options.limit)?;
    for (item, (_hash, path)) in items.iter_mut().zip(page) {
        let call_path = path
            .nodes
            .iter()
            .map(|key| key.id.clone())
            .collect::<Vec<_>>();
        let direct = path.hashes.len() == 2;
        let mut connection = json!({
            "relation": if direct {"direct"} else {"transitive"},
            "call_path": call_path
        });
        if direct {
            connection["edge_kind"] = json!("calls");
            if let Some((edge_path, _)) = path.edges.first() {
                connection["path"] = json!(edge_path);
                if compact
                    && (edge_path == selected_path
                        || edge_path == item["path"].as_str().unwrap_or_default())
                {
                    connection
                        .as_object_mut()
                        .context("invalid direct call connection")?
                        .remove("path");
                }
            }
        }
        item["connection"] = connection;
    }
    Ok(paging::value(items, total, options, generation))
}

fn admit_test_mapping(conn: &Connection, id: &str, inbound: bool) -> Result<()> {
    let seeds = paging::count(
        conn,
        "SELECT count(*) FROM edges WHERE src_hash=?1 AND kind='contains'",
        &[&stable_hash64(id)],
    )?;
    let dependencies = traversal::immediate_links(conn, id, inbound, false)?;
    let immediate = seeds.saturating_add(dependencies);
    if immediate > 1000 {
        bail!("test mapping from {id} starts with {immediate} direct graph links before its bounded exact-call graph walk. Select a narrower module or symbol; reducing limit alone does not reduce traversal work");
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
    let inbound = match direction {
        "inbound" => true,
        "outbound" => false,
        _ => bail!("direction must be inbound or outbound"),
    };
    let generation = paging::generation(conn, options)?;
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

    let (available_nodes, paths) = call_paths(conn, id_hash, inbound)?;
    let mut candidates = paths
        .into_iter()
        .filter_map(|(hash, call_path)| {
            let node = available_nodes.get(&hash)?;
            let include = if inbound {
                node.is_test && node.test_role.as_deref() == Some("test_case")
            } else {
                !node.is_test
            };
            (include && hash != id_hash).then_some((hash, call_path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(left_hash, left), (right_hash, right)| {
        (left.hashes.len(), &left.nodes, &left.edges, left_hash).cmp(&(
            right.hashes.len(),
            &right.nodes,
            &right.edges,
            right_hash,
        ))
    });
    let mut nodes = page_call_nodes(
        conn,
        &candidates,
        options,
        &generation,
        path,
        options.detail == crate::core::response::Detail::Compact,
    )?;
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
                &format!("SELECT {},json_extract(n.details,'$.test_role') AS test_role FROM node_search JOIN nodes n ON n.node_hash=node_search.rowid JOIN path_dictionary p ON p.path_id=n.path_id JOIN files f ON f.path=p.path WHERE node_search MATCH ?1 AND n.is_test=1 AND json_extract(n.details,'$.test_role')='test_case' AND f.is_test=1 AND n.kind IN('function','method','class','struct') AND n.id!=?2 AND (n.name=?3 COLLATE NOCASE OR n.name=('test_'||?3) COLLATE NOCASE OR n.name LIKE ('%'||?4||'%') OR f.path LIKE ('%'||?4||'%')) ORDER BY bm25(node_search),p.path,n.line,n.id", paging::nodes_with_path("n", options.detail, "p.path")),
                &[&lexical_query, &id, &name, &snake],
                options,
            )?;
            if nodes["total"].as_u64().is_some_and(|total| total > 0) {
                discovery_method = "lexical_fallback";
                if let Some(items) = nodes["items"].as_array_mut() {
                    for item in items {
                        item["connection"] = json!({"relation":"lexical_fallback"});
                    }
                }
            }
        }
    }
    let scope_note = if unresolved_count > 0 {
        format!("indexed static dependencies; {unresolved_count} unresolved reference(s) within symbol scope may conceal dynamic test callers")
    } else {
        "indexed static dependencies; no unresolved references in symbol scope".to_string()
    };
    Ok(
        json!({"selector":node,"direction":direction,"nodes":nodes,"discovery_method":discovery_method,"unresolved_references":unresolved_count,"scope":scope_note}),
    )
}
