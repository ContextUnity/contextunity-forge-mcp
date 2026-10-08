use super::*;

#[test]
fn mcp_compact_summary_coverage_sql_and_impact_directions() {
    use serde_json::json;
    let workspace = Workspace::new();
    workspace.write(
        "graph.py",
        "def target():\n    missing_dependency()\n    helper()\ndef helper(): return 1\ndef caller(): return target()\n",
    );
    workspace.write(
        "record.rs",
        "struct Record;\nimpl Record { fn persist(&self) {} }\nmod left { struct Item; impl Item { fn persist(&self) {} } }\nmod right { struct Item; impl Item { fn persist(&self) {} } }\n",
    );
    workspace.build();
    let conn = rusqlite::Connection::open(workspace.db()).unwrap();
    let (record_method, right_method, right_container, right_qualname): (String, String, String, String) = conn
        .query_row(
            "SELECT (SELECT method.id FROM nodes method WHERE method.path_id=(SELECT path_id FROM path_dictionary WHERE path='record.rs') AND method.name='persist' AND method.line=2), method.id, receiver.id, receiver.qualname FROM nodes method JOIN edges e ON e.dst_hash=method.node_hash AND e.kind='contains' JOIN nodes receiver ON receiver.path_id=method.path_id AND receiver.name='Item' AND receiver.kind='struct' WHERE method.path_id=(SELECT path_id FROM path_dictionary WHERE path='record.rs') AND method.name='persist' AND method.kind='method' AND method.line=(SELECT max(line) FROM nodes WHERE (SELECT path FROM path_dictionary WHERE path_id=e.path_id)='record.rs' AND name='persist' AND kind='method') AND receiver.qualname LIKE '%right%'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    drop(conn);
    let mut m = Mcp::new(&workspace);

    let inspected = m.ok("code_map_inspect", json!({"selector":"target"}));
    assert_eq!(inspected["summary"]["signature"], "def target():");
    assert!(inspected.get("coverage").is_none());
    assert_eq!(inspected["summary"]["calls"]["inbound"]["count"], 1);
    assert_eq!(inspected["summary"]["calls"]["outbound"]["count"], 1);
    let rust_method = m.ok("code_map_inspect", json!({"selector":record_method}));
    assert_eq!(rust_method["summary"]["container"]["name"], "Record");
    assert_eq!(rust_method["summary"]["method"]["receiver"], "Record");
    assert_eq!(rust_method["summary"]["method"]["receiver_name"], "self");
    let module_qualified_method = m.ok("code_map_inspect", json!({"selector":right_method}));
    assert_eq!(
        module_qualified_method["summary"]["container"]["id"],
        right_container
    );
    assert_eq!(
        module_qualified_method["summary"]["container"]["qualname"],
        right_qualname
    );
    assert!(right_qualname.contains("right"));
    let with_coverage = m.ok(
        "code_map_inspect",
        json!({"selector":"target","include_coverage":true}),
    );
    assert!(with_coverage["coverage"]["total"].as_u64().unwrap() >= 1);
    let explained = m.ok("code_map_explain", json!({"selector":"target"}));
    assert_eq!(explained["summary"], inspected["summary"]);
    let explained_with_coverage = m.ok(
        "code_map_explain",
        json!({"selector":"target","include_coverage":true}),
    );
    assert!(
        explained_with_coverage["coverage"]["total"]
            .as_u64()
            .unwrap()
            >= 1
    );
    let queried_inspect = m.ok(
        "code_map_query",
        json!({"operation":"inspect","selector":"target"}),
    );
    assert_eq!(queried_inspect["summary"], inspected["summary"]);
    assert!(queried_inspect.get("coverage").is_none());
    let queried_with_coverage = m.ok(
        "code_map_query",
        json!({"operation":"explain","selector":"target","include_coverage":true}),
    );
    assert!(queried_with_coverage["coverage"]["total"].as_u64().unwrap() >= 1);

    let inbound = m.ok("code_map_impact", json!({"selector":"target","depth":1}));
    assert_eq!(inbound["direction"], "inbound");
    assert!(inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "caller"));
    let outbound = m.ok(
        "code_map_impact",
        json!({"selector":"target","direction":"outbound","depth":1}),
    );
    assert_eq!(outbound["direction"], "outbound");
    assert!(outbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "helper"));
    m.err(
        "code_map_impact",
        json!({"selector":"target","direction":"sideways","depth":1}),
        "impact direction must be inbound or outbound",
    );

    let query_outbound = m.ok(
        "code_map_query",
        json!({"operation":"impact","selector":"target","direction":"outbound","depth":1}),
    );
    assert_eq!(query_outbound["direction"], "outbound");
    m.err(
        "code_map_query",
        json!({"operation":"impact","selector":"target","direction":"sideways","depth":1}),
        "impact direction must be inbound or outbound",
    );
    let sql = m.ok(
        "code_map_query",
        json!({"operation":"sql","selector":"SELECT name FROM nodes WHERE kind='function' ORDER BY name","limit":1}),
    );
    assert_eq!(sql["rows"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(sql["rows"]["total"], 3);
    assert!(sql["rows"]["generation"].is_string());
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
    m.err(
        "code_map_impact",
        json!({"selector":"wide.py:target","depth":2,"limit":1}),
        "Retry with depth=1",
    );
    let shallow = m.ok(
        "code_map_impact",
        json!({"selector":"wide.py:target","depth":1,"limit":1}),
    );
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
    m.err(
        "code_map_prove_removal",
        json!({"selector":"scope.py"}),
        "removal scope exceeds 10000 nodes",
    );
}

#[test]
fn mcp_enforces_page_row_and_time_budgets() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("app.py", "def one(): pass\ndef two(): pass\n");
    w.build();
    let mut m = Mcp::new(&w);
    m.err(
        "code_map_search",
        json!({"pattern":"one","limit":0}),
        "limit must be 1..=100",
    );
    m.err(
        "code_map_search",
        json!({"pattern":"one","limit":101}),
        "limit must be 1..=100",
    );
    m.err(
        "code_map_search",
        json!({"pattern":"one","offset":1}),
        "continuation requires generation",
    );
    m.err(
        "code_map_analyze",
        json!({"target":"SELECT hex(zeroblob(3000000)) AS a, hex(zeroblob(3000000)) AS b FROM nodes LIMIT 1"}),
        "8 MiB row budget",
    );
    m.err("code_map_analyze", json!({"target":"WITH RECURSIVE seq(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM seq WHERE x<100000000) SELECT max(x) FROM seq"}), "2-second SQLite budget");
}
