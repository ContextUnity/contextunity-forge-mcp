use super::*;

#[test]
fn linked_task_mcp_and_cli_resolve_repository_roots_and_guidance() {
    let main = Workspace::new();
    let linked = Workspace::new();
    fs::create_dir_all(main.0.join("docs/milestones")).unwrap();
    fs::create_dir_all(linked.0.join("src")).unwrap();
    fs::create_dir_all(linked.0.join("docs/milestones")).unwrap();
    let manifest = "---\nid: m-linked\ntitle: Linked\ndoc_type: contract\nstatus: active\ninvariants: [local-rules]\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    main.write("docs/milestones/010-main.md", manifest);
    main.write("AGENTS.md", "# Main rules\n");
    linked.write("src/lib.rs", "pub fn library() {}\n");
    linked.write("AGENTS.md", "# Library rules\n");
    linked.write(
        "docs/milestones/010-linked.md",
        &manifest.replace('\n', "\r\n"),
    );
    main.write("forge-mcp.yaml", &format!("roots: []\ndocs: []\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n", linked.0.display()));
    let mut client = Client::new(&main);
    client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-main.md"}),
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
    assert_eq!(
        all["workspaces"]["traverse/traverse"]["workspace_root"],
        linked.0.to_str().unwrap()
    );
    let selected = client.payload("task_list", json!({"repository":"traverse"}));
    assert_eq!(selected["workspace_root"], linked.0.to_str().unwrap());
    assert_eq!(selected["workspace"], "traverse");
    assert_eq!(
        selected["agents_guidance"],
        linked.0.join("AGENTS.md").to_str().unwrap()
    );
    assert_eq!(
        selected["tasks"][0],
        json!({
            "task_id":id,"target":"Deliver","status":"ready","stage":"contract/v1",
            "owner":null,"agent_type":"worker","rev":1
        })
    );
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
        json!({"task_id":id,"stage":"contract","worker_id":"builder","worktree":linked.0}),
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
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let claim = if gate == 0 {
            claimed.clone()
        } else {
            client.payload(
                "task_claim",
                json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":linked.0}),
            )
        };
        let proof = if gate == 2 {
            &review_proof
        } else if gate == 0 {
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
            if gate == 3 { "completed" } else { "ready" }
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
    let source = "---\nid: m-handoff\ntitle: Handoff\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: delivery\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    for workspace in [&builder, &reviewer] {
        fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
        workspace.write(
            "docs/milestones/010-tasks.md",
            &source.replace('\n', "\r\n"),
        );
    }
    builder.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntask_repository: contextunity\ntask_project: tooling\n",
    );
    let mut client = Client::new(&builder);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md"}),
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
                "docs/milestones/010-tasks.md",
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
        let workspace = if gate == 2 { &reviewer } else { &builder };
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let claim = client.payload(
            "task_claim",
            json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":workspace.0}),
        );
        let proof = if gate == 2 {
            &review_proof
        } else if gate == 0 {
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
                assert!(rejected.get("error").is_some() || rejected["result"]["isError"] == true);
            }
        }
        let args = json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"});
        if gate == 3 {
            builder.write(
                "docs/milestones/010-tasks.md",
                &source
                    .replace("target: Deliver", "target: Unadmitted change")
                    .replace('\n', "\r\n"),
            );
            let (_, unadmitted) = client.call("task_submit", args.clone());
            assert_eq!(unadmitted["result"]["isError"], true);
            builder.write(
                "docs/milestones/010-tasks.md",
                &source.replace('\n', "\r\n"),
            );
        }
        let submitted = client.payload("task_submit", args.clone());
        assert_eq!(
            submitted["status"],
            if gate == 3 { "completed" } else { "ready" }
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
    assert_eq!(tools.len(), 20);
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
    assert_eq!(names.len(), 5);
    for name in [
        "task_list",
        "task_claim",
        "task_submit",
        "task_manage",
        "task_blackboard",
    ] {
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
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write("docs/milestones/010-tasks.md","---\nid: m-pilot\ntitle: Pilot\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Deliver\nproof_policy: seam-test-first\nscope: [src/]\n```\n");
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md"}),
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
    let args =
        json!({"task_id":id,"stage":"contract","worker_id":"builder","worktree":workspace.0});
    let claimed = client.payload("task_claim", args.clone());
    let (_, collision) = client.call("task_claim", args);
    assert_eq!(collision["result"]["isError"], true);
    let error: Value =
        serde_json::from_str(collision["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["error"]["code"], "TASK_ALREADY_CLAIMED");
    assert_eq!(client.payload("task_list", json!({}))["tasks"], json!([]));
    let active = client.payload("task_list", json!({"status":"in_progress"}));
    assert_eq!(active["tasks"][0]["task_id"], id);
    assert_eq!(active["tasks"][0]["owner"], "builder");
    assert_eq!(active["tasks"][0]["stage"], "contract/v1");
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
        json!({"action":"sync","milestone_ref":"docs/milestones/010-tasks.md","paths":["src/new.rs"]}),
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

#[test]
fn task_blackboard_mcp_and_cli_share_sqlite_messages() {
    use contextunity_forge_mcp::db::tasks_store::TasksStore;

    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/010-blackboard.md",
        "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\n---\n```yaml\ntask_ref: first\ntarget: Coordinate agents\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/010-blackboard.md"}),
    );
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();

    let (_, mcp_post) = client.call(
        "task_blackboard",
        json!({"action":"post","task_id":task_id,"topic":"contract_draft","payload":"{\"red\":true}"}),
    );
    let cli_post = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "post",
            task_id,
            "--topic",
            "build_proof",
            "--payload",
            "cargo test --all-targets passed",
        ])
        .output()
        .unwrap();
    assert!(
        mcp_post.get("error").is_none()
            && mcp_post["result"]["isError"] != true
            && cli_post.status.success(),
        "MCP: {mcp_post}; CLI: {}",
        String::from_utf8_lossy(&cli_post.stderr)
    );
    let mcp_post: Value =
        serde_json::from_str(mcp_post["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let cli_post: Value = serde_json::from_slice(&cli_post.stdout).unwrap();
    let mcp_read = client.payload(
        "task_blackboard",
        json!({"action":"read","task_id":task_id,"limit":2}),
    );
    assert_eq!(mcp_read["messages"].as_array().unwrap().len(), 2);
    assert_eq!(mcp_read["messages"][0]["id"], mcp_post["id"]);
    assert_eq!(mcp_read["messages"][1]["id"], cli_post["id"]);

    let cli_read = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "read",
            task_id,
            "--topic",
            "contract_draft",
        ])
        .output()
        .unwrap();
    assert!(
        cli_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_read.stderr)
    );
    let cli_read: Value = serde_json::from_slice(&cli_read.stdout).unwrap();
    assert_eq!(cli_read["messages"].as_array().unwrap().len(), 1);
    assert_eq!(cli_read["messages"][0]["id"], mcp_post["id"]);

    let store = TasksStore::open(&workspace.0.join("tasks.sqlite")).unwrap();
    let stored = store.blackboard_read(task_id, None, None).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].payload, "{\"red\":true}");
    assert_eq!(stored[1].payload, "cargo test --all-targets passed");
    assert_eq!(stored[0].author, "mcp");
    assert_eq!(stored[1].author, "cli");
}

