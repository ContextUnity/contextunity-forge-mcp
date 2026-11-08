use std::fs;
use std::sync::Barrier;

use serde_json::{json, Value};

use crate::acdd::support::TasksStore;
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_harness::McpTestHarness;

use super::{cli_payload, in_process_cli};

#[test]
fn subtask_mcp_and_cli_operations() {
    let workspace = TaskHarness::mcp_workspace();
    fs::create_dir_all(workspace.root().join("docs/milestones")).unwrap();
    let manifest = "---\nid: m-mcp-subtasks\ntitle: MCP Subtasks\ndoc_type: contract\ninvariants: [local-rules]\n---\n```yaml\ntask_ref: sub-demo\ntarget: Subtask demonstration\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    workspace.write("docs/milestones/010-subtasks.md", manifest);
    workspace.write("AGENTS.md", "# Agent Rules\n");
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    client
        .payload(
            "task_manage",
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-subtasks.md"}),
        )
        .expect("MCP payload should succeed");
    let task_id = "forge-mcp/m-mcp-subtasks:sub-demo";

    // MCP task_manage subtask_add
    let add_res = client
        .payload(
            "task_manage",
            json!({
                "action": "subtask_add",
                "task_id": task_id,
                "subtask_ref": "step-1",
                "title": "Initial exploration"
            }),
        )
        .expect("MCP payload should succeed");
    assert_eq!(add_res["subtask"]["subtask_ref"], "step-1");
    assert_eq!(add_res["subtask"]["status"], "ready");

    // MCP task_manage subtask_update
    let update_res = client
        .payload(
            "task_manage",
            json!({
                "action": "subtask_update",
                "task_id": task_id,
                "subtask_ref": "step-1",
                "subtask_status": "completed",
                "evidence": "Explored symbols"
            }),
        )
        .expect("MCP payload should succeed");
    assert_eq!(update_res["subtask"]["status"], "completed");
    assert_eq!(update_res["subtask"]["evidence"], "Explored symbols");

    for (input, expected) in [
        ("ready", "ready"),
        ("in_progress", "in_progress"),
        ("cancelled", "cancelled"),
        ("blocked", "blocked"),
        ("pending", "ready"),
    ] {
        let updated = client
            .payload(
                "task_manage",
                json!({
                    "action": "subtask_update", "task_id": task_id,
                    "subtask_ref": "step-1", "subtask_status": input,
                }),
            )
            .expect("MCP payload should succeed");
        assert_eq!(updated["subtask"]["status"], expected);
        assert_eq!(updated["subtask"]["evidence"], "Explored symbols");
    }

    // CLI task subtask add
    let cli_add_json = cli_payload(
        &workspace,
        &["task", "subtask", "add", task_id, "step-2", "Run unit test"],
    )
    .unwrap();
    assert_eq!(cli_add_json["subtask"]["subtask_ref"], "step-2");
    assert_eq!(cli_add_json["subtask"]["status"], "ready");

    // The legacy CLI status value remains accepted as the ready alias.
    let legacy_json = cli_payload(
        &workspace,
        &[
            "task", "subtask", "update", task_id, "step-2", "--status", "pending",
        ],
    )
    .unwrap();
    assert_eq!(legacy_json["subtask"]["status"], "ready");

    // CLI task subtask list
    let cli_list_json = cli_payload(&workspace, &["task", "subtask", "list", task_id]).unwrap();
    assert_eq!(cli_list_json["subtasks"].as_array().unwrap().len(), 2);

    // MCP task_manage inspect returns subtasks
    let inspected = client
        .payload(
            "task_manage",
            json!({"action": "inspect", "task_id": task_id}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(inspected["subtasks"].as_array().unwrap().len(), 2);

    workspace.write(
        "docs/milestones/010-subtasks.md",
        &manifest.replace(
            "task_ref: sub-demo",
            "task_ref: sub-demo\nstatus: cancelled",
        ),
    );
    client
        .payload(
            "task_manage",
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-subtasks.md"}),
        )
        .expect("MCP payload should succeed");
    let cancelled = client
        .payload(
            "task_list",
            json!({"status": "cancelled", "milestone_ref": "docs/milestones/010-subtasks.md"}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(cancelled["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(cancelled["tasks"][0]["status"], "cancelled");
    let idle = client.payload(
        "task_manage",
        json!({"action": "next", "worker_id": "worker", "worktree": workspace.root().to_str().unwrap()}),
    ).expect("MCP payload should succeed");
    assert_eq!(idle["status"], "idle");
    let (_, cancelled_response) = client
        .call(
            "task_list",
            json!({"status":"cancelled","milestone_ref":"docs/milestones/010-subtasks.md"}),
        )
        .expect("MCP call should succeed");
    let cancelled_markdown = cancelled_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(cancelled_markdown.contains(task_id));
    assert!(cancelled_markdown.contains("cancelled"));
    client.finish().expect("MCP server should finish");
}

#[test]
fn wave_sync_and_atomic_next_follow_dependency_order() {
    let fixture = TaskHarness::mcp_dispatch_fixture;
    let call = TaskHarness::mcp_dispatch_call;
    {
        // Declaration order, rather than lexical identity, determines overlap edges.
        let (waves, mut client, ids) = fixture(
            "waves",
            &[
                ("z-first", "src/shared/"),
                ("b-second", "src/shared/lib.rs"),
                ("a-third", "src/shared/lib.rs"),
                ("a-independent", "src/other.rs"),
            ],
        );
        let store = TasksStore::open(&waves.root().join("tasks.sqlite")).unwrap();
        let originals: Vec<_> = ids.iter().map(|id| store.inspect(id).unwrap()).collect();
        let edges = TaskHarness::dependency_edges(&store).unwrap();
        let mut expected = vec![
            (ids[1].clone(), ids[0].clone()),
            (ids[2].clone(), ids[0].clone()),
            (ids[2].clone(), ids[1].clone()),
        ];
        expected.sort();
        assert!(
            edges == expected,
            "{}",
            "sync must persist only top-down overlap dependencies"
        );
        for (id, wave) in ids.iter().zip([0, 1, 2, 0]) {
            let inspected = client
                .payload("task_manage", json!({"action":"inspect","task_id":id}))
                .expect("MCP payload should succeed");
            assert!(
                inspected["wave_index"] == wave,
                "{}",
                "sync must expose longest-dependency wave indexes"
            );
        }
        client
            .payload(
                "task_manage",
                json!({"action":"sync","milestone_ref":"docs/milestones/010-dispatch.md"}),
            )
            .expect("MCP payload should succeed");
        for original in &originals {
            let current = store.inspect(&original.task_id).unwrap();
            assert!(
                current.spec.depends_on.is_empty(),
                "{}",
                "derived dependencies must not rewrite TaskSpec.depends_on"
            );
            assert!(
                current.digest == original.digest,
                "{}",
                "repeat sync must preserve authority digests"
            );
        }
        let first = call(
            &mut client,
            "task_manage",
            json!({"action":"next","worker_id":"wave-worker","worktree":waves.root()}),
        );
        assert!(
            first["task_id"] == ids[3],
            "{}",
            "equal-wave contract dispatch must sort by task_id"
        );
        let second = call(
            &mut client,
            "task_manage",
            json!({"action":"next","worker_id":"wave-worker-2","worktree":waves.root()}),
        );
        assert!(
            second["task_id"] == ids[0],
            "{}",
            "inferred dependencies must permit the first overlapping owner to claim"
        );
        assert!(
            second["context_bundle"].is_object(),
            "{}",
            "next must return the claimed zero-shot context bundle"
        );
        client.finish().expect("MCP server should finish");
    }
}

#[test]
fn next_prioritizes_independent_delivery_and_review() {
    let fixture = TaskHarness::mcp_dispatch_fixture;
    let call = TaskHarness::mcp_dispatch_call;
    let advance = TaskHarness::mcp_dispatch_advance;
    {
        // Independent deliver/review gates outrank contract work, while the builder
        // remains ineligible to review or deliver its own accepted candidate.
        let (priority, mut client, ids) = fixture(
            "priority",
            &[
                ("z-deliver", "src/deliver.rs"),
                ("b-review", "src/review.rs"),
                ("a-contract", "src/contract.rs"),
            ],
        );
        for id in &ids[..2] {
            advance(&priority, &mut client, id, "contract", "builder")
                .expect("MCP gate advance and prior server finish should succeed");
            advance(&priority, &mut client, id, "build", "builder")
                .expect("MCP gate advance and prior server finish should succeed");
        }
        advance(&priority, &mut client, &ids[0], "review", "reviewer")
            .expect("MCP gate advance and prior server finish should succeed");
        let excluded = call(
            &mut client,
            "task_manage",
            json!({"action":"next","worker_id":"builder","worktree":priority.root()}),
        );
        assert!(
            excluded["task_id"] == ids[2],
            "{}",
            "next must skip the builder's own review and deliver gates"
        );
        for (id, stage) in [(&ids[0], "deliver"), (&ids[1], "review")] {
            let selected = call(
                &mut client,
                "task_manage",
                json!({"action":"next","worker_id":"independent","worktree":priority.root()}),
            );
            assert!(
                selected["task_id"] == *id && selected["stage"] == stage,
                "{}",
                "next must prioritize independent deliver, then review"
            );
        }
        client.finish().expect("MCP server should finish");
    }
}

#[test]
fn concurrent_mcp_next_persists_exactly_one_claim() {
    let fixture = TaskHarness::mcp_dispatch_fixture;
    let call = TaskHarness::mcp_dispatch_call;
    {
        // Two real in-process MCP servers compete for one durable claim.
        let (race, _client, ids) = fixture("race", &[("only", "src/only.rs")]);
        let barrier = Barrier::new(2);
        let outcomes = std::thread::scope(|scope| {
            let handles: Vec<_> = ["racer-a", "racer-b"]
                .into_iter()
                .map(|worker| {
                    let workspace = &race;
                    let barrier = &barrier;
                    let call = &call;
                    scope.spawn(move || {
                        let mut client = McpTestHarness::new(workspace).expect("MCP new should succeed");
                        barrier.wait();
                        let result = call(
                            &mut client,
                            "task_manage",
                            json!({"action":"next","worker_id":worker,"worktree":workspace.root()}),
                        );
                        client.finish().expect("MCP racer server should finish");
                        result
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(
            outcomes
                .iter()
                .filter(|result| result["task_id"] == ids[0])
                .count()
                == 1,
            "{}",
            "concurrent next requests must return exactly one successful claim"
        );
        let store = TasksStore::open(&race.root().join("tasks.sqlite")).unwrap();
        let open = TaskHarness::open_claim_count(&store, &ids[0]).unwrap();
        assert!(
            open == 1,
            "{}",
            "concurrent next must persist exactly one open claim"
        );
        _client.finish().expect("MCP server should finish");
    }
}

#[test]
fn cli_next_returns_claim_and_context() {
    let fixture = TaskHarness::mcp_dispatch_fixture;
    {
        let (cli, _client, ids) = fixture("cli-next", &[("only", "src/only.rs")]);
        let output = in_process_cli::run_cli_json(
            &cli,
            &[
                "task",
                "next",
                "--worker",
                "cli-worker",
                "--worktree",
                cli.root().to_str().unwrap(),
            ],
        );
        let selected: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        assert!(
            output.status.success()
                && selected["task_id"] == ids[0]
                && selected["context_bundle"].is_object(),
            "{}",
            "CLI task next must expose the same atomic claim and context seam"
        );
        _client.finish().expect("MCP server should finish");
    }
}
