use super::*;

#[test]
fn mcp_selector_and_source_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("scanner.py", "def scanner():\n    return 42\n");
    w.write(
        "src/app.py",
        "# license\ndef first(): pass\ndef last(): pass\n",
    );
    w.write("comments.py", "# license only\n# no code body\n");
    w.write("docs/contract.md", "# Contract\n\nA documented contract.\n");
    w.build();
    let mut m = Mcp::new(&w);

    for tool in [
        "code_map_inspect",
        "code_map_explain",
        "code_map_impact",
        "code_map_tests",
        "code_map_prove_removal",
        "get_code_snippet",
    ] {
        m.err(tool, json!({"selector":"scanner"}), "ambiguous selector");
        m.err(tool, json!({"selector":""}), "selector is empty");
        m.err(tool, json!({"selector":"   "}), "selector is empty");
        m.err(
            tool,
            json!({"selector":"docs/contract.md"}),
            "get_doc or search_docs",
        );
    }
    for selector in [
        "scanner.py:scanner",
        "scanner.py::scanner",
        "scanner.py:1",
        "scanner.py#L1",
    ] {
        let result = m.ok("code_map_inspect", json!({"selector":selector}));
        assert_eq!(result["node"]["kind"], "function", "{selector}: {result}");
    }
    for selector in ["file:src/app.py", "file://src/app.py", "./src/app.py"] {
        let result = m.ok("code_map_inspect", json!({"selector":selector}));
        assert_eq!(result["node"]["kind"], "module", "{selector}: {result}");
    }
    for selector in [":::", "#Labc", "nonexistent/path.py:9999"] {
        m.err(
            "code_map_inspect",
            json!({"selector":selector}),
            "selector not found",
        );
    }
    m.err(
        "code_map_inspect",
        json!({"selector":"%.py:scanner"}),
        "selector not found",
    );
    let first = m.ok(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","leading_lines":0}),
    );
    assert_eq!(first["source"], "def first(): pass\n");
    let last = m.ok(
        "get_code_snippet",
        json!({"selector":"src/app.py:last","leading_lines":0}),
    );
    assert_eq!(last["source"], "def last(): pass\n");
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","max_body_lines":0}),
        "max_body_lines",
    );
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","leading_lines":-1}),
        "expected usize",
    );
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","max_body_lines":-1}),
        "expected usize",
    );
    m.err(
        "get_code_snippet",
        json!({"selector":"src/app.py:first","source_offset":-1}),
        "expected usize",
    );
    let comments = m.ok("get_code_snippet", json!({"selector":"comments.py"}));
    assert_eq!(comments["node"]["kind"], "module");
    assert_eq!(comments["source"], "# license only\n# no code body\n");
}

