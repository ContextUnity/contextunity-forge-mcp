use anyhow::{ensure, Context, Result};
use contextunity_forge_mcp::{
    core::{response::{Detail, QueryOptions, ResponsePolicy}, schema::SCHEMA_DDL},
    db::{reader, traversal, writer},
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{fs, io::{BufRead, BufReader, Write}, path::PathBuf, process::{Child, ChildStdin, ChildStdout, Command, Stdio}, time::{Instant, SystemTime, UNIX_EPOCH}};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("forge_traversal_profile_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }
    fn database(&self, name: &str) -> Result<Connection> {
        let conn = Connection::open(self.0.join(name))?;
        conn.execute_batch(SCHEMA_DDL)?;
        conn.execute("INSERT INTO metadata VALUES('output_root','profile')", [])?;
        conn.execute_batch("PRAGMA temp_store=MEMORY")?;
        Ok(conn)
    }
}
impl Drop for Workspace { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

struct McpClient {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}
impl McpClient {
    fn new(binary: &str, root: &PathBuf) -> Result<Self> {
        let mut child = Command::new(binary).arg("--root").arg(root).arg("serve")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let input = child.stdin.take().context("MCP stdin unavailable")?;
        let output = BufReader::new(child.stdout.take().context("MCP stdout unavailable")?);
        let mut client = Self { child, input, output, id: 0 };
        client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"traversal-workflow-profile","version":"1"}}))?;
        writeln!(client.input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        client.input.flush()?;
        Ok(client)
    }
    fn request(&mut self, method: &str, params: Value) -> Result<(Value, f64)> {
        self.id += 1;
        let request = json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params});
        let started = Instant::now();
        writeln!(self.input, "{request}")?;
        self.input.flush()?;
        let mut line = String::new();
        loop {
            ensure!(self.output.read_line(&mut line)? > 0, "MCP closed stdout");
            let response: Value = serde_json::from_str(&line)?;
            if response["id"] == self.id {
                ensure!(response.get("error").is_none() && response["result"]["isError"] != true, "MCP error: {response}");
                return Ok((response, started.elapsed().as_secs_f64() * 1000.));
            }
            line.clear();
        }
    }
    fn call(&mut self, name: &str, args: Value) -> Result<(Value, f64)> {
        let started = Instant::now();
        let (response, _) = self.request("tools/call", json!({"name":name,"arguments":args}))?;
        let payload: Value = serde_json::from_str(response["result"]["content"][0]["text"].as_str().context("missing MCP text")?)?;
        Ok((payload, started.elapsed().as_secs_f64() * 1000.))
    }
}
impl Drop for McpClient { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

fn steps(inbound: bool) -> String {
    let (from, to) = if inbound { ("dst_public_id", "src_public_id") } else { ("src_public_id", "dst_public_id") };
    format!("SELECT e.{to},w.depth+1 FROM walk w JOIN edges e ON e.{from}=w.id WHERE w.depth<?2 AND e.kind IN('calls','inherits','implements','mutates','handles','references','imports','contains','documents') UNION SELECT e.{from},w.depth+1 FROM walk w JOIN edges e ON e.{to}=w.id WHERE w.depth<?2 AND e.kind='decorates'")
}

// Test-only reference for the previous count-then-page traversal.
fn repeated_reachability(conn: &Connection, selector: &str, depth: u32, inbound: bool, options: &QueryOptions) -> Result<Value> {
    ensure!(depth <= 16, "depth must be <=16");
    QueryOptions::resolve(&ResponsePolicy::default(), Some(options.limit), options.offset, Some(options.detail), options.generation.clone())?;
    let generation: String = conn.query_row("SELECT value FROM metadata WHERE key='output_root'", [], |r| r.get(0))?;
    ensure!(options.generation.as_ref().is_none_or(|g| g == &generation), "index generation changed; restart at offset 0 without generation");
    let mut node = reader::select(conn, selector)?;
    if options.detail == Detail::Compact {
        node.as_object_mut().context("invalid selected node")?.retain(|key, _| ["id", "kind", "name", "path", "line", "end_line", "language"].contains(&key.as_str()));
    }
    let id = node["id"].as_str().context("selected node has no id")?;
    if depth > 1 {
        let (forward, reverse) = if inbound { ("dst_public_id", "src_public_id") } else { ("src_public_id", "dst_public_id") };
        let links = reader::rows(conn, &format!("SELECT (SELECT count(*) FROM edges WHERE {forward}=?1 AND kind IN('calls','inherits','implements','mutates','handles','references','imports','contains','documents'))+(SELECT count(*) FROM edges WHERE {reverse}=?1 AND kind='decorates') count"), &[&id], 1)?;
        ensure!(links[0]["count"].as_u64().context("invalid link count")? <= 1000, "retry with depth=1");
    }
    let columns = match options.detail {
        Detail::Full => "n.*",
        Detail::Compact => "n.id,n.kind,n.name,n.path,n.line,n.end_line,n.language",
    };
    let sql = format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {}), reached(id,distance) AS (SELECT id,min(depth) FROM walk GROUP BY id) SELECT {columns},r.distance FROM reached r JOIN nodes n ON n.id=r.id ORDER BY r.distance,n.id", steps(inbound));
    let count = reader::rows(conn, &format!("SELECT count(*) total FROM ({sql})"), &[&id, &depth], 1)?;
    let total = count[0]["total"].as_u64().context("missing total")? as usize;
    let items = reader::rows(conn, &format!("SELECT * FROM ({sql}) LIMIT ?3 OFFSET ?4"), &[&id, &depth, &(options.limit as i64), &(options.offset as i64)], options.limit)?;
    let next = options.offset.saturating_add(items.len());
    let has_more = next < total;
    Ok(json!({"selector":node,"direction":if inbound {"inbound"} else {"outbound"},"depth":depth,"nodes":{
        "total":total,"offset":options.offset,"limit":options.limit,"has_more":has_more,
        "next_offset":if has_more {Some(next)} else {None},"items":items,"generation":generation,
        "continuation_hint":if has_more {Some("Repeat the same query with next_offset as offset and this generation.")} else {None}
    }}))
}

