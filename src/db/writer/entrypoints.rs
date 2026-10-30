//! Persist effective entrypoint categories after graph resolution.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use super::{CompactGraph, TypedFacts};

/// Allocate the target set before graph changes so removed occurrences remain visible.
pub(super) fn prepare_candidates(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TEMP TABLE entrypoint_candidates(node_hash INTEGER PRIMARY KEY) WITHOUT ROWID;",
    )?;
    Ok(())
}

/// Add targets of changed relationships and nodes replaced by a delta.
pub(super) fn add_delta_candidates(
    conn: &Connection,
    replace_files: &BTreeSet<String>,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT dst_hash FROM changed_edge_keys WHERE kind IN ('handles','routes_to','dispatches','listens_to')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT n.node_hash FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id WHERE p.path IN (SELECT value FROM json_each(?1))",
        [serde_json::to_string(replace_files)?],
    )?;
    Ok(())
}

pub(super) fn rank(category: &str) -> Option<i64> {
    match category {
        "test" => Some(0),
        "main" => Some(1),
        "http_route" => Some(2),
        "mcp_tool" => Some(3),
        "cli_command" => Some(4),
        "event_handler" => Some(5),
        _ => None,
    }
}

pub(super) fn category(rank: i64) -> Option<&'static str> {
    ["test", "main", "http_route", "mcp_tool", "cli_command", "event_handler"]
        .get(rank as usize)
        .copied()
}

/// Resolve graph categories before cold node insertion, using exact occurrences.
pub(super) fn cold_ranks<'a>(
    graph: &'a CompactGraph,
    facts: &'a BTreeMap<String, TypedFacts>,
) -> hashbrown::HashMap<&'a str, Option<i64>> {
    let sources: hashbrown::HashMap<&str, &str> = facts.values().flat_map(|file| {
        file.nodes.iter().filter_map(|node| match node.kind.as_str() {
            "main_registration" | "route" | "tool_registration" |
            "cli_command_registration" | "event_handler_registration" => Some((node.id.as_str(), node.kind.as_str())),
            _ => None,
        })
    }).collect();
    let mut ranks: hashbrown::HashMap<&str, Option<i64>> = hashbrown::HashMap::new();
    for edge in &graph.edges {
        if !matches!(edge.kind.as_str(), "handles" | "routes_to" | "dispatches" | "listens_to") {
            continue;
        }
        let current = ranks.entry(edge.dst.as_str()).or_insert(None);
        if edge.confidence.as_str() != "exact" {
            continue;
        }
        let rank = match edge.kind.as_str() {
            "routes_to" => Some(2),
            "dispatches" | "listens_to" => Some(5),
            "handles" => sources.get(edge.src.as_str()).and_then(|kind| match *kind {
                "main_registration" => Some(1),
                "route" => Some(2),
                "tool_registration" => Some(3),
                "cli_command_registration" => Some(4),
                "event_handler_registration" => Some(5),
                _ => None,
            }),
            _ => None,
        };
        if let Some(rank) = rank {
            *current = Some(current.map_or(rank, |existing| existing.min(rank)));
        }
    }
    ranks
}

