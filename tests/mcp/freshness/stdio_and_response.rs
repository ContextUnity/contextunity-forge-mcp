use super::*;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
#[test]
fn sighup_reloads_replaced_binary_in_same_pid_with_stdio_connected() {
    use std::os::unix::fs::MetadataExt;

    let ws = Workspace::new();
    let binary = ws.path(".forge/server");
    let replacement = ws.path(".forge/server-next");
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_contextunity-forge-mcp"), &binary).unwrap();
    let mut client = StdioClient::with_binary(&ws, &binary);
    let pid = client.child().id();
    fs::copy(&binary, &replacement).unwrap();
    fs::rename(&replacement, &binary).unwrap();
    let new_inode = fs::metadata(&binary).unwrap().ino();

    let reload = Command::new(&binary).arg("reload").output().unwrap();
    assert!(
        reload.status.success(),
        "{}",
        String::from_utf8_lossy(&reload.stderr)
    );
    let response: Value = serde_json::from_slice(&reload.stdout).unwrap();
    assert_eq!(response["signaled_pids"], json!([pid]));

    let proc_exe = PathBuf::from(format!("/proc/{pid}/exe"));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while fs::metadata(&proc_exe).unwrap().ino() != new_inode {
        assert!(
            std::time::Instant::now() < deadline,
            "server did not exec the replacement"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let listed = client.request_value("tools/list", json!({}));
    assert!(listed.get("error").is_none(), "{listed}");
    let tools = listed["result"]["tools"].as_array().expect("{listed}");
    assert!(
        tools.iter().any(|tool| tool["name"] == "task_blackboard"),
        "{listed}"
    );
    assert_eq!(client.child().id(), pid);
}

#[test]
fn mcp_document_retrieval_and_explain_output_controls() {
    let ws = Workspace::new();
    ws.write(
        "main.rs",
        "pub fn target() {}\npub fn caller() { target(); }\n",
    );
    ws.write(
        "README.md",
        "# Contract\n\nThe `target` function preserves the documented contract.\n",
    );
    let mut client = StdioClient::new(&ws);
    let document = client.payload("get_doc", json!({"path_or_id":"README.md"}));
    assert!(document["sections"]["items"][0]["content"]
        .as_str()
        .unwrap()
        .contains("documented contract"));
    let outline = client.payload(
        "get_doc",
        json!({"path_or_id":"README.md","detail":"compact"}),
    );
    assert!(outline["sections"]["items"][0].get("content").is_none());
    assert_eq!(document["sections"]["total"], outline["sections"]["total"]);
    let mut explain = |arguments: Value| client.payload("code_map_explain", arguments);
    let with_docs = explain(json!({"selector":"target"}));
    assert!(with_docs["documents"]["total"].as_u64().unwrap() > 0);
    let without_docs = explain(json!({"selector":"target","show_doc":false}));
    assert!(without_docs.get("documents").is_none() || without_docs["documents"]["total"] == 0);
    assert_eq!(with_docs["coverage"], without_docs["coverage"]);
    assert_eq!(with_docs["incoming"], without_docs["incoming"]);
    for (alias, canonical, omitted) in [
        ("inbound", "incoming", "outgoing"),
        ("outbound", "outgoing", "incoming"),
    ] {
        let alias_result = explain(json!({"selector":"target","direction":alias}));
        let canonical_result = explain(json!({"selector":"target","direction":canonical}));
        assert_eq!(alias_result["direction"], canonical);
        assert_eq!(alias_result[canonical], canonical_result[canonical]);
        assert_eq!(alias_result[omitted]["omitted"], true);
    }
}
#[test]
fn all_mcp_tools_admit_large_inventory_snapshot_without_raising_query_limit() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {}\n");
    ws.write("docs/intro.md", "# Intro\n\nCurrent documentation.\n");
    let database = ws.db();
    db::writer::build(ws.root(), &database, None).unwrap();

    let conn = rusqlite::Connection::open(&database).unwrap();
    let inventory: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='inventory_snapshot'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let padded = format!("{inventory}{}", " ".repeat(8 * 1024 * 1024));
    conn.execute(
        "UPDATE metadata SET value=?1 WHERE key='inventory_snapshot'",
        [&padded],
    )
    .unwrap();
    contextunity_forge_mcp::core::commitments::seal(&conn).unwrap();
    drop(conn);

    let mut client = StdioClient::new(&ws);
    let calls = [
        ("code_map_overview", json!({"detail":"compact","limit":1})),
        ("code_map_inspect", json!({"selector":"main.rs:current"})),
        ("get_code_snippet", json!({"selector":"main.rs:current"})),
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
        let response =
            client.request_value("tools/call", json!({"name":name,"arguments":arguments}));
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        if name == "get_code_snippet" {
            let snippet: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert!(snippet.get("source").is_some(), "{snippet}");
            assert!(snippet.get("coverage").is_none(), "{snippet}");
            assert!(snippet.get("documents").is_none(), "{snippet}");
        }
        if name == "code_map_overview" {
            let overview: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(overview["counts"]["files"], 2);
            assert!(
                overview["freshness"].get("files_checked").is_none()
                    || overview["freshness"]["files_checked"] == 2
            );
        }
    }

    ws.write("main.rs", "pub fn current() { let value = 1; }\n");
    invalidate_mcp_connection_cache(&database).unwrap();
    let response = client.request_value(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact","limit":1}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    let refreshed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(refreshed["freshness"]["refresh"], "delta");

    let server = Server::new(ws.root().to_path_buf(), database);
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

#[test]
fn stdio_response_only_adapter_transitions_preserve_facts_and_continuations() {
    let ws = Workspace::new();
    ws.write(
        "src/README.md",
        "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n",
    );
    ws.write("src/helper.md", "# Helper\ninside\n");
    ws.write("outside.md", "# Outside\noutside\n");
    let mut client = StdioClient::new(&ws);
    let before = client.payload("get_doc", json!({"path_or_id":"src/README.md"}));
    let database = ws.db();
    let identity = db::cache::identity(&database).unwrap();
    let database_hash = || scanner::digest(&fs::read(&database).unwrap());
    let before_hash = database_hash();
    let generation = before["sections"]["generation"].clone();
    assert!(generation.is_string(), "{before}");
    assert_eq!(before["freshness"]["files_checked"], 3);
    let default_limit = before["sections"]["limit"].as_u64().unwrap();
    for (configuration, limit) in [
        (Some("response: {page_size: 1}\n"), 1),
        (Some("response: {page_size: 2}\n"), 2),
        (None, default_limit),
        (Some("{}\n"), default_limit),
        (None, default_limit),
    ] {
        if let Some(configuration) = configuration {
            ws.write("forge-mcp.yaml", configuration);
        } else {
            fs::remove_file(ws.path("forge-mcp.yaml")).unwrap();
        }
        let current = client.payload("get_doc", json!({"path_or_id":"src/README.md"}));
        assert_eq!(current["sections"]["limit"], limit);
        assert_eq!(
            current["sections"]["items"].as_array().unwrap().len(),
            limit.min(4) as usize
        );
        assert!(
            current["freshness"].get("refresh").is_none()
                || current["freshness"]["refresh"] == "none",
            "{configuration:?}: {current}"
        );
        assert_eq!(current["sections"]["generation"], generation);
        for key in ["corpus_hash", "output_root"] {
            if let Some(expected) = before["freshness"].get(key) {
                if let Some(actual) = current["freshness"].get(key) {
                    assert_eq!(actual, expected);
                }
            }
        }
        let continued = client.payload(
            "get_doc",
            json!({"path_or_id":"src/README.md","offset":1,"limit":1,"generation":generation}),
        );
        assert_eq!(continued["sections"]["offset"], 1);
        assert_eq!(continued["sections"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(continued["sections"]["generation"], generation);
        assert_eq!(database_hash(), before_hash);
        assert_eq!(db::cache::identity(&database).unwrap(), identity);
    }
    for invalid in [
        "response: {page_size: 0}\n",
        "response: [\n",
        "response: {page_size: 2}\nroots: ['..']\n",
    ] {
        ws.write("forge-mcp.yaml", invalid);
        let error = client.request_value(
            "tools/call",
            json!({"name":"get_doc","arguments":{"path_or_id":"src/README.md"}}),
        );
        assert_eq!(error["result"]["isError"], true, "{invalid}: {error}");
        assert_eq!(database_hash(), before_hash);
        assert_eq!(db::cache::identity(&database).unwrap(), identity);
    }
    ws.write("forge-mcp.yaml", "response: {page_size: 1}\n");
    let recovered = client.payload(
        "get_doc",
        json!({"path_or_id":"src/README.md","offset":1,"generation":generation}),
    );
    assert!(
        recovered["freshness"].get("refresh").is_none()
            || recovered["freshness"]["refresh"] == "none"
    );
    assert_eq!(recovered["sections"]["generation"], generation);
    assert_eq!(database_hash(), before_hash);

    let mut previous_generation = generation;
    for (settings, files) in [
        ("roots: [src]\n", 2),
        ("roots: [src]\nignore: [helper.md]\n", 1),
        ("roots: [src]\nignore: [helper.md]\nadapter_version: 2\n", 1),
    ] {
        ws.write(
            "forge-mcp.yaml",
            &format!("{settings}response: {{page_size: 1}}\n"),
        );
        let changed = client.payload("get_doc", json!({"path_or_id":"src/README.md"}));
        assert_eq!(
            changed["freshness"]["refresh"], "rebuild",
            "{settings}: {changed}"
        );
        assert_eq!(changed["freshness"]["files_checked"], files);
        assert_eq!(changed["sections"]["limit"], 1);
        assert_ne!(changed["sections"]["generation"], previous_generation);
        previous_generation = changed["sections"]["generation"].clone();
    }
}