fn add_node(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details) VALUES(?1,'function',?1,?1,'fixture.py',1,1,0,'python',0,'{}')", [id])?;
    Ok(())
}
fn add_edge(conn: &Connection, from: &str, to: &str, kind: &str) -> Result<()> {
    conn.execute("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) VALUES(?1,?2,?3,'fixture.py',1,'fixture','exact',1)", params![from, to, kind])?;
    Ok(())
}
fn synthetic(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    for id in ["a", "b", "c", "d", "decorator", "wide"] { add_node(&tx, id)?; }
    for (from,to) in [("a","b"),("a","c"),("b","d"),("c","d"),("d","a")] { add_edge(&tx,from,to,"calls")?; }
    add_edge(&tx, "decorator", "b", "decorates")?;
    for i in 0..1001 { let id = format!("wide{i:04}"); add_node(&tx,&id)?; add_edge(&tx,"wide",&id,"calls")?; }
    for i in 0..600 { add_node(&tx,&format!("mesh{i:04}"))?; }
    for i in 0..600 { for hop in 1..=20 { add_edge(&tx,&format!("mesh{i:04}"),&format!("mesh{:04}",(i+hop)%600),"calls")?; } }
    tx.commit()?;
    Ok(())
}

fn sqlite_memory() -> (i64, i64) {
    let (mut current, mut high) = (0, 0);
    unsafe { rusqlite::ffi::sqlite3_status64(rusqlite::ffi::SQLITE_STATUS_MEMORY_USED, &mut current, &mut high, 0); }
    (current, high)
}
fn reset_memory_peak() {
    let (mut current, mut high) = (0, 0);
    unsafe { rusqlite::ffi::sqlite3_status64(rusqlite::ffi::SQLITE_STATUS_MEMORY_USED, &mut current, &mut high, 1); }
}

