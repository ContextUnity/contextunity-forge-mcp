use super::*;

#[test]
fn linked_task_mcp_and_cli_resolve_repository_roots_and_guidance() {
    let main = Workspace::new();
    let linked = Workspace::new();
    fs::create_dir_all(linked.0.join("src")).unwrap();
    fs::create_dir_all(linked.0.join("docs/milestones")).unwrap();
    let manifest = "---\nid: m-linked\ntitle: Linked\ndoc_type: contract\ninvariants: [local-rules]\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    main.write("010-main.md", manifest);
    main.write("AGENTS.md", "# Main rules\n");
    linked.write("src/lib.rs", "pub fn library() {}\n");
    linked.write("AGENTS.md", "# Library rules\n");
    linked.write(
        "docs/milestones/010-linked.md",
        &manifest.replace('\n', "\r\n"),
    );
    main.write("forge-mcp.yaml", &format!("roots: []\ndoc_roots: []\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n", linked.0.display()));
    let mut client = Client::new(&main);
    client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"010-main.md"}),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            main.0.to_str().unwrap(),
            "task",
            "sync",
            "--workspace",
            "traverse",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let synced: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    assert_eq!(
        client.payload("task_list", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let all = client.payload("task_list", json!({"repository":"all"}));
    assert_eq!(all["tasks"].as_array().unwrap().len(), 2);
    let selected = client.payload("task_list", json!({"repository":"traverse"}));
    assert_eq!(selected["tasks"][0]["task_id"], id);
    let output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            main.0.to_str().unwrap(),
            "task",
            "list",
            "--repository",
            "all",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        all
    );
    let inspected = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
    assert_eq!(inspected["workspace_root"], linked.0.to_str().unwrap());
    assert_eq!(
        inspected["agents_guidance"],
        linked.0.join("AGENTS.md").to_str().unwrap()
    );
    let claimed = client.payload(
        "task_claim",
        json!({"task_id":id,"stage":"design","worker_id":"builder","worktree":linked.0}),
    );
    assert_eq!(claimed["agents_guidance"], inspected["agents_guidance"]);
    assert_eq!(claimed["applicable_invariants"], json!(["local-rules"]));
    let (_, traversal) = client.call(
        "task_manage",
        json!({"action":"extend_scope","task_id":id,"paths":["../foreign.rs"]}),
    );
    assert_eq!(traversal["result"]["isError"], true);
    let synced = client.payload("task_manage", json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/milestones/010-linked.md"}));
    assert_eq!(synced["tasks"][0]["status"], "in_progress");
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let build_proof = json!({"test_proof":{"command":"cargo test","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let contract_proof = json!({"contract_proof":{"seam_test_ref":"tests/mcp_context/tasks.rs","red_exit_code":101}});
    let contours: serde_json::Map<String, Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"linked public-seam verification"}),
                )
            })
            .collect();
    let review_proof = json!({"review_proof":{"decision":"pass","contours":contours}});
    for (gate, stage) in contextunity_forge_mcp::core::tasks::GATES
        .iter()
        .enumerate()
    {
        let worker = match gate { 3 => "reviewer", 4 => "delivery-reviewer", _ => "builder" };
        let claim = if gate == 0 {
            claimed.clone()
        } else {
            client.payload(
                "task_claim",
                json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":linked.0}),
            )
        };
        let proof = if gate == 3 {
            &review_proof
        } else if gate == 1 {
            &contract_proof
        } else {
            &build_proof
        };
        let evidence = json!({"task_id":id,"stage":stage,"claim_revision":claim["claim_revision"],"contract_revision":1,"worker_id":worker,"worktree":linked.0,"commit":commit,"proof":proof});
        let result = client.payload(
            "task_submit",
            json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"}),
        );
        assert_eq!(
            result["status"],
            if gate == 4 { "completed" } else { "ready" }
        );
    }
    drop(client);
    let mut client = Client::new(&main);
    let restored = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
    assert_eq!(restored["status"], "completed");
    assert_eq!(restored["agents_guidance"], inspected["agents_guidance"]);
    assert_eq!(
        client.payload("task_list", json!({"repository":"traverse"}))["tasks"],
        json!([])
    );
}

