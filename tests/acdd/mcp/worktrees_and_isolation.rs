use std::fs;

use serde_json::{json, Value};

use crate::acdd::support::{evidence, restart_mcp_session, review_proof, TasksStore};
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_harness::McpTestHarness;
use contextunity_forge_mcp::core::tasks::profile::ProofKind;

use super::in_process_cli;

#[test]
fn mcp_delivery_preserves_deferred_defects_across_shared_worktrees() {
    let source = include_str!("../fixtures/milestones/deferred_defects.md");
    let builder = TaskHarness::mcp_workspace();
    let reviewer = TaskHarness::mcp_workspace();
    for workspace in [&builder, &reviewer] {
        std::fs::create_dir_all(workspace.root().join("docs/milestones")).unwrap();
        workspace.write("docs/milestones/010-defects.md", source);
    }
    builder.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntask_repository: contextunity\ntask_project: tooling\n",
    );
    reviewer.write(
        "forge-mcp.yaml",
        &format!(
            "roots: []\ndocs: []\ntask_repository: contextunity\ntask_project: tooling\ntasks_db: {}\n",
            builder.root().join(".forge/tasks.sqlite").display()
        ),
    );
    let profile_path = builder.root().join(".forge/acdd/profile.yaml");
    let mut profile = contextunity_forge_mcp::core::tasks::profile::parse_profile(
        &fs::read_to_string(&profile_path).unwrap(),
    )
    .unwrap();
    let delivery_stage = profile
        .executable_gates()
        .into_iter()
        .find(|gate| gate.proof.kind() == Some(ProofKind::Delivery))
        .expect("fixture profile should have a delivery gate")
        .id
        .clone();
    profile
        .find_gate_mut(&delivery_stage)
        .expect("delivery gate should remain addressable")
        .auto_commit = false;
    fs::write(
        &profile_path,
        crate::acdd::support::standalone_profile_yaml(&profile),
    )
    .unwrap();
    let mut current_session = TaskHarness::session_for("builder");
    let mut client =
        McpTestHarness::with_session(&builder, &current_session).expect("MCP client should start");
    let synced = client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"docs/milestones/010-defects.md"}),
        )
        .expect("MCP payload should succeed");
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    assert_eq!(id, "tooling/m-defects:task-a");
    for action in ["preview", "apply", "verify"] {
        let output = in_process_cli::run_cli_json(
            &builder,
            &["migrate", action, "docs/milestones/010-defects.md"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut delivery_commit = None;
    let lifecycle = TaskHarness::attach(&builder, id);
    let gates = lifecycle.executable_gates().unwrap();
    for gate_def in &gates {
        let stage = gate_def.id.as_str();
        let kind = gate_def.proof.kind();
        let is_contract = kind == Some(ProofKind::Contract);
        let is_command = kind == Some(ProofKind::Command);
        let is_review = kind == Some(ProofKind::Review);
        let is_delivery = kind == Some(ProofKind::Delivery);
        let workspace = if is_review { &reviewer } else { &builder };
        let worker = lifecycle.worker_for_gate(gate_def);
        let session = TaskHarness::session_for(&worker);
        if session != current_session {
            client = restart_mcp_session(client, &builder, &session)
                .expect("MCP session restart should succeed");
            current_session = session;
        }
        let claim = client
            .payload(
                "task_claim",
                json!({"task_id":id,"stage":stage,"worker_id":worker,"worktree":workspace.root()}),
            )
            .expect("MCP payload should succeed");
        let proof = if is_delivery {
            json!({"delivery_proof":{"status":"passed"}})
        } else if is_review {
            review_proof("pass")
        } else if is_contract {
            json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
        } else {
            json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}})
        };
        if is_contract || is_command {
            client
                .payload("task_manage", json!({"action":"verify","task_id":id}))
                .expect("MCP payload should succeed");
        }
        let mut evidence = TaskHarness::evidence_for_claim(&claim, proof);
        if is_delivery {
            let (candidate_commit, candidate_baseline) = lifecycle
                .store
                .build_snapshot_candidate(id)
                .unwrap()
                .expect("accepted candidate snapshot should be available");
            evidence.commit = Some(candidate_commit);
            evidence.candidate_baseline_head = Some(candidate_baseline);
        }
        let submitted = client
            .payload(
                "task_submit",
                json!({"task_id":id,"stage":stage,"evidence":evidence,"action":"pass"}),
            )
            .expect("MCP payload should succeed");
        if is_delivery {
            assert_eq!(submitted["status"], "completed");
            delivery_commit = Some(submitted["receipt"]["commit"].as_str().unwrap().to_owned());
        }
    }
    let written =
        fs::read_to_string(builder.root().join("docs/milestones/010-defects.md")).unwrap();
    let milestone =
        contextunity_forge_mcp::core::tasks::Milestone::parse(&written, "contextunity").unwrap();
    assert_eq!(milestone.tasks[0].status.as_deref(), Some("completed"));
    assert_eq!(
        milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        delivery_commit.as_deref()
    );
    assert_eq!(milestone.deferred_defects.len(), 3);
    let frontmatter_defect = &milestone.deferred_defects[0];
    assert_eq!(frontmatter_defect.id, "DEFECT-FRONTMATTER");
    assert_eq!(frontmatter_defect.source, "task_blackboard");
    assert_eq!(frontmatter_defect.title, "Frontmatter defect stays typed");
    assert_eq!(
        frontmatter_defect.path.as_deref(),
        Some("src/engine/tasks.rs")
    );
    assert_eq!(frontmatter_defect.disposition, "subsequent_milestone");
    assert_eq!(
        frontmatter_defect.notes.as_deref(),
        Some("Imported from contract metadata")
    );
    let review_defect = &milestone.deferred_defects[1];
    assert_eq!(review_defect.id, "DEFECT-001");
    assert_eq!(review_defect.source, "review_findings");
    assert_eq!(
        review_defect.title,
        "Unshadowed function resolution bug in edge case"
    );
    assert_eq!(
        review_defect.path.as_deref(),
        Some("src/engine/languages/python.rs")
    );
    assert_eq!(review_defect.disposition, "deferred");
    assert_eq!(
        review_defect.notes.as_deref(),
        Some("Follow up in next milestone")
    );
    client.finish().expect("MCP session should finish");
}