fn without_explanations(mut output: Value) -> Value {
    if let Some(items) = output["nodes"]["items"].as_array_mut() {
        for item in items {
            if let Some(item) = item.as_object_mut() {
                item.remove("is_seed");
                item.remove("via");
            }
        }
    }
    output
}

fn measure(conn: &Connection, scope: &str, selector: &str, depth: u32, inbound: bool, offset: usize, detail: Detail) -> Result<Value> {
    let options = QueryOptions::resolve(&ResponsePolicy::default(), Some(20), offset, Some(detail), (offset != 0).then(|| "profile".into()))?;
    let baseline = repeated_reachability(conn, selector, depth, inbound, &options);
    let candidate = traversal::traverse_paged(conn, selector, depth, inbound, &options);
    match (baseline, candidate) {
        (Ok(base), Ok(candidate)) => {
            ensure!(base == without_explanations(candidate.clone()), "different output: scope={scope} selector={selector} depth={depth} offset={offset}");
            if scope == "synthetic" && selector == "a" && depth == 16 && !inbound && offset == 0 {
                ensure!(base["nodes"]["total"] == 5, "cycle or decorator direction changed reachability");
                let distances: Vec<_> = base["nodes"]["items"].as_array().context("missing items")?.iter().map(|n| (n["id"].clone(),n["distance"].clone())).collect();
                ensure!(distances == vec![(json!("a"),json!(0)),(json!("b"),json!(1)),(json!("c"),json!(1)),(json!("d"),json!(2)),(json!("decorator"),json!(2))], "minimum distance or ordering changed");
            }
            let mut times = [Vec::new(), Vec::new()];
            let mut memory = [[0_i64; 3]; 2];
            for repetition in 0..7 {
                for implementation in if repetition % 2 == 0 { [0,1] } else { [1,0] } {
                    let before = sqlite_memory().0;
                    reset_memory_peak();
                    let start = Instant::now();
                    let output = if implementation == 0 { repeated_reachability(conn,selector,depth,inbound,&options)? } else { traversal::traverse_paged(conn,selector,depth,inbound,&options)? };
                    let encoded = serde_json::to_vec(&output)?;
                    std::hint::black_box(encoded);
                    times[implementation].push(start.elapsed().as_secs_f64()*1000.);
                    let (after, peak) = sqlite_memory();
                    memory[implementation][0] = memory[implementation][0].max(after);
                    memory[implementation][1] = memory[implementation][1].max(peak);
                    memory[implementation][2] = memory[implementation][2].max(peak-before);
                }
            }
            for values in &mut times { values.sort_by(f64::total_cmp); }
            Ok(json!({"scope":scope,"selector":selector,"depth":depth,"inbound":inbound,"offset":offset,"detail":format!("{detail:?}"),"equal":true,"total":base["nodes"]["total"],"page_items":base["nodes"]["items"].as_array().map(Vec::len),"response_bytes":serde_json::to_vec(&base)?.len(),"baseline_median_ms":times[0][3],"candidate_median_ms":times[1][3],"baseline_min_ms":times[0][0],"candidate_min_ms":times[1][0],"speedup":times[0][3]/times[1][3],"sqlite_memory_bytes":{"baseline_current_peak_incremental":memory[0],"candidate_current_peak_incremental":memory[1]}}))
        }
        (Err(base), Err(candidate)) => Ok(json!({"scope":scope,"selector":selector,"depth":depth,"inbound":inbound,"offset":offset,"baseline_error":base.to_string(),"candidate_error":candidate.to_string()})),
        (base,candidate) => Ok(json!({"scope":scope,"selector":selector,"depth":depth,"inbound":inbound,"offset":offset,"equal":false,"baseline_error":base.as_ref().err().map(ToString::to_string),"candidate_error":candidate.as_ref().err().map(ToString::to_string),"baseline_total":base.as_ref().ok().map(|v|v["nodes"]["total"].clone()),"candidate_total":candidate.as_ref().ok().map(|v|v["nodes"]["total"].clone())})),
    }
}

