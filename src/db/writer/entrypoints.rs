//! Persist effective entrypoint categories after graph resolution.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeSet;

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
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT dst_hash FROM changed_edge_keys WHERE kind='handles'",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT n.node_hash FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id WHERE p.path IN (SELECT value FROM json_each(?1))",
        [serde_json::to_string(replace_files)?],
    )?;
    Ok(())
}

/// Populate all possible cold-build targets, including intrinsic entries.
pub(super) fn add_cold_candidates(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT node_hash FROM nodes WHERE json_extract(details,'$.entrypoint_base') IS NOT NULL",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT dst_hash FROM edge_occurrences WHERE kind='handles'",
        [],
    )?;
    Ok(())
}

fn rank(category: &str) -> Option<i64> {
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

/// Recompute effective categories from intrinsic facts and all current exact graph occurrences.
///
/// Returns only node owners whose serialized navigation details actually changed.
pub(super) fn project(conn: &Connection, cold: bool) -> Result<BTreeSet<String>> {
    conn.execute_batch(
        "CREATE TEMP TABLE entrypoint_graph_rank(node_hash INTEGER PRIMARY KEY, rank INTEGER NOT NULL) WITHOUT ROWID;",
    )?;
    let kind_rank = "CASE source.kind WHEN 'route' THEN 2 WHEN 'tool_registration' THEN 3 WHEN 'cli_command_registration' THEN 4 WHEN 'event_handler_registration' THEN 5 END";
    let statement = if cold {
        // Cold persistence defers secondary indexes. Scan occurrences once rather
        // than scanning the graph separately for every candidate.
        format!("INSERT INTO entrypoint_graph_rank SELECT occurrence.dst_hash,min({kind_rank}) FROM edge_occurrences occurrence JOIN entrypoint_candidates candidate ON candidate.node_hash=occurrence.dst_hash JOIN nodes source ON source.node_hash=occurrence.src_hash JOIN coverage_evidence confidence ON confidence.evidence_id=occurrence.confidence_id WHERE occurrence.kind='handles' AND confidence.evidence='exact' AND source.kind IN ('route','tool_registration','cli_command_registration','event_handler_registration') GROUP BY occurrence.dst_hash")
    } else {
        // Delta has the destination and occurrence indexes. Restrict the lookup
        // to changed targets, but inspect every current occurrence for exactness.
        format!("INSERT INTO entrypoint_graph_rank SELECT edge.dst_hash,min({kind_rank}) FROM entrypoint_candidates candidate JOIN edges edge ON edge.dst_hash=candidate.node_hash AND edge.kind='handles' JOIN nodes source ON source.node_hash=edge.src_hash WHERE source.kind IN ('route','tool_registration','cli_command_registration','event_handler_registration') AND EXISTS (SELECT 1 FROM edge_occurrences occurrence JOIN coverage_evidence confidence ON confidence.evidence_id=occurrence.confidence_id WHERE occurrence.src_hash=edge.src_hash AND occurrence.dst_hash=edge.dst_hash AND occurrence.kind=edge.kind AND confidence.evidence='exact') GROUP BY edge.dst_hash")
    };
    conn.execute(&statement, [])?;
    let mut statement = conn.prepare(
        "SELECT n.node_hash,n.details,p.path,graph.rank
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
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut owners = BTreeSet::new();
    let mut update = conn.prepare_cached("UPDATE nodes SET details=?1 WHERE node_hash=?2")?;
    for (hash, original, owner, graph_rank) in rows {
        let details: Value = serde_json::from_str(&original)?;
        let intrinsic = details
            .get("entrypoint_base")
            .and_then(Value::as_str)
            .and_then(rank);
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
