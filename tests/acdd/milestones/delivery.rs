use super::*;
use contextunity_forge_mcp::core::tasks::profile::ProofKind;

#[test]
fn milestone_handoff_prunes_only_its_gate_telemetry_identity() {
    let root = MilestoneHarness::configured_workspace();
    root.write("src/lib.rs", "pub fn fixture() {}\n");
    let old_ref = "docs/milestones/010-gate-telemetry.md";
    let new_ref = "docs/milestones/archive/010-gate-telemetry.md";
    let manifest = MilestoneFixtureBuilder::new("m_test")
        .title("Gate telemetry pruning")
        .without_started_at()
        .task("cancelled", &["src/lib.rs"])
        .task_target("Preserve cancelled task history through handoff")
        .task_status("cancelled")
        .build();
    root.write(old_ref, &manifest);

    let (milestone, mut store) = root.sync_document(old_ref, &manifest);

    let target_identity = format!("forge-mcp/forge-mcp/{}", milestone.id);
    let neighboring_identity = "forge-mcp/forge-mcp/m-test";
    for (identity, label) in [
        (target_identity.as_str(), "target"),
        (neighboring_identity, "neighbor"),
    ] {
        let transition_id = store
            .record_gate_transition(&GateTransition {
                milestone_id: identity.into(),
                subject: format!("{label}-task"),
                gate_path: "tasks.build".into(),
                action: "advance".into(),
                proof_kind: "command".into(),
                proof_payload: json!({"exit_code": 0}),
                git_tree_sha: "fixture-tree".into(),
                worker_id: "fixture-worker".into(),
            })
            .unwrap();
        store
            .record_execution_trace(&GateExecutionTrace {
                milestone_id: identity.into(),
                subject: format!("{label}-task"),
                gate_path: "tasks.build".into(),
                attempt_id: 1,
                command_id: format!("{label}-build"),
                command_str: "cargo test --test acdd".into(),
                status: "completed".into(),
                exit_code: Some(0),
                log_path: None,
                output_snippet: None,
                stdout_sha256: None,
                duration_ms: 1,
            })
            .unwrap();
        store
            .save_checkpoint(&GateCheckpoint {
                breadcrumb: format!("{identity}/tasks.build/{label}-task/build"),
                milestone_id: identity.into(),
                subject: format!("{label}-task"),
                gate_path: "tasks.build".into(),
                transition_id: Some(transition_id),
                git_tree_sha: "fixture-tree".into(),
                state_payload: json!({"stage": "build"}),
            })
            .unwrap();
    }

    let expected_tasks = store.list(None, "all", None).unwrap();
    assert_eq!(expected_tasks.len(), 1);
    TasksStore::handoff_milestone(
        &mut store,
        &format!("forge-mcp/{}:", milestone.id),
        old_ref,
        new_ref,
        &expected_tasks,
        &std::collections::BTreeMap::new(),
    )
    .unwrap();

    for table in [
        "gate_checkpoints",
        "gate_execution_traces",
        "gate_transitions",
    ] {
        for (identity, expected_count) in [(target_identity.as_str(), 0), (neighboring_identity, 1)]
        {
            let count = MilestoneHarness::milestone_rows(&store, table, identity).unwrap();
            assert_eq!(count, expected_count, "{table} for {identity}");
        }
    }
    assert!(store.milestone_tombstoned(&milestone.id).unwrap());
    assert!(!store.milestone_tombstoned("m-test").unwrap());
}

