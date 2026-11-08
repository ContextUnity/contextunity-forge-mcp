use super::*;

fn large_inventory_workspace() -> Workspace {
    let ws = Workspace::with_files(&[
        ("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n"),
        ("main.rs", "pub fn current() {}\n"),
        ("docs/intro.md", "# Intro\n\nCurrent documentation.\n"),
    ]);

    let database = ws.db();
    ws.build_to(&database);

    {
        let conn = rusqlite::Connection::open(&database).unwrap();
        let inventory: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='inventory_snapshot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE metadata SET value=?1 WHERE key='inventory_snapshot'",
            [format!("{inventory}{}", " ".repeat(8 * 1024 * 1024))],
        )
        .unwrap();
        contextunity_forge_mcp::core::commitments::seal(&conn).unwrap();
    }

    ws
}

#[test]
fn all_mcp_tools_admit_large_inventory_snapshot_without_raising_query_limit() {
    let ws = large_inventory_workspace();
    let mut client = McpTestHarness::new(&ws).expect("MCP initialize");
    let calls = [
        (
            "code_map_overview",
            json!({"detail":"compact","limit":1,"format":"json"}),
        ),
        ("code_map_inspect", json!({"selector":"main.rs:current"})),
        (
            "forge_read",
            json!({"selector":"main.rs:current","format":"json"}),
        ),
        (
            "code_map_search",
            json!({"pattern":"current","detail":"compact","limit":10}),
        ),
        ("code_map_tests", json!({"selector":"main.rs:current"})),
        (
            "code_map_impact",
            json!({"selector":"main.rs:current","depth":1}),
        ),
        ("code_map_explain", json!({"selector":"main.rs:current"})),
        ("code_map_query", json!({"operation":"overview"})),
        ("code_map_analyze", json!({"target":""})),
        (
            "code_map_prove_removal",
            json!({"selector":"main.rs:current"}),
        ),
        (
            "ast_grep_search",
            json!({"pattern":"pub fn current() {}","language":"rust","path":"main.rs"}),
        ),
        ("search_docs", json!({"query":"documentation"})),
        ("get_doc", json!({"path_or_id":"docs/intro.md"})),
        ("forge_guide", json!({"topic":"query"})),
        ("session_checkpoint", json!({"action":"list"})),
    ];
    for (name, arguments) in calls {
        let text = response_text(&mut client, name, arguments);
        if name == "forge_read" {
            let snippet: Value = serde_json::from_str(&text).unwrap();
            assert!(snippet.get("source").is_some(), "{snippet}");
            assert!(snippet.get("coverage").is_none(), "{snippet}");
            assert!(snippet.get("documents").is_none(), "{snippet}");
        }
        if name == "code_map_overview" {
            let overview: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(overview["counts"]["files"], 2);
            assert!(
                overview["freshness"].get("files_checked").is_none()
                    || overview["freshness"]["files_checked"] == 2
            );
        }
    }

    let database = ws.db();
    ws.write("main.rs", "pub fn current() { let value = 1; }\n");
    invalidate_mcp_connection_cache(&database).unwrap();
    let response = response_text(
        &mut client,
        "code_map_overview",
        json!({"detail":"compact","limit":1,"format":"json"}),
    );
    let refreshed: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(refreshed["freshness"]["refresh"], "delta");

    client.finish().expect("server must finish successfully");

    let server = Server::new(ws.root().to_path_buf(), ws.db())
        .with_cache_dir(ws.path(".forge/extraction-cache"));
    server
        .read(|conn| {
            assert_eq!(
                conn.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH),
                8 * 1024 * 1024
            );
            Ok(Value::Null)
        })
        .unwrap();
}

struct ResponseFixture {
    workspace: Workspace,
    client: McpTestHarness,
    initial: Value,
    identity: db::cache::Identity,
    digest: String,
}

impl ResponseFixture {
    fn new() -> anyhow::Result<Self> {
        let workspace = Workspace::with_files(&[
            (
                "src/README.md",
                "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n",
            ),
            ("src/helper.md", "# Helper\ninside\n"),
            ("outside.md", "# Outside\noutside\n"),
        ]);

        let mut client = McpTestHarness::new(&workspace)?;
        let initial = client.document("src/README.md", json!({}))?;
        let identity = db::cache::identity(&workspace.db())?;
        let digest = scanner::digest(&fs::read(workspace.db())?);
        Ok(Self {
            workspace,
            client,
            initial,
            identity,
            digest,
        })
    }

