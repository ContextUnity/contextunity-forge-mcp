use super::super::snapshot;
use super::*;

fn check_integration_evidence_cases(scenarios: &[&str]) {
    let scope = &["src/", "docs/guide.md", "scripts/tool.sh", "forge-mcp.yaml"];
    let source = MilestoneFixtureBuilder::new("m-final")
        .title("Final integration evidence")
        .task("first", scope)
        .task_target("First admitted outcome")
        .task_with_deps("second", scope, &["first"])
        .task_target("Combined admitted outcome")
        .build();
    let fixture = delivered(DeliveredProfile {
        milestone_id: "m-final",
        active_path: "docs/milestones/010-final.md",
        source: &source,
        config: "roots: [src]\ntasks_db: .forge/tasks.sqlite\n",
        files: vec![
            (".gitignore", ".forge/\n".into()),
            ("src/lib.rs", "pub fn work() -> u8 { 0 }\n".into()),
            ("src/obsolete.rs", "pub fn retained() {}\n".into()),
            ("docs/guide.md", "# Runtime guide\n".into()),
            ("scripts/tool.sh", "#!/bin/sh\nexit 0\n".into()),
        ],
        seed_paths: &[".gitignore", "forge-mcp.yaml", "src", "docs", "scripts"],
        commit_message: "admit final-evidence fixture",
        disable_git_maintenance: false,
        tasks: vec![
            DeliveredTask {
                task_ref: "first",
                updates: vec![("src/lib.rs", "pub fn work() -> u8 { 1 }\n".into())],
                completion: DeliveredCompletion::Builder("builder-first"),
            },
            DeliveredTask {
                task_ref: "second",
                updates: vec![("src/lib.rs", "pub fn work() -> u8 { 2 }\n".into())],
                completion: DeliveredCompletion::Builder("builder-second"),
            },
        ],
        evidence: DeliveredEvidence::Default {
            command: "cargo test --test acdd",
            passed: 2,
            failed: 0,
        },
        capture_sqlite: false,
    });
    let active = "docs/milestones/010-final.md";
    let archived = "docs/milestones/archive/010-final.md";
    let state_files = [active, "src/lib.rs"];
    let state_tasks = StateTasks::History(&fixture.milestone);
    let mut needs_restore = false;
    for &scenario in scenarios {
        if scenario == "mode" && !cfg!(unix) {
            continue;
        }
        if needs_restore {
            fixture.snapshot.restore(&fixture.root);
        }
        needs_restore = !matches!(
            scenario,
            "foreign"
                | "authority-binding"
                | "builder-first"
                | "builder-second"
                | "review-reject"
                | "scope-finding"
                | "contract-finding"
                | "verification-exit"
                | "verification-tests"
                | "empty-command"
                | "missing-record"
        );
        let store = fixture.root.store();
        let mut evidence = fixture.evidence.clone();
        if scenario == "unobserved-trace" {
            let before = fixture
                .root
                .operation_state(&store, &state_files, state_tasks);
            let identity = crate::common::sqlite::query_value::<String>(
                &store.connection,
                "SELECT milestone_id FROM gate_execution_traces WHERE subject='milestone' ORDER BY id DESC LIMIT 1",
                [],
            );
            let hidden_identity = format!("{identity}/hidden-fixture");
            let hidden =
                MilestoneHarness::rewrite_trace_identity(&store, &identity, &hidden_identity);
            assert!(hidden > 0);
            let error = fixture
                .root
                .handoff_at("m-final", None, &evidence)
                .expect_err("unobserved trace was accepted");
            let diagnostic = format!("{error:#}");
            assert!(
                diagnostic.contains("HANDOFF_VERIFICATION_REQUIRED"),
                "{diagnostic}"
            );
            assert_eq!(
                fixture
                    .root
                    .operation_state(&store, &state_files, state_tasks),
                before
            );
            assert!(!fixture.root.path(archived).exists());
            assert_eq!(
                MilestoneHarness::rewrite_trace_identity(&store, &hidden_identity, &identity),
                hidden
            );
            let (prior_trace_id, prior_transition_id) =
                MilestoneHarness::milestone_telemetry_ids(&store);
            let (fresh, observed) = MilestoneHarness::reverify(
                &fixture.root,
                "m-final",
                &evidence.target_base,
                &evidence,
            );
            let check = &observed["verification"];
            assert_eq!(check["exit_code"], 0);
            assert_eq!(check["tests_failed"], 0);
            evidence = fresh;
            let VerificationFacts {
                transition_id,
                action,
                payload,
                trace_id,
                status,
                exit_code,
                command,
            } = MilestoneHarness::verification_facts(&store);
            assert!(transition_id > prior_transition_id);
            assert_eq!(action, "pass");
            assert_eq!(
                payload["implementation_digest"],
                evidence.binding.implementation_digest
            );
            assert_eq!(
                payload["target_base_digest"],
                evidence.binding.target_base_digest
            );
            assert!(trace_id > prior_trace_id);
            assert_eq!((status.as_str(), exit_code), ("completed", 0));
            assert_eq!(command, evidence.verification.command);
            continue;
        }
        match scenario {
            "bytes" | "dirty" => fixture
                .root
                .write("src/lib.rs", "pub fn work() -> u8 { 99 }\n"),
            "mode" => fixture.root.make_executable("scripts/tool.sh"),
            "deletion" => fixture.root.remove_file("src/obsolete.rs"),
            "addition" => fixture.root.write("src/new.rs", "pub fn unreviewed() {}\n"),
            "documentation" => fixture
                .root
                .write("docs/guide.md", "# Changed runtime contract\n"),
            "configuration" => fixture.root.write(
                "forge-mcp.yaml",
                "roots: [src, docs]\ntasks_db: .forge/tasks.sqlite\n",
            ),
            "authority" => {
                let doc = fixture.root.document(active);
                fixture.root.write(
                    active,
                    &doc.replace("First admitted outcome", "Different admitted outcome"),
                );
            }
            "target-base" => {
                fixture
                    .root
                    .create_branch_at("different-base", &fixture.base);
                fixture
                    .root
                    .write("src/obsolete.rs", "pub fn upstream() {}\n");
                fixture.root.commit_tracked("changed selected base");
                evidence.target_base = fixture.root.head();
                fixture.root.select_revision(&fixture.branch);
            }
            "foreign" => evidence.binding.repository = "foreign-repository".into(),
            "authority-binding" => evidence.binding.authority_digest = "f".repeat(64),
            "receipt-context" => {
                let task_id = fixture.milestone.task_id(&fixture.milestone.tasks[0]);
                let mut task = store.inspect(&task_id).unwrap();
                let receipt = task.receipt.as_mut().unwrap();
                receipt.passed_at = "2026-10-02T10:00:00Z".into();
                let document = fixture.root.document(active);
                fixture.root.write(
                    active,
                    &milestone_engine::render_task_receipt(
                        &document,
                        "first",
                        receipt,
                        &task.spec.subtasks,
                    )
                    .unwrap(),
                );
                task.spec.receipt = task.receipt.clone();
                MilestoneHarness::persist_descriptor(&store, &task);
            }
            "builder-first" | "builder-second" => evidence.review.worker_id = scenario.into(),
            "review-reject" => evidence.review.decision = "reject".into(),
            "scope-finding" => evidence
                .review
                .scope_findings
                .push("Unadmitted implementation".into()),
            "contract-finding" => evidence
                .review
                .contract_findings
                .push("Required outcome missing".into()),
            "verification-exit" => evidence.verification.exit_code = 1,
            "verification-tests" => evidence.verification.tests_failed = 1,
            "empty-command" => evidence.verification.command.clear(),
            "untracked" => fixture
                .root
                .write("untracked-deliverable.rs", "pub fn pending() {}\n"),
            _ => {}
        }
        if matches!(
            scenario,
            "bytes"
                | "mode"
                | "deletion"
                | "addition"
                | "documentation"
                | "configuration"
                | "authority"
                | "receipt-context"
        ) {
            fixture
                .root
                .stage_paths(&["src", "docs", "scripts", "forge-mcp.yaml"]);
            fixture.root.commit_staged("post-review change");
        }
        let file = fixture.root.path(".forge/integration-evidence.json");
        if scenario == "malformed-record" {
            fixture
                .root
                .write(".forge/integration-evidence.json", "{\"binding\":false}");
        }
        let before = fixture
            .root
            .operation_state(&store, &state_files, state_tasks);
        let result = if matches!(scenario, "missing-record" | "malformed-record") {
            contextunity_forge_mcp::cli::milestone::run(
                fixture.root.root(),
                contextunity_forge_mcp::cli::milestone::MilestoneCommand::Handoff {
                    id: "m-final".into(),
                    commit: None,
                    evidence_file: file,
                },
            )
        } else {
            fixture.root.handoff_at("m-final", None, &evidence)
        };
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("{scenario}: stale or untrusted evidence was accepted"),
        };
        let diagnostic = format!("{error:#}");
        if matches!(
            scenario,
            "bytes"
                | "mode"
                | "deletion"
                | "addition"
                | "documentation"
                | "configuration"
                | "target-base"
                | "foreign"
                | "authority-binding"
                | "receipt-context"
        ) {
            assert!(
                diagnostic.contains("HANDOFF_EVIDENCE_STALE"),
                "{scenario}: {diagnostic}"
            );
        }
        if matches!(scenario, "builder-first" | "builder-second") {
            assert!(
                diagnostic.contains("HANDOFF_REVIEW_NOT_INDEPENDENT"),
                "{scenario}: {diagnostic}"
            );
        }
        assert_eq!(
            fixture
                .root
                .operation_state(&store, &state_files, state_tasks),
            before,
            "{scenario}: files or task proof mutated"
        );
        assert!(!fixture.root.path(archived).exists(), "{scenario}");
        if scenario == "bytes" {
            assert!(diagnostic.contains("obtain fresh integration review"));
            assert!(!diagnostic.contains("git checkout"));
        }
    }
}

