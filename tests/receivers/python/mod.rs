#![cfg(feature = "lang-python")]
use contextunity_forge_mcp::{
    core::{
        commitments,
        semantic::{TypeExpr, ValueExpr, ValueFlowFacts},
    },
    engine::{ast, linker},
};
use std::collections::BTreeMap;

use crate::common::Workspace;

fn build_workspace(files: &[(&str, &str)]) -> (Workspace, rusqlite::Connection) {
    let workspace = Workspace::new();
    for (path, source) in files {
        workspace.write(path, source);
    }
    workspace.build();
    let connection = workspace.open();
    (workspace, connection)
}

fn persisted_coverage(
    db: &rusqlite::Connection,
    path: &str,
    expression: &str,
) -> Vec<(String, String)> {
    let mut statement = db
        .prepare(
            "SELECT coverage.status, evidence.evidence \
             FROM resolution_coverage AS coverage \
             JOIN path_dictionary AS paths ON paths.path_id = coverage.path_id \
             JOIN coverage_expressions AS expressions ON expressions.expression_id = coverage.expression_id \
             JOIN coverage_evidence AS evidence ON evidence.evidence_id = coverage.evidence_id \
             WHERE paths.path = ?1 AND expressions.expression = ?2 \
             ORDER BY coverage.line, coverage.status",
        )
        .unwrap();
    statement
        .query_map([path, expression], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn persisted_edge(
    db: &rusqlite::Connection,
    owner_path: &str,
    kind: &str,
    expression: &str,
    target_path: &str,
    target_qualname: &str,
) -> bool {
    db.query_row(
        "SELECT EXISTS( \
         SELECT 1 FROM edge_occurrences AS occurrence \
         JOIN path_dictionary AS owner ON owner.path_id = occurrence.owner_id \
         JOIN coverage_evidence AS evidence ON evidence.evidence_id = occurrence.evidence_id \
         JOIN nodes AS target ON target.node_hash = occurrence.dst_hash \
         WHERE owner.path = ?1 AND occurrence.kind = ?2 AND evidence.evidence = ?3 \
         AND (SELECT path FROM path_dictionary WHERE path_id=target.path_id) = ?4 AND target.qualname = ?5)",
        rusqlite::params![owner_path, kind, expression, target_path, target_qualname],
        |row| row.get(0),
    )
    .unwrap()
}

fn persisted_edge_provenance(
    db: &rusqlite::Connection,
    owner_path: &str,
    expression: &str,
) -> Vec<(String, String, String, String)> {
    let mut statement = db
        .prepare(
            "SELECT occurrence.kind, evidence.evidence, target.qualname, confidence.evidence \
             FROM edge_occurrences AS occurrence \
             JOIN path_dictionary AS owner ON owner.path_id = occurrence.owner_id \
             JOIN coverage_evidence AS evidence ON evidence.evidence_id = occurrence.evidence_id \
             JOIN coverage_evidence AS confidence ON confidence.evidence_id = occurrence.confidence_id \
             JOIN nodes AS target ON target.node_hash = occurrence.dst_hash \
             WHERE owner.path = ?1 AND evidence.evidence = ?2 \
             ORDER BY occurrence.kind, target.qualname",
        )
        .unwrap();
    statement
        .query_map([owner_path, expression], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

mod bindings;
mod branch_joins;
mod builtin_members;
mod c3_mro;
mod class_members;
mod collections;
mod external_api;
mod factories;
mod fields;
mod return_boundaries;
