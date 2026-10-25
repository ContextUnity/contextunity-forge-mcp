use super::super::support::*;
use contextunity_forge_mcp::engine::tasks;

#[test]
fn delivery_requires_terminal_subtasks_with_verification_evidence() {
    for (status, evidence_value, expected_error) in [
        ("in_progress", None, "TASK_SUBTASKS_INCOMPLETE"),
        ("completed", None, "TASK_SUBTASK_EVIDENCE_MISSING"),
        ("completed", Some("   "), "TASK_SUBTASK_EVIDENCE_MISSING"),
    ] {
        let root = ScopedWorkspace::new("forge_delivery_subtask_guard");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root.write(".gitignore", super::CANONICAL_FORGE_GITIGNORE);
        root.write("src/lib.rs", "pub fn delivered() -> bool { true }\n");

        let evidence_field = evidence_value.map_or_else(String::new, |value| {
            format!("    evidence: {}\n", serde_json::to_string(value).unwrap())
        });
        let manifest = format!(
            "---\nid: m-delivery-subtasks\ntitle: Delivery subtask guard\ndoc_type: contract\nstatus: active\n---\n# Delivery subtask guard\n```yaml\ntask_ref: deliver\ntarget: Require completed subtask proof at delivery\nproof_policy: seam-test-first\nscope: [src/lib.rs]\nsubtasks:\n  - subtask_ref: verify\n    title: Verify the delivered behavior\n    status: {status}\n{evidence_field}```\n"
        );
        let milestone_ref = "docs/milestones/010-delivery-subtasks.md";
        root.write(milestone_ref, &manifest);
        let milestone = Milestone::parse(&manifest, "forge-mcp").unwrap();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, milestone_ref, &root.0).unwrap();

        for (stage, worker_id) in [
            ("contract", "builder"),
            ("build", "builder"),
            ("review", "reviewer"),
        ] {
            tasks::claim(
                &root.0,
                tasks::Claim {
                    task_id: task_id.clone(),
                    stage: stage.into(),
                    worker_id: worker_id.into(),
                    worktree: root.0.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            let task = store.inspect(&task_id).unwrap();
            tasks::submit(
                &root.0,
                tasks::Submit {
                    task_id: task_id.clone(),
                    stage: stage.into(),
                    action: tasks::Action::Pass,
                    evidence: serde_json::to_value(evidence(&task)).unwrap(),
                    findings: None,
                },
            )
            .unwrap();
        }

        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.clone(),
                stage: "deliver".into(),
                worker_id: "delivery".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = store.inspect(&task_id).unwrap();
        let error = tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.clone(),
                stage: "deliver".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(evidence(&task)).unwrap(),
                findings: None,
            },
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(expected_error), "{status}: {error}");
        let unchanged = store.inspect(&task_id).unwrap();
        assert_eq!(unchanged.stage, "deliver", "{status}");
        assert_eq!(unchanged.status, "in_progress", "{status}");
    }
}

