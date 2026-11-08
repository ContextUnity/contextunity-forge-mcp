use super::*;

#[derive(Clone, Copy, Debug)]
enum SharedScenario {
    Shared,
    RepackedShared,
    RepackedWithoutMarkdownCommits,
    UnreviewedShared,
    NonEquivalentRepack,
    MalformedReceipt,
    MissingReceiptCommit,
    UnrelatedReceiptCommit,
    ObsoleteReceiptCommit,
    CorruptedLaterReview,
    ReviewEnvelopeBuilder,
    ReviewEnvelopeOtherWorktree,
    ReviewClaimBuilder,
    ReviewClaimAndEnvelopeBuilder,
    DeliveryEnvelopeOtherWorker,
    RetargetedDeliveryUnreviewedShared,
}

fn shared_scope_baseline() -> DeliveredBaseline {
    let source = MilestoneFixtureBuilder::new("m-shared-landings")
        .title("Reviewed shared landings")
        .task("first", &["src/shared.rs", "src/first.rs"])
        .task_target("Deliver first shared version")
        .task_with_deps("second", &["src/shared.rs", "src/second.rs"], &["first"])
        .task_target("Deliver next reviewed shared version")
        .build();
    delivered(DeliveredProfile {
        milestone_id: "m-shared-landings",
        active_path: "docs/milestones/010-shared-landings.md",
        source: &source,
        config:
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nrecord_task_commits: true\n",
        files: vec![
            ("src/shared.rs", "pub fn shared() -> u8 { 0 }\n".into()),
            ("src/first.rs", "pub fn first() -> u8 { 0 }\n".into()),
            ("src/second.rs", "pub fn second() -> u8 { 0 }\n".into()),
        ],
        seed_paths: &["forge-mcp.yaml", "docs", "src"],
        commit_message: "admit shared milestone",
        disable_git_maintenance: true,
        tasks: vec![
            DeliveredTask {
                task_ref: "first",
                updates: vec![
                    ("src/shared.rs", "pub fn shared() -> u8 { 1 }\n".into()),
                    ("src/first.rs", "pub fn first() -> u8 { 1 }\n".into()),
                ],
                completion: DeliveredCompletion::Default,
            },
            DeliveredTask {
                task_ref: "second",
                updates: vec![
                    ("src/shared.rs", "pub fn shared() -> u8 { 2 }\n".into()),
                    ("src/second.rs", "pub fn second() -> u8 { 2 }\n".into()),
                ],
                completion: DeliveredCompletion::Default,
            },
        ],
        evidence: DeliveredEvidence::Default {
            command: "cargo test --test acdd",
            passed: 1,
            failed: 0,
        },
        capture_sqlite: true,
    })
}

#[test]
fn handoff_validates_reviewed_shared_scope_and_repacked_landings_atomically() {
    check_shared_scope_scenarios(
        &[
            SharedScenario::CorruptedLaterReview,
            SharedScenario::ReviewEnvelopeBuilder,
            SharedScenario::ReviewClaimBuilder,
            SharedScenario::ReviewClaimAndEnvelopeBuilder,
            SharedScenario::DeliveryEnvelopeOtherWorker,
            SharedScenario::Shared,
            SharedScenario::RepackedShared,
        ],
        true,
    );
}

#[test]
fn handoff_validates_receipt_authority_and_repacked_landings_atomically() {
    check_shared_scope_scenarios(
        &[
            SharedScenario::UnreviewedShared,
            SharedScenario::NonEquivalentRepack,
            SharedScenario::MalformedReceipt,
            SharedScenario::MissingReceiptCommit,
            SharedScenario::UnrelatedReceiptCommit,
            SharedScenario::ObsoleteReceiptCommit,
            SharedScenario::ReviewEnvelopeOtherWorktree,
        ],
        true,
    );
}

