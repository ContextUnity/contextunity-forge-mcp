#![cfg(feature = "lang-rust")]

use contextunity_forge_mcp::{db, engine::scanner, mcp::server::Server};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge_freshness_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, source: &str) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, source).unwrap();
    }
    fn server(&self) -> Server {
        Server::new(self.0.clone(), self.0.join(".forge/index.sqlite"))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn snapshot(server: &Server) -> anyhow::Result<Value> {
    server.read(|conn| {
        Ok(json!({
            "nodes": db::reader::rows(conn, "SELECT name,path,line,end_line FROM nodes WHERE kind='function' ORDER BY path,name", &[], 100)?,
            "corpus_hash": conn.query_row("SELECT value FROM metadata WHERE key='corpus_hash'", [], |row| row.get::<_, String>(0))?,
            "output_root": conn.query_row("SELECT value FROM metadata WHERE key='output_root'", [], |row| row.get::<_, String>(0))?,
        }))
    })
}

fn wait_for_inventory_ttl() {
    std::thread::sleep(std::time::Duration::from_millis(5100));
}

#[cfg(feature = "lang-python")]
struct StdioClient {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    id: usize,
}

impl StdioClient {
    fn new(ws: &Workspace) -> Self {
        Self::with_binary(ws, env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
    }
    fn with_binary(ws: &Workspace, binary: impl AsRef<std::ffi::OsStr>) -> Self {
        let mut child = Command::new(binary)
            .args(["--root", ws.0.to_str().unwrap(), "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || sender.send(line).is_err() {
                break;
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
        };
        let init = client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"freshness-test","version":"1"}}));
        assert!(init.get("error").is_none(), "{init}");
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client.input.flush().unwrap();
        client
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let line = self
                .output
                .recv_timeout(Duration::from_secs(30))
                .expect("MCP response missing or timed out");
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == self.id {
                return response;
            }
        }
    }
    fn document(&mut self, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({"name":"get_doc","arguments":arguments}),
        );
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[path = "mcp_freshness/freshness_and_admission.rs"]
mod freshness_and_admission;
#[path = "mcp_freshness/stdio_and_response.rs"]
mod stdio_and_response;