#[test]
fn mcp_graph_depth_cycles_and_direction_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write(
        "tests/test_cycle.py",
        "from cycle import a\ndef test_a(): a()\n",
    );
    w.build();
    let mut m = Mcp::new(&w);
    for depth in [0, 1, 16] {
        let result = m.ok(
            "code_map_impact",
            json!({"selector":"cycle.py:a","depth":depth}),
        );
        assert_eq!(result["depth"], depth);
        assert!(result["nodes"]["total"].as_u64().unwrap() >= 1);
    }
    m.err(
        "code_map_impact",
        json!({"selector":"cycle.py:a","depth":17}),
        "depth must be <=16",
    );
    let inbound = m.ok(
        "code_map_tests",
        json!({"selector":"cycle.py:a","direction":"inbound"}),
    );
    assert!(inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "test_a"));
    let test = inbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] == "test_a")
        .unwrap();
    assert_eq!(test["connection"]["relation"], "direct");
    assert_eq!(test["connection"]["edge_kind"], "calls");
    let outbound = m.ok(
        "code_map_tests",
        json!({"selector":"tests/test_cycle.py:test_a","direction":"outbound"}),
    );
    assert!(outbound["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "a"));
    m.err(
        "code_map_tests",
        json!({"selector":"cycle.py:a","direction":"sideways"}),
        "direction must be inbound or outbound",
    );
    let explained = m.ok("code_map_explain", json!({"selector":"cycle.py:a"}));
    assert_eq!(explained["node"]["kind"], "function");
    let removal = m.ok("code_map_prove_removal", json!({"selector":"cycle.py:a"}));
    assert_eq!(removal["safe_to_remove"], false);
}

#[test]
fn mcp_search_ast_and_document_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write(
        "src/app.py",
        "def item_name(): pass\ndef itemXname(): pass\nprint(item_name())\n",
    );
    w.write(
        "docs/intro.md",
        "---\ndoc_type: guide\ntitle: Intro\n---\n# Intro\n\nA searchable contract.\n",
    );
    w.build();
    let mut m = Mcp::new(&w);
    let overview = m.ok("code_map_overview", json!({"limit":1}));
    assert!(overview["generation"].is_string());
    for pattern in ["*", "***", "_", "%", "\\", "'", "\""] {
        m.err(
            "code_map_search",
            json!({"pattern":pattern}),
            "pattern must contain",
        );
    }
    let prefix = m.ok(
        "code_map_search",
        json!({"pattern":"item*","kind":"function","limit":1}),
    );
    assert_eq!(prefix["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(prefix["nodes"]["total"], 2);
    let scoped = m.ok(
        "code_map_search",
        json!({"pattern":"item*","kind":"function","path":"src/app.py"}),
    );
    assert_eq!(scoped["nodes"]["total"], 2);
    let outside = m.ok("code_map_search", json!({"pattern":"item*","path":"docs"}));
    assert_eq!(outside["nodes"]["total"], 0);
    m.err(
        "code_map_search",
        json!({"pattern":"item*","path":"../src"}),
        "workspace-relative",
    );
    let literal_underscore = m.ok("code_map_search", json!({"pattern":"item_*"}));
    assert_eq!(literal_underscore["nodes"]["total"], 1);
    for pattern in ["item%*", "item\\*", "item'*", "item\"*"] {
        let result = m.ok("code_map_search", json!({"pattern":pattern}));
        assert_eq!(result["nodes"]["total"], 0, "{pattern}: {result}");
    }

    let ast = m.ok(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"python","path":"src/app.py"}),
    );
    assert!(!ast["matches"]["items"].as_array().unwrap().is_empty());
    m.err(
        "ast_grep_search",
        json!({"pattern":"def (","language":"python"}),
        "pattern",
    );
    m.err(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"unknown"}),
        "unknown",
    );
    m.err(
        "ast_grep_search",
        json!({"pattern":"print($VALUE)","language":"python","path":""}),
        "path",
    );

    let docs = m.ok("search_docs", json!({"query":"searchable"}));
    assert!(docs["sections"]["total"].as_u64().unwrap() >= 1);
    assert!(docs["sections"]["items"][0].get("excerpt").is_none());
    let excerpt = m.ok(
        "search_docs",
        json!({"query":"searchable","include_excerpt":true}),
    );
    assert!(excerpt["sections"]["items"][0]["excerpt"]
        .as_str()
        .unwrap()
        .contains("[searchable]"));
    m.err("search_docs", json!({"query":""}), "search query is empty");
    let quoted = m.ok("search_docs", json!({"query":"\""}));
    assert_eq!(quoted["sections"]["total"], 0);
    let filtered = m.ok(
        "search_docs",
        json!({"query":"searchable","doc_type":"no-such-type"}),
    );
    assert_eq!(filtered["sections"]["total"], 0);
    let doc = m.ok("get_doc", json!({"path_or_id":"docs/intro.md"}));
    assert!(doc["sections"]["total"].as_u64().unwrap() >= 1);
    m.err(
        "get_doc",
        json!({"path_or_id":"docs/intro.md","section":"NonExistentSection"}),
        "document or section not found",
    );
}

