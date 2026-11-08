use super::*;
use crate::harness::McpTestHarness;

#[test]
fn mcp_and_cli_expose_evolution_tools_and_selector_errors() {
    let mut client = McpTestHarness::indexed(&[
        ("service.py", "def compute():\n    return 42\n"),
        (
            "tests/test_service.py",
            "from service import compute\ndef test_compute(): compute()\n",
        ),
        ("scanner.py", "def scanner():\n    return 42\n"),
    ])
    .expect("MCP initialize");
    let cases = [
        (
            "forge_read",
            serde_json::json!({"selector":"compute","format":"json"}),
        ),
        (
            "code_map_inspect",
            serde_json::json!({"selector":"compute","show_source":true,"format":"json"}),
        ),
        (
            "code_map_search",
            serde_json::json!({"pattern":"comp*","kind":"function","format":"json"}),
        ),
        (
            "code_map_tests",
            serde_json::json!({"selector":"compute","format":"json"}),
        ),
    ];
    for (i, (name, arguments)) in cases.into_iter().enumerate() {
        let (_, response) = client.call(name, arguments).expect("MCP transport");
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
            assert!(payload["nodes"].get("generation").is_none());
        }
    }

    let (_, default_snippet) = client
        .call("forge_read", serde_json::json!({"selector":"compute"}))
        .expect("MCP transport");
    assert_eq!(
        default_snippet["result"]["isError"], false,
        "{default_snippet}"
    );
    let default_text = default_snippet["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(default_text.starts_with("# forge_read"), "{default_text}");

    let (_, explicit_markdown) = client
        .call(
            "forge_read",
            serde_json::json!({"selector":"compute","format":"markdown"}),
        )
        .expect("MCP transport");
    assert_eq!(
        explicit_markdown["result"]["isError"], false,
        "{explicit_markdown}"
    );
    assert_eq!(
        explicit_markdown["result"]["content"][0]["text"], default_text,
        "MCP-only source snippet defaults to Markdown and honors explicit Markdown"
    );

    let explicit_json = client
        .payload(
            "forge_read",
            serde_json::json!({"selector":"compute","format":"json"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(explicit_json["source"], "def compute():\n    return 42\n");
    assert_eq!(explicit_json["node"]["path"], "service.py");

    let output = run_cli(
        client.workspace(),
        &[
            "--format",
            "json",
            "query",
            "inspect",
            "compute",
            "--show-source",
        ],
    );
    assert!(
        output.status.success(),
        "CLI should parse and run inspect --show-source: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON response");
    assert_eq!(response["source"], "def compute():\n    return 42\n");

    let output = run_cli(client.workspace(), &["query", "inspect", "scanner"]);
    assert!(!output.status.success(), "ambiguous selector must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("ambiguous selector scanner"),
        "CLI should report selector ambiguity: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn source_search_and_stale_source_through_mcp() {
    let code = "def OrderRevert():\r\n    return 'Привіт'\r\n";
    let (w, harness) = McpTestHarness::with_files(&[
        ("service.py", code),
        ("types.py", "class PimProductApiRow: pass\n"),
    ]);
    let mut client = harness.expect("MCP initialize");
    let default = client
        .inspect("OrderRevert", serde_json::json!({}))
        .expect("MCP tool should succeed");
    assert!(default.get("source").is_none());
    let source = client
        .inspect("OrderRevert", serde_json::json!({"show_source":true}))
        .expect("MCP tool should succeed");
    assert_eq!(source["source"], code);
    for pattern in ["Order*", "*Revert*", "OrderRevert"] {
        let found = client
            .search(pattern, serde_json::json!({"kind":"function"}))
            .expect("MCP tool should succeed");
        assert_eq!(found["nodes"]["items"].as_array().unwrap().len(), 1);
    }
    let prefix = client
        .search("PimProduct", serde_json::json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(prefix["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(prefix["nodes"]["items"][0]["name"], "PimProductApiRow");
    w.write("service.py", "def OrderRevert():\n    return 'changed'\n");
    let (_, stale) = client
        .call(
            "forge_read",
            serde_json::json!({"selector":"OrderRevert","format":"json"}),
        )
        .expect("MCP transport");
    assert_eq!(stale["result"]["isError"], true, "{stale}");
    client.finish().expect("server must finish successfully");
}

#[test]
#[cfg(all(feature = "lang-typescript", feature = "lang-rust"))]
fn rich_edges_routes_and_delta_persisted() {
    let code = "def decorate(cls):\n    return cls\nclass Base: pass\n@decorate\nclass Child(Base):\n    count = 0\n    def update(self):\n        self.count = 1\ndef handler(): pass\napp.get('/items', handler)\npath('items/', handler)\n@app.post('/new')\ndef create(): pass\n";
    let w = Workspace::with_files(&[
        ("app.py", code),
        (
            "app.ts",
            "interface Store {}\nclass Parent {}\nclass ChildTS extends Parent implements Store {}\nfunction view() {}\nconst routes = [{path: '/page', component: view}];\napp.get('/ts', view);\n",
        ),
        (
            "app.rs",
            "trait Save {}\nstruct Record {}\nimpl Save for Record {}\n",
        ),
        (
            "routes.js",
            "class JsBase {}\nclass JsChild extends JsBase {}\nfunction jsview() {}\napp.put('/js', jsview);\n",
        ),
        (
            "fields.ts",
            "function logged(value: any) { return value; }\n@logged\nclass Box {\n count = 0;\n @Get('/box')\n show() { this.count += 1; }\n}\n",
        ),
    ]);
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
    w.delta(&["app.py"]);
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
    let mut client = McpTestHarness::indexed(&[
        (
            "service.py",
            "def target(): pass\ndef wrapper(): target()\n",
        ),
        (
            "tests/test_service.py",
            "from service import wrapper\ndef test_target(): wrapper()\ndef test_unrelated(): pass\n",
        ),
    ])
    .expect("MCP initialize");
    let tests = client
        .payload("code_map_tests", serde_json::json!({"selector":"target"}))
        .expect("MCP tool should succeed");
    let nodes = tests["nodes"]["items"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["name"] == "test_target"));
    assert!(!nodes.iter().any(|n| n["name"] == "test_unrelated"));
    let production = client
        .payload(
            "code_map_tests",
            serde_json::json!({"selector":"test_target","direction":"outbound"}),
        )
        .expect("MCP tool should succeed");
    assert!(production["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "target"));
    client.finish().expect("server must finish successfully");
}

#[test]
fn linked_source_is_exact_and_rejects_symlinks_and_traversal() {
    use contextunity_forge_mcp::{
        core::response::{CoverageOptions, QueryOptions, ResponsePolicy, SourceOptions},
        db::symbols,
        engine::scanner,
    };
    let w = Workspace::with_files(&[]);
    let linked = Workspace::with_files(&[("lib.py", "def linked_fn():\n    return 7\n")]);
    w.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: shared\n    path: {}\n",
            linked.root().display()
        ),
    );
    w.build();
    let c = w.open();
    let policy = ResponsePolicy::default();
    let page = QueryOptions::resolve(&policy, Some(100), 0, None, None).unwrap();
    let source_options = SourceOptions::resolve(&policy, Some(true), None, None, 0).unwrap();
    let inspect = || {
        symbols::inspect_with_options(
            &c,
            w.root(),
            "linked_fn",
            &symbols::InspectOptions {
                show_doc: false,
                source: &source_options,
                coverage: CoverageOptions::default(),
                page: &page,
            },
        )
    };
    let source = inspect().unwrap();
    assert_eq!(source["source"], "def linked_fn():\n    return 7\n");
    let adapter = scanner::load_adapter(w.root(), None).unwrap();
    assert!(scanner::resolve_file_path(w.root(), &adapter, "[shared]/../escape.py").is_err());
    #[cfg(unix)]
    {
        fs::rename(linked.path("lib.py"), linked.path("actual.py")).unwrap();
        std::os::unix::fs::symlink("actual.py", linked.path("lib.py")).unwrap();
        assert!(inspect().is_err());
    }
}

#[test]
fn queries_bound_results_and_indexes_cover_edge_predicates() {
    use contextunity_forge_mcp::{
        core::response::{QueryOptions, ResponsePolicy},
        db::symbols,
    };
    let w = Workspace::with_files(&[(
        "lib.py",
        "def OrderOne(): pass\ndef OrderTwo(): pass\ndef get_item(): pass\ndef getXitem(): pass\n",
    )]);
    w.build();
    let c = w.open();
    let search =
        |pattern: &str, kind: Option<&str>, limit: usize| -> anyhow::Result<serde_json::Value> {
            let page =
                QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None)?;
            symbols::search_with_options(
                &c,
                pattern,
                &symbols::SearchOptions {
                    kind,
                    path: None,
                    include_docs: false,
                    exact: false,
                    page: &page,
                },
            )
        };
    let found = search("Order*", Some("function"), 1).unwrap();
    assert_eq!(found["nodes"]["has_more"], true);
    assert_eq!(found["nodes"]["items"].as_array().unwrap().len(), 1);
    let found = search("*get_item*", Some("function"), 10).unwrap();
    assert_eq!(found["nodes"]["items"].as_array().unwrap().len(), 1);
    for pattern in ["", "***"] {
        assert!(search(pattern, None, 10).is_err());
    }
    assert!(search("Order*", None, 0).is_err());
    let page = QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    assert!(symbols::tests_paged(&c, "OrderOne", "invalid", &page).is_err());
    for (column, index) in [
        ("src_hash", "PRIMARY KEY"),
        ("dst_hash", "idx_edges_dst_kind"),
    ] {
        let plans = reader::rows(
            &c,
            &format!("EXPLAIN QUERY PLAN SELECT 1 FROM edges WHERE {column}=1 AND kind='calls'"),
            &[],
            10,
        )
        .unwrap();
        assert!(
            plans.iter().any(|p| {
                let detail = p["detail"].as_str().unwrap();
                detail.contains(index)
                    || detail.contains("PRIMARY KEY")
                    || detail.contains("idx_edges_kind")
            }),
            "{plans:?}"
        );
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn rust_test_attributes_and_cyclic_python_dependencies() {
    let mut client = McpTestHarness::indexed(&[
        (
            "lib.rs",
            "fn compute() {}\n#[test]\nfn checks_compute() { compute(); }\nfn unrelated() {}\n",
        ),
        ("cycle.py", "def a(): b()\ndef b(): a()\n"),
        (
            "tests/test_cycle.py",
            "from cycle import a\ndef test_cycle(): a()\n",
        ),
    ])
    .expect("MCP initialize");
    let tests = client
        .payload("code_map_tests", serde_json::json!({"selector":"compute"}))
        .expect("MCP tool should succeed");
    assert!(tests["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "checks_compute"));
    let tests = client
        .payload("code_map_tests", serde_json::json!({"selector":"a"}))
        .expect("MCP tool should succeed");
    assert_eq!(tests["nodes"]["items"].as_array().unwrap().len(), 1);
    client.finish().expect("server must finish successfully");
}

#[test]
fn imported_relationships_relink_when_target_changes() {
    let w = Workspace::with_files(&[
        (
            "base.py",
            "class Base: pass\ndef decorate(cls): return cls\ndef handler(): pass\n",
        ),
        (
            "routes.py",
            "from base import Base, decorate, handler\n@decorate\nclass Child(Base): pass\napp.get('/alias', handler)\n@app.route('/multi', methods=['GET', 'POST'])\ndef multi(): pass\napp.get(dynamic_path, handler)\n",
        ),
    ]);
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
    let mut client = McpTestHarness::new(&w).expect("MCP initialize");
    let impact = client
        .payload("code_map_impact", serde_json::json!({"selector":"Base"}))
        .expect("MCP tool should succeed");
    assert!(impact["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "Child"));
    w.write(
        "base.py",
        "class Base: pass\ndef decorate(cls): return cls\n",
    );
    w.delta(&["base.py"]);
    assert!(!has_edge(&edges(&w), "GET /alias", "handler", "handles"));
    let c = w.open();
    let unresolved = reader::rows(
        &c,
        "SELECT * FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND status='unresolved'",
        &[&"routes.py", &"handler"],
        100,
    )
    .unwrap();
    assert!(!unresolved.is_empty());
    client.finish().expect("server must finish successfully");
}

#[test]
fn test_safe_selector_resolution_and_disambiguation() {
    let (w, harness) = McpTestHarness::indexed_workspace(&[
        ("scanner.py", "def scanner():\n    return 42\n"),
        ("docs/contract.md", "# Contract\n"),
    ]);
    let mut client = harness.expect("MCP initialize");

    // 1. Path-like selector picks the module node
    let inspect_file = client
        .inspect("scanner.py", serde_json::json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(
        inspect_file["node"]["kind"], "module",
        "path-like selector 'scanner.py' must resolve to module node"
    );

    // 2. path:symbol resolves to the function inside the file
    let inspect_path_sym = client
        .inspect("scanner.py:scanner", serde_json::json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(
        inspect_path_sym["node"]["kind"], "function",
        "path:symbol 'scanner.py:scanner' must resolve to function node"
    );

    // 3. path:line resolves to the function containing line 1
    let inspect_path_line = client
        .inspect("scanner.py:1", serde_json::json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(
        inspect_path_line["node"]["kind"], "function",
        "path:line 'scanner.py:1' must resolve to function node at that line"
    );

    // 4. path#Lline resolves to the function containing line 1
    let inspect_path_hash = client
        .inspect("scanner.py#L1", serde_json::json!({}))
        .expect("MCP tool should succeed");
    assert_eq!(
        inspect_path_hash["node"]["kind"], "function",
        "path#Lline 'scanner.py#L1' must resolve to function node at that line"
    );

    // Bare name 'scanner' matches both nodes and stays ambiguous over MCP.
    let rejection = client
        .text(
            "code_map_inspect",
            serde_json::json!({"selector":"scanner"}),
        )
        .expect_err("ambiguous selector should be rejected");
    assert!(
        rejection
            .tool_message()
            .expect("MCP tool error")
            .contains("ambiguous selector scanner"),
        "{rejection}"
    );

    let conn = w.open();
    let ambiguous = reader::select(&conn, "scanner").unwrap_err();
    assert!(matches!(
        ambiguous.downcast_ref::<reader::SelectorError>(),
        Some(reader::SelectorError::Ambiguous { selector, candidates })
            if selector == "scanner" && candidates.len() > 1
    ));
    let missing = reader::select(&conn, "missing_symbol").unwrap_err();
    assert!(matches!(
        missing.downcast_ref::<reader::SelectorError>(),
        Some(reader::SelectorError::NotFound { selector, .. }) if selector == "missing_symbol"
    ));
    let invalid = reader::select(&conn, " ").unwrap_err();
    assert!(matches!(
        invalid.downcast_ref::<reader::SelectorError>(),
        Some(reader::SelectorError::InvalidSyntax { .. })
    ));
    let document = reader::select(&conn, "docs/contract.md").unwrap_err();
    assert!(matches!(
        document.downcast_ref::<reader::SelectorError>(),
        Some(reader::SelectorError::DocLink { target }) if target == "docs/contract.md"
    ));
    client.finish().expect("server must finish successfully");
}
