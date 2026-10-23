use super::support::*;
use contextunity_forge_mcp::db::tasks_store::TasksStore;
use contextunity_forge_mcp::engine::milestones as milestone_engine;
use contextunity_forge_mcp::engine::tasks::list_workspaces;

#[test]
fn cli_migration_reconciles_manifests_and_creation_selects_one_task() {
    let (root, mut store, milestone) = fixture();
    let invoke = |action: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--format",
                "json",
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
            .args([
                "--format",
                "json",
                "--root",
                root.0.to_str().unwrap(),
                "migrate",
                action,
            ])
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
        .args([
            "--format",
            "json",
            "--root",
            root.0.to_str().unwrap(),
            "milestone",
        ])
        .args(args)
        .output()
        .unwrap()
}

fn complete_task_through_engine(root: &ScopedWorkspace, store: &TasksStore, task_id: &str) {
    complete_task_with_builder(root, store, task_id, "builder");
}

fn complete_task_with_builder(
    root: &ScopedWorkspace,
    store: &TasksStore,
    task_id: &str,
    builder: &str,
) {
    for (index, stage) in GATES.iter().enumerate() {
        let worker_id = match index {
            0 => "contract-author",
            1 => builder,
            2 => "independent-reviewer",
            _ => "delivery-reviewer",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.to_owned(),
                stage: (*stage).to_owned(),
                worker_id: worker_id.to_owned(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = store.inspect(task_id).unwrap();
        let mut evidence = evidence(&task);
        evidence.commit = None;
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":task_id,
            "stage":stage,
            "action":"pass",
            "evidence":evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(
            result["status"],
            if index + 1 == GATES.len() {
                "completed"
            } else {
                "ready"
            }
        );
    }
}

#[test]
fn handoff_preserves_proofs_across_reviewed_history_rewrites() {
    for scenario in [
        "orphan",
        "amend",
        "cherry-pick",
        "merge",
        "rebase",
        "squash",
        "conflict",
        "gc",
        "off",
        "milestone-only",
        "legacy-off",
    ] {
        let root = ScopedWorkspace::new("forge_content_handoff");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{scenario}: {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        let recording = !matches!(scenario, "off" | "milestone-only" | "legacy-off");
        let setting = match scenario {
            "off" => "commit_tracking: off\n",
            "milestone-only" => "commit_tracking: milestone_only\n",
            "legacy-off" => "record_task_commits: false\n",
            _ => "",
        };
        root.write(".gitignore", ".forge/\n");
        root.write(
            "forge-mcp.yaml",
            &format!("roots: [src]\ntasks_db: .forge/tasks.sqlite\n{setting}"),
        );
        let source = "---\nid: m-history\ntitle: Content reviewed handoff\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Combined implementation\n```yaml\ntask_ref: first\ntarget: Update first function\nproof_policy: seam-test-first\nscope: [src/shared.rs]\n```\n```yaml\ntask_ref: second\ntarget: Update second function\nproof_policy: seam-test-first\nscope: [src/shared.rs]\ndepends_on: [first]\n```\n";
        let active = "docs/milestones/010-history.md";
        let archived = "docs/milestones/archive/010-history.md";
        root.write(active, source);
        let initial = (0..24)
            .map(|i| format!("pub fn f{i}() -> u8 {{ 0 }}\n"))
            .collect::<String>();
        root.write("src/shared.rs", &initial);
        git(&["add", ".gitignore", "forge-mcp.yaml", "docs", "src"]);
        git(&["commit", "-m", "admit history fixture"]);
        let base = git_head(&root.0);
        let branch = git(&["branch", "--show-current"]);
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, active, &root.0).unwrap();
        let mut deliveries = Vec::new();
        let mut reviewed = initial.clone();
        for (index, spec) in milestone.tasks.iter().enumerate() {
            reviewed = reviewed.replace(
                &format!("f{index}() -> u8 {{ 0 }}"),
                &format!("f{index}() -> u8 {{ {} }}", index + 1),
            );
            root.write("src/shared.rs", &reviewed);
            complete_task_with_builder(
                &root,
                &store,
                &milestone.task_id(spec),
                if index == 0 {
                    "builder-first"
                } else {
                    "builder-second"
                },
            );
            deliveries.push(git_head(&root.0));
        }
        let before: Vec<_> = milestone
            .tasks
            .iter()
            .map(|spec| store.inspect_details(&milestone.task_id(spec)).unwrap())
            .collect();
        let mut evidence =
            fixture_integration_evidence(&root.0, "m-history", "cargo test --test acdd", 1, 0);
        let context = milestone_cli(
            &root,
            &["handoff-context", "m-history", "--target-base", &base],
        );
        assert!(
            context.status.success(),
            "{scenario}: {}",
            String::from_utf8_lossy(&context.stderr)
        );
        let context: Value = serde_json::from_slice(&context.stdout).unwrap();
        assert_eq!(
            serde_json::to_value(&evidence.binding).unwrap(),
            context["binding"]
        );
        assert_eq!(
            evidence.binding.builders,
            vec!["builder-first", "builder-second"]
        );
        match scenario {
            "orphan" => {
                git(&["checkout", "--orphan", "rewritten"]);
                git(&["commit", "-m", "same content without original ancestry"]);
            }
            "amend" => {
                git(&["commit", "--amend", "-m", "amended navigation identity"]);
            }
            "cherry-pick" => {
                git(&["checkout", "-b", "picked", &base]);
                git(&["cherry-pick", "--no-commit", &deliveries[0], &deliveries[1]]);
                git(&["commit", "-m", "picked reviewed content"]);
            }
            "squash" => {
                git(&["reset", "--soft", &base]);
                git(&["commit", "-m", "two tasks in one commit"]);
            }
            "merge" | "rebase" | "conflict" => {
                git(&["checkout", "-b", "upstream", &base]);
                let upstream = if scenario == "conflict" {
                    initial.replacen("{ 0 }", "{ 3 }", 1)
                } else {
                    initial.replace("f23() -> u8 { 0 }", "f23() -> u8 { 3 }")
                };
                root.write("src/shared.rs", &upstream);
                git(&["commit", "-am", "upstream optimization in the same file"]);
                let target = git_head(&root.0);
                git(&["checkout", &branch]);
                if scenario == "rebase" {
                    git(&["rebase", "upstream"]);
                } else if scenario == "conflict" {
                    let output = std::process::Command::new("git")
                        .args(["merge", "--no-ff", "upstream", "-m", "integrate upstream"])
                        .current_dir(&root.0)
                        .output()
                        .unwrap();
                    assert!(!output.status.success());
                    root.write(
                        "src/shared.rs",
                        &reviewed.replace("f0() -> u8 { 1 }", "f0() -> u8 { 4 }"),
                    );
                    git(&["add", "src/shared.rs"]);
                    git(&["commit", "--no-edit"]);
                } else {
                    git(&["merge", "--no-ff", "upstream", "-m", "integrate upstream"]);
                }
                let file = root.0.join(".forge/integration-evidence.json");
                std::fs::write(&file, serde_json::to_vec(&evidence).unwrap()).unwrap();
                let stale = milestone_cli(
                    &root,
                    &[
                        "handoff",
                        "m-history",
                        "--evidence-file",
                        file.to_str().unwrap(),
                    ],
                );
                assert!(!stale.status.success());
                assert!(
                    String::from_utf8_lossy(&stale.stderr).contains("HANDOFF_EVIDENCE_STALE"),
                    "{stale:?}"
                );
                assert!(root.0.join(active).exists());
                assert!(!root.0.join(archived).exists());
                let fresh = milestone_cli(
                    &root,
                    &["handoff-context", "m-history", "--target-base", &target],
                );
                assert!(fresh.status.success(), "{fresh:?}");
                let fresh: Value = serde_json::from_slice(&fresh.stdout).unwrap();
                evidence.binding = serde_json::from_value(fresh["binding"].clone()).unwrap();
                evidence.target_base = fresh["target_base"].as_str().unwrap().to_owned();
            }
            "gc" => {
                let tree = git(&["rev-parse", "HEAD^{tree}"]);
                let obsolete = git(&["commit-tree", &tree, "-m", "temporary navigation object"]);
                let document = std::fs::read_to_string(root.0.join(active)).unwrap();
                let mut receipt = store
                    .inspect(&milestone.task_id(&milestone.tasks[0]))
                    .unwrap()
                    .receipt
                    .unwrap();
                receipt.commit = Some(obsolete.clone());
                root.write(
                    active,
                    &milestone_engine::render_task_receipt(&document, "first", &receipt, &[])
                        .unwrap(),
                );
                git(&["add", active]);
                git(&["commit", "-m", "navigation pointer update"]);
                git(&["reflog", "expire", "--expire=now", "--all"]);
                git(&["gc", "--prune=now"]);
                let missing = std::process::Command::new("git")
                    .args(["cat-file", "-e", &format!("{obsolete}^{{commit}}")])
                    .current_dir(&root.0)
                    .output()
                    .unwrap();
                assert!(
                    !missing.status.success(),
                    "unreferenced navigation object survived pruning"
                );
            }
            _ => {}
        }
        let file = root.0.join(".forge/integration-evidence.json");
        std::fs::write(&file, serde_json::to_vec(&evidence).unwrap()).unwrap();
        let output = milestone_cli(
            &root,
            &[
                "handoff",
                "m-history",
                "--evidence-file",
                file.to_str().unwrap(),
            ],
        );
        assert!(
            output.status.success(),
            "{scenario}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            output["handoff"]["integration_evidence"],
            serde_json::to_value(&evidence).unwrap()
        );
        let archived_text = std::fs::read_to_string(root.0.join(archived)).unwrap();
        let archived_milestone = Milestone::parse(&archived_text, "forge-mcp").unwrap();
        assert!(store.list(None, "all", None).unwrap().is_empty());
        for (index, spec) in archived_milestone.tasks.iter().enumerate() {
            let accepted: contextunity_forge_mcp::core::tasks::Receipt =
                serde_json::from_value(before[index]["receipt"].clone()).unwrap();
            assert!(accepted.matches_proofs(spec.receipt.as_ref().unwrap()));
            if !recording {
                assert!(spec.receipt.as_ref().unwrap().commit.is_none());
            }
        }
        assert!(!root.0.join(active).exists());
        assert!(root.0.join(archived).exists());
        let refs = git(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/evidence/",
        ]);
        let header = archived_text
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap()
            .0;
        let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
        let retained: std::collections::BTreeSet<_> = refs.lines().collect();
        for sha in meta["handoff"]["proof_shas"].as_sequence().unwrap() {
            assert!(!retained
                .contains(format!("refs/forge/evidence/{}", sha.as_str().unwrap()).as_str()));
        }
        assert!(git(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/"
        ])
        .is_empty());
    }
}