#[test]
fn handoff_validates_repacked_landings_without_commit_recording_atomically() {
    check_shared_scope_scenarios(
        &[
            SharedScenario::RepackedWithoutMarkdownCommits,
            SharedScenario::RetargetedDeliveryUnreviewedShared,
        ],
        false,
    );
}

fn check_shared_scope_scenarios(scenarios: &[SharedScenario], record_commits: bool) {
    let mut landings = shared_scope_baseline();
    let mut delivered_baseline = landings.base.clone();
    let mut delivered_first = landings.deliveries[0].clone();
    let mut delivered_second = landings.deliveries[1].clone();
    let mut delivered_evidence = landings.evidence.clone();
    if !record_commits {
        landings.root.reset_history_to(&delivered_baseline);
        landings.root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nrecord_task_commits: false\n",
        );
        landings.root.stage_paths(&["forge-mcp.yaml"]);
        landings
            .root
            .commit_staged("select handoff recording policy");
        let no_recording_baseline = landings.root.head();
        landings.root.cherry_pick_commit(&delivered_first);
        delivered_first = landings.root.head();
        landings.root.cherry_pick_commit(&delivered_second);
        delivered_second = landings.root.head();
        let (evidence, observed) = MilestoneHarness::reverify(
            &landings.root,
            "m-shared-landings",
            &no_recording_baseline,
            &delivered_evidence,
        );
        assert_eq!(observed["verification"]["exit_code"], 0);
        assert_eq!(observed["verification"]["tests_failed"], 0);
        delivered_baseline = no_recording_baseline;
        delivered_evidence = evidence;
        landings.snapshot = landings.root.checkpoint();
    }
    let second_id = landings.milestone.task_id(&landings.milestone.tasks[1]);
    let sqlite_delivered = landings.sqlite.take().unwrap();
    let mut failures = Vec::new();
    let active_ref = "docs/milestones/010-shared-landings.md";
    let archive_ref = "docs/milestones/archive/010-shared-landings.md";
    let mut previous_sql_only = false;

    for (index, &scenario) in scenarios.iter().enumerate() {
        let sql_only = matches!(
            scenario,
            SharedScenario::CorruptedLaterReview
                | SharedScenario::ReviewEnvelopeBuilder
                | SharedScenario::ReviewClaimBuilder
                | SharedScenario::ReviewClaimAndEnvelopeBuilder
                | SharedScenario::DeliveryEnvelopeOtherWorker
        );
        let (mut first_landed, mut second_landed, mut integration, baseline) = {
            if index > 0 {
                if sql_only && previous_sql_only {
                    let db = landings.root.path(".forge/tasks.sqlite");
                    for suffix in ["-wal", "-shm", "-journal"] {
                        let auxiliary = landings.root.path(format!(".forge/tasks.sqlite{suffix}"));
                        if auxiliary.exists() {
                            std::fs::remove_file(auxiliary).unwrap();
                        }
                    }
                    std::fs::write(db, &sqlite_delivered).unwrap();
                } else {
                    landings.snapshot.restore(&landings.root);
                }
            }
            previous_sql_only = sql_only;
            (
                delivered_first.clone(),
                delivered_second.clone(),
                delivered_evidence.clone(),
                delivered_baseline.clone(),
            )
        };
        match scenario {
            SharedScenario::RepackedShared | SharedScenario::NonEquivalentRepack => {
                let (first, second, change_first) =
                    if matches!(scenario, SharedScenario::NonEquivalentRepack) {
                        (&delivered_first, &delivered_second, true)
                    } else {
                        (&delivered_first, &delivered_second, false)
                    };
                landings.root.create_branch_at("repacked-fixture", "HEAD");
                landings.root.reset_history_to(first);
                if change_first {
                    landings
                        .root
                        .write("src/first.rs", "pub fn first() -> u8 { 99 }\n");
                    landings.root.stage_paths(&["src/first.rs"]);
                }
                landings.root.amend_message("repacked first task");
                let repacked_first = landings.root.head();
                landings.root.cherry_pick_commit(second);
                first_landed = repacked_first;
                second_landed = landings.root.head();
            }
            SharedScenario::RepackedWithoutMarkdownCommits => {
                landings.root.create_branch_at("repacked-fixture", "HEAD");
                landings.root.reset_history_to(&delivered_first);
                landings.root.amend_message("repacked first task");
                let repacked_first = landings.root.head();
                landings.root.cherry_pick_commit(&delivered_second);
                first_landed = repacked_first;
                second_landed = landings.root.head();
            }
            _ => {}
        }

        let update_unreviewed_shared = || {
            landings
                .root
                .write("src/shared.rs", "pub fn shared() -> u8 { 3 }\n");
            landings.root.stage_paths(&["src/shared.rs"]);
            landings
                .root
                .commit_staged("second: shared scope follow-up");
        };
        let store = landings.root.open_task_store().unwrap();
        let set_gate_field = |gate: &str, field: &str, value: String| {
            MilestoneHarness::set_passed_gate_field(&store, &second_id, gate, field, &value)
                .unwrap()
        };
        let verified_shared_head = if matches!(scenario, SharedScenario::Shared) {
            let original_branch = landings.root.current_branch();
            let before_state =
                landings
                    .root
                    .operation_state(&store, &[active_ref], StateTasks::Listed);
            landings
                .root
                .create_branch_at("unreviewed-shared-fixture", "HEAD");
            update_unreviewed_shared();
            let unreviewed_head = landings.root.head();
            let error = landings
                .root
                .handoff_at("m-shared-landings", Some(&unreviewed_head), &integration)
                .unwrap_err();
            assert!(
                error.to_string().contains("HANDOFF_EVIDENCE_STALE"),
                "{error:#}"
            );
            assert_eq!(
                landings
                    .root
                    .operation_state(&store, &[active_ref], StateTasks::Listed),
                before_state
            );
            assert!(!landings.root.path(archive_ref).exists());
            assert_eq!(landings.root.head(), unreviewed_head);
            assert_eq!(
                landings.root.document("src/shared.rs"),
                "pub fn shared() -> u8 { 3 }\n"
            );
            landings.root.select_revision(&original_branch);
            assert_eq!(landings.root.head(), second_landed);
            assert_eq!(
                landings.root.document("src/shared.rs"),
                "pub fn shared() -> u8 { 2 }\n"
            );
            assert_eq!(
                landings
                    .root
                    .operation_state(&store, &[active_ref], StateTasks::Listed),
                before_state
            );

            let (prior_trace, prior_transition) = MilestoneHarness::milestone_telemetry_ids(&store);
            let (fresh, observed) = MilestoneHarness::reverify(
                &landings.root,
                "m-shared-landings",
                &integration.target_base,
                &integration,
            );
            let check = &observed["verification"];
            assert_eq!(check["exit_code"], 0);
            assert_eq!(check["tests_failed"], 0);
            integration = fresh;
            let VerificationFacts {
                transition_id,
                action,
                payload,
                trace_id,
                status,
                exit_code,
                command,
            } = MilestoneHarness::verification_facts(&store);
            assert!(transition_id > prior_transition);
            assert_eq!(action, "pass");
            assert_eq!(
                payload["implementation_digest"],
                integration.binding.implementation_digest
            );
            assert_eq!(
                payload["target_base_digest"],
                integration.binding.target_base_digest
            );
            assert!(trace_id > prior_trace);
            assert_eq!((status.as_str(), exit_code), ("completed", 0));
            assert_eq!(command, integration.verification.command);
            Some(observed["candidate_head"].as_str().unwrap().to_owned())
        } else {
            None
        };

        if matches!(scenario, SharedScenario::NonEquivalentRepack) {
            integration
                .review
                .contract_findings
                .push("Earlier task outcome changed without an admitted replacement".into());
        }
        if matches!(
            scenario,
            SharedScenario::UnreviewedShared | SharedScenario::RetargetedDeliveryUnreviewedShared
        ) {
            update_unreviewed_shared();
        }
        let invalid_commit = match scenario {
            SharedScenario::MalformedReceipt => Some("not-a-git-commit".to_owned()),
            SharedScenario::MissingReceiptCommit => Some("f".repeat(40)),
            SharedScenario::UnrelatedReceiptCommit => {
                let tree = landings.root.tree_at(&first_landed);
                Some(
                    landings
                        .root
                        .create_navigation_commit(&tree, "unrelated matching tree"),
                )
            }
            SharedScenario::ObsoleteReceiptCommit => Some(baseline),
            _ => None,
        };
        if let Some(invalid_commit) = invalid_commit {
            let document = landings.root.document(active_ref);
            let milestone = Milestone::parse(&document, "forge-mcp").unwrap();
            let task = milestone
                .tasks
                .iter()
                .find(|task| task.task_ref == "first")
                .unwrap();
            let mut receipt = task.receipt.clone().unwrap();
            receipt.commit = Some(invalid_commit.clone());
            let rewritten =
                milestone_engine::render_task_receipt(&document, "first", &receipt, &[]).unwrap();
            landings.root.write(active_ref, &rewritten);
        }
        if matches!(scenario, SharedScenario::CorruptedLaterReview) {
            let changed = MilestoneHarness::overwrite_passed_gate_evidence(
                &store,
                &second_id,
                "review",
                "{corrupted-review",
            )
            .unwrap();
            assert_eq!(changed, 1);
        }
        if matches!(
            scenario,
            SharedScenario::ReviewEnvelopeBuilder | SharedScenario::ReviewClaimAndEnvelopeBuilder
        ) {
            assert_eq!(set_gate_field("review", "worker_id", "builder".into()), 1);
        }
        if matches!(scenario, SharedScenario::ReviewEnvelopeOtherWorktree) {
            let other_worktree = landings.root.path("other-claimed-worktree");
            std::fs::create_dir_all(&other_worktree).unwrap();
            assert_eq!(
                set_gate_field(
                    "review",
                    "worktree",
                    other_worktree.to_string_lossy().into_owned(),
                ),
                1
            );
        }
        if matches!(
            scenario,
            SharedScenario::ReviewClaimBuilder | SharedScenario::ReviewClaimAndEnvelopeBuilder
        ) {
            assert_eq!(
                store
                    .connection
                    .execute(
                        "UPDATE task_claims SET worker_id=?2 WHERE task_id=?1 AND revision=(SELECT revision FROM task_gates WHERE task_id=?1 AND gate='review' AND state='passed' ORDER BY revision DESC LIMIT 1)",
                        rusqlite::params![second_id, "builder"],
                    )
                    .unwrap(),
                1
            );
        }
        if matches!(scenario, SharedScenario::DeliveryEnvelopeOtherWorker) {
            assert_eq!(
                set_gate_field("deliver", "worker_id", "other-delivery-worker".into()),
                1
            );
        }
        let retargeted_head = if matches!(
            scenario,
            SharedScenario::RetargetedDeliveryUnreviewedShared
        ) {
            let generated_doc = landings.root.document(active_ref);
            let generated = Milestone::parse(&generated_doc, "forge-mcp").unwrap();
            assert!(generated.tasks.iter().all(|task| task
                .receipt
                .as_ref()
                .unwrap()
                .commit
                .is_none()));
            let unreviewed_commit = landings.root.head();
            assert_ne!(unreviewed_commit, second_landed);
            assert_eq!(
                set_gate_field("deliver", "commit", unreviewed_commit.clone()),
                1
            );
            assert_eq!(
                    store
                        .connection
                        .execute(
                            "UPDATE tasks SET descriptor=json_set(descriptor, '$.receipt.commit', ?1) WHERE task_id=?2",
                            rusqlite::params![unreviewed_commit, second_id],
                        )
                        .unwrap(),
                    1
                );
            Some(unreviewed_commit)
        } else {
            None
        };
        if matches!(
            scenario,
            SharedScenario::MalformedReceipt
                | SharedScenario::MissingReceiptCommit
                | SharedScenario::UnrelatedReceiptCommit
                | SharedScenario::ObsoleteReceiptCommit
        ) {
            landings.root.stage_paths(&[active_ref]);
            landings.root.commit_staged("navigation input for handoff");
        }
        let head = match scenario {
            SharedScenario::Shared => verified_shared_head.unwrap(),
            SharedScenario::RepackedShared | SharedScenario::RepackedWithoutMarkdownCommits => {
                second_landed.clone()
            }
            SharedScenario::CorruptedLaterReview
            | SharedScenario::ReviewEnvelopeBuilder
            | SharedScenario::ReviewEnvelopeOtherWorktree
            | SharedScenario::ReviewClaimBuilder
            | SharedScenario::ReviewClaimAndEnvelopeBuilder
            | SharedScenario::DeliveryEnvelopeOtherWorker => {
                landings.snapshot.baseline_commit().to_owned()
            }
            SharedScenario::RetargetedDeliveryUnreviewedShared => retargeted_head.unwrap(),
            SharedScenario::UnreviewedShared
            | SharedScenario::NonEquivalentRepack
            | SharedScenario::MalformedReceipt
            | SharedScenario::MissingReceiptCommit
            | SharedScenario::UnrelatedReceiptCommit
            | SharedScenario::ObsoleteReceiptCommit => landings.root.head(),
        };
        let before_state = landings
            .root
            .operation_state(&store, &[active_ref], StateTasks::Listed);
        drop(store);
        let result = landings
            .root
            .handoff_at("m-shared-landings", Some(&head), &integration);
        let reopened = landings.root.open_task_store().unwrap();
        let accepted = matches!(
            scenario,
            SharedScenario::Shared
                | SharedScenario::MalformedReceipt
                | SharedScenario::RepackedShared
                | SharedScenario::RepackedWithoutMarkdownCommits
                | SharedScenario::MissingReceiptCommit
                | SharedScenario::UnrelatedReceiptCommit
                | SharedScenario::ObsoleteReceiptCommit
        );
        if accepted {
            if let Err(error) = result {
                failures.push(format!(
                    "{scenario:?}: expected verified landing acceptance: {error:#}"
                ));
                assert_eq!(
                    landings
                        .root
                        .operation_state(&reopened, &[active_ref], StateTasks::Listed),
                    before_state
                );
                assert!(!landings.root.path(archive_ref).exists());
                continue;
            }
            assert!(!landings.root.path(active_ref).exists(), "{scenario:?}");
            let archived = landings.root.document(archive_ref);
            let parsed = Milestone::parse(&archived, "forge-mcp").unwrap();
            assert!(reopened.list(None, "all", None).unwrap().is_empty());
            for (index, landed) in [&first_landed, &second_landed].into_iter().enumerate() {
                assert_eq!(
                    parsed.tasks[index]
                        .receipt
                        .as_ref()
                        .unwrap()
                        .commit
                        .as_deref(),
                    record_commits.then_some(landed.as_str()),
                    "{scenario:?}: markdown follows record_task_commits"
                );
            }
            assert!(
                milestone_engine::show(landings.root.root(), "m-shared-landings", false).is_ok(),
                "{scenario:?}: show milestone"
            );
        } else {
            if result.is_ok() {
                failures.push(format!("{scenario:?}: unverified provenance was accepted"));
                continue;
            }
            assert_eq!(
                landings
                    .root
                    .operation_state(&reopened, &[active_ref], StateTasks::Listed),
                before_state,
                "{scenario:?}: active markdown or SQLite receipts changed on failure"
            );
            assert!(
                !landings.root.path(archive_ref).exists(),
                "{scenario:?}: archive created on failure"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