#[test]
fn handoff_rejects_stale_or_untrusted_integration_evidence_atomically() {
    check_integration_evidence_cases(&[
        "foreign",
        "authority-binding",
        "builder-first",
        "builder-second",
        "review-reject",
        "scope-finding",
        "contract-finding",
        "verification-exit",
        "verification-tests",
        "empty-command",
        "missing-record",
        "malformed-record",
        "bytes",
        "mode",
        "deletion",
        "addition",
    ]);
}

#[test]
fn handoff_rejects_tampered_git_receipt_and_trace_integration_evidence_atomically() {
    check_integration_evidence_cases(&[
        "documentation",
        "configuration",
        "authority",
        "target-base",
        "receipt-context",
        "unobserved-trace",
        "dirty",
        "untracked",
    ]);
}

#[test]
fn milestone_verification_reports_registered_gate_outcomes_without_root_checkpoint() {
    snapshot::assert_safe_git_environment();
    let root = MilestoneHarness::configured_workspace();
    let manifest = MilestoneFixtureBuilder::new("m-verification")
        .title("Milestone verification")
        .task(
            "implementation",
            &[
                "src/lib.rs",
                "docs/reference/verification.md",
                "skills/lean-ctx/SKILL.md",
                "skills/lean-ctx/scripts/install.sh",
            ],
        )
        .task_target("Deliver a verified source change")
        .build();
    root.write(".gitignore", ".forge/\n");
    root.write("docs/milestones/010-verification.md", &manifest);
    root.write("src/lib.rs", "pub fn value() -> u8 { 0 }\n");
    root.write(
        "docs/reference/verification.md",
        "# Verification notes\n\nBaseline docs.\n",
    );
    root.write(
        "skills/lean-ctx/SKILL.md",
        "# Lean Context\n\nBaseline skill docs.\n",
    );
    root.write(
        "skills/lean-ctx/scripts/install.sh",
        "#!/bin/sh\nprintf 'baseline skill script\\n'\n",
    );
    root.make_executable("skills/lean-ctx/scripts/install.sh");
    root.write(
        ".forge/acdd/profile.yaml",
        include_str!("../fixtures/milestones/verification_profile.yaml"),
    );
    let (milestone, store) = root.sync_document("docs/milestones/010-verification.md", &manifest);
    commit_handoff_fixture_inputs(
        &root,
        &[
            ".gitignore",
            "forge-mcp.yaml",
            "docs/milestones/010-verification.md",
            "src/lib.rs",
            "docs/reference/verification.md",
            "skills/lean-ctx/SKILL.md",
            "skills/lean-ctx/scripts/install.sh",
        ],
    );
    let target_base = git_head(root.root());
    root.complete_task(CompletionMode::Builder {
        store: &store,
        task_id: &milestone.task_id(&milestone.tasks[0]),
        builder: "milestone-builder",
    })
    .unwrap();
    drop(store);
    let snapshot = snapshot::FixtureSnapshot::capture(&root);
    for (fail_check, expected_status, expected_exit) in [(false, "passed", 0), (true, "failed", 1)]
    {
        if fail_check {
            snapshot.restore(&root);
            root.write(".forge/fail-check", "fail\n");
        }
        let store = root.store();
        let result = root
            .execute_verification("m-verification", &target_base)
            .unwrap();
        assert_eq!(result["status"], expected_status, "{result}");
        assert_eq!(result["verification"]["exit_code"], expected_exit);
        if !fail_check {
            assert_eq!(result["gate"], "verify");
            assert_eq!(result["verification"]["command_id"], "check_full_repo");
        }
        assert!(store
            .get_checkpoint("forge-mcp/forge-mcp/m-verification/milestone/milestone/state")
            .unwrap()
            .is_none());
        drop(store);
    }
}

