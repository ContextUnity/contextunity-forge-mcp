#![cfg(feature = "lang-rust")]

use crate::common::{
    mcp_client::{in_process_server, StdioClient},
    Workspace,
};
use contextunity_forge_mcp::{db, engine::scanner, mcp::server::Server};
use serde_json::{json, Value};
use std::{fs, process::Command, sync::mpsc, time::Duration};
fn snapshot(server: &Server) -> anyhow::Result<Value> {
    server.read(|conn| {
        Ok(json!({
            "nodes": db::reader::rows(conn, "SELECT n.name,(SELECT path FROM path_dictionary WHERE path_id=n.path_id) AS path,n.line,n.end_line FROM nodes n WHERE n.kind='function' ORDER BY path,n.name", &[], 100)?,
            "corpus_hash": conn.query_row("SELECT value FROM metadata WHERE key='corpus_hash'", [], |row| row.get::<_, String>(0))?,
            "output_root": conn.query_row("SELECT value FROM metadata WHERE key='output_root'", [], |row| row.get::<_, String>(0))?,
        }))
    })
}

fn wait_for_inventory_ttl() {
    std::thread::sleep(std::time::Duration::from_millis(5100));
}

#[path = "freshness/freshness_and_admission.rs"]
mod freshness_and_admission;
#[path = "freshness/stdio_and_response.rs"]
mod stdio_and_response;
