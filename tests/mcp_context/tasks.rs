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
        assert_eq!(result["snapshot"]["commit"], &commit[..7]);
        assert_eq!(
            result["snapshot"]["inspect_cmd"],
            format!("git show {}", &commit[..7])
        );
        if gate == 3 {
            assert_eq!(result["receipt"]["commit"], &commit[..7]);
        }
        assert_eq!(
            result["status"],
            if gate == 3 { "completed" } else { "ready" }
        );
    }
    drop(client);
    let mut client = Client::new(&main);
    let restored = client.payload("task_manage", json!({"action":"inspect","task_id":id}));
    assert_eq!(restored["status"], "completed");
    assert_eq!(restored["receipt"]["commit"], &commit[..7]);
    assert_eq!(restored["latest_snapshot"]["commit"], &commit[..7]);
    assert_eq!(
        restored["latest_snapshot"]["inspect_cmd"],
        format!("git show {}", &commit[..7])
    );
    for gate in restored["gates"].as_array().unwrap() {
        assert_eq!(gate["commit"], &commit[..7]);
        let evidence: Value = serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        assert_eq!(evidence["commit"], &commit[..7]);
    }
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
    let source = r#"---
id: m-handoff
title: Handoff
doc_type: contract
status: active
deferred_defects:
  - id: DEFECT-E2E-001
    source: review_findings
    title: Typed defect survives the MCP task lifecycle
    path: src/engine/tasks.rs
    disposition: subsequent_milestone
    notes: Preserved through receipt rewriting
---
```yaml
task_ref: delivery
target: Deliver
proof_policy: seam-test-first
scope: [src/]
```
"#;
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
        if gate == 0 {
            let bundle = &claim["context_bundle"];
            assert!(
                bundle.is_object(),
                "omitted MCP bundle option returns context"
            );
            assert_eq!(bundle["contract"]["task_id"], id);
            assert_eq!(bundle["contract"]["stage"], *stage);
            assert!(bundle["guidance"]["actionable_steps"]
                .as_array()
                .is_some_and(|steps| !steps.is_empty()));
        }
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
        assert_eq!(submitted["snapshot"]["commit"], &commit[..7]);
        assert_eq!(
            submitted["snapshot"]["inspect_cmd"],
            format!("git show {}", &commit[..7])
        );
        if gate == 3 {
            assert_eq!(submitted["receipt"]["commit"], &commit[..7]);
        }
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
    let written = fs::read_to_string(builder.0.join("docs/milestones/010-tasks.md")).unwrap();
    let milestone =
        contextunity_forge_mcp::core::tasks::Milestone::parse(&written, "contextunity").unwrap();
    assert_eq!(
        milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(commit)
    );
    assert_eq!(milestone.deferred_defects.len(), 1);
    let defect = &milestone.deferred_defects[0];
    assert_eq!(defect.id, "DEFECT-E2E-001");
    assert_eq!(defect.source, "review_findings");
    assert_eq!(defect.title, "Typed defect survives the MCP task lifecycle");
    assert_eq!(defect.path.as_deref(), Some("src/engine/tasks.rs"));
    assert_eq!(defect.disposition, "subsequent_milestone");
    assert_eq!(
        defect.notes.as_deref(),
        Some("Preserved through receipt rewriting")
    );
    assert_eq!(milestone.tasks[0].status.as_deref(), Some("completed"));
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
    assert_eq!(mcp_read["messages"][0]["id"], cli_post["id"]);
    assert_eq!(mcp_read["messages"][1]["id"], mcp_post["id"]);

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
    assert_eq!(stored[0].payload.as_deref(), Some("{\"red\":true}"));
    assert_eq!(
        stored[1].payload.as_deref(),
        Some("cargo test --all-targets passed")
    );
    assert_eq!(stored[0].author, "mcp");
    assert_eq!(stored[1].author, "cli");
}