#[test]
fn handoff_validates_reviewed_shared_scope_and_repacked_landings_atomically() {
    #[derive(Clone, Copy, Debug)]
    enum Scenario {
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
    let mut failures = Vec::new();
    for scenario in [
        Scenario::Shared,
        Scenario::RepackedShared,
        Scenario::RepackedWithoutMarkdownCommits,
        Scenario::UnreviewedShared,
        Scenario::NonEquivalentRepack,
        Scenario::MalformedReceipt,
        Scenario::MissingReceiptCommit,
        Scenario::UnrelatedReceiptCommit,
        Scenario::ObsoleteReceiptCommit,
        Scenario::CorruptedLaterReview,
        Scenario::ReviewEnvelopeBuilder,
        Scenario::ReviewEnvelopeOtherWorktree,
        Scenario::ReviewClaimBuilder,
        Scenario::ReviewClaimAndEnvelopeBuilder,
        Scenario::DeliveryEnvelopeOtherWorker,
        Scenario::RetargetedDeliveryUnreviewedShared,
    ] {
        let root = ScopedWorkspace::new("forge_reviewed_shared_handoff");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{scenario:?}: git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        let record_commits = !matches!(
            scenario,
            Scenario::RepackedWithoutMarkdownCommits | Scenario::RetargetedDeliveryUnreviewedShared
        );
        root.write(
            "forge-mcp.yaml",
            &format!(
                "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nrecord_task_commits: {record_commits}\n"
            ),
        );
        let source = "---\nid: m-shared-landings\ntitle: Reviewed shared landings\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Reviewed shared landings\n```yaml\ntask_ref: first\ntarget: Deliver first shared version\nproof_policy: seam-test-first\nscope: [src/shared.rs, src/first.rs]\n```\n```yaml\ntask_ref: second\ntarget: Deliver next reviewed shared version\nproof_policy: seam-test-first\nscope: [src/shared.rs, src/second.rs]\ndepends_on: [first]\n```\n";
        let active_ref = "docs/milestones/010-shared-landings.md";
        let archive_ref = "docs/milestones/archive/010-shared-landings.md";
        root.write(active_ref, source);
        root.write("src/shared.rs", "pub fn shared() -> u8 { 0 }\n");
        root.write("src/first.rs", "pub fn first() -> u8 { 0 }\n");
        root.write("src/second.rs", "pub fn second() -> u8 { 0 }\n");
        git(&["add", "forge-mcp.yaml", "docs", "src"]);
        git(&["commit", "-m", "admit shared milestone"]);
        let baseline = git_head(&root.0);
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, active_ref, &root.0).unwrap();
        let first_id = milestone.task_id(&milestone.tasks[0]);
        let second_id = milestone.task_id(&milestone.tasks[1]);
        root.write("src/shared.rs", "pub fn shared() -> u8 { 1 }\n");
        root.write("src/first.rs", "pub fn first() -> u8 { 1 }\n");
        complete_task_through_engine(&root, &store, &first_id);
        let first_delivery = store
            .inspect(&first_id)
            .unwrap()
            .receipt
            .unwrap()
            .commit
            .unwrap();
        if matches!(
            scenario,
            Scenario::RepackedShared
                | Scenario::RepackedWithoutMarkdownCommits
                | Scenario::NonEquivalentRepack
        ) {
            if matches!(scenario, Scenario::NonEquivalentRepack) {
                root.write("src/first.rs", "pub fn first() -> u8 { 99 }\n");
                git(&["add", "src/first.rs"]);
            }
            // Only the disposable fixture history is rewritten. Positive cases never edit receipts.
            git(&["commit", "--amend", "-m", "repacked first task"]);
            assert_ne!(git_head(&root.0), first_delivery);
        }
        let first_landed = git_head(&root.0);
        root.write("src/shared.rs", "pub fn shared() -> u8 { 2 }\n");
        root.write("src/second.rs", "pub fn second() -> u8 { 2 }\n");
        complete_task_through_engine(&root, &store, &second_id);
        let second_landed = git_head(&root.0);
        let mut integration = fixture_integration_evidence(
            &root.0,
            "m-shared-landings",
            "cargo test --test acdd",
            1,
            0,
        );
        if matches!(scenario, Scenario::NonEquivalentRepack) {
            integration
                .review
                .contract_findings
                .push("Earlier task outcome changed without an admitted replacement".into());
        }
        if matches!(
            scenario,
            Scenario::UnreviewedShared | Scenario::RetargetedDeliveryUnreviewedShared
        ) {
            root.write("src/shared.rs", "pub fn shared() -> u8 { 3 }\n");
            git(&["add", "src/shared.rs"]);
            // A plausible task message and declared overlap cannot authorize this delta.
            git(&["commit", "-m", "second: shared scope follow-up"]);
        }
        let invalid_commit = match scenario {
            Scenario::MalformedReceipt => Some("not-a-git-commit".to_owned()),
            Scenario::MissingReceiptCommit => Some("f".repeat(40)),
            Scenario::UnrelatedReceiptCommit => {
                let tree = git(&["rev-parse", &format!("{first_landed}^{{tree}}")]);
                Some(git(&[
                    "commit-tree",
                    &tree,
                    "-m",
                    "unrelated matching tree",
                ]))
            }
            Scenario::ObsoleteReceiptCommit => Some(baseline),
            _ => None,
        };
        if let Some(invalid_commit) = invalid_commit {
            // Fault injection only: prove malformed/stale caller input is rejected atomically.
            let doc = std::fs::read_to_string(root.0.join(active_ref)).unwrap();
            let mut receipt = store.inspect(&first_id).unwrap().receipt.unwrap();
            receipt.commit = Some(invalid_commit);
            let invalid_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
                &doc,
                "first",
                &receipt,
                &[],
            )
            .unwrap();
            root.write(active_ref, &invalid_doc);
        }
        if matches!(scenario, Scenario::CorruptedLaterReview) {
            assert_eq!(store.connection.execute(
                "UPDATE task_gates SET evidence='{corrupted-review' WHERE task_id=?1 AND gate='review' AND state='passed'",
                [&second_id],
            ).unwrap(), 1);
        }
        if matches!(
            scenario,
            Scenario::ReviewEnvelopeBuilder | Scenario::ReviewClaimAndEnvelopeBuilder
        ) {
            assert_eq!(store.connection.execute(
                "UPDATE task_gates SET evidence=json_set(evidence, '$.worker_id', 'builder') WHERE task_id=?1 AND gate='review' AND state='passed'",
                [&second_id],
            ).unwrap(), 1);
        }
        if matches!(scenario, Scenario::ReviewEnvelopeOtherWorktree) {
            let other_worktree = root.0.join("other-claimed-worktree");
            std::fs::create_dir_all(&other_worktree).unwrap();
            assert_eq!(store.connection.execute(
                "UPDATE task_gates SET evidence=json_set(evidence, '$.worktree', ?1) WHERE task_id=?2 AND gate='review' AND state='passed'",
                rusqlite::params![other_worktree.to_str().unwrap(), second_id],
            ).unwrap(), 1);
        }
        if matches!(
            scenario,
            Scenario::ReviewClaimBuilder | Scenario::ReviewClaimAndEnvelopeBuilder
        ) {
            assert_eq!(store.connection.execute(
                "UPDATE task_claims SET worker_id='builder' WHERE task_id=?1 AND revision=(SELECT revision FROM task_gates WHERE task_id=?1 AND gate='review' AND state='passed' ORDER BY revision DESC LIMIT 1)",
                [&second_id],
            ).unwrap(), 1);
        }
        if matches!(scenario, Scenario::DeliveryEnvelopeOtherWorker) {
            assert_eq!(store.connection.execute(
                "UPDATE task_gates SET evidence=json_set(evidence, '$.worker_id', 'other-delivery-worker') WHERE task_id=?1 AND gate='deliver' AND state='passed'",
                [&second_id],
            ).unwrap(), 1);
        }
        if matches!(scenario, Scenario::RetargetedDeliveryUnreviewedShared) {
            let generated_doc = std::fs::read_to_string(root.0.join(active_ref)).unwrap();
            let generated = Milestone::parse(&generated_doc, "forge-mcp").unwrap();
            assert!(generated.tasks.iter().all(|task| task
                .receipt
                .as_ref()
                .unwrap()
                .commit
                .is_none()));
            let unreviewed_commit = git_head(&root.0);
            assert_ne!(unreviewed_commit, second_landed);
            // Retarget only the accepted delivery pointer and durable receipt. The
            // accepted build/review snapshots remain bound to reviewed shared v2.
            assert_eq!(store.connection.execute(
                "UPDATE task_gates SET evidence=json_set(evidence, '$.commit', ?1) WHERE task_id=?2 AND gate='deliver' AND state='passed'",
                rusqlite::params![unreviewed_commit, second_id],
            ).unwrap(), 1);
            assert_eq!(store.connection.execute(
                "UPDATE tasks SET descriptor=json_set(descriptor, '$.receipt.commit', ?1) WHERE task_id=?2",
                rusqlite::params![unreviewed_commit, second_id],
            ).unwrap(), 1);
        }
        if matches!(
            scenario,
            Scenario::MalformedReceipt
                | Scenario::MissingReceiptCommit
                | Scenario::UnrelatedReceiptCommit
                | Scenario::ObsoleteReceiptCommit
        ) {
            git(&["add", active_ref]);
            git(&["commit", "-m", "navigation input for handoff"]);
        }
        let head = git_head(&root.0);
        let before_doc = std::fs::read(root.0.join(active_ref)).unwrap();
        let before_tasks = serde_json::to_value(store.list(None, "all", None).unwrap()).unwrap();
        drop(store);
        let result =
            milestone_engine::handoff(&root.0, "m-shared-landings", Some(&head), &integration);
        let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        let accepted = matches!(
            scenario,
            Scenario::Shared
                | Scenario::MalformedReceipt
                | Scenario::RepackedShared
                | Scenario::RepackedWithoutMarkdownCommits
                | Scenario::MissingReceiptCommit
                | Scenario::UnrelatedReceiptCommit
                | Scenario::ObsoleteReceiptCommit
        );
        if accepted {
            if let Err(error) = result {
                failures.push(format!(
                    "{scenario:?}: expected verified landing acceptance: {error:#}"
                ));
                assert_eq!(std::fs::read(root.0.join(active_ref)).unwrap(), before_doc);
                assert_eq!(
                    serde_json::to_value(reopened.list(None, "all", None).unwrap()).unwrap(),
                    before_tasks
                );
                assert!(!root.0.join(archive_ref).exists());
                continue;
            }
            assert!(!root.0.join(active_ref).exists(), "{scenario:?}");
            let archived = std::fs::read_to_string(root.0.join(archive_ref)).unwrap();
            let parsed = Milestone::parse(&archived, "forge-mcp").unwrap();
            assert!(reopened.list(None, "all", None).unwrap().is_empty());
            for (index, (_, landed)) in [(&first_id, &first_landed), (&second_id, &second_landed)]
                .into_iter()
                .enumerate()
            {
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
            let shown = milestone_cli(&root, &["show", "m-shared-landings"]);
            assert!(
                shown.status.success(),
                "{scenario:?}: {}",
                String::from_utf8_lossy(&shown.stderr)
            );
        } else {
            if result.is_ok() {
                failures.push(format!("{scenario:?}: unverified provenance was accepted"));
                continue;
            }
            assert_eq!(
                std::fs::read(root.0.join(active_ref)).unwrap(),
                before_doc,
                "{scenario:?}: active markdown changed on failure"
            );
            assert_eq!(
                serde_json::to_value(reopened.list(None, "all", None).unwrap()).unwrap(),
                before_tasks,
                "{scenario:?}: SQLite receipts changed on failure"
            );
            assert!(
                !root.0.join(archive_ref).exists(),
                "{scenario:?}: archive created on failure"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
            "--format",
            "json",
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
    assert!(text.contains("## Deferred and out-of-scope defects"));
    assert!(text.contains("deferred_defects: []"));
    assert!(Milestone::parse(&text, "forge-mcp")
        .unwrap()
        .deferred_defects
        .is_empty());
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
fn milestone_specification_parses_typed_deferred_defects_block() {
    let spec = r#"---
id: m-defects
title: Defects test
doc_type: contract
deferred_defects:
  - id: DEFECT-FRONTMATTER
    source: task_blackboard
    title: "Frontmatter defect stays typed"
    path: src/engine/tasks.rs
    disposition: subsequent_milestone
    notes: "Imported from contract metadata"
---
# Defects test

### task: task-a
```yaml
task_ref: task-a
target: Task A target
proof_policy: direct-proof
scope: [src/]
```

## Deferred and out-of-scope defects

```yaml
deferred_defects:
  - id: DEFECT-001
    source: review_findings
    title: "Unshadowed function resolution bug in edge case"
    path: src/engine/languages/python.rs
    disposition: deferred
    notes: "Follow up in next milestone"
  - id: DEFECT-002
    source: task_blackboard
    title: "Minor syntax warning"
```
"#;
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    assert_eq!(milestone.deferred_defects.len(), 3);
    assert_eq!(milestone.deferred_defects[0].id, "DEFECT-FRONTMATTER");
    assert_eq!(milestone.deferred_defects[0].source, "task_blackboard");
    assert_eq!(
        milestone.deferred_defects[0].title,
        "Frontmatter defect stays typed"
    );
    assert_eq!(
        milestone.deferred_defects[0].path.as_deref(),
        Some("src/engine/tasks.rs")
    );
    assert_eq!(
        milestone.deferred_defects[0].disposition,
        "subsequent_milestone"
    );
    assert_eq!(
        milestone.deferred_defects[0].notes.as_deref(),
        Some("Imported from contract metadata")
    );
    assert_eq!(milestone.deferred_defects[1].id, "DEFECT-001");
    assert_eq!(milestone.deferred_defects[1].source, "review_findings");
    assert_eq!(
        milestone.deferred_defects[1].title,
        "Unshadowed function resolution bug in edge case"
    );
    assert_eq!(
        milestone.deferred_defects[1].path.as_deref(),
        Some("src/engine/languages/python.rs")
    );
    assert_eq!(milestone.deferred_defects[1].disposition, "deferred");
    assert_eq!(
        milestone.deferred_defects[1].notes.as_deref(),
        Some("Follow up in next milestone")
    );
    assert_eq!(milestone.deferred_defects[2].id, "DEFECT-002");
    assert_eq!(milestone.deferred_defects[2].source, "task_blackboard");
    assert_eq!(milestone.deferred_defects[2].disposition, "deferred");
    assert!(milestone.deferred_defects[2].path.is_none());
    assert!(milestone.deferred_defects[2].notes.is_none());
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
    let source = "---\nid: m-close\ntitle: Close work\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Close work\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish work\nproof_policy: seam-test-first\nscope: [src/lib.rs]\n```\n```yaml\ntask_ref: cancelled\ntarget: Transferred work\nproof_policy: seam-test-first\nscope: [cancelled/]\nstatus: cancelled\n```\n";
    root.write("src/lib.rs", "pub fn finish() {}\n");
    root.write("docs/milestones/010-close.md", source);
    commit_handoff_fixture_inputs(
        &root,
        &[
            "forge-mcp.yaml",
            "src/lib.rs",
            "docs/milestones/010-close.md",
        ],
    );
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(
            &Milestone::parse(source, "forge-mcp").unwrap(),
            "docs/milestones/010-close.md",
            &root.0,
        )
        .unwrap();
    let commit = git_head(&root.0);
    let args = [
        "handoff",
        "m-close",
        "--commit",
        commit.as_str(),
        "--evidence-file",
        &fixture_evidence_file(&root.0, "m-close", "cargo test --all-targets", 7, 0),
    ];
    let rejected = milestone_cli(&root, &args);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("finish"));
    assert!(root.0.join("docs/milestones/010-close.md").exists());
    let task = store
        .list(None, "all", None)
        .unwrap()
        .into_iter()
        .find(|task| task.spec.task_ref == "finish")
        .unwrap();
    store
        .extend_scope(&task.task_id, &["src/extra.rs".to_owned()], &root.0)
        .unwrap();
    root.write("src/lib.rs", "pub fn finish() {}\n");
    root.write("src/extra.rs", "pub fn admitted_extension() {}\n");
    complete_task_through_engine(&root, &store, &task.task_id);
    let foreign_id = "other/m-close:waiting";
    let mut foreign_task = store.inspect(&task.task_id).unwrap();
    foreign_task.task_id = foreign_id.into();
    store
        .connection
        .execute(
            "INSERT INTO tasks VALUES(?1,?2,?3)",
            rusqlite::params![
                foreign_id,
                "docs/milestones/010-close.md",
                serde_json::to_string(&foreign_task).unwrap()
            ],
        )
        .unwrap();
    for id in [&task.task_id, &foreign_id.to_owned()] {
        store
            .connection
            .execute("INSERT OR IGNORE INTO task_revisions VALUES(?1,1)", [id])
            .unwrap();
    }
    for dependency in [&task.task_id, &"forge-mcp/m-close:__milestone__".to_owned()] {
        store
            .connection
            .execute(
                "INSERT INTO task_dependencies VALUES(?1,?2,0)",
                rusqlite::params![foreign_id, dependency],
            )
            .unwrap();
    }
    let old_scope = store
        .milestone_scope_ref("docs/milestones/010-close.md")
        .unwrap();
    let new_scope = store
        .milestone_scope_ref("docs/milestones/archive/010-close.md")
        .unwrap();
    for scope in [
        old_scope.as_str(),
        new_scope.as_str(),
        "other/other/docs/milestones/010-close.md",
    ] {
        store.connection.execute("INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'fixture','notes','retained until handoff',0)", [scope]).unwrap();
    }
    drop(store);
    let handoff_commit = git_head(&root.0);
    let completed_args = [
        "handoff",
        "m-close",
        "--commit",
        handoff_commit.as_str(),
        "--evidence-file",
        &fixture_evidence_file(&root.0, "m-close", "cargo test --all-targets", 7, 0),
    ];
    let delivered_active =
        std::fs::read_to_string(root.0.join("docs/milestones/010-close.md")).unwrap();
    let foreign_parent = Workspace::new();
    let foreign = foreign_parent.root().join("linked");
    let git = |cwd: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    git(
        &root.0,
        &[
            "worktree",
            "add",
            "--detach",
            foreign.to_str().unwrap(),
            &handoff_commit,
        ],
    );
    git(
        &foreign,
        &["commit", "--allow-empty", "-m", "foreign historical target"],
    );
    let selected_target = String::from_utf8(git(&foreign, &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_owned();
    std::fs::write(foreign.join("docs/milestones/020-foreign.md"), "---\nid: m-foreign\nproject: other\ntitle: Foreign active handoff\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n```yaml\ntask_ref: transfer\ntarget: Transferred work\nproof_policy: seam-test-first\nscope: [foreign/]\nstatus: cancelled\n```\n").unwrap();
    git(&foreign, &["add", "docs/milestones/020-foreign.md"]);
    git(&foreign, &["commit", "-m", "foreign active handoff"]);
    let foreign_cli = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--format", "json", "--root", foreign.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    foreign_cli(&["task", "sync", "--file", "docs/milestones/020-foreign.md"]);
    foreign_cli(&[
        "milestone",
        "handoff-context",
        "m-foreign",
        "--target-base",
        &handoff_commit,
    ]);
    let foreign_context = foreign_cli(&[
        "milestone",
        "handoff-context",
        "m-foreign",
        "--target-base",
        &selected_target,
    ]);
    let foreign_context: serde_json::Value =
        serde_json::from_slice(&foreign_context.stdout).unwrap();
    assert_eq!(foreign_context["target_base"], selected_target);
    assert_ne!(foreign_context["commit"], selected_target);
    assert_ne!(handoff_commit, selected_target);
    git(
        &foreign,
        &[
            "merge-base",
            "--is-ancestor",
            &selected_target,
            foreign_context["commit"].as_str().unwrap(),
        ],
    );
    std::fs::write(foreign.join("docs/milestones/022-same-id.md"), "---\nid: m-close\nproject: other\ntitle: Foreign same-ID authority\nstatus: active\n---\n```yaml\ntask_ref: transfer\ntarget: Transferred work\nproof_policy: seam-test-first\nscope: [foreign/]\nstatus: ready\n```\n").unwrap();
    git(&foreign, &["add", "docs/milestones/022-same-id.md"]);
    std::fs::write(foreign.join("docs/milestones/021-proof.md"), "---\nid: m-proof\nproject: other\ntitle: Active foreign task\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n```yaml\ntask_ref: proof\ntarget: Keep an active proof\nproof_policy: seam-test-first\nscope: [foreign/]\n```\n").unwrap();
    git(&foreign, &["add", "docs/milestones/021-proof.md"]);
    git(&foreign, &["commit", "-m", "foreign task authority"]);
    foreign_cli(&["task", "sync", "--file", "docs/milestones/021-proof.md"]);
    tasks::claim(
        &foreign,
        tasks::Claim {
            task_id: "other/m-proof:proof".into(),
            stage: "contract".into(),
            worker_id: "foreign-author".into(),
            worktree: foreign.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut foreign_store = TasksStore::open_project(
        &tasks::database_path(&foreign).unwrap(),
        "forge-mcp",
        "other",
    )
    .unwrap();
    let foreign_task = foreign_store.inspect("other/m-proof:proof").unwrap();
    let accepted = tasks::submit(&foreign, serde_json::from_value(json!({"task_id":foreign_task.task_id,"stage":"contract","action":"pass","evidence":evidence(&foreign_task)})).unwrap()).unwrap();
    let foreign_proof = accepted["snapshot"]["commit"].as_str().unwrap().to_owned();
    assert!(git(&foreign, &["status", "--porcelain"]).stdout.is_empty());
    // Simulate a crash after the archive write, before the SQLite transaction.
    root.write(
        "docs/milestones/archive/010-close.md",
        &format!("interrupted first archive write\n{foreign_proof}\n{selected_target}\n"),
    );
    assert!(std::process::Command::new("git").args(["notes", "--ref=forge", "add", "-f", "-m", r#"{"milestone_passports":{"forge-mcp/other/m-close":{"retained":true}},"external_note":"preserved"}"#, &handoff_commit]).current_dir(&root.0).output().unwrap().status.success());
    for reference in [
        "refs/forge/state/forge-mcp/forge-mcp/m-close/keep",
        "refs/forge/state/other/m-close/keep",
        "refs/forge/state/m-close/keep",
    ] {
        assert!(std::process::Command::new("git")
            .args(["update-ref", reference, &handoff_commit])
            .current_dir(&root.0)
            .output()
            .unwrap()
            .status
            .success());
    }
    // A terminal foreign manifest with unknown legacy preparation ownership
    // cannot authorize deletion of a global pin whose target is unknown.
    let anchor = git(&root.0, &["hash-object", "--stdin"]);
    let anchor = String::from_utf8(anchor.stdout).unwrap();
    let preparation = git(&root.0, &["notes", "--ref=forge", "show", anchor.trim()]);
    let mut unowned: serde_json::Value = serde_json::from_slice(&preparation.stdout).unwrap();
    unowned["handoff_proof_owners"]
        .as_object_mut()
        .unwrap()
        .retain(|_, owner| owner["identity"] != "forge-mcp/other/m-foreign");
    git(
        &root.0,
        &[
            "notes",
            "--ref=forge",
            "add",
            "-f",
            "-m",
            &serde_json::to_string(&unowned).unwrap(),
            anchor.trim(),
        ],
    );
    let before_active = std::fs::read(root.0.join("docs/milestones/010-close.md")).unwrap();
    let before_archive =
        std::fs::read(root.0.join("docs/milestones/archive/010-close.md")).unwrap();
    let legacy = milestone_cli(&root, &completed_args);
    assert!(!legacy.status.success());
    assert!(String::from_utf8_lossy(&legacy.stderr).contains("HANDOFF_PROOF_OWNERSHIP_UNAVAILABLE"));
    assert_eq!(
        std::fs::read(root.0.join("docs/milestones/010-close.md")).unwrap(),
        before_active
    );
    assert_eq!(
        std::fs::read(root.0.join("docs/milestones/archive/010-close.md")).unwrap(),
        before_archive
    );
    git(
        &root.0,
        &[
            "notes",
            "--ref=forge",
            "add",
            "-f",
            "-m",
            std::str::from_utf8(&preparation.stdout).unwrap(),
            anchor.trim(),
        ],
    );
    let completed = milestone_cli(&root, &completed_args);
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stderr)
    );
    assert!(!root.0.join("docs/milestones/010-close.md").exists());
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
    for table in [
        "tasks",
        "task_claims",
        "task_gates",
        "task_findings",
        "task_scope_paths",
        "task_dependencies",
        "task_submissions",
        "task_subtasks",
        "task_revisions",
    ] {
        let count: i64 = reopened
            .connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE task_id=?1"),
                [&task.task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    let foreign_tasks: i64 = reopened
        .connection
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE task_id=?1",
            [foreign_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(foreign_tasks, 1);
    let foreign_revisions: i64 = reopened
        .connection
        .query_row(
            "SELECT COUNT(*) FROM task_revisions WHERE task_id=?1",
            [foreign_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(foreign_revisions, 1);
    let satisfied: i64 = reopened
        .connection
        .query_row(
            "SELECT COUNT(*) FROM task_dependencies WHERE task_id=?1 AND satisfied=1",
            [foreign_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(satisfied, 2);
    let scopes: Vec<String> = reopened
        .connection
        .prepare("SELECT milestone_ref FROM task_blackboard WHERE task_id IS NULL")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(scopes, ["other/other/docs/milestones/010-close.md"]);
    let (terminal, completed): (i64, String) = reopened.connection.query_row(
        "SELECT all_tasks_terminal, completed_task_ids FROM milestone_tombstones WHERE milestone_id='forge-mcp/forge-mcp/m-close'", [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(terminal, 1);
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&completed).unwrap(),
        std::slice::from_ref(&task.task_id)
    );
    let tombstones: i64 = reopened
        .connection
        .query_row("SELECT COUNT(*) FROM milestone_tombstones", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(tombstones, 1);
    // A completed handoff remains successful after its task/gate rows are gone.
    let retried = milestone_cli(&root, &completed_args);
    assert!(
        retried.status.success(),
        "post-prune handoff retry must not require live tasks: {}",
        String::from_utf8_lossy(&retried.stderr)
    );
    // A stale manifest from an unmerged worktree must not resurrect archived tasks.
    root.write("docs/milestones/010-close.md", source);
    let synced = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("forge-mcp".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(synced["total"], 0);
    for reference in [
        "docs/milestones/010-close.md",
        "docs/milestones/archive/010-close.md",
    ] {
        let error = tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(reference.into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("STORE_SYNC_FAILED"));
    }
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
    std::fs::remove_file(root.0.join("docs/milestones/010-close.md")).unwrap();
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
        Some(handoff_commit.as_str())
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
    let passport: serde_json::Value =
        serde_yaml::from_value(meta["handoff"]["passport"].clone()).unwrap();
    assert_eq!(passport["tasks_count"], 2);
    assert_eq!(passport["linter"]["status"], "unobserved");
    assert_eq!(passport["test_suite_results"]["tests_passed"], 7);
    assert!(passport["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["status"] == "completed" && task["execution_wall_seconds"].is_u64()));
    assert!(passport["ast_symbol_deltas"].is_object());
    assert_eq!(
        passport["verified_handoff_proof"].as_str().unwrap().len(),
        64
    );
    let note = std::process::Command::new("git")
        .args(["notes", "--ref=forge", "show", &handoff_commit])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(note.status.success());
    let note: serde_json::Value = serde_json::from_slice(&note.stdout).unwrap();
    assert_eq!(
        note["milestone_passports"]["forge-mcp/forge-mcp/m-close"],
        passport
    );
    assert_eq!(
        note["milestone_passports"]["forge-mcp/other/m-close"]["retained"],
        true
    );
    assert_eq!(note["external_note"], "preserved");
    let anchor = git(&root.0, &["hash-object", "--stdin"]);
    let anchor = String::from_utf8(anchor.stdout).unwrap();
    let owners = git(&root.0, &["notes", "--ref=forge", "show", anchor.trim()]);
    let owners: serde_json::Value = serde_json::from_slice(&owners.stdout).unwrap();
    let owners = owners["handoff_proof_owners"].as_object().unwrap();
    assert!(!owners
        .values()
        .any(|owner| owner["identity"] == "forge-mcp/forge-mcp/m-close"));
    assert!(owners
        .values()
        .any(|owner| owner["identity"] == "forge-mcp/other/m-foreign"
            && owner["proof_shas"]
                .as_array()
                .unwrap()
                .iter()
                .any(|sha| sha == &json!(selected_target))));
    assert_eq!(
        owners
            .values()
            .filter(|owner| owner["identity"] == "forge-mcp/other/m-foreign")
            .count(),
        1
    );
    assert!(meta["handoff"]["proof_shas"]
        .as_sequence()
        .unwrap()
        .iter()
        .any(|sha| sha.as_str() == Some(selected_target.as_str())));
    for (reference, retained) in [
        ("refs/forge/state/forge-mcp/forge-mcp/m-close/keep", false),
        ("refs/forge/state/other/m-close/keep", true),
        ("refs/forge/state/m-close/keep", true),
    ] {
        assert_eq!(
            std::process::Command::new("git")
                .args(["show-ref", "--verify", reference])
                .current_dir(&root.0)
                .output()
                .unwrap()
                .status
                .success(),
            retained
        );
    }
    assert!(!root
        .0
        .join("docs/milestones/archive/010-close.md.bak")
        .exists());
    let mut evidence_ref_observations = Vec::new();
    for sha in meta["handoff"]["proof_shas"].as_sequence().unwrap() {
        let reference = format!("refs/forge/evidence/{}", sha.as_str().unwrap());
        evidence_ref_observations.push((
            reference.clone(),
            std::process::Command::new("git")
                .args(["show-ref", "--verify", &reference])
                .current_dir(&root.0)
                .output()
                .unwrap()
                .status
                .success(),
            sha.as_str() == Some(selected_target.as_str())
                || sha.as_str() == Some(foreign_proof.as_str()),
        ));
    }
    let mut recovery_failures = Vec::new();
    for scenario in [
        "post-prune",
        "torn",
        "foreign",
        "nonterminal",
        "cancelled-to-completed",
        "completed-to-cancelled",
        "swapped-terminal-statuses",
        "missing-active-receipt",
        "altered-active-receipt",
        "missing-receipts",
        "foreign-receipt",
        "altered-landed-receipt",
        "missing-terminal-metadata",
        "missing-source-receipt-key",
        "missing-landed-receipt-key",
        "unknown-terminal-task",
        "unknown-terminal-status",
        "missing-passport-outcome",
        "duplicate-passport-outcome",
        "unmerged",
        "git-archive",
    ] {
        let active = "docs/milestones/010-close.md";
        let archive = "docs/milestones/archive/010-close.md";
        root.write(active, &delivered_active);
        root.write(archive, &archived);
        match scenario {
            "torn"
            | "nonterminal"
            | "cancelled-to-completed"
            | "completed-to-cancelled"
            | "swapped-terminal-statuses"
            | "missing-active-receipt"
            | "altered-active-receipt"
            | "missing-receipts"
            | "foreign-receipt"
            | "altered-landed-receipt"
            | "missing-terminal-metadata"
            | "missing-source-receipt-key"
            | "missing-landed-receipt-key"
            | "unknown-terminal-task"
            | "unknown-terminal-status"
            | "missing-passport-outcome"
            | "duplicate-passport-outcome" => root.write(archive, "---\nid: m-close\nbroken: [\n"),
            "foreign" => root.write(
                archive,
                &archived.replacen("id: m-close", "id: m-foreign", 1),
            ),
            "unmerged" => std::fs::remove_file(root.0.join(archive)).unwrap(),
            "git-archive" => {
                std::fs::remove_file(root.0.join(active)).unwrap();
                commit_handoff_fixture_inputs(&root, &["docs"]);
                std::fs::remove_file(root.0.join(archive)).unwrap();
            }
            _ => {}
        }
        if scenario == "nonterminal" {
            root.write(active, source);
        }
        if scenario == "cancelled-to-completed" {
            root.write(
                active,
                &delivered_active.replace("status: cancelled", "status: completed"),
            );
        } else if scenario == "completed-to-cancelled" {
            // The first completed status belongs to the delivered task, not the active header.
            root.write(
                active,
                &delivered_active.replacen("status: completed", "status: cancelled", 1),
            );
        } else if scenario == "swapped-terminal-statuses" {
            root.write(
                active,
                &delivered_active
                    .replace("status: completed", "status: TEMP")
                    .replace("status: cancelled", "status: completed")
                    .replace("status: TEMP", "status: cancelled"),
            );
        }
        if scenario == "missing-active-receipt" {
            let mut skipping = false;
            let mut changed = String::new();
            for line in delivered_active.split_inclusive('\n') {
                if line.starts_with("receipt:") {
                    skipping = true;
                    continue;
                }
                if skipping && (line.starts_with(' ') || line.trim().is_empty()) {
                    continue;
                }
                skipping = false;
                changed.push_str(line);
            }
            assert_ne!(changed, delivered_active);
            root.write(active, &changed);
        } else if scenario == "altered-active-receipt" {
            let receipt = Milestone::parse(&delivered_active, "forge-mcp")
                .unwrap()
                .tasks[0]
                .receipt
                .clone()
                .unwrap();
            root.write(
                active,
                &delivered_active.replace(&receipt.passed_at, "2000-01-01T00:00:00Z"),
            );
        }
        let mut current_note = note.clone();
        let durable = &mut current_note["milestone_passports"]["forge-mcp/forge-mcp/m-close"];
        let task_id = "forge-mcp/m-close:finish";
        match scenario {
            "missing-receipts" => durable["landed_receipts"] = json!({}),
            "foreign-receipt" => {
                durable["landed_receipts"]["other/m-close:finish"] =
                    durable["landed_receipts"][task_id].clone()
            }
            "altered-landed-receipt" => {
                durable["landed_receipts"][task_id]["passed_at"] = "2000-01-01T00:00:00Z".into()
            }
            "missing-terminal-metadata" => {
                durable.as_object_mut().unwrap().remove("terminal_tasks");
            }
            "missing-source-receipt-key" | "missing-landed-receipt-key" => {
                let key = if scenario == "missing-source-receipt-key" {
                    "source_receipt"
                } else {
                    "landed_receipt"
                };
                durable["terminal_tasks"][task_id]
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
            }
            "unknown-terminal-task" => {
                durable["terminal_tasks"]["forge-mcp/m-close:unknown"] =
                    durable["terminal_tasks"][task_id].clone()
            }
            "unknown-terminal-status" => {
                durable["terminal_tasks"][task_id]["status"] = "unknown".into()
            }
            "missing-passport-outcome" => {
                durable["tasks"].as_array_mut().unwrap().pop();
            }
            "duplicate-passport-outcome" => {
                let duplicate = durable["tasks"][0].clone();
                durable["tasks"].as_array_mut().unwrap().push(duplicate);
            }
            _ => {}
        }
        // Keep the checksum valid so semantic reconciliation, rather than the codec,
        // must reject contradictory durable terminal data.
        use sha2::{Digest, Sha256};
        durable
            .as_object_mut()
            .unwrap()
            .remove("verified_handoff_proof");
        durable["verified_handoff_proof"] =
            format!("{:x}", Sha256::digest(serde_json::to_vec(durable).unwrap())).into();
        git(
            &root.0,
            &[
                "notes",
                "--ref=forge",
                "add",
                "-f",
                "-m",
                &serde_json::to_string(&current_note).unwrap(),
                &handoff_commit,
            ],
        );
        let before_active = std::fs::read(root.0.join(active)).ok();
        let before_archive = std::fs::read(root.0.join(archive)).ok();
        let output = milestone_cli(&root, &completed_args);
        let success = matches!(scenario, "post-prune" | "torn" | "git-archive");
        if output.status.success() != success {
            recovery_failures.push(format!(
                "{scenario}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
            continue;
        }
        if success {
            assert!(!root.0.join(active).exists(), "{scenario}");
            if scenario == "torn" {
                let repaired = std::fs::read_to_string(root.0.join(archive)).unwrap();
                let repaired = Milestone::parse(&repaired, "forge-mcp").unwrap();
                let original = Milestone::parse(&archived, "forge-mcp").unwrap();
                assert_eq!(
                    serde_json::to_value(&repaired.tasks[0].receipt).unwrap(),
                    serde_json::to_value(&original.tasks[0].receipt).unwrap()
                );
            }
        } else {
            assert_eq!(
                std::fs::read(root.0.join(active)).ok(),
                before_active,
                "{scenario}"
            );
            assert_eq!(
                std::fs::read(root.0.join(archive)).ok(),
                before_archive,
                "{scenario}"
            );
            if scenario == "unmerged" {
                assert!(String::from_utf8_lossy(&output.stderr)
                    .contains("MILESTONE_ALREADY_HANDED_OFF"));
            }
        }
        assert!(reopened.list(None, "all", None).unwrap().is_empty());
    }
    assert!(
        recovery_failures.is_empty(),
        "recovery matrix: {recovery_failures:?}"
    );
    for (reference, exists, owned_by_foreign) in evidence_ref_observations {
        assert_eq!(
            exists, owned_by_foreign,
            "active foreign prepared target owns {reference}"
        );
    }
    // Once both genuine foreign owners leave active state, repeated outgoing
    // cleanup releases the shared pins instead of retaining all evidence forever.
    foreign_store
        .delete(Some("other/m-proof:proof"), None, true, 0)
        .unwrap();
    for path in ["020-foreign.md", "021-proof.md", "022-same-id.md"] {
        std::fs::remove_file(foreign.join("docs/milestones").join(path)).unwrap();
    }
    for _ in 0..2 {
        let retried = milestone_cli(&root, &completed_args);
        assert!(
            retried.status.success(),
            "{}",
            String::from_utf8_lossy(&retried.stderr)
        );
        for sha in [&selected_target, &foreign_proof] {
            let pin = format!("refs/forge/evidence/{sha}");
            assert!(!std::process::Command::new("git")
                .args(["show-ref", "--verify", &pin])
                .current_dir(&root.0)
                .output()
                .unwrap()
                .status
                .success());
        }
    }
    let fallback = ScopedWorkspace::new("forge_handoff_claim_fallback");
    fallback.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nhandoff_pruning: false\n",
    );
    fallback.write("src/lib.rs", "pub fn fallback() {}\n");
    let source = "---\nid: m-fallback\ntitle: Fallback work\ndoc_type: contract\nstatus: active\n---\n# Fallback work\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish fallback\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    fallback.write("docs/milestones/020-fallback.md", source);
    commit_handoff_fixture_inputs(
        &fallback,
        &["forge-mcp.yaml", "docs/milestones/020-fallback.md"],
    );
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let mut fallback_store = TasksStore::open(&fallback.0.join(".forge/tasks.sqlite")).unwrap();
    fallback_store
        .sync(&milestone, "docs/milestones/020-fallback.md", &fallback.0)
        .unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    complete_task_through_engine(&fallback, &fallback_store, &id);
    let claim_time = fallback_store
        .earliest_claim("forge-mcp/m-fallback:")
        .unwrap()
        .unwrap();
    drop(fallback_store);
    fallback.write(".forge/acdd/profile.yaml", "commands:\n  project-check: 'test -s src/lib.rs'\n  project-lint: '! grep -q BAD_LINT src/lib.rs'\n  failing-lint: 'grep -q BAD_LINT src/lib.rs'\n");
    let before_polish = git_head(&fallback.0);
    fallback.write(
        "src/lib.rs",
        "pub fn fallback() { /* integrated source polish */ }\n",
    );
    let mut fallback_evidence =
        fixture_integration_evidence(&fallback.0, "m-fallback", "cargo test", 1, 0);
    let fallback_commit = git_head(&fallback.0);
    assert_ne!(fallback_commit, before_polish);
    let evidence_path = fallback.0.join(".forge/polish-evidence.json");
    fallback_evidence.verification.project_command = Some("project-check".into());
    fallback_evidence.verification.lint_command = Some("failing-lint".into());
    std::fs::write(
        &evidence_path,
        serde_json::to_vec(&fallback_evidence).unwrap(),
    )
    .unwrap();
    let failed_lint = milestone_cli(
        &fallback,
        &[
            "handoff",
            "m-fallback",
            "--evidence-file",
            evidence_path.to_str().unwrap(),
        ],
    );
    assert!(!failed_lint.status.success());
    assert!(String::from_utf8_lossy(&failed_lint.stderr).contains("HANDOFF_VERIFICATION_FAILED"));
    assert!(fallback.0.join("docs/milestones/020-fallback.md").exists());
    assert!(!fallback
        .0
        .join("docs/milestones/archive/020-fallback.md")
        .exists());
    fallback_evidence.verification.lint_command = Some("project-lint".into());
    std::fs::write(
        &evidence_path,
        serde_json::to_vec(&fallback_evidence).unwrap(),
    )
    .unwrap();
    let result = milestone_cli(
        &fallback,
        &[
            "handoff",
            "m-fallback",
            "--commit",
            fallback_commit.as_str(),
            "--evidence-file",
            evidence_path.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let retained = TasksStore::open(&fallback.0.join(".forge/tasks.sqlite")).unwrap();
    let retained_task = retained.inspect(&id).unwrap();
    assert_eq!(retained_task.status, "completed");
    assert_eq!(
        retained_task.milestone_ref,
        "docs/milestones/archive/020-fallback.md"
    );
    assert!(!retained.milestone_tombstoned("m-fallback").unwrap());
    let text = std::fs::read_to_string(fallback.0.join("docs/milestones/archive/020-fallback.md"))
        .unwrap();
    let header = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    assert_eq!(
        meta["handoff"]["passport"]["linter"]["status"].as_str(),
        Some("clean")
    );
    assert_eq!(
        meta["handoff"]["passport"]["linter"]["observation"]["exit_code"].as_i64(),
        Some(0)
    );
    assert_eq!(
        meta["handoff"]["passport"]["linter"]["observation"]["command"].as_str(),
        Some("! grep -q BAD_LINT src/lib.rs")
    );
    assert_eq!(
        meta["handoff"]["passport"]["automatic_project_verification"]["selection"].as_str(),
        Some("project-check")
    );
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

    let mut failures = Vec::new();
    for scenario in [
        "archive",
        "ready",
        "manifest-status",
        "digest",
        "revision",
        "missing-task",
        "cancelled-scope",
        "cancelled-receipt",
        "old-ref",
        "scope-tamper",
        "dependent",
        "rollback",
    ] {
        let root = ScopedWorkspace::new("forge_retained_cancelled_handoff");
        root.write(".gitignore", ".forge/\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root.write("src/lib.rs", "pub fn finish() -> u8 { 0 }\n");
        root.write("deferred/work.rs", "pub fn transferred() -> u8 { 0 }\n");
        let active = "docs/milestones/030-terminal.md";
        let archive = "docs/milestones/archive/030-terminal.md";
        let source = "---\nid: m-terminal\ntitle: Retained terminal authority\ndoc_type: contract\nstatus: active\n---\n# Terminal authority\n```yaml\ntask_ref: finish\ntarget: Complete admitted work\nproof_policy: seam-test-first\nscope: [src/lib.rs]\n```\n```yaml\ntask_ref: transferred\ntarget: Work transferred to another milestone\nproof_policy: seam-test-first\ncontract_revision: 1\nscope: [deferred/]\ndepends_on: [finish]\nsubtasks:\n- subtask_ref: retained-case\n  title: Retain cancellation history\n```\n";
        root.write(active, source);
        commit_handoff_fixture_inputs(
            &root,
            &[".gitignore", "forge-mcp.yaml", "src", "deferred", "docs"],
        );
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, active, &root.0).unwrap();
        let finish_id = milestone.task_id(&milestone.tasks[0]);
        let transferred_id = milestone.task_id(&milestone.tasks[1]);
        root.write("src/lib.rs", "pub fn finish() -> u8 { 1 }\n");
        complete_task_through_engine(&root, &store, &finish_id);
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: transferred_id.clone(),
                stage: "contract".into(),
                worker_id: "transferred-contract-author".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let delivered = std::fs::read_to_string(root.0.join(active)).unwrap();
        let cancelled = delivered.replace(
            "task_ref: transferred\n",
            "task_ref: transferred\nstatus: cancelled\n",
        );
        root.write(active, &cancelled);
        store
            .sync(
                &Milestone::parse(&cancelled, "forge-mcp").unwrap(),
                active,
                &root.0,
            )
            .unwrap();
        let cancelled_history = store.inspect_details(&transferred_id).unwrap();
        assert_eq!(cancelled_history["status"], "cancelled");
        assert!(!cancelled_history["attempts"].as_array().unwrap().is_empty());
        assert!(store.inspect(&transferred_id).unwrap().receipt.is_none());
        let completed_receipt = store.inspect(&finish_id).unwrap().receipt.unwrap();
        match scenario {
            "ready" => {
                let ready = cancelled
                    .replace("status: cancelled\n", "status: ready\n")
                    .replace(
                        "contract_revision: 1\nscope: [deferred/]",
                        "contract_revision: 2\nscope: [deferred/]",
                    );
                root.write(active, &ready);
                store
                    .sync(
                        &Milestone::parse(&ready, "forge-mcp").unwrap(),
                        active,
                        &root.0,
                    )
                    .unwrap();
            }
            "manifest-status" => root.write(
                active,
                &cancelled.replace("status: cancelled\n", "status: ready\n"),
            ),
            "digest" => root.write(
                active,
                &cancelled.replace(
                    "Work transferred to another milestone",
                    "Unaccepted changed authority",
                ),
            ),
            "revision" => root.write(
                active,
                &cancelled.replace(
                    "contract_revision: 1\nscope: [deferred/]",
                    "contract_revision: 2\nscope: [deferred/]",
                ),
            ),
            "missing-task" => root.write(
                active,
                cancelled
                    .split("```yaml\ntask_ref: transferred")
                    .next()
                    .unwrap(),
            ),
            "cancelled-scope" => {
                root.write("deferred/work.rs", "pub fn transferred() -> u8 { 99 }\n")
            }
            "cancelled-receipt" | "old-ref" => {
                let mut task = store.inspect(&transferred_id).unwrap();
                if scenario == "cancelled-receipt" {
                    task.receipt = Some(completed_receipt.clone());
                    task.spec.receipt = task.receipt.clone();
                } else {
                    task.milestone_ref = "docs/milestones/foreign.md".into();
                }
                store
                    .connection
                    .execute(
                        "UPDATE tasks SET descriptor=?1, milestone_ref=?2 WHERE task_id=?3",
                        rusqlite::params![
                            serde_json::to_string(&task).unwrap(),
                            task.milestone_ref,
                            transferred_id
                        ],
                    )
                    .unwrap();
            }
            "scope-tamper" => {
                let mut task = store.inspect(&finish_id).unwrap();
                task.spec.scope.push("unadmitted/outside.rs".into());
                store
                    .connection
                    .execute(
                        "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
                        rusqlite::params![serde_json::to_string(&task).unwrap(), finish_id],
                    )
                    .unwrap();
            }
            "dependent" => {
                let pending = format!("{cancelled}\n```yaml\ntask_ref: dependent\ntarget: Must remain blocked on cancelled work\nproof_policy: seam-test-first\nscope: [dependent/]\ndepends_on: [transferred]\n```\n");
                root.write(active, &pending);
                let pending_milestone = Milestone::parse(&pending, "forge-mcp").unwrap();
                store.sync(&pending_milestone, active, &root.0).unwrap();
                let claimed = tasks::claim(
                    &root.0,
                    tasks::Claim {
                        task_id: pending_milestone.task_id(&pending_milestone.tasks[2]),
                        stage: "contract".into(),
                        worker_id: "dependent-author".into(),
                        worktree: root.0.to_string_lossy().into_owned(),
                        ..Default::default()
                    },
                );
                assert!(
                    claimed.is_err(),
                    "cancelled task must not satisfy dependencies"
                );
            }
            "rollback" => {
                store.connection.execute_batch("CREATE TRIGGER reject_cancelled_pruning BEFORE DELETE ON tasks WHEN OLD.task_id LIKE '%:transferred' BEGIN SELECT RAISE(ABORT, 'fixture pruning failure'); END;").unwrap();
            }
            _ => {}
        }
        commit_handoff_fixture_inputs(&root, &["docs", "deferred"]);
        let before_doc = std::fs::read(root.0.join(active)).unwrap();
        let before_tasks = store
            .list(None, "all", None)
            .unwrap()
            .into_iter()
            .map(|task| {
                let details = store.inspect_details(&task.task_id).unwrap();
                (task.task_id, details)
            })
            .collect::<Vec<_>>();
        let evidence_file =
            fixture_evidence_file(&root.0, "m-terminal", "cargo test --test acdd", 1, 0);
        if scenario == "rollback" {
            root.write(archive, "previous torn archive preserved on rollback\n");
        }
        if scenario == "archive" {
            let evidence =
                fixture_integration_evidence(&root.0, "m-terminal", "cargo test --test acdd", 1, 0);
            let context =
                milestone_engine::handoff_context(&root.0, "m-terminal", &evidence.target_base);
            if let Ok(context) = context {
                assert_eq!(context["binding"]["builders"], json!(["builder"]));
            } else {
                failures.push(format!(
                    "{scenario}: retained cancellation blocked handoff context: {}",
                    context.unwrap_err()
                ));
            }
        }
        let output = milestone_cli(
            &root,
            &["handoff", "m-terminal", "--evidence-file", &evidence_file],
        );
        let expected_success = scenario == "archive";
        if output.status.success() != expected_success {
            failures.push(format!(
                "{scenario}: expected success={expected_success}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
            continue;
        }
        if scenario == "scope-tamper" {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("TASK_AUTHORITY_CHANGED"),
                "unrecorded task-scope mutation must fail authority validation: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if expected_success {
            assert!(!root.0.join(active).exists());
            let archived_text = std::fs::read_to_string(root.0.join(archive)).unwrap();
            let archived = Milestone::parse(&archived_text, "forge-mcp").unwrap();
            let retained = archived
                .tasks
                .iter()
                .find(|spec| spec.task_ref == "transferred")
                .unwrap();
            assert_eq!(retained.status.as_deref(), Some("cancelled"));
            assert!(retained.receipt.is_none());
            assert_eq!(
                serde_json::to_value(&retained.subtasks).unwrap(),
                cancelled_history["subtasks"]
            );
            assert!(store.list(None, "all", None).unwrap().is_empty());
            let accepted = archived
                .tasks
                .iter()
                .find(|spec| spec.task_ref == "finish")
                .unwrap();
            assert!(completed_receipt.matches_proofs(accepted.receipt.as_ref().unwrap()));
            let dependencies: i64 = store.connection.query_row("SELECT count(*) FROM task_dependencies WHERE dependency_id=?1 AND satisfied<>0", [&transferred_id], |row| row.get(0)).unwrap();
            assert_eq!(dependencies, 0);
        } else {
            assert_eq!(
                std::fs::read(root.0.join(active)).unwrap(),
                before_doc,
                "{scenario}"
            );
            if scenario == "rollback" {
                assert_eq!(
                    std::fs::read_to_string(root.0.join(archive)).unwrap(),
                    "previous torn archive preserved on rollback\n"
                );
            } else {
                assert!(!root.0.join(archive).exists(), "{scenario}");
            }
            assert!(
                !store.milestone_tombstoned("m-terminal").unwrap(),
                "{scenario}"
            );
            for (id, details) in before_tasks {
                assert_eq!(
                    store.inspect_details(&id).unwrap(),
                    details,
                    "{scenario}: task history changed"
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn manual_delivery_handoff_rewrites_markdown_receipt_to_landed_commit() {
    let root = ScopedWorkspace::new("forge_manual_delivery_handoff");
    root.write("src/lib.rs", "pub fn manual_delivery() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom/manual.yaml\n",
    );
    let mut profile = (*contextunity_forge_mcp::core::tasks::profile::compiled()).clone();
    profile.gates.last_mut().unwrap().auto_commit = false;
    root.write(
        "custom/manual.yaml",
        &serde_yaml::to_string(&profile).unwrap(),
    );
    let source = "---\nid: m-manual\ntitle: Manual delivery\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Manual delivery\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish manually delivered work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-manual.md", source);
    commit_handoff_fixture_inputs(
        &root,
        &[
            "forge-mcp.yaml",
            "custom/manual.yaml",
            "docs/milestones/010-manual.md",
        ],
    );
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-manual.md", &root.0)
        .unwrap();
    let baseline = git_head(&root.0);
    complete_task_through_engine(&root, &store, &task_id);

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
    let candidate_parents = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", candidate])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(candidate_parents.status.success());
    assert_eq!(
        String::from_utf8_lossy(&candidate_parents.stdout)
            .split_whitespace()
            .count(),
        1,
        "captured candidates are parentless root snapshots"
    );
    assert_eq!(
        candidate_baseline, baseline,
        "the accepted snapshot already stores the branch baseline needed by handoff"
    );
    assert_eq!(
        git_head(&root.0),
        baseline,
        "manual delivery leaves HEAD unchanged"
    );

    let staged = std::process::Command::new("git")
        .args(["add", "--", "src/lib.rs", "docs/milestones/010-manual.md"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(staged.status.success());
    let commit = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "manual task landing",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let landed_commit = git_head(&root.0);
    drop(store);

    let result = milestone_cli(
        &root,
        &[
            "handoff",
            "m-manual",
            "--commit",
            landed_commit.as_str(),
            "--evidence-file",
            &fixture_evidence_file(&root.0, "m-manual", "cargo test --test acdd", 1, 0),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let archive =
        std::fs::read_to_string(root.0.join("docs/milestones/archive/010-manual.md")).unwrap();
    let archived = Milestone::parse(&archive, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(landed_commit.as_str()),
        "handoff must persist the landed Git commit into Markdown before archival"
    );
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
}

#[test]
fn terminal_task_delivery_rolls_up_durable_context_and_prunes_blackboard() {
    let root = ScopedWorkspace::new("forge_task_context_rollup");
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
    let milestone = Milestone::parse(&source, "forge-mcp").unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, milestone_ref, &root.0).unwrap();
    let candidate_baseline = git_head(&root.0);
    let run_git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };

    for (index, stage) in GATES.iter().enumerate() {
        let worker = match index {
            2 => "independent-reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: first.clone(),
                stage: (*stage).to_owned(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = store.inspect(&first).unwrap();
        let mut proof = evidence(&claimed);
        proof.commit = None;
        if index == 3 {
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
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":first,
                "stage":stage,
                "action":"pass",
                "evidence":proof,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, rejected).is_err());
            assert_eq!(store.inspect(&first).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&first, None, None).unwrap().len(), 2);
            root.write(milestone_ref, &source);
        }
        if index == 3 {
            root.write("src/head-moved-after-candidate.rs", "pub fn moved() {}\n");
            run_git(&["add", "--", "src/head-moved-after-candidate.rs"]);
            let committed = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=ACDD test",
                    "-c",
                    "user.email=acdd-test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-m",
                    "move HEAD after candidate capture",
                ])
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(
                committed.status.success(),
                "{}",
                String::from_utf8_lossy(&committed.stderr)
            );
            let moved_head = git_head(&root.0);
            assert_ne!(moved_head, candidate_baseline);
            root.write("src/staged-sentinel.rs", "pub fn staged() {}\n");
            run_git(&["add", "--", "src/staged-sentinel.rs"]);
            let staged_before = run_git(&["diff", "--cached", "--binary"]);
            let receipt_before = std::fs::read(root.0.join(milestone_ref)).unwrap();
            let task_before = store.inspect(&first).unwrap();
            let submissions_before: i64 = store
                .connection
                .query_row(
                    "SELECT count(*) FROM task_submissions WHERE task_id=?1",
                    [&first],
                    |row| row.get(0),
                )
                .unwrap();
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":first,
                "stage":stage,
                "action":"pass",
                "evidence":proof,
            }))
            .unwrap();
            let error = tasks::submit(&root.0, rejected).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("TASK_CANDIDATE_BASELINE_MISMATCH"),
                "{error:#}"
            );
            assert_eq!(git_head(&root.0), moved_head);
            assert_eq!(run_git(&["diff", "--cached", "--binary"]), staged_before);
            assert_eq!(
                std::fs::read(root.0.join(milestone_ref)).unwrap(),
                receipt_before
            );
            let task_after = store.inspect(&first).unwrap();
            assert_eq!(task_after.status, task_before.status);
            assert_eq!(task_after.stage, task_before.stage);
            assert_eq!(task_after.claim_revision, task_before.claim_revision);
            assert!(task_after.receipt.is_none());
            let submissions_after: i64 = store
                .connection
                .query_row(
                    "SELECT count(*) FROM task_submissions WHERE task_id=?1",
                    [&first],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(submissions_after, submissions_before);

            let reset = std::process::Command::new("git")
                .args(["reset", "--hard"])
                .arg(&candidate_baseline)
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(
                reset.status.success(),
                "{}",
                String::from_utf8_lossy(&reset.stderr)
            );
            assert_eq!(git_head(&root.0), candidate_baseline);
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":first,
            "stage":stage,
            "action":"pass",
            "evidence":proof,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        if index == 3 {
            assert_eq!(result["status"], "completed");
        }
    }

    let delivered = store.inspect(&first).unwrap();
    let receipt = serde_json::to_value(delivered.receipt.unwrap()).unwrap();
    assert_eq!(receipt["commit"], git_head(&root.0));
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
    let written = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
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
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
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
    for (index, stage) in GATES.iter().enumerate() {
        let worker = match index {
            2 => "second-independent-reviewer",
            3 => "second-delivery-reviewer",
            _ => "second-builder",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: second.clone(),
                stage: (*stage).to_owned(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = store.inspect(&second).unwrap();
        let proof = evidence(&claimed);
        if index == 1 {
            accepted_build = proof.proof.clone();
        }
        if index == 2 {
            accepted_review = proof.proof.clone();
        }
        if index == 3 {
            store
                .blackboard_post(&second, "architect", "decisions", "Retain retry notes.")
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
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":second,
                "stage":stage,
                "action":"pass",
                "evidence":proof,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, rejected).is_err());
            assert_eq!(store.inspect(&second).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 2);
            recovered.rollup.as_mut().unwrap().architectural_notes[0] =
                "decisions: Retain retry notes.".into();
            root.write(milestone_ref, &recovery_text(&recovered));
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":second,
            "stage":stage,
            "action":"pass",
            "evidence":proof,
        }))
        .unwrap();
        let completed = tasks::submit(&root.0, request).unwrap();
        if index == 3 {
            assert_eq!(completed["status"], "completed");
            assert_eq!(completed["receipt"]["passed_at"], "2026-10-02T00:00:00Z");
            assert!(store
                .blackboard_read(&second, None, None)
                .unwrap()
                .is_empty());
        }
    }

    let custom = ScopedWorkspace::new("forge_custom_multi_review_rollup");
    custom.write("src/lib.rs", "pub fn candidate() { let _version = 1; }\n");
    custom.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    custom.write(
        ".forge/acdd/profile.yaml",
        r#"gates:
  - id: contract
    role: contract_author
    proof: contract
  - id: build
    role: builder
    proof: command
    sha_snapshot: true
  - id: candidate_final
    role: builder
    proof: command
    sha_snapshot: true
  - id: review
    role: independent_reviewer
    proof: review
    independent_from: candidate_final
    reject_to: candidate_final
    contours: standard
  - id: security_review
    role: independent_reviewer
    proof: review
    independent_from: candidate_final
    reject_to: candidate_final
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: security_review
    review_sources: [review, security_review]
    reject_to: candidate_final
    auto_commit: true
"#,
    );
    let custom_ref = "docs/010-custom-review.md";
    let custom_source = "---\nid: m-custom-review\ntitle: Custom review rollup\ndoc_type: contract\nstatus: active\ninvariants: [custom-rollup]\n---\n# Custom review rollup\n```yaml\ntask_ref: custom\ntarget: Deliver a final reviewed candidate\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    custom.write(custom_ref, custom_source);
    let staged = std::process::Command::new("git")
        .args([
            "add",
            "--",
            "src/lib.rs",
            "forge-mcp.yaml",
            ".forge/acdd/profile.yaml",
            custom_ref,
        ])
        .current_dir(&custom.0)
        .output()
        .unwrap();
    assert!(
        staged.status.success(),
        "{}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let committed = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "initialize custom multi-review workflow",
        ])
        .current_dir(&custom.0)
        .output()
        .unwrap();
    assert!(
        committed.status.success(),
        "{}",
        String::from_utf8_lossy(&committed.stderr)
    );
    tasks::manage(
        &custom.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":custom_ref})).unwrap(),
    )
    .unwrap();
    let custom_id = "forge-mcp/m-custom-review:custom";
    let read_custom_task = || {
        let store = tasks::store(&custom.0).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [custom_id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str::<contextunity_forge_mcp::db::tasks_store::Task>(&descriptor).unwrap()
    };
    let custom_baseline = git_head(&custom.0);
    let mut build_candidate: Option<String> = None;
    let mut final_candidate: Option<String> = None;
    for (stage, worker) in [
        ("contract", "contract-author"),
        ("build", "builder-initial"),
        ("candidate_final", "builder-final"),
        ("review", "reviewer"),
        ("security_review", "security-reviewer"),
        ("deliver", "delivery-reviewer"),
    ] {
        if stage == "candidate_final" {
            custom.write("src/lib.rs", "pub fn candidate() { let _version = 2; }\n");
        }
        tasks::claim(
            &custom.0,
            tasks::Claim {
                task_id: custom_id.into(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: custom.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = read_custom_task();
        let mut proof = evidence(&claimed);
        match stage {
            "candidate_final" => {
                proof.proof = json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}});
            }
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
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":custom_id,
                "stage":stage,
                "action":"pass",
                "evidence":wrong_candidate,
            }))
            .unwrap();
            let error = tasks::submit(&custom.0, rejected).unwrap_err();
            assert!(
                error.to_string().contains("TASK_CANDIDATE_MISMATCH"),
                "{error:#}"
            );
            let unchanged = tasks::store(&custom.0).unwrap().inspect(custom_id).unwrap();
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
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":custom_id,
            "stage":stage,
            "action":"pass",
            "evidence":proof,
        }))
        .unwrap();
        tasks::submit(&custom.0, request).unwrap();
        if stage == "build" {
            build_candidate = Some(pinned_candidate(&custom.0, custom_id));
        }
        if stage == "candidate_final" {
            final_candidate = Some(pinned_candidate(&custom.0, custom_id));
            assert_ne!(final_candidate.as_deref(), Some(custom_baseline.as_str()));
        }
    }
    let candidate = final_candidate.unwrap();
    let custom_store = tasks::store(&custom.0).unwrap();
    let delivered_descriptor: String = custom_store
        .connection
        .query_row(
            "SELECT descriptor FROM tasks WHERE task_id=?1",
            [custom_id],
            |row| row.get(0),
        )
        .unwrap();
    let delivered: contextunity_forge_mcp::db::tasks_store::Task =
        serde_json::from_str(&delivered_descriptor).unwrap();
    let receipt = serde_json::to_value(delivered.receipt.unwrap()).unwrap();
    assert_eq!(
        receipt["evidence"]["command_proof"]["command"],
        "cargo test --test acdd"
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
    let stored_reviews: Vec<(String, String)> = {
        let mut statement = custom_store
            .connection
            .prepare("SELECT gate,evidence FROM task_gates WHERE task_id=?1 AND gate IN ('review','security_review') AND state='passed' ORDER BY gate")
            .unwrap();
        statement
            .query_map([custom_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(stored_reviews.len(), 2);
    for (_, evidence) in stored_reviews {
        let evidence: Value = serde_json::from_str(&evidence).unwrap();
        assert_eq!(evidence["commit"], candidate);
    }
    let reopened = tasks::store(&custom.0).unwrap().inspect(custom_id).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.receipt.unwrap()).unwrap(),
        receipt
    );

    let stale = ScopedWorkspace::new("forge_stale_review_source");
    stale.write("src/lib.rs", "pub fn candidate() { let _version = 1; }\n");
    stale.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    stale.write(
        ".forge/acdd/profile.yaml",
        r#"gates:
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
    independent_from: build
    reject_to: build
    contours: standard
  - id: candidate_final
    role: builder
    proof: command
    sha_snapshot: true
  - id: security_review
    role: independent_reviewer
    proof: review
    independent_from: candidate_final
    reject_to: candidate_final
    contours: standard
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: security_review
    review_sources: [review, security_review]
    reject_to: candidate_final
    auto_commit: true
"#,
    );
    let stale_ref = "docs/010-stale-review.md";
    let stale_contract = "---\nid: m-stale-review\ntitle: Stale review source\ndoc_type: contract\nstatus: active\n---\n# Stale review source\n```yaml\ntask_ref: stale\ntarget: Reject a stale review source at delivery\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    stale.write(stale_ref, stale_contract);
    let staged = std::process::Command::new("git")
        .args([
            "add",
            "--",
            "src/lib.rs",
            "forge-mcp.yaml",
            ".forge/acdd/profile.yaml",
            stale_ref,
        ])
        .current_dir(&stale.0)
        .output()
        .unwrap();
    assert!(
        staged.status.success(),
        "{}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let committed = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "initialize stale review workflow",
        ])
        .current_dir(&stale.0)
        .output()
        .unwrap();
    assert!(
        committed.status.success(),
        "{}",
        String::from_utf8_lossy(&committed.stderr)
    );
    tasks::manage(
        &stale.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":stale_ref})).unwrap(),
    )
    .unwrap();
    let stale_id = "forge-mcp/m-stale-review:stale";
    let read_stale_task = || {
        let store = tasks::store(&stale.0).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [stale_id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str::<contextunity_forge_mcp::db::tasks_store::Task>(&descriptor).unwrap()
    };
    let stale_persisted_state = || {
        let store = tasks::store(&stale.0).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [stale_id],
                |row| row.get(0),
            )
            .unwrap();
        let gates: Vec<(String, u64, String, Option<String>)> = {
            let mut statement = store
                .connection
                .prepare("SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY gate,revision")
                .unwrap();
            statement
                .query_map([stale_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let claims: Vec<(u64, String, String, i64, i64)> = {
            let mut statement = store
                .connection
                .prepare("SELECT revision,worker_id,worktree,claimed_at,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")
                .unwrap();
            statement
                .query_map([stale_id], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let submissions: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_submissions WHERE task_id=?1",
                [stale_id],
                |row| row.get(0),
            )
            .unwrap();
        (descriptor, gates, claims, submissions)
    };
    let mut stale_build_candidate: Option<String> = None;
    let mut stale_final_candidate: Option<String> = None;
    for (stage, worker) in [
        ("contract", "contract-author"),
        ("build", "builder-before-snapshot"),
        ("review", "early-reviewer"),
        ("candidate_final", "builder-final"),
        ("security_review", "final-reviewer"),
        ("deliver", "delivery-reviewer"),
    ] {
        if stage == "candidate_final" {
            stale.write("src/lib.rs", "pub fn candidate() { let _version = 2; }\n");
        }
        tasks::claim(
            &stale.0,
            tasks::Claim {
                task_id: stale_id.into(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: stale.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = read_stale_task();
        let mut proof = evidence(&claimed);
        match stage {
            "candidate_final" => {
                proof.proof = json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}});
            }
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
                let store = tasks::store(&stale.0).unwrap();
                let reviews: Vec<(String, String)> = {
                    let mut statement = store
                        .connection
                        .prepare("SELECT gate,evidence FROM task_gates WHERE task_id=?1 AND gate IN ('review','security_review') AND state='passed' ORDER BY gate")
                        .unwrap();
                    statement
                        .query_map([stale_id], |row| Ok((row.get(0)?, row.get(1)?)))
                        .unwrap()
                        .collect::<rusqlite::Result<_>>()
                        .unwrap()
                };
                assert_eq!(reviews.len(), 2);
                for (gate, evidence) in reviews {
                    let evidence: Value = serde_json::from_str(&evidence).unwrap();
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

                let stage_file = std::process::Command::new("git")
                    .args(["add", "--", "src/lib.rs"])
                    .current_dir(&stale.0)
                    .output()
                    .unwrap();
                assert!(stage_file.status.success());
                let head_before = git_head(&stale.0);
                let index_before = std::process::Command::new("git")
                    .args(["diff", "--cached", "--binary"])
                    .current_dir(&stale.0)
                    .output()
                    .unwrap();
                assert!(index_before.status.success());
                assert!(
                    !index_before.stdout.is_empty(),
                    "the stale-source case should preserve a non-empty staged index"
                );
                let document_before = std::fs::read(stale.0.join(stale_ref)).unwrap();
                let state_before = stale_persisted_state();
                let request: tasks::Submit = serde_json::from_value(json!({
                    "task_id":stale_id,
                    "stage":stage,
                    "action":"pass",
                    "evidence":proof,
                }))
                .unwrap();
                let error = tasks::submit(&stale.0, request).unwrap_err();
                assert!(
                    error.to_string().contains("TASK_CANDIDATE_MISMATCH")
                        && error.to_string().contains("review source 'review'"),
                    "delivery should identify the stale review source: {error:#}"
                );
                assert_eq!(git_head(&stale.0), head_before);
                let index_after = std::process::Command::new("git")
                    .args(["diff", "--cached", "--binary"])
                    .current_dir(&stale.0)
                    .output()
                    .unwrap();
                assert!(index_after.status.success());
                assert_eq!(index_after.stdout, index_before.stdout);
                assert_eq!(
                    std::fs::read(stale.0.join(stale_ref)).unwrap(),
                    document_before
                );
                assert_eq!(stale_persisted_state(), state_before);
                continue;
            }
            _ => {}
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":stale_id,
            "stage":stage,
            "action":"pass",
            "evidence":proof,
        }))
        .unwrap();
        tasks::submit(&stale.0, request).unwrap();
        if stage == "build" {
            stale_build_candidate = Some(pinned_candidate(&stale.0, stale_id));
        }
        if stage == "candidate_final" {
            stale_final_candidate = Some(pinned_candidate(&stale.0, stale_id));
            assert_ne!(stale_build_candidate, stale_final_candidate);
        }
    }
}
#[test]
fn first_cli_task_claim_includes_claim_guidance_and_activates_planned_milestone() {
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
            "--format",
            "json",
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
    let claim: Value = serde_json::from_slice(&output.stdout).unwrap();
    let guidance = &claim["workflow_guidance"];
    const PROTECTED_RULE: &str = "PROTECTED: milestone id, invariants, owners, depends_on, and acdd_profile are immutable while tasks are active. Declare task invariants inside task.invariants.";
    let guidance_text = serde_json::to_string(guidance).unwrap();
    let actual_rules = guidance["rules"].as_array().cloned().unwrap_or_default();
    let expected_rules = [
        format!(
            "ISOLATION: Develop exclusively inside dedicated worktree '{}'.",
            root.0.display()
        ),
        "IMMUTABILITY: Keep admitted scope unchanged after review pass and during deliver."
            .to_owned(),
        "INDEPENDENCE: Review stage requires a worker_id distinct from the accepted builder."
            .to_owned(),
    ];
    let missing_rules: Vec<_> = expected_rules
        .iter()
        .filter(|expected| {
            !actual_rules
                .iter()
                .any(|actual| actual.as_str() == Some(expected.as_str()))
        })
        .collect();
    assert!(
        guidance_text.contains(PROTECTED_RULE) && missing_rules.is_empty(),
        "CLI claim omitted protected guidance or operational rules; missing {missing_rules:?}; guidance: {guidance}"
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
            .args([
                "--format",
                "json",
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "blackboard",
            ])
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
    let stages = ["contract", "build", "review", "deliver"];
    let mut candidate_commit = None;
    let mut delivery_commit = None;

    for (index, stage) in stages.iter().enumerate() {
        let before = inspect();
        assert_eq!(before["workflow_guidance"]["active_stage"], *stage);
        assert!(before["workflow_guidance"]["role"].is_string());
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
        assert_eq!(
            claimed["workflow_guidance"]["active_stage"],
            before["workflow_guidance"]["active_stage"]
        );
        assert_eq!(
            claimed["workflow_guidance"]["role"],
            before["workflow_guidance"]["role"]
        );
        assert!(claimed["workflow_guidance"]["blackboard_messages"].is_array());
        if index == 2 {
            blackboard(&[
                "post",
                &task_id,
                "--author",
                "architect",
                "--topic",
                "decisions",
                "--payload",
                "Keep task context in the milestone receipt.",
            ]);
            blackboard(&[
                "post",
                &task_id,
                "--author",
                "builder",
                "--topic",
                "notes",
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
            assert!(rejected.to_string().contains("TASK_STAGE_UNKNOWN"));
            assert_eq!(inspect()["status"], "in_progress");
        }
        let mut submitted_evidence = evidence(&stored);
        submitted_evidence.commit = None;
        let submitted = tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id":task_id,
                "stage":stage,
                "action":"pass",
                "evidence":serde_json::to_value(submitted_evidence).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        if index == 1 {
            candidate_commit = Some(pinned_candidate(&root.0, &task_id));
        }
        if let Some(candidate) = candidate_commit.as_deref() {
            if matches!(index, 1 | 2) {
                let short_candidate = &candidate[..7];
                assert_eq!(submitted["snapshot"]["commit"], candidate);
                assert_eq!(
                    submitted["snapshot"]["inspect_cmd"],
                    format!("git show {short_candidate}")
                );
            }
        }
        if index == stages.len() - 1 {
            assert_eq!(submitted["status"], "completed");
            let landed = git_head(&root.0);
            assert_eq!(submitted["snapshot"]["commit"], landed);
            assert_eq!(submitted["receipt"]["commit"], landed);
            delivery_commit = Some(landed);
        }
    }

    let details = tasks::store(&root.0)
        .unwrap()
        .inspect_details(&task_id)
        .unwrap();
    let candidate = candidate_commit.as_deref().unwrap();
    let short_candidate = &candidate[..7];
    assert_eq!(details["latest_snapshot"]["commit"], candidate);
    assert_eq!(
        details["latest_snapshot"]["inspect_cmd"],
        format!("git show {short_candidate}")
    );
    let delivery = delivery_commit.as_deref().unwrap();
    assert_ne!(candidate, delivery);

    let written = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
    let parsed = Milestone::parse(&written, "forge-mcp").unwrap();
    let completed = &parsed.tasks[0];
    assert_eq!(completed.status.as_deref(), Some("completed"));
    assert_eq!(completed.agent_type.as_deref(), Some("gpt-6-sol"));
    let receipt = completed.receipt.as_ref().unwrap();
    assert!(receipt.commit.is_none());
    let sqlite_receipt = tasks::store(&root.0)
        .unwrap()
        .inspect(&task_id)
        .unwrap()
        .receipt
        .unwrap();
    assert_eq!(sqlite_receipt.commit.as_deref(), Some(delivery));
    let mut sqlite_receipt = serde_json::to_value(sqlite_receipt).unwrap();
    sqlite_receipt.as_object_mut().unwrap().remove("commit");
    assert_eq!(serde_json::to_value(receipt).unwrap(), sqlite_receipt);
    assert!(receipt.evidence.get("command_proof").is_some());
    assert!(receipt.review.get("review_proof").is_some());
    let rollup = receipt.rollup.as_ref().unwrap();
    assert_eq!(rollup.verified_invariants, ["durable-context", "reviewed"]);
    assert_eq!(
        rollup.architectural_notes,
        ["decisions: Keep task context in the milestone receipt."]
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
fn milestone_and_plan_directories_configured_and_excluded_from_scanner() {
    let root = ScopedWorkspace::new("forge_multi_milestones_and_plans");
    std::fs::write(
        root.0.join(".git/info/exclude"),
        ".forge/\ncode_map.sqlite*\n",
    )
    .unwrap();
    root.write("src/lib.rs", "pub fn platform() {}\n");
    root.write(
        "docs/architecture.md",
        "# Platform Architecture\nGeneral documentation.\n",
    );
    root.write(
        "docs/milestones/README.md",
        "# Milestone Index\nrootmilestoneoverviewtoken\n",
    );
    root.write(
        "docs/milestones/archive/README.md",
        "archivemilestoneoverviewtoken\n",
    );
    root.write(
        "docs/plans/README.md",
        "# Plan Index\nrootplanoverviewtoken\n",
    );
    root.write("docs/plans/archive/README.md", "archiveplanoverviewtoken\n");
    root.write("docs/plans/platform_plan.md", "---\nid: p-platform\ntitle: Platform Plan\ndoc_type: plan\npurpose: Platform\n---\n# Platform Plan\nrootplancontractisolationtoken\n");
    root.write("docs/milestones/010-platform.md", "---\nid: m-platform\ntitle: Platform Milestone\ndoc_type: contract\nstatus: active\n---\n# Platform Milestone\nrootmilestonecontractisolationtoken\n### task: ptask\n```yaml\ntask_ref: ptask\ntarget: Deliver ptask\nproof_policy: direct-proof\nscope: [src/]\n```\n");
    root.write("extensions/commerce/src/models.py", "# Commerce models\n");
    root.write(
        "extensions/commerce/docs/README.md",
        "# Commerce Readme\nDocumentation for commerce.\n",
    );
    root.write(
        "extensions/commerce/docs/milestones/README.md",
        "# Commerce Milestones\ncommercemilestoneoverviewtoken\n",
    );
    root.write(
        "extensions/commerce/docs/plans/README.md",
        "# Commerce Plans\ncommerceplanoverviewtoken\n",
    );
    root.write("extensions/commerce/docs/plans/commerce_plan.md", "---\nid: p-commerce\ntitle: Commerce Plan\ndoc_type: plan\npurpose: Commerce\n---\n# Commerce Plan\ncommerceplancontractisolationtoken\n");
    root.write("extensions/commerce/docs/milestones/010-commerce.md", "---\nid: m-commerce\ntitle: Commerce Milestone\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Commerce Milestone\ncommercemilestonecontractisolationtoken\n### task: ctask\n```yaml\ntask_ref: ctask\ntarget: Deliver ctask\nproof_policy: direct-proof\nscope: [extensions/commerce/]\n```\n");

    root.write(
        "forge-mcp.yaml",
        "roots: [src, extensions]\ndocs: [docs]\nmilestones:\n  - docs/milestones\n  - extensions/*/docs/milestones\nplans:\n  - docs/plans\n  - extensions/*/docs/plans\ntasks_db: .forge/tasks.sqlite\n",
    );

    // Documentation navigation includes contracts and proposals; lifecycle
    // discovery still uses its configured directories and separate authority.
    let scan = contextunity_forge_mcp::engine::scanner::scan(&root.0, None).unwrap();
    let scanned_paths: Vec<_> = scan.entries.iter().map(|e| e.path.as_str()).collect();

    assert!(scanned_paths.contains(&"src/lib.rs"));
    assert!(scanned_paths.contains(&"docs/architecture.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/src/models.py"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/README.md"));
    assert!(scanned_paths.contains(&"docs/milestones/README.md"));
    assert!(scanned_paths.contains(&"docs/plans/README.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/milestones/README.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/plans/README.md"));

    assert!(scanned_paths.contains(&"docs/plans/platform_plan.md"));
    assert!(scanned_paths.contains(&"docs/milestones/010-platform.md"));
    assert!(scanned_paths.contains(&"docs/plans/archive/README.md"));
    assert!(scanned_paths.contains(&"docs/milestones/archive/README.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/plans/commerce_plan.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/milestones/010-commerce.md"));

    // Overview README files reach doc_search through the public cold-build and reader path,
    // together with contract/proposal navigation, without changing task authority.
    let db_path = root.0.join("code_map.sqlite");
    writer::build(&root.0, &db_path, None).expect("cold build failed");
    let conn =
        contextunity_forge_mcp::db::reader::open(&db_path, &root.0).expect("reader open failed");
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(10),
        0,
        None,
        None,
    )
    .unwrap();
    let search_doc_paths = |query: &str| {
        contextunity_forge_mcp::db::reader::search_docs_with_options(
            &conn,
            query,
            &contextunity_forge_mcp::db::reader::DocSearchOptions {
                doc_type: None,
                component: None,
                include_excerpt: false,
                page: &page,
            },
        )
        .unwrap()["sections"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(search_doc_paths("rootmilestoneoverviewtoken")
        .contains(&"docs/milestones/README.md".to_owned()));
    assert!(search_doc_paths("rootplanoverviewtoken").contains(&"docs/plans/README.md".to_owned()));
    assert!(search_doc_paths("commercemilestoneoverviewtoken")
        .contains(&"extensions/commerce/docs/milestones/README.md".to_owned()));
    assert!(search_doc_paths("commerceplanoverviewtoken")
        .contains(&"extensions/commerce/docs/plans/README.md".to_owned()));
    for (query, path) in [
        (
            "rootmilestonecontractisolationtoken",
            "docs/milestones/010-platform.md",
        ),
        (
            "rootplancontractisolationtoken",
            "docs/plans/platform_plan.md",
        ),
        (
            "commercemilestonecontractisolationtoken",
            "extensions/commerce/docs/milestones/010-commerce.md",
        ),
        (
            "commerceplancontractisolationtoken",
            "extensions/commerce/docs/plans/commerce_plan.md",
        ),
        (
            "archivemilestoneoverviewtoken",
            "docs/milestones/archive/README.md",
        ),
        ("archiveplanoverviewtoken", "docs/plans/archive/README.md"),
    ] {
        assert!(
            search_doc_paths(query).contains(&path.to_owned()),
            "{query}/{path}"
        );
    }

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
    commit_handoff_fixture_inputs(&root, &["forge-mcp.yaml", "src", "docs", "extensions"]);

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
    assert!(task_id.starts_with("extensions.commerce/"));

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
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);

    let commit = git_head(&root.0);
    let handoff_res = handoff_with_fixture_evidence(
        &root.0,
        "m-commerce",
        Some(&commit),
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
    .unwrap_err();
    assert!(format!("{by_archive_path:#}").contains("STORE_SYNC_FAILED"));

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
fn milestone_lifecycle_sync_validates_status_and_preserves_cancellation_and_subtask_state() {
    let root = ScopedWorkspace::new("forge_milestone_lifecycle_sync");
    root.write("src/lib.rs", "pub fn lifecycle() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let database_path = tasks::database_path(&root.0).unwrap();
    std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
    let initial_store = TasksStore::open(&database_path).unwrap();
    drop(initial_store);

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
    let crlf_id = "forge-mcp/m-crlf-activation:activate";
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
            stage: "contract".into(),
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
    assert!(format!("{:#}", sync(archived_default_path).unwrap_err()).contains("STORE_SYNC_FAILED"));
    let archived_default_view =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-archived-default", true)
            .unwrap();
    assert_eq!(archived_default_view["status"], "completed");

    for (id, status, path) in [
        ("m-planned", "planned", "docs/milestones/020-planned.md"),
        (
            "m-completed",
            "completed",
            "docs/milestones/030-completed.md",
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
    let cancelled_id = "forge-mcp/m-cancelled:retired";
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

    let remaining_id = "forge-mcp/m-default:remaining";
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
             VALUES(?1, ?2, NULL, 'reviewer', 'decisions', 'task context', 1)",
            rusqlite::params![cancelled_scope_ref, cancelled_id],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref, task_id, subtask_ref, author, topic, payload, created_at) \
             VALUES(?1, NULL, NULL, 'reviewer', 'decisions', 'milestone context', 2)",
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
fn repacked_task_commits_sync_and_handoff_cleanly() {
    let root = ScopedWorkspace::new("forge_repack_handoff");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\n",
    );
    root.write("docs/AGENTS.md", "# Agent Guidance\n");
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let milestone_content = r#"---
id: m-repack
title: Repack test
doc_type: contract
---
# Repack test

```yaml
task_ref: first
target: Complete first task
proof_policy: seam-test-first
scope:
  - src/lib.rs
```
"#;
    root.write("docs/milestones/010-repack.md", milestone_content);

    // Initial commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            ".",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Initial milestone contract",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let _initial_commit = git_head(&root.0);

    // 1. Sync milestone
    let sync_res = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(sync_res["tasks"].as_array().unwrap().len(), 1);

    // 2. Complete the task through engine
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    let task = store.list(None, "all", None).unwrap().pop().unwrap();

    // Create an intermediate commit on the task branch before delivery modifying only src/lib.rs
    root.write("src/lib.rs", "pub fn work() -> bool { false }\n// step 1\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-am",
            "Intermediate commit before delivery",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let c1_sha = git_head(&root.0);

    // Deliver task with final implementation in scope
    root.write(
        "src/lib.rs",
        "pub fn work() -> bool { true }\n// completed\n",
    );
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);

    // 3. Verify milestone markdown document was updated on disk with the receipt
    let milestone_doc =
        std::fs::read_to_string(root.0.join("docs/milestones/010-repack.md")).unwrap();
    assert!(
        milestone_doc.contains("receipt:"),
        "milestone document must have receipt written"
    );
    assert!(
        milestone_doc.contains("status: completed"),
        "milestone task must be completed"
    );

    // 4. Repack task commit via amend (reword commit message while preserving delivered scope tree)
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--amend",
            "-m",
            "Repacked clean task commit",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    let repacked_sha = git_head(&root.0);

    // Update milestone markdown with the repacked SHA in receipt
    let parsed_before = Milestone::parse(&milestone_doc, "forge-mcp").unwrap();
    let mut updated_receipt = parsed_before.tasks[0].receipt.clone().unwrap();
    updated_receipt.commit = Some(repacked_sha.clone());
    let updated_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &milestone_doc,
        "first",
        &updated_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &updated_doc);

    // 5. Test tamper rejection: if receipt decision is modified, sync must reject
    let mut tampered_receipt = updated_receipt.clone();
    tampered_receipt.decision = "fail".into();
    let tampered_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &updated_doc,
        "first",
        &tampered_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &tampered_doc);

    let tamper_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        tamper_sync.is_err(),
        "tampered receipt must be rejected by task sync"
    );

    // 5b. Tamper passed_at: sync must reject
    let mut tampered_time = updated_receipt.clone();
    tampered_time.passed_at = "2099-01-01T00:00:00Z".into();
    let tampered_time_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &updated_doc,
        "first",
        &tampered_time,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &tampered_time_doc);
    let tamper_time_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        tamper_time_sync.is_err(),
        "tampered passed_at must be rejected by task sync"
    );

    // 5c. Foreign commit touching files outside task scope: sync must reject
    root.write("tests/unrelated.rs", "fn foreign() {}\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "tests/unrelated.rs",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Foreign commit outside scope",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let foreign_sha = git_head(&root.0);
    let mut foreign_receipt = updated_receipt.clone();
    foreign_receipt.commit = Some(foreign_sha.clone());
    let foreign_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &updated_doc,
        "first",
        &foreign_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &foreign_doc);
    let foreign_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        foreign_sync.is_err(),
        "commit outside task scope must be rejected by task sync"
    );

    // 5d. Commit modifying protected profile without scope: sync must reject
    root.write(".forge/acdd/profile.yaml", "gates: []\n");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            ".forge/acdd/profile.yaml",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Modify protected profile without scope",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let protected_sha = git_head(&root.0);
    let mut protected_receipt = updated_receipt.clone();
    protected_receipt.commit = Some(protected_sha.clone());
    let protected_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &updated_doc,
        "first",
        &protected_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &protected_doc);
    let protected_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        protected_sync.is_err(),
        "commit modifying protected profile must be rejected by task sync"
    );

    // 5e. Obsolete ancestor commit: sync must reject
    let mut ancestor_receipt = updated_receipt.clone();
    ancestor_receipt.commit = Some(c1_sha.clone());
    let ancestor_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &updated_doc,
        "first",
        &ancestor_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &ancestor_doc);
    let ancestor_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        ancestor_sync.is_err(),
        "obsolete ancestor commit must be rejected by task sync"
    );

    // 5f. Divergent commit from the same baseline with different content in scope: sync must reject
    let divergent_sha = {
        let _ = std::process::Command::new("git")
            .args(["checkout", "-q", &c1_sha])
            .current_dir(&root.0)
            .output();
        root.write(
            "src/lib.rs",
            "pub fn work() -> bool { false }\n// different code\n",
        );
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=ACDD test",
                "-c",
                "user.email=acdd-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-am",
                "Divergent commit from same baseline with different scope code",
            ])
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(output.status.success());
        let d_sha = git_head(&root.0);
        let _ = std::process::Command::new("git")
            .args(["checkout", "-q", &protected_sha])
            .current_dir(&root.0)
            .output();
        root.write(
            "src/lib.rs",
            "pub fn work() -> bool { true }\n// completed\n",
        );
        d_sha
    };
    let mut divergent_receipt = updated_receipt.clone();
    divergent_receipt.commit = Some(divergent_sha.clone());
    let divergent_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &updated_doc,
        "first",
        &divergent_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-repack.md", &divergent_doc);
    let divergent_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        divergent_sync.is_err(),
        "divergent commit with different code in scope must be rejected by task sync"
    );

    // 5g. Corrupted evidence in task_gates: sync and migrate verify must fail closed
    let valid_gate_evidence = {
        let store_corrupt = TasksStore::open_project(
            &root.0.join(".forge/tasks.sqlite"),
            "forge-mcp",
            "forge-mcp",
        )
        .unwrap();
        let valid: Vec<(String, u64, String)> = {
            let mut statement = store_corrupt
                .connection
                .prepare("SELECT gate,revision,evidence FROM task_gates WHERE task_id=?1 AND state='passed' ORDER BY revision,gate")
                .unwrap();
            statement
                .query_map([&task.task_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        store_corrupt
            .connection
            .execute(
                "UPDATE task_gates SET evidence='{corrupted-non-json: candidate_baseline_head}' WHERE task_id=?1 AND state='passed' AND evidence LIKE '%candidate_baseline_head%'",
                [&task.task_id],
            )
            .unwrap();
        valid
    };
    root.write("docs/milestones/010-repack.md", &updated_doc);
    let corrupt_sync = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    );
    assert!(
        corrupt_sync.is_err(),
        "sync must fail closed when task_gates evidence is corrupted"
    );
    assert!(
        corrupt_sync
            .unwrap_err()
            .to_string()
            .contains("TASK_GATE_EVIDENCE_CORRUPTED"),
        "expected TASK_GATE_EVIDENCE_CORRUPTED error"
    );
    let corrupt_verify = contextunity_forge_mcp::cli::migrate::run(
        &root.0,
        contextunity_forge_mcp::cli::migrate::MigrateCommand::Verify {
            milestone_ref: Some("docs/milestones/010-repack.md".into()),
        },
    );
    assert!(
        corrupt_verify.is_err(),
        "migrate verify must fail closed when task_gates evidence is corrupted"
    );

    // Restore valid task_gates evidence
    {
        let store_restore = TasksStore::open_project(
            &root.0.join(".forge/tasks.sqlite"),
            "forge-mcp",
            "forge-mcp",
        )
        .unwrap();
        for (gate, revision, evidence) in &valid_gate_evidence {
            store_restore
                .connection
                .execute(
                    "UPDATE task_gates SET evidence=?1 WHERE task_id=?2 AND gate=?3 AND revision=?4",
                    rusqlite::params![evidence, task.task_id, gate, revision],
                )
                .unwrap();
        }
    }

    // Restore the valid repacked doc
    root.write("docs/milestones/010-repack.md", &updated_doc);

    // 6. Test that task sync succeeds with the repacked commit
    let sync_repack = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-repack.md"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(sync_repack["tasks"].as_array().unwrap().len(), 1);

    // Verify SQLite task receipt has repacked SHA
    let store2 = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    let inspected = store2.inspect(&task.task_id).unwrap();
    assert_eq!(
        inspected.receipt.unwrap().commit.as_deref(),
        Some(repacked_sha.as_str())
    );
    drop(store2);

    // 7. Verify migrate check passes without conflicts
    let verify_res = contextunity_forge_mcp::cli::migrate::run(
        &root.0,
        contextunity_forge_mcp::cli::migrate::MigrateCommand::Verify {
            milestone_ref: Some("docs/milestones/010-repack.md".into()),
        },
    )
    .unwrap();
    assert_eq!(verify_res["conflicts"].as_array().unwrap().len(), 0);

    // Commit the milestone doc change before handoff
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "docs/milestones/010-repack.md",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Record repacked SHA in milestone",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let doc_only_sha = git_head(&root.0);

    // Merge divergent_sha into milestone branch so it becomes an ancestor of handoff_commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "merge",
            "-s",
            "ours",
            "-m",
            "Merge divergent branch for ancestry check",
            &divergent_sha,
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    // Migration still validates explicit pointer updates; handoff treats them as navigation.
    let mut doc_only_receipt = updated_receipt.clone();
    doc_only_receipt.commit = Some(doc_only_sha);
    let doc_only_doc =
        milestone_engine::render_task_receipt(&updated_doc, "first", &doc_only_receipt, &[])
            .unwrap();
    root.write("docs/milestones/010-repack.md", &doc_only_doc);

    // 7e. Test migrate verify rejects when markdown has foreign commit or commit not touching scope
    let verify_foreign = contextunity_forge_mcp::cli::migrate::run(
        &root.0,
        contextunity_forge_mcp::cli::migrate::MigrateCommand::Verify {
            milestone_ref: Some("docs/milestones/010-repack.md".into()),
        },
    );
    assert!(
        verify_foreign.is_err(),
        "migrate verify must reject markdown receipt specifying commit that does not touch scope"
    );

    // Restore valid markdown doc with repacked SHA before handoff
    root.write("docs/milestones/010-repack.md", &updated_doc);

    // Verify migrate verify passes for the valid repacked SHA
    let verify_valid = contextunity_forge_mcp::cli::migrate::run(
        &root.0,
        contextunity_forge_mcp::cli::migrate::MigrateCommand::Verify {
            milestone_ref: Some("docs/milestones/010-repack.md".into()),
        },
    );
    assert!(
        verify_valid.is_ok(),
        "migrate verify must succeed for valid repacked SHA"
    );

    // Remove the deliberately foreign migration fixtures from the final integration.
    for path in ["tests/unrelated.rs", ".forge/acdd/profile.yaml"] {
        std::fs::remove_file(root.0.join(path)).unwrap();
    }
    commit_handoff_fixture_inputs(&root, &["tests/unrelated.rs", ".forge/acdd/profile.yaml"]);
    let handoff_commit = git_head(&root.0);

    // 8. Run milestone handoff
    let handoff_res = handoff_with_fixture_evidence(
        &root.0,
        "m-repack",
        Some(&handoff_commit),
        "cargo test",
        1,
        0,
    )
    .unwrap();
    assert_eq!(handoff_res["id"], "m-repack");

    let archived_path = root.0.join("docs/milestones/archive/010-repack.md");
    assert!(archived_path.exists());
    let archived_doc = std::fs::read_to_string(&archived_path).unwrap();
    let archived_milestone = Milestone::parse(&archived_doc, "forge-mcp").unwrap();
    assert!(
        archived_milestone.tasks[0]
            .receipt
            .as_ref()
            .unwrap()
            .matches_proofs(&updated_receipt),
        "handoff preserves the accepted proofs independently of optional navigation SHAs"
    );
}

#[test]
fn handoff_resolves_repacked_commit_directly_from_markdown_without_prior_sync() {
    let root = ScopedWorkspace::new("forge_repack_direct_handoff");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\n",
    );
    root.write("docs/AGENTS.md", "# Agent Guidance\n");
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let milestone_content = r#"---
id: m-direct-repack
title: Direct repack test
doc_type: contract
---
# Direct repack test

```yaml
task_ref: direct
target: Complete direct task
proof_policy: seam-test-first
scope:
  - src/lib.rs
```
"#;
    root.write("docs/milestones/010-direct.md", milestone_content);

    // Initial commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            ".",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Initial milestone contract",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    // 1. Sync milestone
    contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-direct.md"
        }))
        .unwrap(),
    )
    .unwrap();

    // 2. Complete the task through engine
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    root.write(
        "src/lib.rs",
        "pub fn work() -> bool { true }\n// direct delivered\n",
    );
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);

    let milestone_doc =
        std::fs::read_to_string(root.0.join("docs/milestones/010-direct.md")).unwrap();

    // 3. Repack commit via amend (squash / reword commit message without modifying verified scope tree)
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--amend",
            "-m",
            "Repacked direct task commit",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    let repacked_sha = git_head(&root.0);

    // Update milestone markdown with the repacked SHA in receipt
    let parsed_before = Milestone::parse(&milestone_doc, "forge-mcp").unwrap();
    let mut updated_receipt = parsed_before.tasks[0].receipt.clone().unwrap();
    updated_receipt.commit = Some(repacked_sha.clone());
    let updated_doc = contextunity_forge_mcp::engine::milestones::render_task_receipt(
        &milestone_doc,
        "direct",
        &updated_receipt,
        &[],
    )
    .unwrap();
    root.write("docs/milestones/010-direct.md", &updated_doc);

    // Notice: We deliberately DO NOT run task sync here!
    // SQLite still has the old delivery commit.

    // Commit the milestone doc change
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "docs/milestones/010-direct.md",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Record repacked SHA in milestone doc",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    let handoff_commit = git_head(&root.0);

    // 4. Run milestone handoff: it should pick the commit from the milestone doc!
    let handoff_res = handoff_with_fixture_evidence(
        &root.0,
        "m-direct-repack",
        Some(&handoff_commit),
        "cargo test",
        1,
        0,
    )
    .unwrap();
    assert_eq!(handoff_res["id"], "m-direct-repack");

    let archived_path = root.0.join("docs/milestones/archive/010-direct.md");
    assert!(archived_path.exists());
    let archived_doc = std::fs::read_to_string(&archived_path).unwrap();
    let archived_milestone = Milestone::parse(&archived_doc, "forge-mcp").unwrap();
    assert_eq!(
        archived_milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|r| r.commit.as_deref()),
        Some(repacked_sha.as_str()),
        "archived receipt must preserve the repacked task commit from markdown"
    );

    // The archive is durable; active SQLite history is pruned.
    let store2 = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    assert!(store2.list(None, "all", None).unwrap().is_empty());
}

#[test]
fn record_task_commits_false_archives_commitless_markdown_receipt_and_syncs_cleanly() {
    let root = ScopedWorkspace::new("forge_commitless_handoff");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\nrecord_task_commits: false\n",
    );
    root.write("docs/AGENTS.md", "# Agent Guidance\n");
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let milestone_content = r#"---
id: m-commitless
title: Commitless test
doc_type: contract
---
# Commitless test

```yaml
task_ref: work
target: Complete work task
proof_policy: seam-test-first
scope:
  - src/lib.rs
```
"#;
    root.write("docs/milestones/010-commitless.md", milestone_content);

    // Initial commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            ".",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Initial milestone contract",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    // 1. Sync milestone
    contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-commitless.md"
        }))
        .unwrap(),
    )
    .unwrap();

    // 2. Complete task through engine with implementation in scope
    root.write(
        "src/lib.rs",
        "pub fn work() -> bool { true }\n// implementation\n",
    );
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);

    let handoff_commit = git_head(&root.0);

    // 3. Run milestone handoff
    let handoff_res = handoff_with_fixture_evidence(
        &root.0,
        "m-commitless",
        Some(&handoff_commit),
        "cargo test",
        1,
        0,
    )
    .unwrap();
    assert_eq!(handoff_res["id"], "m-commitless");

    // 4. Verify the archived document has a commitless receipt (no commit SHA in markdown)
    let archived_path = root.0.join("docs/milestones/archive/010-commitless.md");
    assert!(archived_path.exists());
    let archived_doc = std::fs::read_to_string(&archived_path).unwrap();
    let archived_milestone = Milestone::parse(&archived_doc, "forge-mcp").unwrap();
    assert!(
        archived_milestone.tasks[0]
            .receipt
            .as_ref()
            .unwrap()
            .commit
            .is_none(),
        "archived receipt must be commitless in Markdown when record_task_commits is false"
    );

    // 5. The commitless archive remains durable after SQLite pruning.
    let store2 = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    assert!(store2.list(None, "all", None).unwrap().is_empty());

    // 6. Explicit archive sync fails closed even for a valid commitless receipt.
    let sync_res = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/archive/010-commitless.md"
        }))
        .unwrap(),
    );
    assert!(format!("{:#}", sync_res.unwrap_err()).contains("STORE_SYNC_FAILED"));

    // 7. Rejected archive sync does not resurrect SQLite history.
    let store3 = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    assert!(store3.list(None, "all", None).unwrap().is_empty());
    drop(store3);

    // 8. Verify migrate verify succeeds with 0 conflicts on the commitless archive
    let migrate_res = contextunity_forge_mcp::cli::migrate::run(
        &root.0,
        contextunity_forge_mcp::cli::migrate::MigrateCommand::Verify {
            milestone_ref: Some("docs/milestones/archive/010-commitless.md".into()),
        },
    )
    .unwrap();
    assert_eq!(
        migrate_res["conflicts"].as_array().unwrap().len(),
        0,
        "migrate verify must pass for commitless archive"
    );
}

#[test]
fn handoff_accepts_content_equivalent_history_rewrite() {
    let root = ScopedWorkspace::new("forge_squashed_handoff");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\n",
    );
    root.write("docs/AGENTS.md", "# Agent Guidance\n");
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let milestone_content = r#"---
id: m-squash
title: Squash test
doc_type: contract
---
# Squash test

```yaml
task_ref: work
target: Complete work task
proof_policy: seam-test-first
scope:
  - src/lib.rs
  - docs/notes.md
```
"#;
    root.write("docs/milestones/010-squash.md", milestone_content);

    // Initial commit
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            ".",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Initial milestone contract",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    // 1. Sync milestone
    contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-squash.md"
        }))
        .unwrap(),
    )
    .unwrap();

    // 2. Complete task through engine
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);

    // Rewriting the full history preserves content, not the original baseline.
    let delivery_sha = git_head(&root.0);
    std::process::Command::new("git")
        .args(["checkout", "--orphan", "rewritten-history"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Squashed release commit containing task implementation",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();

    let squashed_sha = git_head(&root.0);
    assert_ne!(delivery_sha, squashed_sha);

    // 4. Create an additional commit on top (e.g. docs commit)
    root.write("docs/notes.md", "release documentation\n");
    std::process::Command::new("git")
        .args(["add", "docs/notes.md"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Documentation commit on top of squash",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let tip_sha = git_head(&root.0);
    assert_ne!(squashed_sha, tip_sha);

    // 5. Handoff at tip succeeds and correctly resolves squashed_sha (introducing commit), NOT tip_sha!
    let handoff_res =
        handoff_with_fixture_evidence(&root.0, "m-squash", Some(&tip_sha), "cargo test", 1, 0)
            .unwrap();
    assert_eq!(handoff_res["id"], "m-squash");

    let store2 = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    assert!(store2.list(None, "all", None).unwrap().is_empty());
    let archive =
        std::fs::read_to_string(root.0.join("docs/milestones/archive/010-squash.md")).unwrap();
    let archived = Milestone::parse(&archive, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0].receipt.as_ref().and_then(|r| r.commit.as_deref()),
        Some(squashed_sha.as_str()),
        "handoff must record the squashed commit that introduced the scoped changes, not the tip commit on top"
    );
}

#[test]
fn task_settings_parses_various_commit_tracking_formats() {
    let yaml_bool: contextunity_forge_mcp::engine::tasks::TaskSettings =
        serde_yaml::from_str("commit_tracking: off\n").unwrap();
    assert!(!yaml_bool.records_task_commits());

    let yaml_str: contextunity_forge_mcp::engine::tasks::TaskSettings =
        serde_yaml::from_str("commit_tracking: \"milestone_only\"\n").unwrap();
    assert!(!yaml_str.records_task_commits());

    let yaml_record_false: contextunity_forge_mcp::engine::tasks::TaskSettings =
        serde_yaml::from_str("record_task_commits: false\n").unwrap();
    assert!(!yaml_record_false.records_task_commits());

    let yaml_default: contextunity_forge_mcp::engine::tasks::TaskSettings =
        serde_yaml::from_str("").unwrap();
    assert!(yaml_default.records_task_commits());
}

#[test]
fn bulk_sync_rejects_invalid_numbered_manifests_in_custom_directories() {
    for directory in ["contracts", "contracts/archive", "contracts/nested"] {
        for header in [
            "no frontmatter\n",
            "---\nid: [broken\n---\n",
            "---\ntitle: Missing identity\n---\n",
            "---\nid: 17\n---\n",
        ] {
            let root = ScopedWorkspace::new("forge_custom_manifest_validation");
            root.write(
                "forge-mcp.yaml",
                "roots: [src]\nmilestones: [contracts]\ntask_repository: fixture\ntask_project: fixture\n",
            );
            root.write("src/lib.rs", "pub fn fixture() {}\n");
            root.write("contracts/README.md", "# Contracts\n");
            let path = format!("{directory}/001-fixture.md");
            let body = "\n### task: one\n```yaml\ntask_ref: one\ntarget: Fixture\nproof_policy: direct-proof\nscope: [src/lib.rs]\n```\n";
            root.write(
                &path,
                &format!("---\nid: m-fixture\nstatus: active\n---\n{body}"),
            );
            let invoke = |reference: Option<&str>| {
                let mut command =
                    std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
                command.args([
                    "--format",
                    "json",
                    "--root",
                    root.0.to_str().unwrap(),
                    "task",
                    "sync",
                ]);
                if let Some(reference) = reference {
                    command.arg(reference);
                } else {
                    command.args(["--workspace", "fixture"]);
                }
                command.output().unwrap()
            };
            let valid = invoke(None);
            assert!(valid.status.success(), "{valid:?}");
            let valid: Value = serde_json::from_slice(&valid.stdout).unwrap();
            if directory == "contracts/archive" {
                assert_eq!(valid["total"], 0);
                root.write(&path, &format!("{header}{body}"));
                let skipped = invoke(None);
                assert!(skipped.status.success(), "{skipped:?}");
                let skipped: Value = serde_json::from_slice(&skipped.stdout).unwrap();
                assert_eq!(skipped["total"], 0);
                let explicit = invoke(Some(&path));
                assert!(!explicit.status.success());
                assert!(String::from_utf8_lossy(&explicit.stderr).contains(&path));
                continue;
            }
            assert_eq!(valid["total"], 1);
            let workspaces = list_workspaces(&root.0).unwrap();
            assert_eq!(workspaces, vec!["fixture"]);
            let database = root.0.join(".forge/tasks.sqlite");
            let before = TasksStore::open_project(&database, "fixture", "fixture")
                .unwrap()
                .inspect_details("fixture/m-fixture:one")
                .unwrap();

            root.write(&path, &format!("{header}{body}"));
            let request = tasks::Manage {
                action: tasks::ManageAction::Sync,
                workspace: Some("fixture".into()),
                ..Default::default()
            };
            let bulk_result = contextunity_forge_mcp::engine::tasks::manage(&root.0, request);
            assert!(
                bulk_result.is_err(),
                "bulk engine sync accepted {directory}: {header:?}"
            );
            for reference in [None, Some(path.as_str())] {
                let output = invoke(reference);
                assert!(
                    !output.status.success(),
                    "sync {reference:?} accepted {header:?}: {output:?}"
                );
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains(&path),
                    "diagnostic omits {path}: {output:?}"
                );
            }
            let after = TasksStore::open_project(&database, "fixture", "fixture")
                .unwrap()
                .inspect_details("fixture/m-fixture:one")
                .unwrap();
            assert_eq!(before, after, "invalid input changed persisted task state");
        }
    }
}

#[test]
fn subproject_milestones_prefers_declared_projects_preventing_duplicate_phantom_workspaces() {
    let root = ScopedWorkspace::new("forge_subproject_no_phantom_workspaces");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\nmilestones:\n  - docs/milestones\n  - extensions/*/docs/milestones\nowners:\n  \"extensions\": ext\n  \"extensions/base\": base\naliases:\n  base: base\n",
    );
    root.write(
        "docs/milestones/001-root.md",
        "---\nid: M-ROOT\ntitle: Root Milestone\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: root-task\ntarget: Root target\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    root.write(
        "extensions/base/docs/milestones/BS-102-document-loaders.md",
        "---\nid: BS-102\ntitle: Base Milestone\nproject: base\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: base-task\ntarget: Base target\nproof_policy: direct-proof\nscope: [extensions/base/]\n```\n",
    );

    // Verify registry workspaces directly: phantom "extensions.base" must NOT exist
    let workspace_names = contextunity_forge_mcp::engine::tasks::list_workspaces(&root.0).unwrap();
    assert!(
        workspace_names.contains(&"test".to_string()),
        "expected primary workspace 'test', got: {workspace_names:?}"
    );
    assert!(
        workspace_names.contains(&"base".to_string()),
        "expected subproject workspace 'base', got: {workspace_names:?}"
    );
    assert!(
        !workspace_names.contains(&"extensions.base".to_string()),
        "phantom workspace 'extensions.base' must NOT be registered: {workspace_names:?}"
    );
    assert!(
        !workspace_names.contains(&"ext".to_string()),
        "shorter owner prefix 'ext' must NOT shadow longer prefix 'base': {workspace_names:?}"
    );

    // Both milestones must resolve cleanly without cross-workspace ambiguity
    let sync_root = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Manage {
            action: contextunity_forge_mcp::engine::tasks::ManageAction::Sync,
            milestone_ref: Some("M-ROOT".into()),
            workspace: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_root["tasks"].as_array().unwrap().len(), 1);

    let sync_subproject = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Manage {
            action: contextunity_forge_mcp::engine::tasks::ManageAction::Sync,
            milestone_ref: Some("BS-102".into()),
            workspace: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_subproject["tasks"].as_array().unwrap().len(), 1);

    let list_base = contextunity_forge_mcp::engine::tasks::list(
        &root.0,
        contextunity_forge_mcp::engine::tasks::List {
            repository: Some("base".into()),
            milestone_ref: Some("BS-102".into()),
            milestone_status: None,
            status: Some(contextunity_forge_mcp::engine::tasks::Status::All),
            stage: None,
            detail: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(list_base["tasks"].as_array().unwrap().len(), 1);
}

#[test]
fn subproject_milestones_handles_readme_owners_and_non_trivial_aliases() {
    let root = ScopedWorkspace::new("forge_subproject_readme_owners_aliases");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\nmilestones:\n  - docs/milestones\n  - extensions/*/docs/milestones\nowners:\n  \"extensions/base\": base\naliases:\n  legacy: base\n",
    );
    root.write(
        "docs/milestones/001-root.md",
        "---\nid: M-ROOT\ntitle: Root Milestone\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: root-task\ntarget: Root target\nproof_policy: direct-proof\nscope: [src/]\n```\n",
    );
    // 1. README.md alongside milestones in extensions/base/docs/milestones: must NOT register phantom workspace
    root.write(
        "extensions/base/docs/milestones/README.md",
        "# Base Milestones\nThis document describes milestones for the base extension.\n",
    );
    root.write(
        "extensions/readme-only/docs/milestones/README.md",
        "# Future milestones\n",
    );
    std::fs::create_dir_all(root.0.join("extensions/empty/docs/milestones")).unwrap();
    // 2. BS-103: milestone WITHOUT explicit project in frontmatter (relies on owners: {extensions/base: base})
    root.write(
        "extensions/base/docs/milestones/BS-103-owners-milestone.md",
        "---\nid: BS-103\ntitle: Base Milestone via Owners\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: owners-task\ntarget: Owners target\nproof_policy: seam-test-first\nscope: [extensions/base/src/]\n```\n",
    );
    // 3. BS-104: milestone with project: legacy (relies on aliases: {legacy: base})
    root.write(
        "extensions/base/docs/milestones/BS-104-aliased-milestone.md",
        "---\nid: BS-104\ntitle: Base Milestone via Aliases\nproject: legacy\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: alias-task\ntarget: Alias target\nproof_policy: seam-test-first\nscope: [extensions/base/src/]\n```\n",
    );
    root.write(
        "extensions/base/docs/milestones/BS-106-other-project.md",
        "---\nid: BS-106\ntitle: Other project in the same directory\nproject: other\ndoc_type: contract\n---\n```yaml\ntask_ref: other-task\ntarget: Deliver the declared project\nproof_policy: seam-test-first\nscope: [extensions/base/src/]\n```\n",
    );

    // Verify registry workspaces directly: phantom "extensions.base" or "legacy" must NOT exist
    let workspace_names = contextunity_forge_mcp::engine::tasks::list_workspaces(&root.0).unwrap();
    assert!(
        workspace_names.contains(&"test".to_string()),
        "expected primary workspace 'test', got: {workspace_names:?}"
    );
    assert!(
        workspace_names.contains(&"base".to_string()),
        "expected subproject workspace 'base', got: {workspace_names:?}"
    );
    assert!(workspace_names.contains(&"other".to_string()));
    assert!(
        !workspace_names.contains(&"extensions.base".to_string()),
        "phantom workspace 'extensions.base' must NOT be registered: {workspace_names:?}"
    );
    assert!(
        !workspace_names.contains(&"legacy".to_string()),
        "aliased project 'legacy' must NOT create a separate workspace: {workspace_names:?}"
    );
    assert!(
        !workspace_names.contains(&"extensions.readme-only".to_string())
            && !workspace_names.contains(&"extensions.empty".to_string()),
        "directories without milestones must not create workspaces: {workspace_names:?}"
    );

    // Sync BS-103 (owners without explicit project frontmatter) must succeed without TASK_PROJECT_MISMATCH
    let sync_owners = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Manage {
            action: contextunity_forge_mcp::engine::tasks::ManageAction::Sync,
            milestone_ref: Some("BS-103".into()),
            workspace: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_owners["tasks"].as_array().unwrap().len(), 1);

    // Sync BS-104 (project: legacy aliased to base) must succeed without TASK_PROJECT_MISMATCH
    let sync_alias = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Manage {
            action: contextunity_forge_mcp::engine::tasks::ManageAction::Sync,
            milestone_ref: Some("BS-104".into()),
            workspace: Some("all".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_alias["tasks"].as_array().unwrap().len(), 1);
    let sync_other = tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"BS-106","workspace":"all"}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sync_other["tasks"][0]["task_id"], "other/BS-106:other-task");

    root.write("extensions/base/src/lib.rs", "pub fn extension() {}\n");
    commit_handoff_fixture_inputs(
        &root,
        &["AGENTS.md", "forge-mcp.yaml", "docs", "extensions"],
    );
    let cases = [
        (
            "BS-103",
            "owners-task",
            "BS-103-owners-milestone.md",
            "base",
        ),
        (
            "BS-104",
            "alias-task",
            "BS-104-aliased-milestone.md",
            "base",
        ),
        ("BS-106", "other-task", "BS-106-other-project.md", "other"),
    ];
    for (milestone_id, task_ref, file, project) in cases {
        let listed = tasks::list(
            &root.0,
            tasks::List {
                repository: Some(project.into()),
                milestone_ref: Some(milestone_id.into()),
                status: Some(tasks::Status::All),
                ..Default::default()
            },
        )
        .unwrap();
        let task_id = format!("{project}/{milestone_id}:{task_ref}");
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed["tasks"][0]["task_id"], task_id);
        assert_eq!(listed["tasks"][0]["status"], "ready");
        let store =
            TasksStore::open_project(&root.0.join(".forge/tasks.sqlite"), "test", project).unwrap();
        complete_task_through_engine(&root, &store, &task_id);
        let delivered = store.inspect(&task_id).unwrap();
        assert_eq!(delivered.status, "completed");
        assert_eq!(delivered.receipt.unwrap().decision, "pass");
        let text = std::fs::read_to_string(
            root.0
                .join(format!("extensions/base/docs/milestones/{file}")),
        )
        .unwrap();
        let parsed = Milestone::parse(&text, "test").unwrap();
        assert_eq!(parsed.tasks[0].status.as_deref(), Some("completed"));
        assert!(parsed.tasks[0].receipt.is_some());
        let shown = milestone_engine::show(&root.0, milestone_id, false).unwrap();
        assert_eq!(shown["completion"], "1/1");
        let evidence_file =
            fixture_evidence_file(&root.0, milestone_id, "cargo test --test acdd", 1, 0);
        let handoff_args = ["handoff", milestone_id, "--evidence-file", &evidence_file];
        let output = milestone_cli(&root, &handoff_args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(root
            .0
            .join(format!("extensions/base/docs/milestones/archive/{file}"))
            .is_file());
        let archive_path = format!("extensions/base/docs/milestones/archive/{file}");
        let accepted_archive = std::fs::read_to_string(root.0.join(&archive_path)).unwrap();
        let header: serde_yaml::Value = serde_yaml::from_str(
            accepted_archive
                .strip_prefix("---\n")
                .unwrap()
                .split_once("\n---\n")
                .unwrap()
                .0,
        )
        .unwrap();
        assert!(header["handoff"]["passport"]["terminal_tasks"]
            .get(&task_id)
            .is_some());
        root.write(&format!("extensions/base/docs/milestones/{file}"), &text);
        root.write(&archive_path, "---\nid: [torn\n");
        for _ in 0..2 {
            let retry = milestone_cli(&root, &handoff_args);
            assert!(
                retry.status.success(),
                "{milestone_id}: {}",
                String::from_utf8_lossy(&retry.stderr)
            );
        }
        let repaired = std::fs::read_to_string(root.0.join(&archive_path)).unwrap();
        assert_eq!(
            Milestone::parse(&repaired, "test").unwrap().tasks[0].receipt,
            Milestone::parse(&accepted_archive, "test").unwrap().tasks[0].receipt,
        );
        let shown = milestone_engine::show(&root.0, milestone_id, false).unwrap();
        assert_eq!(shown["status"], "completed");
        let archived = std::fs::read_to_string(
            root.0
                .join(format!("extensions/base/docs/milestones/archive/{file}")),
        )
        .unwrap();
        let archived = Milestone::parse(&archived, "base").unwrap();
        assert!(archived
            .tasks
            .iter()
            .all(|task| task.status.as_deref() == Some("completed")));
        commit_handoff_fixture_inputs(&root, &["extensions/base/docs/milestones"]);
    }
    let initialized = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "105",
            "--slug",
            "owned-init",
            "--title",
            "Owned init",
            "--dir",
            "extensions/base/docs/milestones",
        ],
    );
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let text = std::fs::read_to_string(
        root.0
            .join("extensions/base/docs/milestones/105-owned-init.md"),
    )
    .unwrap();
    assert_eq!(Milestone::parse(&text, "test").unwrap().project, "base");

    let left = ScopedWorkspace::new("forge_milestone_ambiguity_left");
    let right = ScopedWorkspace::new("forge_milestone_ambiguity_right");
    for (linked, project) in [(&left, "left"), (&right, "right")] {
        linked.write(
            "forge-mcp.yaml",
            &format!("task_repository: linked\ntask_project: {project}\n"),
        );
        linked.write(
            "docs/milestones/099-shared.md",
            "---\nid: m-shared\ntitle: Shared reference\ndoc_type: contract\n---\n```yaml\ntask_ref: shared-task\ntarget: Shared target\nproof_policy: direct-proof\nscope: [src/]\n```\n",
        );
    }
    let config = std::fs::read_to_string(root.0.join("forge-mcp.yaml")).unwrap();
    root.write(
        "forge-mcp.yaml",
        &format!(
            "{config}linked_workspaces:\n  - name: left\n    path: {}\n    tasks: {{enabled: true}}\n  - name: right\n    path: {}\n    tasks: {{enabled: true}}\n",
            left.0.display(),
            right.0.display()
        ),
    );
    for workspace in [None, Some("all")] {
        let error = tasks::list(
            &root.0,
            serde_json::from_value(json!({"milestone_ref":"099","repository":workspace})).unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("ambiguous milestone reference"), "{error}");
        assert!(error.contains("left") && error.contains("right"), "{error}");
    }
}