    fn digest(&self) -> String {
        scanner::digest(&fs::read(self.workspace.db()).unwrap())
    }

    fn finish(self) -> crate::harness::HarnessResult<()> {
        self.client.finish()
    }
}

#[test]
fn adapter_changes_preserve_or_rebuild_facts_in_one_workspace() {
    let mut fixture = ResponseFixture::new().expect("response fixture should initialize");
    let generation = fixture.initial["sections"]["generation"].clone();
    assert!(generation.is_string());
    assert_eq!(fixture.initial["freshness"]["files_checked"], 3);
    let default_limit = fixture.initial["sections"]["limit"].as_u64().unwrap();
    for (configuration, limit) in [
        (Some("response: {page_size: 1}\n"), 1),
        (Some("response: {page_size: 2}\n"), 2),
        (None, default_limit),
        (Some("{}\n"), default_limit),
        (None, default_limit),
    ] {
        if let Some(configuration) = configuration {
            fixture.workspace.write("forge-mcp.yaml", configuration);
        } else {
            fs::remove_file(fixture.workspace.path("forge-mcp.yaml")).unwrap();
        }
        let current = fixture
            .client
            .document("src/README.md", json!({}))
            .expect("MCP tool should succeed");
        assert_eq!(current["sections"]["limit"], limit);
        assert_eq!(
            crate::json_array(&current["sections"]["items"]).len(),
            limit.min(4) as usize
        );
        assert!(
            current["freshness"].get("refresh").is_none()
                || current["freshness"]["refresh"] == "none"
        );
        assert_eq!(current["sections"]["generation"], generation);
        for key in ["corpus_hash", "output_root"] {
            if let (Some(expected), Some(actual)) = (
                fixture.initial["freshness"].get(key),
                current["freshness"].get(key),
            ) {
                assert_eq!(actual, expected);
            }
        }
        let continued = fixture
            .client
            .document(
                "src/README.md",
                json!({"offset":1,"limit":1,"generation":generation}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(continued["sections"]["offset"], 1);
        assert_eq!(crate::json_array(&continued["sections"]["items"]).len(), 1);
        assert_eq!(continued["sections"]["generation"], generation);
        assert_eq!(fixture.digest(), fixture.digest);
        assert_eq!(
            db::cache::identity(&fixture.workspace.db()).unwrap(),
            fixture.identity
        );
    }
    for invalid in [
        "response: {page_size: 0}\n",
        "response: [\n",
        "response: {page_size: 2}\nroots: ['..']\n",
    ] {
        fixture.workspace.write("forge-mcp.yaml", invalid);
        let error = fixture
            .client
            .request_value(
                "tools/call",
                json!({"name":"get_doc","arguments":{"path_or_id":"src/README.md"}}),
            )
            .expect("MCP transport");
        assert_eq!(error["result"]["isError"], true, "{invalid}: {error}");
        assert_eq!(fixture.digest(), fixture.digest);
        assert_eq!(
            db::cache::identity(&fixture.workspace.db()).unwrap(),
            fixture.identity
        );
    }
    fixture
        .workspace
        .write("forge-mcp.yaml", "response: {page_size: 1}\n");
    let recovered = fixture
        .client
        .document("src/README.md", json!({"offset":1,"generation":generation}))
        .expect("document recovery should succeed");
    assert!(
        recovered["freshness"].get("refresh").is_none()
            || recovered["freshness"]["refresh"] == "none"
    );
    assert_eq!(recovered["sections"]["generation"], generation);
    assert_eq!(fixture.digest(), fixture.digest);

    let mut previous_generation = generation.clone();
    for (settings, files) in [
        ("roots: [src]\n", 2),
        ("roots: [src]\nignore: [helper.md]\n", 1),
        ("roots: [src]\nignore: [helper.md]\nadapter_version: 2\n", 1),
    ] {
        fixture.workspace.write(
            "forge-mcp.yaml",
            &format!("{settings}response: {{page_size: 1}}\n"),
        );
        let changed = fixture
            .client
            .document("src/README.md", json!({}))
            .expect("MCP tool should succeed");
        assert_eq!(
            changed["freshness"]["refresh"], "rebuild",
            "{settings}: {changed}"
        );
        assert_eq!(changed["freshness"]["files_checked"], files);
        assert_eq!(changed["sections"]["limit"], 1);
        assert_ne!(changed["sections"]["generation"], previous_generation);
        previous_generation = changed["sections"]["generation"].clone();
    }
    fixture
        .finish()
        .expect("response server must finish successfully");
}