#[test]
fn subtask_mcp_and_cli_operations() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    let manifest = "---\nid: m-mcp-subtasks\ntitle: MCP Subtasks\ndoc_type: contract\ninvariants: [local-rules]\n---\n```yaml\ntask_ref: sub-demo\ntarget: Subtask demonstration\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    workspace.write("docs/milestones/010-subtasks.md", manifest);
    workspace.write("AGENTS.md", "# Agent Rules\n");
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    let mut client = Client::new(&workspace);
    client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-subtasks.md"}),
    );
    let task_id = "forge-mcp/forge-mcp/m-mcp-subtasks:sub-demo";

    // MCP task_manage subtask_add
    let add_res = client.payload(
        "task_manage",
        json!({
            "action": "subtask_add",
            "task_id": task_id,
            "subtask_ref": "step-1",
            "title": "Initial exploration"
        }),
    );
    assert_eq!(add_res["subtask"]["subtask_ref"], "step-1");
    assert_eq!(add_res["subtask"]["status"], "pending");

    // MCP task_manage subtask_update
    let update_res = client.payload(
        "task_manage",
        json!({
            "action": "subtask_update",
            "task_id": task_id,
            "subtask_ref": "step-1",
            "subtask_status": "completed",
            "evidence": "Explored symbols"
        }),
    );
    assert_eq!(update_res["subtask"]["status"], "completed");
    assert_eq!(update_res["subtask"]["evidence"], "Explored symbols");

    // CLI task subtask add
    let cli_add = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "subtask",
            "add",
            task_id,
            "step-2",
            "Run unit test",
        ])
        .output()
        .unwrap();
    assert!(
        cli_add.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_add.stderr)
    );
    let cli_add_json: Value = serde_json::from_slice(&cli_add.stdout).unwrap();
    assert_eq!(cli_add_json["subtask"]["subtask_ref"], "step-2");

    // CLI task subtask list
    let cli_list = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "subtask",
            "list",
            task_id,
        ])
        .output()
        .unwrap();
    assert!(
        cli_list.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_list.stderr)
    );
    let cli_list_json: Value = serde_json::from_slice(&cli_list.stdout).unwrap();
    assert_eq!(cli_list_json["subtasks"].as_array().unwrap().len(), 2);

    // MCP task_manage inspect returns subtasks
    let inspected = client.payload(
        "task_manage",
        json!({"action": "inspect", "task_id": task_id}),
    );
    assert_eq!(inspected["subtasks"].as_array().unwrap().len(), 2);
}

