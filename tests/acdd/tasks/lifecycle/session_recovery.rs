use super::*;

#[test]
fn production_cli_session_fallback_controls_physical_review_independence() {
    let manifest = MilestoneFixtureBuilder::new("m-cli-session-fallback")
        .task("independent", &["src/"])
        .task_proof_policy("direct-proof")
        .build();
    let mut profile = fixture_profile();
    profile.gates = serde_yaml::from_str(
        "- id: build\n  role: builder\n  proof: none\n- id: review\n  role: independent_reviewer\n  proof: review\n  independent_from: build\n  reject_to: build\n  contours: guidance.contours\n- id: finish\n  role: builder\n  proof: delivery\n  review_sources: [review]\n  auto_commit: false\n",
    )
    .unwrap();
    profile.validate().unwrap();
    let harness =
        TaskHarness::disk_with_profile(&manifest, "docs/010-cli-session-fallback.md", profile);
    let worktree = harness.root().to_string_lossy().into_owned();
    let builder_session = "cli-session-alpha";

    let build_claim = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", worktree.as_str(), "--format", "json", "task"])
        .args([
            "claim",
            harness.id.as_str(),
            "--stage",
            "build",
            "--worker",
            "builder",
            "--worktree",
            worktree.as_str(),
        ])
        .env("FORGE_AGENT_SESSION_ID", builder_session)
        .current_dir(harness.root())
        .output()
        .unwrap();
    assert!(
        build_claim.status.success(),
        "claim stdout: {}\nclaim stderr: {}",
        String::from_utf8_lossy(&build_claim.stdout),
        String::from_utf8_lossy(&build_claim.stderr)
    );
    let build = harness.store.inspect(&harness.id).unwrap();
    assert_eq!(build.stage, "build");
    assert_eq!(build.status, "in_progress");
    assert_eq!(build.worker_id.as_deref(), Some("builder"));
    assert_eq!(
        harness.persisted_claim_session(build.claim_revision),
        builder_session
    );

    let build_evidence = harness.evidence_for_gate(&build).unwrap();
    let build_evidence_json = serde_json::to_string(&build_evidence).unwrap();
    let build_submit = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", worktree.as_str(), "--format", "json", "task"])
        .args([
            "submit",
            harness.id.as_str(),
            "--stage",
            "build",
            "--action",
            "pass",
            "--evidence",
            build_evidence_json.as_str(),
        ])
        .env("FORGE_AGENT_SESSION_ID", builder_session)
        .current_dir(harness.root())
        .output()
        .unwrap();
    assert!(
        build_submit.status.success(),
        "submit stdout: {}\nsubmit stderr: {}",
        String::from_utf8_lossy(&build_submit.stdout),
        String::from_utf8_lossy(&build_submit.stderr)
    );
    let ready_review = harness.store.inspect(&harness.id).unwrap();
    assert_eq!(ready_review.stage, "review");
    assert_eq!(ready_review.status, "ready");
    assert_eq!(
        harness.persisted_gate_evidence("build")["worker_id"],
        "builder"
    );

    let same_session_claim =
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", worktree.as_str(), "--format", "json", "task"])
            .args([
                "claim",
                harness.id.as_str(),
                "--stage",
                "review",
                "--worker",
                "independent_reviewer",
                "--worktree",
                worktree.as_str(),
            ])
            .env("FORGE_AGENT_SESSION_ID", builder_session)
            .current_dir(harness.root())
            .output()
            .unwrap();
    assert!(!same_session_claim.status.success());
    let same_session_error = format!(
        "{}\n{}",
        String::from_utf8_lossy(&same_session_claim.stdout),
        String::from_utf8_lossy(&same_session_claim.stderr)
    );
    assert!(
        same_session_error.contains("TASK_REVIEW_NOT_INDEPENDENT"),
        "{same_session_error}"
    );
    assert!(same_session_error.contains("build"), "{same_session_error}");
    assert!(
        same_session_error.contains("review"),
        "{same_session_error}"
    );
    assert!(
        same_session_error.contains(builder_session),
        "{same_session_error}"
    );
    let still_ready = harness.store.inspect(&harness.id).unwrap();
    assert_eq!(still_ready.stage, "review");
    assert_eq!(still_ready.status, "ready");
    assert_eq!(still_ready.claim_revision, ready_review.claim_revision);

    let reviewer_session = "cli-session-beta";
    let review_claim = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", worktree.as_str(), "--format", "json", "task"])
        .args([
            "claim",
            harness.id.as_str(),
            "--stage",
            "review",
            "--worker",
            "independent_reviewer",
            "--worktree",
            worktree.as_str(),
        ])
        .env("FORGE_AGENT_SESSION_ID", reviewer_session)
        .current_dir(harness.root())
        .output()
        .unwrap();
    assert!(
        review_claim.status.success(),
        "claim stdout: {}\nclaim stderr: {}",
        String::from_utf8_lossy(&review_claim.stdout),
        String::from_utf8_lossy(&review_claim.stderr)
    );
    let review = harness.store.inspect(&harness.id).unwrap();
    assert_eq!(review.stage, "review");
    assert_eq!(review.status, "in_progress");
    assert_eq!(review.worker_id.as_deref(), Some("independent_reviewer"));
    assert_eq!(
        harness.persisted_claim_session(review.claim_revision),
        reviewer_session
    );

    let review_evidence = harness.evidence_for_gate(&review).unwrap();
    let review_evidence_json = serde_json::to_string(&review_evidence).unwrap();
    let review_submit = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", worktree.as_str(), "--format", "json", "task"])
        .args([
            "submit",
            harness.id.as_str(),
            "--stage",
            "review",
            "--action",
            "pass",
            "--evidence",
            review_evidence_json.as_str(),
        ])
        .env("FORGE_AGENT_SESSION_ID", reviewer_session)
        .current_dir(harness.root())
        .output()
        .unwrap();
    assert!(
        review_submit.status.success(),
        "submit stdout: {}\nsubmit stderr: {}",
        String::from_utf8_lossy(&review_submit.stdout),
        String::from_utf8_lossy(&review_submit.stderr)
    );
    let finish = harness.store.inspect(&harness.id).unwrap();
    assert_eq!(finish.stage, "finish");
    assert_eq!(finish.status, "ready");
    assert_eq!(
        harness.persisted_gate_evidence("review")["worker_id"],
        "independent_reviewer"
    );
    assert_eq!(
        harness.persisted_claim_session(review.claim_revision),
        reviewer_session
    );
}

