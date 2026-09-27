#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::db::{reader, writer};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
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
