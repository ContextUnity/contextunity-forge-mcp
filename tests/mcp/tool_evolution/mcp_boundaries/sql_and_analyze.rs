use super::*;

#[test]
fn unified_navigation_parameters_and_sql_preserve_mcp_results() {
    let (w, harness) = McpTestHarness::indexed_workspace(&[
        ("src/app.py", "def first(): return 1\n"),
        ("docs/contract.md", "# Contract\n\nA documented contract.\n"),
        ("other/contract.md", "# Outside\n\nAnother contract.\n"),
    ]);
    let mut m = harness.expect("MCP initialize");
    let catalog = m
        .request_value("tools/list", json!({}))
        .expect("MCP transport");
    let tools = crate::json_array(&catalog["result"]["tools"]);
    for (name, canonical, aliases) in [
        ("get_doc", "path_or_id", vec!["path", "selector"]),
        ("search_docs", "query", vec!["pattern", "path"]),
        ("ast_grep_search", "pattern", vec!["query"]),
        ("code_map_search", "pattern", vec!["query"]),
        ("code_map_analyze", "target", vec!["path", "selector"]),
    ] {
        let schema = &tools.iter().find(|tool| tool["name"] == name).unwrap()["inputSchema"];
        assert!(
            !schema["required"]
                .as_array()
                .is_some_and(|required| required.contains(&json!(canonical))),
            "{name} requires canonical field despite its aliases: {schema}"
        );
        for alias in aliases {
            assert!(
                schema["properties"][alias].is_object(),
                "{name} omits {alias}: {schema}"
            );
        }
    }
    for (tool, canonical, alternatives) in [
        (
            "search_docs",
            json!({"query":"contract","component":"docs","include_excerpt":true}),
            vec![json!({"pattern":"contract","path":"docs","include_excerpt":true})],
        ),
        (
            "get_doc",
            json!({"path_or_id":"docs/contract.md","detail":"compact"}),
            vec![
                json!({"path":"docs/contract.md","detail":"compact"}),
                json!({"selector":"docs/contract.md","detail":"compact"}),
            ],
        ),
        (
            "code_map_analyze",
            json!({"target":"src/app.py"}),
            vec![
                json!({"path":"src/app.py"}),
                json!({"selector":"src/app.py"}),
            ],
        ),
        ("code_map_analyze", json!({"target":""}), vec![json!({})]),
        (
            "ast_grep_search",
            json!({"pattern":"def first():","language":"python","path":"src"}),
            vec![json!({"query":"def first():","language":"python","path":"src"})],
        ),
        (
            "code_map_query",
            json!({"operation":"inspect","selector":"src/app.py:first"}),
            vec![json!({"operation":"inspect","query":"src/app.py:first"})],
        ),
        (
            "code_map_query",
            json!({"operation":"sql","selector":"SELECT count(*) FROM nodes"}),
            vec![
                json!({"sql":"SELECT count(*) FROM nodes"}),
                json!({"query":"SELECT count(*) FROM nodes"}),
                json!({"operation":"sql","query":"SELECT count(*) FROM nodes"}),
                json!({"operation":"SELECT count(*) FROM nodes"}),
                json!({"query":"SELECT\tcount(*) FROM nodes"}),
                json!({"operation":"SELECT\r\ncount(*) FROM nodes"}),
                json!({"query":"WITH\titems AS (SELECT * FROM nodes) SELECT count(*) FROM items"}),
                json!({"operation":"WITH\r\nitems AS (SELECT * FROM nodes) SELECT count(*) FROM items"}),
            ],
        ),
    ] {
        let expected = m.payload(tool, canonical).expect("MCP tool should succeed");
        for arguments in alternatives {
            assert_eq!(
                m.payload(tool, arguments.clone())
                    .expect("MCP tool should succeed"),
                expected,
                "{tool}: {arguments}"
            );
        }
    }
    for (tool, arguments) in [
        (
            "get_doc",
            json!({"path":"docs/contract.md","selector":"docs/contract.md"}),
        ),
        (
            "search_docs",
            json!({"query":"contract","pattern":"contract"}),
        ),
        (
            "search_docs",
            json!({"query":"contract","component":"docs","path":"docs"}),
        ),
        (
            "ast_grep_search",
            json!({"pattern":"def first():","query":"def first():","language":"python"}),
        ),
        (
            "code_map_search",
            json!({"pattern":"first","query":"first"}),
        ),
        (
            "code_map_analyze",
            json!({"target":"src/app.py","path":"src/app.py"}),
        ),
    ] {
        let rejection = m
            .text(tool, arguments)
            .expect_err("tool should reject input");
        let expected: &str = "duplicate field";
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
    }
    let expected = m
        .payload(
            "code_map_query",
            json!({"sql":"SELECT count(*) FROM nodes"}),
        )
        .expect("MCP tool should succeed");
    for sql in [
        "SELECT\tcount(*) FROM nodes",
        "SELECT\r\ncount(*) FROM nodes",
        "WITH\titems AS (SELECT * FROM nodes) SELECT count(*) FROM items",
        "WITH\r\nitems AS (SELECT * FROM nodes) SELECT count(*) FROM items",
    ] {
        let result = w.query(&["query", "run", sql]).expect("CLI should succeed");
        assert_eq!(result["rows"]["items"], expected["rows"]["items"]);
    }
    m.finish().expect("server must finish successfully");
}

