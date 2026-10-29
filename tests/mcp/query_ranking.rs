use crate::common::Workspace;
use contextunity_forge_mcp::db::{reader, writer};

#[cfg(feature = "lang-python")]
#[test]
fn hybrid_search_ranks_connected_exact_symbols_before_path_order() {
    use contextunity_forge_mcp::core::response::{Detail, QueryOptions, ResponsePolicy};
    use contextunity_forge_mcp::db::symbols;

    let ws = Workspace::new();
    ws.write(
        "a.py",
        "def process():\n    return 1\ndef process_work():\n    return 1\n",
    );
    ws.write(
        "z.py",
        "def process():\n    \"\"\"Processes one request. Further details stay out of the preview.\"\"\"\n    return 1\ndef process_work():\n    return 1\n",
    );
    ws.write(
        "caller.py",
        "from z import process, process_work\n\ndef run():\n    process_work()\n    return process()\n",
    );
    ws.write(
        "signature.py",
        "@pytest.fixture\ndef typed_query(order: GateProfile) -> Result:\n    return order\n\ndef returns_gate() -> GateProfile:\n    return None\n\ndef takes_result(value: Result) -> Other:\n    return value\n\n@pytest.other\ndef other_decorator():\n    return 1\n\n@other.fixture\ndef unrelated_fixture():\n    return 1\n",
    );
    ws.write("types.py", "class GateProfile:\n    pass\n");
    ws.write("underscored.py", "class Gate_Profile:\n    pass\ndef accepts_profile(value: Gate_Profile):\n    return value\n");
    ws.write("types.rs", "#[derive(Debug)]\nstruct Example {}\n");
    let db_path = ws.root().join(".forge/code-map.sqlite");
    writer::build(ws.root(), &db_path, None).unwrap();
    let conn = reader::open(&db_path, ws.root()).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let result = symbols::search_with_options(
        &conn,
        "process",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let nodes = result["nodes"]["items"].as_array().unwrap();
    assert_eq!(result["nodes"]["total"], 2);
    assert_eq!(nodes.len(), 2);
    assert_eq!(
        nodes[0]["path"], "z.py",
        "inbound graph connectivity breaks equivalent exact-name scores"
    );
    assert_eq!(nodes[1]["path"], "a.py");
    assert_eq!(nodes[0]["name"], "process");
    assert_eq!(nodes[1]["name"], "process");
    assert_eq!(nodes[0]["match_reason"], "exact_name");
    for (pattern, expected) in [
        ("GateProfile", &["returns_gate", "typed_query"][..]),
        ("returns:Result", &["typed_query"][..]),
        ("returns:esul", &["typed_query"][..]),
        ("takes:GateProfile", &["typed_query"][..]),
        ("takes:Profile", &["typed_query"][..]),
        ("returns:GateProfile", &["returns_gate"][..]),
        ("takes:Result", &["takes_result"][..]),
        ("takes:order", &[][..]),
        ("takes:%", &[][..]),
        ("takes:_", &[][..]),
        ("@pytest.fixture", &["typed_query"][..]),
    ] {
        let result = symbols::search_with_options(
            &conn,
            pattern,
            &symbols::SearchOptions {
                kind: Some("function"),
                path: Some("signature.py"),
                include_docs: false,
                exact: false,
                page: &options,
            },
        )
        .unwrap();
        let mut names: Vec<_> = result["nodes"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["name"].as_str().unwrap())
            .collect();
        names.sort_unstable();
        assert_eq!(names, expected, "signature search {pattern}: {result}");
    }
    for invalid in ["takes:", "returns:*", "takes:Gate*"] {
        assert!(
            symbols::search_with_options(
                &conn,
                invalid,
                &symbols::SearchOptions {
                    kind: Some("function"),
                    path: Some("signature.py"),
                    include_docs: false,
                    exact: false,
                    page: &options,
                },
            )
            .is_err(),
            "invalid filter accepted: {invalid}"
        );
    }
    let all_types = symbols::search_with_options(
        &conn,
        "GateProfile",
        &symbols::SearchOptions {
            kind: None,
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let all_names: Vec<_> = all_types["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(all_names.first(), Some(&"GateProfile"));
    assert!(
        all_names.contains(&"typed_query") && all_names.contains(&"returns_gate"),
        "{all_types}"
    );
    let underscored = symbols::search_with_options(
        &conn,
        "Gate_Profile",
        &symbols::SearchOptions {
            kind: None,
            path: Some("underscored.py"),
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let names: Vec<_> = underscored["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.first(), Some(&"Gate_Profile"));
    assert!(names.contains(&"accepts_profile"), "{underscored}");
    let attributes = symbols::search_with_options(
        &conn,
        "derive",
        &symbols::SearchOptions {
            kind: Some("struct"),
            path: Some("types.rs"),
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(attributes["nodes"]["items"][0]["name"], "Example");
    {
        let mut client = crate::common::mcp_client::StdioClient::new(&ws);
        let mcp = client.payload(
            "code_map_search",
            serde_json::json!({
                "pattern":"returns:Result", "kind":"function", "format":"json"
            }),
        );
        let cli = crate::common::mcp_client::run_cli_json(
            &ws,
            &["query", "search", "returns:Result", "--kind", "function"],
        );
        assert!(
            cli.status.success(),
            "{}",
            String::from_utf8_lossy(&cli.stderr)
        );
        let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
        let mcp_names: Vec<_> = mcp["nodes"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        let cli_names: Vec<_> = cli["nodes"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        assert_eq!(mcp_names, ["typed_query"]);
        assert_eq!(mcp_names, cli_names);
    }
    let prefix = symbols::search_with_options(
        &conn,
        "process_*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let nodes = prefix["nodes"]["items"].as_array().unwrap();
    assert_eq!(prefix["nodes"]["total"], 2);
    assert_eq!(
        nodes[0]["path"], "z.py",
        "connected prefix matches receive the same graph ranking regardless of identifier punctuation"
    );
    assert_eq!(nodes[1]["path"], "a.py");
    drop(conn);

    ws.write(
        "signature.py",
        "def updated(value: NewProfile) -> NewResult:\n    return value\n",
    );
    ws.write("types.rs", "struct Example {}\n");
    ws.delta(&["signature.py", "types.rs"]);
    let incremental = ws.open();
    let changed = symbols::search_with_options(
        &incremental,
        "NewProfile",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: Some("signature.py"),
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(changed["nodes"]["items"][0]["name"], "updated");
    let old = symbols::search_with_options(
        &incremental,
        "GateProfile",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: Some("signature.py"),
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(old["nodes"]["total"], 0);
    let removed_attribute = symbols::search_with_options(
        &incremental,
        "derive",
        &symbols::SearchOptions {
            kind: Some("struct"),
            path: Some("types.rs"),
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(removed_attribute["nodes"]["total"], 0);
    let incremental_root: String = incremental
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(incremental);
    let cold_path = ws.root().join(".forge/signature-cold.sqlite");
    ws.build_to(&cold_path);
    let cold = reader::open(&cold_path, ws.root()).unwrap();
    let cold_root: String = cold
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        incremental_root, cold_root,
        "signature FTS must be cold/delta deterministic"
    );
    drop(cold);
    let stale = rusqlite::Connection::open(&cold_path).unwrap();
    stale.execute(
        "UPDATE metadata SET value='10:compact-storage-v8:legacy' WHERE key='index_semantics_version'",
        [],
    ).unwrap();
    contextunity_forge_mcp::core::commitments::seal(&stale).unwrap();
    drop(stale);
    let error = reader::open(&cold_path, ws.root()).unwrap_err();
    assert!(
        error.to_string().contains("incompatible index semantics"),
        "{error:#}"
    );

    let catalog: String = (0..501)
        .map(|index| format!("def rankterm_{index:03}():\n    return 1\n"))
        .collect();
    ws.write("catalog.py", &catalog);
    ws.write("exact_a.py", "def rankterm():\n    return 1\n");
    ws.write(
        "exact_z.py",
        "def rankterm():\n    \"\"\"Prioritizes the exact rank. Additional prose is omitted.\"\"\"\n    return 1\n",
    );
    ws.write("rank_caller.py", "from exact_z import rankterm\nfrom catalog import rankterm_500\ndef run_ranked():\n    rankterm_500()\n    return rankterm()\n");
    let linked = Workspace::new();
    linked.write(
        "linked module+%#1.py",
        "def linked_uri_symbol():\n    return 1\n",
    );
    ws.write(
        "tests/test_search_role.py",
        "def test_search_role():\n    \"\"\"Marks a test search result. Further details stay out.\"\"\"\n    from exact_z import rankterm\n    return rankterm()\n",
    );
    ws.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [.]\n",
            linked.root().display()
        ),
    );
    linked.write(
        "space/pyproject.toml",
        "[project]\nname = 'linked-search-test'\n",
    );
    writer::build(ws.root(), &db_path, None).unwrap();
    let conn = reader::open(&db_path, ws.root()).unwrap();
    let mut page_options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(100),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let exact = symbols::search_with_options(
        &conn,
        "rankterm",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &page_options,
        },
    )
    .unwrap();
    let exact_nodes = exact["nodes"]["items"].as_array().unwrap();
    assert_eq!(exact["nodes"]["total"], 2);
    assert_eq!(exact_nodes.len(), 2);
    assert_eq!(exact_nodes[0]["name"], "rankterm");
    assert_eq!(exact_nodes[1]["name"], "rankterm");
    assert_eq!(exact_nodes[0]["path"], "exact_z.py");
    assert_eq!(exact_nodes[1]["path"], "exact_a.py");
    assert_eq!(exact_nodes[0]["match_reason"], "exact_name");
    let test_role = symbols::search_with_options(
        &conn,
        "test_search_role",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let test_symbol = &test_role["nodes"]["items"][0];
    assert_eq!(test_symbol["name"], "test_search_role");
    let linked_hit = symbols::search_with_options(
        &conn,
        "linked_uri_symbol",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(
        linked_hit["nodes"]["items"][0]["path"],
        "[library]/linked module+%#1.py"
    );
    let mut all = Vec::new();
    loop {
        let result = symbols::search_with_options(
            &conn,
            "rankterm*",
            &symbols::SearchOptions {
                kind: Some("function"),
                path: None,
                include_docs: false,
                exact: false,
                page: &page_options,
            },
        )
        .unwrap();
        let page = &result["nodes"];
        assert_eq!(page["total"], 503);
        all.extend(page["items"].as_array().unwrap().iter().cloned());
        if page["has_more"] == false {
            break;
        }
        page_options.offset = page["next_offset"].as_u64().unwrap() as usize;
        page_options.generation = Some(page["generation"].as_str().unwrap().to_owned());
    }
    assert_eq!(all.len(), 503);
    let ids: std::collections::BTreeSet<_> = all
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 503);
    assert!(all.iter().all(|item| {
        item["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("rankterm"))
    }));
    assert!(exact_nodes
        .iter()
        .all(|item| ids.contains(item["id"].as_str().unwrap())));
    page_options.offset = 501;
    page_options.limit = 2;
    let tail = symbols::search_with_options(
        &conn,
        "rankterm*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &page_options,
        },
    )
    .unwrap();
    assert_eq!(tail["nodes"]["items"], serde_json::json!(&all[501..]));
    assert_eq!(tail["nodes"]["total"], 503);
    assert_eq!(tail["nodes"]["has_more"], false);

    // Preserve reader JSON and render the ranked search through actual MCP Markdown.
    use contextunity_forge_mcp::core::format::{self, OutputFormat};
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&format::render(
            &exact,
            OutputFormat::Json,
            "code_map_search"
        ))
        .unwrap(),
        exact
    );
    let mut client = crate::common::mcp_client::StdioClient::new(&ws);
    let (_, response) = client.call("code_map_search", serde_json::json!({"pattern":"rankterm","kind":"function","limit":100,"detail":"full","format":"markdown"}));
    assert_ne!(response["result"]["isError"], true, "{response}");
    let markdown = response["result"]["content"][0]["text"].as_str().unwrap();
    for item in exact_nodes {
        let coordinate = format!(
            "file://{}#L{}",
            ws.root().join(item["path"].as_str().unwrap()).display(),
            item["line"].as_u64().unwrap()
        );
        assert!(
            markdown.contains(&coordinate),
            "search must render actual hit coordinates: {markdown}"
        );
    }
    assert!(
        markdown.contains("production"),
        "classification comes from indexed test metadata: {markdown}"
    );
    assert!(
        markdown.contains("caller"),
        "search graph pulse must come from actual graph evidence: {markdown}"
    );
    for expected in [
        "Search for `rankterm`",
        "Match reason: `exact symbol name`",
        "Documentation: Prioritizes the exact rank.",
        "verified covering tests: 1",
        "test_search_role",
    ] {
        assert!(markdown.contains(expected), "{markdown}");
    }
    let (_, json_response) = client.call(
        "code_map_search",
        serde_json::json!({"pattern":"rankterm","kind":"function","limit":100,"detail":"full","format":"json"}),
    );
    assert_ne!(json_response["result"]["isError"], true, "{json_response}");
    let json_text = json_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let enriched: serde_json::Value = serde_json::from_str(json_text).unwrap();
    let enriched_items = enriched["nodes"]["items"].as_array().unwrap();
    let exact_hit = enriched_items
        .iter()
        .find(|item| item["path"] == "exact_z.py")
        .unwrap();
    assert_eq!(exact_hit["classification"], "production");
    assert_eq!(
        exact_hit["docstring_first_sentence"],
        "Prioritizes the exact rank."
    );
    assert_eq!(exact_hit["match_reason"], "exact_name");
    assert_eq!(
        exact_hit["graph_pulse"]["covering_tests"]["discovery_method"],
        "graph"
    );
    assert_eq!(
        exact_hit["graph_pulse"]["covering_tests"]["items"][0]["path"],
        "tests/test_search_role.py"
    );
    let (_, test_json_response) = client.call(
        "code_map_search",
        serde_json::json!({"pattern":"test_search_role","kind":"function","limit":10,"detail":"full","format":"json"}),
    );
    assert_ne!(
        test_json_response["result"]["isError"], true,
        "{test_json_response}"
    );
    let test_json_text = test_json_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let test_enriched: serde_json::Value = serde_json::from_str(test_json_text).unwrap();
    let test_hit = &test_enriched["nodes"]["items"][0];
    assert_eq!(test_hit["is_test"], true);
    assert_eq!(test_hit["classification"], "test");
    assert_eq!(
        test_hit["docstring_first_sentence"],
        "Marks a test search result."
    );
    assert_eq!(test_hit["match_reason"], "exact_name");
    let linked_uri = format!(
        "file://{}/linked%20module%2B%25%231.py#L1",
        linked.root().display()
    );
    let (_, linked_response) = client.call(
        "code_map_search",
        serde_json::json!({"pattern":"linked_uri_symbol","kind":"function","limit":10,"format":"markdown"}),
    );
    assert_ne!(
        linked_response["result"]["isError"], true,
        "{linked_response}"
    );
    let linked_markdown = linked_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        linked_markdown.contains(&linked_uri),
        "linked coordinate must use the selected workspace root and URI escaping: {linked_markdown}"
    );
    assert!(
        markdown.find("exact_z.py").unwrap() < markdown.find("exact_a.py").unwrap(),
        "presentation must preserve producer ranking"
    );
}