#[test]
fn parking_preserves_and_resumes_the_original_worktree() {
    let fixture = TaskHarness::mcp_dispatch_fixture;
    let call = TaskHarness::mcp_dispatch_call;
    let advance = TaskHarness::mcp_dispatch_advance;
    {
        let (parking, mut client, ids) = fixture(
            "parking",
            &[
                ("holder", "src/holder.rs"),
                ("parked", "src/parked.rs"),
                ("ordinary", "src/ordinary.rs"),
            ],
        );
        parking.write(
            "src/parked.rs",
            "pub fn unfinished_work_is_preserved() {}\n",
        );
        let unfinished = fs::read(parking.root().join("src/parked.rs")).unwrap();
        for (id, worker) in [(&ids[0], "holder-worker"), (&ids[1], "park-worker")] {
            advance(&parking, &mut client, id, "contract", worker)
                .expect("MCP gate advance and prior server finish should succeed");
            client.payload(
                "task_claim",
                json!({"task_id":id,"stage":"build","worker_id":worker,"worktree":parking.root()}),
            ).expect("MCP payload should succeed");
        }
        let parked = call(
            &mut client,
            "task_manage",
            json!({"action":"extend_scope","task_id":ids[1],"paths":["src/holder.rs"]}),
        );
        assert!(
            parked["status"] == "parked" && parked["blocked_by"] == ids[0],
            "{}",
            "extend_scope must park behind the active holder"
        );
        if parked["status"] == "parked" {
            let task_ids = |payload: &Value| {
                let mut listed: Vec<String> = payload["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|task| task["task_id"].as_str().unwrap().to_owned())
                    .collect();
                listed.sort();
                listed
            };
            let mut all_ids = ids.clone();
            all_ids.sort();
            for (status, expected) in [
                ("parked", vec![ids[1].clone()]),
                ("ready", vec![ids[2].clone()]),
                ("in_progress", vec![ids[0].clone()]),
                ("blocked", Vec::new()),
                ("completed", Vec::new()),
                ("all", all_ids),
            ] {
                let listed = call(&mut client, "task_list", json!({"status":status}));
                assert!(
                    task_ids(&listed) == expected,
                    "{}",
                    &format!(
                        "MCP status={status} must preserve the exact task selection after parking"
                    )
                );
            }
            assert!(
                task_ids(&call(&mut client, "task_list", json!({}))) == vec![ids[2].clone()],
                "{}",
                "default MCP status must remain ready and exclude parked work"
            );
            let store = TasksStore::open(&parking.root().join("tasks.sqlite")).unwrap();
            let canonical_parking = parking.root().canonicalize().unwrap();
            let descriptor = serde_json::to_value(store.inspect(&ids[1]).unwrap()).unwrap();
            assert!(
                descriptor["parked_worker_id"] == "park-worker",
                "{}",
                "parking must retain the builder identity"
            );
            assert!(
                descriptor["parked_worktree"] == canonical_parking.to_str().unwrap(),
                "{}",
                "parking must retain the original worktree"
            );
            assert!(
                descriptor["spec"]["scope"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("src/holder.rs")),
                "{}",
                "parking must admit the requested scope"
            );
            let open = TaskHarness::open_claim_count(&store, &ids[1]).unwrap();
            assert!(
                open == 0,
                "{}",
                "parking must release its active claim atomically"
            );
            let blocked = TaskHarness::dependency_wait_count(&store, &ids[1], &ids[0]).unwrap();
            assert!(
                blocked == 1,
                "{}",
                "parking must persist its unsatisfied holder dependency"
            );
            let notes = TaskHarness::blocker_note_count(&store, &ids[1]).unwrap();
            assert!(
                notes > 0,
                "{}",
                "parking must publish a blocker blackboard note"
            );
            let foreign = call(
                &mut client,
                "task_manage",
                json!({"action":"next","worker_id":"foreign","worktree":parking.root()}),
            );
            assert!(
                foreign["task_id"] == ids[2],
                "{}",
                "another worker must not steal blocked parked work"
            );
            let active = store.inspect(&ids[0]).unwrap();
            client.payload(
            "task_submit",
            json!({"task_id":ids[0],"stage":"build","action":"pass","evidence":evidence(&active)}),
        ).expect("MCP payload should succeed");
            advance(&parking, &mut client, &ids[0], "review", "holder-reviewer")
                .expect("MCP gate advance and prior server finish should succeed");
            advance(&parking, &mut client, &ids[0], "deliver", "holder-reviewer")
                .expect("MCP gate advance and prior server finish should succeed");
            let ordinary = store.inspect(&ids[2]).unwrap();
            client.payload("task_submit", json!({"task_id":ids[2],"stage":"contract","action":"pass","evidence":evidence(&ordinary)})).expect("MCP payload should succeed");
            let another_worktree = TaskHarness::mcp_workspace();
            another_worktree.write(
                "forge-mcp.yaml",
                &format!(
                    "roots: []\ndocs: []\ntasks_db: {}\n",
                    parking.root().join("tasks.sqlite").display()
                ),
            );
            another_worktree.write(
                "docs/milestones/010-dispatch.md",
                &fs::read_to_string(parking.root().join("docs/milestones/010-dispatch.md"))
                    .unwrap(),
            );
            let resumed = call(
                &mut client,
                "task_manage",
                json!({"action":"next","worker_id":"park-worker","worktree":another_worktree.root()}),
            );
            assert!(
                resumed["task_id"] == ids[1] && resumed["stage"] == "build",
                "{}",
                "same-worker parked work must outrank ordinary build work after unblocking"
            );
            assert!(
                resumed["worktree"] == canonical_parking.to_str().unwrap(),
                "{}",
                "resume must use the preserved worktree rather than the dispatch argument"
            );
            assert!(
                fs::read(parking.root().join("src/parked.rs")).unwrap() == unfinished,
                "{}",
                "parking and resume must preserve unfinished source bytes"
            );
        }
        client.finish().expect("MCP server should finish");
    }
}
