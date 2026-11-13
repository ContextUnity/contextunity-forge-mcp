use super::*;

#[test]
fn delivery_rejects_commit_that_differs_from_candidate_when_auto_commit_is_disabled() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn candidate() -> bool { true }\n");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n",
    );
    root.write(".gitignore", super::super::CANONICAL_FORGE_GITIGNORE);
    TaskHarness::commit_handoff_fixture_inputs(&root, &[".forge/acdd/profile.yaml"]);
    root.write(
        "custom_profile.yaml",
        r#"inherit: none
roles:
  contract_author:
    mode: inline
  builder:
    mode: inline
  independent_reviewer:
    mode: inline
guidance:
  contour.paths: "Verify path boundaries."
  contour.claims: "Verify contract claims."
  contour.concurrency: "Verify concurrency invariants."
  contour.project_isolation: "Verify project isolation."
  contour.administration: "Verify governance."
commands:
  test:
    command: ": --seam-ref tests/acdd/tasks.rs {scope_target} && test -x .forge/runner_fixture && if test -f .forge/runner_fixture_green; then .forge/runner_fixture; else FORGE_FIXTURE_RED=1 .forge/runner_fixture --nocapture; fi"
    output:
      require_counts: false
      seam_ref_pattern: '--seam-ref (\S+)'
  rust-check-and-test:
    command: ": {scope_target} && test -x .forge/runner_fixture && .forge/runner_fixture"
    output:
      require_counts: true
      counts: {passed: '(\d+) passed', failed: '(\d+) failed'}
gates:
  - id: contract
    role: contract_author
    proof: contract
    command: test
  - id: build
    role: builder
    proof: command
    command: rust-check-and-test
    sha_snapshot: true
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: build
    independent_from: build
    contours: guidance.contours
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
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("docs/010-candidate.md".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let task_id = "forge-mcp/m-candidate:candidate";
    let mut harness = TaskHarness::attach(&root, task_id);
    harness.advance_to("deliver").unwrap();
    harness.claim_next().unwrap();

    let mut store = tasks::store_for_task(root.root(), task_id).unwrap();
    let task = store.inspect(task_id).unwrap();
    let (candidate_snapshot, baseline) = store.build_snapshot_candidate(task_id).unwrap().unwrap();
    let mut supplied = TaskHarness::default_evidence(&task);
    supplied.commit = Some("caller-selected-commit".into());
    supplied.candidate_baseline_head = Some(baseline);
    assert_eq!(
        store
            .submit(task_id, "deliver", &supplied, "pass", None, Some(&TaskHarness::session_for(&supplied.worker_id)))
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {candidate_snapshot}. Do: resubmit with commit {candidate_snapshot}."
        )
    );
}

#[test]
fn on_demand_checkpoint_cannot_override_immutable_candidate_or_claim_checkpoint() {
    let (root, store, _milestone) = fixture(DEFAULT_SEAM);
    let task_id = "forge-mcp/m-test:first";
    drop(store);

    let mut harness = TaskHarness::attach(&root, task_id);
    harness.advance_to("build").unwrap();
    harness.claim_next().unwrap();
    root.write("src/lib.rs", "pub fn updated() {}\n");
    harness.submit_pass().unwrap();
    drop(harness);
    let claim_res = TaskHarness::claim_at(root.root(), task_id, "review", "reviewer").unwrap();
    let review_breadcrumb = claim_res["breadcrumb"].as_str().unwrap();
    let store = tasks::store_for_task(root.root(), task_id).unwrap();

    // Attempting to overwrite the claim checkpoint via save_checkpoint_payload must fail
    let err = store
        .save_checkpoint_payload(
            review_breadcrumb,
            "1111111111111111111111111111111111111111",
            json!({"custom": "data"}),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("TASK_CHECKPOINT_OWNED"),
        "overwriting claim checkpoint must fail closed: {err}"
    );

    // Supplying an on-demand checkpoint under another breadcrumb with the same milestone & subject
    let fake_breadcrumb = "forge-mcp/forge-mcp/m-test/tasks.other/first/custom";
    store
        .save_checkpoint_payload(
            fake_breadcrumb,
            "2222222222222222222222222222222222222222",
            json!({"commit": "3333333333333333333333333333333333333333", "candidate_baseline_head": "4444444444444444444444444444444444444444"}),
        )
        .unwrap();

    // Now tamper with the scoped file during the immutable review gate
    root.write("src/lib.rs", "pub fn tampered() {}\n");

    // Submitting review must detect divergence and reject with TASK_CANDIDATE_MISMATCH
    let task = store.inspect(task_id).unwrap();
    let submit_err = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "review", &TaskHarness::default_evidence(&task)),
    )
    .unwrap_err();
    assert!(
        submit_err.to_string().contains("TASK_CANDIDATE_MISMATCH"),
        "tampering with file during immutable gate must fail with TASK_CANDIDATE_MISMATCH: {submit_err}"
    );
}