#[test]
fn task_stdio_lifecycle_submits_inline_evidence_in_independent_worktrees() {
    use contextunity_forge_mcp::core::tasks::{gates::REVIEW_CONTOURS, GATES};
    let builder = Workspace::new();
    let reviewer = Workspace::new();
    let source = "---\nid: m-handoff\ntitle: Handoff\ndoc_type: contract\n---\n```yaml\ntask_ref: delivery\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    for workspace in [&builder, &reviewer] {
        workspace.write("010-tasks.md", &source.replace('\n', "\r\n"));
    }
    builder.write(
        "forge-mcp.yaml",
        "roots: []\ndoc_roots: []\ntask_repository: contextunity\ntask_project: tooling\n",
    );
    let mut client = Client::new(&builder);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"010-tasks.md"}),
    );
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    assert_eq!(id, "contextunity/tooling/m-handoff:delivery");
    for action in ["preview", "apply", "verify"] {
        let output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                builder.0.to_str().unwrap(),
                "migrate",
                action,
                "010-tasks.md",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let build_proof = json!({"test_proof":{"command":"cargo test","exit_code":0,"tests_passed":1,"tests_failed":0}});
    let contract_proof = json!({"contract_proof":{"seam_test_ref":"tests/mcp_context/tasks.rs","red_exit_code":101}});
    let contours: serde_json::Map<String, Value> = REVIEW_CONTOURS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                json!({"applicable":true,"evidence":"independent fixture review"}),
            )
        })
        .collect();
    let review_proof = json!({"review_proof":{"decision":"pass","contours":contours}});
    for (gate, stage) in GATES.iter().enumerate() {
        let workspace = if gate == 3 { &reviewer } else { &builder };
        let worker = match gate { 3 => "reviewer", 4 => "delivery-reviewer", _ => "builder" };
        let claim = client.payload(
            "task_claim",
            json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":workspace.0}),
        );
        let proof = if gate == 3 {
            &review_proof
        } else if gate == 1 {
            &contract_proof
        } else {
            &build_proof
        };
        let evidence = json!({"task_id":id,"stage":stage,"claim_revision":claim["claim_revision"],"contract_revision":1,"worker_id":worker,"worktree":workspace.0,"commit":commit,"proof":proof});
        if gate == 0 {
            for invalid in [
                json!({"task_id":id,"stage":stage,"action":"pass","evidence_ref":"proof.yaml"}),
                json!({"task_id":id,"stage":stage,"action":"pass","evidence":42}),
            ] {
                let (_, rejected) = client.call("task_submit", invalid);
                assert!(
                    rejected.get("error").is_some() || rejected["result"]["isError"] == true
                );
            }
        }
        let args = json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"});
        if gate == 4 {
            builder.write("010-tasks.md", &source.replace("target: Deliver", "target: Unadmitted change").replace('\n', "\r\n"));
            let (_, unadmitted) = client.call("task_submit", args.clone());
            assert_eq!(unadmitted["result"]["isError"], true);
            builder.write("010-tasks.md", &source.replace('\n', "\r\n"));
        }
        let submitted = client.payload("task_submit", args.clone());
        assert_eq!(
            submitted["status"],
            if gate == 4 { "completed" } else { "ready" }
        );
        assert_eq!(client.payload("task_submit", args), submitted);
    }
    assert_eq!(client.payload("task_list", json!({}))["tasks"], json!([]));
    assert_eq!(
        client.payload("task_list", json!({"status":"completed"}))["tasks"][0]["task_id"],
        id
    );
}

#[test]
fn stdio_tool_catalog_uses_object_schemas_for_every_property() {
    let workspace = Workspace::new();
    let mut client = Client::new(&workspace);
    let (_, response) = client.request("tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 19);
    for tool in tools {
        for (name, schema) in tool["inputSchema"]["properties"].as_object().unwrap() {
            assert!(schema.is_object(), "{}.{name}: {schema}", tool["name"]);
        }
    }
    let names: Vec<_> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .filter(|n| n.starts_with("task_"))
        .collect();
    assert_eq!(names.len(), 4);
    for name in ["task_list", "task_claim", "task_submit", "task_manage"] {
        assert!(names.contains(&name));
        let schema = &tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"];
        assert_eq!(schema["additionalProperties"], false);
    }
    let content = &tools
        .iter()
        .find(|tool| tool["name"] == "session_checkpoint")
        .unwrap()["inputSchema"]["properties"]["content"];
    assert!(content.is_object());
    for value in [
        json!(true),
        json!(42),
        json!("text"),
        json!([1]),
        json!({"key":1}),
    ] {
        client.payload(
            "session_checkpoint",
            json!({"action":"save","name":"sample","content":value}),
        );
        assert_eq!(
            client.payload(
                "session_checkpoint",
                json!({"action":"get","name":"sample"})
            ),
            value
        );
    }
}

#[test]
fn task_mcp_and_cli_share_ready_claim_reset_and_selectors() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndoc_roots: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write("010-tasks.md","---\nid: m-pilot\ntitle: Pilot\ndoc_type: contract\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n");
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"010-tasks.md"}),
    );
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let ready = client.payload("task_list", json!({}));
    assert_eq!(ready["tasks"].as_array().unwrap().len(), 1);
    let cli = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", workspace.0.to_str().unwrap(), "task", "list"])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(serde_json::from_slice::<Value>(&cli.stdout).unwrap(), ready);
    let args = json!({"task_id":id,"stage":"design","worker_id":"builder","worktree":workspace.0});
    let claimed = client.payload("task_claim", args.clone());
    let (_, collision) = client.call("task_claim", args);
    assert_eq!(collision["result"]["isError"], true);
    let error: Value =
        serde_json::from_str(collision["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["error"]["code"], "TASK_ALREADY_CLAIMED");
    assert_eq!(client.payload("task_list", json!({}))["tasks"], json!([]));
    assert_eq!(
        client.payload("task_list", json!({"status":"in_progress"}))["tasks"][0]["task_id"],
        id
    );
    let reset = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", workspace.0.to_str().unwrap(), "task", "reset", id])
        .output()
        .unwrap();
    assert!(reset.status.success());
    let reset: Value = serde_json::from_slice(&reset.stdout).unwrap();
    assert!(
        reset["claim_revision"].as_u64().unwrap() > claimed["claim_revision"].as_u64().unwrap()
    );
    for args in [
        json!({"action":"inspect","task_id":id,"force":true}),
        json!({"action":"sync","milestone_ref":"010-tasks.md","paths":["src/new.rs"]}),
        json!({"unexpected":true}),
    ] {
        let name = if args.get("action").is_some() {
            "task_manage"
        } else {
            "task_list"
        };
        let (_, response) = client.call(name, args);
        assert!(response["result"]["isError"] == true || response.get("error").is_some());
    }
}