#[test]
fn task_claim_bundle_and_task_manage_context_returns_unified_agent_context() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("src")).unwrap();
    fs::create_dir_all(workspace.0.join("tests")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/adr")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();

    workspace.write("AGENTS.md", "# Test guidance\n");
    workspace.write(
        "docs/adr/001-task-routing.md",
        "---\ntitle: Task Routing Invariants\nstatus: accepted\n---\n# Task Routing Invariants\nContext and rules for routing.",
    );
    workspace.write(
        "docs/adr/002-unrelated.md",
        "---\ntitle: Storage Layout\nstatus: accepted\n---\n# Storage Layout\nSQLite pages and retention.",
    );
    workspace.write(
        "src/routing.rs",
        "pub struct Router;\npub fn route_task() {}\n",
    );
    workspace.write("tests/test_routing.rs", "#[test]\nfn test_routing() {}\n");

    let manifest = "---\nid: m-context\ntitle: Context Milestone\ndoc_type: contract\ninvariants: [routing-invariant]\n---\n```yaml\ntask_ref: routing-task\ntarget: Deliver unified routing\nproof_policy: seam-test-first\nscope:\n  - src/routing.rs\n  - tests/test_routing.rs\n```\n";
    workspace.write("docs/milestones/010-context.md", manifest);
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    contextunity_forge_mcp::db::writer::build(
        &workspace.0,
        &workspace.0.join(".forge/code-map.sqlite"),
        None,
    )
    .unwrap();

    let mut client = Client::new(&workspace);
    let sync_res = client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-context.md"}),
    );
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    // Post blackboard message
    client.payload(
        "task_blackboard",
        json!({
            "action": "post",
            "task_id": task_id,
            "topic": "architectural_notes",
            "payload": "Route tasks through unified zero-shot context bundle",
            "author": "architect"
        }),
    );

    // 1. MCP task_manage action: "context"
    let context_res = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
    let bundle = &context_res["context_bundle"];
    assert!(bundle.is_object(), "bundle must be present in response");

    // Check contract in bundle
    assert_eq!(bundle["contract"]["task_id"], task_id);
    assert_eq!(bundle["contract"]["target"], "Deliver unified routing");
    assert_eq!(bundle["contract"]["stage"], "contract/v1");
    assert_eq!(bundle["contract"]["status"], "ready");
    assert_eq!(
        bundle["contract"]["allowed_scope"],
        json!(["src/routing.rs", "tests/test_routing.rs"])
    );
    assert_eq!(
        bundle["contract"]["invariants"],
        json!(["routing-invariant"])
    );

    // Check guidance in bundle
    assert_eq!(bundle["guidance"]["stage"], "contract/v1");
    assert_eq!(bundle["guidance"]["subagent_role"], "contract_author");
    assert!(bundle["guidance"]["recommended_tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty()));
    assert!(bundle["guidance"]["actionable_steps"]
        .as_array()
        .is_some_and(|steps| !steps.is_empty()));
    assert_eq!(
        bundle["guidance"]["subtask_dod"].as_array().unwrap().len(),
        5
    );

    // Check scope-to-ADR mapping
    let adrs = bundle["adrs"].as_array().unwrap();
    assert!(!adrs.is_empty(), "expected at least 1 mapped ADR");
    let adr = &adrs[0];
    assert_eq!(adr["path"], "docs/adr/001-task-routing.md");
    assert_eq!(adr["title"], "Task Routing Invariants");
    assert_eq!(adr["status"], "accepted");
    assert_eq!(adr["relevance"], "direct");
    assert_eq!(
        adrs.len(),
        1,
        "unrelated ADRs must not appear as governing context"
    );

    let symbols = bundle["scope_symbols"].as_array().unwrap();
    assert!(symbols.iter().any(|symbol| symbol["name"] == "route_task"));

    // Check covering tests
    let tests = bundle["covering_tests"].as_array().unwrap();
    assert!(
        tests
            .iter()
            .any(|t| t["path"].as_str() == Some("tests/test_routing.rs")),
        "covering_tests must include tests/test_routing.rs"
    );

    // Check blackboard
    let bb = bundle["blackboard"].as_array().unwrap();
    assert_eq!(bb.len(), 1);
    assert_eq!(bb[0]["topic"], "architectural_notes");
    assert_eq!(bb[0]["author"], "architect");
    assert_eq!(
        bb[0]["payload"],
        "Route tasks through unified zero-shot context bundle"
    );

    // 2. MCP task_claim with bundle: true
    let claim_res = client.payload(
        "task_claim",
        json!({
            "task_id": task_id,
            "stage": "contract",
            "worker_id": "author-1",
            "worktree": workspace.0,
            "bundle": true
        }),
    );
    assert!(claim_res["context_bundle"].is_object());
    assert_eq!(
        claim_res["context_bundle"]["contract"]["worker_id"],
        "author-1"
    );
    assert_eq!(
        claim_res["context_bundle"]["adrs"][0]["relevance"],
        "direct"
    );
    assert_eq!(
        claim_res["context_bundle"]["blackboard"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // 3. CLI task context command
    let cli_context = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "context",
            task_id,
        ])
        .output()
        .unwrap();
    assert!(
        cli_context.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_context.stderr)
    );
    let cli_json: Value = serde_json::from_slice(&cli_context.stdout).unwrap();
    assert!(cli_json["context_bundle"].is_object());
    assert_eq!(cli_json["context_bundle"]["contract"]["task_id"], task_id);
}