#[test]
fn materialized_reachability_preserves_cycles_pages_and_admission() -> Result<()> {
    let workspace = Workspace::new()?;
    let mut conn = workspace.database("contract.sqlite")?;
    synthetic(&mut conn)?;
    conn.execute_batch("PRAGMA query_only=ON")?;
    for depth in [0,1,16] {
        for inbound in [false,true] {
            for offset in [0,2,9999] {
                for detail in [Detail::Compact,Detail::Full] {
                    let options = QueryOptions::resolve(&ResponsePolicy::default(),Some(2),offset,Some(detail),(offset!=0).then(||"profile".into()))?;
                    ensure!(without_explanations(traversal::traverse_paged(&conn,"a",depth,inbound,&options)?) == repeated_reachability(&conn,"a",depth,inbound,&options)?, "different reachability/page");
                }
            }
        }
    }
    let options = QueryOptions::resolve(&ResponsePolicy::default(),Some(20),0,None,None)?;
    for (selector,depth) in [("wide",16),("a",17)] {
        ensure!(traversal::traverse_paged(&conn,selector,depth,false,&options).is_err(), "baseline missing guard");
        ensure!(repeated_reachability(&conn,selector,depth,false,&options).is_err(), "reference missing guard");
    }
    let stale = QueryOptions {generation:Some("stale".into()),..options};
    ensure!(traversal::traverse_paged(&conn,"a",1,false,&stale).is_err(), "baseline accepts stale generation");
    ensure!(repeated_reachability(&conn,"a",1,false,&stale).is_err(), "reference accepts stale generation");
    Ok(())
}

#[test]
#[ignore = "isolated traversal benchmark; run serially in release mode"]
fn compare_repeated_and_materialized_reachability() -> Result<()> {
    let workspace = Workspace::new()?;
    let mut conn = workspace.database("synthetic.sqlite")?;
    synthetic(&mut conn)?;
    conn.execute_batch("PRAGMA query_only=ON")?;
    let mut results = Vec::new();
    for (selector, depth, inbound, offset, detail) in [
        ("a",0,false,0,Detail::Compact), ("a",1,false,0,Detail::Compact),
        ("a",16,false,0,Detail::Compact), ("a",16,true,0,Detail::Full),
        ("a",16,false,2,Detail::Compact), ("a",16,false,9999,Detail::Compact),
        ("wide",1,false,0,Detail::Compact), ("wide",16,false,0,Detail::Compact),
        ("mesh0000",4,false,0,Detail::Compact), ("mesh0000",16,false,0,Detail::Compact),
        ("mesh0000",16,false,100,Detail::Compact), ("a",17,false,0,Detail::Compact),
    ] {
        let result = measure(&conn,"synthetic",selector,depth,inbound,offset,detail)?;
        println!("TRAVERSAL_PROFILE {result}"); results.push(result);
    }
    drop(conn);
    if let Ok(source) = std::env::var("FORGE_TRAVERSAL_DB") {
        let conn = workspace.database("real.sqlite")?;
        let source = PathBuf::from(source).canonicalize()?;
        let source = format!("file:{}?mode=ro",source.display());
        conn.execute("ATTACH DATABASE ?1 AS original", [&source])?;
        conn.execute_batch("BEGIN; INSERT INTO nodes SELECT * FROM original.nodes; INSERT INTO edges SELECT * FROM original.edges; COMMIT; DETACH DATABASE original; PRAGMA query_only=ON;")?;
        let selectors = reader::rows(&conn, "SELECT n.id FROM nodes n WHERE n.name IN('ChannelSyncEntry','resolve_conflict','products_detail_api','tool_node') ORDER BY n.id LIMIT 4", &[], 4)?;
        for node in selectors {
            let selector = node["id"].as_str().context("missing selector")?;
            for depth in [1,4,16] {
                let result = measure(&conn,"real",selector,depth,true,0,Detail::Compact)?;
                println!("TRAVERSAL_PROFILE {result}"); results.push(result);
            }
        }
        let counts = reader::rows(&conn,"SELECT (SELECT count(*) FROM nodes) nodes,(SELECT count(*) FROM edges) edges",&[],1)?;
        println!("TRAVERSAL_GRAPH {}",counts[0]);
    }
    let report = json!({"method":"Warm alternating 7 repetitions: previous two-walk count/page reference versus production one-materialized-walk count/page/reason query, including JSON serialization. SQLite allocator counters are process-wide, not total Rust RSS. Generation admission and source copying are excluded equally. Recursive walk still retains (id,depth) states.","results":results});
    if let Ok(report_path) = std::env::var("FORGE_TRAVERSAL_REPORT") { fs::write(report_path,serde_json::to_vec_pretty(&report)?)?; }
    Ok(())
}

