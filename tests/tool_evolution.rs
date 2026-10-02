#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::db::{reader, writer};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static WORKSPACE_NONCE: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = WORKSPACE_NONCE.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "forge_evolution_{}_{}_{}",
            std::process::id(),
            nonce,
            sequence
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, path: &str, value: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) {
        writer::build(&self.0, &self.db(), None).unwrap();
    }
    fn cli(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn query(&self, args: &[&str]) -> Value {
        let out = self.cli(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn persisted_state(workspace: &Workspace) -> Value {
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
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

struct Mcp {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u32,
}
impl Mcp {
    fn new(w: &Workspace) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(&w.0)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut this = Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            next_id: 1,
        };
        let init = this.request(
            "initialize",
            serde_json::json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"stress-test","version":"1"}}),
        );
        assert!(init.get("error").is_none(), "{init}");
        writeln!(
            this.input,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        this.input.flush().unwrap();
        this
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(
            self.input,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "MCP closed stdout on {method}"
            );
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == id {
                return response;
            }
        }
    }
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            serde_json::json!({"name":name,"arguments":arguments}),
        )
    }
    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.call(name, arguments);
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
    fn err(&mut self, name: &str, arguments: Value, expected: &str) {
        let response = self.call(name, arguments);
        assert_eq!(response["result"]["isError"], true, "{name}: {response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            message.contains(expected),
            "{name}: expected {expected:?}, got {message:?}"
        );
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn edges(w: &Workspace) -> Vec<Value> {
    let c = reader::open(&w.db(), &w.0).unwrap();
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