#[test]
fn task_list_status_filter_is_typed_and_cli_flags_are_available() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    fs::create_dir_all(workspace.0.join("src")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones/archive")).unwrap();
    workspace.write("src/lib.rs", "pub fn task_list_fixture() {}\n");
    workspace.write(
        "docs/milestones/010-active.md",
        "---\nid: m-task-list\ntitle: Task list\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: visible\ntarget: Visible task\nproof_policy: direct-proof\nscope: [src/]\nsubtasks:\n  - subtask_ref: explain\n    title: Detailed subtask\n    status: completed\n    evidence: Verified through the listing seam\n```\n",
    );
    workspace.write(
        "docs/milestones/011-planned.md",
        "---\nid: m-planned-list\ntitle: Planned task list\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: planned\ntarget: Planned task\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    workspace.write(
        "docs/milestones/archive/009-completed.md",
        "---\nid: m-completed-list\ntitle: Completed task list\ndoc_type: contract\nstatus: completed\n---\n# Tasks\n```yaml\ntask_ref: completed\ntarget: Completed task\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    let mut client = Client::new(&workspace);
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"010-active"}),
    );
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let planned = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"011-planned"}),
    );
    let planned_task_id = planned["tasks"][0]["task_id"].as_str().unwrap();
    let completed = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"009-completed"}),
    );
    let completed_task_id = completed["tasks"][0]["task_id"].as_str().unwrap();

    let (_, catalog) = client.request("tools/list", json!({}));
    let task_list = catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "task_list")
        .unwrap();
    let status_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["milestone_status"]).unwrap();
    for status in ["active", "planned", "completed", "all"] {
        assert!(status_schema.contains(status));
    }
    assert!(!status_schema.contains("cancelled"));
    assert!(task_list["inputSchema"]["properties"]
        .get("include_archived")
        .is_none());
    let detail_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["detail"]).unwrap();
    assert!(detail_schema.contains("compact"));
    assert!(detail_schema.contains("full"));

    let (_, invalid_status) = client.call("task_list", json!({"milestone_status":"cancelled"}));
    assert!(invalid_status.get("error").is_some() || invalid_status["result"]["isError"] == true);
    assert_eq!(
        client.payload("task_list", json!({"milestone_status":"active"}))["tasks"][0]["task_id"],
        task_id
    );
    assert_eq!(
        client.payload("task_list", json!({"milestone_status":"planned"}))["tasks"][0]["task_id"],
        planned_task_id
    );
    assert_eq!(
        client.payload("task_list", json!({"milestone_status":"completed"}))["tasks"][0]["task_id"],
        completed_task_id
    );
    let mut mcp_all: Vec<_> = client.payload("task_list", json!({"milestone_status":"all"}))
        ["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap().to_owned())
        .collect();
    mcp_all.sort();
    let mut expected_all = vec![
        task_id.to_owned(),
        planned_task_id.to_owned(),
        completed_task_id.to_owned(),
    ];
    expected_all.sort();
    assert_eq!(mcp_all, expected_all);

    let cli_filter = |flags: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", workspace.0.to_str().unwrap(), "task", "list"])
            .args(flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        payload["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["task_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(cli_filter(&["--milestone-status", "active"]), [task_id]);
    assert_eq!(
        cli_filter(&["--milestone-status", "planned"]),
        [planned_task_id]
    );
    assert_eq!(
        cli_filter(&["--milestone-status", "completed"]),
        [completed_task_id]
    );
    let mut cli_all = cli_filter(&["--milestone-status", "all"]);
    cli_all.sort();
    assert_eq!(cli_all, expected_all);
    assert_eq!(cli_filter(&["--planned"]), [planned_task_id]);
    assert_eq!(cli_filter(&["--completed"]), [completed_task_id]);
    let mut cli_shorthand_all = cli_filter(&["--all"]);
    cli_shorthand_all.sort();
    assert_eq!(cli_shorthand_all, expected_all);

    let cli_help = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--help",
        ])
        .output()
        .unwrap();
    assert!(cli_help.status.success());
    let cli_help = String::from_utf8(cli_help.stdout).unwrap();
    assert!(cli_help.contains("--all"));
    assert!(cli_help.contains("cancelled tasks pending sync pruning"));

    assert_eq!(cli_filter(&["--full"]), [task_id]);

    let compact = client.payload("task_list", json!({}));
    assert_eq!(compact["tasks"].as_array().unwrap().len(), 1);
    let compact_subtask = &compact["tasks"][0]["subtasks"][0];
    assert_eq!(compact_subtask["subtask_ref"], "explain");
    assert_eq!(compact_subtask["status"], "completed");
    assert!(compact_subtask.get("title").is_none());
    assert!(compact_subtask.get("evidence").is_none());

    let full = client.payload("task_list", json!({"detail":"full"}));
    assert_eq!(full["tasks"][0]["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        full["tasks"][0]["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let subtask_details = client.payload(
        "task_manage",
        json!({"action":"subtask_list","task_id":task_id}),
    );
    assert_eq!(subtask_details["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        subtask_details["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let inspected = client.payload("task_manage", json!({"action":"inspect","task_id":task_id}));
    assert_eq!(inspected["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        inspected["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );

    let cli_full = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "list",
            "--milestone-status",
            "active",
            "--full",
        ])
        .output()
        .unwrap();
    assert!(
        cli_full.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_full.stderr)
    );
    let cli_full_json: Value = serde_json::from_slice(&cli_full.stdout).unwrap();
    assert_eq!(
        cli_full_json["tasks"][0]["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );

    for conflicting_flags in [
        vec!["--planned", "--completed"],
        vec!["--planned", "--all"],
        vec!["--completed", "--all"],
        vec!["--milestone-status", "active", "--planned"],
        vec!["--milestone-status", "all", "--all"],
    ] {
        let conflict = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", workspace.0.to_str().unwrap(), "task", "list"])
            .args(conflicting_flags)
            .output()
            .unwrap();
        assert!(!conflict.status.success());
    }
}