#[test]
fn review_rejection_retains_findings_and_requires_a_new_build() {
    let (root, store, milestone) = fixture(DEFAULT_SEAM);
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nmilestones: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let id = milestone.task_id(&milestone.tasks[0]);
    drop(store);
    let mut harness = TaskHarness::attach(&root, &id);
    harness.advance_to("review").unwrap();
    let builder_session = TaskHarness::session_for("builder");
    let error = harness
        .claim_next_with_session(Some(&builder_session))
        .unwrap_err();
    assert!(error.to_string().contains("TASK_REVIEW_NOT_INDEPENDENT"));
    let reviewer = harness.worker_for_stage("review").unwrap();
    let reviewer_session = TaskHarness::session_for(&reviewer);
    let claimed = harness
        .claim_next_with_session(Some(&reviewer_session))
        .unwrap();
    let findings = json!({"paths":["src/lib.rs"],"decision":"remediate"});
    let mut review = TaskHarness::default_evidence(&claimed);
    review.proof = review_proof("reject");
    review.commit = harness
        .persisted_gate_evidence("build")
        .get("commit")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let rejected = harness
        .store
        .submit(
            &id,
            "review",
            &review,
            "reject",
            Some(&findings),
            Some(&TaskHarness::session_for(&review.worker_id)),
        )
        .unwrap();
    assert_eq!(rejected.gate, 1);
    assert_eq!(rejected.status, "ready");
    assert_eq!(
        harness.store.inspect_details(&id).unwrap()["findings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let rebuilt = harness
        .claim_next_with_session(Some(&builder_session))
        .unwrap();
    let before = harness.store.inspect_details(&id).unwrap();
    let maximum: u64 = harness
        .store
        .connection
        .query_row(
            "SELECT revision FROM task_revisions WHERE task_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    drop(harness);
    let keeper_ref = format!("refs/forge/state/forge-mcp/{}", id.replace(':', "/"));
    let checkpoint = contextunity_forge_mcp::db::tasks_store::recovery::read_checkpoint(
        root.root(),
        &keeper_ref,
    )
    .unwrap();
    let keeper = contextunity_forge_mcp::core::git::git_stdout(
        root.root(),
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
            root.root(),
            &keeper_ref,
            &serde_json::to_vec(&state).unwrap(),
            std::slice::from_ref(&keeper),
        )
        .unwrap();
        let database = root.root().join(".forge/tasks.sqlite");
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
        let refused = TaskHarness::run_cli_for_root(
            root.root(),
            &["--format", "json", "milestone", "db-recover"],
        );
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
        assert!(
            TaskHarness::update_reference(root.root(), &keeper_ref, &keeper, None)
                .status
                .success()
        );
    }
    std::fs::remove_file(root.root().join(".forge/tasks.sqlite")).unwrap();
    let recovery = TaskHarness::run_cli_for_root(
        root.root(),
        &["--format", "json", "milestone", "db-recover"],
    );
    assert!(
        recovery.status.success(),
        "{}",
        String::from_utf8_lossy(&recovery.stderr)
    );
    let mut store = tasks::store_for_task(root.root(), &id).unwrap();
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
        .submit(
            &id,
            "build",
            &TaskHarness::default_evidence(&rebuilt),
            "pass",
            None,
            Some(&TaskHarness::session_for(
                rebuilt.worker_id.as_deref().unwrap(),
            )),
        )
        .unwrap();
    assert_eq!(store.inspect(&id).unwrap().gate, 2);
}
