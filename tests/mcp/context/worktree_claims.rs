use super::*;

#[test]
fn mcp_worktree_selectors_preserve_claim_independent_defaults() {
    let workspace = Workspace::with_files(&[
        (
            "forge-mcp.yaml",
            "roots:\n  - src\ntasks_db: .forge/tasks.sqlite\n",
        ),
        ("src/main.rs", "pub fn primary_func() {}\n"),
    ]);
    std::fs::create_dir_all(workspace.path("src")).unwrap();

    let worktree_dir = workspace.path(".worktrees/021-task");
    std::fs::create_dir_all(worktree_dir.join("src")).unwrap();
    std::fs::write(
        worktree_dir.join("src/semantic.rs"),
        "pub fn semantic_worktree_func() {}\n",
    )
    .unwrap();
    let selected_config = worktree_dir.join("forge-mcp.yaml");
    std::fs::write(&selected_config, "roots: [src]\n").unwrap();

    let mut client = McpTestHarness::new(&workspace).expect("MCP initialize");

    let get_workspace_root = |v: &Value| -> String {
        crate::json_array(&v["metadata"])
            .iter()
            .find(|item| item["key"] == "workspace_root")
            .unwrap()["value"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // 1. Without claim and without worktree parameter -> returns primary workspace
    let overview_primary = client
        .payload(
            "code_map_overview",
            json!({"aspects":["counts","metadata"]}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        get_workspace_root(&overview_primary),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    // 2. Explicit worktree parameter -> scopes to worktree and auto-indexes
    let wt_str = worktree_dir.to_str().unwrap();
    let selected_index = worktree_dir.join(".forge/code-map.sqlite");
    assert!(!selected_index.exists());
    let invalid_formats = [
        ("escaping string", json!("\0".repeat(2_000))),
        ("non-string", json!(false)),
    ];
    let checkpoint_path = workspace.path(".forge/checkpoints.json");
    let checkpoint_before = fs::read(&checkpoint_path).ok();
    let mut invalid_policy_failures = Vec::new();
    std::fs::write(
        &selected_config,
        "roots: [src]\nresponse:\n  max_output_bytes: 1024\n",
    )
    .unwrap();
    for (label, invalid_format) in &invalid_formats {
        let mut arguments = json!({"worktree":wt_str});
        arguments["format"] = invalid_format.clone();
        let (wire_bytes, response) = client
            .call("code_map_overview", arguments)
            .expect("MCP transport");
        let is_error = response["result"]["isError"] == true;
        let error_code = response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value["error"]["code"].as_str().map(str::to_owned))
            .unwrap_or_default();
        if wire_bytes > 1024 || !is_error || error_code != "INVALID_FORMAT" {
            invalid_policy_failures.push(format!(
                "explicit selected root {label}: {wire_bytes} wire bytes, isError={is_error}, code={error_code:?}"
            ));
        }
        if selected_index.exists() {
            invalid_policy_failures.push(format!(
                "explicit selected root {label} created its index before rejection"
            ));
        }
        if fs::read(&checkpoint_path).ok() != checkpoint_before {
            invalid_policy_failures.push(format!(
                "explicit selected root {label} changed checkpoint bytes before rejection"
            ));
        }
    }
    std::fs::write(&selected_config, "roots: [src]\n").unwrap();

    let overview_wt = client
        .payload(
            "code_map_overview",
            json!({"worktree": wt_str, "aspects":["counts","metadata"]}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        get_workspace_root(&overview_wt),
        worktree_dir.canonicalize().unwrap().to_str().unwrap()
    );

    // Search in worktree finds semantic_worktree_func
    let search_wt = client
        .search(
            "semantic_worktree_func",
            json!({ "exact": true, "worktree": wt_str}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(search_wt["nodes"]["total"], 1);
    assert_eq!(
        search_wt["nodes"]["items"][0]["name"],
        "semantic_worktree_func"
    );
    workspace.write("task/src/lib.rs", "pub fn local_shadow() {}\n");

    for alias in ["021-task", "task", "021", "21", "021-ta", "1-tas"] {
        let result = client
            .search(
                "semantic_worktree_func",
                json!({ "exact":true, "worktree":alias}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(result["nodes"]["total"], 1, "alias {alias}: {result}");
    }
    for (name, symbol) in [
        ("023-sql-schema-topology", "schema_only"),
        ("060-acdd", "acdd_only"),
        ("099-tail-021", "tail_only"),
        ("099-tail-schema", "suffix_only"),
    ] {
        workspace.write(
            format!(".worktrees/{name}/src/lib.rs"),
            &format!("pub fn {symbol}() {{}}\n"),
        );
        workspace.write(
            format!(".worktrees/{name}/forge-mcp.yaml"),
            "roots: [src]\n",
        );
    }
    for (alias, name, symbol) in [
        ("023", "023-sql-schema-topology", "schema_only"),
        ("23", "023-sql-schema-topology", "schema_only"),
        ("023-sql", "023-sql-schema-topology", "schema_only"),
        ("topology", "023-sql-schema-topology", "schema_only"),
        ("sql-schema", "023-sql-schema-topology", "schema_only"),
        ("060", "060-acdd", "acdd_only"),
        ("60", "060-acdd", "acdd_only"),
        ("021", "021-task", "semantic_worktree_func"),
        ("schema", "099-tail-schema", "suffix_only"),
    ] {
        let overview = client
            .payload(
                "code_map_overview",
                json!({"worktree":alias, "aspects":["metadata"]}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(
            get_workspace_root(&overview),
            workspace
                .path(format!(".worktrees/{name}"))
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap(),
            "selector {alias}"
        );
        let result = client
            .payload(
                "code_map_search",
                json!({"worktree":alias, "pattern":symbol, "exact":true}),
            )
            .expect("MCP tool should succeed");
        assert_eq!(
            result["nodes"]["items"][0]["name"], symbol,
            "{alias}: {result}"
        );
    }
    let explicit = client
        .payload(
            "code_map_search",
            json!({"worktree":"./task", "pattern":"local_shadow", "exact":true}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(explicit["nodes"]["items"][0]["name"], "local_shadow");

    // Search in primary does NOT find semantic_worktree_func
    let search_primary = client
        .search("semantic_worktree_func", json!({ "exact": true}))
        .expect("MCP tool should succeed");
    assert_eq!(search_primary["nodes"]["total"], 0);

    // 3. Invalid worktree parameter -> returns tool error
    for invalid in ["nonexistent_dir_12345", "", "src/main.rs"] {
        let (_, invalid_res) = client
            .call("code_map_overview", json!({"worktree": invalid}))
            .expect("MCP transport");
        assert_eq!(invalid_res["result"]["isError"], true);
        assert!(
            invalid_res["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("WORKTREE_NOT_FOUND"),
            "{invalid_res}"
        );
    }

    // Active claims cannot redirect an omitted worktree selector.
    let tasks_db_path = workspace.path(".forge/tasks.sqlite");
    std::fs::create_dir_all(tasks_db_path.parent().unwrap()).unwrap();
    let claims = rusqlite::Connection::open(&tasks_db_path).unwrap();
    claims
        .execute_batch("CREATE TABLE tasks(task_id TEXT PRIMARY KEY, milestone_ref TEXT, descriptor TEXT, status TEXT); CREATE TABLE task_claims(task_id TEXT, revision INTEGER, worker_id TEXT, worktree TEXT, claimed_at INTEGER, ended INTEGER, PRIMARY KEY(task_id, revision));")
        .unwrap();
    let canonical_wt = worktree_dir.canonicalize().unwrap();
    claims
        .execute(
            "INSERT INTO task_claims(task_id, revision, worker_id, worktree, claimed_at, ended) VALUES(?1, 1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                "021-task",
                "worker-1",
                canonical_wt.to_str().unwrap(),
                1000_i64
            ],
        )
        .unwrap();

    std::fs::write(
        &selected_config,
        "roots: [src]\nresponse:\n  max_output_bytes: 1024\n",
    )
    .unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/tasks.sqlite\nresponse:\n  max_output_bytes: 1024\n",
    );
    let index_before_auto_rejection = fs::read(&selected_index).unwrap();
    let tasks_db_before_auto_rejection = fs::read(&tasks_db_path).unwrap();
    for (label, invalid_format) in &invalid_formats {
        let mut arguments = json!({});
        arguments["format"] = invalid_format.clone();
        let (wire_bytes, response) = client
            .call("code_map_overview", arguments)
            .expect("MCP transport");
        let is_error = response["result"]["isError"] == true;
        let error_code = response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value["error"]["code"].as_str().map(str::to_owned))
            .unwrap_or_default();
        if wire_bytes > 1024 || !is_error || error_code != "INVALID_FORMAT" {
            invalid_policy_failures.push(format!(
                "default server root {label}: {wire_bytes} wire bytes, isError={is_error}, code={error_code:?}"
            ));
        }
        if fs::read(&selected_index).ok().as_deref() != Some(index_before_auto_rejection.as_slice())
        {
            invalid_policy_failures.push(format!(
                "default server root {label} changed another root's index during rejection"
            ));
        }
        if fs::read(&tasks_db_path).ok().as_deref()
            != Some(tasks_db_before_auto_rejection.as_slice())
        {
            invalid_policy_failures.push(format!(
                "default server root {label} changed task database bytes during rejection"
            ));
        }
        if fs::read(&checkpoint_path).ok() != checkpoint_before {
            invalid_policy_failures.push(format!(
                "default server root {label} changed checkpoint bytes during rejection"
            ));
        }
    }
    assert!(
        invalid_policy_failures.is_empty(),
        "selected-root invalid-format contract failed: {}",
        invalid_policy_failures.join("; ")
    );
    std::fs::write(&selected_config, "roots: [src]\n").unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/tasks.sqlite\n",
    );

    // Active claims never change the default server root.
    let overview_auto = client
        .payload(
            "code_map_overview",
            json!({"aspects":["counts","metadata"]}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        get_workspace_root(&overview_auto),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    // Default searches retain the primary corpus with active claims.
    let search_auto = client
        .search("semantic_worktree_func", json!({ "exact": true}))
        .expect("MCP tool should succeed");
    assert_eq!(search_auto["nodes"]["total"], 0);

    // Multiple claims, including path aliases, cannot redirect the server.
    claims
        .execute(
            "INSERT INTO task_claims(task_id, revision, worker_id, worktree, claimed_at, ended) VALUES(?1, 1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                "same-tree",
                "worker-2",
                std::path::Path::new(".worktrees/021-task/../021-task")
                    .to_str()
                    .unwrap(),
                2000_i64
            ],
        )
        .unwrap();
    assert_eq!(
        get_workspace_root(
            &client
                .payload("code_map_overview", json!({"aspects":["metadata"]}))
                .expect("MCP tool should succeed")
        ),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    let other = workspace.path(".worktrees/022-task");
    fs::create_dir_all(other.join("src")).unwrap();
    fs::write(other.join("src/lib.rs"), "pub fn other_tree() {}\n").unwrap();
    claims
        .execute(
            "INSERT INTO task_claims(task_id, revision, worker_id, worktree, claimed_at, ended) VALUES(?1, 1, ?2, ?3, ?4, 0)",
            rusqlite::params!["other-tree", "worker-3", other.to_str().unwrap(), 3000_i64],
        )
        .unwrap();
    for arguments in [json!({"worktree":"task"}), json!({"worktree":"02"})] {
        let (_, response) = client
            .call("code_map_overview", arguments)
            .expect("MCP transport");
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("WORKTREE_AMBIGUOUS"),
            "{response}"
        );
    }
    assert_eq!(
        get_workspace_root(
            &client
                .payload(
                    "code_map_overview",
                    json!({"worktree":"021-task", "aspects":["metadata"]})
                )
                .expect("MCP tool should succeed")
        ),
        canonical_wt.to_str().unwrap()
    );
    assert_eq!(
        get_workspace_root(
            &client
                .payload(
                    "code_map_overview",
                    json!({"worktree":".", "aspects":["metadata"]})
                )
                .expect("MCP tool should succeed")
        ),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );
    claims
        .execute(
            "UPDATE task_claims SET ended=1 WHERE task_id != ?1",
            ["021-task"],
        )
        .unwrap();

    // Ending a claim also leaves the server root unchanged.
    claims
        .execute(
            "UPDATE task_claims SET ended=?1 WHERE task_id=?2",
            rusqlite::params![true, "021-task"],
        )
        .unwrap();
    let overview_ended = client
        .payload(
            "code_map_overview",
            json!({"aspects":["counts","metadata"]}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        get_workspace_root(&overview_ended),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    // Changing task-store settings does not select a different code corpus.
    claims
        .execute(
            "UPDATE task_claims SET ended=?1 WHERE task_id=?2",
            rusqlite::params![false, "021-task"],
        )
        .unwrap();
    let _alternate =
        TaskClaimsFixture::worktrees_only(&workspace, ".forge/alternate-tasks.sqlite", &[&other]);
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/alternate-tasks.sqlite\n",
    );
    assert_eq!(
        get_workspace_root(
            &client
                .payload("code_map_overview", json!({"aspects":["metadata"]}))
                .expect("MCP tool should succeed")
        ),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );

    // Missing, empty, and changed configuration retain the same default root.
    let primary = workspace.root().canonicalize().unwrap();
    for (configuration, expected) in [
        (None, &primary),
        (Some("{}\n"), &primary),
        (None, &primary),
        (
            Some("roots: [src]\ntasks_db: .forge/alternate-tasks.sqlite\n"),
            &primary,
        ),
    ] {
        match configuration {
            Some(content) => workspace.write("forge-mcp.yaml", content),
            None => workspace.remove("forge-mcp.yaml"),
        }
        assert_eq!(
            get_workspace_root(
                &client
                    .payload("code_map_overview", json!({"aspects":["metadata"]}))
                    .expect("MCP tool should succeed")
            ),
            expected.to_str().unwrap(),
            "configuration {configuration:?}"
        );
    }

    #[cfg(unix)]
    {
        let alias = workspace.path(".forge/alias-tasks.sqlite");
        std::os::unix::fs::symlink("alternate-tasks.sqlite", &alias).unwrap();
        workspace.write(
            "forge-mcp.yaml",
            "roots: [src]\ntasks_db: .forge/alias-tasks.sqlite\n",
        );
        for (target, expected) in [
            ("alternate-tasks.sqlite", primary.clone()),
            ("tasks.sqlite", primary.clone()),
            ("missing-tasks.sqlite", primary.clone()),
        ] {
            fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(target, &alias).unwrap();
            assert_eq!(
                get_workspace_root(
                    &client
                        .payload("code_map_overview", json!({"aspects":["metadata"]}))
                        .expect("MCP tool should succeed")
                ),
                expected.to_str().unwrap(),
                "task DB symlink target {target}"
            );
        }
    }

    workspace.write("forge-mcp.yaml", "roots: [src]\ntasks_db: [invalid]\n");
    let (_, malformed) = client
        .call("code_map_overview", json!({}))
        .expect("MCP transport");
    assert_eq!(malformed["result"]["isError"], true, "{malformed}");
    assert!(
        malformed["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("TASK_DATABASE_COLLISION"),
        "{malformed}"
    );
    // An explicit target does not depend on the primary task-store configuration.
    assert_eq!(
        client
            .search(
                "semantic_worktree_func",
                json!({ "exact":true, "worktree":"021-task"
                })
            )
            .expect("MCP tool should succeed")["nodes"]["total"],
        1
    );
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ntasks_db: .forge/alternate-tasks.sqlite\n",
    );
    assert_eq!(
        get_workspace_root(
            &client
                .payload("code_map_overview", json!({"aspects":["metadata"]}))
                .expect("MCP tool should succeed")
        ),
        workspace.root().canonicalize().unwrap().to_str().unwrap()
    );
    workspace.write(".worktrees/task/src/lib.rs", "pub fn exact_only() {}\n");
    let exact = client
        .payload(
            "code_map_search",
            json!({"worktree":"task", "pattern":"exact_only", "exact":true}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(exact["nodes"]["items"][0]["name"], "exact_only");
    client.finish().expect("server must finish successfully");
}
