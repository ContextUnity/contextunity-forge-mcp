#![cfg(feature = "lang-python")]

use crate::cli::run_cli;
pub(super) use crate::common::Workspace;
#[allow(unused_imports)]
pub(super) use crate::harness::McpTestHarness;
use contextunity_forge_mcp::db::reader;
use serde_json::Value;
use std::fs;

fn edges(w: &Workspace) -> Vec<Value> {
    crate::sqlite_fixture::edge_rows(w).unwrap()
}
fn has_edge(rows: &[Value], src: &str, dst: &str, kind: &str) -> bool {
    rows.iter()
        .any(|r| r["src"] == src && r["dst"] == dst && r["kind"] == kind)
}

#[path = "tool_evolution/cli_and_relations.rs"]
mod cli_and_relations;
#[path = "tool_evolution/mcp_boundaries.rs"]
mod mcp_boundaries;
#[path = "tool_evolution/mcp_limits.rs"]
mod mcp_limits;
