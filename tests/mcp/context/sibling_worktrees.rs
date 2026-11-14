use super::*;

#[test]
fn mcp_server_keeps_its_linked_worktree_and_resolves_registered_sibling_names() {
    let primary = Workspace::with_files(&[
        (
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        ),
        (
            ".forge/acdd/profile.yaml",
            "version: 2\ninherit: none\nroles:\n  worker: {mode: subagent}\ngates:\n  - tasks:\n      gates:\n        - id: contract\n          role: worker\n          proof: contract\n        - id: build\n          role: worker\n          proof: none\n        - id: review\n          role: worker\n          proof: none\n          independent_from: build\n        - id: deliver\n          role: worker\n          receipt: true\n          need: []\n          save: snapshot\n",
        ),
        ("src/lib.rs", "pub fn primary_only() {}\n"),
    ]);
    primary.seed_git().expect("seed real Git baseline");
    let linked = primary.path(".worktrees/049-current");
    let sibling_container = Workspace::with_files(&[]);
    let sibling = sibling_container.path("051-external");
    let mirror = sibling_container.path("052-mirror");
    for worktree in [&linked, &sibling, &mirror] {
        let path = worktree
            .to_str()
            .expect("Git worktree path should be valid UTF-8");
        primary
            .run_git(&["worktree", "add", "--detach", path])
            .expect("create real Git worktree");
    }
    fs::write(
        linked.join("src/lib.rs"),
        "pub fn linked_only() {}\npub fn shared_source() {\n    linked_only();\n    let b = 2;\n    let c = 3;\n}\n",
    )
    .unwrap();
    fs::write(
        sibling.join("src/lib.rs"),
        "pub fn sibling_only() {}\npub fn shared_source() {\n    sibling_only();\n    let y = 2;\n    let z = 3;\n}\n#[cfg(test)]\nmod tests {\n    use super::sibling_only;\n    #[test]\n    fn exercises_sibling_symbol() {\n        sibling_only();\n    }\n}\n",
    )
    .unwrap();
    fs::create_dir_all(sibling.join("src/space dir")).unwrap();
    fs::write(
        sibling.join("src/space dir/#unicode-λ.rs"),
        "pub fn escaped_path_symbol() {}\n",
    )
    .unwrap();
    fs::create_dir_all(linked.join("docs")).unwrap();
    fs::write(
        linked.join("docs/linked symbols.md"),
        "# Linked symbol reference\n\nThe `linked_only` function is declared in the source.\n",
    )
    .unwrap();
    fs::write(
        mirror.join("src/lib.rs"),
        "pub fn linked_only() {}\npub fn shared_source() {\n let a = 1;\n let b = 2;\n let c = 3;\n}\n",
    )
    .unwrap();
    let claims = TaskClaimsFixture::worktrees_only(
        &primary,
        ".forge/tasks.sqlite",
        &[&linked, &sibling, &mirror],
    );
    let custom_db = linked.join(".forge/custom.sqlite");
    let mut client =
        McpTestHarness::with_root_and_db(&linked, &custom_db, "linked-routing-session")
            .expect("MCP initialize");
    for worktree in [None, Some(".")] {
        let mut args = json!({"pattern":"linked_only", "exact":true});
        if let Some(worktree) = worktree {
            args["worktree"] = json!(worktree);
        }
        assert_eq!(
            client
                .payload("code_map_search", args)
                .expect("MCP tool should succeed")["nodes"]["total"],
            1
        );
    }
    let branch_manifest = sibling.join("docs/milestones/060-branch-only.md");
    fs::create_dir_all(branch_manifest.parent().unwrap()).unwrap();
    fs::write(&branch_manifest, "---\nid: m-branch-only\ntitle: Branch only\ndoc_type: contract\nstatus: active\n---\n\n### task: branch-task\n\n```yaml\ntask_ref: branch-task\ntarget: branch task\nproof_policy: direct-proof\nscope: [src/lib.rs]\n```\n").unwrap();
    for alias in ["051", "51", "051-ex", "external", "1-exter"] {
        assert_eq!(
            client
                .search("sibling_only", json!({ "exact":true, "worktree":alias}))
                .expect("MCP tool should succeed")["nodes"]["total"],
            1
        );
    }
    drop(claims);
    primary.remove(".forge/tasks.sqlite");
    fs::write(
        sibling.join("AGENTS.md"),
        "Load the contextunity-forge skill.\n",
    )
    .unwrap();
    let sync = client
        .payload(
            "task_manage",
            json!({"action":"sync", "milestone_ref":"060", "worktree":"51"}),
        )
        .expect("MCP tool should succeed");
    assert!(sync.to_string().contains("branch-task"), "{sync}");
    let list = client
        .payload(
            "task_list",
            json!({"milestone_ref":"060", "status":"all", "worktree":"51"}),
        )
        .expect("MCP tool should succeed");
    assert!(list.to_string().contains("branch-task"), "{list}");
    let posted = client.payload(
        "task_blackboard",
        json!({"action":"post", "scope":"milestone", "milestone_ref":"060", "topic":"notes", "payload":"branch-only message", "worktree":"51"}),
    ).expect("MCP tool should succeed");
    let board = client
        .payload(
            "task_blackboard",
            json!({"action":"read", "scope":"milestone", "milestone_ref":"060", "worktree":"51"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(board["messages"][0]["id"], posted["id"], "{board}");
    let message = client
        .payload(
            "task_blackboard",
            json!({"action":"inspect", "message_id":posted["id"], "worktree":"51"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        message["message"]["payload"], "branch-only message",
        "{message}"
    );
    let task_id = "forge-mcp/m-branch-only:branch-task";
    let claim = client.payload("task_claim", json!({"task_id":task_id, "stage":"contract", "worker_id":"routing-author", "worktree":"51", "bundle":false})).expect("MCP tool should succeed");
    let canonical_sibling = sibling.canonicalize().unwrap();
    assert_eq!(
        claim["worktree"],
        canonical_sibling.to_str().unwrap(),
        "{claim}"
    );
    let session = crate::sqlite_fixture::claim_session_fingerprint(
        &primary,
        task_id,
        claim["claim_revision"].as_u64().unwrap(),
    )
    .unwrap();
    assert_eq!(session, "linked-routing-session");
    client
        .payload(
            "task_manage",
            json!({"action":"reset", "task_id":task_id, "worktree":"51"}),
        )
        .expect("MCP tool should succeed");
    let next = client
        .payload(
            "task_manage",
            json!({"action":"next", "worker_id":"routing-author", "worktree":"051"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        next["worktree"],
        canonical_sibling.to_str().unwrap(),
        "{next}"
    );
    let mut evidence = next["workflow_guidance"]["expected_evidence_template"].clone();
    evidence["proof"] =
        json!({"contract_proof":{"seam_test_ref":"tests/routing.rs", "red_exit_code":101}});
    let rejected = client.payload("task_submit", json!({"task_id":task_id, "stage":"contract", "action":"reject", "worktree":"51", "evidence":evidence, "findings":{"summary":"Release the fixture claim after proving selected-root submission"}})).expect("MCP tool should succeed");
    assert_eq!(rejected["stage"], "contract", "{rejected}");
    let linked_checkpoint = "forge-mcp/forge-mcp/m-branch-only/tasks/linked-routing/contract";
    let sibling_checkpoint = "forge-mcp/forge-mcp/m-branch-only/tasks/sibling-routing/contract";
    for (selector, value, name) in [
        (".", "linked", linked_checkpoint),
        ("51", "sibling", sibling_checkpoint),
    ] {
        client
            .payload(
                "session_checkpoint",
                json!({"action":"save", "name":name, "content":value, "worktree":selector}),
            )
            .expect("MCP tool should succeed");
    }
    for (selector, value, name) in [
        (None, "linked", linked_checkpoint),
        (Some("51"), "sibling", sibling_checkpoint),
    ] {
        let mut args = json!({"action":"get", "name":name});
        if let Some(selector) = selector {
            args["worktree"] = json!(selector);
        }
        let checkpoint = client
            .payload("session_checkpoint", args)
            .expect("MCP tool should succeed");
        assert_eq!(checkpoint, json!(value));
    }

    assert!(custom_db.is_file());
    assert!(!linked.join(".forge/code-map.sqlite").exists());
    let sibling_result = client
        .search(
            "sibling_only",
            json!({ "exact":true, "worktree":"external"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(sibling_result["nodes"]["total"], 1);
    assert!(sibling_result["nodes"]["items"][0].get("uri").is_none());
    let linked_page = client
        .source(
            "shared_source",
            json!({"max_body_lines":1,"leading_lines":0,"worktree":"."}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(linked_page["source_preview"]["next_source_offset"], 1);
    let (_, mirror_continuation) = client.call(
        "forge_read",
        json!({"selector":"shared_source","max_body_lines":1,"leading_lines":0,"source_offset":1,"worktree":"mirror","format":"json"}),
    ).expect("MCP transport");
    assert_eq!(mirror_continuation["result"]["isError"], true);
    assert!(mirror_continuation["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("generation"));
    let (_, sibling_continuation) = client.call(
        "forge_read",
        json!({"selector":"shared_source","max_body_lines":1,"leading_lines":0,"source_offset":1,"worktree":"external","format":"json"}),
    ).expect("MCP transport");
    assert_eq!(sibling_continuation["result"]["isError"], true);

    let expected_sibling_source =
        crate::common::workspace::canonical_file_uri(&sibling, "src/lib.rs", Some(1));
    let expected_special_source = crate::common::workspace::canonical_file_uri(
        &sibling,
        "src/space dir/#unicode-λ.rs",
        Some(1),
    );
    let expected_linked_source =
        crate::common::workspace::canonical_file_uri(&linked, "src/lib.rs", Some(1));
    let uri_cases = [
        (
            "relative root symbol",
            "code_map_search",
            json!({"pattern":"linked_only","exact":true,"worktree":".","format":"markdown"}),
            expected_linked_source,
        ),
        (
            "registered linked-workspace symbol",
            "code_map_search",
            json!({"pattern":"sibling_only","exact":true,"worktree":"external","format":"markdown"}),
            expected_sibling_source.clone(),
        ),
        (
            "encoded physical path",
            "code_map_search",
            json!({"pattern":"escaped_path_symbol","exact":true,"worktree":"external","format":"markdown"}),
            expected_special_source.clone(),
        ),
        (
            "AST match",
            "ast_grep_search",
            json!({"language":"rust","pattern":"pub fn escaped_path_symbol() {}","worktree":"external","format":"markdown"}),
            expected_special_source,
        ),
        (
            "inspect symbol",
            "code_map_inspect",
            json!({"selector":"shared_source","worktree":"external","format":"markdown"}),
            crate::common::workspace::canonical_file_uri(&sibling, "src/lib.rs", Some(2)),
        ),
        (
            "impact traversal node",
            "code_map_impact",
            json!({"selector":"sibling_only","direction":"inbound","worktree":"external","format":"markdown"}),
            crate::common::workspace::canonical_file_uri(&sibling, "src/lib.rs", Some(2)),
        ),
        (
            "traversal call edge",
            "code_map_explain",
            json!({"selector":"shared_source","direction":"outbound","worktree":"external","format":"markdown"}),
            crate::common::workspace::canonical_file_uri(&sibling, "src/lib.rs", Some(3)),
        ),
        (
            "test edge",
            "code_map_tests",
            json!({"selector":"sibling_only","direction":"inbound","worktree":"external","format":"markdown"}),
            crate::common::workspace::canonical_file_uri(&sibling, "src/lib.rs", Some(11)),
        ),
        (
            "documentation section",
            "search_docs",
            json!({"query":"linked symbol reference","format":"markdown"}),
            crate::common::workspace::canonical_file_uri(&linked, "docs/linked symbols.md", None),
        ),
    ];
    let mut uri_failures = Vec::new();
    for (label, tool, arguments, expected_uri) in uri_cases {
        let (_, response) = client.call(tool, arguments).expect("MCP transport");
        assertions::assert_rpc_succeeded(&response);
        let markdown = response["result"]["content"][0]["text"]
            .as_str()
            .expect("Markdown tool call should return text");
        if !markdown.contains(&expected_uri) {
            uri_failures.push(format!(
                "{label} did not link to {expected_uri}: {markdown}"
            ));
        }
        if markdown.contains("(link unavailable)") {
            uri_failures.push(format!(
                "{label} retained a link-unavailable marker: {markdown}"
            ));
        }
    }

    let doc_path = "docs/linked symbols.md";
    let doc_json = client
        .payload("get_doc", json!({"path_or_id":doc_path}))
        .expect("MCP tool should succeed");
    let referenced_symbol = doc_json["referenced_symbols"]["items"]
        .as_array()
        .and_then(|items| items.first());
    if let Some(referenced_symbol) = referenced_symbol {
        assert_eq!(referenced_symbol["path"], "src/lib.rs");
        assert_eq!(referenced_symbol["line"], 1);
        assert_eq!(referenced_symbol["citation_line"], 3);
        assert!(referenced_symbol["doc_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
        assert!(referenced_symbol.get("uri").is_none());
    } else {
        uri_failures.push(format!(
            "get_doc did not retain the referenced symbol row: {doc_json}"
        ));
    }
    let doc_line = doc_json["sections"]["items"][0]["line"]
        .as_u64()
        .unwrap_or(1);
    let expected_doc_uri = crate::common::workspace::canonical_file_uri(
        &linked,
        "docs/linked symbols.md",
        Some(doc_line.try_into().unwrap()),
    );
    let (_, doc_markdown_response) = client
        .call(
            "get_doc",
            json!({"path_or_id":doc_path,"format":"markdown"}),
        )
        .expect("MCP transport");
    assertions::assert_rpc_succeeded(&doc_markdown_response);
    let doc_markdown = doc_markdown_response["result"]["content"][0]["text"]
        .as_str()
        .expect("get_doc should return Markdown text");
    for expected_uri in [
        expected_doc_uri,
        crate::common::workspace::canonical_file_uri(&linked, "src/lib.rs", Some(1)),
    ] {
        if !doc_markdown.contains(&expected_uri) {
            uri_failures.push(format!(
                "get_doc did not link to {expected_uri}: {doc_markdown}"
            ));
        }
    }
    assert!(
        uri_failures.is_empty(),
        "coordinate URI failures: {uri_failures:#?}"
    );
    client.finish().expect("server must finish successfully");
}