#[test]
fn task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks() {
    use contextunity_forge_mcp::{core::response::ResponsePolicy, mcp::response};
    use std::io::{BufRead, Write};

    // A separate process reaches BEGIN IMMEDIATE after a successful stale read.
    // Its SQLite busy callback is the rendezvous; no timing assumptions are needed.
    if let Ok(root) = std::env::var("AC002_BLOCKED_SYNC_ROOT") {
        let root = std::path::PathBuf::from(root);
        let mut store = TasksStore::open(&root.join(".forge/tasks.sqlite")).unwrap();
        let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
        assert!(!store.milestone_tombstoned(&milestone.id).unwrap());
        store
            .connection
            .busy_handler(Some(|attempt| {
                if attempt == 0 {
                    println!("AC002_WRITER_WAITING");
                    std::io::stdout().flush().unwrap();
                }
                std::thread::yield_now();
                true
            }))
            .unwrap();
        let result = if std::env::var("AC002_BLOCKED_SYNC_MODE").unwrap() == "create" {
            store.create(&milestone, "docs/010-test.md", &root, "first")
        } else {
            store.sync(&milestone, "docs/010-test.md", &root)
        };
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("STORE_SYNC_FAILED"));
        assert!(store.list(None, "all", None).unwrap().is_empty());
        return;
    }

    let mut race_failures = Vec::new();
    for mode in ["sync", "create"] {
        let (root, mut store, _) = fixture();
        let tx = store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "acdd::tasks::lifecycle::task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks", "--nocapture"])
            .env("AC002_BLOCKED_SYNC_ROOT", &root.0)
            .env("AC002_BLOCKED_SYNC_MODE", mode)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            assert_ne!(
                output.read_line(&mut line).unwrap(),
                0,
                "child never reached writer lock: {line}"
            );
            if line.contains("AC002_WRITER_WAITING") {
                break;
            }
            line.clear();
        }
        // These are the committed SQLite effects of a competing handoff. The
        // full public handoff, cascade, sentinel and rollback seam is in milestones.
        tx.execute_batch(
            "DELETE FROM task_revisions; DELETE FROM tasks;
            INSERT INTO milestone_tombstones VALUES('forge-mcp/forge-mcp/m-test',1,1,'[]');",
        )
        .unwrap();
        tx.commit().unwrap();
        let result = child.wait_with_output().unwrap();
        if !result.status.success() || !store.list(None, "all", None).unwrap().is_empty() {
            race_failures.push(format!(
                "{mode}: {}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
    }
    assert!(race_failures.is_empty(), "{}", race_failures.join("\n"));

    for action in [
        "sync",
        "sync_many",
        "claim",
        "minimal",
        "next",
        "context",
        "reader",
    ] {
        let (root, mut store, _) = fixture();
        let task_id = "forge-mcp/m-test:first";
        if action == "reader" {
            let tx = store
                .connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            let reader = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).expect(
                "initialized readers must open while another connection holds the writer lock",
            );
            assert_eq!(reader.inspect(task_id).unwrap().task_id, task_id);
            tx.rollback().unwrap();
            continue;
        }
        let large = if action == "sync_many" {
            "isolated".into()
        } else {
            "\\\"λ\n".repeat(20_000)
        };
        let mut spec = SPEC.replace(
            "invariants: [isolated]",
            &format!("invariants: [{}]", serde_json::to_string(&large).unwrap()),
        );
        if action == "sync_many" {
            for index in 0..200 {
                spec.push_str(&format!("```yaml\ntask_ref: extra-{index}\ntarget: Extra task\nproof_policy: seam-test-first\nscope: [src/extra-{index}.rs]\n```\n"));
            }
        }
        let milestone = Milestone::parse(&spec, "forge-mcp").unwrap();
        // Use the same initial contract with large, valid authority metadata.
        store
            .delete(
                None,
                Some("docs/010-test.md"),
                true,
                contextunity_forge_mcp::db::tasks_store::now(),
            )
            .unwrap();
        root.write("docs/010-test.md", &spec);
        store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
        let value = if matches!(action, "claim" | "minimal") {
            tasks::claim(
                &root.0,
                tasks::Claim {
                    task_id: task_id.into(),
                    stage: "contract".into(),
                    worker_id: "builder".into(),
                    worktree: root.0.to_string_lossy().into_owned(),
                    bundle: Some(action != "minimal"),
                },
            )
            .unwrap()
        } else {
            tasks::manage(
                &root.0,
                tasks::Manage {
                    action: match action {
                        "sync" | "sync_many" => tasks::ManageAction::Sync,
                        "next" => tasks::ManageAction::Next,
                        _ => tasks::ManageAction::Context,
                    },
                    milestone_ref: action
                        .starts_with("sync")
                        .then(|| "docs/010-test.md".into()),
                    task_id: (action == "context").then(|| task_id.into()),
                    worker_id: (action == "next").then(|| "builder".into()),
                    worktree: (action == "next").then(|| root.0.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let wire = response::result(Ok(value.clone()), &ResponsePolicy::default());
        assert_ne!(
            wire.is_error,
            Some(true),
            "{action}: mutation must return usable success: {wire:?}"
        );
        assert!(response::serialized_bytes(&wire) < 65536, "{action}");
        assert!(value.to_string().len() <= 24 * 1024, "{action}");
        if action == "sync_many" {
            assert_eq!(value["total"], 202);
            assert_eq!(value["byte_limited"], true);
            assert!(value["returned"].as_u64().unwrap() < 202);
            let count: usize = store
                .connection
                .query_row("SELECT count(*) FROM tasks", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 202, "response bounding must not skip sync effects");
        } else if !action.starts_with("sync") {
            assert_eq!(value["byte_limited"], true);
            assert!(!value["omitted_fields"].as_array().unwrap().is_empty());
        }
        assert_eq!(
            store.inspect(task_id).unwrap().applicable_invariants,
            vec![large]
        );
        if matches!(action, "claim" | "minimal" | "next") {
            assert_eq!(value["task_id"], task_id);
            assert_eq!(
                value["claim_revision"],
                store.inspect(task_id).unwrap().claim_revision
            );
            assert!(value["workflow_guidance"].is_object());
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
    let mut proof = evidence(&contract);
    proof.commit = None;
    let raw_evidence = serde_json::to_value(&proof).unwrap();
    for invalid in [
        json!({"task_id":task_id,"stage":"contract","action":"pass"}),
        json!({"task_id":task_id,"stage":"contract","action":"pass","evidence_ref":"proof.yaml"}),
        json!({"task_id":task_id,"stage":"contract","action":"pass","evidence":raw_evidence,"evidence_ref":"proof.yaml"}),
    ] {
        assert!(serde_json::from_value::<tasks::Submit>(invalid).is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let nonobject: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract",
        "action": "pass",
        "evidence": 42
    }))
    .unwrap();
    assert!(tasks::submit(&root.0, nonobject).is_err());
    assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    let request: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract",
        "action": "pass",
        "evidence": raw_evidence
    }))
    .unwrap();

    let submitted = tasks::submit(&root.0, request).unwrap();
    assert_eq!(submitted["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    let mut expected = raw_evidence.clone();
    expected["commit"] = stored["commit"].clone();
    expected["candidate_baseline_head"] = stored["candidate_baseline_head"].clone();
    assert_eq!(stored, expected);
    let snapshot_commit = stored["commit"].as_str().unwrap();
    assert_eq!(snapshot_commit.len(), 40);
    assert_eq!(snapshot_commit, submitted["snapshot"]["commit"]);
    assert_eq!(stored["candidate_baseline_head"], git_head(&root.0));
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
        stored
    );
}

#[test]
fn task_cli_submit_accepts_json_object_and_persists_gate_evidence() {
    use contextunity_forge_mcp::core::tasks::profile::{self, CommandDef};
    use contextunity_forge_mcp::engine::milestones::execute_command_gate;

    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut profile_override = (*profile::compiled()).clone();
    let command =
        "printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'";
    profile_override.commands.insert(
        "task6-cli-check".into(),
        CommandDef::Detailed {
            command: command.into(),
            timeout_seconds: Some(5),
            description: Some("Run the zero-argument CLI proof fixture".into()),
        },
    );
    profile_override
        .gates
        .iter_mut()
        .find(|gate| gate.id == "build")
        .unwrap()
        .command = Some("task6-cli-check".into());
    let profile_path = root.0.join(".forge/acdd/profile.yaml");
    std::fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
    std::fs::write(
        &profile_path,
        serde_yaml::to_string(&profile_override).unwrap(),
    )
    .unwrap();
    std::fs::write(root.0.join(".gitignore"), ".forge/\n").unwrap();
    let ignore = std::process::Command::new("git")
        .args(["add", ".gitignore"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(ignore.status.success(), "{ignore:?}");
    let staged = std::process::Command::new("git")
        .args(["add", "-f", ".forge/acdd/profile.yaml"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(staged.status.success(), "{staged:?}");
    let baseline = std::process::Command::new("git")
        .args(["commit", "-m", "configure task CLI proof fixture"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(baseline.status.success(), "{baseline:?}");
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
                "contract",
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
            "--format",
            "json",
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "submit",
            &task_id,
            "--stage",
            "contract",
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
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    let mut expected = serde_json::to_value(proof).unwrap();
    expected["commit"] = stored["commit"].clone();
    expected["candidate_baseline_head"] = stored["candidate_baseline_head"].clone();
    assert_eq!(stored, expected);
    let snapshot_commit = stored["commit"].as_str().unwrap();
    assert_eq!(snapshot_commit.len(), 40);
    assert_eq!(snapshot_commit, result["snapshot"]["commit"]);
    assert_eq!(stored["candidate_baseline_head"], git_head(&root.0));

    store
        .claim(&task_id, "build", "cli-worker", root.0.to_str().unwrap())
        .unwrap();
    let trace = execute_command_gate(
        &root.0,
        &task_id,
        "tasks.build",
        &profile_override.commands["task6-cli-check"],
        &["src/lib.rs".to_owned()],
    )
    .unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].exit_code, Some(0));

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--format",
            "json",
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "submit",
            &task_id,
            "--stage",
            "build",
            "--action",
            "pass",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let automatic: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(automatic["status"], "ready");
    assert_eq!(automatic["stage"], "review");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='build' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(
        stored["proof"]["command_proof"]["command"],
        "task6-cli-check"
    );
    assert_eq!(stored["proof"]["command_proof"]["exit_code"], 0);
    assert_eq!(stored["proof"]["command_proof"]["tests_passed"], 1);
}

#[test]
fn typed_task_proofs_pass_through_engine_and_real_store() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let extension = vec!["src/reviewed.rs".to_owned()];
    store.extend_scope(&task_id, &extension, &root.0).unwrap();
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
            json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}}),
            vec![
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":0}}),
                json!({"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}),
                json!({"contract_proof":{"seam_test":"tests/acdd/tasks.rs","red_exit_code":101}}),
            ],
        ),
        (
            "build",
            "builder",
            json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"running 2 tests\ntest result: ok. 2 passed ✓"}}),
            vec![
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":1,"tests_passed":1,"tests_failed":1}}),
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"x".repeat(65_537)}}),
                json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0}}),
                json!({"command":"cargo test --test acdd","result":"passed","artifacts":[]}),
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"output":"passed"}}),
            ],
        ),
        (
            "review",
            "independent-reviewer",
            json!({"review_proof":{"decision":"pass","contours":contours.clone(),"extended_scope_review":{"approved_paths":["src/reviewed.rs"],"reason_verified":true}}}),
            vec![
                json!({"review_proof":{"decision":"pass","contours":contours.clone()}}),
                json!({"review_proof":{"decision":"pass","contours":contours.clone(),"extended_scope_review":{"approved_paths":["src/reviewed.rs"],"reason_verified":false}}}),
                json!({"review_proof":{"decision":"pass","contours":contours.clone(),"extended_scope_review":{"approved_paths":["src/reviewed.rs","src/extra.rs"],"reason_verified":true}}}),
                json!({"review_proof":{"decision":"pass","contours":contours.clone(),"extended_scope_review":{"approved_paths":["src/reviewed.rs","src/reviewed.rs"],"reason_verified":true}}}),
                json!({"review_proof":{"decision":"pass","contours":contours.clone(),"extended_scope_review":{"approved_paths":["src/mismatched.rs"],"reason_verified":true}}}),
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
                json!({"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}),
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}}),
            ),
            1 => (
                json!({"command":"cargo test","result":"failed","artifacts":[]}),
                json!({"command_proof":{"command":"cargo test","exit_code":101,"tests_passed":0,"tests_failed":1,"log":"one test failed"}}),
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
fn tasks_coordinate_claims_dependencies_reset_scope_and_claim_diagnostic() {
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
            ..Default::default()
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
    let winner = store.inspect(&id).unwrap();
    let claimed_at: i64 = store
        .connection
        .query_row(
            "SELECT claimed_at FROM task_claims WHERE task_id=?1 AND ended=0",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    let winner_id = winner.worker_id.as_deref().unwrap();
    let expected_collision = format!(
        "TASK_ALREADY_CLAIMED: {id}. Claimed by worker '{winner_id}' in worktree '{}' (revision {}, claimed at {claimed_at}). Do: submit the active claim from worker '{winner_id}' before opening another.",
        winner.worktree.as_deref().unwrap(),
        winner.claim_revision,
    );
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.to_string() == expected_collision));
    assert!(milestone.acdd_profile.is_none());
    assert!(milestone.tasks[0].acdd_profile.is_none());
    let before_sync = store.inspect(&id).unwrap();
    assert_eq!(before_sync.profile_pin, None);
    let synced = store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    assert_eq!(synced.len(), 2);
    let after_sync = store.inspect(&id).unwrap();
    assert_eq!(after_sync.profile_pin, None);
    assert_eq!(after_sync.digest, before_sync.digest);
    assert_eq!(after_sync.status, before_sync.status);
    assert_eq!(after_sync.stage, before_sync.stage);
    assert_eq!(after_sync.claim_revision, before_sync.claim_revision);
    let before = store.inspect(&id).unwrap();
    let reset = store.reset(&id).unwrap();
    assert!(reset.claim_revision > before.claim_revision);
    assert_eq!(reset.status, "ready");
    let extended = store
        .extend_scope(
            &id,
            &["src/next.rs".into(), "tests/fixture.rs".into()],
            &root.0,
        )
        .unwrap();
    assert!(extended.spec.scope.contains(&"src/next.rs".into()));
    assert!(extended.spec.scope.contains(&"tests/fixture.rs".into()));
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
    let id = milestone.task_id(&milestone.tasks[0]);
    store
        .blackboard_post(
            &id,
            "builder",
            "decisions",
            "Keep the parser boundary stable.",
        )
        .unwrap();
    store
        .blackboard_post(
            &id,
            "builder",
            "deferred",
            "Measure large fixture cost later.",
        )
        .unwrap();
    store
        .connection
        .execute(
            "UPDATE task_blackboard SET created_at=1 WHERE task_id=?1",
            [&id],
        )
        .unwrap();
    for (gate, stage) in GATES.iter().enumerate() {
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.clone(),
                stage: (*stage).to_owned(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = store.inspect(&id).unwrap();
        let mut evidence = evidence(&task);
        evidence.commit = None;
        if gate == 3 {
            evidence.proof = json!({"stage":stage});
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":id,
                "stage":stage,
                "action":"pass",
                "evidence":evidence,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, rejected).is_err());
            assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
            evidence.proof = json!({"delivery_proof":{}});
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":id,
            "stage":stage,
            "action":"pass",
            "evidence":evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(
            result["status"],
            if gate == 3 { "completed" } else { "ready" }
        );
    }
    let completed = store.inspect(&id).unwrap();
    let rollup = completed.receipt.as_ref().unwrap().rollup.as_ref().unwrap();
    assert_eq!(
        rollup.architectural_notes,
        vec![
            "decisions: Keep the parser boundary stable.",
            "deferred: Measure large fixture cost later."
        ]
    );
    assert!(completed.receipt.as_ref().unwrap().commit.is_some());
    let remaining_messages: i64 = store
        .connection
        .query_row(
            "SELECT COUNT(*) FROM task_blackboard WHERE task_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining_messages, 0);
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
        commit: Some("candidate".into()),
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
fn claim_reports_structured_authority_gap_for_frontmatter_and_task_drift() {
    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let changed = SPEC
        .replace(
            "status: active\n",
            "status: active\nowners: [docs/AGENTS.md]\n",
        )
        .replace("target: Deliver first", "target: Deliver amended first");
    root.write("docs/010-test.md", &changed);

    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();

    assert_eq!(
        error,
        format!(
            "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; frontmatter fields [milestone.owners], task fields [task.target]. Do: revert [milestone.owners] in docs/010-test.md, set contract_revision above 1, and run contextunity-forge-mcp task sync."
        )
    );
}

#[test]
fn authority_gap_attributes_milestone_invariants_and_owners_with_specific_repair() {
    for (expected_fields, advice) in [
        (
            "milestone.invariants",
            ". Declare task invariants inside task.invariants",
        ),
        ("milestone.owners", ""),
    ] {
        let (root, _, milestone) = fixture();
        let id = milestone.task_id(&milestone.tasks[0]);
        let changed = if expected_fields == "milestone.invariants" {
            SPEC.replace("invariants: [isolated]", "invariants: [changed]")
        } else {
            SPEC.replace(
                "status: active\n",
                "status: active\nowners: [docs/AGENTS.md]\n",
            )
        };
        root.write("docs/010-test.md", &changed);
        let error = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.clone(),
                stage: "contract".into(),
                worker_id: "header-authority-check".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            error,
            format!(
                "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; digest fields [{expected_fields}]. Do: revert [{expected_fields}] in docs/010-test.md{advice}."
            )
        );
    }
}

#[test]
fn sync_reports_structured_authority_gap_for_task_drift_at_equal_revision() {
    let (root, mut store, milestone) = fixture();
    let changed_text = SPEC.replace("target: Deliver first", "target: Deliver changed");
    let changed = Milestone::parse(&changed_text, "forge-mcp").unwrap();
    let error = store
        .sync(&changed, "docs/010-test.md", &root.0)
        .unwrap_err()
        .to_string();
    let id = milestone.task_id(&milestone.tasks[0]);
    assert_eq!(
        error,
        format!(
            "AUTHORITY_GAP: task '{id}'. file revision 1 <= stored 1; digest fields [task.target]. Do: set contract_revision above 1 and run contextunity-forge-mcp task sync."
        )
    );
}

#[test]
fn milestone_id_change_is_rejected_before_sync_admission_and_claim_lookup() {
    let (root, mut store, milestone) = fixture();
    let original_id = milestone.task_id(&milestone.tasks[0]);
    let changed_text = SPEC.replace("id: m-test", "id: m-renamed");
    root.write("docs/010-test.md", &changed_text);
    let changed = Milestone::parse(&changed_text, "forge-mcp").unwrap();
    let sync_error = store
        .sync(&changed, "docs/010-test.md", &root.0)
        .unwrap_err()
        .to_string();
    assert_eq!(
        sync_error,
        "AUTHORITY_GAP: milestone 'docs/010-test.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-test.md. Task ids are forge-mcp/m-renamed:first; sync would admit them as new tasks."
    );
    assert!(store.inspect("forge-mcp/m-renamed:first").is_err());

    let claim_error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: original_id,
            stage: "contract".into(),
            worker_id: "id-change-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        claim_error,
        "AUTHORITY_GAP: forge-mcp/m-test:first. milestone 'docs/010-test.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-test.md. Task ids are forge-mcp/m-renamed:first; task cannot be resolved."
    );
}

