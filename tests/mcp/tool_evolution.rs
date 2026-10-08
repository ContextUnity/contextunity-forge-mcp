#![cfg(feature = "lang-python")]

use crate::common::{
    assertions,
    mcp_client::{run_cli, StdioClient},
    Workspace,
};
use contextunity_forge_mcp::db::reader;
use serde_json::Value;
use std::fs;

trait WorkspaceQueryExt {
    fn query(&self, args: &[&str]) -> Value;
}

impl WorkspaceQueryExt for Workspace {
    fn query(&self, args: &[&str]) -> Value {
        let out = run_cli(self, args);
        assertions::assert_command_succeeded(&out);
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

fn persisted_state(workspace: &Workspace) -> Value {
    let conn = workspace.open();
    let schema = reader::rows(
        &conn,
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name",
        &[],
        100_000,
    )
    .unwrap();
    let table_names = {
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let tables = table_names
        .into_iter()
        .map(|name| {
            let quoted_name = name.replace('"', "\"\"");
            let mut rows = reader::rows(
                &conn,
                &format!("SELECT * FROM \"{quoted_name}\""),
                &[],
                100_000,
            )
            .unwrap();
            rows.sort_by_key(|row| row.to_string());
            (name, rows)
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema": schema,
        "tables": tables,
        "pragmas": {
            "application_id": conn.query_row("PRAGMA application_id", [], |row| row.get::<_, i64>(0)).unwrap(),
            "freelist_count": conn.query_row("PRAGMA freelist_count", [], |row| row.get::<_, i64>(0)).unwrap(),
            "journal_mode": conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0)).unwrap(),
            "page_count": conn.query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0)).unwrap(),
            "schema_version": conn.query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0)).unwrap(),
            "user_version": conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)).unwrap(),
        },
    })
}

type Mcp = StdioClient;

trait McpAssertionExt {
    fn ok(&mut self, name: &str, arguments: Value) -> Value;
    fn err(&mut self, name: &str, arguments: Value, expected: &str);
}

impl McpAssertionExt for StdioClient {
    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        self.payload(name, arguments)
    }

    fn err(&mut self, name: &str, arguments: Value, expected: &str) {
        let (_, response) = self.call(name, arguments);
        assertions::assert_rpc_succeeded(&response);
        assert_eq!(response["result"]["isError"], true, "{name}: {response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            message.contains(expected),
            "{name}: expected {expected:?}, got {message:?}"
        );
    }
}

fn edges(w: &Workspace) -> Vec<Value> {
    let c = w.open();
    reader::rows(&c, "SELECT a.name src,b.name dst,e.kind FROM edges e JOIN nodes a ON a.node_hash=e.src_hash JOIN nodes b ON b.node_hash=e.dst_hash", &[], 1000).unwrap()
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
