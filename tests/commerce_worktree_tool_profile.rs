//! Opt-in release MCP profile against a stable indexed worktree.
//! Run with `PHASE5_WORKTREE_ROOT=... cargo test --release --test commerce_worktree_tool_profile -- --ignored --nocapture`.

use anyhow::{ensure, Context, Result};
use contextunity_forge_mcp::db::reader;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_worktree_tools_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }

    fn db(&self) -> PathBuf {
        self.0.join("code-map.sqlite")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Mcp {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    next_id: u64,
}

impl Mcp {
    fn new(root: &std::path::Path, db: &std::path::Path) -> Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(root)
            .arg("--db")
            .arg(db)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("start isolated Forge MCP server")?;
        let input = child.stdin.take().context("MCP stdin missing")?;
        let mut reader = BufReader::new(child.stdout.take().context("MCP stdout missing")?);
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
            next_id: 0,
        };
        let (_, response) = client.request(
            "initialize",
            json!({
                "protocolVersion":"2025-03-26",
                "capabilities":{},
                "clientInfo":{"name":"commerce-tool-profile","version":"1"}
            }),
        )?;
        ensure!(
            response.get("error").is_none(),
            "MCP initialize failed: {response}"
        );
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )?;
        client.input.flush()?;
        Ok(client)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<(Duration, Value)> {
        self.next_id += 1;
        let id = self.next_id;
        let started = Instant::now();
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )?;
        self.input.flush()?;
        loop {
            let line = self
                .output
                .recv_timeout(Duration::from_secs(120))
                .context("MCP response timed out")?;
            let response: Value = serde_json::from_str(&line)?;
            if response["id"] == id {
                return Ok((started.elapsed(), response));
            }
        }
    }

    fn call(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<(Duration, usize, bool, Option<Value>, Option<String>)> {
        let (elapsed, response) =
            self.request("tools/call", json!({"name":name,"arguments":arguments}))?;
        if let Some(error) = response.get("error") {
            return Ok((elapsed, 0, false, None, Some(error.to_string())));
        }
        let result = &response["result"];
        let is_error = result["isError"] == true;
        let output = result["content"][0]["text"]
            .as_str()
            .with_context(|| format!("{name} returned no text content: {response}"))?;
        let parsed = serde_json::from_str(output).ok();
        let error = is_error.then(|| output.to_owned());
        Ok((elapsed, output.len(), !is_error, parsed, error))
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

#[test]
#[ignore = "isolated release profile against the stable phase5 worktree"]
fn phase5_all_tool_latency_profile() -> Result<()> {
    let root = PathBuf::from(std::env::var("PHASE5_WORKTREE_ROOT")?);
    ensure!(
        root.is_dir(),
        "phase5 worktree does not exist: {}",
        root.display()
    );
    let source_db = root.join(".forge/code-map.sqlite");
    ensure!(
        source_db.is_file(),
        "phase5 index is missing: {}",
        source_db.display()
    );
    let source_wal = PathBuf::from(format!("{}-wal", source_db.display()));
    ensure!(
        fs::metadata(&source_wal)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            == 0,
        "phase5 index has an active WAL; snapshot it before profiling"
    );
    let source_conn = Connection::open_with_flags(
        &source_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let indexed_files: i64 =
        source_conn.query_row("SELECT count(*) FROM files", [], |row| row.get(0))?;
    let indexed_nodes: i64 =
        source_conn.query_row("SELECT count(*) FROM nodes", [], |row| row.get(0))?;
    let indexed_edges: i64 =
        source_conn.query_row("SELECT count(*) FROM edges", [], |row| row.get(0))?;
    let source_semantics: String = source_conn.query_row(
        "SELECT value FROM metadata WHERE key='index_semantics_version'",
        [],
        |row| row.get(0),
    )?;
    let current_semantics = contextunity_forge_mcp::engine::scanner::INDEX_SEMANTICS_VERSION;
    let index_needs_rebuild = source_semantics != current_semantics;
    let (selector_path, selector_line, selector_name): (String, usize, String) = source_conn.query_row(
        "SELECT path,line,name FROM nodes WHERE path NOT LIKE '[%' AND kind='function' AND is_test=1 AND name LIKE 'test_%' AND language='python' ORDER BY path,line LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let doc_path: String = source_conn.query_row(
        "SELECT path FROM doc_sections WHERE path NOT LIKE '[%' AND path LIKE 'docs/%' ORDER BY path LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    drop(source_conn);

    let workspace = Workspace::new()?;
    let copy_started = Instant::now();
    fs::copy(&source_db, workspace.db())
        .context("copy phase5 index into isolated temporary workspace")?;
    let index_copy_ms = copy_started.elapsed().as_secs_f64() * 1000.0;
    let integrity_started = Instant::now();
    let copied_conn = Connection::open(workspace.db())?;
    let integrity: String =
        copied_conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity == "ok",
        "isolated index integrity check failed: {integrity}"
    );
    drop(copied_conn);
    let copied_index_integrity_ms = integrity_started.elapsed().as_secs_f64() * 1000.0;
    let copied_index_verify_ms = if index_needs_rebuild {
        None
    } else {
        let verify_started = Instant::now();
        drop(reader::open(&workspace.db(), &root)?);
        Some(verify_started.elapsed().as_secs_f64() * 1000.0)
    };

    let selector = format!("{selector_path}:{selector_line}");
    ensure!(
        root.join(&selector_path).is_file(),
        "selected source is missing: {selector_path}"
    );
    let args = vec![
        (
            "ast_grep_search",
            json!({"pattern":"def $NAME($$$ARGS):","language":"python","path":selector_path,"detail":"compact","limit":1}),
        ),
        (
            "code_map_analyze",
            json!({"target":"SELECT count(*) AS total FROM nodes","limit":1}),
        ),
        (
            "code_map_explain",
            json!({"selector":selector.clone(),"detail":"compact","limit":1}),
        ),
        (
            "code_map_impact",
            json!({"selector":selector.clone(),"depth":1,"limit":1}),
        ),
        (
            "code_map_inspect",
            json!({"selector":selector.clone(),"detail":"compact"}),
        ),
        ("code_map_overview", json!({"detail":"compact","limit":1})),
        (
            "code_map_prove_removal",
            json!({"selector":selector.clone(),"detail":"compact","limit":1}),
        ),
        (
            "code_map_query",
            json!({"operation":"slice","selector":selector.clone(),"depth":1,"detail":"compact","limit":1}),
        ),
        (
            "code_map_search",
            json!({"pattern":format!("{selector_name}*"),"detail":"compact","limit":1}),
        ),
        (
            "code_map_tests",
            json!({"selector":selector.clone(),"direction":"outbound","limit":1}),
        ),
        ("forge_guide", json!({"topic":"query"})),
        (
            "get_code_snippet",
            json!({"selector":selector,"leading_lines":0,"max_body_lines":20}),
        ),
        (
            "get_doc",
            json!({"path_or_id":doc_path,"detail":"compact","limit":1}),
        ),
        ("search_docs", json!({"query":"architecture","limit":1})),
        ("session_checkpoint", json!({"action":"list"})),
    ];

    let mut client = Mcp::new(&root, &workspace.db())?;
    let (_, listed) = client.request("tools/list", json!({}))?;
    ensure!(listed.get("error").is_none(), "tools/list failed: {listed}");
    let listed_names: std::collections::BTreeSet<String> = listed["result"]["tools"]
        .as_array()
        .context("tools/list omitted tools")?
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect();
    let expected_names: std::collections::BTreeSet<String> =
        args.iter().map(|(name, _)| (*name).to_owned()).collect();
    ensure!(
        listed_names == expected_names,
        "MCP tool set differs: {listed_names:?}"
    );

    let (initial_ms, initial_bytes, initial_ok, initial_payload, initial_error) =
        client.call("code_map_overview", json!({"detail":"compact","limit":1}))?;
    let mut results = Vec::new();
    for (name, arguments) in args {
        let mut samples = Vec::new();
        let mut output_bytes = 0;
        let mut ok = true;
        let mut error = None;
        for _ in 0..2 {
            let (_, bytes, succeeded, _, message) = client.call(name, arguments.clone())?;
            output_bytes = bytes;
            ok &= succeeded;
            error = error.or(message);
        }
        for _ in 0..7 {
            let (elapsed, bytes, succeeded, _, message) = client.call(name, arguments.clone())?;
            samples.push(elapsed.as_secs_f64() * 1000.0);
            output_bytes = bytes;
            ok &= succeeded;
            error = error.or(message);
        }
        results.push(json!({
            "tool":name,
            "ok":ok,
            "error":error,
            "median_ms":median(&samples),
            "min_ms":samples.iter().copied().fold(f64::INFINITY, f64::min),
            "max_ms":samples.iter().copied().fold(0.0, f64::max),
            "output_bytes":output_bytes,
        }));
    }
    drop(client);
    let current_conn = Connection::open_with_flags(
        workspace.db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let current_files: i64 =
        current_conn.query_row("SELECT count(*) FROM files", [], |row| row.get(0))?;
    let current_nodes: i64 =
        current_conn.query_row("SELECT count(*) FROM nodes", [], |row| row.get(0))?;
    let current_edges: i64 =
        current_conn.query_row("SELECT count(*) FROM edges", [], |row| row.get(0))?;
    drop(current_conn);
    eprintln!(
        "phase5_tool_profile={}",
        json!({
            "index_root":root,
            "source_db_bytes":fs::metadata(&source_db)?.len(),
            "files":indexed_files,
            "nodes":indexed_nodes,
            "edges":indexed_edges,
            "rebuilt_files":current_files,
            "rebuilt_nodes":current_nodes,
            "rebuilt_edges":current_edges,
            "rebuilt_db_bytes":fs::metadata(workspace.db())?.len(),
            "source_index_semantics":source_semantics,
            "current_index_semantics":current_semantics,
            "index_needs_rebuild":index_needs_rebuild,
            "tool_count":listed_names.len(),
            "index_copy_ms":index_copy_ms,
            "copied_index_integrity_ms":copied_index_integrity_ms,
            "copied_index_verify_ms":copied_index_verify_ms,
            "first_overview_ms":initial_ms.as_secs_f64()*1000.0,
            "first_overview_output_bytes":initial_bytes,
            "first_overview_ok":initial_ok,
            "first_overview_error":initial_error,
            "first_overview_freshness":initial_payload.as_ref().and_then(|value| value.get("freshness")),
            "tools":results,
        })
    );
    ensure!(initial_ok, "initial overview failed: {initial_error:?}");
    ensure!(
        results.iter().all(|result| result["ok"] == true),
        "one or more MCP tools returned an error; see phase5_tool_profile above"
    );
    Ok(())
}