#[test]
fn milestone_id_guard_ignores_foreign_project_at_the_same_manifest_path() {
    let root = ScopedWorkspace::new("forge_tasks_shared_manifest_path");
    let foreign_root = ScopedWorkspace::new("forge_tasks_foreign_manifest_path");
    let database = root.0.join(".forge/tasks.sqlite");
    let mut foreign_store = TasksStore::open_project(&database, "forge-mcp", "aaa").unwrap();
    let mut current_store = TasksStore::open_project(&database, "forge-mcp", "zzz").unwrap();
    let specification = |project: &str, id: &str| {
        format!(
            "---\nrepository: forge-mcp\nproject: {project}\nid: {id}\ntitle: Shared path\ndoc_type: contract\nstatus: active\n---\n# Shared path\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
        )
    };
    let foreign = Milestone::parse(&specification("aaa", "m-test"), "forge-mcp").unwrap();
    foreign_root.write(
        "docs/milestones/010-shared.md",
        &specification("aaa", "m-test"),
    );
    root.write(
        "docs/milestones/010-shared.md",
        &specification("zzz", "m-test"),
    );
    foreign_store
        .sync(&foreign, "docs/milestones/010-shared.md", &foreign_root.0)
        .unwrap();
    let current = Milestone::parse(&specification("zzz", "m-test"), "forge-mcp").unwrap();
    current_store
        .sync(&current, "docs/milestones/010-shared.md", &root.0)
        .unwrap();

    let renamed = Milestone::parse(&specification("zzz", "m-renamed"), "forge-mcp").unwrap();
    let error = current_store
        .sync(&renamed, "docs/milestones/010-shared.md", &root.0)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "AUTHORITY_GAP: milestone 'docs/milestones/010-shared.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/milestones/010-shared.md. Task ids are zzz/m-renamed:first; sync would admit them as new tasks."
    );
    assert!(current_store.inspect("zzz/m-renamed:first").is_err());
    current_store
        .connection
        .execute_batch(
            "INSERT INTO task_dependencies(task_id,dependency_id,satisfied) VALUES
         ('zzz/m-test:first','aaa/m-test:first',0),
         ('zzz/m-test:first','aaa/m-test:__milestone__',0);",
        )
        .unwrap();

    // Both projects legitimately use this ID and manifest path. A sync that
    // commits first must still be removed by the subsequent public handoff.
    for (store, mut milestone, project) in [
        (&mut foreign_store, foreign, "aaa"),
        (&mut current_store, current, "zzz"),
    ] {
        let workspace = if project == "aaa" {
            &foreign_root.0
        } else {
            &root.0
        };
        assert!(!store.milestone_tombstoned("m-test").unwrap(), "{project}");
        if project == "zzz" {
            root.write("src/lib.rs", "pub fn shared() {}\n");
            root.write(
                "docs/milestones/010-shared.md",
                &specification("zzz", "m-test"),
            );
            root.write("forge-mcp.yaml", "roots: [src]\ndocs: [docs]\ntask_repository: forge-mcp\ntask_project: zzz\ntasks_db: .forge/tasks.sqlite\n");
            store.connection.execute_batch("CREATE TRIGGER reject_live_sync BEFORE INSERT ON tasks
                WHEN NEW.task_id='zzz/m-test:first' BEGIN SELECT RAISE(ABORT,'non-archive admission failure'); END;").unwrap();
            let error = tasks::manage(
                &root.0,
                tasks::Manage {
                    action: tasks::ManageAction::Sync,
                    workspace: Some("zzz".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(
                format!("{error:#}").contains("non-archive admission failure"),
                "{error:#}"
            );
            store
                .connection
                .execute_batch("DROP TRIGGER reject_live_sync")
                .unwrap();
            for explicit in [false, true] {
                let result = tasks::manage(
                    &root.0,
                    tasks::Manage {
                        action: tasks::ManageAction::Sync,
                        workspace: Some("zzz".into()),
                        milestone_ref: explicit.then(|| "docs/milestones/010-shared.md".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(result["total"], 1);
            }
            store
                .create(
                    &milestone,
                    "docs/milestones/010-shared.md",
                    &root.0,
                    "first",
                )
                .unwrap();
        }
        milestone.tasks[0].status = Some("cancelled".into());
        let terminal = store
            .sync(&milestone, "docs/milestones/010-shared.md", workspace)
            .unwrap();
        store
            .handoff_milestone(
                &format!("{project}/m-test:"),
                "docs/milestones/010-shared.md",
                "docs/archive/010-shared.md",
                &terminal,
                &Default::default(),
            )
            .unwrap();
        assert!(store.milestone_tombstoned("m-test").unwrap());
        assert!(store.list(None, "all", None).unwrap().is_empty());
        if project == "aaa" {
            let dependencies: Vec<(String, bool)> = store.connection
                .prepare("SELECT dependency_id,satisfied FROM task_dependencies WHERE task_id='zzz/m-test:first' ORDER BY dependency_id")
                .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(
                dependencies,
                vec![
                    ("aaa/m-test:__milestone__".into(), true),
                    ("aaa/m-test:first".into(), false),
                ]
            );
        }
    }
    let tombstones: Vec<(String, String)> = foreign_store.connection
        .prepare("SELECT milestone_id,completed_task_ids FROM milestone_tombstones ORDER BY milestone_id")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(
        tombstones,
        vec![
            ("forge-mcp/aaa/m-test".into(), "[]".into()),
            ("forge-mcp/zzz/m-test".into(), "[]".into()),
        ]
    );
}

#[test]
fn claim_distinguishes_missing_milestone_file_from_missing_task() {
    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    std::fs::remove_file(root.0.join("docs/010-test.md")).unwrap();
    let missing_file = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "missing-file-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        missing_file,
        format!(
            "TASK_AUTHORITY_MISSING: {id}. milestone file 'docs/010-test.md' is missing. Do: restore milestone file 'docs/010-test.md'."
        )
    );

    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    root.write(
        "docs/010-test.md",
        &SPEC
            .replace("task_ref: first", "task_ref: renamed")
            .replace("depends_on: [first]", "depends_on: [renamed]"),
    );
    let missing_task = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "missing-task-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        missing_task,
        format!(
            "TASK_AUTHORITY_MISSING: {id}. task '{id}' not found in milestone document 'docs/010-test.md'. Do: restore task '{id}' in 'docs/010-test.md'."
        )
    );
}

#[test]
fn unsynced_higher_revision_with_header_drift_reports_header_and_sync_action() {
    let (root, _, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let changed = SPEC
        .replace(
            "status: active\n",
            "status: active\nowners: [docs/AGENTS.md]\n",
        )
        .replace("task_ref: first", "task_ref: first\ncontract_revision: 2");
    root.write("docs/010-test.md", &changed);
    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "unsynced-revision-check".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        format!(
            "AUTHORITY_GAP: {id}. task '{id}'. file revision 2 > stored 1 (unsynced); digest fields [milestone.owners]. Do: revert [milestone.owners] in docs/010-test.md and run contextunity-forge-mcp task sync."
        )
    );
    assert!(!error.contains("task.contract_revision"));
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
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nmilestones: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
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
    let before = store.inspect_details(&id).unwrap();
    let maximum: u64 = store
        .connection
        .query_row(
            "SELECT revision FROM task_revisions WHERE task_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    drop(store);
    let keeper_ref = format!("refs/forge/state/forge-mcp/{}", id.replace(':', "/"));
    let checkpoint =
        contextunity_forge_mcp::db::tasks_store::recovery::read_checkpoint(&root.0, &keeper_ref)
            .unwrap();
    let keeper = contextunity_forge_mcp::core::git::git_stdout(
        &root.0,
        &["rev-parse", &keeper_ref],
        "FIXTURE_KEEPER",
    )
    .unwrap();
    for (gate_stage, case) in ["contract", "review"].into_iter().flat_map(|stage| {
        [
            "stage",
            "null",
            "empty",
            "unknown",
            "contract",
            "raw-contract",
            "worker",
            "worktree",
            "claim",
            "duplicate-claim",
            "action",
            "result",
        ]
        .into_iter()
        .map(move |case| (stage, case))
    }) {
        let mut state = serde_json::to_value(&checkpoint).unwrap();
        let gate = state["gates"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| {
                row[0] == gate_stage && matches!(row[2].as_str(), Some("passed" | "rejected"))
            })
            .unwrap();
        let revision = gate[1].clone();
        let mut submitted_evidence: Value =
            serde_json::from_str(gate[3].as_str().unwrap()).unwrap();
        match case {
            "stage" => submitted_evidence["stage"] = json!("deliver"),
            "null" => submitted_evidence["proof"] = Value::Null,
            "empty" => submitted_evidence["proof"] = json!({}),
            "unknown" => submitted_evidence["proof"] = json!({"unknown_proof":{}}),
            "contract" | "raw-contract" => submitted_evidence["contract_revision"] = json!(999),
            "worker" => submitted_evidence["worker_id"] = json!("foreign"),
            "worktree" => submitted_evidence["worktree"] = json!("/tmp/foreign"),
            "claim" => submitted_evidence["claim_revision"] = json!(999),
            _ => (),
        }
        gate[3] = json!(serde_json::to_string(&submitted_evidence).unwrap());
        let submission = state["submissions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row[0] == revision)
            .unwrap();
        let mut value: Value = serde_json::from_str(submission[1].as_str().unwrap()).unwrap();
        value["evidence"] = submitted_evidence;
        if case == "action" {
            value["action"] = json!(if gate_stage == "contract" {
                "reject"
            } else {
                "pass"
            });
        }
        if case == "raw-contract" {
            value["task"]["contract_revision"] = json!(999);
        }
        if case == "result" {
            value["task"]["status"] = json!("in_progress");
        }
        submission[1] = json!(serde_json::to_string(&value).unwrap());
        if case == "duplicate-claim" {
            let duplicate = state["claims"]
                .as_array()
                .unwrap()
                .iter()
                .find(|claim| claim[0] == revision)
                .unwrap()
                .clone();
            state["claims"].as_array_mut().unwrap().push(duplicate);
        }
        contextunity_forge_mcp::core::git::git_checkpoint(
            &root.0,
            &keeper_ref,
            &serde_json::to_vec(&state).unwrap(),
            std::slice::from_ref(&keeper),
        )
        .unwrap();
        let database = root.0.join(".forge/tasks.sqlite");
        let files = [
            database.clone(),
            database.with_file_name("tasks.sqlite-wal"),
            database.with_file_name("tasks.sqlite-shm"),
        ];
        let original = files.each_ref().map(|path| std::fs::read(path).ok());
        for path in &files {
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let refused = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "--format",
                "json",
                "milestone",
                "db-recover",
            ])
            .output()
            .unwrap();
        assert!(
            !refused.status.success(),
            "checkpoint {case} must refuse: {}",
            String::from_utf8_lossy(&refused.stdout)
        );
        assert!(!database.exists());
        for (path, bytes) in files.iter().zip(original) {
            if let Some(bytes) = bytes {
                std::fs::write(path, bytes).unwrap();
            }
        }
        assert!(std::process::Command::new("git")
            .args(["update-ref", &keeper_ref, &keeper])
            .current_dir(&root.0)
            .status()
            .unwrap()
            .success());
    }
    std::fs::remove_file(root.0.join(".forge/tasks.sqlite")).unwrap();
    let recovery = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "--format",
            "json",
            "milestone",
            "db-recover",
        ])
        .output()
        .unwrap();
    assert!(
        recovery.status.success(),
        "{}",
        String::from_utf8_lossy(&recovery.stderr)
    );
    let mut store = tasks::store(&root.0).unwrap();
    assert_eq!(store.inspect_details(&id).unwrap(), before);
    let restored_maximum: u64 = store
        .connection
        .query_row(
            "SELECT revision FROM task_revisions WHERE task_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(restored_maximum >= maximum);
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
    for version in ["1", "2", "3", "4", "99"] {
        let old = root.0.join(format!("schema-{version}.sqlite"));
        let conn = rusqlite::Connection::open(&old).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE task_store_metadata(key TEXT PRIMARY KEY,value TEXT);\
             INSERT INTO task_store_metadata VALUES('schema_version','{version}');\
             CREATE TABLE sentinel(value TEXT NOT NULL);\
             INSERT INTO sentinel VALUES('preserve me');"
        ))
        .unwrap();
        assert!(
            TasksStore::open(&old).is_err(),
            "schema {version} must fail closed"
        );
        let stored_version: String = conn
            .query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_version, version, "schema marker {version} changed");
        let sentinel: String = conn
            .query_row("SELECT value FROM sentinel", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sentinel, "preserve me");
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(tables, ["sentinel", "task_store_metadata"]);
        drop(conn);
        root.write(
            "forge-mcp.yaml",
            &format!("tasks_db: schema-{version}.sqlite\n"),
        );
        let wal = std::path::PathBuf::from(format!("{}-wal", old.display()));
        let shm = std::path::PathBuf::from(format!("{}-shm", old.display()));
        std::fs::write(&wal, b"preserve refused WAL bytes").unwrap();
        std::fs::write(&shm, b"preserve refused SHM bytes").unwrap();
        let before = std::fs::read(&old).unwrap();
        let refused = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "milestone",
                "db-recover",
            ])
            .output()
            .unwrap();
        assert!(
            !refused.status.success(),
            "unsupported schema {version} must not be recovered"
        );
        assert_eq!(std::fs::read(&old).unwrap(), before);
        assert_eq!(std::fs::read(wal).unwrap(), b"preserve refused WAL bytes");
        assert_eq!(std::fs::read(shm).unwrap(), b"preserve refused SHM bytes");
    }

    let unversioned = root.0.join("unversioned.sqlite");
    let conn = rusqlite::Connection::open(&unversioned).unwrap();
    conn.execute_batch(
        "CREATE TABLE old_data(value TEXT); INSERT INTO old_data VALUES('preserve me');",
    )
    .unwrap();
    assert!(TasksStore::open(&unversioned).is_err());
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        tables, 1,
        "unversioned non-empty DB must not be initialized"
    );
    let value: String = conn
        .query_row("SELECT value FROM old_data", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "preserve me");

    let unversioned_view = root.0.join("unversioned-view.sqlite");
    let conn = rusqlite::Connection::open(&unversioned_view).unwrap();
    conn.execute_batch("CREATE VIEW old_projection AS SELECT 1 AS value;")
        .unwrap();
    assert!(TasksStore::open(&unversioned_view).is_err());
    let objects: Vec<(String, String)> = conn
        .prepare(
            "SELECT type,name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(objects, [("view".into(), "old_projection".into())]);

    let current = root.0.join("current.sqlite");
    drop(TasksStore::open(&current).expect("empty DB should initialize as schema v7"));
    let reopened = TasksStore::open(&current).expect("schema v7 should remain supported");
    let version: String = reopened
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "7");
    // Exercise migration from an actual v5 layout, preserving existing data.
    reopened.connection.execute_batch("DROP TABLE milestone_tombstones; UPDATE task_store_metadata SET value='5' WHERE key='schema_version';").unwrap();
    drop(reopened);
    let migrated = TasksStore::open(&current).unwrap();
    let version: String = migrated
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "7");
    let count: i64 = migrated
        .connection
        .query_row("SELECT COUNT(*) FROM milestone_tombstones", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    migrated
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .unwrap();
    drop(TasksStore::open(&current).expect("v6 open must not acquire the writer lock"));
    migrated.connection.execute_batch("ROLLBACK").unwrap();
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
    let task_id = "forge-mcp/m-reopen:reopenable";

    // Advance task through all 4 gates to completed
    let stages = ["contract", "build", "review", "deliver"];
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
        if *stage == "deliver" {
            ev.proof = json!({
                "delivery_proof": {"status": "passed"}
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
    assert_eq!(reopen_res["stage"], "contract");
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
    assert_eq!(sub.status, "ready");

    // Reopened task can be claimed again at contract
    let re_claimed = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
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
fn task_manage_sync_and_list_resolve_milestone_reference_by_id_prefix_stem_and_path() {
    use contextunity_forge_mcp::engine::tasks as engine_tasks;

    let root = ScopedWorkspace::new("forge_milestone_ref_resolution");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\nmilestones: [docs/milestones, contracts]\n",
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
    let list_by_id = engine_tasks::list(
        &root.0,
        engine_tasks::List {
            repository: Some("test".into()),
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
            ..Default::default()
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

    let list_by_prefix = engine_tasks::list(
        &root.0,
        engine_tasks::List {
            repository: None,
            milestone_ref: Some("30".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
            ..Default::default()
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

    let alias_path = "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md";
    let alias_task_ref = "normalize-and-resolve-milestone-references";
    let alias_content = format!(
        "---\nid: m-mcp-response-deduplication-and-agent-efficiency\ntitle: MCP response deduplication\ndoc_type: contract\nstatus: active\ninvariants: []\n---\n```yaml\ntask_ref: {alias_task_ref}\ntarget: Normalize milestone references\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    );
    root.write(alias_path, &alias_content);
    let alias_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(alias_path.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let alias_task_id = alias_sync["tasks"][0]["task_id"].as_str().unwrap();

    let configured_path = "contracts/040-configured-milestone-with-a-long-name.md";
    let configured_content = "---\nid: m-configured-milestone\ntitle: Configured milestone\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: configured-task\ntarget: Resolve configured manifest directory\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(configured_path, configured_content);
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(configured_path.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let archived_path = "docs/milestones/archive/041-archived-milestone-with-a-long-name.md";
    let archived_content = "---\nid: m-archived-milestone\ntitle: Archived milestone\ndoc_type: contract\nstatus: completed\n---\n```yaml\ntask_ref: archived-task\ntarget: Resolve archived manifest path\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(archived_path, archived_content);
    let archived_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(archived_path.into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(format!("{archived_sync:#}").contains("STORE_SYNC_FAILED"));
    // Retained history from an opt-out or an older store still supports list selectors.
    tasks::store(&root.0)
        .unwrap()
        .sync(
            &Milestone::parse(archived_content, "test").unwrap(),
            archived_path,
            &root.0,
        )
        .unwrap();

    let list_for = |reference: &str, repository: Option<&str>| {
        engine_tasks::list(
            &root.0,
            engine_tasks::List {
                repository: repository.map(str::to_owned),
                milestone_ref: Some(reference.into()),
                status: Some(tasks::Status::All),
                ..Default::default()
            },
        )
    };
    for alias in [
        "UX-001",
        "UX-001.md",
        "docs/milestones/UX-001",
        "docs/milestones/UX-001.md",
        "milestones/UX-001.md",
    ] {
        for reference in [alias.to_owned(), format!("/{alias}")] {
            let listed = list_for(&reference, None).unwrap();
            assert_eq!(listed["tasks"].as_array().unwrap().len(), 1, "{reference}");
            assert_eq!(listed["tasks"][0]["task_id"], alias_task_id, "{reference}");
            assert!(
                listed.get("resolution").is_none(),
                "a direct milestone alias must not fabricate a task-fallback warning: {reference}"
            );
        }
    }

    for reference in [alias_task_ref, alias_task_id] {
        let listed = list_for(reference, Some("test")).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1, "{reference}");
        assert_eq!(listed["tasks"][0]["task_id"], alias_task_id);
        assert_eq!(listed["resolution"]["kind"], "task_parent");
        assert_eq!(listed["resolution"]["input"], reference);
        assert_eq!(listed["resolution"]["task_id"], alias_task_id);
        assert_eq!(
            listed["resolution"]["milestone_id"],
            "m-mcp-response-deduplication-and-agent-efficiency"
        );
        assert_eq!(listed["resolution"]["milestone_ref"], alias_path);
    }

    for (reference, task_id) in [
        (
            "m-configured-milestone",
            "test/m-configured-milestone:configured-task",
        ),
        ("040", "test/m-configured-milestone:configured-task"),
        (
            configured_path,
            "test/m-configured-milestone:configured-task",
        ),
        (
            "m-archived-milestone",
            "test/m-archived-milestone:archived-task",
        ),
        ("041", "test/m-archived-milestone:archived-task"),
        (archived_path, "test/m-archived-milestone:archived-task"),
    ] {
        let listed = list_for(reference, None).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1, "{reference}");
        assert_eq!(listed["tasks"][0]["task_id"], task_id, "{reference}");
    }

    let task_slug_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(alias_task_ref.into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        task_slug_sync.to_string().contains("not found"),
        "sync must not reinterpret a task slug as a milestone: {task_slug_sync}"
    );
    assert!(tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: alias_task_ref.into(),
            stage: "contract".into(),
            worker_id: "slug-is-not-a-task-id".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        }
    )
    .is_err());
    let before_slug_delete = contextunity_forge_mcp::db::tasks_store::TasksStore::open(
        &root.0.join(".forge/tasks.sqlite"),
    )
    .unwrap()
    .connection
    .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))
    .unwrap();
    assert!(
        tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Delete,
                milestone_ref: Some(alias_task_ref.into()),
                ..Default::default()
            }
        )
        .is_err(),
        "delete must not reinterpret a task slug as a milestone"
    );
    assert_eq!(
        contextunity_forge_mcp::db::tasks_store::TasksStore::open(
            &root.0.join(".forge/tasks.sqlite"),
        )
        .unwrap()
        .connection
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))
        .unwrap(),
        before_slug_delete,
        "a rejected destructive selector must not change task records"
    );
    for action in [
        tasks::ManageAction::Inspect,
        tasks::ManageAction::Delete,
        tasks::ManageAction::Reset,
        tasks::ManageAction::Reopen,
        tasks::ManageAction::SubtaskList,
    ] {
        assert!(
            tasks::manage(
                &root.0,
                tasks::Manage {
                    action,
                    task_id: Some(alias_task_ref.into()),
                    ..Default::default()
                }
            )
            .is_err(),
            "{action:?} must not reinterpret a task slug as a task ID"
        );
    }
    for reference in ["../../outside/UX-001.md", "/etc/UX-001.md"] {
        assert!(
            list_for(reference, None).is_err(),
            "unsafe or arbitrary absolute paths must not fall back by basename: {reference}"
        );
    }
    let foreign = ScopedWorkspace::new("forge_foreign_milestone_reference");
    let foreign_path = "docs/milestones/UX-001-foreign.md";
    foreign.write(
        foreign_path,
        "---\nid: m-foreign\ntitle: Foreign\ndoc_type: contract\n---\n",
    );
    let foreign_absolute = foreign.0.join(foreign_path);
    assert!(
        list_for(&foreign_absolute.to_string_lossy(), None).is_err(),
        "a foreign workspace path must not be reinterpreted as its basename"
    );

    let ambiguous = ScopedWorkspace::new("forge_ambiguous_milestone_alias");
    ambiguous.write(
        "forge-mcp.yaml",
        "roots: []\nmilestones: [contracts, proposals]\n",
    );
    for (path, id) in [
        ("contracts/UX-001-primary.md", "m-primary-alias"),
        ("proposals/UX-001-secondary.md", "m-secondary-alias"),
    ] {
        ambiguous.write(
            path,
            &format!("---\nid: {id}\ntitle: Alias candidate\ndoc_type: contract\n---\n"),
        );
    }
    let ambiguous_error = engine_tasks::list(
        &ambiguous.0,
        engine_tasks::List {
            milestone_ref: Some("UX-001".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        ambiguous_error.contains("ambiguous")
            && ambiguous_error.contains("contracts/UX-001-primary.md")
            && ambiguous_error.contains("proposals/UX-001-secondary.md"),
        "normalized milestone aliases must fail closed with both candidates: {ambiguous_error}"
    );

    let primary = ScopedWorkspace::new("forge_task_parent_primary");
    let linked = ScopedWorkspace::new("forge_task_parent_linked");
    primary.write(
        "forge-mcp.yaml",
        &format!(
            "task_repository: primary\ntask_project: primary\nroots: []\nlinked_workspaces:\n  - name: linked\n    path: {}\n    tasks: {{enabled: true}}\n",
            linked.0.display()
        ),
    );
    linked.write(
        "forge-mcp.yaml",
        "task_repository: linked\ntask_project: linked\nroots: []\n",
    );
    let primary_milestone = "---\nid: m-direct-match\ntitle: Primary\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: shared-task\ntarget: Primary shared task\nproof_policy: direct-proof\nscope: [src/]\n```\n```yaml\ntask_ref: primary-only\ntarget: Primary task for direct milestone match\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    primary.write(
        "docs/milestones/010-primary-long-name.md",
        primary_milestone,
    );
    let linked_milestone = "---\nid: m-linked\ntitle: Linked\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: shared-task\ntarget: Linked shared task\nproof_policy: direct-proof\nscope: [src/]\n```\n```yaml\ntask_ref: m-direct-match\ntarget: Linked task whose slug resembles a milestone ID\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    linked.write("docs/milestones/020-linked-long-name.md", linked_milestone);
    let primary_sync = tasks::manage(
        &primary.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("primary".into()),
            milestone_ref: Some("docs/milestones/010-primary-long-name.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let linked_sync = tasks::manage(
        &primary.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("linked".into()),
            milestone_ref: Some("docs/milestones/020-linked-long-name.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let primary_shared_id = primary_sync["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["spec"]["task_ref"] == "shared-task")
        .unwrap()["task_id"]
        .as_str()
        .unwrap();
    let linked_shared_id = linked_sync["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["spec"]["task_ref"] == "shared-task")
        .unwrap()["task_id"]
        .as_str()
        .unwrap();
    let unique_slug = engine_tasks::list(
        &primary.0,
        engine_tasks::List {
            repository: Some("primary".into()),
            milestone_ref: Some("shared-task".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unique_slug["tasks"].as_array().unwrap().len(), 2);
    assert!(unique_slug["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_id"] == primary_shared_id));
    assert_eq!(unique_slug["resolution"]["kind"], "task_parent");
    assert_eq!(unique_slug["resolution"]["task_id"], primary_shared_id);
    let linked_full_id = engine_tasks::list(
        &primary.0,
        engine_tasks::List {
            repository: Some("linked".into()),
            milestone_ref: Some(linked_shared_id.into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(linked_full_id["tasks"].as_array().unwrap().len(), 2);
    assert!(linked_full_id["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_id"] == linked_shared_id));
    assert_eq!(linked_full_id["resolution"]["input"], linked_shared_id);
    primary.write(
        "docs/milestones/011-primary-second.md",
        "---\nid: m-primary-second\ntitle: Another primary milestone\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: shared-task\ntarget: Same slug in another milestone\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    let second_primary_sync = tasks::manage(
        &primary.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("primary".into()),
            milestone_ref: Some("docs/milestones/011-primary-second.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let second_primary_task_id = second_primary_sync["tasks"][0]["task_id"].as_str().unwrap();
    let same_workspace_collision = engine_tasks::list(
        &primary.0,
        engine_tasks::List {
            repository: Some("primary".into()),
            milestone_ref: Some("shared-task".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        same_workspace_collision.contains("ambiguous")
            && same_workspace_collision.contains(primary_shared_id)
            && same_workspace_collision.contains(second_primary_task_id),
        "task slug collisions within one workspace must report both parents: {same_workspace_collision}"
    );
    let all_workspace_collision = engine_tasks::list(
        &primary.0,
        engine_tasks::List {
            repository: Some("all".into()),
            milestone_ref: Some("shared-task".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        all_workspace_collision.contains("ambiguous")
            && all_workspace_collision.contains(primary_shared_id)
            && all_workspace_collision.contains(second_primary_task_id)
            && all_workspace_collision.contains(linked_shared_id),
        "task slug collisions across selected workspaces must report candidates: {all_workspace_collision}"
    );
    let unknown_task_selector = engine_tasks::list(
        &primary.0,
        engine_tasks::List {
            repository: Some("all".into()),
            milestone_ref: Some("missing-task".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        unknown_task_selector.contains("not found"),
        "unknown task selectors remain errors: {unknown_task_selector}"
    );
    let direct_milestone = engine_tasks::list(
        &primary.0,
        engine_tasks::List {
            repository: Some("all".into()),
            milestone_ref: Some("m-direct-match".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(direct_milestone["tasks"].as_array().unwrap().len(), 2);
    let primary_only_id = primary_sync["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["spec"]["task_ref"] == "primary-only")
        .unwrap()["task_id"]
        .as_str()
        .unwrap();
    let direct_task_ids = direct_milestone["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(direct_task_ids.contains(&primary_only_id));
    assert!(direct_task_ids.contains(&primary_shared_id));
    assert!(!direct_task_ids.contains(&"linked/linked:m-direct-match"));
    assert!(
        direct_milestone.get("resolution").is_none(),
        "an existing m-... milestone must win over an equal task slug"
    );

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

    root.write(
        "contracts/UX-001.md",
        "---\nid: [invalid\ntitle: broken\n---\n",
    );
    let invalid_manifest = list_for("contracts/UX-001.md", None)
        .unwrap_err()
        .to_string();
    assert!(
        invalid_manifest.contains("invalid") || invalid_manifest.contains("parse"),
        "an existing invalid manifest must fail instead of falling back by basename: {invalid_manifest}"
    );
    root.write("docs/milestones/UX-001.md", "---\nid: [invalid\n---\n");
    let invalid_alias_manifest = list_for("docs/milestones/UX-001.md", None)
        .unwrap_err()
        .to_string();
    assert!(
        invalid_alias_manifest.contains("invalid") || invalid_alias_manifest.contains("parse"),
        "an invalid existing alias path must not fall back to a similar manifest: {invalid_alias_manifest}"
    );
}

#[test]
fn task_list_filters_milestone_status_and_controls_subtask_details() {
    use contextunity_forge_mcp::engine::tasks;

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
    assert_eq!(tasks_arr[0]["task_id"], "forge-mcp/m-active:active-task");

    let default_ready = tasks::list(&root.0, tasks::List::default()).unwrap();
    assert_eq!(default_ready["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(default_ready["tasks"][0]["status"], "ready");

    let cli_page = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--format",
            "json",
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "list",
            "--status",
            "all",
            "--limit",
            "1",
            "--offset",
            "0",
        ])
        .output()
        .unwrap();
    assert!(
        cli_page.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_page.stderr)
    );
    let cli_page: serde_json::Value = serde_json::from_slice(&cli_page.stdout).unwrap();
    assert_eq!(cli_page["pagination"]["total"], 1);
    assert_eq!(cli_page["pagination"]["limit"], 1);
    assert_eq!(cli_page["pagination"]["offset"], 0);
    assert_eq!(cli_page["pagination"]["returned"], 1);
    assert_eq!(cli_page["pagination"]["has_more"], false);
    assert!(!cli_page["pagination"]["generation"]
        .as_str()
        .unwrap()
        .is_empty());

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
            "forge-mcp/m-active:active-task",
        ),
        (
            tasks::MilestoneStatusFilter::Planned,
            "forge-mcp/m-planned:planned-task",
        ),
        (
            tasks::MilestoneStatusFilter::Completed,
            "forge-mcp/m-archived:archived-task",
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
    assert!(all_ids.contains(&"forge-mcp/m-cancelled:cancelled-task"));

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
        "forge-mcp/m-archived:archived-task"
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
        "forge-mcp/m-cancelled:cancelled-task"
    );

    root.write(
        "docs/milestones/010-active.md",
        &active_spec.replace("status: active", "status: completed"),
    );
    let no_active = tasks::list(&root.0, tasks::List::default()).unwrap();
    assert!(no_active["tasks"].as_array().unwrap().is_empty());

    root.write("docs/milestones/010-active.md", active_spec);
    let second_active_spec = "---\nid: m-second-active\ntitle: Second Active Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: second-active-task\ntarget: Second active task target\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let second_active_path = "docs/milestones/011-second-active.md";
    root.write(second_active_path, second_active_spec);
    store
        .sync(
            &Milestone::parse(second_active_spec, "forge-mcp").unwrap(),
            second_active_path,
            &root.0,
        )
        .unwrap();
    let ambiguity = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(ambiguity.contains("010-active"), "{ambiguity}");
    assert!(ambiguity.contains("011-second-active"), "{ambiguity}");
}

#[test]
fn cross_milestone_dependency_diagnostic_gates_readiness_and_claim() {
    let root = ScopedWorkspace::new("forge_cross_milestone");
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let m1_text = "---\nid: m-first\ntitle: First\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# First\n```yaml\ntask_ref: producer\ntarget: Deliver producer\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let m2_text = "---\nid: m-second\ntitle: Second\ndoc_type: contract\nstatus: planned\ndepends_on: [m-first]\ninvariants: [isolated]\n---\n# Second\n```yaml\ntask_ref: consumer\ntarget: Deliver consumer\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [\"m-first:producer\"]\n```\n";
    let m1 = Milestone::parse(m1_text, "forge-mcp").unwrap();
    let m2 = Milestone::parse(m2_text, "forge-mcp").unwrap();
    root.write("src/lib.rs", "pub fn f() {}\n");
    root.write("docs/milestones/m1.md", m1_text);
    root.write("docs/milestones/m2.md", m2_text);

    store.sync(&m1, "docs/milestones/m1.md", &root.0).unwrap();
    store.sync(&m2, "docs/milestones/m2.md", &root.0).unwrap();

    let producer_id = m1.task_id(&m1.tasks[0]);
    let consumer_id = m2.task_id(&m2.tasks[0]);

    let tasks = store.list(None, "all", None).unwrap();
    let producer_task = tasks.iter().find(|t| t.task_id == producer_id).unwrap();
    let consumer_task = tasks.iter().find(|t| t.task_id == consumer_id).unwrap();
    assert_eq!(producer_task.status, "ready");
    assert_eq!(consumer_task.status, "blocked");

    // Persist the computed blocked state so the claim seam can exercise status precedence.
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'blocked') WHERE task_id = ?1",
            [&consumer_id],
        )
        .unwrap();

    // Attempting to claim consumer should fail because its status is not ready.
    let worktree = root.0.to_string_lossy().into_owned();
    let status_err = store
        .claim(&consumer_id, "contract", "worker-1", &worktree)
        .unwrap_err();
    let status_message = status_err.to_string();

    // Exercise the dependency-specific diagnostic after status becomes ready.
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'ready') WHERE task_id = ?1",
            [&consumer_id],
        )
        .unwrap();
    let dependency_err = store
        .claim(&consumer_id, "contract", "worker-1", &worktree)
        .unwrap_err();
    let dependency_message = dependency_err.to_string();
    let expected_status_message = format!(
        "TASK_NOT_READY: task '{consumer_id}'. status is 'blocked' (active stage 'contract'). Do: claim only ready tasks at their active stage."
    );
    let expected_dependency_message = format!(
        "TASK_NOT_READY: task '{consumer_id}' has unsatisfied dependencies: [{producer_id}]. Do: complete dependencies before claiming stage 'contract'."
    );
    assert!(
        status_message == expected_status_message
            && dependency_message == expected_dependency_message,
        "unexpected TASK_NOT_READY diagnostics; status: {status_message:?}; dependencies: {dependency_message:?}"
    );

    // Complete producer task
    store.connection.execute(
        "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'completed') WHERE task_id = ?1",
        [&producer_id],
    ).unwrap();

    // Now consumer should be ready!
    let tasks_after = store.list(None, "all", None).unwrap();
    let consumer_after = tasks_after
        .iter()
        .find(|t| t.task_id == consumer_id)
        .unwrap();
    assert_eq!(consumer_after.status, "ready");

    // And claiming consumer should now succeed!
    let claimed = store
        .claim(&consumer_id, "contract", "worker-1", &worktree)
        .unwrap();
    assert_eq!(claimed.task_id, consumer_id);
    assert_eq!(claimed.status, "in_progress");

    // Archived predecessors unlock newly synced consumers without bridge rows.
    let mut authority_failures = Vec::new();
    for (root_mode, (authority, dependency, expected_ready)) in [
        ("tombstone", "m-first:producer", true),
        ("tombstone", "m-first:cancelled", false),
        ("tombstone", "m-first", true),
        ("nonterminal-tombstone", "m-first", false),
        ("nonterminal-tombstone", "m-first:producer", true),
        ("foreign-tombstone", "m-first:producer", false),
        ("archive", "m-first:producer", true),
        ("archive", "m-first:cancelled", false),
        ("archive", "m-first", true),
        ("archive", "other/m-first:producer", false),
        ("alias-archive", "old/m-first:producer", true),
        ("late-archive", "m-first:producer", true),
        ("torn-archive", "m-first:producer", false),
        ("active-archive", "m-first:producer", false),
        ("nonterminal-archive", "m-first:producer", false),
        ("undated-archive", "m-first:producer", false),
        ("foreign-archive", "m-first:producer", false),
    ]
    .into_iter()
    .flat_map(|case| ["default", "wildcard", "concrete"].map(|mode| (mode, case)))
    {
        let root = ScopedWorkspace::new("forge_archived_dependency");
        root.write("src/lib.rs", "pub fn f() {}\n");
        root.write(
            "forge-mcp.yaml",
            "task_project: old\naliases: {old: forge-mcp, forge-mcp: other}\n",
        );
        let nested = root_mode != "default";
        if nested {
            let dir = if root_mode == "wildcard" {
                "extensions/*/docs/milestones"
            } else {
                "extensions/base/docs/milestones"
            };
            root.write("forge-mcp.yaml", &format!("task_project: old\nmilestones: [docs/milestones, '{dir}']\nowners: {{extensions/base: old}}\naliases: {{old: forge-mcp, forge-mcp: other}}\n"));
        }
        let mut deferred_archive = None;
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        if authority.ends_with("tombstone") {
            let identity = if authority == "foreign-tombstone" {
                "forge-mcp/other/m-first"
            } else {
                "forge-mcp/forge-mcp/m-first"
            };
            store
                .connection
                .execute(
                    "INSERT INTO milestone_tombstones VALUES(?1,0,?2,?3)",
                    rusqlite::params![
                        identity,
                        authority != "nonterminal-tombstone",
                        "[\"forge-mcp/m-first:producer\"]"
                    ],
                )
                .unwrap();
        } else {
            let archive = "---\nid: m-first\nstatus: completed\nhandoff:\n  completed_at: 2026-10-01T10:00:00Z\n  commit: 0123456789012345678901234567890123456789\n---\n```yaml\ntask_ref: producer\ntarget: Archived producer\nstatus: completed\nscope: [src/]\n```\n```yaml\ntask_ref: cancelled\ntarget: Transferred work\nstatus: cancelled\nscope: [other/]\n```\n";
            let archive = archive
                .replace(
                    "target: Archived producer\n",
                    "target: Archived producer\nproof_policy: seam-test-first\n",
                )
                .replace(
                    "target: Transferred work\n",
                    "target: Transferred work\nproof_policy: seam-test-first\n",
                );
            let archive = match authority {
                "torn-archive" => "---\nid: [torn".to_owned(),
                "active-archive" => archive.replacen("status: completed", "status: active", 1),
                "nonterminal-archive" => archive.replace("status: cancelled", "status: ready"),
                "undated-archive" => archive.replace("2026-10-01T10:00:00Z", "not-a-date"),
                "alias-archive" => archive.replace("id: m-first", "id: m-first\nproject: old"),
                "foreign-archive" => archive.replace("id: m-first", "id: m-other"),
                _ => archive,
            };
            if authority == "late-archive" {
                deferred_archive = Some(archive);
            } else {
                root.write(
                    if nested {
                        "extensions/base/docs/milestones/archive/m-first.md"
                    } else {
                        "docs/milestones/archive/m-first.md"
                    },
                    &archive,
                );
            }
        }
        let consumer = m2_text.replace("m-first:producer", dependency);
        let milestone = Milestone::parse(&consumer, "forge-mcp").unwrap();
        root.write("docs/milestones/m2.md", &consumer);
        store
            .sync(&milestone, "docs/milestones/m2.md", &root.0)
            .unwrap();
        if let Some(archive) = deferred_archive {
            root.write(
                if nested {
                    "extensions/base/docs/milestones/archive/m-first.md"
                } else {
                    "docs/milestones/archive/m-first.md"
                },
                &archive,
            );
        }
        let tasks = store.list(None, "all", None).unwrap();
        assert_eq!(
            tasks.len(),
            1,
            "{root_mode}/{authority}/{dependency}: no bridge tasks"
        );
        let claim = store.claim(
            &tasks[0].task_id,
            "contract",
            "archived-consumer",
            root.0.to_str().unwrap(),
        );
        if (tasks[0].status == "ready") != expected_ready || claim.is_ok() != expected_ready {
            authority_failures.push(format!(
                "{root_mode}/{authority}/{dependency}: status={}, claim={claim:?}",
                tasks[0].status
            ));
        }
    }
    assert!(
        authority_failures.is_empty(),
        "archive authority matrix: {authority_failures:?}"
    );
}

#[test]
fn task_manage_selector_diagnostics_report_fields_before_resolution() {
    let root = ScopedWorkspace::new("forge_selector_diagnostics");
    let cases = [
        (
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                ..Default::default()
            },
            "inspect",
            "task_id",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                task_id: Some("forge-mcp/m-missing:task".into()),
                milestone_ref: Some("docs/010-test.md".into()),
                ..Default::default()
            },
            "inspect",
            "milestone_ref",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                ..Default::default()
            },
            "sync",
            "milestone_ref, workspace",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::ExtendScope,
                task_id: Some("forge-mcp/m-test:first".into()),
                paths: Some(Vec::new()),
                ..Default::default()
            },
            "extend_scope",
            "paths",
        ),
    ];

    for (request, action, fields) in cases {
        let selector = request
            .task_id
            .as_deref()
            .or(request.milestone_ref.as_deref())
            .unwrap_or("unselected")
            .to_owned();
        let error = tasks::manage(&root.0, request).unwrap_err().to_string();
        assert_eq!(
            error,
            format!(
                "TASK_SELECTOR_INVALID: {selector}. action '{action}'. missing or invalid fields: [{fields}]. Do: pass required fields for '{action}'."
            )
        );
    }
}

#[test]
fn task_store_diagnostics_cover_not_found_stale_claim_and_submission_precedence() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let missing_id = "forge-mcp/m-test:missing";
    let expected_not_found = format!(
        "TASK_NOT_FOUND: task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
    );
    assert_eq!(
        store.inspect(missing_id).unwrap_err().to_string(),
        expected_not_found
    );
    let not_found_evidence = Evidence {
        task_id: missing_id.into(),
        stage: "contract".into(),
        claim_revision: 1,
        contract_revision: 1,
        worker_id: "diagnostic-builder".into(),
        worktree: root.0.to_string_lossy().into_owned(),
        commit: None,
        candidate_baseline_head: None,
        proof: json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}}),
    };
    assert_eq!(
        store
            .claim(
                missing_id,
                "contract",
                "diagnostic-builder",
                &not_found_evidence.worktree
            )
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        store.reset(missing_id).unwrap_err().to_string(),
        expected_not_found
    );
    assert_eq!(
        store
            .release_failed_claim(missing_id, 1)
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        store
            .extend_scope(missing_id, &["src/other.rs".into()], &root.0)
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        store
            .submit(missing_id, "contract", &not_found_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        expected_not_found
    );
    assert_eq!(
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: missing_id.into(),
                stage: "contract".into(),
                worker_id: "diagnostic-builder".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_NOT_FOUND: {missing_id}. task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
        )
    );
    for action in [
        tasks::ManageAction::Inspect,
        tasks::ManageAction::ExtendScope,
    ] {
        let error = tasks::manage(
            &root.0,
            tasks::Manage {
                action,
                task_id: Some(missing_id.into()),
                paths: (action == tasks::ManageAction::ExtendScope)
                    .then(|| vec!["src/other.rs".into()]),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            error,
            format!(
                "TASK_NOT_FOUND: {missing_id}. task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
            )
        );
    }
    assert_eq!(
        tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Create,
                milestone_ref: Some("docs/010-test.md".into()),
                task_ref: Some("missing".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_NOT_FOUND: docs/010-test.md. task '{missing_id}' not found in tasks database. Do: verify task id and sync milestone."
        )
    );

    let ready = store.inspect(&id).unwrap();
    assert_eq!(
        store
            .release_failed_claim(&id, ready.claim_revision)
            .unwrap_err()
            .to_string(),
        format!("TASK_STALE_CLAIM: task '{id}'. status is 'ready'. Do: leave the claim unchanged.")
    );

    let worktree = root.0.to_string_lossy().into_owned();
    let claimed = store
        .claim(&id, "contract", "diagnostic-builder", &worktree)
        .unwrap();
    let wrong_revision = claimed.claim_revision + 1;
    assert_eq!(
        store
            .release_failed_claim(&id, wrong_revision)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_CLAIM: task '{id}'. active claim_revision {} != requested {wrong_revision}. Do: release claim_revision {}.",
            claimed.claim_revision, claimed.claim_revision
        )
    );
    store
        .connection
        .execute(
            "UPDATE task_claims SET ended = 1 WHERE task_id = ?1 AND revision = ?2",
            rusqlite::params![id, claimed.claim_revision],
        )
        .unwrap();
    assert_eq!(
        store
            .release_failed_claim(&id, claimed.claim_revision)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_CLAIM: task '{id}'. claim_revision {} has no open claim row. Do: claim the task again before modifying it.",
            claimed.claim_revision
        )
    );
    store.reset(&id).unwrap();
    drop(store);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let mut active = store
        .claim(&id, "contract", "diagnostic-builder", &worktree)
        .unwrap();
    let mut stage_evidence = evidence(&active);
    stage_evidence.stage = "build".into();
    assert_eq!(
        store
            .submit(&id, "contract", &stage_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STAGE_INVALID: task '{id}'. evidence.stage 'build' != submitted stage 'contract'. Do: set evidence.stage to 'contract'."
        )
    );

    let mut empty_commit = evidence(&active);
    empty_commit.commit = Some(" \t".into());
    assert_eq!(
        store
            .submit(&id, "contract", &empty_commit, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{id}' for stage 'contract'. commit field is empty. Do: set commit to candidate snapshot unavailable."
        )
    );

    for (value, expected) in [
        (
            Value::Null,
            format!(
                "TASK_EVIDENCE_INVALID: {id}. task '{id}'. evidence is not a valid JSON object. Do: pass evidence as a JSON object."
            ),
        ),
        (
            json!({}),
            format!(
                "TASK_EVIDENCE_INVALID: {id}. task '{id}'. evidence JSON does not match the evidence object. Do: pass the evidence fields required by stage 'contract'."
            ),
        ),
    ] {
        assert_eq!(
            tasks::submit(
                &root.0,
                tasks::Submit {
                    task_id: id.clone(),
                    stage: "contract".into(),
                    action: tasks::Action::Pass,
                    evidence: value,
                    findings: None,
                }
            )
            .unwrap_err()
            .to_string(),
            expected
        );
    }
    let mut submitted_evidence = evidence(&active);
    submitted_evidence.stage = "build".into();
    assert_eq!(
        store
            .submit(&id, "build", &submitted_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STAGE_INVALID: task '{id}'. submitted stage 'build' does not match active stage 'contract'. Do: submit for active stage 'contract'."
        )
    );

    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'ready') WHERE task_id = ?1",
            [&id],
        )
        .unwrap();
    let ready_with_claim = store.inspect(&id).unwrap();
    let ready_evidence = evidence(&ready_with_claim);
    assert_eq!(
        store
            .submit(&id, "contract", &ready_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_SUBMISSION: task '{id}'. status is 'ready'. Do: claim the task before submitting."
        )
    );
    store
        .connection
        .execute(
            "UPDATE tasks SET descriptor = json_set(descriptor, '$.status', 'in_progress') WHERE task_id = ?1",
            [&id],
        )
        .unwrap();
    active = store.inspect(&id).unwrap();

    let active_revision = active.claim_revision;
    let active_contract_revision = active.contract_revision;
    let active_worker = active.worker_id.clone().unwrap();
    let active_worktree = active.worktree.clone().unwrap();
    let cases = vec![
        {
            let mut supplied = evidence(&active);
            supplied.claim_revision += 1;
            (
                "claim_revision",
                supplied.claim_revision.to_string(),
                active_revision.to_string(),
                supplied,
            )
        },
        {
            let mut supplied = evidence(&active);
            supplied.contract_revision += 1;
            (
                "contract_revision",
                supplied.contract_revision.to_string(),
                active_contract_revision.to_string(),
                supplied,
            )
        },
        {
            let mut supplied = evidence(&active);
            supplied.task_id = "forge-mcp/m-other:task".into();
            ("task_id", supplied.task_id.clone(), id.clone(), supplied)
        },
        {
            let mut supplied = evidence(&active);
            supplied.worker_id = "other-worker".into();
            (
                "worker_id",
                supplied.worker_id.clone(),
                active_worker,
                supplied,
            )
        },
        {
            let mut supplied = evidence(&active);
            supplied.worktree = "/other/worktree".into();
            (
                "worktree",
                supplied.worktree.clone(),
                active_worktree,
                supplied,
            )
        },
    ];
    for (field, evidence_value, active_value, supplied) in cases {
        assert_eq!(
            store
                .submit(&id, "contract", &supplied, "pass", None)
                .unwrap_err()
                .to_string(),
            format!(
                "TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {evidence_value} != active {active_value}. Do: resubmit with active {field} {active_value}."
            )
        );
    }

    let mut invalid_proof = evidence(&active);
    invalid_proof.proof = Value::Null;
    assert_eq!(
        store
            .submit(&id, "contract", &invalid_proof, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{id}' for stage 'contract'. proof is null. Do: provide non-empty proof for stage 'contract'."
        )
    );

    let accepted = evidence(&active);
    store
        .submit(&id, "contract", &accepted, "pass", None)
        .unwrap();
    let mut different = accepted;
    different.proof =
        json!({"contract_proof":{"seam_test_ref":"tests/acdd/other.rs","red_exit_code":101}});
    assert_eq!(
        store
            .submit(&id, "contract", &different, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_STALE_SUBMISSION: task '{id}'. claim_revision {} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {}.",
            active.claim_revision, active.claim_revision
        )
    );
}

#[test]
fn delivery_rejects_commit_that_differs_from_candidate_when_auto_commit_is_disabled() {
    let root = ScopedWorkspace::new("forge_delivery_candidate_diagnostic");
    root.write("src/lib.rs", "pub fn candidate() -> bool { true }\n");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n",
    );
    root.write(".gitignore", super::CANONICAL_FORGE_GITIGNORE);
    root.write(
        "custom_profile.yaml",
        r#"
gates:
  - id: contract
    role: contract_author
    proof: contract
  - id: build
    role: builder
    proof: command
    sha_snapshot: true
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: build
    independent_from: build
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    review_sources: [review]
    reject_to: build
    auto_commit: false
"#,
    );
    root.write(
        "docs/010-candidate.md",
        "---\nid: m-candidate\ntitle: Candidate diagnostics\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Candidate diagnostics\n```yaml\ntask_ref: candidate\ntarget: Verify delivery candidate diagnostics\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("docs/010-candidate.md".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let task_id = "forge-mcp/m-candidate:candidate";
    for (stage, worker) in [
        ("contract", "author"),
        ("build", "builder"),
        ("review", "reviewer"),
    ] {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.into(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::store_for_task(&root.0, task_id)
            .unwrap()
            .inspect(task_id)
            .unwrap();
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: stage.into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(evidence(&task)).unwrap(),
                findings: None,
            },
        )
        .unwrap();
    }
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: "delivery".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut store = tasks::store_for_task(&root.0, task_id).unwrap();
    let task = store.inspect(task_id).unwrap();
    let (candidate_snapshot, baseline) = store.build_snapshot_candidate(task_id).unwrap().unwrap();
    let mut supplied = evidence(&task);
    supplied.commit = Some("caller-selected-commit".into());
    supplied.candidate_baseline_head = Some(baseline);
    assert_eq!(
        store
            .submit(task_id, "deliver", &supplied, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {candidate_snapshot}. Do: resubmit with commit {candidate_snapshot}."
        )
    );
}
