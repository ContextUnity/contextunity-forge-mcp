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
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT dst_hash FROM changed_edge_keys WHERE kind IN ('handles','routes_to','dispatches','listens_to')",
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
        "INSERT OR IGNORE INTO entrypoint_candidates SELECT dst_hash FROM edge_occurrences WHERE kind IN ('handles','routes_to','dispatches','listens_to')",
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
    let kind_rank = "CASE occurrence.kind WHEN 'routes_to' THEN 2 WHEN 'dispatches' THEN 5 WHEN 'listens_to' THEN 5 ELSE CASE source.kind WHEN 'main_registration' THEN 1 WHEN 'route' THEN 2 WHEN 'tool_registration' THEN 3 WHEN 'cli_command_registration' THEN 4 WHEN 'event_handler_registration' THEN 5 END END";
    let graph_filter = "(occurrence.kind IN ('routes_to','dispatches','listens_to') OR (occurrence.kind='handles' AND source.kind IN ('main_registration','route','tool_registration','cli_command_registration','event_handler_registration')))";
    let statement = if cold {
        // Cold persistence defers secondary indexes. Scan occurrences once rather
        // than scanning the graph separately for every candidate.
        format!("INSERT INTO entrypoint_graph_rank SELECT occurrence.dst_hash,min({kind_rank}) FROM edge_occurrences occurrence JOIN entrypoint_candidates candidate ON candidate.node_hash=occurrence.dst_hash JOIN nodes source ON source.node_hash=occurrence.src_hash JOIN coverage_evidence confidence ON confidence.evidence_id=occurrence.confidence_id WHERE {graph_filter} AND confidence.evidence='exact' GROUP BY occurrence.dst_hash")
    } else {
        // Delta has the destination and occurrence indexes. Restrict the lookup
        // to changed targets, but inspect every current occurrence for exactness.
        let edge_rank = kind_rank.replace("occurrence.kind", "edge.kind");
        let edge_filter = graph_filter.replace("occurrence.kind", "edge.kind");
        format!("INSERT INTO entrypoint_graph_rank SELECT edge.dst_hash,min({edge_rank}) FROM entrypoint_candidates candidate JOIN edges edge ON edge.dst_hash=candidate.node_hash AND edge.kind IN ('handles','routes_to','dispatches','listens_to') JOIN nodes source ON source.node_hash=edge.src_hash WHERE {edge_filter} AND EXISTS (SELECT 1 FROM edge_occurrences occurrence JOIN coverage_evidence confidence ON confidence.evidence_id=occurrence.confidence_id WHERE occurrence.src_hash=edge.src_hash AND occurrence.dst_hash=edge.dst_hash AND occurrence.kind=edge.kind AND confidence.evidence='exact') GROUP BY edge.dst_hash")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_graph_categories_follow_exact_occurrences_in_cold_and_delta() {
        for (kind, expected) in [
            ("routes_to", "http_route"),
            ("dispatches", "event_handler"),
            ("listens_to", "event_handler"),
        ] {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(
                "CREATE TABLE nodes(node_hash INTEGER PRIMARY KEY, kind TEXT, details TEXT, owner_path_id INTEGER);
                 CREATE TABLE path_dictionary(path_id INTEGER PRIMARY KEY, path TEXT);
                 CREATE TABLE coverage_evidence(evidence_id INTEGER PRIMARY KEY, evidence TEXT);
                 CREATE TABLE edge_occurrences(src_hash INTEGER, dst_hash INTEGER, kind TEXT, confidence_id INTEGER);
                 CREATE TABLE edges(src_hash INTEGER, dst_hash INTEGER, kind TEXT);
                 CREATE TEMP TABLE changed_edge_keys(src_hash INTEGER, dst_hash INTEGER, kind TEXT);
                 INSERT INTO path_dictionary VALUES(1,'source.py'),(2,'target.py');
                 INSERT INTO nodes VALUES(1,'module','{}',1),(2,'function','{}',2),(3,'function','{}',2);
                 INSERT INTO coverage_evidence VALUES(1,'exact'),(2,'inferred');",
            ).unwrap();
            conn.execute("INSERT INTO edge_occurrences VALUES(1,2,?1,2)", [kind])
                .unwrap();
            conn.execute("INSERT INTO edges VALUES(1,2,?1)", [kind])
                .unwrap();
            prepare_candidates(&conn).unwrap();
            add_cold_candidates(&conn).unwrap();
            project(&conn, true).unwrap();
            let category = |id| {
                conn.query_row(
                    "SELECT json_extract(details,'$.entrypoint') FROM nodes WHERE node_hash=?1",
                    [id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .unwrap()
            };
            assert_eq!(category(2), None, "{kind}: nonexact must not classify");
            conn.execute("UPDATE edge_occurrences SET confidence_id=1", [])
                .unwrap();
            conn.execute_batch("DROP TABLE entrypoint_graph_rank")
                .unwrap();
            project(&conn, true).unwrap();
            assert_eq!(category(2).as_deref(), Some(expected), "{kind}: exact cold");
            conn.execute(
                "INSERT INTO changed_edge_keys VALUES(1,2,?1),(1,3,?1)",
                [kind],
            )
            .unwrap();
            conn.execute("UPDATE edge_occurrences SET dst_hash=3", [])
                .unwrap();
            conn.execute("UPDATE edges SET dst_hash=3", []).unwrap();
            conn.execute_batch(
                "DROP TABLE entrypoint_graph_rank; DELETE FROM entrypoint_candidates;",
            )
            .unwrap();
            add_delta_candidates(&conn, &BTreeSet::new()).unwrap();
            let owners = project(&conn, false).unwrap();
            assert_eq!(owners, BTreeSet::from(["target.py".to_owned()]));
            assert_eq!(category(2), None, "{kind}: old target cleared");
            assert_eq!(category(3).as_deref(), Some(expected), "{kind}: new target");
            conn.execute("DELETE FROM edge_occurrences", []).unwrap();
            conn.execute("DELETE FROM edges", []).unwrap();
            conn.execute_batch("DROP TABLE entrypoint_graph_rank")
                .unwrap();
            project(&conn, false).unwrap();
            assert_eq!(category(3), None, "{kind}: removed");
        }
    }
}
