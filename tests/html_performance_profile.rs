//! Opt-in, isolated profile for the incremental cost of HTML HTMX extraction.
//! Run with `cargo test --release --test html_performance_profile -- --ignored --nocapture`.
#![cfg(all(feature = "lang-html", feature = "lang-typescript", feature = "lang-vue"))]

use anyhow::{ensure, Context, Result};
use contextunity_forge_mcp::db::writer;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("forge_html_profile_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }
    fn root(&self, htmx: bool) -> PathBuf {
        self.0.join(if htmx { "htmx" } else { "plain" })
    }
    fn write(&self, htmx: bool, path: &str, content: &str) -> Result<()> {
        let path = self.root(htmx).join(path);
        fs::create_dir_all(path.parent().context("fixture path has no parent")?)?;
        fs::write(path, content)?;
        Ok(())
    }
}
impl Drop for Workspace {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn html_page(index: usize, htmx: bool) -> String {
    let attr = if htmx { "hx-get" } else { "data-x" };
    let target = if htmx { "hx-target" } else { "data-xxxx" };
    format!(
        "<!doctype html>\n<html><body><main id='page-{index}'>\n<h1>Catalog page {index}</h1>\n<button {attr}='/api/items/{index}' {target}='#result'>Load</button>\n<div id='result'>Ready</div>\n</main></body></html>\n"
    )
}

fn fixture(ws: &Workspace, htmx: bool) -> Result<()> {
    for i in 0..60 { ws.write(htmx, &format!("pages/page_{i:02}.html"), &html_page(i, htmx))?; }
    for i in 0..10 {
        ws.write(htmx, &format!("components/Widget{i:02}.vue"), &format!(
            "<template><section><h2>Widget {i}</h2><button @click=\"load\">Go</button></section></template>\n<script setup lang=\"ts\">\nfunction load() {{ return {i}; }}\n</script>\n"
        ))?;
        ws.write(htmx, &format!("components/Widget{i:02}.tsx"), &format!(
            "export function Widget{i}() {{ return <section><h2>Widget {i}</h2><button>Go</button></section>; }}\n"
        ))?;
    }
    Ok(())
}

fn counts(db: &Path) -> Result<(i64, i64, i64, i64)> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(conn.query_row(
        "SELECT (SELECT count(*) FROM files), (SELECT count(*) FROM nodes), (SELECT count(*) FROM edges), (SELECT count(*) FROM nodes WHERE kind='htmx_request')",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?)
}

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}
impl Client {
    fn new(binary: &Path, root: &Path, db: &Path) -> Result<Self> {
        let mut child = Command::new(binary).arg("--root").arg(root).arg("--db").arg(db).arg("serve")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let input = child.stdin.take().context("missing MCP stdin")?;
        let output = BufReader::new(child.stdout.take().context("missing MCP stdout")?);
        let mut client = Self { child, input, output, id: 0 };
        client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"html-profile","version":"1"}}))?;
        writeln!(client.input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        client.input.flush()?;
        Ok(client)
    }
    fn request(&mut self, method: &str, params: Value) -> Result<(Value, f64)> {
        self.id += 1;
        let started = Instant::now();
        writeln!(self.input, "{}", json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}))?;
        self.input.flush()?;
        let mut line = String::new();
        loop {
            ensure!(self.output.read_line(&mut line)? != 0, "MCP closed stdout");
            let response: Value = serde_json::from_str(&line)?;
            if response["id"] == self.id {
                ensure!(response.get("error").is_none() && response["result"]["isError"] != true, "MCP error: {response}");
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                return Ok((response, elapsed));
            }
            line.clear();
        }
    }
    fn call(&mut self, name: &str, args: Value) -> Result<(Value, f64)> {
        let (response, ms) = self.request("tools/call", json!({"name":name,"arguments":args}))?;
        let text = response["result"]["content"][0]["text"].as_str().context("missing tool text")?;
        Ok((serde_json::from_str(text)?, ms))
    }
}
impl Drop for Client {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[test]
#[ignore = "isolated release performance profile; prints data, does not assert machine-specific timings"]
fn html_htmx_incremental_cost() -> Result<()> {
    let ws = Workspace::new()?;
    fixture(&ws, false)?;
    fixture(&ws, true)?;
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
    let mut cold = [Vec::<f64>::new(), Vec::<f64>::new()];
    let mut delta = [Vec::<f64>::new(), Vec::<f64>::new()];
    let mut last_db = [PathBuf::new(), PathBuf::new()];
    for round in 0..20 {
        let order = if round % 2 == 0 { [false, true] } else { [true, false] };
        for htmx in order {
            let index = usize::from(htmx);
            let root = ws.root(htmx);
            let db = root.join(format!(".forge/profile-{index}-{round}.sqlite"));
            let started = Instant::now();
            let build_report = writer::build(&root, &db, None)?;
            let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
            let path = root.join("pages/page_00.html");
            let original = fs::read_to_string(&path)?;
            fs::write(&path, format!("{original}\n"))?;
            let started = Instant::now();
            let delta_report = writer::delta(&root, &db, &[PathBuf::from("pages/page_00.html")])?;
            let delta_ms = started.elapsed().as_secs_f64() * 1000.0;
            fs::write(&path, original)?;
            writer::delta(&root, &db, &[PathBuf::from("pages/page_00.html")])?;
            let graph = counts(&db)?;
            println!("HTML_PROFILE {}", json!({"variant":if htmx {"htmx"} else {"plain"},"round":round,"cold_ms":cold_ms,"delta_ms":delta_ms,"cold_scan_ms":build_report["scan_ms"],"cold_extract_ms":build_report["extract_ms"],"cold_link_ms":build_report["link_ms"],"cold_persist_ms":build_report["persist_ms"],"cold_rows_ms":build_report["rows_ms"],"cold_indexes_ms":build_report["indexes_ms"],"cold_seal_ms":build_report["seal_ms"],"delta_scan_ms":delta_report["scan_ms"],"delta_extract_ms":delta_report["extract_ms"],"delta_graph_ms":delta_report["graph_ms"],"delta_commitments_ms":delta_report["commitments_ms"],"files":graph.0,"nodes":graph.1,"edges":graph.2,"htmx_requests":graph.3,"db_bytes":fs::metadata(&db)?.len()}));
            cold[index].push(cold_ms);
            delta[index].push(delta_ms);
            last_db[index] = db;
        }
    }
    let mut clients = [Client::new(&binary, &ws.root(false), &last_db[0])?, Client::new(&binary, &ws.root(true), &last_db[1])?];
    let mut search = [Vec::<f64>::new(), Vec::<f64>::new()];
    let mut inspect = [Vec::<f64>::new(), Vec::<f64>::new()];
    for round in 0..24 {
        let order = if round % 2 == 0 { [0, 1] } else { [1, 0] };
        for index in order {
            let (_, search_ms) = clients[index].call("code_map_search", json!({"pattern":"Widget*","detail":"compact","limit":10}))?;
            let (_, inspect_ms) = clients[index].call("code_map_inspect", json!({"selector":"pages/page_00.html","detail":"compact","limit":10}))?;
            if round >= 4 { search[index].push(search_ms); inspect[index].push(inspect_ms); }
        }
    }
    let htmx_id: String = Connection::open_with_flags(&last_db[1], OpenFlags::SQLITE_OPEN_READ_ONLY)?
        .query_row("SELECT id FROM nodes WHERE kind='htmx_request' ORDER BY id LIMIT 1", [], |row| row.get(0))?;
    let mut htmx_search = Vec::new();
    let mut htmx_inspect = Vec::new();
    for round in 0..24 {
        let (_, search_ms) = clients[1].call("code_map_search", json!({"pattern":"GET*","kind":"htmx_request","detail":"compact","limit":10}))?;
        let (_, inspect_ms) = clients[1].call("code_map_inspect", json!({"selector":htmx_id,"detail":"compact","limit":10}))?;
        if round >= 4 { htmx_search.push(search_ms); htmx_inspect.push(inspect_ms); }
    }
    println!("HTML_PROFILE_SUMMARY {}", json!({
        "cold_median_ms":{"plain":median(cold[0].clone()),"htmx":median(cold[1].clone())},
        "delta_median_ms":{"plain":median(delta[0].clone()),"htmx":median(delta[1].clone())},
        "warm_search_median_ms":{"plain":median(search[0].clone()),"htmx":median(search[1].clone())},
        "warm_inspect_median_ms":{"plain":median(inspect[0].clone()),"htmx":median(inspect[1].clone())},
        "htmx_search_median_ms":median(htmx_search),
        "htmx_inspect_median_ms":median(htmx_inspect),
        "samples":{"cold":20,"delta":20,"warm_per_variant":20}
    }));
    Ok(())
}