#[test]
fn milestone_handoff_accepts_integration_evidence_for_cancelled_tasks_and_prunes_sqlite() {
    snapshot::assert_safe_git_environment();
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\nmilestones: [docs/milestones]\n",
    );
    root.write("src/lib.rs", "pub fn value() {}\n");
    let manifest = "docs/milestones/010-cancelled.md";
    let text = MilestoneFixtureBuilder::new("m-cancelled-root")
        .title("Cancelled tasks")
        .task("cancelled", &["src/"])
        .task_target("Cancelled implementation")
        .task_status("cancelled")
        .task_proof_policy("direct-proof")
        .build();
    root.write(manifest, &text);
    root.write(
        ".forge/acdd/profile.yaml",
        include_str!("../fixtures/milestones/verification_profile.yaml"),
    );
    MilestoneHarness::git_at(root.root(), &["add", "-A"]).unwrap();
    root.commit_staged("admit cancelled milestone");
    root.manage(tasks::Manage {
        action: tasks::ManageAction::Sync,
        milestone_ref: Some(manifest.into()),
        ..Default::default()
    })
    .unwrap();

    let target = root.head();
    let context = root.handoff_context("m-cancelled-root", &target).unwrap();
    let observed = root
        .execute_verification("m-cancelled-root", &target)
        .unwrap();
    let check = &observed["verification"];
    let verification: milestone_engine::IntegrationVerification = serde_json::from_value(json!({
        "command": check["command"], "exit_code": check["exit_code"],
        "tests_passed": check["tests_passed"], "tests_failed": check["tests_failed"],
        "project_command": check["command_id"]
    }))
    .unwrap();
    let evidence: milestone_engine::IntegrationEvidence = serde_json::from_value(json!({
        "binding": context["binding"], "target_base": context["target_base"],
        "review": {"worker_id": "independent-reviewer", "decision": "pass",
            "summary": "Reviewed the final candidate and task authority",
            "scope_findings": [], "contract_findings": []},
        "verification": verification
    }))
    .unwrap();

    let result = root
        .handoff_at("m-cancelled-root", None, &evidence)
        .unwrap();
    assert_eq!(result["status"], "completed");
    assert!(!root.path(manifest).exists());
    assert!(root
        .root()
        .join("docs/milestones/archive/010-cancelled.md")
        .exists());

    let store = root.store();
    assert!(store.list(None, "all", None).unwrap().is_empty());
    assert!(store.milestone_tombstoned("m-cancelled-root").unwrap());
    for table in [
        "gate_checkpoints",
        "gate_execution_traces",
        "gate_transitions",
    ] {
        let count =
            MilestoneHarness::milestone_rows(&store, table, "forge-mcp/forge-mcp/m-cancelled-root")
                .unwrap();
        assert_eq!(count, 0, "{table} should be pruned at handoff");
    }
}
