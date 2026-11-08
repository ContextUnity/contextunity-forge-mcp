#![cfg(feature = "lang-typescript")]

use super::harness::{query_scalar, ExtractorHarness, LinkedFacts};
use contextunity_forge_mcp::core::models::Facts;
use std::collections::BTreeMap;

fn linked(path: &str, language: &str, source: &str) -> LinkedFacts {
    ExtractorHarness::for_lang(language)
        .at_path(path)
        .linked(source)
}

fn persisted_coverage(
    conn: &rusqlite::Connection,
    path: &str,
    line: Option<i64>,
    expression: &str,
) -> (String, String) {
    conn.query_row(
        "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (?2 IS NULL OR line=?2) AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?3",
        (path, line, expression), |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap()
}

fn persisted_call_paths(conn: &rusqlite::Connection, path: &str) -> Vec<String> {
    conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap().query_map([path], |row| row.get(0)).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

fn extracted_files(files: &[(&str, &str)], language: Option<&str>) -> BTreeMap<String, Facts> {
    files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ExtractorHarness::for_lang(language.unwrap_or(if path.ends_with(".js") {
                    "javascript"
                } else {
                    "typescript"
                }))
                .at_path(path)
                .extract(source),
            )
        })
        .collect()
}

fn linked_files(
    files: &[(&str, &str)],
) -> (
    BTreeMap<String, Facts>,
    contextunity_forge_mcp::core::models::Graph,
) {
    let facts = extracted_files(files, None);
    let graph = ExtractorHarness::link_graph(&facts);
    (facts, graph)
}

fn calls(files: &[(&str, &str)], expression: &str) -> Vec<String> {
    let (facts, graph) = linked_files(files);
    graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == expression)
        .map(|edge| {
            facts
                .values()
                .flat_map(|facts| &facts.nodes)
                .find(|node| node.id == edge.dst)
                .unwrap()
                .path
                .clone()
        })
        .collect()
}

fn resolves(source: &str, expression: &str) -> bool {
    ExtractorHarness::for_lang("typescript")
        .at_path("service.ts")
        .linked(source)
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == expression)
}

mod dom;
mod frameworks;
mod grammar;
mod modules;