fn workflow_fixture(workspace: &Workspace) -> Result<()> {
    let mut graph = String::new();
    for i in 0..400 {
        graph.push_str(&format!("def fn_{i:03}():\n"));
        for hop in 1..=8 {
            graph.push_str(&format!("    fn_{:03}()\n", (i + hop) % 400));
        }
        graph.push('\n');
    }
    fs::write(workspace.0.join("graph.py"), graph)?;
    for i in 0..80 {
        fs::write(workspace.0.join(format!("leaf_{i:03}.py")), format!("def leaf_{i:03}():\n    return {i}\n"))?;
    }
    fs::write(workspace.0.join("chain.py"), "def chain_a():\n    chain_b()\ndef chain_b():\n    chain_c()\ndef chain_c():\n    pass\n")?;
    writer::build(&workspace.0, &workspace.0.join(".forge/code-map.sqlite"), None)?;
    Ok(())
}

fn workflow(client: &mut McpClient, depth: u32, selector: &str) -> Result<(Vec<Value>, Vec<f64>)> {
    let pattern = selector.rsplit(':').next().context("invalid benchmark selector")?;
    let cases = [
        ("code_map_overview", json!({"detail":"compact","limit":1})),
        ("code_map_search", json!({"pattern":pattern,"detail":"compact","limit":10})),
        ("code_map_inspect", json!({"selector":selector,"detail":"compact","show_doc":false,"show_source":false,"limit":10})),
        ("code_map_impact", json!({"selector":selector,"depth":depth,"detail":"compact","limit":20})),
    ];
    let mut outputs = Vec::new();
    let mut durations = Vec::new();
    for (name, arguments) in cases {
        let (result, elapsed_ms) = client.call(name, arguments)?;
        outputs.push(result);
        durations.push(elapsed_ms);
    }
    Ok((outputs, durations))
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn impact_page_identity(output: &Value) -> Result<Value> {
    let page = &output["nodes"];
    let items = page["items"].as_array().context("impact page is missing")?;
    Ok(json!({"total":page["total"],"offset":page["offset"],"items":items.iter().map(|node| json!({"id":node["id"],"distance":node["distance"]})).collect::<Vec<_>>()}))
}

#[test]
#[ignore = "release-only paired MCP transport benchmark; set FORGE_WORKFLOW_BASELINE and FORGE_WORKFLOW_CANDIDATE"]
fn compare_full_mcp_agent_workflow() -> Result<()> {
    let baseline_binary = std::env::var("FORGE_WORKFLOW_BASELINE")?;
    let candidate_binary = std::env::var("FORGE_WORKFLOW_CANDIDATE")?;
    let depth = std::env::var("FORGE_WORKFLOW_DEPTH").unwrap_or_else(|_| "16".into()).parse::<u32>()?;
    let selector = std::env::var("FORGE_WORKFLOW_SELECTOR").unwrap_or_else(|_| "graph.py:fn_250".into());
    ensure!([1,16].contains(&depth), "benchmark depth must be 1 or 16");
    ensure!(["graph.py:fn_250","leaf_000.py:leaf_000","chain.py:chain_a"].contains(&selector.as_str()),"benchmark selector must be a fixture selector");
    let workspaces = [Workspace::new()?, Workspace::new()?];
    workflow_fixture(&workspaces[0])?;
    workflow_fixture(&workspaces[1])?;
    let mut clients = [
        McpClient::new(&baseline_binary, &workspaces[0].0)?,
        McpClient::new(&candidate_binary, &workspaces[1].0)?,
    ];
    for _ in 0..3 {
        let (baseline, _) = workflow(&mut clients[0], depth, &selector)?;
        let (candidate, _) = workflow(&mut clients[1], depth, &selector)?;
        ensure!(impact_page_identity(&baseline[3])? == impact_page_identity(&candidate[3])?, "MCP impact reachability differs during warmup");
    }
    let mut per_call = [vec![Vec::<f64>::new(); 4], vec![Vec::<f64>::new(); 4]];
    let mut per_workflow = [Vec::<f64>::new(), Vec::<f64>::new()];
    let mut overview_inventory_scan_ms = [Vec::<Value>::new(),Vec::<Value>::new()];
    let mut overview_refresh = [Vec::<Value>::new(),Vec::<Value>::new()];
    let mut count = Value::Null;
    for round in 0..15 {
        let order = if round % 2 == 0 { [0, 1] } else { [1, 0] };
        let mut outputs = [None, None];
        for implementation in order {
            let (values, durations) = workflow(&mut clients[implementation], depth, &selector)?;
            ensure!(values[0]["freshness"]["refresh"] == "none", "benchmark index rebuilt during timed round");
            if implementation == 0 {
                count = values[3]["nodes"]["total"].clone();
            }
            overview_inventory_scan_ms[implementation].push(values[0]["freshness"]["inventory_scan_ms"].clone());
            overview_refresh[implementation].push(values[0]["freshness"]["refresh"].clone());
            per_workflow[implementation].push(durations.iter().sum::<f64>());
            for (index, duration) in durations.into_iter().enumerate() {
                per_call[implementation][index].push(duration);
            }
            outputs[implementation] = Some(values);
        }
        ensure!(impact_page_identity(&outputs[0].as_ref().context("missing baseline")?[3])? == impact_page_identity(&outputs[1].as_ref().context("missing candidate")?[3])?, "MCP impact reachability differs in round {round}");
    }
    ensure!(count.as_u64().is_some_and(|n| n > if selector.starts_with("leaf_") { 0 } else if selector.starts_with("chain.") { 2 } else if depth == 1 { 1 } else { 20 }), "fixture did not exercise expected graph depth");
    let names = ["overview", "search", "inspect", "impact"];
    let calls: Vec<_> = names.iter().enumerate().map(|(index, name)| json!({
        "name": name,
        "baseline_median_ms": median(per_call[0][index].clone()),
        "candidate_median_ms": median(per_call[1][index].clone()),
        "baseline_samples_ms": per_call[0][index],
        "candidate_samples_ms": per_call[1][index],
    })).collect();
    let report = json!({
        "method": "Two long-lived stdio MCP servers, separate identical temporary source fixtures and indexes, identical four-call requests, 3 warmups then 15 paired alternating-order rounds. Timed from client call start to parsed tool payload: transport, parameter parsing, full admission and source scan, selector, SQL/pagination, response serialization and client JSON parsing. Startup and cold build excluded. Timed rounds require refresh=none. Impact page IDs, distances, total and offset compared; new explanatory fields and unrelated tool output revisions are allowed.",
        "fixture": {"source_files":82,"graph_functions":400,"calls_per_graph_function":8,"selector":selector,"impact_depth":depth,"impact_total":count},
        "baseline_workflow_median_ms":median(per_workflow[0].clone()),
        "candidate_workflow_median_ms":median(per_workflow[1].clone()),
        "baseline_workflow_samples_ms":per_workflow[0],
        "candidate_workflow_samples_ms":per_workflow[1],
        "overview_inventory_scan_ms":{"baseline":overview_inventory_scan_ms[0],"candidate":overview_inventory_scan_ms[1]},
        "overview_refresh":{"baseline":overview_refresh[0],"candidate":overview_refresh[1]},
        "calls":calls,
    });
    println!("MCP_WORKFLOW_PROFILE {report}");
    if let Ok(path) = std::env::var("FORGE_WORKFLOW_REPORT") {
        fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(())
}
