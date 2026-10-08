#![cfg(feature = "lang-python")]

use super::support::Workspace as PythonWorkspace;
use contextunity_forge_mcp::{
    core::models::ReceiverHint,
    engine::{ast, linker},
};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn linked_python_roots() -> (PythonWorkspace, PythonWorkspace) {
    let primary = PythonWorkspace::new();
    let second = PythonWorkspace::new();
    primary.write(
        "forge-mcp.yaml",
        &format!(
        "roots: [src]\nlinked_workspaces:\n  - name: second\n    path: '{}'\n    roots: [src]\n",
        second.0.display()
    ),
    );
    (primary, second)
}

fn persisted_import(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    name: &str,
) -> (String, Vec<String>) {
    let status = conn.query_row(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        rusqlite::params![path, line, name], |row| row.get(0),
    ).unwrap_or_else(|_| panic!("missing import coverage {path}:{line} {name}"));
    let targets = conn.prepare(
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) || ':' || dst.name FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE p.path=?1 AND e.line=?2 AND e.kind='imports' AND v.evidence=?3 ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id),dst.name",
    ).unwrap().query_map(rusqlite::params![path, line, name], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    (status, targets)
}

fn persisted_status(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    expression: &str,
) -> String {
    conn.query_row(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        rusqlite::params![path, line, expression], |row| row.get(0),
    ).unwrap_or_else(|_| panic!("missing coverage {path}:{line} {expression}"))
}

fn persisted_coverage(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    expression: &str,
) -> (String, String) {
    conn.query_row(
        "SELECT c.status,e.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        rusqlite::params![path, line, expression], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap_or_else(|_| panic!("missing coverage {path}:{line} {expression}"))
}

fn persisted_status_rows(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    expression: &str,
) -> Vec<String> {
    conn.prepare(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3 ORDER BY c.status",
    ).unwrap().query_map(rusqlite::params![path, line, expression], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
}

mod frameworks;
mod grammar;
mod modules;
mod semantics;