#[test]
fn default_project_alias_is_applied_once_across_task_lifecycle() {
    let root = ScopedWorkspace::new("forge_default_project_alias");
    root.write("AGENTS.md", "# Rules\n");
    root.write("src/lib.rs", "pub fn primary() {}\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: legacy\nroots: []\nmilestones: [docs/milestones, 'extensions/*/docs/milestones']\nowners:\n  extensions/new: old-owner\naliases:\n  legacy: base\n  old-owner: base\n  base: other\n",
    );
    root.write(
        "docs/milestones/001-default.md",
        "---\nid: m-default\ntitle: Default alias\ndoc_type: contract\nstatus: in_progress\n---\n```yaml\ntask_ref: first\ntarget: Deliver the default project\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );
    commit_handoff_fixture_inputs(&root, &["AGENTS.md", "forge-mcp.yaml", "docs", "src"]);
    let synced = tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"001"})).unwrap(),
    )
    .unwrap();
    let task_id = "base/m-default:first";
    assert_eq!(synced["tasks"][0]["task_id"], task_id);
    let listed = tasks::list(&root.0, tasks::List::default()).unwrap();
    assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(listed["tasks"][0]["task_id"], task_id);
    assert_eq!(tasks::list_workspaces(&root.0).unwrap(), vec!["test"]);
    let store = tasks::store(&root.0).unwrap();
    assert_eq!(store.inspect(task_id).unwrap().task_id, task_id);
    complete_task_through_engine(&root, &store, task_id);
    assert_eq!(store.inspect(task_id).unwrap().status, "completed");
    assert_eq!(
        milestone_engine::show(&root.0, "001", false).unwrap()["completion"],
        "1/1"
    );
    let evidence_file = fixture_evidence_file(&root.0, "001", "cargo test --test acdd", 1, 0);
    let handoff_args = ["handoff", "001", "--evidence-file", &evidence_file];
    let active_text =
        std::fs::read_to_string(root.0.join("docs/milestones/001-default.md")).unwrap();
    let handed_off = milestone_cli(&root, &handoff_args);
    assert!(
        handed_off.status.success(),
        "{}",
        String::from_utf8_lossy(&handed_off.stderr)
    );
    assert_eq!(
        contextunity_forge_mcp::db::tasks_store::read_milestone_archive(
            &root.0,
            &root.0.join("docs/milestones/archive/001-default.md"),
            "test",
            "base"
        )
        .unwrap()
        .unwrap()
        .project,
        "base"
    );
    root.write("docs/milestones/001-default.md", &active_text);
    root.write("docs/milestones/archive/001-default.md", "---\nid: [torn\n");
    for _ in 0..2 {
        let retry = milestone_cli(&root, &handoff_args);
        assert!(
            retry.status.success(),
            "{}",
            String::from_utf8_lossy(&retry.stderr)
        );
    }
    assert_eq!(
        contextunity_forge_mcp::db::tasks_store::read_milestone_archive(
            &root.0,
            &root.0.join("docs/milestones/archive/001-default.md"),
            "test",
            "base"
        )
        .unwrap()
        .unwrap()
        .project,
        "base"
    );
    std::fs::create_dir_all(root.0.join("extensions/new/docs/milestones")).unwrap();
    let initialized = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "002",
            "--slug",
            "aliased-init",
            "--title",
            "Aliased init",
            "--active",
            "--dir",
            "extensions/new/docs/milestones",
        ],
    );
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let synced = tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"002"})).unwrap(),
    )
    .unwrap();
    let initialized_id = "base/m-aliased-init:aliased-init";
    assert_eq!(synced["tasks"][0]["task_id"], initialized_id);
    let next = tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"next","worker_id":"contract-author","worktree":root.0}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(next["task_id"], initialized_id);

    let mut topology_failures = Vec::new();
    for (case, default, aliases, directory, declaration, qualifier, project) in [
        ("default", "base", "{}", "docs/milestones", "", "", "base"),
        (
            "single-hop",
            "old",
            "{old: base}",
            "docs/milestones",
            "",
            "",
            "base",
        ),
        (
            "default-chain",
            "old",
            "{old: base, base: other}",
            "docs/milestones",
            "",
            "",
            "base",
        ),
        (
            "configured-chain",
            "fallback",
            "{old: base, base: other}",
            "extensions/new/docs/milestones",
            "",
            "",
            "base",
        ),
        (
            "explicit-old",
            "old",
            "{old: base, base: other}",
            "docs/milestones",
            "",
            "old/",
            "base",
        ),
        (
            "explicit-base",
            "old",
            "{old: base, base: other}",
            "docs/milestones",
            "project: base\n",
            "base/",
            "other",
        ),
    ] {
        let topology = ScopedWorkspace::new("forge_passport_topology");
        topology.write(".gitignore", ".forge/\n");
        topology.write("src/lib.rs", "pub fn topology() {}\n");
        topology.write("forge-mcp.yaml", &format!("roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\ntask_repository: topology-repo\ntask_project: {default}\nmilestones: [docs/milestones, 'extensions/*/docs/milestones']\nowners:\n  extensions/new: old\naliases: {aliases}\n"));
        let manifest = |id: &str, state: &str, dependencies: &str, task_status: &str| {
            let handoff = if state == "completed" {
                "handoff:\n  completed_at: 2026-10-01T00:00:00Z\n  commit: 0123456789012345678901234567890123456789\n"
            } else {
                ""
            };
            format!("---\nid: {id}\ntitle: Passport topology\nrepository: topology-repo\n{declaration}status: {state}\nstarted_at: 2026-10-01T00:00:00Z\n{dependencies}{handoff}---\n```yaml\ntask_ref: work\ntarget: Preserve topology\nproof_policy: seam-test-first\nscope: [src/lib.rs]\nstatus: {task_status}\n```\n")
        };
        topology.write(
            &format!("{directory}/archive/001-up.md"),
            &manifest("m-up", "completed", "", "cancelled"),
        );
        let current = format!("{directory}/002-current.md");
        topology.write(
            &current,
            &manifest(
                "m-current",
                "active",
                &format!("depends_on: [{qualifier}m-up, {qualifier}m-missing]\n"),
                "cancelled",
            ),
        );
        topology.write(
            &format!("{directory}/003-down.md"),
            &manifest(
                "m-down",
                "active",
                &format!("depends_on: [{qualifier}m-current]\n"),
                "ready",
            ),
        );
        commit_handoff_fixture_inputs(
            &topology,
            &[".gitignore", "forge-mcp.yaml", "src", directory],
        );
        let synced = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--format",
                "json",
                "--root",
                topology.0.to_str().unwrap(),
                "task",
                "sync",
                "--file",
                &current,
            ])
            .output()
            .unwrap();
        assert!(
            synced.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&synced.stderr)
        );
        let context = milestone_cli(
            &topology,
            &["handoff-context", "m-current", "--target-base", "HEAD"],
        );
        assert!(
            context.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&context.stderr)
        );
        let context: serde_json::Value = serde_json::from_slice(&context.stdout).unwrap();
        assert_eq!(context["binding"]["project"], project, "{case}");
        let evidence = json!({"binding":context["binding"],"target_base":context["target_base"],"review":{"worker_id":"topology-reviewer","decision":"pass","summary":"Topology fixture content","scope_findings":[],"contract_findings":[]},"verification":{"command":"true","exit_code":0,"tests_passed":0,"tests_failed":0}});
        let evidence_path = topology.0.join(".forge/topology-evidence.json");
        std::fs::write(&evidence_path, serde_json::to_vec(&evidence).unwrap()).unwrap();
        let handed_off = milestone_cli(
            &topology,
            &[
                "handoff",
                "m-current",
                "--evidence-file",
                evidence_path.to_str().unwrap(),
            ],
        );
        assert!(
            handed_off.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&handed_off.stderr)
        );
        let handed_off: serde_json::Value = serde_json::from_slice(&handed_off.stdout).unwrap();
        let expected = json!({"upstream":[format!("topology-repo/{project}/m-missing"),format!("topology-repo/{project}/m-up")],"downstream":[format!("topology-repo/{project}/m-down")],"unresolved_upstream":[format!("topology-repo/{project}/m-missing")]});
        let archive = std::fs::read_to_string(
            topology
                .0
                .join(format!("{directory}/archive/002-current.md")),
        )
        .unwrap();
        let header: serde_yaml::Value = serde_yaml::from_str(
            archive
                .strip_prefix("---\n")
                .unwrap()
                .split_once("\n---\n")
                .unwrap()
                .0,
        )
        .unwrap();
        let note = std::process::Command::new("git")
            .args([
                "notes",
                "--ref=forge",
                "show",
                context["commit"].as_str().unwrap(),
            ])
            .current_dir(&topology.0)
            .output()
            .unwrap();
        assert!(note.status.success(), "{case}: {note:?}");
        let note: serde_json::Value = serde_json::from_slice(&note.stdout).unwrap();
        let identity = format!("topology-repo/{project}/m-current");
        for (location, observed) in [
            (
                "response",
                handed_off["handoff"]["passport"]["topology"].clone(),
            ),
            (
                "archive",
                serde_json::to_value(&header["handoff"]["passport"]["topology"]).unwrap(),
            ),
            (
                "note",
                note["milestone_passports"][&identity]["topology"].clone(),
            ),
        ] {
            if observed != expected {
                topology_failures.push(format!(
                    "{case}/{location}: {observed}, expected {expected}"
                ));
            }
        }
    }
    assert!(
        topology_failures.is_empty(),
        "passport topology matrix: {topology_failures:?}"
    );
}

