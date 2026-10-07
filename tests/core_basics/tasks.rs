use super::ScopedWorkspace;
use contextunity_forge_mcp::{
    core::tasks::{gates::Evidence, Milestone, Receipt, ReceiptRollup, ReviewSummary, GATES},
    db::{
        tasks_store::{TasksStore, RETENTION_SECONDS},
        writer,
    },
    engine::tasks,
};
use serde_json::{json, Value};
fn review_proof(decision: &str) -> serde_json::Value {
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"public-seam verification"}),
                )
            })
            .collect();
    json!({"review_proof":{"decision":decision,"contours":contours}})
}
fn passing_review_proof() -> serde_json::Value {
    review_proof("pass")
}
fn evidence(task: &contextunity_forge_mcp::db::tasks_store::Task) -> Evidence {
    Evidence {
        task_id: task.task_id.clone(),
        stage: GATES[task.gate].into(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        proof: match task.gate {
            0 => {
                json!({"contract_proof":{"seam_test_ref":"tests/core_basics/tasks.rs","red_exit_code":101}})
            }
            1 => {
                json!({"test_proof":{"command":"cargo test --test core_basics","exit_code":0,"tests_passed":1,"tests_failed":0}})
            }
            2 => passing_review_proof(),
            _ => json!({"stage":GATES[task.gate]}),
        },
    }
}
const SPEC: &str = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";
fn fixture() -> (ScopedWorkspace, TasksStore, Milestone) {
    let root = ScopedWorkspace::new("forge_tasks");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-test.md", SPEC);
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    (root, store, milestone)
}

#[test]
fn task_guidance_follows_agent_metadata_workspace_config_and_active_stage() {
    let specification = |agent_type: &str| {
        format!(
        "---\nid: m-guided\ntitle: Guided tasks\ndoc_type: contract\n---\n# Guided tasks\n```yaml\ntask_ref: guided\ntarget: Deliver guidance\nagent_type: {agent_type}\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    )
    };
    let root = ScopedWorkspace::new("forge_guidance_root");
    let linked = ScopedWorkspace::new("forge_guidance_linked");
    let missing = ScopedWorkspace::new("forge_guidance_missing");
    let missing_configured = ScopedWorkspace::new("forge_guidance_missing_configured");
    for (workspace, agent_type) in [
        (&root, "gpt-6-sol"),
        (&linked, "reviewer"),
        (&missing, "flash"),
        (&missing_configured, "worker"),
    ] {
        workspace.write("src/lib.rs", "pub fn guided() {}\n");
        workspace.write("docs/010-guided.md", &specification(agent_type));
    }
    root.write("docs/team-guidance.md", "# Root task guidance\n");
    linked.write("docs/linked-guidance.md", "# Linked task guidance\n");
    root.write(
        "forge-mcp.yaml",
        &format!(
            "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/team-guidance.md\nlinked_workspaces:\n  - name: linked\n    path: {}\n    tasks:\n      enabled: true\n      agents_guidance: docs/linked-guidance.md\n",
            linked.0.display()
        ),
    );
    linked.write(
        "forge-mcp.yaml",
        "task_repository: linked\ntask_project: linked\n",
    );
    missing.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    missing_configured.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/unavailable.md\n",
    );

    for (server, workspace, selector, repository, agent_type, expected_path, absent) in [
        (
            &root.0,
            &root.0,
            None,
            "forge-mcp",
            "gpt-6-sol",
            root.0.join("docs/team-guidance.md"),
            false,
        ),
        (
            &root.0,
            &linked.0,
            Some("linked"),
            "linked",
            "reviewer",
            linked.0.join("docs/linked-guidance.md"),
            false,
        ),
        (
            &missing.0,
            &missing.0,
            None,
            "forge-mcp",
            "flash",
            missing.0.join("AGENTS.md"),
            true,
        ),
        (
            &missing_configured.0,
            &missing_configured.0,
            None,
            "forge-mcp",
            "worker",
            missing_configured.0.join("docs/unavailable.md"),
            true,
        ),
    ] {
        let id = format!("{repository}/{repository}/m-guided:guided");
        tasks::manage(
            server,
            serde_json::from_value(json!({
                "action": "sync",
                "workspace": selector,
                "milestone_ref": "docs/010-guided.md"
            }))
            .unwrap(),
        )
        .unwrap();
        let inspect = || {
            tasks::manage(
                server,
                serde_json::from_value(json!({"action": "inspect", "task_id": id})).unwrap(),
            )
            .unwrap()
        };
        let before = inspect();
        assert_eq!(before["spec"]["agent_type"], agent_type);
        assert_eq!(before["agents_guidance"], expected_path.to_str().unwrap());
        let guidance = &before["workflow_guidance"];
        assert_eq!(guidance["active_stage"], "contract/v1");
        assert_eq!(guidance["agent_type"], agent_type);
        assert_eq!(guidance["subagent_role"], "contract_author");
        assert!(guidance["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()));
        if absent {
            assert_eq!(guidance["warning"]["code"], "TASK_GUIDANCE_MISSING");
            assert!(guidance.to_string().contains("https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md"));
        } else {
            assert!(guidance["warning"].is_null());
        }
        let claimed = tasks::claim(
            server,
            tasks::Claim {
                task_id: id.clone(),
                stage: "contract/v1".into(),
                worker_id: "guidance-contract-author".into(),
                worktree: workspace.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(claimed["workflow_guidance"], *guidance);
        if repository == "forge-mcp" && !absent {
            let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "contract/v1", &evidence(&task), "pass", None)
                .unwrap();
            let build = inspect();
            assert_eq!(build["workflow_guidance"]["active_stage"], "build/v1");
            assert_eq!(build["workflow_guidance"]["subagent_role"], "builder");
            assert_ne!(build["workflow_guidance"]["steps"], guidance["steps"]);
            assert!(build["workflow_guidance"]["steps"]
                .as_array()
                .is_some_and(|steps| steps.len() >= 2));
            let claimed_build = tasks::claim(
                server,
                tasks::Claim {
                    task_id: id.clone(),
                    stage: "build/v1".into(),
                    worker_id: "guidance-builder".into(),
                    worktree: workspace.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                claimed_build["workflow_guidance"]["subagent_role"],
                "builder"
            );
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "build/v1", &evidence(&task), "pass", None)
                .unwrap();
            let review = inspect();
            assert_eq!(review["workflow_guidance"]["active_stage"], "review/v1");
            assert_eq!(
                review["workflow_guidance"]["subagent_role"],
                "independent_reviewer"
            );
            assert_eq!(
                review["workflow_guidance"]["independent_from_worker_id"],
                "guidance-builder"
            );
            assert!(review["workflow_guidance"]["independence_rule"]
                .as_str()
                .is_some_and(|rule| rule.contains("different")));
        }
    }
}

#[test]
fn task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite() {
    let schema = serde_json::to_value(schemars::schema_for!(tasks::Submit)).unwrap();
    assert_eq!(schema["properties"]["evidence"]["type"], "object");
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let contract = store
        .claim(&task_id, "contract", "contract-author", worktree)
        .unwrap();
    let proof = evidence(&contract);
    let raw_evidence = serde_json::to_value(&proof).unwrap();
    for invalid in [
        json!({"task_id":task_id,"stage":"contract/v1","action":"pass"}),
        json!({"task_id":task_id,"stage":"contract/v1","action":"pass","evidence_ref":"proof.yaml"}),
        json!({"task_id":task_id,"stage":"contract/v1","action":"pass","evidence":raw_evidence,"evidence_ref":"proof.yaml"}),
    ] {
        assert!(serde_json::from_value::<tasks::Submit>(invalid).is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let nonobject: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract/v1",
        "action": "pass",
        "evidence": 42
    }))
    .unwrap();
    assert!(tasks::submit(&root.0, nonobject).is_err());
    assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    let request: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract/v1",
        "action": "pass",
        "evidence": raw_evidence
    }))
    .unwrap();

    let submitted = tasks::submit(&root.0, request).unwrap();
    assert_eq!(submitted["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract/v1' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        raw_evidence
    );
    let submission: String = store
        .connection
        .query_row(
            "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
            rusqlite::params![task_id, proof.claim_revision],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"],
        raw_evidence
    );
}

#[test]
fn task_cli_submit_accepts_json_object_and_persists_gate_evidence() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let claimed = store
        .claim(&task_id, "contract", "cli-worker", root.0.to_str().unwrap())
        .unwrap();
    let proof = evidence(&claimed);
    let raw = serde_json::to_string(&proof).unwrap();
    for invalid in ["proof.yaml", "42"] {
        let rejected = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "submit",
                &task_id,
                "--stage",
                "contract/v1",
                "--action",
                "pass",
                "--evidence",
                invalid,
            ])
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "submit",
            &task_id,
            "--stage",
            "contract/v1",
            "--action",
            "pass",
            "--evidence",
            &raw,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract/v1' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        serde_json::to_value(proof).unwrap()
    );
}

