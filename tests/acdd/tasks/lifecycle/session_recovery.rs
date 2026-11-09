use super::*;

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
        .claim_gate_in_session("review", "builder", Some("fresh-review-session"))
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
