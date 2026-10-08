use super::*;

#[test]
fn mcp_exposes_and_executes_evolution_tools() {
    let w = Workspace::new();
    w.write("service.py", "def compute():\n    return 42\n");
    w.write(
        "tests/test_service.py",
        "from service import compute\ndef test_compute(): compute()\n",
    );
    w.build();
    let mut client = StdioClient::new(&w);
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
        let (_, response) = client.call(name, arguments);
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
}

#[test]
fn source_search_and_stale_source_through_cli() {
    let w = Workspace::new();
    let code = "def OrderRevert():\r\n    return 'Привіт'\r\n";
    w.write("service.py", code);
    w.write("types.py", "class PimProductApiRow: pass\n");
    w.build();
    let default = w.query(&["query", "inspect", "OrderRevert"]);
    assert!(default.get("source").is_none());
    let source = w.query(&["query", "inspect", "OrderRevert", "--show-source"]);
    assert_eq!(source["source"], code);
    for pattern in ["Order*", "*Revert*", "OrderRevert"] {
        let found = w.query(&["query", "search", pattern, "--kind", "function"]);
        assert_eq!(found["nodes"]["items"].as_array().unwrap().len(), 1);
    }
    let prefix = w.query(&["query", "search", "PimProduct"]);
    assert_eq!(prefix["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(prefix["nodes"]["items"][0]["name"], "PimProductApiRow");
    w.write("service.py", "def OrderRevert():\n    return 'changed'\n");
    assert!(
        !run_cli(&w, &["query", "inspect", "OrderRevert", "--show-source"])
            .status
            .success()
    );
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
    let nodes = tests["nodes"]["items"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["name"] == "test_target"));
    assert!(!nodes.iter().any(|n| n["name"] == "test_unrelated"));
    let production = w.query(&["query", "tests", "test_target", "--direction", "outbound"]);
    assert!(production["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "target"));
}

#[test]
fn linked_source_is_exact_and_rejects_symlinks_and_traversal() {
    use contextunity_forge_mcp::{
        core::response::{CoverageOptions, QueryOptions, ResponsePolicy, SourceOptions},
        db::symbols,
        engine::scanner,
    };
    let w = Workspace::new();
    let linked = Workspace::new();
    linked.write("lib.py", "def linked_fn():\n    return 7\n");
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
    let w = Workspace::new();
    w.write(
        "lib.py",
        "def OrderOne(): pass\ndef OrderTwo(): pass\ndef get_item(): pass\ndef getXitem(): pass\n",
    );
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
    assert!(tests["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "checks_compute"));
    let tests = w.query(&["query", "tests", "a"]);
    assert_eq!(tests["nodes"]["items"].as_array().unwrap().len(), 1);
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
    let unresolved = reader::rows(&c, "SELECT * FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='routes.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='handler' AND status='unresolved'", &[], 100).unwrap();
    assert!(!unresolved.is_empty());
}

#[test]
fn test_safe_selector_resolution_and_disambiguation() {
    let w = Workspace::new();
    w.write("scanner.py", "def scanner():\n    return 42\n");
    w.write("docs/contract.md", "# Contract\n");
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
    let bare_cmd = run_cli(&w, &["query", "inspect", "scanner"]);
    assert!(
        !bare_cmd.status.success(),
        "bare selector 'scanner' matching both module and function must not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&bare_cmd.stderr);
    assert!(
        stderr.contains("ambiguous selector scanner"),
        "stderr must report ambiguous selector, got: {stderr}"
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
}