/// Recompute effective categories from intrinsic facts and all current exact graph occurrences.
///
/// Returns only node owners whose serialized navigation details actually changed.
pub(super) fn project(conn: &Connection) -> Result<BTreeSet<String>> {
    conn.execute_batch(
        "CREATE TEMP TABLE entrypoint_graph_rank(node_hash INTEGER PRIMARY KEY, rank INTEGER NOT NULL) WITHOUT ROWID;",
    )?;
    let kind_rank = "CASE occurrence.kind WHEN 'routes_to' THEN 2 WHEN 'dispatches' THEN 5 WHEN 'listens_to' THEN 5 ELSE CASE source.kind WHEN 'main_registration' THEN 1 WHEN 'route' THEN 2 WHEN 'tool_registration' THEN 3 WHEN 'cli_command_registration' THEN 4 WHEN 'event_handler_registration' THEN 5 END END";
    let graph_filter = "(occurrence.kind IN ('routes_to','dispatches','listens_to') OR (occurrence.kind='handles' AND source.kind IN ('main_registration','route','tool_registration','cli_command_registration','event_handler_registration')))";
    let edge_rank = kind_rank.replace("occurrence.kind", "edge.kind");
    let edge_filter = graph_filter.replace("occurrence.kind", "edge.kind");
    let statement = format!("INSERT INTO entrypoint_graph_rank SELECT edge.dst_hash,min({edge_rank}) FROM entrypoint_candidates candidate JOIN edges edge ON edge.dst_hash=candidate.node_hash AND edge.kind IN ('handles','routes_to','dispatches','listens_to') JOIN nodes source ON source.node_hash=edge.src_hash WHERE {edge_filter} AND EXISTS (SELECT 1 FROM edge_occurrences occurrence JOIN coverage_evidence confidence ON confidence.evidence_id=occurrence.confidence_id WHERE occurrence.src_hash=edge.src_hash AND occurrence.dst_hash=edge.dst_hash AND occurrence.kind=edge.kind AND confidence.evidence='exact') GROUP BY edge.dst_hash");
    conn.execute(&statement, [])?;
    let mut statement = conn.prepare(
        "SELECT n.node_hash,n.details,p.path,graph.rank,n.id
         FROM entrypoint_candidates candidate
         JOIN nodes n ON n.node_hash=candidate.node_hash
         JOIN path_dictionary p ON p.path_id=n.owner_path_id
         LEFT JOIN entrypoint_graph_rank graph ON graph.node_hash=n.node_hash
         ORDER BY n.node_hash",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut owners = BTreeSet::new();
    let mut intrinsic_by_owner = BTreeMap::new();
    let mut update = conn.prepare_cached("UPDATE nodes SET details=?1 WHERE node_hash=?2")?;
    for (hash, original, owner, graph_rank, id) in rows {
        let details: Value = serde_json::from_str(&original)?;
        let intrinsic = {
            if !intrinsic_by_owner.contains_key(&owner) {
                let blob: Vec<u8> = conn.query_row(
                    "SELECT facts_blob FROM local_facts WHERE path=?1",
                    [&owner],
                    |row| row.get(0),
                )?;
                let facts = crate::core::typed_facts::decode_facts(&blob)?;
                let ranks: hashbrown::HashMap<String, i64> = facts.facts.nodes.into_iter().filter_map(|node| {
                    node.details.get("entrypoint_base").and_then(Value::as_str).and_then(rank)
                        .map(|value| (node.id, value))
                }).collect();
                intrinsic_by_owner.insert(owner.clone(), ranks);
            }
            intrinsic_by_owner.get(&owner).and_then(|ranks| ranks.get(&id)).copied()
        };
        let category = intrinsic.into_iter().chain(graph_rank).min().and_then(|r| {
            [
                "test",
                "main",
                "http_route",
                "mcp_tool",
                "cli_command",
                "event_handler",
            ]
            .get(r as usize)
            .copied()
        });
        if details.get("entrypoint").and_then(Value::as_str) == category {
            continue;
        }
        // Preserve the original serialized key order: commitments hash exact bytes,
        // so reserializing the whole Value would make removal differ from a cold build.
        let encoded: String = if let Some(category) = category {
            if details.is_object() {
                conn.query_row(
                    "SELECT json_set(?1,'$.entrypoint',?2)",
                    params![original, category],
                    |row| row.get(0),
                )?
            } else {
                conn.query_row(
                    "SELECT json_set('{}','$.entrypoint_original_json',?1,'$.entrypoint',?2)",
                    params![original, category],
                    |row| row.get(0),
                )?
            }
        } else if let Some(previous) = details.as_object().and_then(|object| {
            (object.len() == 2)
                .then(|| {
                    object
                        .get("entrypoint_original_json")
                        .and_then(Value::as_str)
                })
                .flatten()
        }) {
            previous.to_owned()
        } else {
            conn.query_row(
                "SELECT json_remove(?1,'$.entrypoint')",
                [&original],
                |row| row.get(0),
            )?
        };
        if encoded != original {
            update.execute(params![encoded, hash])?;
            owners.insert(owner);
        }
    }
    Ok(owners)
}