#[test]
fn handoff_rejects_stale_or_untrusted_integration_evidence_atomically() {
    for scenario in [
        "bytes",
        "mode",
        "deletion",
        "addition",
        "documentation",
        "configuration",
        "authority",
        "target-base",
        "foreign",
        "authority-binding",
        "receipt-context",
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
        "dirty",
        "untracked",
    ] {
        if scenario == "mode" && !cfg!(unix) {
            continue;
        }
        let root = ScopedWorkspace::new("forge_final_evidence");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(output.status.success(), "{scenario}: {args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        root.write(".gitignore", ".forge/\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root.write("src/lib.rs", "pub fn work() -> u8 { 0 }\n");
        root.write("src/obsolete.rs", "pub fn retained() {}\n");
        root.write("docs/guide.md", "# Runtime guide\n");
        root.write("scripts/tool.sh", "#!/bin/sh\nexit 0\n");
        let active = "docs/milestones/010-final.md";
        let archived = "docs/milestones/archive/010-final.md";
        let source = "---\nid: m-final\ntitle: Final integration evidence\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Final content\n```yaml\ntask_ref: first\ntarget: First admitted outcome\nproof_policy: seam-test-first\nscope: [src/, docs/guide.md, scripts/tool.sh, forge-mcp.yaml]\n```\n```yaml\ntask_ref: second\ntarget: Combined admitted outcome\nproof_policy: seam-test-first\nscope: [src/, docs/guide.md, scripts/tool.sh, forge-mcp.yaml]\ndepends_on: [first]\n```\n";
        root.write(active, source);
        git(&[
            "add",
            ".gitignore",
            "forge-mcp.yaml",
            "src",
            "docs",
            "scripts",
        ]);
        git(&["commit", "-m", "admit final-evidence fixture"]);
        let base = git_head(&root.0);
        let branch = git(&["branch", "--show-current"]);
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, active, &root.0).unwrap();
        for (index, spec) in milestone.tasks.iter().enumerate() {
            root.write(
                "src/lib.rs",
                &format!("pub fn work() -> u8 {{ {} }}\n", index + 1),
            );
            complete_task_with_builder(
                &root,
                &store,
                &milestone.task_id(spec),
                if index == 0 {
                    "builder-first"
                } else {
                    "builder-second"
                },
            );
        }
        let mut evidence =
            fixture_integration_evidence(&root.0, "m-final", "cargo test --test acdd", 2, 0);
        match scenario {
            "bytes" | "dirty" => root.write("src/lib.rs", "pub fn work() -> u8 { 99 }\n"),
            "mode" => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        root.0.join("scripts/tool.sh"),
                        std::fs::Permissions::from_mode(0o755),
                    )
                    .unwrap();
                }
            }
            "deletion" => std::fs::remove_file(root.0.join("src/obsolete.rs")).unwrap(),
            "addition" => root.write("src/new.rs", "pub fn unreviewed() {}\n"),
            "documentation" => root.write("docs/guide.md", "# Changed runtime contract\n"),
            "configuration" => root.write(
                "forge-mcp.yaml",
                "roots: [src, docs]\ntasks_db: .forge/tasks.sqlite\n",
            ),
            "authority" => {
                let doc = std::fs::read_to_string(root.0.join(active)).unwrap();
                root.write(
                    active,
                    &doc.replace("First admitted outcome", "Different admitted outcome"),
                );
            }
            "target-base" => {
                git(&["checkout", "-b", "different-base", &base]);
                root.write("src/obsolete.rs", "pub fn upstream() {}\n");
                git(&["commit", "-am", "changed selected base"]);
                evidence.target_base = git_head(&root.0);
                git(&["checkout", &branch]);
            }
            "foreign" => evidence.binding.repository = "foreign-repository".into(),
            "authority-binding" => evidence.binding.authority_digest = "f".repeat(64),
            "receipt-context" => {
                let mut task = store
                    .inspect(&milestone.task_id(&milestone.tasks[0]))
                    .unwrap();
                let receipt = task.receipt.as_mut().unwrap();
                receipt.passed_at = "2026-10-02T10:00:00Z".into();
                let doc = std::fs::read_to_string(root.0.join(active)).unwrap();
                root.write(
                    active,
                    &milestone_engine::render_task_receipt(
                        &doc,
                        "first",
                        receipt,
                        &task.spec.subtasks,
                    )
                    .unwrap(),
                );
                task.spec.receipt = task.receipt.clone();
                store
                    .connection
                    .execute(
                        "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
                        rusqlite::params![serde_json::to_string(&task).unwrap(), task.task_id],
                    )
                    .unwrap();
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
            "untracked" => root.write("untracked-deliverable.rs", "pub fn pending() {}\n"),
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
            git(&["add", "src", "docs", "scripts", "forge-mcp.yaml"]);
            git(&["commit", "-m", "post-review change"]);
        }
        let file = root.0.join(".forge/integration-evidence.json");
        if scenario != "missing-record" {
            std::fs::write(
                &file,
                if scenario == "malformed-record" {
                    b"{\"binding\":false}".to_vec()
                } else {
                    serde_json::to_vec(&evidence).unwrap()
                },
            )
            .unwrap();
        }
        let before_doc = std::fs::read(root.0.join(active)).unwrap();
        let before: Vec<_> = milestone
            .tasks
            .iter()
            .map(|spec| store.inspect_details(&milestone.task_id(spec)).unwrap())
            .collect();
        let before_source = std::fs::read(root.0.join("src/lib.rs")).unwrap();
        let output = milestone_cli(
            &root,
            &[
                "handoff",
                "m-final",
                "--evidence-file",
                file.to_str().unwrap(),
            ],
        );
        assert!(
            !output.status.success(),
            "{scenario}: stale or untrusted evidence was accepted"
        );
        let diagnostic = String::from_utf8_lossy(&output.stderr);
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
            std::fs::read(root.0.join(active)).unwrap(),
            before_doc,
            "{scenario}"
        );
        assert_eq!(
            std::fs::read(root.0.join("src/lib.rs")).unwrap(),
            before_source,
            "{scenario}"
        );
        assert!(!root.0.join(archived).exists(), "{scenario}");
        if scenario == "bytes" {
            assert!(diagnostic.contains("obtain fresh integration review"));
            assert!(!diagnostic.contains("git checkout"));
        }
        for (index, spec) in milestone.tasks.iter().enumerate() {
            assert_eq!(
                store.inspect_details(&milestone.task_id(spec)).unwrap(),
                before[index],
                "{scenario}: task proof mutated"
            );
        }
    }
}
