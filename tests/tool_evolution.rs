#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::db::{reader, writer};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("forge_evolution_{}_{nonce}", std::process::id()));
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
            assert!(self.output.read_line(&mut line).unwrap() > 0, "MCP closed stdout on {method}");
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
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap()
    }
    fn err(&mut self, name: &str, arguments: Value, expected: &str) {
        let response = self.call(name, arguments);
        assert_eq!(response["result"]["isError"], true, "{name}: {response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(message.contains(expected), "{name}: expected {expected:?}, got {message:?}");
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_exposes_and_executes_evolution_tools() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let w = Workspace::new();
    w.write("service.py", "def compute():\n    return 42\n");
    w.write(
        "tests/test_service.py",
        "from service import compute\ndef test_compute(): compute()\n",
    );
    w.build();
    let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .arg("--root")
        .arg(&w.0)
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    {
        let mut request = |id: u32, method: &str, params: Value| -> Value {
            writeln!(
                input,
                "{}",
                serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            )
            .unwrap();
            input.flush().unwrap();
            loop {
                let mut line = String::new();
                assert!(
                    output.read_line(&mut line).unwrap() > 0,
                    "MCP closed stdout"
                );
                let response: Value = serde_json::from_str(&line).unwrap();
                if response["id"] == id {
                    return response;
                }
            }
        };
        let init = request(
            1,
            "initialize",
            serde_json::json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        );
        assert!(init.get("error").is_none(), "{init}");
    }
    // The initialized notification has no response.
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    let cases = [
        (
            "get_code_snippet",
            serde_json::json!({"selector":"compute"}),
        ),
        (
            "code_map_inspect",
            serde_json::json!({"selector":"compute","show_source":true}),
        ),
        (
            "code_map_search",
            serde_json::json!({"pattern":"comp*","kind":"function"}),
        ),
        ("code_map_tests", serde_json::json!({"selector":"compute"})),
    ];
    for (i, (name, arguments)) in cases.into_iter().enumerate() {
        let id = i + 2;
        writeln!(input,"{}",serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        let payload: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        if i < 2 {
            assert_eq!(payload["source"], "def compute():\n    return 42\n");
        } else {
            assert!(!payload["nodes"]["items"].as_array().unwrap().is_empty());
            assert!(payload["nodes"]["total"].as_u64().unwrap() > 0);
            assert!(payload["nodes"]["generation"].is_string());
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn source_search_and_stale_source_through_cli() {
    let w = Workspace::new();
    let code = "def OrderRevert():\r\n    return 'Привіт'\r\n";
    w.write("service.py", code);
    w.build();
    let default = w.query(&["query", "inspect", "OrderRevert"]);
    assert!(default.get("source").is_none());
    let source = w.query(&["query", "inspect", "OrderRevert", "--show-source"]);
    assert_eq!(source["source"], code);
    for pattern in ["Order*", "*Revert*", "OrderRevert"] {
        let found = w.query(&["query", "search", pattern, "--kind", "function"]);
        assert_eq!(found["nodes"].as_array().unwrap().len(), 1);
    }
    w.write("service.py", "def OrderRevert():\n    return 'changed'\n");
    assert!(!w
        .cli(&["query", "inspect", "OrderRevert", "--show-source"])
        .status
        .success());
}

fn edges(w: &Workspace) -> Vec<Value> {
    let c = reader::open(&w.db(), &w.0).unwrap();
    reader::rows(&c, "SELECT a.name src,b.name dst,e.kind FROM edges e JOIN nodes a ON a.id=e.src_public_id JOIN nodes b ON b.id=e.dst_public_id", &[], 1000).unwrap()
}
fn has_edge(rows: &[Value], src: &str, dst: &str, kind: &str) -> bool {
    rows.iter()
        .any(|r| r["src"] == src && r["dst"] == dst && r["kind"] == kind)
}

#[test]
#[cfg(all(feature = "lang-typescript", feature = "lang-rust"))]
fn rich_edges_routes_and_delta_persisted() {
    let w = Workspace::new();
    let code = "def decorate(cls):\n    return cls\nclass Base: pass\n@decorate\nclass Child(Base):\n    count = 0\n    def update(self):\n        self.count = 1\ndef handler(): pass\napp.get('/items', handler)\npath('items/', handler)\n@app.post('/new')\ndef create(): pass\n";
    w.write("app.py", code);
    w.write("app.ts", "interface Store {}\nclass Parent {}\nclass ChildTS extends Parent implements Store {}\nfunction view() {}\nconst routes = [{path: '/page', component: view}];\napp.get('/ts', view);\n");
    w.write(
        "app.rs",
        "trait Save {}\nstruct Record {}\nimpl Save for Record {}\n",
    );
    w.write("routes.js", "class JsBase {}\nclass JsChild extends JsBase {}\nfunction jsview() {}\napp.put('/js', jsview);\n");
    w.write("fields.ts", "function logged(value: any) { return value; }\n@logged\nclass Box {\n count = 0;\n @Get('/box')\n show() { this.count += 1; }\n}\n");
    w.build();
    let rows = edges(&w);
    for (src, dst, kind) in [
        ("JsChild", "JsBase", "inherits"),
        ("GET /box", "show", "handles"),
        ("show", "count", "mutates"),
        ("logged", "Box", "decorates"),
        ("PUT /js", "jsview", "handles"),
        ("Child", "Base", "inherits"),
        ("decorate", "Child", "decorates"),
        ("update", "count", "mutates"),
        ("ChildTS", "Parent", "inherits"),
        ("ChildTS", "Store", "implements"),
        ("Record", "Save", "implements"),
        ("GET /items", "handler", "handles"),
        ("ANY items/", "handler", "handles"),
        ("POST /new", "create", "handles"),
        ("ANY /page", "view", "handles"),
        ("GET /ts", "view", "handles"),
    ] {
        assert!(
            has_edge(&rows, src, dst, kind),
            "missing {src} -{kind}-> {dst}: {rows:?}"
        );
    }
    w.write("app.py", &code.replace("'/items'", "'/changed'"));
    writer::delta(&w.0, &w.db(), &[Path::new("app.py").to_owned()]).unwrap();
    let delta_edges = edges(&w);
    assert!(has_edge(&delta_edges, "GET /changed", "handler", "handles"));
    assert!(!has_edge(&delta_edges, "GET /items", "handler", "handles"));
    w.build();
    let mut delta_sorted: Vec<_> = delta_edges.iter().map(Value::to_string).collect();
    let mut full_sorted: Vec<_> = edges(&w).iter().map(Value::to_string).collect();
    delta_sorted.sort();
    full_sorted.sort();
    assert_eq!(delta_sorted, full_sorted);
}

#[test]
fn transitive_tests_exclude_unrelated_and_support_reverse_direction() {
    let w = Workspace::new();
    w.write(
        "service.py",
        "def target(): pass\ndef wrapper(): target()\n",
    );
    w.write(
        "tests/test_service.py",
        "from service import wrapper\ndef test_target(): wrapper()\ndef test_unrelated(): pass\n",
    );
    w.build();
    let tests = w.query(&["query", "tests", "target"]);
    let nodes = tests["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["name"] == "test_target"));
    assert!(!nodes.iter().any(|n| n["name"] == "test_unrelated"));
    let production = w.query(&["query", "tests", "test_target", "--direction", "outbound"]);
    assert!(production["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "target"));
}

#[test]
fn linked_source_is_exact_and_rejects_symlinks_and_traversal() {
    use contextunity_forge_mcp::{db::symbols, engine::scanner};
    let w = Workspace::new();
    let linked = Workspace::new();
    linked.write("lib.py", "def linked_fn():\n    return 7\n");
    w.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: shared\n    path: {}\n",
            linked.0.display()
        ),
    );
    w.build();
    let c = reader::open(&w.db(), &w.0).unwrap();
    let source = symbols::inspect(&c, &w.0, "linked_fn", false, true).unwrap();
    assert_eq!(source["source"], "def linked_fn():\n    return 7\n");
    let adapter = scanner::load_adapter(&w.0, None).unwrap();
    assert!(scanner::resolve_file_path(&w.0, &adapter, "[shared]/../escape.py").is_err());
    #[cfg(unix)]
    {
        fs::rename(linked.0.join("lib.py"), linked.0.join("actual.py")).unwrap();
        std::os::unix::fs::symlink("actual.py", linked.0.join("lib.py")).unwrap();
        assert!(symbols::inspect(&c, &w.0, "linked_fn", false, true).is_err());
    }
}

#[test]
fn queries_bound_results_and_indexes_cover_edge_predicates() {
    use contextunity_forge_mcp::db::symbols;
    let w = Workspace::new();
    w.write(
        "lib.py",
        "def OrderOne(): pass\ndef OrderTwo(): pass\ndef get_item(): pass\ndef getXitem(): pass\n",
    );
    w.build();
    let c = reader::open(&w.db(), &w.0).unwrap();
    let found = symbols::search(&c, "Order*", Some("function"), 1).unwrap();
    assert_eq!(found["truncated"], true);
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);
    let found = symbols::search(&c, "*get_item*", Some("function"), 10).unwrap();
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);
    for pattern in ["", "***"] {
        assert!(symbols::search(&c, pattern, None, 10).is_err());
    }
    assert!(symbols::search(&c, "Order*", None, 0).is_err());
    assert!(symbols::tests(&c, "OrderOne", "invalid", 10).is_err());
    for (column, index) in [
        ("src_public_id", "idx_edges_src_kind"),
        ("dst_public_id", "idx_edges_dst_kind"),
    ] {
        let plans = reader::rows(
            &c,
            &format!("EXPLAIN QUERY PLAN SELECT 1 FROM edges WHERE {column}='id' AND kind='calls'"),
            &[],
            10,
        )
        .unwrap();
        assert!(
            plans
                .iter()
                .any(|p| p["detail"].as_str().unwrap().contains(index)),
            "{plans:?}"
        );
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn rust_test_attributes_and_cyclic_python_dependencies() {
    let w = Workspace::new();
    w.write(
        "lib.rs",
        "fn compute() {}\n#[test]\nfn checks_compute() { compute(); }\nfn unrelated() {}\n",
    );
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write(
        "tests/test_cycle.py",
        "from cycle import a\ndef test_cycle(): a()\n",
    );
    w.build();
    let tests = w.query(&["query", "tests", "compute"]);
    assert!(tests["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "checks_compute"));
    let tests = w.query(&["query", "tests", "a"]);
    assert_eq!(tests["nodes"].as_array().unwrap().len(), 1);
}

#[test]
fn imported_relationships_relink_when_target_changes() {
    let w = Workspace::new();
    w.write(
        "base.py",
        "class Base: pass\ndef decorate(cls): return cls\ndef handler(): pass\n",
    );
    w.write("routes.py", "from base import Base, decorate, handler\n@decorate\nclass Child(Base): pass\napp.get('/alias', handler)\n@app.route('/multi', methods=['GET', 'POST'])\ndef multi(): pass\napp.get(dynamic_path, handler)\n");
    w.build();
    let rows = edges(&w);
    for (src, dst, kind) in [
        ("Child", "Base", "inherits"),
        ("decorate", "Child", "decorates"),
        ("GET /alias", "handler", "handles"),
        ("GET /multi", "multi", "handles"),
        ("POST /multi", "multi", "handles"),
    ] {
        assert!(
            has_edge(&rows, src, dst, kind),
            "missing {src} -{kind}-> {dst}"
        );
    }
    let impact = w.query(&["query", "impact", "Base"]);
    assert!(impact["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "Child"));
    w.write(
        "base.py",
        "class Base: pass\ndef decorate(cls): return cls\n",
    );
    writer::delta(&w.0, &w.db(), &[PathBuf::from("base.py")]).unwrap();
    assert!(!has_edge(&edges(&w), "GET /alias", "handler", "handles"));
    let c = reader::open(&w.db(), &w.0).unwrap();
    let unresolved = reader::rows(&c, "SELECT * FROM resolution_coverage WHERE path='routes.py' AND expression='handler' AND status='unresolved'", &[], 100).unwrap();
    assert!(!unresolved.is_empty());
}

#[test]
#[cfg(feature = "lang-rust")]
fn audit_findings_covered_and_verified() {
    // 1. Unknown receiver call does not falsely declare method safe to remove
    let w1 = Workspace::new();
    w1.write(
        "app.py",
        "class Service:\n    def get(self): return 1\ndef use(service):\n    return service.get()\n",
    );
    w1.build();
    let removal = w1.query(&["query", "remove", "py:app.py:2:get"]);
    assert_eq!(
        removal["safe_to_remove"], false,
        "method called on unknown receiver must not be safe_to_remove"
    );

    // 2. Decorator tests inbound traversal connects to tests of decorated functions
    let w2 = Workspace::new();
    w2.write(
        "app.py",
        "def deco(fn): return fn\n@deco\ndef target(): pass\n",
    );
    w2.write(
        "tests/test_app.py",
        "from app import target\ndef test_target(): target()\n",
    );
    w2.build();
    let deco_tests = w2.query(&["query", "tests", "py:app.py:1:deco"]);
    let nodes = deco_tests["nodes"].as_array().unwrap();
    assert!(
        nodes.iter().any(|n| n["name"] == "test_target"),
        "deco must transitively find tests of decorated target: {nodes:?}"
    );

    // 3. Generic impl Record<T> links struct Record, not impl block
    let w3 = Workspace::new();
    w3.write(
        "lib.rs",
        "trait Save {}\nstruct Record<T> { value: T }\nimpl<T> Save for Record<T> {}\n",
    );
    w3.build();
    let conn = reader::open(&w3.db(), &w3.0).unwrap();
    let rows = reader::rows(
        &conn,
        "SELECT n1.name as src_name, n1.kind as src_kind, e.kind, n2.name as dst_name FROM edges e JOIN nodes n1 ON n1.id=e.src_public_id JOIN nodes n2 ON n2.id=e.dst_public_id WHERE e.kind='implements'",
        &[],
        10,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["src_name"], "Record");
    assert_eq!(rows[0]["src_kind"], "struct");
    assert_eq!(rows[0]["dst_name"], "Save");

    // 4. slice --limit 1 respects the limit
    let w4 = Workspace::new();
    w4.write("app.py", "def a(): b()\ndef b(): c()\ndef c(): return 1\n");
    w4.build();
    let slice = w4.query(&[
        "query",
        "run",
        "slice",
        "py:app.py:1:a",
        "--depth",
        "3",
        "--limit",
        "1",
    ]);
    assert_eq!(
        slice["nodes"].as_array().unwrap().len(),
        1,
        "slice must respect limit=1"
    );
    assert_eq!(slice["truncated"], true);

    // 5 & 6. analyze path-filtered cycles and truncated reporting
    let w5 = Workspace::new();
    w5.write("a.py", "def f(): pass\n");
    w5.write(
        "other.py",
        "def cycle_a(): cycle_b()\ndef cycle_b(): cycle_a()\n",
    );
    w5.build();
    let analyze_a = w5.query(&["query", "analyze", "a.py"]);
    assert_eq!(
        analyze_a["cycles"].as_array().unwrap().len(),
        0,
        "cycles in other.py must not be returned for a.py"
    );
    let analyze_all = w5.query(&["query", "analyze", ""]);
    assert!(
        !analyze_all["cycles"].as_array().unwrap().is_empty(),
        "global cycles must be returned when target is empty"
    );
}

#[test]
fn test_safe_selector_resolution_and_disambiguation() {
    let w = Workspace::new();
    w.write("scanner.py", "def scanner():\n    return 42\n");
    w.build();

    // 1. Path-like selector picks the module node
    let inspect_file = w.query(&["query", "inspect", "scanner.py"]);
    assert_eq!(
        inspect_file["node"]["kind"], "module",
        "path-like selector 'scanner.py' must resolve to module node"
    );

    // 2. path:symbol resolves to the function inside the file
    let inspect_path_sym = w.query(&["query", "inspect", "scanner.py:scanner"]);
    assert_eq!(
        inspect_path_sym["node"]["kind"], "function",
        "path:symbol 'scanner.py:scanner' must resolve to function node"
    );

    // 3. path:line resolves to the function containing line 1
    let inspect_path_line = w.query(&["query", "inspect", "scanner.py:1"]);
    assert_eq!(
        inspect_path_line["node"]["kind"], "function",
        "path:line 'scanner.py:1' must resolve to function node at that line"
    );

    // 4. path#Lline resolves to the function containing line 1
    let inspect_path_hash = w.query(&["query", "inspect", "scanner.py#L1"]);
    assert_eq!(
        inspect_path_hash["node"]["kind"], "function",
        "path#Lline 'scanner.py#L1' must resolve to function node at that line"
    );

    // 5. Bare name 'scanner' matches BOTH module 'scanner.py' and function 'scanner()'.
    // Safe disambiguation MUST NOT hijack it to module! It must fail with ambiguous selector.
    let bare_cmd = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(&["--root", w.0.to_str().unwrap(), "query", "inspect", "scanner"])
        .output()
        .unwrap();
    assert!(
        !bare_cmd.status.success(),
        "bare selector 'scanner' matching both module and function must not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&bare_cmd.stderr);
    assert!(
        stderr.contains("ambiguous selector scanner"),
        "stderr must report ambiguous selector, got: {stderr}"
    );
}

#[test]
fn mcp_selector_and_source_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("scanner.py", "def scanner():\n    return 42\n");
    w.write("src/app.py", "# license\ndef first(): pass\ndef last(): pass\n");
    w.write("comments.py", "# license only\n# no code body\n");
    w.write("docs/contract.md", "# Contract\n\nA documented contract.\n");
    w.build();
    let mut m = Mcp::new(&w);

    for tool in ["code_map_inspect", "code_map_explain", "code_map_impact", "code_map_tests", "code_map_prove_removal", "get_code_snippet"] {
        m.err(tool, json!({"selector":"scanner"}), "ambiguous selector");
        m.err(tool, json!({"selector":""}), "selector is empty");
        m.err(tool, json!({"selector":"   "}), "selector is empty");
        m.err(tool, json!({"selector":"docs/contract.md"}), "get_doc or search_docs");
    }
    for selector in ["scanner.py:scanner", "scanner.py::scanner", "scanner.py:1", "scanner.py#L1"] {
        let result = m.ok("code_map_inspect", json!({"selector":selector}));
        assert_eq!(result["node"]["kind"], "function", "{selector}: {result}");
    }
    for selector in ["file:src/app.py", "file://src/app.py", "./src/app.py"] {
        let result = m.ok("code_map_inspect", json!({"selector":selector}));
        assert_eq!(result["node"]["kind"], "module", "{selector}: {result}");
    }
    for selector in [":::", "#Labc", "nonexistent/path.py:9999"] {
        m.err("code_map_inspect", json!({"selector":selector}), "selector not found");
    }
    m.err("code_map_inspect", json!({"selector":"%.py:scanner"}), "selector not found");
    let first = m.ok("get_code_snippet", json!({"selector":"src/app.py:first","leading_lines":0}));
    assert_eq!(first["source"], "def first(): pass\n");
    let last = m.ok("get_code_snippet", json!({"selector":"src/app.py:last","leading_lines":0}));
    assert_eq!(last["source"], "def last(): pass\n");
    m.err("get_code_snippet", json!({"selector":"src/app.py:first","max_body_lines":0}), "max_body_lines");
    m.err("get_code_snippet", json!({"selector":"src/app.py:first","leading_lines":-1}), "expected usize");
    m.err("get_code_snippet", json!({"selector":"src/app.py:first","source_offset":-1}), "expected usize");
    let comments = m.ok("get_code_snippet", json!({"selector":"comments.py"}));
    assert_eq!(comments["node"]["kind"], "module");
    assert_eq!(comments["source"], "# license only\n# no code body\n");
}

#[test]
fn mcp_graph_depth_cycles_and_direction_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write("tests/test_cycle.py", "from cycle import a\ndef test_a(): a()\n");
    w.build();
    let mut m = Mcp::new(&w);
    for depth in [0, 1, 16] {
        let result = m.ok("code_map_impact", json!({"selector":"cycle.py:a","depth":depth}));
        assert_eq!(result["depth"], depth);
        assert!(result["nodes"]["total"].as_u64().unwrap() >= 1);
    }
    m.err("code_map_impact", json!({"selector":"cycle.py:a","depth":17}), "depth must be <=16");
    let inbound = m.ok("code_map_tests", json!({"selector":"cycle.py:a","direction":"inbound"}));
    assert!(inbound["nodes"]["items"].as_array().unwrap().iter().any(|node| node["name"] == "test_a"));
    let outbound = m.ok("code_map_tests", json!({"selector":"tests/test_cycle.py:test_a","direction":"outbound"}));
    assert!(outbound["nodes"]["items"].as_array().unwrap().iter().any(|node| node["name"] == "a"));
    m.err("code_map_tests", json!({"selector":"cycle.py:a","direction":"sideways"}), "direction must be inbound or outbound");
    let explained = m.ok("code_map_explain", json!({"selector":"cycle.py:a"}));
    assert_eq!(explained["node"]["kind"], "function");
    let removal = m.ok("code_map_prove_removal", json!({"selector":"cycle.py:a"}));
    assert_eq!(removal["safe_to_remove"], false);
}

#[test]
fn mcp_search_ast_and_document_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("src/app.py", "def item_name(): pass\ndef itemXname(): pass\nprint(item_name())\n");
    w.write("docs/intro.md", "---\ndoc_type: guide\ntitle: Intro\n---\n# Intro\n\nA searchable contract.\n");
    w.build();
    let mut m = Mcp::new(&w);
    let overview = m.ok("code_map_overview", json!({"limit":1}));
    assert!(overview["generation"].is_string());
    for pattern in ["*", "***", "_", "%", "\\", "'", "\""] {
        m.err("code_map_search", json!({"pattern":pattern}), "pattern must contain");
    }
    let prefix = m.ok("code_map_search", json!({"pattern":"item*","kind":"function","limit":1}));
    assert_eq!(prefix["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(prefix["nodes"]["total"], 2);
    let literal_underscore = m.ok("code_map_search", json!({"pattern":"item_*"}));
    assert_eq!(literal_underscore["nodes"]["total"], 1);
    for pattern in ["item%*", "item\\*", "item'*", "item\"*"] {
        let result = m.ok("code_map_search", json!({"pattern":pattern}));
        assert_eq!(result["nodes"]["total"], 0, "{pattern}: {result}");
    }

    let ast = m.ok("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","path":"src/app.py"}));
    assert!(ast["matches"]["items"].as_array().unwrap().len() >= 1);
    m.err("ast_grep_search", json!({"pattern":"def (","language":"python"}), "pattern");
    m.err("ast_grep_search", json!({"pattern":"print($VALUE)","language":"unknown"}), "unknown");
    m.err("ast_grep_search", json!({"pattern":"print($VALUE)","language":"python","path":""}), "path");

    let docs = m.ok("search_docs", json!({"query":"searchable"}));
    assert!(docs["sections"]["total"].as_u64().unwrap() >= 1);
    m.err("search_docs", json!({"query":""}), "search query is empty");
    let quoted = m.ok("search_docs", json!({"query":"\""}));
    assert_eq!(quoted["sections"]["total"], 0);
    let filtered = m.ok("search_docs", json!({"query":"searchable","doc_type":"no-such-type"}));
    assert_eq!(filtered["sections"]["total"], 0);
    let doc = m.ok("get_doc", json!({"path_or_id":"docs/intro.md"}));
    assert!(doc["sections"]["total"].as_u64().unwrap() >= 1);
    m.err("get_doc", json!({"path_or_id":"docs/intro.md","section":"NonExistentSection"}), "document or section not found");
}

#[test]
fn mcp_analyze_router_checkpoint_and_guide_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write("other.py", "def idle(): pass\n");
    w.build();
    let mut m = Mcp::new(&w);

    for target in [
        "; DROP TABLE nodes; --",
        "INSERT INTO files VALUES ('x')",
        "UPDATE nodes SET kind='hacked'",
        "ATTACH DATABASE '/tmp/pwn.db' AS pwn",
        "PRAGMA journal_mode=WAL",
    ] {
        let response = m.call("code_map_analyze", json!({"target":target}));
        assert_eq!(response["result"]["isError"], true, "unsafe target accepted: {target}: {response}");
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(message.contains("write or administrative SQL is not allowed") || message.contains("exactly one SELECT or WITH statement is allowed"), "{target}: {message}");
    }
    m.err("code_map_analyze", json!({"target":"SELECT count(*) FROM nodes; DROP TABLE nodes"}), "exactly one SELECT or WITH statement is allowed");
    let count = m.ok("code_map_analyze", json!({"target":"SELECT count(*) AS count FROM nodes","limit":1}));
    assert!(count["rows"]["items"][0]["count"].as_u64().unwrap() >= 2);
    let paged = m.ok("code_map_analyze", json!({"target":"SELECT id FROM nodes ORDER BY id","limit":1}));
    assert_eq!(paged["rows"]["items"].as_array().unwrap().len(), 1);
    assert!(paged["rows"]["total"].as_u64().unwrap() > 1);
    for target in ["", "cycle.py", "other.py"] {
        let off = m.ok("code_map_analyze", json!({"target":target,"include_cycles":false}));
        assert_eq!(off["cycles"]["omitted"], true);
        let on = m.ok("code_map_analyze", json!({"target":target,"include_cycles":true}));
        if target == "other.py" {
            assert_eq!(on["cycles"]["total"], 0);
        } else {
            assert!(on["cycles"]["total"].as_u64().unwrap() >= 1);
        }
    }

    for (operation, selector) in [
        ("overview", ""), ("inspect", "cycle.py:a"), ("explain", "cycle.py:a"),
        ("impact", "cycle.py:a"), ("slice", "cycle.py:a"), ("unwired", ""),
        ("cypher", "MATCH (n) RETURN n"), ("raw_cypher", "MATCH (n) RETURN n"),
        ("doctor", ""), ("search", "a"),
    ] {
        m.ok("code_map_query", json!({"operation":operation,"selector":selector,"depth":1,"limit":5}));
    }
    m.ok("code_map_query", json!({"operation":"MATCH (n) RETURN n","limit":5}));
    m.err("code_map_query", json!({"operation":"MATCH (n) RETURN n LIMIT 5"}), "Pass limit as a separate tool argument");
    m.err("code_map_query", json!({"operation":"delete_all"}), "supported: overview,inspect,explain,impact,slice,unwired,cypher");

    assert!(m.ok("session_checkpoint", json!({"action":"list"})).is_object());
    m.err("session_checkpoint", json!({"action":"save","content":{"note":"x"}}), "name");
    m.err("session_checkpoint", json!({"action":"save","name":"note"}), "content required");
    m.err("session_checkpoint", json!({"action":"get","name":"missing"}), "not found");
    m.err("session_checkpoint", json!({"action":"delete","name":"missing"}), "not found");
    m.err("session_checkpoint", json!({"action":"save","name":"../../etc/passwd","content":{"note":"x"}}), "path traversal");
    m.err("session_checkpoint", json!({"action":"save","name":"..\n/../etc/passwd","content":{"note":"x"}}), "path traversal");
    m.ok("session_checkpoint", json!({"action":"save","name":"note","content":{"state":1}}));
    assert_eq!(m.ok("session_checkpoint", json!({"action":"get","name":"note"}))["state"], 1);
    assert!(m.ok("session_checkpoint", json!({"action":"list"})).get("note").is_some());
    m.ok("session_checkpoint", json!({"action":"delete","name":"note"}));
    m.err("session_checkpoint", json!({"action":"get","name":"note"}), "not found");
    w.write(".forge/checkpoints.json", "{invalid JSON");
    m.err("session_checkpoint", json!({"action":"list"}), "key must be a string");

    let guide = m.ok("forge_guide", json!({"topic":"query"}));
    assert!(guide["start"].is_string());
    m.err("forge_guide", json!({"topic":"no-such-topic"}), "unknown guide topic");
}

#[test]
fn mcp_rejects_wide_graph_before_deep_traversal() {
    use serde_json::json;
    let w = Workspace::new();
    let mut code = String::from("def target(): pass\n");
    for index in 0..1001 {
        code.push_str(&format!("def caller_{index}(): target()\n"));
    }
    w.write("wide.py", &code);
    w.build();
    let mut m = Mcp::new(&w);
    m.err("code_map_impact", json!({"selector":"wide.py:target","depth":2,"limit":1}), "Retry with depth=1");
    let shallow = m.ok("code_map_impact", json!({"selector":"wide.py:target","depth":1,"limit":1}));
    assert!(shallow["nodes"]["total"].as_u64().unwrap() > 1000);
}

#[test]
fn mcp_rejects_removal_scope_over_ten_thousand_nodes() {
    use serde_json::json;
    let w = Workspace::new();
    let mut code = String::new();
    for index in 0..10001 {
        code.push_str(&format!("def item_{index}(): pass\n"));
    }
    w.write("scope.py", &code);
    w.build();
    let mut m = Mcp::new(&w);
    m.err("code_map_prove_removal", json!({"selector":"scope.py"}), "removal scope exceeds 10000 nodes");
}

#[test]
fn mcp_enforces_page_row_and_time_budgets() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("app.py", "def one(): pass\ndef two(): pass\n");
    w.build();
    let mut m = Mcp::new(&w);
    m.err("code_map_search", json!({"pattern":"one","limit":0}), "limit must be 1..=100");
    m.err("code_map_search", json!({"pattern":"one","limit":101}), "limit must be 1..=100");
    m.err("code_map_search", json!({"pattern":"one","offset":1}), "continuation requires generation");
    m.err("code_map_analyze", json!({"target":"SELECT hex(zeroblob(2000000)) AS giant FROM nodes LIMIT 1"}), "8 MiB row budget");
    m.err("code_map_analyze", json!({"target":"WITH RECURSIVE seq(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM seq WHERE x<100000000) SELECT max(x) FROM seq"}), "2-second SQLite budget");
}