#[test]
fn analyze_sql_rejects_writes_and_administrative_statements_without_mutation() {
    let fixture = AnalysisFixture::indexed();
    let w = fixture.workspace();
    let mut m = fixture
        .start_mcp()
        .expect("indexed MCP fixture should initialize");
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
    let before = persisted_state(w).expect("capture persisted SQLite state");
    let marker = PersistedWriteMarker::new(w);
    for target in &targets {
        let (_, response) = m
            .call("code_map_analyze", json!({"target":target}))
            .expect("MCP transport");
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
            marker.current(),
            *marker.baseline(),
            "read-only query must not mutate SQLite state: {target}"
        );
        assert!(
            !attachment.exists(),
            "SQL created attached database: {target}"
        );
    }
    let multi_statement = "SELECT count(*) FROM nodes; DROP TABLE nodes";
    let rejection = m
        .text("code_map_analyze", json!({"target":multi_statement}))
        .expect_err("tool should reject input");
    let expected: &str = "exactly one SELECT or WITH statement is allowed";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    assert_eq!(
        marker.current(),
        *marker.baseline(),
        "read-only query must not mutate SQLite state: {multi_statement}"
    );
    let commented_multi = "/* ; */ SELECT 1; SELECT 2";
    let cte_write = "WITH value AS (SELECT 1) DELETE FROM nodes";
    for (tool, args, expected_error) in [
        (
            "code_map_analyze",
            json!({"target":commented_multi}),
            Some("exactly one SELECT or WITH statement is allowed"),
        ),
        (
            "code_map_query",
            json!({"operation":"sql","selector":commented_multi}),
            Some("exactly one SELECT or WITH statement is allowed"),
        ),
        (
            "code_map_analyze",
            json!({"target":cte_write}),
            Some("query must be read-only"),
        ),
        (
            "code_map_query",
            json!({"operation":"sql","selector":cte_write}),
            Some("query must be read-only"),
        ),
        (
            "code_map_query",
            json!({"query":cte_write}),
            Some("query must be read-only"),
        ),
        (
            "code_map_query",
            json!({"sql":"SELECT 1; DROP TABLE nodes"}),
            Some("exactly one SELECT or WITH statement is allowed"),
        ),
        (
            "code_map_query",
            json!({"operation":"SELECT 1; DROP TABLE nodes"}),
            Some("exactly one SELECT or WITH statement is allowed"),
        ),
    ] {
        let argument_details = args.to_string();
        let (_, response) = m.call(tool, args).expect("MCP transport");
        assert_eq!(response["result"]["isError"], true, "{tool}: {response}");
        if let Some(expected_error) = expected_error {
            let message = response["result"]["content"][0]["text"]
                .as_str()
                .expect("SQL rejection should include a tool message");
            assert!(
                message.contains(expected_error),
                "{tool} returned an unexpected SQL rejection: {message}"
            );
        }
        assert_eq!(
            marker.current(),
            *marker.baseline(),
            "read-only query must not mutate SQLite state: {argument_details}"
        );
    }
    assert_eq!(
        persisted_state(w).expect("capture persisted SQLite state"),
        before
    );
    m.finish().expect("server must finish successfully");
}