#[test]
fn mcp_analyze_router_checkpoint_and_guide_boundaries() {
    use serde_json::json;
    let w = Workspace::new();
    w.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    w.write("other.py", "def idle(): pass\n");
    w.build();
    let mut m = Mcp::new(&w);

    let attachment = w.db().parent().unwrap().join("sql-attachment.sqlite");
    let mut targets = vec![
        "; DROP TABLE nodes; --".to_owned(),
        "INSERT INTO metadata(key,value) VALUES ('sql-audit-sentinel','changed')".to_owned(),
        "UPDATE nodes SET kind='hacked'".to_owned(),
        "UPDATE metadata SET value='mutated' WHERE key='workspace_root'".to_owned(),
        "DELETE FROM nodes".to_owned(),
        "DELETE FROM files".to_owned(),
        "DROP TABLE nodes".to_owned(),
        "PRAGMA user_version=32767".to_owned(),
        "PRAGMA journal_mode=WAL".to_owned(),
    ];
    targets.push(format!("ATTACH DATABASE '{}' AS pwn", attachment.display()));
    let before = persisted_state(&w);
    for target in &targets {
        let response = m.call("code_map_analyze", json!({"target":target}));
        assert_eq!(
            response["result"]["isError"], true,
            "unsafe target accepted: {target}: {response}"
        );
        let message = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            message.contains("write or administrative SQL is not allowed")
                || message.contains("exactly one SELECT or WITH statement is allowed"),
            "{target}: {message}"
        );
        assert_eq!(
            persisted_state(&w),
            before,
            "SQL changed database state: {target}"
        );
        assert!(
            !attachment.exists(),
            "SQL created attached database: {target}"
        );
    }
    let multi_statement = "SELECT count(*) FROM nodes; DROP TABLE nodes";
    m.err(
        "code_map_analyze",
        json!({"target":multi_statement}),
        "exactly one SELECT or WITH statement is allowed",
    );
    assert_eq!(
        persisted_state(&w),
        before,
        "multi-statement SQL changed database state"
    );
    let count = m.ok(
        "code_map_analyze",
        json!({"target":"SELECT count(*) AS count FROM nodes","limit":1}),
    );
    assert!(count["rows"]["items"][0]["count"].as_u64().unwrap() >= 2);
    let paged = m.ok(
        "code_map_analyze",
        json!({"target":"SELECT id FROM nodes ORDER BY id","limit":1}),
    );
    assert_eq!(paged["rows"]["items"].as_array().unwrap().len(), 1);
    assert!(paged["rows"]["total"].as_u64().unwrap() > 1);
    for target in ["", "cycle.py", "other.py"] {
        let off = m.ok(
            "code_map_analyze",
            json!({"target":target,"include_cycles":false}),
        );
        assert_eq!(off["cycles"]["omitted"], true);
        let on = m.ok(
            "code_map_analyze",
            json!({"target":target,"include_cycles":true}),
        );
        if target == "other.py" {
            assert_eq!(on["cycles"]["total"], 0);
        } else {
            assert!(on["cycles"]["total"].as_u64().unwrap() >= 1);
        }
    }

    for (operation, selector) in [
        ("overview", ""),
        ("inspect", "cycle.py:a"),
        ("explain", "cycle.py:a"),
        ("impact", "cycle.py:a"),
        ("slice", "cycle.py:a"),
        ("unwired", ""),
        ("sql", "SELECT count(*) AS total FROM nodes"),
        ("doctor", ""),
        ("search", "a"),
    ] {
        m.ok(
            "code_map_query",
            json!({"operation":operation,"selector":selector,"depth":1,"limit":5}),
        );
    }
    m.err(
        "code_map_query",
        json!({"operation":"cypher","selector":"MATCH (n) RETURN n"}),
        "supported: overview,inspect,explain,impact,slice,unwired,sql",
    );
    m.err(
        "code_map_query",
        json!({"operation":"sql","selector":"SELECT 1; SELECT 2"}),
        "exactly one SELECT or WITH statement is allowed",
    );
    m.err(
        "code_map_query",
        json!({"operation":"sql"}),
        "sql operation requires a SELECT or WITH selector",
    );
    m.err(
        "code_map_query",
        json!({"operation":"delete_all"}),
        "supported: overview,inspect,explain,impact,slice,unwired,sql",
    );

    assert!(m
        .ok("session_checkpoint", json!({"action":"list"}))
        .is_object());
    m.err(
        "session_checkpoint",
        json!({"action":"save","content":{"note":"x"}}),
        "name",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"save","name":"note"}),
        "content required",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"get","name":"missing"}),
        "not found",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"delete","name":"missing"}),
        "not found",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"save","name":"../../etc/passwd","content":{"note":"x"}}),
        "path traversal",
    );
    m.err(
        "session_checkpoint",
        json!({"action":"save","name":"..\n/../etc/passwd","content":{"note":"x"}}),
        "path traversal",
    );
    m.ok(
        "session_checkpoint",
        json!({"action":"save","name":"note","content":{"state":1}}),
    );
    assert_eq!(
        m.ok("session_checkpoint", json!({"action":"get","name":"note"}))["state"],
        1
    );
    assert_eq!(
        m.ok("session_checkpoint", json!({"action":"list"})),
        json!({"note":{"bytes":11}})
    );
    m.ok(
        "session_checkpoint",
        json!({"action":"delete","name":"note"}),
    );
    m.err(
        "session_checkpoint",
        json!({"action":"get","name":"note"}),
        "not found",
    );
    let malformed = w.cli(&[
        "checkpoint",
        "save",
        "--name",
        "malformed",
        "--content",
        "{invalid",
    ]);
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("key must be a string"));
    m.err(
        "session_checkpoint",
        json!({"action":"get","name":"malformed"}),
        "not found",
    );
    w.write(".forge/checkpoints.json", "{invalid JSON");
    m.err(
        "session_checkpoint",
        json!({"action":"list"}),
        "key must be a string",
    );

    let guide = m.ok("forge_guide", json!({"topic":"query"}));
    assert!(guide["start"].is_string());
    let acdd = m.ok("forge_guide", json!({"topic":"acdd"}));
    assert!(acdd["gates"].is_array());
    assert!(acdd["proof_policies"].is_object());
    m.err(
        "forge_guide",
        json!({"topic":"no-such-topic"}),
        "unknown guide topic",
    );
}