#[test]
fn typed_task_proofs_pass_through_engine_and_real_store() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable": true, "evidence": "verified at the public seam"}),
                )
            })
            .collect();
    let cases = [
        (
            "contract",
            "contract-reviewer",
            json!({"contract_proof":{"seam_test_ref":"tests/core_basics/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}}),
            vec![
                json!({"contract_proof":{"seam_test_ref":"tests/core_basics/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":0}}),
                json!({"seam_test_ref":"tests/core_basics/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}),
                json!({"contract_proof":{"seam_test":"tests/core_basics/tasks.rs","red_exit_code":101}}),
            ],
        ),
        (
            "build",
            "builder",
            json!({"test_proof":{"command":"cargo test --test core_basics","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"running 2 tests\ntest result: ok. 2 passed ✓"}}),
            vec![
                json!({"test_proof":{"command":"cargo test --test core_basics","exit_code":1,"tests_passed":1,"tests_failed":1}}),
                json!({"test_proof":{"command":"cargo test --test core_basics","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"x".repeat(65_537)}}),
                json!({"command":"cargo test --test core_basics","result":"passed","artifacts":[]}),
                json!({"test_proof":{"command":"cargo test --test core_basics","exit_code":0,"tests_passed":2,"tests_failed":0,"output":"passed"}}),
            ],
        ),
        (
            "review",
            "independent-reviewer",
            json!({"review_proof":{"decision":"pass","contours":contours.clone()}}),
            vec![
                json!({"review_proof":{"decision":"pass","contours":{}}}),
                json!({"decision":"pass","contours":contours}),
            ],
        ),
    ];
    for (stage, worker, proof, invalid_cases) in cases {
        let claimed = store.claim(&task_id, stage, worker, worktree).unwrap();
        let mut evidence = evidence(&claimed);
        for invalid in invalid_cases {
            evidence.proof = invalid;
            let request: tasks::Submit = serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": evidence,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, request).is_err());
            assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
        }
        evidence.proof = proof.clone();
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id": task_id,
            "stage": stage,
            "action": "pass",
            "evidence": evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(result["status"], "ready");
        let stored: String = store
            .connection
            .query_row(
                "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed'",
                rusqlite::params![task_id, GATES[claimed.gate]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["proof"],
            proof
        );
        let submission: String = store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![task_id, claimed.claim_revision],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"]["proof"],
            proof
        );
    }
}

#[test]
fn rejected_task_gates_require_typed_proof_and_persist_failing_evidence() {
    for gate in [0_usize, 1, 2] {
        let (root, mut store, milestone) = fixture();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let worktree = root.0.to_str().unwrap();
        for stage in GATES.iter().take(gate) {
            let claimed = store.claim(&task_id, stage, "builder", worktree).unwrap();
            store
                .submit(&task_id, stage, &evidence(&claimed), "pass", None)
                .unwrap();
        }
        let worker = if gate == 2 { "reviewer" } else { "builder" };
        let claimed = store
            .claim(&task_id, GATES[gate], worker, worktree)
            .unwrap();
        let (bare, typed) = match gate {
            0 => (
                json!({"seam_test_ref":"tests/core_basics/tasks.rs","red_exit_code":0}),
                json!({"contract_proof":{"seam_test_ref":"tests/core_basics/tasks.rs","red_exit_code":0}}),
            ),
            1 => (
                json!({"command":"cargo test","result":"failed","artifacts":[]}),
                json!({"test_proof":{"command":"cargo test","exit_code":101,"tests_passed":0,"tests_failed":1,"log":"one test failed"}}),
            ),
            _ => (
                json!({"decision":"reject","contours":{}}),
                review_proof("reject"),
            ),
        };
        let findings = json!({"decision":"remediate"});
        let mut proof = evidence(&claimed);
        proof.proof = bare;
        assert!(store
            .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
            .is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
        if gate == 2 {
            for invalid in [
                json!({"review_proof":{"decision":"reject","contours":{}}}),
                passing_review_proof(),
            ] {
                proof.proof = invalid;
                assert!(store
                    .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
                    .is_err());
                assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
            }
        }
        proof.proof = typed.clone();
        let submitted = store
            .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
            .unwrap();
        assert_eq!(submitted.status, "ready");
        let stored: String = store
            .connection
            .query_row(
                "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='rejected'",
                rusqlite::params![task_id, GATES[gate]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["proof"],
            typed
        );
        let submission: String = store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![task_id, claimed.claim_revision],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"]["proof"],
            typed
        );
    }
}

#[test]
fn blackboard_persists_task_scoped_messages_in_chronological_order() {
    let (root, store, milestone) = fixture();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let first_id = store
        .blackboard_post(&first, "author-a", "contract_draft", "red test")
        .unwrap();
    let second_id = store
        .blackboard_post(&first, "author-b", "architectural_notes", "use WAL")
        .unwrap();
    store
        .blackboard_post(&second, "author-c", "contract_draft", "other task")
        .unwrap();
    assert!(second_id > first_id);
    let all = store.blackboard_read(&first, None, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, first_id);
    assert_eq!(all[0].author, "author-a");
    assert_eq!(all[1].id, second_id);
    assert!(all[0].created_at <= all[1].created_at);
    assert_eq!(
        store
            .blackboard_read(&first, Some("architectural_notes"), Some(1))
            .unwrap()[0]
            .payload
            .as_deref(),
        Some("use WAL")
    );
    assert_eq!(store.blackboard_clear(&first).unwrap(), 2);
    assert!(store
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 1);
    let index: String = store.connection.query_row(
        "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='task_blackboard' AND sql LIKE '%task_id, created_at%'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(index, "idx_task_blackboard_task_created");
    drop(root);
}

#[test]
fn blackboard_schema_v1_upgrade_keeps_task_store_operational() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    store
        .connection
        .execute_batch(
            "DROP TABLE task_blackboard;
             CREATE TABLE task_blackboard(
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,
                 author TEXT NOT NULL,
                 topic TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             INSERT INTO task_blackboard(id,task_id,author,topic,payload,created_at)
             VALUES(73,'forge-mcp/forge-mcp/m-test:first','legacy-worker','build_proof','preserved payload',1234);
             UPDATE task_store_metadata SET value='1' WHERE key='schema_version';",
        )
        .unwrap();
    drop(store);

    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(reopened.inspect(&task_id).unwrap().status, "ready");
    let schema_version: String = reopened
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, "2");
    let task_id_nullable: i64 = reopened
        .connection
        .query_row(
            "SELECT \"notnull\" FROM pragma_table_info('task_blackboard') WHERE name='task_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(task_id_nullable, 0);
}

#[test]
fn blackboard_scope_resolution_isolates_milestone_task_and_subtask_messages() {
    let root = ScopedWorkspace::new("forge_blackboard_hierarchy");
    let manifest = "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\nstatus: active\n---\n# Blackboard\n```yaml\ntask_ref: first\ntarget: Coordinate task agents\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Coordinate another task\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    root.write("docs/milestones/010-blackboard.md", manifest);
    let milestone = Milestone::parse(manifest, "forge-mcp").unwrap();
    let database = root.0.join(".forge/tasks.sqlite");
    let mut store = TasksStore::open(&database).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-blackboard.md", &root.0)
        .unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    store
        .claim(&first, "contract", "worker-one", root.0.to_str().unwrap())
        .unwrap();
    store
        .subtask_add(&first, "active-slice", "Resolve the active subtask")
        .unwrap();
    store
        .subtask_update(&first, "active-slice", "in_progress", None)
        .unwrap();

    let call = |args| tasks::blackboard(&root.0, serde_json::from_value(args).unwrap(), "test");
    let milestone_post = call(json!({
        "action":"post","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md",
        "topic":"architecture","payload":"milestone coordination"
    }))
    .unwrap();
    let milestone_message_id = milestone_post["id"].as_u64().unwrap();
    let task_post = call(json!({
        "action":"post","task_id":first,
        "topic":"build_proof","payload":"task coordination"
    }))
    .unwrap();
    let task_message_id = task_post["id"].as_u64().unwrap();
    let subtask_post = call(json!({
        "action":"post","scope":"subtask",
        "topic":"architectural_seam","payload":"subtask coordination"
    }))
    .unwrap();
    let subtask_message_id = subtask_post["id"].as_u64().unwrap();

    let milestone_page = call(json!({
        "action":"read","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md"
    }))
    .unwrap();
    assert_eq!(milestone_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(milestone_page["messages"][0]["id"], milestone_message_id);
    assert_eq!(milestone_page["messages"][0]["task_id"], Value::Null);
    assert!(milestone_page["messages"][0].get("payload").is_none());

    let task_page = call(json!({"action":"read","limit":100})).unwrap();
    assert_eq!(task_page["pagination"]["limit"], 50);
    assert_eq!(task_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(task_page["messages"][0]["id"], task_message_id);
    assert_eq!(task_page["messages"][0]["task_id"], first);
    assert_eq!(task_page["messages"][0]["subtask_ref"], Value::Null);
    assert!(task_page["messages"][0].get("payload").is_none());

    let subtask_page = call(json!({"action":"read","scope":"subtask"})).unwrap();
    assert_eq!(subtask_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(subtask_page["messages"][0]["id"], subtask_message_id);
    assert_eq!(subtask_page["messages"][0]["task_id"], first);
    assert_eq!(subtask_page["messages"][0]["subtask_ref"], "active-slice");

    let inspected = call(json!({"action":"inspect","message_id":subtask_message_id})).unwrap();
    assert_eq!(inspected["message"]["payload"], "subtask coordination");

    store
        .claim(&second, "contract", "worker-two", root.0.to_str().unwrap())
        .unwrap();
    let ambiguous = call(json!({"action":"read","scope":"task"})).unwrap_err();
    assert!(ambiguous
        .to_string()
        .contains("TASK_BLACKBOARD_AMBIGUOUS_TASK"));
    let still_isolated = call(json!({
        "action":"read","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md"
    }))
    .unwrap();
    assert_eq!(still_isolated["messages"][0]["id"], milestone_message_id);
}

#[test]
fn blackboard_automatic_scope_fails_closed_without_one_active_context() {
    for (name, manifests, expected) in [
        (
            "forge_blackboard_no_active_context",
            vec![("010-planned.md", "planned")],
            "TASK_BLACKBOARD_NO_ACTIVE_CONTEXT",
        ),
        (
            "forge_blackboard_ambiguous_milestones",
            vec![("010-first.md", "active"), ("020-second.md", "active")],
            "TASK_BLACKBOARD_AMBIGUOUS_MILESTONE",
        ),
    ] {
        let root = ScopedWorkspace::new(name);
        root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
        for (filename, status) in manifests {
            root.write(
                &format!("docs/milestones/{filename}"),
                &format!("---\nid: m-{filename}\ntitle: {filename}\ndoc_type: contract\nstatus: {status}\n---\n"),
            );
        }
        let request: tasks::BlackboardRequest =
            serde_json::from_value(json!({"action":"read"})).unwrap();
        let error = tasks::blackboard(&root.0, request, "test").unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn blackboard_concurrent_connections_persist_and_cascade() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let database = root.0.join(".forge/tasks.sqlite");
    let workers: Vec<_> = (0..2)
        .map(|worker| {
            let database = database.clone();
            let task_id = task_id.clone();
            std::thread::spawn(move || {
                let connection = TasksStore::open(&database).unwrap();
                for index in 0..20 {
                    connection
                        .blackboard_post(
                            &task_id,
                            &format!("worker-{worker}"),
                            "build_proof",
                            &index.to_string(),
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    drop(store);
    let reopened = TasksStore::open(&database).unwrap();
    assert_eq!(
        reopened
            .blackboard_read(&task_id, None, None)
            .unwrap()
            .len(),
        40
    );
    reopened
        .connection
        .execute("DELETE FROM tasks WHERE task_id=?1", [&task_id])
        .unwrap();
    assert!(reopened
        .blackboard_read(&task_id, None, None)
        .unwrap()
        .is_empty());
}

static BLACKBOARD_BUSY_SIGNAL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> =
    std::sync::OnceLock::new();
static BLACKBOARD_BUSY_NOTIFIED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn signal_blackboard_write_contention(_: i32) -> bool {
    if !BLACKBOARD_BUSY_NOTIFIED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        if let Some(signal) = BLACKBOARD_BUSY_SIGNAL.get() {
            let _ = signal.send(());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(1));
    true
}

#[test]
fn blackboard_post_fails_closed_when_task_completes_during_write() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let task = store.inspect(&task_id).unwrap();
    let milestone_ref = store.milestone_scope_ref(&task.milestone_ref).unwrap();
    drop(store);

    let database = root.0.join(".forge/tasks.sqlite");
    let mut completion = TasksStore::open(&database).unwrap();
    let poster = TasksStore::open(&database).unwrap();
    let (busy_tx, busy_rx) = std::sync::mpsc::channel();
    BLACKBOARD_BUSY_NOTIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(BLACKBOARD_BUSY_SIGNAL.set(busy_tx).is_ok());
    poster
        .connection
        .busy_handler(Some(signal_blackboard_write_contention))
        .unwrap();

    let transition = completion
        .connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transition
        .execute(
            "UPDATE tasks SET descriptor=json_set(descriptor,'$.status','completed') WHERE task_id=?1",
            [&task_id],
        )
        .unwrap();
    let post = std::thread::spawn(move || {
        poster.blackboard_post_scoped(
            &milestone_ref,
            Some(&task_id),
            None,
            "worker",
            "build_proof",
            "post races task completion",
        )
    });

    busy_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("post reached SQLite write contention");
    transition.commit().unwrap();
    let error = post.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("TASK_TERMINAL"), "{error}");
}

#[test]
fn linked_task_workspaces_share_storage_and_confine_local_scope() {
    let (root, _, _) = fixture();
    let linked = ScopedWorkspace::new("forge_linked_tasks");
    let missing = ScopedWorkspace::new("forge_missing_milestones");
    let empty = ScopedWorkspace::new("forge_empty_milestones");
    empty.write("docs/milestones/README.md", "# No task manifests\n");
    linked.write("src/lib.rs", "pub fn linked() {}\n");
    linked.write("docs/AGENTS.md", "# Linked repository rules\n");
    linked.write(
        "forge-mcp.yaml",
        "task_repository: traverse-library\ntask_project: tooling\n",
    );
    linked.write(
        "contracts/010-linked.md",
        &SPEC
            .replace("id: m-test", "id: m-linked")
            .replace("isolated", "linked-rules"),
    );
    root.write("forge-mcp.yaml", &format!("roots: [src]\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: docs/AGENTS.md\n  - name: missing\n    path: {}\n    tasks: {{enabled: true}}\n  - name: empty\n    path: {}\n    tasks: {{enabled: true}}\n  - name: disabled\n    path: {}\n    tasks: {{enabled: false}}\n  - name: index-only\n    path: {}\n",linked.0.display(),missing.0.display(),empty.0.display(),linked.0.display(),linked.0.display()));
    let sync = |workspace: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"sync","workspace":workspace})).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(sync("missing")["tasks"], json!([]));
    assert_eq!(sync("empty")["tasks"], json!([]));
    let imported = sync("traverse");
    assert_eq!(imported["tasks"].as_array().unwrap().len(), 2);
    let id = imported["tasks"][0]["task_id"].as_str().unwrap();
    assert!(id.starts_with("traverse-library/tooling/"));
    let list =
        |arguments| tasks::list(&root.0, serde_json::from_value(arguments).unwrap()).unwrap();
    let linked_by_milestone = list(json!({"milestone_ref":"m-linked","status":"all"}));
    let linked_by_milestone_tasks = linked_by_milestone["tasks"].as_array().unwrap();
    assert_eq!(linked_by_milestone_tasks.len(), 2);
    assert!(linked_by_milestone_tasks.iter().all(|task| {
        task["task_id"]
            .as_str()
            .unwrap()
            .starts_with("traverse-library/tooling/")
    }));
    assert_eq!(list(json!({}))["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        list(json!({"repository":"all"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        list(json!({"repository":"traverse"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        list(json!({"repository":"all","status":"all"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    linked.write(
        "contracts/020-linked.md",
        &SPEC.replace("m-test", "m-linked-020"),
    );
    root.write(
        "docs/milestones/020-primary-alpha.md",
        &SPEC.replace("m-test", "m-primary-alpha"),
    );
    root.write(
        "docs/milestones/020-primary-beta.md",
        &SPEC.replace("m-test", "m-primary-beta"),
    );
    let ambiguous_primary = tasks::list(
        &root.0,
        serde_json::from_value(json!({"milestone_ref":"020","status":"all"})).unwrap(),
    )
    .unwrap_err();
    assert!(
        ambiguous_primary
            .to_string()
            .contains("multiple active documents"),
        "a primary ambiguity must not fall through to a unique linked match: {ambiguous_primary}"
    );
    let inspect = tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"inspect","task_id":id})).unwrap(),
    )
    .unwrap();
    assert_eq!(inspect["workspace_root"], linked.0.to_str().unwrap());
    assert_eq!(
        inspect["agents_guidance"],
        linked.0.join("docs/AGENTS.md").to_str().unwrap()
    );
    assert_eq!(inspect["applicable_invariants"], json!(["linked-rules"]));
    let extend = |path: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    assert!(extend("src/new.rs").is_ok());
    std::os::unix::fs::symlink(root.0.join("src"), linked.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/main.rs").is_err());
    assert!(extend("../outside.rs").is_err());
    let claim = |worktree: &std::path::Path| {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.into(),
                stage: "contract".into(),
                worker_id: "linked-builder".into(),
                worktree: worktree.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
    };
    assert!(claim(&root.0).is_err());
    let claimed = claim(&linked.0).unwrap();
    assert_eq!(claimed["workspace_root"], inspect["workspace_root"]);
    assert_eq!(claimed["agents_guidance"], inspect["agents_guidance"]);
    let connection = rusqlite::Connection::open(root.0.join(".forge/tasks.sqlite")).unwrap();
    let namespaces: i64 = connection
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(namespaces, 4);
}
#[test]
fn linked_scope_extension_validates_owner_and_claimed_worktree() {
    let (root, _, _) = fixture();
    let owner = ScopedWorkspace::new("forge_scope_owner");
    let worktree = ScopedWorkspace::new("forge_scope_worktree");
    for workspace in [&owner, &worktree] {
        workspace.write("src/lib.rs", "pub fn local() {}\n");
        workspace.write("docs/010-test.md", SPEC);
    }
    root.write(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n",
            owner.0.display()
        ),
    );
    let synced = tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/010-test.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: worktree.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let extend = |path: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    std::os::unix::fs::symlink(root.0.join("src"), owner.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    std::fs::remove_file(owner.0.join("src/foreign")).unwrap();
    std::os::unix::fs::symlink(root.0.join("src"), worktree.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    assert!(extend("src/local.rs").is_ok());
}
#[test]
fn nested_repository_tasks_preserve_distinct_write_perimeters() {
    let main = ScopedWorkspace::new("forge_nested_repositories");
    main.write("src/main.rs", "pub fn main_owner() {}\n");
    main.write(
        "docs/010-main.md",
        &SPEC.replace("scope: [src/]", "scope: [src/main.rs]"),
    );
    main.write("src/library/src/lib.rs", "pub fn library_owner() {}\n");
    main.write("src/library/docs/milestones/010-lib.md", SPEC);
    main.write("forge-mcp.yaml", "linked_workspaces:\n  - name: library\n    path: src/library\n    tasks: {enabled: true}\n");
    let imported = tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-main.md"}))
            .unwrap(),
    )
    .unwrap();
    let primary_id = imported["tasks"][0]["task_id"].as_str().unwrap();
    let imported_linked = tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","workspace":"library"})).unwrap(),
    )
    .unwrap();
    let library_id = imported_linked["tasks"][0]["task_id"].as_str().unwrap();
    let extend = |path: &str| {
        tasks::manage(
            &main.0,
            serde_json::from_value(
                json!({"action":"extend_scope","task_id":primary_id,"paths":[path]}),
            )
            .unwrap(),
        )
    };
    assert!(extend("src/library/src/foreign.rs").is_err());
    assert!(extend("src/main_helpers.rs").is_ok());
    let claimed = tasks::claim(
        &main.0,
        tasks::Claim {
            task_id: library_id.into(),
            stage: "contract".into(),
            worker_id: "library-builder".into(),
            worktree: main.0.join("src/library").to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        claimed["workspace_root"],
        main.0.join("src/library").to_str().unwrap()
    );
    main.write(
        "docs/010-wide.md",
        &SPEC.replace("id: m-test", "id: m-wide"),
    );
    assert!(tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-wide.md"}))
            .unwrap()
    )
    .is_err());
}
#[test]
fn task_specification_and_store_survive_code_index_rebuild() {
    let (root, mut store, milestone) = fixture();
    let first = &milestone.tasks[0];
    let digest = milestone.digest(first).unwrap();
    let mut changed = milestone.clone();
    changed.tasks[1].target = "Sibling amendment".into();
    assert_eq!(digest, changed.digest(first).unwrap());
    let id = milestone.task_id(first);
    let claim = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    drop(store);
    let store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        store.inspect(&id).unwrap().claim_revision,
        claim.claim_revision
    );
    assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
    let wal: String = store
        .connection
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wal, "wal");
    let timeout: i64 = store
        .connection
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5000);
}
#[test]
fn task_configuration_defaults_and_crlf_preserve_operational_identity() {
    let (root, _, milestone) = fixture();
    root.write("forge-mcp.yaml", "roots: [src]\n");
    assert_eq!(
        tasks::database_path(&root.0).unwrap(),
        root.0.join(".forge/tasks.sqlite")
    );
    let crlf = Milestone::parse(&SPEC.replace('\n', "\r\n"), "forge-mcp").unwrap();
    assert_eq!(
        crlf.task_id(&crlf.tasks[0]),
        milestone.task_id(&milestone.tasks[0])
    );
    assert_eq!(
        crlf.digest(&crlf.tasks[0]).unwrap(),
        milestone.digest(&milestone.tasks[0]).unwrap()
    );
    let id = milestone.task_id(&milestone.tasks[0]);
    let missing = root.0.join("not-created");
    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: missing.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("WORKTREE_NOT_FOUND"));
    let task = tasks::store(&root.0).unwrap().inspect(&id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(task.worker_id.is_none());
}
#[test]
fn linked_worktree_resolves_tasks_db_to_primary_worktree_root() {
    let primary = ScopedWorkspace::new("forge_primary_git");
    let wt = ScopedWorkspace::new("forge_wt_git");
    let run = |dir: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };
    run(&primary.0, &["init", "-b", "main"]);
    run(&primary.0, &["config", "user.name", "Test User"]);
    run(&primary.0, &["config", "user.email", "test@example.com"]);
    primary.write("README.md", "# test\n");
    primary.write("forge-mcp.yaml", "roots: [src]\n");
    run(&primary.0, &["add", "."]);
    run(&primary.0, &["commit", "-m", "initial"]);
    let wt_out = run(
        &primary.0,
        &["worktree", "add", wt.0.to_str().unwrap(), "-b", "test-wt"],
    );
    if wt_out.status.success() {
        let expected = primary
            .0
            .canonicalize()
            .unwrap()
            .join(".forge/tasks.sqlite");
        let resolved = tasks::database_path(&wt.0).unwrap();
        assert_eq!(resolved, expected);
        // Fallback when forge-mcp.yaml is absent in worktree:
        let _ = std::fs::remove_file(wt.0.join("forge-mcp.yaml"));
        let resolved_fallback = tasks::database_path(&wt.0).unwrap();
        assert_eq!(resolved_fallback, expected);
        let _ = run(
            &primary.0,
            &["worktree", "remove", "--force", wt.0.to_str().unwrap()],
        );
    }
}
#[test]
fn tasks_coordinate_claims_dependencies_reset_and_scope() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let listed = tasks::list(
        &root.0,
        tasks::List {
            repository: None,
            milestone_ref: None,
            milestone_status: None,
            status: None,
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(listed["tasks"][0]["task_id"], id);
    let path = root.0.join(".forge/tasks.sqlite");
    let worktree = root.0.to_str().unwrap().to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let path = path.clone();
            let worktree = worktree.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = TasksStore::open(&path).unwrap();
                barrier.wait();
                store.claim(&id, "contract", &format!("worker-{n}"), &worktree)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.to_string() == "TASK_ALREADY_CLAIMED"));
    let before = store.inspect(&id).unwrap();
    let reset = store.reset(&id).unwrap();
    assert!(reset.claim_revision > before.claim_revision);
    assert_eq!(reset.status, "ready");
    let extended = store
        .extend_scope(&id, &["src/next.rs".into()], &root.0)
        .unwrap();
    assert!(extended.spec.scope.contains(&"src/next.rs".into()));
    for path in ["../escape", "docs/outside.rs"] {
        assert!(store.extend_scope(&id, &[path.into()], &root.0).is_err());
    }
    let external = ScopedWorkspace::new("forge_scope_external");
    std::os::unix::fs::symlink(&external.0, root.0.join("src/escape")).unwrap();
    assert!(store
        .extend_scope(&id, &["src/escape/file.rs".into()], &root.0)
        .is_err());
    std::os::unix::fs::symlink(root.0.join("docs"), root.0.join("src/module_escape")).unwrap();
    assert!(store
        .extend_scope(&id, &["src/module_escape/file.rs".into()], &root.0)
        .is_err());
}
#[test]
fn gates_write_receipts_and_preserve_retention_outcomes() {
    let (root, mut store, milestone) = fixture();
    let claimed_worktree = ScopedWorkspace::new("forge_claim_receipt");
    claimed_worktree.write("src/lib.rs", "pub fn example() {}\n");
    let id = milestone.task_id(&milestone.tasks[0]);
    for (gate, stage) in GATES.iter().enumerate() {
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let worktree = if gate == 3 { &claimed_worktree } else { &root };
        let task = store
            .claim(&id, stage, worker, worktree.0.to_str().unwrap())
            .unwrap();
        let evidence = Evidence {
            task_id: id.clone(),
            stage: stage.to_string(),
            claim_revision: task.claim_revision,
            contract_revision: task.contract_revision,
            worker_id: worker.into(),
            worktree: worktree.0.to_str().unwrap().into(),
            commit: "0123456789abcdef0123456789abcdef01234567".into(),
            proof: match gate {
                0 => {
                    json!({"contract_proof":{"seam_test_ref":"tests/core_basics/tasks.rs","red_exit_code":101}})
                }
                1 => {
                    json!({"test_proof":{"command":"cargo test --test core_basics","exit_code":0,"tests_passed":1,"tests_failed":0}})
                }
                2 => passing_review_proof(),
                _ => json!({"stage":stage}),
            },
        };
        if gate == 3 {
            assert!(store.submit(&id, stage, &evidence, "pass", None).is_err());
            assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
            claimed_worktree.write("docs/010-test.md", SPEC);
        }
        let result = store.submit(&id, stage, &evidence, "pass", None).unwrap();
        let retry = store.submit(&id, stage, &evidence, "pass", None).unwrap();
        assert_eq!(retry.claim_revision, result.claim_revision);
        assert_eq!(result.status, if gate == 3 { "completed" } else { "ready" });
    }
    let completed = store.inspect(&id).unwrap();
    assert!(completed.receipt.as_ref().unwrap().rollup.is_some());
    let at = completed.completed_at.unwrap();
    assert!(store
        .delete(Some(&id), None, false, at + RETENTION_SECONDS)
        .is_err());
    assert_eq!(store.cleanup(at + RETENTION_SECONDS + 1).unwrap(), vec![id]);
    assert_eq!(store.list(None, "ready", None).unwrap().len(), 1);
    let rows: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM task_claims", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
#[test]
fn specifications_validate_unique_identities_and_governing_digests() {
    let parsed = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let first = &parsed.tasks[0];
    let digest = parsed.digest(first).unwrap();
    let mut completed = first.clone();
    completed.status = Some("completed".into());
    completed.receipt = Some(Receipt {
        commit: "candidate".into(),
        contract_revision: 1,
        passed_at: "timestamp".into(),
        evidence: json!({}),
        review: json!({}),
        decision: "pass".into(),
        rollup: None,
    });
    assert_eq!(parsed.digest(&completed).unwrap(), digest);
    let mut constrained = parsed.clone();
    constrained.invariants.push("WAL coordination".into());
    assert_ne!(constrained.digest(first).unwrap(), digest);
    for malformed in [
        SPEC.replace("task_ref: second", "task_ref: first"),
        SPEC.replace("depends_on: [first]", "depends_on: [missing]"),
        SPEC.replace("scope: [src/]", "scope: [../outside]"),
        SPEC.replace("id: m-test", "id: []"),
    ] {
        assert!(Milestone::parse(&malformed, "forge-mcp").is_err());
    }
}
#[test]
fn contract_readmission_and_reset_fence_old_evidence() {
    let (root, mut store, mut milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    let stale = evidence(&first);
    let reset = store.reset(&id).unwrap();
    assert!(store.submit(&id, "contract", &stale, "pass", None).is_err());
    let second = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    assert!(second.claim_revision > reset.claim_revision);
    milestone.tasks[0].target = "Amended target".into();
    assert!(store.sync(&milestone, "docs/010-test.md", &root.0).is_err());
    assert_eq!(
        store.inspect(&id).unwrap().worker_id.as_deref(),
        Some("builder")
    );
    milestone.tasks[0].contract_revision = 2;
    let synced = store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    assert_eq!(synced[0].contract_revision, 2);
    assert_eq!(synced[0].gate, 0);
    assert!(synced[0].claim_revision > second.claim_revision);
    assert!(store
        .submit(&id, "contract", &evidence(&second), "pass", None)
        .is_err());
}
#[test]
fn review_rejection_retains_findings_and_requires_a_new_build() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    for stage in ["contract", "build"] {
        let claimed = store
            .claim(&id, stage, "builder", root.0.to_str().unwrap())
            .unwrap();
        store
            .submit(&id, stage, &evidence(&claimed), "pass", None)
            .unwrap();
    }
    let claimed = store
        .claim(&id, "review", "builder", root.0.to_str().unwrap())
        .unwrap();
    let mut review = evidence(&claimed);
    review.proof = passing_review_proof();
    assert!(store.submit(&id, "review", &review, "pass", None).is_err());
    store.reset(&id).unwrap();
    let claimed = store
        .claim(&id, "review", "reviewer", root.0.to_str().unwrap())
        .unwrap();
    let findings = json!({"paths":["src/lib.rs"],"decision":"remediate"});
    let mut review = evidence(&claimed);
    review.proof = review_proof("reject");
    let rejected = store
        .submit(&id, "review", &review, "reject", Some(&findings))
        .unwrap();
    assert_eq!(rejected.gate, 1);
    assert_eq!(rejected.status, "ready");
    assert_eq!(
        store.inspect_details(&id).unwrap()["findings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let rebuilt = store
        .claim(&id, "build", "builder", root.0.to_str().unwrap())
        .unwrap();
    store
        .submit(&id, "build", &evidence(&rebuilt), "pass", None)
        .unwrap();
    assert_eq!(store.inspect(&id).unwrap().gate, 2);
}
#[test]
fn force_deletion_preserves_project_boundaries_and_revision_fencing() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "contract", "worker", root.0.to_str().unwrap())
        .unwrap();
    assert!(store
        .delete(None, Some("docs/010-test.md"), false, 0)
        .is_err());
    assert_eq!(store.list(None, "all", None).unwrap().len(), 2);
    let deleted = store.delete(Some(&id), None, true, 0).unwrap();
    assert_eq!(deleted.revoked_claims, 1);
    assert_eq!(deleted.count, 1);
    assert!(store.list(None, "ready", None).unwrap().is_empty());
    let recreated = store
        .create(&milestone, "docs/010-test.md", &root.0, "first")
        .unwrap();
    assert!(recreated[0].claim_revision > first.claim_revision);
    let other = contextunity_forge_mcp::db::tasks_store::TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "other",
        "project",
    )
    .unwrap();
    assert!(other.list(None, "all", None).unwrap().is_empty());
    assert!(other.inspect(&id).is_err());
}
#[test]
fn dedicated_store_rejects_index_and_unknown_schema_without_ddl() {
    let root = ScopedWorkspace::new("forge_task_schema");
    root.write("src/lib.rs", "pub fn sample() {}\n");
    root.write("forge-mcp.yaml", "roots: [src]\ndocs: []\n");
    let index = root.0.join("index.sqlite");
    writer::build(&root.0, &index, None).unwrap();
    assert!(TasksStore::open(&index).is_err());
    let conn = rusqlite::Connection::open(&index).unwrap();
    let task_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='tasks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_tables, 0);
    let future = root.0.join("future.sqlite");
    let conn = rusqlite::Connection::open(&future).unwrap();
    conn.execute_batch("CREATE TABLE task_store_metadata(key TEXT PRIMARY KEY,value TEXT); INSERT INTO task_store_metadata VALUES('schema_version','99');").unwrap();
    assert!(TasksStore::open(&future).is_err());
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1);
}
#[test]
fn cli_migration_reconciles_manifests_and_creation_selects_one_task() {
    let (root, mut store, milestone) = fixture();
    let invoke = |action: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "migrate",
                action,
                "docs/010-test.md",
            ])
            .output()
            .unwrap()
    };
    for action in ["preview", "apply", "apply", "verify"] {
        let output = invoke(action);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["mode"], action);
    }
    store
        .delete(None, Some("docs/010-test.md"), true, 0)
        .unwrap();
    let created = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Create,
            milestone_ref: Some("docs/010-test.md".into()),
            task_ref: Some("first".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(created["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(store.list(None, "all", None).unwrap().len(), 1);
    assert_eq!(
        store.list(None, "all", None).unwrap()[0].task_id,
        milestone.task_id(&milestone.tasks[0])
    );
    assert!(!invoke("verify").status.success());
    assert!(invoke("apply").status.success());
    assert!(invoke("verify").status.success());
    root.write("docs/milestones/archive/010-test.md", SPEC);
    for action in ["preview", "apply", "verify"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", root.0.to_str().unwrap(), "migrate", action])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["manifests"].as_array().unwrap().len(), 1);
    }
}
fn milestone_cli(root: &ScopedWorkspace, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", root.0.to_str().unwrap(), "milestone"])
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn milestone_init_scaffolds_numbered_planned_and_active_documents() {
    use std::io::Write;
    let root = ScopedWorkspace::new("forge_milestone_init");
    root.write("docs/milestones/010-first.md", "---\nid: m-first\n---\n");
    root.write(
        "docs/milestones/archive/1050-prior.md",
        "---\nid: m-prior\n---\n",
    );
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "milestone",
            "init",
            "--slug",
            "next-work",
            "--title",
            "Next work",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"A concrete outcome.\n\n### task: first\n\n```yaml\ntask_ref: first\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/]\n```\n\nTask notes: preserve this detail.\n\n### task: second\n\n```yaml\ntask_ref: second\ntarget: Second task\nproof_policy: seam-test-first\nscope: [src/]\n```\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = root.0.join("docs/milestones/1060-next-work.md");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("status: planned"));
    assert!(!text.contains("started_at:"));
    assert!(text.contains("A concrete outcome."));
    let note = text.find("Task notes: preserve this detail.").unwrap();
    assert!(text.find("task_ref: first").unwrap() < note);
    assert!(note < text.find("task_ref: second").unwrap());
    let output = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "011",
            "--slug",
            "active-work",
            "--title",
            "Active work",
            "--active",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(root.0.join("docs/milestones/011-active-work.md")).unwrap();
    assert!(text.contains("status: active"));
    let started = text
        .lines()
        .find_map(|line| line.strip_prefix("started_at: "))
        .unwrap();
    let parsed = chrono::DateTime::parse_from_rfc3339(started).unwrap();
    assert!(
        (chrono::Utc::now() - parsed.with_timezone(&chrono::Utc))
            .num_minutes()
            .abs()
            < 2
    );
    let output = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "012",
            "--slug",
            "described",
            "--title",
            "Described",
            "--desc",
            "Explicit description",
            "--depends-on",
            "m-first",
        ],
    );
    assert!(output.status.success());
    let text = std::fs::read_to_string(root.0.join("docs/milestones/012-described.md")).unwrap();
    assert!(text.contains("Explicit description"));
    assert!(text.contains("m-first"));
}
#[test]
fn milestone_init_imports_plan_metadata_and_rejects_duplicate_number() {
    let root = ScopedWorkspace::new("forge_milestone_plan");
    root.write("docs/plans/proposal.md", "---\ntitle: Planned title\ndoc_type: plan\ndepends_on: [m-prior]\n---\n# Planned title\n\nA scoped purpose.\n\n## Delivery notes\n\nA separate implementation note.\n");
    let invoke = || {
        milestone_cli(
            &root,
            &[
                "init",
                "--num",
                "025",
                "--slug",
                "planned-title",
                "--plan",
                "docs/plans/proposal.md",
            ],
        )
    };
    let output = invoke();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response["sync"].as_str().unwrap().contains("task sync"));
    assert!(response["claim"].as_str().unwrap().contains("task claim"));
    let text =
        std::fs::read_to_string(root.0.join("docs/milestones/025-planned-title.md")).unwrap();
    assert!(text.contains("title: Planned title"));
    assert!(text.contains("m-prior"));
    assert!(text.contains("A scoped purpose."));
    assert!(text.contains("## Source plan notes"));
    assert!(text.find("## Source plan notes").unwrap() > text.find("A scoped purpose.").unwrap());
    assert!(text.contains("A separate implementation note."));
    assert!(!invoke().status.success());
    let same_id = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "035",
            "--slug",
            "planned-title",
            "--title",
            "Another title",
        ],
    );
    assert!(!same_id.status.success());
    root.write(
        "docs/milestones/archive/18446744073709551616-legacy.md",
        "---\nid: m-legacy\n---\n",
    );
    let overflow = milestone_cli(&root, &["init", "--slug", "later", "--title", "Later"]);
    assert!(!overflow.status.success());
}
#[test]
fn milestone_list_and_show_report_scoped_documents_and_sqlite_progress() {
    let root = ScopedWorkspace::new("forge_milestone_inspection");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let queued = "---\nid: m-queued\ntitle: Queued work\ndoc_type: contract\nstatus: planned\n---\n# Queued work\n### task: draft\n```yaml\ntask_ref: draft\ntarget: Draft work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let active = "---\nid: m-live\ntitle: Live work\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ndepends_on: [m-queued]\ninvariants: [Keep receipts]\n---\n# Live work\n## Outcome and purpose\nDeliver visible progress.\n### task: first\n```yaml\ntask_ref: first\ntarget: First result\nproof_policy: seam-test-first\nscope: [src/]\n```\nImplementation steps for first.\n### task: second\n```yaml\ntask_ref: second\ntarget: Second result\nproof_policy: seam-test-first\nscope: [src/]\n```\nDelivery notes for second.\n";
    let archived = "---\nid: m-prior\ntitle: Prior work\ndoc_type: contract\nstatus: completed\nstarted_at: 2026-09-01T10:00:00Z\n---\n# Prior work\n### task: shipped\n```yaml\ntask_ref: shipped\ntarget: Shipped result\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    for (path, source) in [
        ("docs/milestones/010-queued.md", queued),
        ("docs/milestones/011-live.md", active),
        ("docs/milestones/archive/020-prior.md", archived),
    ] {
        root.write(path, source);
    }
    root.write(
        "docs/milestones/README.md",
        "---\ntitle: Milestone guide\ndoc_type: guide\n---\n# Milestone guide\n",
    );
    root.write(
        "docs/milestones/archive/README.md",
        "---\ntitle: Archive guide\ndoc_type: guide\n---\n# Archive guide\n",
    );
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    for (path, source) in [
        ("docs/milestones/010-queued.md", queued),
        ("docs/milestones/011-live.md", active),
        ("docs/milestones/archive/020-prior.md", archived),
    ] {
        store
            .sync(
                &Milestone::parse(source, "forge-mcp").unwrap(),
                path,
                &root.0,
            )
            .unwrap();
    }
    root.write("docs/milestones/010-queued.md", &format!("{queued}### task: file-only\n```yaml\ntask_ref: file-only\ntarget: File-declared completion\nproof_policy: seam-test-first\nscope: [src/]\nstatus: completed\n```\n"));
    root.write(
        "docs/milestones/archive/020-prior.md",
        &archived.replace("scope: [src/]", "scope: [src/]\nstatus: completed"),
    );
    for task_ref in ["first", "shipped"] {
        let mut task = store
            .list(None, "all", None)
            .unwrap()
            .into_iter()
            .find(|t| t.task_id.ends_with(&format!(":{task_ref}")))
            .unwrap();
        task.status = "completed".into();
        store
            .connection
            .execute(
                "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
                rusqlite::params![serde_json::to_string(&task).unwrap(), task.task_id],
            )
            .unwrap();
    }
    let run = |args: &[&str]| {
        let output = milestone_cli(&root, args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let listed = run(&["list"]);
    let rows = listed["milestones"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for heading in ["ID", "title", "status", "started_at", "completion"] {
        assert!(listed["table"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains(&heading.to_ascii_lowercase()));
    }
    let queued_row = rows.iter().find(|row| row["id"] == "m-queued").unwrap();
    assert_eq!(
        (
            queued_row["title"].as_str(),
            queued_row["status"].as_str(),
            queued_row["started_at"].as_str(),
            queued_row["completion"].as_str()
        ),
        (Some("Queued work"), Some("planned"), Some("-"), Some("0/2"))
    );
    assert!(queued_row["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "file-only" && task["status"] == "unsynced"));
    let live = rows.iter().find(|row| row["id"] == "m-live").unwrap();
    assert!(listed["table"]
        .as_str()
        .unwrap()
        .lines()
        .any(|line| line.contains("m-live") && line.contains("1/2")));
    assert_eq!(
        (live["status"].as_str(), live["completion"].as_str()),
        (Some("active"), Some("1/2"))
    );
    assert_eq!(live["tasks"].as_array().unwrap().len(), 2);
    assert!(live["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "first" && task["status"] == "completed"));
    assert!(live["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "second" && task["status"] == "ready"));
    for args in [&["list", "--archive"][..], &["list", "--status", "all"][..]] {
        assert_eq!(run(args)["milestones"].as_array().unwrap().len(), 3);
    }
    let completed = run(&["list", "--status", "completed"]);
    assert_eq!(completed["milestones"][0]["id"], "m-prior");
    assert_eq!(completed["milestones"][0]["completion"], "1/1");
    let shown = run(&["show", "m-live"]);
    assert!(shown.get("full_document").is_none());
    assert_eq!(
        (
            shown["id"].as_str(),
            shown["title"].as_str(),
            shown["status"].as_str()
        ),
        (Some("m-live"), Some("Live work"), Some("active"))
    );
    assert_eq!(shown["frontmatter"]["id"], "m-live");
    assert_eq!(shown["depends_on"][0], "m-queued");
    assert_eq!(shown["invariants"][0], "Keep receipts");
    assert!(shown["outcome"]
        .as_str()
        .unwrap()
        .contains("Deliver visible progress."));
    assert_eq!(shown["tasks"].as_array().unwrap().len(), 2);
    assert!(shown["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "first" && task["target"] == "First result"));
    let by_prefix = run(&["show", "011", "--full"]);
    assert!(by_prefix.get("full_document").is_some());
    assert_eq!(by_prefix["id"], shown["id"]);
    assert!(by_prefix["full_document"]
        .as_str()
        .unwrap()
        .contains("Implementation steps for first."));
    assert!(by_prefix["full_document"]
        .as_str()
        .unwrap()
        .contains("Delivery notes for second."));
    let by_path = run(&["show", "docs/milestones/011-live.md"]);
    assert_eq!(by_path["id"], shown["id"]);
    assert_eq!(run(&["show", "020"])["status"], "completed");
    let override_source = "---\nid: m-override\ntitle: Override work\ndoc_type: contract\nstatus: active\nrepository: alternate\nproject: special\n---\n# Override work\n### task: delivered\n```yaml\ntask_ref: delivered\ntarget: Delivered override\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/025-override.md", override_source);
    let mut alternate =
        TasksStore::open_project(&root.0.join(".forge/tasks.sqlite"), "alternate", "special")
            .unwrap();
    alternate
        .sync(
            &Milestone::parse_with_identity(override_source, "alternate", "special").unwrap(),
            "docs/milestones/025-override.md",
            &root.0,
        )
        .unwrap();
    let mut delivered = alternate.list(None, "all", None).unwrap().pop().unwrap();
    delivered.status = "completed".into();
    alternate
        .connection
        .execute(
            "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
            rusqlite::params![
                serde_json::to_string(&delivered).unwrap(),
                delivered.task_id
            ],
        )
        .unwrap();
    let overridden = run(&["list"]);
    let override_row = overridden["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "m-override")
        .unwrap();
    assert_eq!(override_row["completion"], "1/1");
    assert_eq!(override_row["tasks"][0]["status"], "completed");
    root.write("docs/milestones/archive/030-broken.md", "---\nid: m-broken\ntitle: Broken archive\ndoc_type: contract\nstatus: completed\n---\n# Broken archive\n```yaml\ntask_ref: [invalid\n```\n");
    assert_eq!(run(&["list"])["milestones"].as_array().unwrap().len(), 3);
    assert_eq!(run(&["show", "011"])["id"], "m-live");
    root.write(
        "docs/milestones/032-duplicate.md",
        &active
            .replace("m-live", "m-duplicate")
            .replace("Live work", "Duplicate active"),
    );
    root.write(
        "docs/milestones/archive/032-duplicate.md",
        &archived
            .replace("m-prior", "m-duplicate-archived")
            .replace("Prior work", "Duplicate archived"),
    );
    assert_eq!(run(&["show", "032"])["id"], "m-duplicate");
    assert_eq!(
        run(&["show", "docs/milestones/archive/032-duplicate.md"])["id"],
        "m-duplicate-archived"
    );
    root.write(
        "docs/milestones/031-malformed-frontmatter.md",
        "---\nid: m-invalid\ntitle: Invalid frontmatter\ndoc_type: contract\nbroken: [\n---\n",
    );
    let invalid_id = milestone_cli(&root, &["show", "m-does-not-exist"]);
    let invalid_id_output = format!(
        "{}{}",
        String::from_utf8_lossy(&invalid_id.stdout),
        String::from_utf8_lossy(&invalid_id.stderr)
    );
    assert!(
        invalid_id_output.contains("failed to read milestone id from"),
        "milestone ID lookup must preserve frontmatter parse errors: {invalid_id_output}"
    );
}
#[test]
fn milestone_handoff_requires_completed_tasks_and_archives_typed_receipt() {
    let root = ScopedWorkspace::new("forge_milestone_handoff");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let source = "---\nid: m-close\ntitle: Close work\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Close work\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-close.md", source);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(
            &Milestone::parse(source, "forge-mcp").unwrap(),
            "docs/milestones/010-close.md",
            &root.0,
        )
        .unwrap();
    let args = [
        "handoff",
        "m-close",
        "--commit",
        "0123456789abcdef0123456789abcdef01234567",
        "--verification-command",
        "cargo test --all-targets",
        "--tests-passed",
        "7",
        "--tests-failed",
        "0",
    ];
    let rejected = milestone_cli(&root, &args);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("finish"));
    assert!(root.0.join("docs/milestones/010-close.md").exists());
    let mut task = store.list(None, "all", None).unwrap().pop().unwrap();
    task.status = "completed".into();
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
            rusqlite::params![serde_json::to_string(&task).unwrap(), task.task_id],
        )
        .unwrap();
    let completed = milestone_cli(&root, &args);
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stderr)
    );
    assert!(!root.0.join("docs/milestones/010-close.md").exists());
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        reopened.list(None, "all", None).unwrap()[0].milestone_ref,
        "docs/milestones/archive/010-close.md"
    );
    let archived =
        std::fs::read_to_string(root.0.join("docs/milestones/archive/010-close.md")).unwrap();
    let header = archived
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    assert_eq!(meta["status"].as_str(), Some("completed"));
    assert_eq!(
        meta["handoff"]["commit"].as_str(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    let started =
        chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
    let ended =
        chrono::DateTime::parse_from_rfc3339(meta["handoff"]["completed_at"].as_str().unwrap())
            .unwrap();
    let minutes = (ended - started).num_minutes();
    assert_eq!(
        meta["handoff"]["duration"].as_str().unwrap(),
        format!("{}h {}m", minutes / 60, minutes % 60)
    );
    let verification = &meta["handoff"]["verification"];
    assert_eq!(
        verification["command"].as_str(),
        Some("cargo test --all-targets")
    );
    assert_eq!(verification["status"].as_str(), Some("passed"));
    assert_eq!(verification["tests_passed"].as_i64(), Some(7));
    assert_eq!(verification["tests_failed"].as_i64(), Some(0));
    let fallback = ScopedWorkspace::new("forge_handoff_claim_fallback");
    fallback.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    fallback.write("src/lib.rs", "pub fn fallback() {}\n");
    let source = "---\nid: m-fallback\ntitle: Fallback work\ndoc_type: contract\nstatus: active\n---\n# Fallback work\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish fallback\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    fallback.write("docs/milestones/020-fallback.md", source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let mut fallback_store = TasksStore::open(&fallback.0.join(".forge/tasks.sqlite")).unwrap();
    fallback_store
        .sync(&milestone, "docs/milestones/020-fallback.md", &fallback.0)
        .unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let mut claimed = fallback_store
        .claim(&id, "contract", "worker", fallback.0.to_str().unwrap())
        .unwrap();
    let claim_time = fallback_store
        .earliest_claim("forge-mcp/forge-mcp/m-fallback:")
        .unwrap()
        .unwrap();
    claimed.status = "completed".into();
    fallback_store
        .connection
        .execute(
            "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
            rusqlite::params![serde_json::to_string(&claimed).unwrap(), id],
        )
        .unwrap();
    let result = milestone_cli(
        &fallback,
        &[
            "handoff",
            "m-fallback",
            "--commit",
            "0123456789abcdef0123456789abcdef01234567",
            "--verification-command",
            "cargo test",
            "--tests-passed",
            "1",
            "--tests-failed",
            "0",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = std::fs::read_to_string(fallback.0.join("docs/milestones/archive/020-fallback.md"))
        .unwrap();
    let header = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    let started =
        chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
    let ended =
        chrono::DateTime::parse_from_rfc3339(meta["handoff"]["completed_at"].as_str().unwrap())
            .unwrap();
    let minutes = (ended - started).num_minutes();
    assert_eq!(started.timestamp(), claim_time);
    assert_eq!(
        meta["handoff"]["duration"].as_str().unwrap(),
        format!("{}h {}m", minutes / 60, minutes % 60)
    );
}

#[test]
fn terminal_task_delivery_rolls_up_durable_context_and_prunes_blackboard() {
    let root = ScopedWorkspace::new("forge_task_context_rollup");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let source = SPEC.replace(
        "task_ref: first\n",
        "task_ref: first\ninvariants: [first-rule]\n",
    );
    let milestone_ref = "docs/milestones/010-test.md";
    root.write(milestone_ref, &source);
    let milestone = Milestone::parse(&source, "forge-mcp").unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, milestone_ref, &root.0).unwrap();

    for (index, stage) in GATES.iter().enumerate() {
        let worker = match index {
            2 => "independent-reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let claimed = store
            .claim(&first, stage, worker, root.0.to_str().unwrap())
            .unwrap();
        if index == 3 {
            store
                .blackboard_post(
                    &first,
                    "architect",
                    "architectural_notes",
                    "Keep durable task outcomes in the milestone receipt.",
                )
                .unwrap();
            store
                .blackboard_post(&first, "builder", "debug", "temporary trace")
                .unwrap();
            store
                .blackboard_post(&second, "sibling", "debug", "other task context")
                .unwrap();
            root.write(
                milestone_ref,
                &source.replace("target: Deliver first", "target: Changed without admission"),
            );
            assert!(store
                .submit(&first, stage, &evidence(&claimed), "pass", None)
                .is_err());
            assert_eq!(store.inspect(&first).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&first, None, None).unwrap().len(), 2);
            root.write(milestone_ref, &source);
        }
        let result = store
            .submit(&first, stage, &evidence(&claimed), "pass", None)
            .unwrap();
        if index == 3 {
            assert_eq!(result.status, "completed");
        }
    }

    let delivered = store.inspect(&first).unwrap();
    let receipt = serde_json::to_value(delivered.receipt.unwrap()).unwrap();
    assert_eq!(
        receipt["commit"],
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(
        receipt["rollup"]["verified_invariants"],
        json!(["isolated", "first-rule"])
    );
    assert_eq!(
        receipt["rollup"]["architectural_notes"],
        json!(["Keep durable task outcomes in the milestone receipt."])
    );
    assert_eq!(receipt["rollup"]["review_summary"]["decision"], "pass");
    for contour in contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS {
        assert_eq!(
            receipt["rollup"]["review_summary"]["contours"][contour],
            "accepted"
        );
    }
    let written = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
    let parsed = Milestone::parse(&written, "forge-mcp").unwrap();
    let persisted = parsed
        .tasks
        .iter()
        .find(|task| task.task_ref == "first")
        .unwrap();
    assert_eq!(persisted.status.as_deref(), Some("completed"));
    assert_eq!(
        serde_json::to_value(persisted.receipt.as_ref().unwrap()).unwrap(),
        receipt
    );
    assert!(store
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 1);
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let closed = reopened
        .blackboard_post(&first, "late-worker", "debug", "after delivery")
        .unwrap_err();
    assert_eq!(closed.to_string(), "TASK_TERMINAL");
    assert!(reopened
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());

    let mut accepted_build = json!(null);
    let mut accepted_review = json!(null);
    for (index, stage) in GATES.iter().enumerate() {
        let worker = match index {
            2 => "second-independent-reviewer",
            3 => "second-delivery-reviewer",
            _ => "second-builder",
        };
        let claimed = store
            .claim(&second, stage, worker, root.0.to_str().unwrap())
            .unwrap();
        let proof = evidence(&claimed);
        if index == 1 {
            accepted_build = proof.proof.clone();
        }
        if index == 2 {
            accepted_review = proof.proof.clone();
        }
        if index == 3 {
            store
                .blackboard_post(
                    &second,
                    "architect",
                    "architectural_notes",
                    "Retain retry notes.",
                )
                .unwrap();
            let current = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
            let parsed = Milestone::parse(&current, "forge-mcp").unwrap();
            let mut recovered = Receipt {
                commit: proof.commit.clone(),
                contract_revision: 1,
                passed_at: "2026-10-02T00:00:00Z".into(),
                evidence: accepted_build.clone(),
                review: accepted_review.clone(),
                decision: "pass".into(),
                rollup: Some(ReceiptRollup {
                    verified_invariants: vec!["isolated".into()],
                    architectural_notes: vec!["Retain retry notes.".into()],
                    review_summary: ReviewSummary {
                        decision: "pass".into(),
                        contours: contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
                            .into_iter()
                            .map(|name| (name.into(), "accepted".into()))
                            .collect(),
                    },
                }),
            };
            let original_block = "task_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n";
            assert!(current.contains(original_block));
            let recovery_text = |receipt: &Receipt| {
                let mut completed = parsed.tasks[1].clone();
                completed.status = Some("completed".into());
                completed.receipt = Some(receipt.clone());
                current.replace(original_block, &serde_yaml::to_string(&completed).unwrap())
            };
            recovered.rollup.as_mut().unwrap().architectural_notes[0] = "stale note".into();
            root.write(milestone_ref, &recovery_text(&recovered));
            assert!(store.submit(&second, stage, &proof, "pass", None).is_err());
            assert_eq!(store.inspect(&second).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 2);
            recovered.rollup.as_mut().unwrap().architectural_notes[0] =
                "Retain retry notes.".into();
            root.write(milestone_ref, &recovery_text(&recovered));
        }
        let completed = store.submit(&second, stage, &proof, "pass", None).unwrap();
        if index == 3 {
            assert_eq!(completed.status, "completed");
            assert_eq!(
                completed.receipt.as_ref().unwrap().passed_at,
                "2026-10-02T00:00:00Z"
            );
            assert!(store
                .blackboard_read(&second, None, None)
                .unwrap()
                .is_empty());
        }
    }
}
#[test]
fn first_cli_task_claim_activates_planned_milestone_with_started_at() {
    let root = ScopedWorkspace::new("forge_milestone_claim_start");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("src/lib.rs", "pub fn task() {}\n");
    let source = "---\nid: m-start\ntitle: Start work\ndoc_type: contract\nstatus: planned\n---\n# Start work\n### task: begin\n```yaml\ntask_ref: begin\ntarget: Begin work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-start.md", source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-start.md", &root.0)
        .unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "claim",
            &id,
            "--stage",
            "contract",
            "--worker",
            "first",
            "--worktree",
            root.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(root.0.join("docs/milestones/010-start.md")).unwrap();
    let header = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    assert_eq!(meta["status"].as_str(), Some("active"));
    let started =
        chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
    assert!(
        (chrono::Utc::now() - started.with_timezone(&chrono::Utc))
            .num_minutes()
            .abs()
            < 2
    );
}

#[test]
fn deferred_final_task_context_survives_delivery_and_prunes_blackboard() {
    let root = ScopedWorkspace::new("forge_milestone_012_e2e");
    let milestone_ref = "docs/milestones/012-context.md";
    let source = "---\nid: m-context\ntitle: Context retention\ndoc_type: contract\nstatus: planned\ninvariants: [durable-context]\n---\n# Context retention\n### task: implement\n```yaml\ntask_ref: implement\ntarget: Deliver context retention\nagent_type: gpt-6-sol\nproof_policy: seam-test-first\nscope: [src/]\ninvariants: [reviewed]\n```\n";
    root.write("src/lib.rs", "pub fn context() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(milestone_ref, source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":milestone_ref})).unwrap(),
    )
    .unwrap();

    let blackboard = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", root.0.to_str().unwrap(), "task", "blackboard"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let inspect = || {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"inspect","task_id":task_id})).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(inspect()["spec"]["agent_type"], "gpt-6-sol");
    let stages = ["contract/v1", "build/v1", "review/v1", "deliver/v1"];

    for (index, stage) in stages.iter().enumerate() {
        let before = inspect();
        assert_eq!(before["workflow_guidance"]["active_stage"], *stage);
        assert_eq!(before["workflow_guidance"]["agent_type"], "gpt-6-sol");
        assert!(before["workflow_guidance"]["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()));
        let worker = if index >= 2 {
            "independent-reviewer"
        } else {
            "builder"
        };
        if index == stages.len() - 1 {
            let previous_name = tasks::claim(
                &root.0,
                tasks::Claim {
                    task_id: task_id.clone(),
                    stage: "handoff/v1".into(),
                    worker_id: worker.into(),
                    worktree: root.0.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(previous_name.to_string().contains("TASK_STAGE_INVALID"));
            assert_eq!(inspect()["status"], "ready");
        }
        let claimed = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.clone(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(claimed["workflow_guidance"], before["workflow_guidance"]);
        if index == 2 {
            blackboard(&[
                "post",
                &task_id,
                "--author",
                "architect",
                "--topic",
                "architectural_notes",
                "--payload",
                "Keep task context in the milestone receipt.",
            ]);
            blackboard(&[
                "post",
                &task_id,
                "--author",
                "builder",
                "--topic",
                "build_notes",
                "--payload",
                "The direct JSON proof passed SQLite verification.",
            ]);
            let messages = blackboard(&["read", &task_id]);
            assert_eq!(messages["messages"].as_array().unwrap().len(), 2);
            assert_eq!(messages["messages"][0]["author"], "builder");
            assert_eq!(messages["messages"][1]["author"], "architect");
        }
        let stored = tasks::store(&root.0).unwrap().inspect(&task_id).unwrap();
        if index == stages.len() - 1 {
            let mut previous_proof = evidence(&stored);
            previous_proof.stage = "handoff/v1".into();
            let previous_submit: tasks::Submit = serde_json::from_value(json!({
                "task_id": task_id,
                "stage": "handoff/v1",
                "action": "pass",
                "evidence": serde_json::to_value(previous_proof).unwrap()
            }))
            .unwrap();
            let rejected = tasks::submit(&root.0, previous_submit).unwrap_err();
            assert!(rejected.to_string().contains("TASK_STAGE_INVALID"));
            assert_eq!(inspect()["status"], "in_progress");
        }
        let submitted = tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id":task_id,
                "stage":stage,
                "action":"pass",
                "evidence":serde_json::to_value(evidence(&stored)).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        if index == stages.len() - 1 {
            assert_eq!(submitted["status"], "completed");
        }
    }

    let written = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
    let parsed = Milestone::parse(&written, "forge-mcp").unwrap();
    let completed = &parsed.tasks[0];
    assert_eq!(completed.status.as_deref(), Some("completed"));
    assert_eq!(completed.agent_type.as_deref(), Some("gpt-6-sol"));
    let receipt = completed.receipt.as_ref().unwrap();
    assert_eq!(receipt.commit, "0123456789abcdef0123456789abcdef01234567");
    assert!(receipt.evidence.get("test_proof").is_some());
    assert!(receipt.review.get("review_proof").is_some());
    let rollup = receipt.rollup.as_ref().unwrap();
    assert_eq!(rollup.verified_invariants, ["durable-context", "reviewed"]);
    assert_eq!(
        rollup.architectural_notes,
        ["Keep task context in the milestone receipt."]
    );
    assert_eq!(rollup.review_summary.decision, "pass");
    assert!(blackboard(&["read", &task_id])["messages"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect(&task_id)
            .unwrap()
            .status,
        "completed"
    );
}

#[test]
fn task_subtasks_lifecycle_management_and_digest_independence() {
    let spec = "---\nid: m-subtasks\ntitle: Subtasks Test\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: core-feature\ntarget: Deliver core feature\nproof_policy: seam-test-first\nscope: [src/]\nsubtasks:\n  - subtask_ref: sub-1\n    title: Initial discovery\n    status: in_progress\n    evidence: Found relevant files\n```\n";
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let task = &milestone.tasks[0];
    assert_eq!(task.subtasks.len(), 1);
    assert_eq!(task.subtasks[0].subtask_ref, "sub-1");
    assert_eq!(task.subtasks[0].title, "Initial discovery");
    assert_eq!(task.subtasks[0].status, "in_progress");
    assert_eq!(
        task.subtasks[0].evidence.as_deref(),
        Some("Found relevant files")
    );

    // Digest independence: subtask updates or additions do not invalidate the parent task's contract revision or SHA256 digest
    let base_digest = milestone.digest(task).unwrap();
    let mut modified_task = task.clone();
    modified_task
        .subtasks
        .push(contextunity_forge_mcp::core::tasks::SubtaskSpec {
            subtask_ref: "sub-2".into(),
            title: "Additional spike".into(),
            status: "completed".into(),
            evidence: Some("Tests verified".into()),
        });
    modified_task.subtasks[0].status = "completed".into();
    let modified_digest = milestone.digest(&modified_task).unwrap();
    assert_eq!(
        base_digest, modified_digest,
        "Subtasks must not alter contract digest"
    );

    let root = ScopedWorkspace::new("forge_subtasks");
    root.write("src/lib.rs", "pub fn core() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-subtasks.md", spec);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();

    let task_id = "forge-mcp/forge-mcp/m-subtasks:core-feature";
    let loaded = store.inspect(task_id).unwrap();
    assert_eq!(loaded.spec.subtasks.len(), 1);
    assert_eq!(loaded.spec.subtasks[0].subtask_ref, "sub-1");

    let subtasks_table = store.subtask_list(task_id).unwrap();
    assert_eq!(subtasks_table.len(), 1);
    assert_eq!(subtasks_table[0].subtask_ref, "sub-1");
    assert_eq!(subtasks_table[0].status, "in_progress");

    // Add subtask via store
    let added = store
        .subtask_add(task_id, "sub-2", "Implement edge case")
        .unwrap();
    assert_eq!(added.subtask_ref, "sub-2");
    assert_eq!(added.status, "pending");

    // Duplicate subtask ref rejection
    let dup_err = store
        .subtask_add(task_id, "sub-2", "Duplicate")
        .unwrap_err();
    assert!(dup_err.to_string().contains("TASK_SUBTASK_DUPLICATE"));

    // Update subtask
    let updated = store
        .subtask_update(task_id, "sub-2", "completed", Some("Added unit test"))
        .unwrap();
    assert_eq!(updated.status, "completed");
    assert_eq!(updated.evidence.as_deref(), Some("Added unit test"));

    // Invalid status rejection
    let invalid_err = store
        .subtask_update(task_id, "sub-2", "unknown_status", None)
        .unwrap_err();
    assert!(invalid_err
        .to_string()
        .contains("TASK_SUBTASK_STATUS_INVALID"));

    // Test engine manage commands
    drop(store);

    // List via engine
    let list_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskList,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let subtask_list = list_res["subtasks"].as_array().unwrap();
    assert_eq!(subtask_list.len(), 2);

    // Add via engine
    let add_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskAdd,
            task_id: Some(task_id.into()),
            subtask_ref: Some("sub-3".into()),
            title: Some("Documentation update".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(add_res["subtask"]["subtask_ref"], "sub-3");
    assert_eq!(add_res["subtask"]["status"], "pending");

    // Update via engine
    let update_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(task_id.into()),
            subtask_ref: Some("sub-3".into()),
            subtask_status: Some("in_progress".into()),
            evidence: Some("Drafted changes".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(update_res["subtask"]["status"], "in_progress");
    assert_eq!(update_res["subtask"]["evidence"], "Drafted changes");

    // Inspect task includes subtasks
    let inspected = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Inspect,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(inspected["subtasks"].as_array().unwrap().len(), 3);

    // List tasks includes subtasks
    let tasks_list = tasks::list(
        &root.0,
        serde_json::from_value(json!({"status": "all"})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        tasks_list["tasks"][0]["subtasks"].as_array().unwrap().len(),
        3
    );

    // Repeated sync: verify that re-syncing from milestone doc does NOT overwrite live SQLite progress
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();
    let re_inspected = store.inspect(task_id).unwrap();
    assert_eq!(re_inspected.spec.subtasks.len(), 3);
    let sub2 = re_inspected
        .spec
        .subtasks
        .iter()
        .find(|s| s.subtask_ref == "sub-2")
        .unwrap();
    assert_eq!(sub2.status, "completed");
    assert_eq!(sub2.evidence.as_deref(), Some("Added unit test"));
    let sub3 = re_inspected
        .spec
        .subtasks
        .iter()
        .find(|s| s.subtask_ref == "sub-3")
        .unwrap();
    assert_eq!(sub3.status, "in_progress");
    drop(store);

    // Deliver task through all 4 gates and prove subtasks are rendered into durable milestone markdown
    let stages = ["contract/v1", "build/v1", "review/v1", "deliver/v1"];
    for (index, stage) in stages.iter().enumerate() {
        let worker = if index >= 2 { "reviewer" } else { "builder" };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.to_string(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
        tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": serde_json::to_value(evidence(&stored)).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
    }

    // Milestone Markdown now contains durable subtasks block
    let updated_doc = std::fs::read_to_string(root.0.join("docs/010-subtasks.md")).unwrap();
    let delivered_milestone = Milestone::parse(&updated_doc, "forge-mcp").unwrap();
    let delivered_task = &delivered_milestone.tasks[0];
    assert_eq!(delivered_task.status.as_deref(), Some("completed"));
    assert_eq!(delivered_task.subtasks.len(), 3);
    assert_eq!(delivered_task.subtasks[1].subtask_ref, "sub-2");
    assert_eq!(delivered_task.subtasks[1].status, "completed");
    assert_eq!(
        delivered_task.subtasks[1].evidence.as_deref(),
        Some("Added unit test")
    );

    // Post-delivery immutability: modifying subtasks on a completed task fails with TASK_TERMINAL
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let add_post_err = store
        .subtask_add(task_id, "sub-4", "Late addition")
        .unwrap_err();
    assert!(add_post_err.to_string().contains("TASK_TERMINAL"));
    let update_post_err = store
        .subtask_update(task_id, "sub-2", "pending", None)
        .unwrap_err();
    assert!(update_post_err.to_string().contains("TASK_TERMINAL"));

    // Reopening the completed task via tasks::reset clears terminal receipt from markdown and restores ready state in SQLite
    let reopened_task = tasks::reset(&root.0, task_id).unwrap();
    assert_eq!(reopened_task.status, "ready");
    assert_eq!(reopened_task.gate, 0);
    assert!(reopened_task.receipt.is_none());
    assert!(reopened_task.completed_at.is_none());

    // Markdown file reflects the reopened state (status: completed and receipt removed, subtasks preserved)
    let reopened_doc = std::fs::read_to_string(root.0.join("docs/010-subtasks.md")).unwrap();
    let reopened_milestone = Milestone::parse(&reopened_doc, "forge-mcp").unwrap();
    assert_eq!(reopened_milestone.tasks[0].status.as_deref(), None);
    assert!(reopened_milestone.tasks[0].receipt.is_none());
    assert_eq!(reopened_milestone.tasks[0].subtasks.len(), 3);

    // After reopening, adding and updating subtasks works cleanly
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let sub4 = store
        .subtask_add(task_id, "sub-4", "Audit findings subtask")
        .unwrap();
    assert_eq!(sub4.subtask_ref, "sub-4");
    assert_eq!(sub4.status, "pending");

    let sub4_updated = store
        .subtask_update(task_id, "sub-4", "completed", Some("Audit verified"))
        .unwrap();
    assert_eq!(sub4_updated.status, "completed");
    assert_eq!(sub4_updated.evidence.as_deref(), Some("Audit verified"));

    // Syncing after reopen maintains the ready state without errors
    let synced = store
        .sync(&reopened_milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();
    assert_eq!(synced[0].status, "ready");

    // Contract re-admission: bumping contract_revision in milestone markdown syncs cleanly even if completed
    let readmitted_doc = reopened_doc.replace(
        "proof_policy: seam-test-first",
        "contract_revision: 2\nproof_policy: seam-test-first",
    );
    let readmitted_milestone = Milestone::parse(&readmitted_doc, "forge-mcp").unwrap();
    let readmitted = store
        .sync(&readmitted_milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();
    assert_eq!(readmitted[0].status, "ready");
    assert_eq!(readmitted[0].contract_revision, 2);
}

#[test]
fn direct_proof_and_deferred_final_test_accept_zero_exit_code_and_seam_test_first_rejects_it() {
    let spec = "---\nid: m-policy\ntitle: Policy Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/]\n```\n```yaml\ntask_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/]\n```\n```yaml\ntask_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = ScopedWorkspace::new("forge_policy");
    root.write("src/lib.rs", "pub fn policy() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-policy.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-policy.md", &root.0)
        .unwrap();

    let direct_id = "forge-mcp/forge-mcp/m-policy:direct";
    let deferred_id = "forge-mcp/forge-mcp/m-policy:deferred";
    let seam_id = "forge-mcp/forge-mcp/m-policy:seam";

    // direct-proof allows red_exit_code: 0
    store
        .claim(
            direct_id,
            "contract/v1",
            "builder",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    let d_task = store.inspect(direct_id).unwrap();
    let mut direct_evidence = evidence(&d_task);
    direct_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/core_basics/tasks.rs",
            "red_exit_code": 0
        }
    });
    let direct_submit = store.submit(direct_id, "contract/v1", &direct_evidence, "pass", None);
    assert!(
        direct_submit.is_ok(),
        "direct-proof policy must accept exit code 0"
    );

    // deferred-final-test allows red_exit_code: 0
    store
        .claim(
            deferred_id,
            "contract/v1",
            "builder",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    let def_task = store.inspect(deferred_id).unwrap();
    let mut def_evidence = evidence(&def_task);
    def_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/core_basics/tasks.rs",
            "red_exit_code": 0
        }
    });
    let def_submit = store.submit(deferred_id, "contract/v1", &def_evidence, "pass", None);
    assert!(
        def_submit.is_ok(),
        "deferred-final-test policy must accept exit code 0"
    );

    // seam-test-first rejects red_exit_code: 0
    store
        .claim(seam_id, "contract/v1", "builder", root.0.to_str().unwrap())
        .unwrap();
    let s_task = store.inspect(seam_id).unwrap();

    // seam-test-first rejects red_exit_code: 0
    let mut seam_evidence = evidence(&s_task);
    seam_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/core_basics/tasks.rs",
            "red_exit_code": 0
        }
    });
    let seam_submit_err = store
        .submit(seam_id, "contract/v1", &seam_evidence, "pass", None)
        .unwrap_err();
    assert!(seam_submit_err
        .to_string()
        .contains("TASK_EVIDENCE_INVALID: red seam test and nonzero exit code required"));

    // seam-test-first accepts nonzero exit code
    seam_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/core_basics/tasks.rs",
            "red_exit_code": 101
        }
    });
    let seam_submit = store.submit(seam_id, "contract/v1", &seam_evidence, "pass", None);
    assert!(
        seam_submit.is_ok(),
        "seam-test-first accepts nonzero exit code"
    );
}

#[test]
fn concurrent_subtask_add_preserves_all_subtasks_in_sqlite_and_descriptor() {
    let spec = "---\nid: m-concurrent\ntitle: Concurrent Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: worker\ntarget: Deliver worker\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = ScopedWorkspace::new("forge_concurrent_subtasks");
    root.write("src/lib.rs", "pub fn work() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-concurrent.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-concurrent.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/forge-mcp/m-concurrent:worker";

    let db_path = root.0.join(".forge/tasks.sqlite");
    let t1 = {
        let db_path = db_path.clone();
        let task_id = task_id.to_string();
        std::thread::spawn(move || {
            let mut s = TasksStore::open(&db_path).unwrap();
            for i in 0..10 {
                s.subtask_add(&task_id, &format!("thread1-sub-{}", i), "Thread 1 subtask")
                    .unwrap();
            }
        })
    };
    let t2 = {
        let db_path = db_path.clone();
        let task_id = task_id.to_string();
        std::thread::spawn(move || {
            let mut s = TasksStore::open(&db_path).unwrap();
            for i in 0..10 {
                s.subtask_add(&task_id, &format!("thread2-sub-{}", i), "Thread 2 subtask")
                    .unwrap();
            }
        })
    };
    t1.join().unwrap();
    t2.join().unwrap();

    let fresh = TasksStore::open(&db_path).unwrap();
    let list = fresh.subtask_list(task_id).unwrap();
    assert_eq!(
        list.len(),
        20,
        "All 20 subtasks from both threads must exist in subtask_list"
    );
    let inspected = fresh.inspect(task_id).unwrap();
    assert_eq!(
        inspected.spec.subtasks.len(),
        20,
        "All 20 subtasks must exist in tasks.descriptor"
    );
}

#[test]
fn subtask_update_preserves_evidence_when_none_provided_and_contract_bump_resets_subtasks() {
    let spec_v1 = "---\nid: m-bump\ntitle: Bump Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: feature\ntarget: Deliver feature\nproof_policy: seam-test-first\nscope: [src/]\ncontract_revision: 1\nsubtasks:\n  - subtask_ref: sub-orig\n    title: Original subtask\n    status: pending\n```\n";
    let root = ScopedWorkspace::new("forge_subtasks_bump");
    root.write("src/lib.rs", "pub fn bump() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-bump.md", spec_v1);
    let milestone_v1 = Milestone::parse(spec_v1, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone_v1, "docs/010-bump.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/forge-mcp/m-bump:feature";

    // 1. Update subtask with evidence
    let updated = store
        .subtask_update(task_id, "sub-orig", "in_progress", Some("Initial evidence"))
        .unwrap();
    assert_eq!(updated.status, "in_progress");
    assert_eq!(updated.evidence.as_deref(), Some("Initial evidence"));

    // 2. Update status only (evidence: None) - verify evidence is NOT erased
    let status_only = store
        .subtask_update(task_id, "sub-orig", "completed", None)
        .unwrap();
    assert_eq!(status_only.status, "completed");
    assert_eq!(
        status_only.evidence.as_deref(),
        Some("Initial evidence"),
        "Existing evidence must be preserved when evidence is None"
    );

    // Also check subtask_list and inspect
    let list = store.subtask_list(task_id).unwrap();
    assert_eq!(list[0].evidence.as_deref(), Some("Initial evidence"));
    let inspect = store.inspect(task_id).unwrap();
    assert_eq!(
        inspect.spec.subtasks[0].evidence.as_deref(),
        Some("Initial evidence")
    );

    // 3. Add dynamic CLI subtask
    store
        .subtask_add(task_id, "sub-dynamic", "Dynamic subtask")
        .unwrap();
    assert_eq!(store.subtask_list(task_id).unwrap().len(), 2);
    assert_eq!(store.inspect(task_id).unwrap().spec.subtasks.len(), 2);

    // 4. Contract revision bump to v2 with a new specification
    let spec_v2 = "---\nid: m-bump\ntitle: Bump Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: feature\ntarget: Deliver feature v2\nproof_policy: seam-test-first\nscope: [src/]\ncontract_revision: 2\nsubtasks:\n  - subtask_ref: sub-new\n    title: New subtask in v2\n    status: pending\n```\n";
    root.write("docs/010-bump.md", spec_v2);
    let milestone_v2 = Milestone::parse(spec_v2, "forge-mcp").unwrap();
    store
        .sync(&milestone_v2, "docs/010-bump.md", &root.0)
        .unwrap();

    // After contract bump, old subtasks (both sub-orig and sub-dynamic) must be reset,
    // and both subtask_list and inspect must return EXACTLY the same subtasks from v2!
    let list_v2 = store.subtask_list(task_id).unwrap();
    let inspect_v2 = store.inspect(task_id).unwrap();
    assert_eq!(list_v2.len(), 1);
    assert_eq!(list_v2[0].subtask_ref, "sub-new");
    assert_eq!(inspect_v2.spec.subtasks.len(), 1);
    assert_eq!(inspect_v2.spec.subtasks[0].subtask_ref, "sub-new");
}

#[test]
fn completed_tasks_sync_and_inspect_without_panic() {
    let root = ScopedWorkspace::new("forge_completed_task_sync");
    root.write("src/lib.rs", "pub fn legacy() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-completed\ntitle: Completed Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: t1\ntarget: Deliver t1\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/010-completed.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-completed.md", &root.0)
        .unwrap();

    let t1_id = "forge-mcp/forge-mcp/m-completed:t1";

    // 1. Deliver t1 cleanly through all 4 gates so it has a valid durable receipt in markdown
    for (i, stage) in GATES.iter().enumerate() {
        let worker = if i >= 2 { "reviewer" } else { "builder" };
        store
            .claim(t1_id, stage, worker, root.0.to_str().unwrap())
            .unwrap();
        let t1_task = store.inspect(t1_id).unwrap();
        store
            .submit(t1_id, stage, &evidence(&t1_task), "pass", None)
            .unwrap();
    }
    assert_eq!(store.inspect(t1_id).unwrap().status, "completed");
    assert_eq!(store.inspect(t1_id).unwrap().gate, 3);

    // 2. Re-sync from markdown with the completed task and verify gate is 3 without index panic
    let updated_doc = std::fs::read_to_string(root.0.join("docs/010-completed.md")).unwrap();
    let milestone_with_receipt = Milestone::parse(&updated_doc, "forge-mcp").unwrap();
    store
        .sync(&milestone_with_receipt, "docs/010-completed.md", &root.0)
        .unwrap();
    let resynced = store.inspect(t1_id).unwrap();
    assert_eq!(resynced.status, "completed");
    assert_eq!(resynced.gate, 3);
}

#[test]
fn milestone_and_plan_directories_configured_and_excluded_from_scanner() {
    let root = ScopedWorkspace::new("forge_multi_milestones_and_plans");
    root.write("src/lib.rs", "pub fn platform() {}\n");
    root.write(
        "docs/architecture.md",
        "# Platform Architecture\nGeneral documentation.\n",
    );
    root.write("docs/plans/platform_plan.md", "---\nid: p-platform\ntitle: Platform Plan\ndoc_type: plan\npurpose: Platform\n---\n# Platform Plan\n");
    root.write("docs/milestones/010-platform.md", "---\nid: m-platform\ntitle: Platform Milestone\ndoc_type: contract\nstatus: active\n---\n# Platform Milestone\n### task: ptask\n```yaml\ntask_ref: ptask\ntarget: Deliver ptask\nproof_policy: direct-proof\nscope: [src/]\n```\n");
    root.write("extensions/commerce/src/models.py", "# Commerce models\n");
    root.write(
        "extensions/commerce/docs/README.md",
        "# Commerce Readme\nDocumentation for commerce.\n",
    );
    root.write("extensions/commerce/docs/plans/commerce_plan.md", "---\nid: p-commerce\ntitle: Commerce Plan\ndoc_type: plan\npurpose: Commerce\n---\n# Commerce Plan\n");
    root.write("extensions/commerce/docs/milestones/010-commerce.md", "---\nid: m-commerce\ntitle: Commerce Milestone\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Commerce Milestone\n### task: ctask\n```yaml\ntask_ref: ctask\ntarget: Deliver ctask\nproof_policy: direct-proof\nscope: [extensions/commerce/]\n```\n");

    root.write(
        "forge-mcp.yaml",
        "roots: [src, extensions]\ndocs: [docs]\nmilestones:\n  - docs/milestones\n  - extensions/*/docs/milestones\nplans:\n  - docs/plans\n  - extensions/*/docs/plans\ntasks_db: .forge/tasks.sqlite\n",
    );

    // 1. Verify scanner automatically excludes plans and milestones from code/doc index
    let scan = contextunity_forge_mcp::engine::scanner::scan(&root.0, None).unwrap();
    let scanned_paths: Vec<_> = scan.entries.iter().map(|e| e.path.as_str()).collect();

    assert!(scanned_paths.contains(&"src/lib.rs"));
    assert!(scanned_paths.contains(&"docs/architecture.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/src/models.py"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/README.md"));

    assert!(!scanned_paths.contains(&"docs/plans/platform_plan.md"));
    assert!(!scanned_paths.contains(&"docs/milestones/010-platform.md"));
    assert!(!scanned_paths.contains(&"extensions/commerce/docs/plans/commerce_plan.md"));
    assert!(!scanned_paths.contains(&"extensions/commerce/docs/milestones/010-commerce.md"));

    // 2. Verify milestones::list discovers milestones across all configured milestone directories
    let listed = contextunity_forge_mcp::engine::milestones::list(&root.0, false, None).unwrap();
    let milestones = listed["milestones"].as_array().unwrap();
    assert_eq!(milestones.len(), 2);
    let ids: Vec<_> = milestones
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"m-platform"));
    assert!(ids.contains(&"m-commerce"));

    // 3. Verify milestones::show finds milestone in extension directory
    let show_commerce =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-commerce", false).unwrap();
    assert_eq!(show_commerce["id"], "m-commerce");
    assert_eq!(show_commerce["title"], "Commerce Milestone");

    // 4. Verify milestones::init with --plan automatically infers extension milestone directory
    let init_result = contextunity_forge_mcp::engine::milestones::init(
        &root.0,
        contextunity_forge_mcp::engine::milestones::Init {
            num: None,
            slug: Some("commerce-v2".into()),
            title: None,
            plan: Some("extensions/commerce/docs/plans/commerce_plan.md".into()),
            dir: None,
            desc: None,
            depends_on: vec![],
            active: false,
            stdin: String::new(),
        },
    )
    .unwrap();
    assert_eq!(init_result["id"], "m-commerce-v2");
    assert_eq!(
        init_result["path"],
        "extensions/commerce/docs/milestones/020-commerce-v2.md"
    );
    let init_v2_content = std::fs::read_to_string(
        root.0
            .join("extensions/commerce/docs/milestones/020-commerce-v2.md"),
    )
    .unwrap();
    assert!(init_v2_content.contains("project: extensions.commerce"));

    // 5. Verify task sync on extension milestone creates task in subproject namespace
    let synced = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "extensions/commerce/docs/milestones/010-commerce.md"
        }))
        .unwrap(),
    )
    .unwrap();
    let tasks_array = synced["tasks"].as_array().unwrap();
    assert_eq!(tasks_array.len(), 1);
    let task_id = tasks_array[0]["task_id"].as_str().unwrap();
    assert!(task_id.contains("/extensions.commerce/"));

    // Verify milestones::list now shows task as synced (ready, not unsynced)
    let listed_after_sync =
        contextunity_forge_mcp::engine::milestones::list(&root.0, false, None).unwrap();
    let commerce_m = listed_after_sync["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "m-commerce")
        .unwrap();
    assert_eq!(commerce_m["tasks"][0]["status"], "ready");

    // Complete the task and verify handoff succeeds and archives into extension's archive
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "extensions.commerce",
    )
    .unwrap();
    let mut task = store.list(None, "all", None).unwrap().pop().unwrap();
    task.status = "completed".into();
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
            rusqlite::params![serde_json::to_string(&task).unwrap(), task.task_id],
        )
        .unwrap();
    drop(store);

    let handoff_res = contextunity_forge_mcp::engine::milestones::handoff(
        &root.0,
        "m-commerce",
        Some("0123456789abcdef0123456789abcdef01234567"),
        "pytest extensions/commerce/tests",
        5,
        0,
    )
    .unwrap();
    assert_eq!(handoff_res["id"], "m-commerce");
    assert!(root
        .0
        .join("extensions/commerce/docs/milestones/archive/010-commerce.md")
        .exists());
    assert!(!root
        .0
        .join("extensions/commerce/docs/milestones/010-commerce.md")
        .exists());

    // 6. Verify init with --dir rejects path escaping workspace or not in configured milestones
    let escaping_init = contextunity_forge_mcp::engine::milestones::init(
        &root.0,
        contextunity_forge_mcp::engine::milestones::Init {
            num: None,
            slug: Some("escape".into()),
            title: Some("Escape".into()),
            plan: None,
            dir: Some("../outside".into()),
            desc: None,
            depends_on: vec![],
            active: false,
            stdin: String::new(),
        },
    );
    assert!(escaping_init.is_err());

    let unconfigured_init = contextunity_forge_mcp::engine::milestones::init(
        &root.0,
        contextunity_forge_mcp::engine::milestones::Init {
            num: None,
            slug: Some("unconfigured".into()),
            title: Some("Unconfigured".into()),
            plan: None,
            dir: Some("unconfigured/milestones".into()),
            desc: None,
            depends_on: vec![],
            active: false,
            stdin: String::new(),
        },
    );
    assert!(unconfigured_init.is_err());
}

#[test]
fn infer_project_from_path_supports_arbitrary_monorepo_structures_universally() {
    use contextunity_forge_mcp::core::tasks::infer_project_from_path;
    use std::path::Path;

    // Standard docs/milestones and docs/plans under arbitrary subprojects
    assert_eq!(
        infer_project_from_path(Path::new("extensions/commerce/docs/milestones/010.md")),
        Some("extensions.commerce".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/cli/docs/milestones/010.md")),
        Some("packages.cli".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("services/brain/docs/plans/010.md")),
        Some("services.brain".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("apps/web/docs/milestones/010.md")),
        Some("apps.web".into())
    );

    // Arbitrary custom folders - Forge does not dictate folder naming conventions
    assert_eq!(
        infer_project_from_path(Path::new("custom_dir/my-plugin/milestones/010.md")),
        Some("custom_dir.my-plugin".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new(
            "team_alpha/backend/analytics/docs/milestones/020.md"
        )),
        Some("team_alpha.backend.analytics".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("user_service/docs/milestones/010.md")),
        Some("user_service".into())
    );
    assert_ne!(
        infer_project_from_path(Path::new("foo.bar/docs/milestones/010.md")),
        infer_project_from_path(Path::new("foo/bar/docs/milestones/010.md"))
    );
    assert_ne!(
        infer_project_from_path(Path::new("a~d/docs/milestones/010.md")),
        infer_project_from_path(Path::new("a.b/docs/milestones/010.md"))
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/my app/docs/milestones/010.md")),
        Some("packages.my~u32~app".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/über/docs/milestones/010.md")),
        Some("packages.~u252~ber".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("microservices/billing/docs/plans/030.md")),
        Some("microservices.billing".into())
    );
    assert_ne!(
        infer_project_from_path(Path::new("packages/api/docs/milestones/010.md")),
        infer_project_from_path(Path::new("services/api/docs/milestones/010.md"))
    );
    assert_eq!(
        infer_project_from_path(Path::new("standalone_tool/milestones/010.md")),
        Some("standalone_tool".into())
    );

    // Root repository milestones and plans have no subproject
    assert_eq!(
        infer_project_from_path(Path::new("docs/milestones/010.md")),
        None
    );
    assert_eq!(
        infer_project_from_path(Path::new("milestones/010.md")),
        None
    );
    assert_eq!(
        infer_project_from_path(Path::new("docs/plans/010.md")),
        None
    );
    assert_eq!(infer_project_from_path(Path::new("plans/010.md")), None);

    let root = ScopedWorkspace::new("forge_project_hierarchy");
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\nmilestones:\n  - docs/milestones\n  - packages/api/docs/milestones\n  - services/api/docs/milestones\n  - plugins/api/docs/milestones\n");
    let task_block = "# Tasks\n```yaml\ntask_ref: shared\ntarget: Resolve one project\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    for (dir, project) in [
        ("packages/api", ""),
        ("services/api", ""),
        ("plugins/api", "project: explicit-api\n"),
    ] {
        root.write(
            &format!("{dir}/docs/milestones/010-shared.md"),
            &format!(
                "---\nid: m-shared\ntitle: Shared\ndoc_type: contract\n{project}---\n{task_block}"
            ),
        );
    }
    for (filename, project) in [
        ("010-default.md", ""),
        ("020-alpha.md", "project: alpha\n"),
        ("030-beta.md", "project: beta\n"),
    ] {
        root.write(
            &format!("docs/milestones/{filename}"),
            &format!(
                "---\nid: m-shared\ntitle: Shared\ndoc_type: contract\n{project}---\n{task_block}"
            ),
        );
    }
    let mut ids = std::collections::BTreeSet::new();
    for reference in [
        "packages/api/docs/milestones/010-shared.md",
        "services/api/docs/milestones/010-shared.md",
        "plugins/api/docs/milestones/010-shared.md",
        "docs/milestones/010-default.md",
        "docs/milestones/020-alpha.md",
        "docs/milestones/030-beta.md",
    ] {
        let response = tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(reference.to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        ids.insert(response["tasks"][0]["task_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(ids.len(), 6);
    assert!(ids.iter().any(|id| id.contains("/explicit-api/")));
    assert!(ids.iter().any(|id| id.contains("/forge-mcp/")));
    assert!(ids.iter().any(|id| id.contains("/alpha/")));
    assert!(ids.iter().any(|id| id.contains("/beta/")));
}

#[test]
fn completed_task_reopen_and_mcp_manage_action_lifecycle() {
    let spec = "---\nid: m-reopen\ntitle: Reopen Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: reopenable\ntarget: Deliver reopenable feature\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let root = ScopedWorkspace::new("forge_reopen");
    root.write("src/lib.rs", "pub fn reopenable() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-reopen.md", spec);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-reopen.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/forge-mcp/m-reopen:reopenable";

    // Advance task through all 4 gates to completed
    let stages = ["contract/v1", "build/v1", "review/v1", "deliver/v1"];
    for (index, stage) in stages.iter().enumerate() {
        let worker = if index >= 2 { "reviewer" } else { "builder" };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.into(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();

        let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
        let mut ev = evidence(&stored);
        if *stage == "deliver/v1" {
            ev.proof = json!({
                "review_proof": {
                    "decision": "pass",
                    "contours": {
                        "audit": {"applicable": true, "evidence": "Reopen verification passed"}
                    }
                }
            });
        }
        tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": serde_json::to_value(&ev).unwrap(),
            }))
            .unwrap(),
        )
        .unwrap();
    }

    // Verify task is completed in SQLite
    let completed_task = store.inspect(task_id).unwrap();
    assert_eq!(completed_task.status, "completed");
    assert!(completed_task.completed_at.is_some());
    assert!(completed_task.receipt.is_some());

    let milestone_path = root.0.join("docs/010-reopen.md");
    let completed_text = std::fs::read_to_string(&milestone_path).unwrap();

    // Subtask modifications fail with informative TASK_TERMINAL message
    let add_err = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap_err();
    assert!(add_err.to_string().contains("TASK_TERMINAL"));
    assert!(add_err.to_string().contains("reopen or reset"));

    // Reopen requires its durable milestone receipt to be available before SQLite changes.
    let held_path = root.0.join("docs/010-reopen-held.md");
    std::fs::rename(&milestone_path, &held_path).unwrap();
    assert!(tasks::reopen(&root.0, task_id).is_err());
    assert_eq!(store.inspect(task_id).unwrap().status, "completed");
    std::fs::rename(&held_path, &milestone_path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        completed_text
    );

    // MCP task_manage with action: "reopen"
    let reopen_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Reopen,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reopen_res["status"], "ready");
    assert_eq!(reopen_res["gate"], 0);
    assert!(reopen_res["receipt"].is_null());
    assert!(reopen_res["completed_at"].is_null());
    let reopened_text = std::fs::read_to_string(&milestone_path).unwrap();
    assert!(!reopened_text.contains("status: completed"));
    assert!(!reopened_text.contains("receipt:"));

    // Reopened task can accept subtasks
    let sub = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap();
    assert_eq!(sub.subtask_ref, "sub-audit");
    assert_eq!(sub.status, "pending");

    // Reopened task can be claimed again at contract/v1
    let re_claimed = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract/v1".into(),
            worker_id: "auditor".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(re_claimed["status"], "in_progress");

    // MCP task_manage with action: "reset" resets in-progress claim
    let reset_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Reset,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reset_res["status"], "ready");
}

#[test]
fn milestone_resolution_prefers_active_path_components_and_propagates_list_errors() {
    let root = ScopedWorkspace::new("forge_milestone_archive_precedence");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\n",
    );

    let active_spec = "---\nid: m-active-archive-fix\ntitle: Active Archive Fix\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: active-match\ntarget: Active manifest\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    let archived_spec = "---\nid: m-archived-archive-fix\ntitle: Archived Archive Fix\ndoc_type: contract\nstatus: completed\n---\n```yaml\ntask_ref: archived-match\ntarget: Archived manifest\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/051-archive-fix.md", active_spec);
    root.write("docs/milestones/archive/051-archive-fix.md", archived_spec);

    let by_stem = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("051-archive-fix".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let by_stem_tasks = by_stem["tasks"].as_array().unwrap();
    assert_eq!(by_stem_tasks.len(), 1);
    assert_eq!(
        by_stem_tasks[0]["milestone_ref"],
        "docs/milestones/051-archive-fix.md"
    );

    let by_archive_path = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("docs/milestones/archive/051-archive-fix.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let by_archive_tasks = by_archive_path["tasks"].as_array().unwrap();
    assert_eq!(by_archive_tasks.len(), 1);
    assert_eq!(
        by_archive_tasks[0]["milestone_ref"],
        "docs/milestones/archive/051-archive-fix.md"
    );

    let unscoped_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            ..Default::default()
        },
    );
    assert!(
        unscoped_sync.is_err(),
        "sync requires a workspace or milestone selector"
    );

    root.write("docs/milestones/README.md", "# Milestone index\n");
    let non_manifest_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": "docs/milestones/README.md",
        "status": "all"
    }))
    .unwrap();
    assert!(tasks::list(&root.0, non_manifest_params).is_err());

    let outside = ScopedWorkspace::new("forge_milestone_outside");
    outside.write("010-outside.md", active_spec);
    std::os::unix::fs::symlink(
        outside.0.join("010-outside.md"),
        root.0.join("docs/milestones/053-symlink.md"),
    )
    .unwrap();
    for reference in ["../outside.md", "docs/milestones/053-symlink.md"] {
        let params = serde_json::from_value(serde_json::json!({
            "repository": "test",
            "milestone_ref": reference,
            "status": "all"
        }))
        .unwrap();
        assert!(
            tasks::list(&root.0, params).is_err(),
            "milestone path must remain confined to the workspace: {reference}"
        );
    }

    let list_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": "m-does-not-exist",
        "status": "all",
        "stage": null
    }))
    .unwrap();
    let list_error = tasks::list(&root.0, list_params).unwrap_err();
    assert!(
        list_error.to_string().contains("not found in workspace"),
        "task_list must propagate milestone resolution errors: {list_error}"
    );

    let malformed_spec = "---\nid: m-invalid-frontmatter\ntitle: Invalid frontmatter\ndoc_type: contract\nbroken: [\n---\n";
    let malformed_path = "docs/milestones/nested/052-invalid-frontmatter.md";
    root.write(malformed_path, malformed_spec);

    let malformed_id_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": "m-does-not-exist",
        "status": "all"
    }))
    .unwrap();
    let malformed_id_error = tasks::list(&root.0, malformed_id_params).unwrap_err();
    assert!(
        malformed_id_error.to_string().contains(
            "failed to read milestone id from 'docs/milestones/nested/052-invalid-frontmatter.md'"
        ),
        "ID lookup must preserve manifest parse errors: {malformed_id_error}"
    );

    let malformed_list_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": malformed_path,
        "status": "all"
    }))
    .unwrap();
    let malformed_list_error = tasks::list(&root.0, malformed_list_params).unwrap_err();
    assert!(
        malformed_list_error
            .to_string()
            .contains(&format!("invalid milestone manifest '{malformed_path}'")),
        "task_list must propagate malformed milestone frontmatter: {malformed_list_error}"
    );

    let malformed_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(malformed_path.into()),
            ..Default::default()
        },
    );
    assert!(
        malformed_sync
            .unwrap_err()
            .to_string()
            .contains(&format!("invalid milestone manifest '{malformed_path}'")),
        "task_manage sync must reject malformed milestone frontmatter"
    );
}

#[test]
fn task_manage_sync_and_list_resolve_milestone_reference_by_id_prefix_stem_and_path() {
    let root = ScopedWorkspace::new("forge_milestone_ref_resolution");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\n",
    );
    let milestone_content = "---\nid: m-tool-performance-and-storage-compaction\ntitle: Performance\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: compaction\ntarget: Compact storage\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(
        "docs/milestones/030-tool-performance-and-storage-compaction.md",
        milestone_content,
    );

    // 1. Sync by milestone ID ("m-tool-performance-and-storage-compaction")
    let sync_by_id = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            workspace: Some("test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_id["tasks"].as_array().unwrap().len(), 1);

    // List by milestone ID
    let list_by_id = tasks::list(
        &root.0,
        tasks::List {
            repository: Some("test".into()),
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(list_by_id["tasks"].as_array().unwrap().len(), 1);

    // 2. Sync by numeric prefix ("030" and "30")
    let sync_by_prefix = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("030".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_prefix["tasks"].as_array().unwrap().len(), 1);

    let list_by_prefix = tasks::list(
        &root.0,
        tasks::List {
            repository: None,
            milestone_ref: Some("30".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(list_by_prefix["tasks"].as_array().unwrap().len(), 1);

    // 3. Sync by filename stem ("030-tool-performance-and-storage-compaction")
    let sync_by_stem = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("030-tool-performance-and-storage-compaction".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_stem["tasks"].as_array().unwrap().len(), 1);

    // 4. Sync by full relative file path
    let sync_by_path = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(
                "docs/milestones/030-tool-performance-and-storage-compaction.md".into(),
            ),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_path["tasks"].as_array().unwrap().len(), 1);

    // 5. Sync with a workspace selector imports all workspace manifests
    let sync_all = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_all["tasks"].as_array().unwrap().len(), 1);

    // 6. Non-existent milestone reference produces descriptive error instead of raw os error 2
    let err = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("m-non-existent".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("not found in workspace"),
        "error must be descriptive: {err}"
    );
}

#[test]
fn task_list_filters_milestone_status_and_controls_subtask_details() {
    let root = ScopedWorkspace::new("forge_task_list_archive_and_evidence");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let active_spec = "---\nid: m-active\ntitle: Active Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: active-task\ntarget: Active task target\nproof_policy: seam-test-first\nscope: [src/]\nsubtasks:\n  - subtask_ref: sub-1\n    title: Active subtask\n    status: in_progress\n    evidence: Secret heavy evidence blob\n```\n";
    root.write("docs/milestones/010-active.md", active_spec);

    let archived_spec = "---\nid: m-archived\ntitle: Archived Milestone\ndoc_type: contract\nstatus: completed\n---\n# Tasks\n```yaml\ntask_ref: archived-task\ntarget: Archived task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/archive/009-archived.md", archived_spec);

    let planned_spec = "---\nid: m-planned\ntitle: Planned Milestone\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: planned-task\ntarget: Planned task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/008-planned.md", planned_spec);

    let cancelled_active_spec = "---\nid: m-cancelled\ntitle: Cancelled Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: cancelled-task\ntarget: Cancelled task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(
        "docs/milestones/archive/007-cancelled.md",
        cancelled_active_spec,
    );
    let cancelled_spec = "---\nid: m-cancelled\ntitle: Cancelled Milestone\ndoc_type: contract\nstatus: cancelled\nclosure:\n  reason: Replaced by a later milestone\n---\n# Tasks\n```yaml\ntask_ref: cancelled-task\ntarget: Cancelled task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    for (spec, path) in [
        (active_spec, "docs/milestones/010-active.md"),
        (archived_spec, "docs/milestones/archive/009-archived.md"),
        (planned_spec, "docs/milestones/008-planned.md"),
        (
            cancelled_active_spec,
            "docs/milestones/archive/007-cancelled.md",
        ),
    ] {
        store
            .sync(&Milestone::parse(spec, "forge-mcp").unwrap(), path, &root.0)
            .unwrap();
    }
    // Simulate the pre-sync window: the manifest is cancelled, but its stored task has not been pruned.
    root.write("docs/milestones/archive/007-cancelled.md", cancelled_spec);

    // Active milestones are the default, regardless of task status selection.
    let active_only = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let tasks_arr = active_only["tasks"].as_array().unwrap();
    assert_eq!(tasks_arr.len(), 1);
    assert_eq!(
        tasks_arr[0]["task_id"],
        "forge-mcp/forge-mcp/m-active:active-task"
    );

    // Compact listings retain the subtask reference and status, omitting details.
    let subtasks = tasks_arr[0]["subtasks"].as_array().unwrap();
    assert_eq!(subtasks.len(), 1);
    assert_eq!(subtasks[0]["subtask_ref"], "sub-1");
    assert!(subtasks[0].get("title").is_none());
    assert_eq!(subtasks[0]["status"], "in_progress");
    assert!(
        subtasks[0].get("evidence").is_none(),
        "subtask evidence must be stripped in task_list"
    );

    let full_detail = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            detail: Some(tasks::TaskListDetail::Full),
            ..Default::default()
        },
    )
    .unwrap();
    let full_subtask = &full_detail["tasks"][0]["subtasks"][0];
    assert_eq!(full_subtask["title"], "Active subtask");
    assert_eq!(full_subtask["evidence"], "Secret heavy evidence blob");

    // The only explicit milestone filters are active, planned, completed, and all.
    for (filter, expected_id) in [
        (
            tasks::MilestoneStatusFilter::Active,
            "forge-mcp/forge-mcp/m-active:active-task",
        ),
        (
            tasks::MilestoneStatusFilter::Planned,
            "forge-mcp/forge-mcp/m-planned:planned-task",
        ),
        (
            tasks::MilestoneStatusFilter::Completed,
            "forge-mcp/forge-mcp/m-archived:archived-task",
        ),
    ] {
        let filtered = tasks::list(
            &root.0,
            tasks::List {
                milestone_status: Some(filter),
                status: Some(tasks::Status::All),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["tasks"][0]["task_id"], expected_id);
    }

    let all_statuses = tasks::list(
        &root.0,
        tasks::List {
            milestone_status: Some(tasks::MilestoneStatusFilter::All),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let all_ids: Vec<_> = all_statuses["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(all_ids.len(), 4);
    assert!(all_ids.contains(&"forge-mcp/forge-mcp/m-cancelled:cancelled-task"));

    // A targeted milestone reference exposes its tasks by default, including a pre-sync cancellation.
    let direct_archived = tasks::list(
        &root.0,
        tasks::List {
            milestone_ref: Some("009".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let direct_arr = direct_archived["tasks"].as_array().unwrap();
    assert_eq!(direct_arr.len(), 1);
    assert_eq!(
        direct_arr[0]["task_id"],
        "forge-mcp/forge-mcp/m-archived:archived-task"
    );
    let direct_cancelled = tasks::list(
        &root.0,
        tasks::List {
            milestone_ref: Some("007".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(direct_cancelled["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        direct_cancelled["tasks"][0]["task_id"],
        "forge-mcp/forge-mcp/m-cancelled:cancelled-task"
    );
}

#[test]
fn milestone_lifecycle_sync_validates_status_and_preserves_cancellation_and_subtask_state() {
    let root = ScopedWorkspace::new("forge_milestone_lifecycle_sync");
    root.write("src/lib.rs", "pub fn lifecycle() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let database_path = tasks::database_path(&root.0).unwrap();
    std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&database_path).unwrap();
    db.execute_batch(
        "CREATE TABLE task_store_metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO task_store_metadata VALUES('schema_version', '1');
         CREATE TABLE task_blackboard(
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             milestone_ref TEXT NOT NULL,
             task_id TEXT REFERENCES tasks ON DELETE CASCADE,
             subtask_ref TEXT,
             author TEXT NOT NULL,
             topic TEXT NOT NULL,
             payload TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE INDEX idx_task_blackboard_task_created ON task_blackboard(task_id, created_at);",
    )
    .unwrap();
    drop(db);

    let manifest = |id: &str,
                    status: Option<&str>,
                    closure_reason: Option<&str>,
                    task_ref: &str,
                    revision: u64,
                    target: &str,
                    subtask_ref: &str| {
        let status = status
            .map(|value| format!("status: {value}\n"))
            .unwrap_or_default();
        let closure = match (status.as_str(), closure_reason) {
            (_, Some(reason)) => format!("closure:\n  reason: \"{reason}\"\n"),
            ("status: cancelled\n", None) => "closure:\n  owner: platform\n".into(),
            _ => String::new(),
        };
        format!(
            "---\nid: {id}\ntitle: {id}\ndoc_type: contract\n{status}{closure}---\n# Tasks\n```yaml\ntask_ref: {task_ref}\ntarget: {target}\nproof_policy: seam-test-first\ncontract_revision: {revision}\nscope: [src/]\nsubtasks:\n  - subtask_ref: {subtask_ref}\n    title: {subtask_ref}\n    status: pending\n```\n"
        )
    };
    let sync = |path: &str| {
        tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(path.to_owned()),
                ..Default::default()
            },
        )
    };

    let crlf_path = "docs/milestones/015-crlf-activation.md";
    let crlf_id = "forge-mcp/forge-mcp/m-crlf-activation:activate";
    let crlf_manifest = manifest(
        "m-crlf-activation",
        Some("planned"),
        None,
        "activate",
        1,
        "Activate CRLF milestone on claim",
        "activate-subtask",
    )
    .replace('\n', "\r\n");
    root.write(crlf_path, &crlf_manifest);
    sync(crlf_path).unwrap();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: crlf_id.into(),
            stage: "contract/v1".into(),
            worker_id: "crlf-lifecycle-test".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            bundle: None,
        },
    )
    .unwrap();
    let crlf_activated =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-crlf-activation", true)
            .unwrap();
    assert_eq!(crlf_activated["status"], "active");
    assert!(crlf_activated["started_at"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));

    let default_path = "docs/milestones/010-default.md";
    let default_spec = manifest(
        "m-default",
        None,
        None,
        "remaining",
        1,
        "Keep remaining work",
        "preserve-progress",
    );
    root.write(default_path, &default_spec);
    let default_sync = sync(default_path).unwrap();
    assert_eq!(default_sync["tasks"].as_array().unwrap().len(), 1);
    let default_view =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-default", false).unwrap();
    assert_eq!(default_view["status"], "planned");

    let invalid_path = "docs/milestones/001-invalid-status.md";
    root.write(
        invalid_path,
        &manifest(
            "m-invalid-status",
            Some("deferred"),
            None,
            "invalid",
            1,
            "Reject invalid lifecycle status",
            "invalid-subtask",
        ),
    );
    let invalid_status = sync(invalid_path);
    assert!(
        invalid_status.is_err(),
        "milestone sync must reject statuses outside active, planned, completed, and cancelled: {invalid_status:?}"
    );

    let archived_default_path = "docs/milestones/archive/011-archived-default.md";
    root.write(
        archived_default_path,
        &manifest(
            "m-archived-default",
            None,
            None,
            "archived-default",
            1,
            "Default archived milestone",
            "archived-subtask",
        ),
    );
    sync(archived_default_path).unwrap();
    let archived_default_view =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-archived-default", true)
            .unwrap();
    assert_eq!(archived_default_view["status"], "completed");

    for (id, status, path) in [
        ("m-planned", "planned", "docs/milestones/020-planned.md"),
        (
            "m-completed",
            "completed",
            "docs/milestones/archive/030-completed.md",
        ),
    ] {
        root.write(
            path,
            &manifest(
                id,
                Some(status),
                None,
                status,
                1,
                "Accept a supported lifecycle status",
                "status-subtask",
            ),
        );
        assert_eq!(sync(path).unwrap()["tasks"].as_array().unwrap().len(), 1);
    }

    let cancelled_path = "docs/milestones/040-cancelled.md";
    let cancelled_id = "forge-mcp/forge-mcp/m-cancelled:retired";
    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("active"),
            None,
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    assert_eq!(
        sync(cancelled_path).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let remaining_id = "forge-mcp/forge-mcp/m-default:remaining";
    let store = TasksStore::open(&database_path).unwrap();
    let progress = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(remaining_id.into()),
            subtask_ref: Some("preserve-progress".into()),
            subtask_status: Some("in_progress".into()),
            evidence: Some("verified retained work".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(progress["subtask"]["status"], "in_progress");

    root.write(
        default_path,
        &manifest(
            "m-default",
            None,
            None,
            "remaining",
            2,
            "Keep remaining work after revision",
            "preserve-progress",
        ),
    );
    sync(default_path).unwrap();
    let retained = store
        .subtask_list(remaining_id)
        .unwrap()
        .into_iter()
        .find(|subtask| subtask.subtask_ref == "preserve-progress")
        .unwrap();
    assert_eq!(retained.status, "in_progress");
    assert_eq!(retained.evidence.as_deref(), Some("verified retained work"));

    store
        .connection
        .execute(
            "INSERT INTO task_dependencies(task_id, dependency_id, satisfied) VALUES (?1, ?2, 0)",
            rusqlite::params![remaining_id, cancelled_id],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_dependencies(task_id, dependency_id, satisfied) VALUES (?1, ?2, 0)",
            rusqlite::params![cancelled_id, remaining_id],
        )
        .unwrap();
    let cancelled_scope_ref = store.milestone_scope_ref(cancelled_path).unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref, task_id, subtask_ref, author, topic, payload, created_at) \
             VALUES(?1, ?2, NULL, 'reviewer', 'architectural_notes', 'task context', 1)",
            rusqlite::params![cancelled_scope_ref, cancelled_id],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref, task_id, subtask_ref, author, topic, payload, created_at) \
             VALUES(?1, NULL, NULL, 'reviewer', 'architectural_notes', 'milestone context', 2)",
            [&cancelled_scope_ref],
        )
        .unwrap();

    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("cancelled"),
            None,
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    let missing_reason = sync(cancelled_path);
    assert!(
        missing_reason.is_err(),
        "cancelled milestone sync must require closure.reason: {missing_reason:?}"
    );
    assert!(store.inspect(cancelled_id).is_ok());
    assert_eq!(
        store
            .blackboard_read(cancelled_id, None, None)
            .unwrap()
            .len(),
        1
    );

    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("cancelled"),
            Some("   "),
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    let blank_reason = sync(cancelled_path);
    assert!(
        blank_reason.is_err(),
        "cancelled milestone sync must reject a blank closure.reason: {blank_reason:?}"
    );
    assert!(store.inspect(cancelled_id).is_ok());

    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("cancelled"),
            Some("Replaced by the remaining milestone"),
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    let cancelled_sync = sync(cancelled_path).unwrap();
    assert!(cancelled_sync["tasks"].as_array().unwrap().is_empty());
    assert!(store.inspect(cancelled_id).is_err());
    assert!(store
        .blackboard_read(cancelled_id, None, None)
        .unwrap()
        .is_empty());

    let incoming_satisfied: i64 = store
        .connection
        .query_row(
            "SELECT satisfied FROM task_dependencies WHERE task_id=?1 AND dependency_id=?2",
            rusqlite::params![remaining_id, cancelled_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(incoming_satisfied, 0);
    let cancelled_blackboard_rows: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&cancelled_scope_ref],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cancelled_blackboard_rows, 0);
    let outgoing_count: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM task_dependencies WHERE task_id=?1",
            [cancelled_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(outgoing_count, 0);
    let remaining = store
        .list(None, "all", None)
        .unwrap()
        .into_iter()
        .find(|task| task.task_id == remaining_id)
        .unwrap();
    assert_eq!(remaining.status, "blocked");
}

#[test]
fn cancelled_milestone_blackboard_pruning_is_project_scoped() {
    let root = ScopedWorkspace::new("forge_cancelled_milestone_project_isolation");
    let database_path = root.0.join(".forge/tasks.sqlite");
    let initialized = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    drop(initialized);
    let db = rusqlite::Connection::open(&database_path).unwrap();
    db.execute_batch(
        "ALTER TABLE task_blackboard RENAME TO task_blackboard_tasks;
         CREATE TABLE task_blackboard(
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             milestone_ref TEXT NOT NULL,
             task_id TEXT REFERENCES tasks ON DELETE CASCADE,
             subtask_ref TEXT,
             author TEXT NOT NULL,
             topic TEXT NOT NULL,
             payload TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         DROP TABLE task_blackboard_tasks;",
    )
    .unwrap();
    drop(db);

    let path = "docs/milestones/040-cancelled.md";
    let primary = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    let linked = TasksStore::open_project(&database_path, "linked", "app").unwrap();
    let primary_ref = primary.milestone_scope_ref(path).unwrap();
    let linked_ref = linked.milestone_scope_ref(path).unwrap();
    assert_ne!(primary_ref, linked_ref);
    primary
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'a','note','primary',1)",
            [&primary_ref],
        )
        .unwrap();
    primary
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'b','note','linked',2)",
            [&linked_ref],
        )
        .unwrap();

    let mut selected = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    assert_eq!(selected.prune_cancelled(path).unwrap(), 0);
    let selected_rows: i64 = primary
        .connection
        .query_row(
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&primary_ref],
            |row| row.get(0),
        )
        .unwrap();
    let linked_rows: i64 = primary
        .connection
        .query_row(
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&linked_ref],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(selected_rows, 0);
    assert_eq!(linked_rows, 1);
}
