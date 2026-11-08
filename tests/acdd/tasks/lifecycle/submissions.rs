use super::*;

#[test]
fn task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite() {
    let mut h = TaskHarness::disk();
    {
        let schema = serde_json::to_value(schemars::schema_for!(tasks::Submit)).unwrap();
        assert_eq!(schema["properties"]["evidence"]["type"], "object");
        let proof = serde_json::to_value(unobserved_evidence(&h.claim("contract"))).unwrap();
        let id = &h.id;
        for invalid in [
            json!({"task_id":id,"stage":"contract","action":"pass"}),
            json!({"task_id":id,"stage":"contract","action":"pass","evidence_ref":"proof.yaml"}),
            json!({"task_id":id,"stage":"contract","action":"pass","evidence":proof,"evidence_ref":"proof.yaml"}),
        ] {
            assert!(serde_json::from_value::<tasks::Submit>(invalid).is_err());
            assert_eq!(h.store.inspect(id).unwrap().status, "in_progress");
        }
    }

    h.store.reset(&h.id).unwrap();
    {
        let contract = h.claim_next().unwrap();
        let mut proof = TaskHarness::default_evidence(&contract);
        proof.commit = None;
        let raw_evidence = serde_json::to_value(&proof).unwrap();
        let id = &h.id;
        let nonobject: tasks::Submit = serde_json::from_value(json!({
            "task_id": id, "stage": "contract", "action": "pass", "evidence": 42
        }))
        .unwrap();
        assert!(tasks::submit(h.root(), nonobject).is_err());
        assert_eq!(h.store.inspect(id).unwrap().status, "in_progress");
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id": id, "stage": "contract", "action": "pass", "evidence": raw_evidence
        }))
        .unwrap();
        let submitted = tasks::submit(h.root(), request).unwrap();
        assert_eq!(submitted["status"], "ready");
        let stored = h.persisted_gate_evidence("contract");
        let mut expected = raw_evidence;
        expected["commit"] = stored["commit"].clone();
        expected["candidate_baseline_head"] = stored["candidate_baseline_head"].clone();
        assert_eq!(stored, expected);
        let snapshot_commit = stored["commit"].as_str().unwrap();
        assert_eq!(snapshot_commit.len(), 40);
        assert_eq!(snapshot_commit, submitted["snapshot"]["commit"]);
        assert_eq!(stored["candidate_baseline_head"], git_head(h.root()));
        assert_eq!(
            h.persisted_submission_evidence(proof.claim_revision),
            stored
        );
    }
}

#[test]
fn task_cli_submit_accepts_json_object_and_persists_gate_evidence() {
    use contextunity_forge_mcp::core::tasks::profile::CommandDef;
    let mut profile = fixture_profile();
    profile.commands.insert(
        "task6-cli-check".into(),
        CommandDef::Detailed {
            command: "printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'".into(),
            timeout_seconds: Some(5),
            description: Some("Run the zero-argument CLI proof fixture".into()),
            scope_args_by_pattern: Default::default(),
            output: None,
        },
    );
    profile.find_gate_mut("build").unwrap().command = Some("task6-cli-check".into());
    let mut h = TaskHarness::disk_with_profile(SPEC, "docs/010-test.md", profile);
    let task_id = h.id.clone();
    h.claim_next().unwrap();
    let proof = h.observed_evidence();
    let raw = serde_json::to_string(&proof).unwrap();
    for invalid in ["proof.yaml", "42"] {
        let rejected = h.run_cli_task(&[
            "submit",
            &task_id,
            "--stage",
            "contract",
            "--action",
            "pass",
            "--evidence",
            invalid,
        ]);
        assert!(!rejected.status.success());
        assert_eq!(h.store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let output = h.run_cli_task(&[
        "submit",
        &task_id,
        "--stage",
        "contract",
        "--action",
        "pass",
        "--evidence",
        &raw,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "ready");
    let stored = h.persisted_gate_evidence("contract");
    let mut expected = serde_json::to_value(proof).unwrap();
    expected["commit"] = stored["commit"].clone();
    expected["candidate_baseline_head"] = stored["candidate_baseline_head"].clone();
    assert_eq!(stored, expected);
    let snapshot_commit = stored["commit"].as_str().unwrap();
    assert_eq!(snapshot_commit.len(), 40);
    assert_eq!(snapshot_commit, result["snapshot"]["commit"]);
    assert_eq!(stored["candidate_baseline_head"], git_head(h.root()));
    h.advance_to("build").unwrap();
    h.claim_next().unwrap();
    let task = h.store.inspect(&h.id).unwrap();
    let profile = h.profile().unwrap();
    let (gate_path, gate) = profile
        .executable_gate_paths()
        .into_iter()
        .find(|(path, _)| {
            path == &task.stage || path.rsplit('.').next() == Some(task.stage.as_str())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "TASK_STAGE_UNKNOWN: current stage '{}' is absent from profile",
                task.stage
            )
        })
        .unwrap();
    let gate_path = if gate_path.starts_with("tasks.") {
        gate_path
    } else {
        format!("tasks.{gate_path}")
    };
    let command_name = gate.command.as_deref().unwrap();
    let command = profile.commands.get(command_name).unwrap();
    let trace = contextunity_forge_mcp::engine::milestones::execute_command_gate(
        h.root(),
        &h.id,
        &gate_path,
        command,
        &["src/lib.rs".to_owned()],
    )
    .unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].exit_code, Some(0));

    let output = h.run_cli_task(&["submit", &task_id, "--stage", "build", "--action", "pass"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let automatic: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(automatic["status"], "ready");
    assert_eq!(automatic["stage"], "review");
    let stored = h.persisted_gate_evidence("build");
    assert_eq!(
        stored["proof"]["command_proof"]["command"],
        "task6-cli-check"
    );
    assert_eq!(stored["proof"]["command_proof"]["exit_code"], 0);
    assert_eq!(stored["proof"]["command_proof"]["tests_passed"], 1);
}

#[test]
fn gate_proof_validation_persists_only_the_typed_accepted_payload() {
    let (workspace, mut store, milestone) = fixture(
        "tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite",
    );
    let task_id = milestone.task_id(&milestone.tasks[0]);
    store
        .extend_scope(&task_id, &["src/reviewed.rs".into()], workspace.root())
        .unwrap();
    drop(store);
    let mut h = TaskHarness::attach(&workspace, &task_id);

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
    let proof_cases = [
        (
            "contract",
            json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}}),
            vec![
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":0}}),
                json!({"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}),
                json!({"contract_proof":{"seam_test":"tests/acdd/tasks.rs","red_exit_code":101}}),
            ],
        ),
        (
            "build",
            json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0,"log":"running 1 test\ntest result: ok. 1 passed ✓"}}),
            vec![
                json!({"command_proof":{"command":"rust-check-and-test","exit_code":1,"tests_passed":1,"tests_failed":1}}),
                json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"x".repeat(65_537)}}),
                json!({"test_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":2,"tests_failed":0}}),
                json!({"command":"rust-check-and-test","result":"passed","artifacts":[]}),
                json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":2,"tests_failed":0,"output":"passed"}}),
            ],
        ),
        (
            "review",
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

    for (stage, proof, invalid_cases) in proof_cases {
        let claim = h.claim_next().unwrap();
        assert_eq!(claim.stage, stage, "proof case for {stage}");
        let mut ev = h.evidence_for_gate(&claim).unwrap();
        for (case_index, invalid) in invalid_cases.into_iter().enumerate() {
            ev.proof = invalid;
            assert!(
                h.submit_evidence(&ev).is_err(),
                "{stage} proof case {case_index}"
            );
            assert_eq!(
                h.store.inspect(&h.id).unwrap().status,
                "in_progress",
                "{stage} proof case {case_index}"
            );
        }
        ev.proof = proof.clone();
        assert_eq!(
            h.submit_evidence(&ev).unwrap()["status"],
            "ready",
            "{stage}"
        );
        assert_eq!(h.persisted_gate_proof(stage), proof, "{stage}");
        assert_eq!(
            h.persisted_submission_proof(ev.claim_revision),
            proof,
            "{stage}"
        );
    }
}