#[test]
fn sql_tools_preserve_literal_public_values_and_internal_structural_rows() {
    let fixture = AnalysisFixture::indexed();
    let mut m = fixture
        .start_mcp()
        .expect("indexed MCP fixture should initialize");
    let count = m
        .payload(
            "code_map_analyze",
            json!({"target":"SELECT count(*) AS count FROM nodes","limit":1}),
        )
        .expect("MCP tool should succeed");
    assert!(count["rows"]["items"][0]["count"].as_u64().unwrap() >= 2);
    for sql in [
        "SELECT ';' AS marker",
        "SELECT ';' AS \"semi;column\"",
        "/* ; */ SELECT ';' AS marker",
        "-- ; comment\nSELECT ';' AS marker",
        "SELECT ';' AS marker /* ; */;  \n",
        "SELECT ';' AS marker -- trailing ; comment",
        "WITH value AS (SELECT ';' AS marker) SELECT marker FROM value;",
    ] {
        for (tool, args) in [
            ("code_map_analyze", json!({"target":sql})),
            ("code_map_query", json!({"operation":"sql","selector":sql})),
        ] {
            let result = m.payload(tool, args).expect("MCP tool should succeed");
            let row = result["rows"]["items"][0].as_object().unwrap();
            assert_eq!(
                row.values().collect::<Vec<_>>(),
                vec![&json!(";")],
                "{tool}: {sql}"
            );
        }
    }
    let sql = "SELECT 1 AS path_id, 2 AS owner_path_id, 'node' AS node_hash, 'path' AS path_hash, '{\"a\":1}' AS details, 'null' AS invariants, '[\"symbol\"]' AS referenced_symbols, 'raw text' AS note";
    for (tool, args) in [
        ("code_map_analyze", json!({"target":sql})),
        ("code_map_query", json!({"operation":"sql","selector":sql})),
    ] {
        let result = m.payload(tool, args).expect("MCP tool should succeed");
        assert_eq!(
            result["rows"]["items"][0],
            json!({
                "path_id":1,"owner_path_id":2,"node_hash":"node","path_hash":"path",
                "details":"{\"a\":1}","invariants":"null","referenced_symbols":"[\"symbol\"]","note":"raw text"
            }),
            "{tool}"
        );
    }
    let structural_rows = fixture
        .rows(
            "SELECT 1 AS path_id, 2 AS owner_path_id, 'node' AS node_hash, 'path' AS path_hash, '{\"a\":1}' AS details, 'null' AS invariants, '[\"symbol\"]' AS referenced_symbols",
            &[],
            1,
        )
        .expect("read structural literal rows from the indexed fixture");
    assert_eq!(
        structural_rows[0],
        json!({
            "details":{"a":1},
            "invariants":null,
            "referenced_symbols":["symbol"]
        })
    );
    let paged = m
        .payload(
            "code_map_analyze",
            json!({"target":"SELECT id FROM nodes ORDER BY id","limit":1}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(crate::json_array(&paged["rows"]["items"]).len(), 1);
    assert!(paged["rows"]["total"].as_u64().unwrap() > 1);
    m.finish().expect("server must finish successfully");
}

#[test]
fn query_router_accepts_registered_operations_and_rejects_invalid_sql_and_names() {
    let fixture = AnalysisFixture::indexed();
    let mut m = fixture
        .start_mcp()
        .expect("indexed MCP fixture should initialize");
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
        m.payload(
            "code_map_query",
            json!({"operation":operation,"selector":selector,"depth":1,"limit":5}),
        )
        .expect("MCP tool should succeed");
    }
    let supported = "supported: overview,inspect,explain,impact,slice,unwired,sql";
    for (arguments, expected) in [
        (
            json!({"operation":"cypher","selector":"MATCH (n) RETURN n"}),
            supported,
        ),
        (
            json!({"operation":"sql"}),
            "sql operation requires a SELECT or WITH selector",
        ),
        (json!({"operation":"delete_all"}), supported),
    ] {
        let rejection = m
            .text("code_map_query", arguments)
            .expect_err("tool should reject input");
        let expected: &str = expected;
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
    }
    m.finish().expect("server must finish successfully");
}

#[test]
fn checkpoint_crud_enforces_scope_and_uses_sqlite_instead_of_legacy_json() {
    let (w, m) =
        McpTestHarness::with_files(&[("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n")]);
    let mut m = m.expect("MCP initialize");
    assert!(m
        .payload("session_checkpoint", json!({"action":"list"}))
        .expect("MCP tool should succeed")
        .is_object());
    for (arguments, expected) in [
        (
            json!({"action":"save","content":{"note":"x"}}),
            "checkpoint breadcrumb required",
        ),
        (
            json!({"action":"save","name":"forge-mcp/forge-mcp/m-test/tasks/note/build"}),
            "content required",
        ),
        (
            json!({"action":"get","name":"forge-mcp/forge-mcp/m-test/tasks/missing/build"}),
            "not found",
        ),
        (
            json!({"action":"delete","name":"forge-mcp/forge-mcp/m-test/tasks/missing/build"}),
            "not found",
        ),
        (
            json!({"action":"save","name":"../../etc/passwd","content":{"note":"x"}}),
            "TASK_CHECKPOINT_SCOPE",
        ),
        (
            json!({"action":"save","name":"..\n/../etc/passwd","content":{"note":"x"}}),
            "TASK_CHECKPOINT_SCOPE",
        ),
    ] {
        let rejection = m
            .text("session_checkpoint", arguments)
            .expect_err("tool should reject input");
        let expected: &str = expected;
        assert!(
            rejection
                .tool_message()
                .expect("expected MCP tool rejection")
                .contains(expected),
            "{rejection}"
        );
    }
    m.payload(
        "session_checkpoint",
        json!({"action":"save","name":"forge-mcp/forge-mcp/m-test/tasks/note/build","content":{"state":1}}),
    ).expect("MCP tool should succeed");
    assert_eq!(
        m.payload(
            "session_checkpoint",
            json!({"action":"get","name":"forge-mcp/forge-mcp/m-test/tasks/note/build"})
        )
        .expect("MCP tool should succeed")["state"],
        1
    );
    assert_eq!(
        m.payload("session_checkpoint", json!({"action":"list"}))
            .expect("MCP tool should succeed"),
        json!({"forge-mcp/forge-mcp/m-test/tasks/note/build":{"bytes":11}})
    );
    m.payload(
        "session_checkpoint",
        json!({"action":"delete","name":"forge-mcp/forge-mcp/m-test/tasks/note/build"}),
    )
    .expect("MCP tool should succeed");
    let rejection = m
        .text(
            "session_checkpoint",
            json!({"action":"get","name":"forge-mcp/forge-mcp/m-test/tasks/note/build"}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "not found";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    let malformed = run_cli(
        &w,
        &[
            "checkpoint",
            "save",
            "--name",
            "malformed",
            "--content",
            "{invalid",
        ],
    );
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("key must be a string"));
    let rejection = m
        .text(
            "session_checkpoint",
            json!({"action":"get","name":"forge-mcp/forge-mcp/m-test/tasks/malformed/build"}),
        )
        .expect_err("tool should reject input");
    let expected: &str = "not found";
    assert!(
        rejection
            .tool_message()
            .expect("expected MCP tool rejection")
            .contains(expected),
        "{rejection}"
    );
    w.write(".forge/checkpoints.json", "{invalid JSON");
    assert_eq!(
        m.payload("session_checkpoint", json!({"action":"list"}))
            .expect("MCP tool should succeed"),
        json!({}),
        "legacy JSON checkpoint files do not affect the SQLite checkpoint store"
    );
    m.finish().expect("server must finish successfully");
}