#[test]
fn milestone_blackboard_lists_are_bounded_payload_free_and_inspectable() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/010-blackboard.md",
        "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\nstatus: active\n---\n",
    );
    let mut client = Client::new(&workspace);

    let mut message_id = 0;
    for index in 0..12 {
        let (_, response) = client.call(
            "task_blackboard",
            json!({
                "action":"post",
                "topic":"architectural_notes",
                "payload":format!("milestone note {index}")
            }),
        );
        assert!(
            response.get("error").is_none() && response["result"]["isError"] != true,
            "{response}"
        );
        let posted: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        message_id = posted["id"].as_u64().unwrap();
    }

    let messages = client.payload("task_blackboard", json!({"action":"read"}));
    assert_eq!(messages["messages"].as_array().unwrap().len(), 10);
    assert_eq!(messages["pagination"]["limit"], 10);
    assert_eq!(messages["pagination"]["offset"], 0);
    assert_eq!(messages["pagination"]["has_more"], true);
    assert_eq!(messages["pagination"]["next_offset"], 10);
    assert_eq!(messages["messages"][0]["id"], message_id);
    assert_eq!(messages["messages"][0]["task_id"], Value::Null);
    assert!(messages["messages"][0].get("payload").is_none());

    let inspected = client.payload(
        "task_blackboard",
        json!({"action":"inspect","message_id":message_id}),
    );
    assert_eq!(inspected["message"]["id"], message_id);
    assert_eq!(inspected["message"]["payload"], "milestone note 11");

    let cli_inspect = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "blackboard",
            "inspect",
            &message_id.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        cli_inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_inspect.stderr)
    );
    let cli_inspect: Value = serde_json::from_slice(&cli_inspect.stdout).unwrap();
    assert_eq!(cli_inspect["message"]["id"], message_id);
    assert_eq!(cli_inspect["message"]["payload"], "milestone note 11");
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
    let routing_adr = "---\ntitle: Task Routing Invariants\nstatus: accepted\n---\n# Task Routing Invariants\nContext and rules for routing.";
    workspace.write(
        "docs/adr/001-task-routing.md",
        &routing_adr.replace('\n', "\r\n"),
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
    workspace.write("src/cli_routing.rs", "pub fn route_cli_task() {}\n");
    workspace.write(
        "tests/test_cli_routing.rs",
        "#[test]\nfn test_cli_routing() {}\n",
    );

    let manifest = "---\nid: m-context\ntitle: Context Milestone\ndoc_type: contract\ninvariants: [routing-invariant]\n---\n```yaml\ntask_ref: routing-task\ntarget: Deliver unified routing\nproof_policy: seam-test-first\nscope:\n  - src/routing.rs\n  - tests/test_routing.rs\n```\n```yaml\ntask_ref: cli-routing-task\ntarget: Deliver CLI unified routing\nproof_policy: seam-test-first\nscope:\n  - src/cli_routing.rs\n  - tests/test_cli_routing.rs\n```\n```yaml\ntask_ref: minimal-routing-task\ntarget: Deliver minimal routing\nproof_policy: seam-test-first\nscope:\n  - src/routing.rs\n```\n";
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
    let cli_task_id = sync_res["tasks"][1]["task_id"].as_str().unwrap();
    let minimal_task_id = sync_res["tasks"][2]["task_id"].as_str().unwrap();

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
    client.payload(
        "task_blackboard",
        json!({
            "action": "post",
            "task_id": cli_task_id,
            "topic": "hypothesis",
            "payload": "Sibling task context is milestone scoped",
            "author": "cli-author"
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
    assert_eq!(bb.len(), 2);
    assert_eq!(bb[0]["topic"], "architectural_notes");
    assert_eq!(bb[0]["author"], "architect");
    assert_eq!(
        bb[0]["payload"],
        "Route tasks through unified zero-shot context bundle"
    );
    assert!(bb
        .iter()
        .any(|message| { message["payload"] == "Sibling task context is milestone scoped" }));

    // 2. MCP task_claim returns the full bundle when the optional flag is omitted
    let claim_res = client.payload(
        "task_claim",
        json!({
            "task_id": task_id,
            "stage": "contract",
            "worker_id": "author-1",
            "worktree": workspace.0
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
        2
    );

    // 3. CLI task claim returns the same bundle by default without --bundle
    let cli_claim = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            workspace.0.to_str().unwrap(),
            "task",
            "claim",
            cli_task_id,
            "--stage",
            "contract",
            "--worker",
            "cli-author",
            "--worktree",
            workspace.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        cli_claim.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_claim.stderr)
    );
    let cli_claim_json: Value = serde_json::from_slice(&cli_claim.stdout).unwrap();
    assert!(cli_claim_json["context_bundle"].is_object());
    assert_eq!(
        cli_claim_json["context_bundle"]["contract"]["task_id"],
        cli_task_id
    );
    assert_eq!(
        cli_claim_json["context_bundle"]["contract"]["allowed_scope"],
        json!(["src/cli_routing.rs", "tests/test_cli_routing.rs"])
    );
    assert_eq!(
        cli_claim_json["context_bundle"]["blackboard"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // 4. CLI task context command
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
    assert_eq!(
        cli_json["context_bundle"]["contract"]["depends_on"],
        json!([])
    );

    // 5. Minimal claim (bundle: false) prunes gates, attempts, findings, receipt, and spec
    let minimal_claim = client.payload(
        "task_claim",
        json!({
            "task_id": minimal_task_id,
            "stage": "contract",
            "worker_id": "author-minimal",
            "worktree": workspace.0,
            "bundle": false
        }),
    );
    assert!(minimal_claim.get("context_bundle").is_none());
    assert!(minimal_claim.get("gates").is_none());
    assert!(minimal_claim.get("attempts").is_none());
    assert!(minimal_claim.get("findings").is_none());
    assert!(minimal_claim.get("receipt").is_none());
    assert!(minimal_claim.get("spec").is_none());
    assert_eq!(minimal_claim["depends_on"], json!([]));
    assert!(minimal_claim["subtasks"].is_array());
    assert!(minimal_claim["workflow_guidance"].is_object());
    assert!(minimal_claim["allowed_write_scope"].is_array());
    assert_eq!(minimal_claim["task_id"], minimal_task_id);

    // 6. Milestone-scoped blackboard message (task_id IS NULL) with multi-byte Ukrainian text > 500 chars
    let ukr_payload = "Тестовий запис архітектури українською мовою для перевірки безпечного обрізання символів UTF-8. ".repeat(10);
    client.payload(
        "task_blackboard",
        json!({
            "action": "post",
            "milestone_ref": "docs/milestones/010-context.md",
            "topic": "hypothesis",
            "payload": ukr_payload,
            "author": "ukr-architect"
        }),
    );
    let context_after = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
    let bb_after = context_after["context_bundle"]["blackboard"]
        .as_array()
        .unwrap();
    assert_eq!(
        bb_after.len(),
        3,
        "milestone-scoped message with task_id IS NULL must be included"
    );
    let ukr_msg = bb_after
        .iter()
        .find(|m| m["author"] == "ukr-architect")
        .unwrap();
    assert_eq!(ukr_msg["scope"], "milestone");
    assert!(ukr_msg["task_id"].is_null());
    assert!(ukr_msg["subtask_ref"].is_null());
    let ukr_text = ukr_msg["payload"].as_str().unwrap();
    assert!(ukr_text.ends_with("... [truncated]"));
    assert!(ukr_text.chars().count() <= 520);

    let task_msg = bb_after
        .iter()
        .find(|m| m["author"] == "architect")
        .unwrap();
    assert_eq!(task_msg["scope"], "task");
    assert_eq!(task_msg["task_id"], task_id);

    let sibling_msg = bb_after
        .iter()
        .find(|m| m["author"] == "cli-author")
        .unwrap();
    assert_eq!(sibling_msg["scope"], "sibling");
    assert_eq!(sibling_msg["task_id"], cli_task_id);
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

#[test]
fn task_context_candidate_snapshot_delivery_reject_and_completed_receipt() {
    use contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS;
    let workspace = Workspace::new();
    let reviewer = Workspace::new();
    fs::create_dir_all(workspace.0.join("src")).unwrap();
    fs::create_dir_all(workspace.0.join("docs/milestones")).unwrap();
    fs::create_dir_all(reviewer.0.join("src")).unwrap();
    fs::create_dir_all(reviewer.0.join("docs/milestones")).unwrap();

    let manifest = "---\nid: m-snapshots\ntitle: Snapshots and Rejections\ndoc_type: contract\nstatus: active\ninvariants: [review-contour-check]\n---\n```yaml\ntask_ref: candidate-task\ntarget: Deliver candidate\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    workspace.write("docs/milestones/010-snap.md", manifest);
    reviewer.write("docs/milestones/010-snap.md", manifest);
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: .forge/tasks.sqlite\n",
    );
    reviewer.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: .forge/tasks.sqlite\n",
    );

    let mut client = Client::new(&workspace);
    let sync_res = client.payload(
        "task_manage",
        json!({"action": "sync", "milestone_ref": "docs/milestones/010-snap.md"}),
    );
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    // 1. Pass contract with an explicit contract commit
    let contract_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "contract/v1", "worker_id": "author", "worktree": workspace.0}),
    );
    let contract_commit = "1111111111111111111111111111111111111111";
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "contract/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "contract/v1",
                "claim_revision": contract_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "author",
                "worktree": workspace.0,
                "commit": contract_commit,
                "proof": {"contract_proof": {"seam_test_ref": "tests/test.rs", "red_exit_code": 0}}
            }
        }),
    );

    // 2. Pass build/v1 WITHOUT a commit in evidence
    let build_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "build/v1", "worker_id": "builder", "worktree": workspace.0}),
    );
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "build/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "build/v1",
                "claim_revision": build_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "builder",
                "worktree": workspace.0,
                "proof": {"test_proof": {"command": "cargo test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}}
            }
        }),
    );

    // 3. Claim review/v1: candidate_snapshot MUST be None because build/v1 had no commit
    // (must NOT fall back to contract_commit!)
    let review_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "review/v1", "worker_id": "reviewer", "worktree": reviewer.0}),
    );
    assert!(
        review_claim["context_bundle"].get("candidate_snapshot").is_none(),
        "candidate_snapshot must be omitted when build/v1 has no commit (must not fall back to contract commit)"
    );

    // Reject review with findings -> task bounces back to build/v1
    let contours: serde_json::Map<String, Value> = REVIEW_CONTOURS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                json!({"applicable": true, "evidence": "verified"}),
            )
        })
        .collect();
    let review_reject_proof =
        json!({"review_proof": {"decision": "reject", "contours": contours.clone()}});
    let review_findings = json!({"decision": "reject", "notes": "Need build commit"});
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "review/v1",
            "action": "reject",
            "evidence": {
                "task_id": task_id,
                "stage": "review/v1",
                "claim_revision": review_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "reviewer",
                "worktree": reviewer.0,
                "proof": review_reject_proof
            },
            "findings": review_findings
        }),
    );

    // Build claim after review reject: verify unresolved_review_findings contains review_findings
    let build_claim2 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "build/v1", "worker_id": "builder", "worktree": workspace.0}),
    );
    assert_eq!(
        build_claim2["context_bundle"]["unresolved_review_findings"],
        review_findings
    );

    let build_commit = "2222222222222222222222222222222222222222";
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "build/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "build/v1",
                "claim_revision": build_claim2["claim_revision"],
                "contract_revision": 1,
                "worker_id": "builder",
                "worktree": workspace.0,
                "commit": build_commit,
                "proof": {"test_proof": {"command": "cargo test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}}
            }
        }),
    );

    // Now claim review/v1: candidate_snapshot MUST match build_commit
    let review_claim2 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "review/v1", "worker_id": "reviewer", "worktree": reviewer.0}),
    );
    assert_eq!(
        review_claim2["context_bundle"]["candidate_snapshot"]["commit"],
        &build_commit[..7]
    );

    // Pass review/v1
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "review/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "review/v1",
                "claim_revision": review_claim2["claim_revision"],
                "contract_revision": 1,
                "worker_id": "reviewer",
                "worktree": reviewer.0,
                "commit": build_commit,
                "proof": {"review_proof": {"decision": "pass", "contours": contours.clone()}}
            }
        }),
    );

    // 4. Claim deliver/v1 and REJECT delivery with findings
    let deliver_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "deliver/v1", "worker_id": "delivery-lead", "worktree": workspace.0}),
    );
    // At deliver/v1 before completion, receipt must be None
    assert!(deliver_claim["context_bundle"].get("receipt").is_none());

    let delivery_findings = json!({"decision": "reject", "notes": "Missing changelog entry"});
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "deliver/v1",
            "action": "reject",
            "evidence": {
                "task_id": task_id,
                "stage": "deliver/v1",
                "claim_revision": deliver_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "delivery-lead",
                "worktree": workspace.0,
                "commit": build_commit,
                "proof": {"delivery_proof": {"status": "rejected"}}
            },
            "findings": delivery_findings
        }),
    );

    // Task bounced back to build/v1: claim build/v1 and verify unresolved_review_findings contains delivery findings
    let rebounce_build_claim = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "build/v1", "worker_id": "builder", "worktree": workspace.0}),
    );
    assert_eq!(
        rebounce_build_claim["context_bundle"]["unresolved_review_findings"],
        delivery_findings
    );

    // 5. Complete build, review, deliver and verify receipt in context_bundle
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "build/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "build/v1",
                "claim_revision": rebounce_build_claim["claim_revision"],
                "contract_revision": 1,
                "worker_id": "builder",
                "worktree": workspace.0,
                "commit": build_commit,
                "proof": {"test_proof": {"command": "cargo test", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}}
            }
        }),
    );

    let review_claim3 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "review/v1", "worker_id": "reviewer", "worktree": reviewer.0}),
    );
    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "review/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "review/v1",
                "claim_revision": review_claim3["claim_revision"],
                "contract_revision": 1,
                "worker_id": "reviewer",
                "worktree": reviewer.0,
                "commit": build_commit,
                "proof": {"review_proof": {"decision": "pass", "contours": contours}}
            }
        }),
    );

    let deliver_claim2 = client.payload(
        "task_claim",
        json!({"task_id": task_id, "stage": "deliver/v1", "worker_id": "delivery-lead", "worktree": workspace.0}),
    );
    assert!(deliver_claim2["context_bundle"].get("receipt").is_none());

    client.payload(
        "task_submit",
        json!({
            "task_id": task_id,
            "stage": "deliver/v1",
            "action": "pass",
            "evidence": {
                "task_id": task_id,
                "stage": "deliver/v1",
                "claim_revision": deliver_claim2["claim_revision"],
                "contract_revision": 1,
                "worker_id": "delivery-lead",
                "worktree": workspace.0,
                "commit": build_commit,
                "proof": {"delivery_proof": {"status": "passed"}}
            }
        }),
    );

    // Completed task: context_bundle contains receipt, root envelope does not
    let completed_context = client.payload(
        "task_manage",
        json!({"action": "context", "task_id": task_id}),
    );
    assert!(
        completed_context.get("receipt").is_none(),
        "root envelope must prune receipt"
    );
    assert!(
        completed_context["context_bundle"]["receipt"].is_object(),
        "context_bundle must contain receipt for completed task"
    );
}