#[test]
fn rejected_task_gates_require_typed_proof_and_persist_failing_evidence() {
    use contextunity_forge_mcp::core::tasks::profile::ProofKind;

    let (root, _, milestone) = fixture(DEFAULT_SEAM);
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut harness = TaskHarness::attach(&root, &task_id);
    let gates = harness.executable_gates().unwrap();
    let mut proof_kinds = [ProofKind::Contract, ProofKind::Command, ProofKind::Review];
    proof_kinds.sort_by_key(|proof_kind| {
        gates
            .iter()
            .position(|gate| gate.proof.kind() == Some(*proof_kind))
            .expect("the fixture profile must expose the requested proof kind")
    });
    // Share the persisted task and advance through earlier gates once per proof kind.
    for proof_kind in proof_kinds {
        let definition = gates
            .iter()
            .find(|gate| gate.proof.kind() == Some(proof_kind))
            .expect("the fixture profile must expose the requested proof kind");
        let stage = definition.id.as_str();
        harness.advance_to(stage).unwrap();
        let worker = harness.worker_for_gate(definition);
        let session = TaskHarness::session_for(&worker);
        let claimed = harness
            .claim_gate_in_session(stage, &worker, Some(&session))
            .unwrap();
        let proof_kind = definition.proof.kind();
        let (bare, typed) = match proof_kind {
            Some(ProofKind::Contract) => (
                json!({"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}),
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}}),
            ),
            Some(ProofKind::Command) => (
                json!({"command":"rust-check-and-test","result":"failed","artifacts":[]}),
                json!({"command_proof":{"command":"rust-check-and-test","exit_code":101,"tests_passed":0,"tests_failed":1,"log":"one test failed"}}),
            ),
            Some(ProofKind::Review) => (
                json!({"decision":"reject","contours":{}}),
                review_proof("reject"),
            ),
            other => panic!("expected contract, command, or review proof gate; got {other:?}"),
        };
        let findings = json!({"decision":"remediate"});
        let mut proof = harness.evidence_for_gate(&claimed).unwrap();
        proof.proof = bare;
        assert!(harness
            .submit_rejection_in_session(&proof, Some(&session), Some(&findings))
            .is_err());
        assert_eq!(
            harness.store.inspect(&task_id).unwrap().status,
            "in_progress"
        );
        if matches!(
            proof_kind,
            Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::Review)
        ) {
            for invalid in [
                json!({"review_proof":{"decision":"reject","contours":{}}}),
                passing_review_proof(),
            ] {
                proof.proof = invalid;
                assert!(harness
                    .submit_rejection_in_session(&proof, Some(&session), Some(&findings))
                    .is_err());
                assert_eq!(
                    harness.store.inspect(&task_id).unwrap().status,
                    "in_progress"
                );
            }
        }
        proof.proof = typed.clone();
        let submitted = harness
            .submit_rejection_in_session(&proof, Some(&session), Some(&findings))
            .unwrap();
        assert_eq!(submitted.status, "ready");
        assert_eq!(
            harness
                .persisted_gate_evidence_for_state(stage, "rejected")
                .unwrap()["proof"],
            typed
        );
        assert_eq!(
            harness.persisted_submission_evidence(claimed.claim_revision)["proof"],
            typed
        );
    }
}