#[test]
fn manual_delivery_handoff_rewrites_markdown_receipt_to_landed_commit() {
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn manual_delivery() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom/manual.yaml\n",
    );
    let mut profile = fixture_profile();
    profile.find_gate_mut("deliver").unwrap().auto_commit = false;
    root.write(
        "custom/manual.yaml",
        &serde_yaml::to_string(&profile).unwrap(),
    );
    let source = MilestoneFixtureBuilder::new("m-manual")
        .title("Manual delivery")
        .task("finish", &["src/"])
        .task_target("Finish manually delivered work")
        .build();
    root.write("docs/milestones/010-manual.md", &source);
    root.commit_fixture_inputs(&[
        "forge-mcp.yaml",
        "custom/manual.yaml",
        "docs/milestones/010-manual.md",
    ]);
    let (milestone, store) = root.sync_document("docs/milestones/010-manual.md", &source);
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let baseline = root.head();
    root.complete_task(CompletionMode::Store {
        store: &store,
        task_id: &task_id,
    })
    .unwrap();

    let task = store.inspect(&task_id).unwrap();
    let candidate = task
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .expect("manual delivery receipt retains its accepted candidate SHA");
    let (snapshot_candidate, candidate_baseline) = store
        .build_snapshot_candidate(&task_id)
        .unwrap()
        .expect("delivery retains the captured snapshot candidate and its baseline");
    assert_eq!(candidate, snapshot_candidate);
    let candidate_parents = root.candidate_parents(candidate).unwrap();
    assert!(
        candidate_parents.is_empty(),
        "captured candidates are parentless root snapshots"
    );
    assert_eq!(
        candidate_baseline, baseline,
        "the accepted snapshot already stores the branch baseline needed by handoff"
    );
    assert_eq!(
        root.head(),
        baseline,
        "manual delivery leaves HEAD unchanged"
    );

    let staged = root.stage_paths(&["src/lib.rs", "docs/milestones/010-manual.md"]);
    assert!(staged.status.success());
    let commit = root.commit_staged("manual task landing");
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let landed_commit = root.head();
    drop(store);

    let evidence = root.default_fixture_evidence("m-manual", "cargo test --test acdd", 1, 0);
    root.handoff_at("m-manual", Some(&landed_commit), &evidence)
        .unwrap();

    let archive = root.document("docs/milestones/archive/010-manual.md");
    let archived = Milestone::parse(&archive, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(landed_commit.as_str()),
        "handoff must persist the landed Git commit into Markdown before archival"
    );
    let reopened = root.store();
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
}

