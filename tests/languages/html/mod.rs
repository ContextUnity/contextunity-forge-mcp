#![cfg(feature = "lang-html")]

use contextunity_forge_mcp::{
    core::{
        commitments,
        response::{Detail, QueryOptions, ResponsePolicy, SourceOptions},
    },
    db::{reader, symbols, writer},
    engine::{ast, scanner},
};
use std::{collections::BTreeMap, fs, path::PathBuf};

use super::support::Workspace;

#[cfg(feature = "lang-typescript")]
fn coverage_aggregate_rows(conn: &rusqlite::Connection) -> (Vec<String>, Vec<String>) {
    let owner_language = conn
        .prepare(
            "SELECT json_array(p.path, c.line, e.expression, c.status, v.evidence, c.language) \
             FROM coverage_owner_language c \
             JOIN path_dictionary p ON p.path_id=c.path_id \
             JOIN coverage_expressions e ON e.expression_id=c.expression_id \
             JOIN coverage_evidence v ON v.evidence_id=c.evidence_id \
             ORDER BY p.path, c.line, e.expression, c.status, v.evidence, c.language",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let language_counts = conn
        .prepare(
            "SELECT json_array(p.path, c.language, c.status, c.records) \
             FROM coverage_language_counts c \
             JOIN path_dictionary p ON p.path_id=c.path_id \
             ORDER BY p.path, c.language, c.status",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    (owner_language, language_counts)
}

#[cfg(feature = "lang-typescript")]
#[path = "classic_wire.rs"]
mod classic_wire;
mod syntax;
#[path = "template_masking.rs"]
mod template_masking;
#[cfg(feature = "lang-python")]
#[path = "template_origins.rs"]
mod template_origins;
mod templates;
