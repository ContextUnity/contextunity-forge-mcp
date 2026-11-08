#[test]
fn mcp_compact_summary_coverage_sql_and_impact_directions() {
    use serde_json::json;
    let mut m = crate::harness::McpTestHarness::indexed(&[
        (
            "graph.py",
            "def target():\n    missing_dependency()\n    helper()\ndef helper(): return 1\ndef caller(): return target()\n",
        ),
        (
            "record.rs",
            "struct Record;\nimpl Record { fn persist(&self) {} }\nmod left { struct Item; impl Item { fn persist(&self) {} } }\nmod right { struct Item; impl Item { fn persist(&self) {} } }\n",
        ),
    ])
    .expect("MCP initialize");
    let mut inspect_method = |container: &str| {
        let search = m
            .payload(
                "code_map_search",
                json!({"pattern":"persist","kind":"method","path":"record.rs","exact":true,"limit":10,"detail":"full"}),
            )
            .expect("MCP tool should succeed");
        let methods = search["nodes"]["items"]
            .as_array()
            .expect("method search should return nodes");
        let mut found = None;
        for method in methods {
            let id = method["id"]
                .as_str()
                .expect("method result should have an id");
            let inspected = m
                .payload("code_map_inspect", json!({"selector":id}))
                .expect("MCP tool should succeed");
            if inspected["summary"]["container"]["qualname"]
                .as_str()
                .is_some_and(|qualname| qualname.contains(container))
            {
                found = Some((id.to_owned(), inspected));
                break;
            }
        }
        found.unwrap_or_else(|| panic!("no persist method matched container {container}"))
    };
    let (record_method, _) = inspect_method("Record");
    let (right_method, right_snapshot) = inspect_method("right");
    let right_container = right_snapshot["summary"]["container"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let right_qualname = right_snapshot["summary"]["container"]["qualname"]
        .as_str()
        .unwrap()
        .to_owned();

    let inspected = m
        .payload("code_map_inspect", json!({"selector":"target"}))
        .expect("MCP tool should succeed");
    assert_eq!(inspected["summary"]["signature"], "def target():");
    assert!(inspected.get("coverage").is_none());
    assert_eq!(inspected["summary"]["calls"]["inbound"]["count"], 1);
    assert_eq!(inspected["summary"]["calls"]["outbound"]["count"], 1);
    let rust_method = m
        .payload("code_map_inspect", json!({"selector":record_method}))
        .expect("MCP tool should succeed");
    assert_eq!(rust_method["summary"]["container"]["name"], "Record");
    assert_eq!(rust_method["summary"]["method"]["receiver"], "Record");
    assert_eq!(rust_method["summary"]["method"]["receiver_name"], "self");
    let module_qualified_method = m
        .payload("code_map_inspect", json!({"selector":right_method}))
        .expect("MCP tool should succeed");
    assert_eq!(
        module_qualified_method["summary"]["container"]["id"],
        right_container
    );
    assert_eq!(
        module_qualified_method["summary"]["container"]["qualname"],
        right_qualname
    );
    assert!(right_qualname.contains("right"));
    let with_coverage = m
        .payload(
            "code_map_inspect",
            json!({"selector":"target","include_coverage":true}),
        )
        .expect("MCP tool should succeed");
    assert!(with_coverage["coverage"]["total"].as_u64().unwrap() >= 1);
    let explained = m
        .payload("code_map_explain", json!({"selector":"target"}))
        .expect("MCP tool should succeed");
    assert_eq!(explained["summary"], inspected["summary"]);
    let explained_with_coverage = m
        .payload(
            "code_map_explain",
            json!({"selector":"target","include_coverage":true}),
        )
        .expect("MCP tool should succeed");
    assert!(
        explained_with_coverage["coverage"]["total"]
            .as_u64()
            .unwrap()
            >= 1
    );
    let queried_inspect = m
        .payload(
            "code_map_query",
            json!({"operation":"inspect","selector":"target"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(queried_inspect["summary"], inspected["summary"]);
    assert!(queried_inspect.get("coverage").is_none());
    let queried_with_coverage = m
        .payload(
            "code_map_query",
            json!({"operation":"explain","selector":"target","include_coverage":true}),
        )
        .expect("MCP tool should succeed");
    assert!(queried_with_coverage["coverage"]["total"].as_u64().unwrap() >= 1);

    let inbound = m
        .payload("code_map_impact", json!({"selector":"target","depth":1}))
        .expect("MCP tool should succeed");
    assert_eq!(inbound["direction"], "inbound");
    assert!(inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "caller"));
    let outbound = m
        .payload(
            "code_map_impact",
            json!({"selector":"target","direction":"outbound","depth":1}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(outbound["direction"], "outbound");
    assert!(outbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "helper"));
    let rejection = m
        .text(
            "code_map_impact",
            json!({"selector":"target","direction":"sideways","depth":1}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "impact direction must be inbound or outbound";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );

    let query_outbound = m
        .payload(
            "code_map_query",
            json!({"operation":"impact","selector":"target","direction":"outbound","depth":1}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(query_outbound["direction"], "outbound");
    let rejection = m
        .text(
            "code_map_query",
            json!({"operation":"impact","selector":"target","direction":"sideways","depth":1}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "impact direction must be inbound or outbound";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let sql = m.payload(
        "code_map_query",
        json!({"operation":"sql","selector":"SELECT name FROM nodes WHERE kind='function' ORDER BY name","limit":1}),
    ).expect("MCP tool should succeed");
    assert_eq!(sql["rows"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(sql["rows"]["total"], 3);
    assert!(sql["rows"]["generation"].is_string());
    m.finish().expect("server must finish successfully");
}

#[test]
fn mcp_rejects_wide_graph_before_deep_traversal() {
    use serde_json::json;
    let mut code = String::from("def target(): pass\n");
    for index in 0..1001 {
        code.push_str(&format!("def caller_{index}(): target()\n"));
    }
    let mut m =
        crate::harness::McpTestHarness::indexed(&[("wide.py", &code)]).expect("MCP initialize");
    let rejection = m
        .text(
            "code_map_impact",
            json!({"selector":"wide.py:target","depth":2,"limit":1}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "Retry with depth=1";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let shallow = m
        .payload(
            "code_map_impact",
            json!({"selector":"wide.py:target","depth":1,"limit":1}),
        )
        .expect("MCP tool should succeed");
    assert!(shallow["nodes"]["total"].as_u64().unwrap() > 1000);
    m.finish().expect("server must finish successfully");
}

#[test]
fn mcp_rejects_removal_scope_over_ten_thousand_nodes() {
    use serde_json::json;
    let mut code = String::new();
    for index in 0..10001 {
        code.push_str(&format!("def item_{index}(): pass\n"));
    }
    let mut m =
        crate::harness::McpTestHarness::indexed(&[("scope.py", &code)]).expect("MCP initialize");
    let rejection = m
        .text("code_map_prove_removal", json!({"selector":"scope.py"}))
        .expect_err("tool should reject input");
    let expected: &str = "removal scope exceeds 10000 nodes";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    m.finish().expect("server must finish successfully");
}

#[test]
fn mcp_enforces_page_row_and_time_budgets() {
    use serde_json::json;
    let mut m = crate::harness::McpTestHarness::indexed(&[(
        "app.py",
        "def one(): pass\ndef two(): pass\n",
    )])
    .expect("MCP initialize");
    let rejection = m
        .text("code_map_search", json!({"pattern":"one","limit":0}))
        .expect_err("tool should reject input");
    let expected: &str = "limit must be 1..=100";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let rejection = m
        .text("code_map_search", json!({"pattern":"one","limit":101}))
        .expect_err("tool should reject input");
    let expected: &str = "limit must be 1..=100";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let rejection = m
        .text("code_map_search", json!({"pattern":"one","offset":1}))
        .expect_err("tool should reject input");
    let expected: &str = "continuation requires generation";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let rejection = m.text("code_map_analyze", json!({"target":"SELECT hex(zeroblob(3000000)) AS a, hex(zeroblob(3000000)) AS b FROM nodes LIMIT 1"})).expect_err("tool should reject input");
    let expected: &str = "8 MiB row budget";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let rejection = m.text("code_map_analyze", json!({"target":"WITH RECURSIVE seq(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM seq WHERE x<100000000) SELECT max(x) FROM seq"})).expect_err("tool should reject input");
    let expected: &str = "2-second SQLite budget";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    m.finish().expect("server must finish successfully");
}