#[test]
fn terminal_task_delivery_rolls_up_durable_context_and_prunes_blackboard() {
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let source = SPEC
        .replace(
            "task_ref: first\n",
            "task_ref: first\ninvariants: [first-rule]\n",
        )
        .replace(
            "invariants: [isolated]\n",
            "invariants: [isolated]\ndeferred_defects:\n  - id: DEFECT-RECEIPT\n    source: task_blackboard\n    title: Keep this typed finding across receipt writes\n    path: src/engine/tasks.rs\n    disposition: deferred\n    notes: Persist milestone-level defect metadata.\n",
        );
    let milestone_ref = "docs/milestones/010-test.md";
    root.write(milestone_ref, &source);
    let (milestone, store) = root.sync_document(milestone_ref, &source);
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let candidate_baseline = root.head();

    let mut task_flow = TaskHarness::attach(&root, &first);
    loop {
        if store.inspect(&first).unwrap().status == "completed" {
            break;
        }
        let claimed = task_flow.claim_next().unwrap();
        let stage = claimed.stage.as_str();
        let profile = task_flow.profile().unwrap();
        let kind = profile.by_stage(stage).unwrap().proof.kind();
        let mut proof = task_flow.evidence_for_gate(&claimed).unwrap();
        proof.commit = None;
        if kind == Some(ProofKind::Delivery) {
            assert!(profile.by_stage(stage).unwrap().auto_commit);
            store
                .blackboard_post(
                    &first,
                    "architect",
                    "decisions",
                    "Keep durable task outcomes in the milestone receipt.",
                )
                .unwrap();
            store
                .blackboard_post(&first, "builder", "notes", "temporary trace")
                .unwrap();
            store
                .blackboard_post(&second, "sibling", "notes", "other task context")
                .unwrap();
            root.write(
                milestone_ref,
                &source.replace("target: Deliver first", "target: Changed without admission"),
            );
            let rejected: tasks::Submit = TaskHarness::pass_request(&first, stage, &proof);
            assert!(root.submit_request(rejected).is_err());
            assert_eq!(store.inspect(&first).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&first, None, None).unwrap().len(), 2);
            root.write(milestone_ref, &source);
        }
        if kind == Some(ProofKind::Delivery) {
            root.write("src/head-moved-after-candidate.rs", "pub fn moved() {}\n");
            root.stage_paths(&["src/head-moved-after-candidate.rs"]);
            let committed = root.commit_staged("move HEAD after candidate capture");
            assert!(
                committed.status.success(),
                "{}",
                String::from_utf8_lossy(&committed.stderr)
            );
            let moved_head = root.head();
            assert_ne!(moved_head, candidate_baseline);
            root.write("src/staged-sentinel.rs", "pub fn staged() {}\n");
            root.stage_paths(&["src/staged-sentinel.rs"]);
            let staged_before = root.staged_changes().stdout;
            let receipt_before = root.file_bytes(milestone_ref);
            let task_before = store.inspect(&first).unwrap();
            let submissions_before: i64 =
                MilestoneHarness::task_rows(&store, "task_submissions", &first).unwrap();
            let rejected: tasks::Submit = TaskHarness::pass_request(&first, stage, &proof);
            let error = root.submit_request(rejected).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("TASK_CANDIDATE_BASELINE_MISMATCH"),
                "{error:#}"
            );
            assert_eq!(root.head(), moved_head);
            assert_eq!(root.staged_changes().stdout, staged_before);
            assert_eq!(root.file_bytes(milestone_ref), receipt_before);
            let task_after = store.inspect(&first).unwrap();
            assert_eq!(task_after.status, task_before.status);
            assert_eq!(task_after.stage, task_before.stage);
            assert_eq!(task_after.claim_revision, task_before.claim_revision);
            assert!(task_after.receipt.is_none());
            let submissions_after: i64 =
                MilestoneHarness::task_rows(&store, "task_submissions", &first).unwrap();
            assert_eq!(submissions_after, submissions_before);

            let reset = root.reset_history_to(&candidate_baseline);
            assert!(
                reset.status.success(),
                "{}",
                String::from_utf8_lossy(&reset.stderr)
            );
            assert_eq!(root.head(), candidate_baseline);
        }
        let request: tasks::Submit = TaskHarness::pass_request(&first, stage, &proof);
        let result = root.submit_request(request).unwrap();
        if kind == Some(ProofKind::Delivery) {
            assert_eq!(result["status"], "completed");
        }
    }

    let delivered = store.inspect(&first).unwrap();
    let receipt = serde_json::to_value(delivered.receipt.unwrap()).unwrap();
    assert_eq!(receipt["commit"], root.head());
    assert_eq!(
        receipt["rollup"]["verified_invariants"],
        json!(["isolated", "first-rule"])
    );
    assert_eq!(
        receipt["rollup"]["architectural_notes"],
        json!(["decisions: Keep durable task outcomes in the milestone receipt."])
    );
    assert_eq!(receipt["rollup"]["review_summary"]["decision"], "pass");
    for contour in contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS {
        assert_eq!(
            receipt["rollup"]["review_summary"]["contours"][contour],
            "accepted"
        );
    }
    let written = root.document(milestone_ref);
    let parsed = Milestone::parse(&written, "forge-mcp").unwrap();
    assert_eq!(parsed.deferred_defects.len(), 1);
    assert_eq!(parsed.deferred_defects[0].id, "DEFECT-RECEIPT");
    assert_eq!(
        parsed.deferred_defects[0].title,
        "Keep this typed finding across receipt writes"
    );
    assert_eq!(
        parsed.deferred_defects[0].notes.as_deref(),
        Some("Persist milestone-level defect metadata.")
    );
    let persisted = parsed
        .tasks
        .iter()
        .find(|task| task.task_ref == "first")
        .unwrap();
    assert_eq!(persisted.status.as_deref(), Some("completed"));
    let document_receipt = persisted.receipt.as_ref().unwrap();
    assert!(document_receipt.commit.is_none());
    let mut database_receipt = receipt.clone();
    database_receipt.as_object_mut().unwrap().remove("commit");
    assert_eq!(
        serde_json::to_value(document_receipt).unwrap(),
        database_receipt
    );
    assert!(store
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 1);
    let reopened = root.store();
    let closed = reopened
        .blackboard_post(&first, "late-worker", "notes", "after delivery")
        .unwrap_err();
    assert_eq!(closed.to_string(), "TASK_TERMINAL");
    assert!(reopened
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());

    let mut accepted_build = json!(null);
    let mut accepted_review = json!(null);
    let mut task_flow = TaskHarness::attach(&root, &second);
    loop {
        if store.inspect(&second).unwrap().status == "completed" {
            break;
        }
        let claimed = task_flow.claim_next().unwrap();
        let stage = claimed.stage.as_str();
        let profile = task_flow.profile().unwrap();
        let kind = profile.by_stage(stage).unwrap().proof.kind();
        let proof = task_flow.evidence_for_gate(&claimed).unwrap();
        if kind == Some(ProofKind::Command) {
            accepted_build = proof.proof.clone();
        }
        if kind == Some(ProofKind::Review) {
            accepted_review = proof.proof.clone();
        }
        if kind == Some(ProofKind::Delivery) {
            store
                .blackboard_post(&second, "architect", "decisions", "Retain retry notes.")
                .unwrap();
            let current = root.document(milestone_ref);
            let parsed = Milestone::parse(&current, "forge-mcp").unwrap();
            let mut recovered = Receipt {
                delivery_gate: None,
                candidate_gate: None,
                commit: None,
                contract_revision: 1,
                passed_at: "2026-10-02T00:00:00Z".into(),
                evidence: accepted_build.clone(),
                review: accepted_review.clone(),
                decision: "pass".into(),
                rollup: Some(ReceiptRollup {
                    verified_invariants: vec!["isolated".into()],
                    architectural_notes: vec!["decisions: Retain retry notes.".into()],
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
            recovered.rollup.as_mut().unwrap().architectural_notes[0] =
                "decisions: stale note".into();
            root.write(milestone_ref, &recovery_text(&recovered));
            let rejected: tasks::Submit = TaskHarness::pass_request(&second, stage, &proof);
            assert!(root.submit_request(rejected).is_err());
            assert_eq!(store.inspect(&second).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 2);
            recovered.rollup.as_mut().unwrap().architectural_notes[0] =
                "decisions: Retain retry notes.".into();
            root.write(milestone_ref, &recovery_text(&recovered));
        }
        let request: tasks::Submit = TaskHarness::pass_request(&second, stage, &proof);
        let completed = root.submit_request(request).unwrap();
        if kind == Some(ProofKind::Delivery) {
            assert_eq!(completed["status"], "completed");
            assert_eq!(completed["receipt"]["passed_at"], "2026-10-02T00:00:00Z");
            assert!(store
                .blackboard_read(&second, None, None)
                .unwrap()
                .is_empty());
        }
    }
}

#[test]
fn terminal_delivery_checks_custom_review_rollup_and_stale_candidates() {
    let contract = MilestoneFixtureBuilder::new("m-custom-review")
        .title("Custom review rollup")
        .without_started_at()
        .invariants(&["custom-rollup"])
        .task("custom", &["src/"])
        .task_target("Deliver a final reviewed candidate");
    let custom = MilestoneHarness::review_workflow("docs/010-custom-review.md", contract, false);
    let custom_id = "forge-mcp/m-custom-review:custom";
    let custom_baseline = custom.head();
    let mut build_candidate: Option<String> = None;
    let mut final_candidate: Option<String> = None;
    let mut task = TaskHarness::attach(&custom, custom_id);
    for _ in task.executable_gates().unwrap() {
        let claimed = task.claim_next().unwrap();
        let stage = claimed.stage.as_str();
        if stage == "candidate_final" {
            custom.write("src/lib.rs", "pub fn candidate() { let _version = 2; }\n");
        }
        let mut proof = evidence(&claimed);
        match stage {
            "review" | "security_review" => proof.proof = passing_review_proof(),
            _ => {}
        }
        if stage == "review" {
            let expected = final_candidate
                .as_ref()
                .expect("the configured nearest snapshot gate must precede reviews");
            let first_candidate = build_candidate.as_ref().unwrap();
            assert_ne!(
                first_candidate, expected,
                "the final snapshot must replace the prior candidate"
            );
            let mut wrong_candidate = proof.clone();
            wrong_candidate.commit = Some(first_candidate.clone());
            let rejected: tasks::Submit =
                TaskHarness::pass_request(custom_id, stage, &wrong_candidate);
            let error = custom.submit_request(rejected).unwrap_err();
            assert!(
                error.to_string().contains("TASK_CANDIDATE_MISMATCH"),
                "{error:#}"
            );
            let unchanged = custom.read_task(custom_id);
            assert_eq!(unchanged.status, "in_progress");
            assert_eq!(unchanged.stage, "review");
            assert_eq!(unchanged.claim_revision, claimed.claim_revision);
        }
        if stage == "security_review" {
            proof.commit = Some(
                final_candidate
                    .as_ref()
                    .expect("security review uses the final candidate")
                    .clone(),
            );
        }
        let request: tasks::Submit = TaskHarness::pass_request(custom_id, stage, &proof);
        custom.submit_request(request).unwrap();
        if stage == "build" {
            build_candidate = Some(pinned_candidate(custom.root(), custom_id));
        }
        if stage == "candidate_final" {
            final_candidate = Some(pinned_candidate(custom.root(), custom_id));
            assert_ne!(final_candidate.as_deref(), Some(custom_baseline.as_str()));
        }
    }
    let candidate = final_candidate.unwrap();
    let custom_store =
        contextunity_forge_mcp::engine::tasks::store_for_task(custom.root(), custom_id).unwrap();
    let delivered = custom_store.inspect(custom_id).unwrap();
    let receipt = serde_json::to_value(delivered.receipt.unwrap()).unwrap();
    assert_eq!(
        receipt["evidence"]["command_proof"]["command"],
        "rust-check-and-test"
    );
    assert_eq!(
        receipt["review"]["review_sources"]["review"]["review_proof"]["decision"],
        "pass"
    );
    assert_eq!(
        receipt["review"]["review_sources"]["security_review"]["review_proof"]["decision"],
        "pass"
    );
    assert_eq!(
        receipt["rollup"]["review_summary"]["contours"]["review.paths"],
        "accepted"
    );
    assert_eq!(
        receipt["rollup"]["review_summary"]["contours"]["security_review.paths"],
        "accepted"
    );
    let stored_reviews = MilestoneHarness::stored_reviews(&custom_store, custom_id);
    assert_eq!(stored_reviews.len(), 2);
    for (_, evidence) in stored_reviews {
        assert_eq!(evidence["commit"], candidate);
    }
    let reopened = custom.read_task(custom_id);
    assert_eq!(
        serde_json::to_value(reopened.receipt.unwrap()).unwrap(),
        receipt
    );
}

#[test]
fn terminal_delivery_rejects_reviews_of_an_earlier_candidate() {
    let stale_ref = "docs/010-stale-review.md";
    let contract = MilestoneFixtureBuilder::new("m-stale-review")
        .title("Stale review source")
        .without_started_at()
        .task("stale", &["src/"])
        .task_target("Reject a stale review source at delivery");
    let stale = MilestoneHarness::review_workflow(stale_ref, contract, true);
    let stale_id = "forge-mcp/m-stale-review:stale";
    let mut stale_build_candidate: Option<String> = None;
    let mut stale_final_candidate: Option<String> = None;
    let mut task = TaskHarness::attach(&stale, stale_id);
    for _ in task.executable_gates().unwrap() {
        let claimed = task.claim_next().unwrap();
        let stage = claimed.stage.as_str();
        if stage == "candidate_final" {
            stale.write("src/lib.rs", "pub fn candidate() { let _version = 2; }\n");
        }
        let mut proof = evidence(&claimed);
        match stage {
            "review" | "security_review" => proof.proof = passing_review_proof(),
            _ => {}
        }
        match stage {
            "review" => {
                proof.commit = Some(
                    stale_build_candidate
                        .as_ref()
                        .expect("the first review follows the build snapshot")
                        .clone(),
                );
            }
            "security_review" => {
                proof.commit = Some(
                    stale_final_candidate
                        .as_ref()
                        .expect("the security review follows the final snapshot")
                        .clone(),
                );
            }
            "deliver" => {
                let store = stale.store();
                let reviews = MilestoneHarness::stored_reviews(&store, stale_id);
                assert_eq!(reviews.len(), 2);
                for (gate, evidence) in reviews {
                    let expected = if gate == "review" {
                        stale_build_candidate.as_ref().unwrap()
                    } else {
                        stale_final_candidate.as_ref().unwrap()
                    };
                    assert_eq!(
                        evidence["commit"].as_str(),
                        Some(expected.as_str()),
                        "{gate} should have passed its then-current snapshot"
                    );
                }

                let stage_file = stale.stage_paths(&["src/lib.rs"]);
                assert!(stage_file.status.success());
                let head_before = stale.head();
                let index_before = stale.staged_changes();
                assert!(index_before.status.success());
                assert!(
                    !index_before.stdout.is_empty(),
                    "the stale-source case should preserve a non-empty staged index"
                );
                let document_before = stale.file_bytes(stale_ref);
                let state_before = (
                    store.inspect_details(stale_id).unwrap(),
                    MilestoneHarness::task_rows(&store, "task_submissions", stale_id).unwrap(),
                );
                let request: tasks::Submit = TaskHarness::pass_request(stale_id, stage, &proof);
                let error = stale.submit_request(request).unwrap_err();
                assert!(
                    error.to_string().contains("TASK_CANDIDATE_MISMATCH")
                        && error.to_string().contains("review source 'review'"),
                    "delivery should identify the stale review source: {error:#}"
                );
                assert_eq!(stale.head(), head_before);
                let index_after = stale.staged_changes();
                assert!(index_after.status.success());
                assert_eq!(index_after.stdout, index_before.stdout);
                assert_eq!(stale.file_bytes(stale_ref), document_before);
                let state_after = (
                    store.inspect_details(stale_id).unwrap(),
                    MilestoneHarness::task_rows(&store, "task_submissions", stale_id).unwrap(),
                );
                assert_eq!(state_after, state_before);
                continue;
            }
            _ => {}
        }
        let request: tasks::Submit = TaskHarness::pass_request(stale_id, stage, &proof);
        stale.submit_request(request).unwrap();
        if stage == "build" {
            stale_build_candidate = Some(pinned_candidate(stale.root(), stale_id));
        }
        if stage == "candidate_final" {
            stale_final_candidate = Some(pinned_candidate(stale.root(), stale_id));
            assert_ne!(stale_build_candidate, stale_final_candidate);
        }
    }
}