#[test]
fn concurrent_pre_snapshot_immutable_gate_rejects_on_demand_checkpoint_races() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(
        ".forge/acdd/profile.yaml",
        r#"inherit: none
version: 2
roles:
  worker:
    mode: inline
gates:
  - id: sample
    role: worker
    proof: none
    persistence:
      pre: snapshot
      immutable: true
  - id: deliver
    role: worker
    proof: delivery
"#,
    );
    root.write("src/lib.rs", "pub fn original() {}\n");
    let [added, committed] = TaskHarness::initial_fixture_commit(root.root());
    assert!(added.status.success(), "{added:?}");
    assert!(committed.status.success(), "{committed:?}");

    let manifest = "---\nid: m-atomic\nstatus: active\ndoc_type: contract\n---\n```yaml\ntask_ref: guarded\ntarget: Test atomic claim protection\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/010-atomic.md", manifest);

    tasks::manage(
        root.root(),
        serde_json::from_value(json!({"action": "sync", "milestone_ref": "docs/010-atomic.md"}))
            .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-atomic:guarded";
    let db_path = root.root().join(".forge/tasks.sqlite");
    let breadcrumb = "forge-mcp/forge-mcp/m-atomic/tasks/guarded/sample".to_owned();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = stop.clone();
    let breadcrumb_clone = breadcrumb.clone();
    let db_path_clone = db_path.clone();

    let attacker = std::thread::spawn(move || {
        let store = TasksStore::open(&db_path_clone).unwrap();
        while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = store.save_checkpoint_payload(
                &breadcrumb_clone,
                "0000000000000000000000000000000000000000",
                json!({"malicious": "replacement"}),
            );
            let _ = store.delete_checkpoint(&breadcrumb_clone);
            std::thread::yield_now();
        }
    });

    let claim_res = TaskHarness::claim_at(root.root(), task_id, "sample", "worker")
        .expect("claiming sample gate should succeed");

    assert_eq!(claim_res["status"], "in_progress");
    assert_eq!(claim_res["breadcrumb"], breadcrumb);

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    attacker.join().unwrap();

    let store = TasksStore::open(&db_path).unwrap();
    let checkpoints = store.list_checkpoints().unwrap();
    let sample_checkpoint = checkpoints
        .iter()
        .find(|cp| cp.breadcrumb == breadcrumb)
        .expect("claim checkpoint must not be deleted by concurrent attacker");

    assert_eq!(
        sample_checkpoint.state_payload["task_id"], task_id,
        "checkpoint must retain engine task_id"
    );
    assert_eq!(
        sample_checkpoint.state_payload["claim_revision"], claim_res["claim_revision"],
        "checkpoint must retain engine claim_revision"
    );

    let save_err = store
        .save_checkpoint_payload(
            &breadcrumb,
            "1111111111111111111111111111111111111111",
            json!({"tamper": true}),
        )
        .unwrap_err();
    assert!(
        save_err.to_string().contains("TASK_CHECKPOINT_OWNED"),
        "overwriting active claim checkpoint must fail closed: {save_err}"
    );

    let del_err = store.delete_checkpoint(&breadcrumb).unwrap_err();
    assert!(
        del_err.to_string().contains("TASK_CHECKPOINT_OWNED"),
        "deleting active claim checkpoint must fail closed: {del_err}"
    );

    root.write("src/lib.rs", "pub fn tampered() {}\n");

    let evidence = contextunity_forge_mcp::core::tasks::gates::Evidence {
        task_id: task_id.into(),
        stage: "sample".into(),
        claim_revision: claim_res["claim_revision"].as_u64().unwrap(),
        contract_revision: claim_res["contract_revision"].as_u64().unwrap(),
        worker_id: "worker".into(),
        worktree: root.root().to_str().unwrap().into(),
        commit: None,
        candidate_baseline_head: None,
        proof: json!({"none_proof": {}}),
    };

    let submit_err = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "sample", &evidence),
    )
    .unwrap_err();
    assert!(
        submit_err.to_string().contains("TASK_CANDIDATE_MISMATCH"),
        "tampering with file during immutable gate must fail closed: {submit_err}"
    );

    root.write("src/lib.rs", "pub fn original() {}\n");
    let submit_ok = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "sample", &evidence),
    )
    .expect("restoring scoped content to pre-snapshot baseline allows submission");
    assert_eq!(submit_ok["status"], "ready");
    assert_eq!(submit_ok["stage"], "deliver");
}
