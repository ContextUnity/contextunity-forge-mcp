use super::*;

#[test]
fn foreign_handoff_repairs_archive_and_recovers_interrupted_journal() {
    let mut fixture = ForeignHandoffFixture::new();
    let base_context =
        MilestoneHarness::handoff_context_at(&fixture.foreign, "m-foreign", &fixture.commit)
            .unwrap();
    assert_eq!(base_context["target_base"], fixture.commit);
    let _ =
        milestone_engine::handoff_context(&fixture.foreign, "m-foreign", &fixture.commit).unwrap();
    fixture.context =
        milestone_engine::handoff_context(&fixture.foreign, "m-foreign", &fixture.selected_target)
            .unwrap();
    let documents: std::collections::BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../fixtures/milestones/foreign_handoff.json"
    ))
    .unwrap();
    for path in [
        "docs/milestones/022-same-id.md",
        "docs/milestones/021-proof.md",
    ] {
        fixture
            .foreign_parent
            .write(format!("linked/{path}"), &documents[path]);
        MilestoneHarness::git_at(&fixture.foreign, &["add", path]).unwrap();
    }
    MilestoneHarness::git_at(
        &fixture.foreign,
        &["commit", "-m", "foreign task authority"],
    )
    .unwrap();
    for path in [
        "docs/milestones/021-proof.md",
        "docs/milestones/022-same-id.md",
    ] {
        tasks::manage(
            &fixture.foreign,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(path.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    TaskHarness::claim_at(
        &fixture.foreign,
        "other/m-proof:proof",
        "contract",
        "foreign-author",
    )
    .unwrap();
    let store = TasksStore::open_project(
        &tasks::database_path(&fixture.foreign).unwrap(),
        "forge-mcp",
        "other",
    )
    .unwrap();
    for dependency in [&fixture.task_id, "forge-mcp/m-close:__milestone__"] {
        MilestoneHarness::seed_dependency(&store, FOREIGN_WAITING, dependency);
    }
    let task = store.inspect("other/m-proof:proof").unwrap();
    let accepted = tasks::submit(
        &fixture.foreign,
        TaskHarness::pass_request(&task.task_id, "contract", &evidence(&task)),
    )
    .unwrap();
    fixture.foreign_proof = accepted["snapshot"]["commit"].as_str().unwrap().to_owned();
    for sha in [&fixture.selected_target, &fixture.foreign_proof] {
        MilestoneHarness::git_at(
            &fixture.foreign,
            &["update-ref", &format!("refs/forge/evidence/{sha}"), sha],
        )
        .unwrap();
    }
    let selected_context = fixture.context.clone();
    assert_eq!(selected_context["target_base"], fixture.selected_target);
    assert_ne!(selected_context["commit"], fixture.selected_target);
    assert_ne!(fixture.commit, fixture.selected_target);
    let prepared: Value = serde_json::from_slice(
        &MilestoneHarness::git_at(
            &fixture.foreign,
            &["show", &format!("{FOREIGN_KEEPER}:checkpoint.json")],
        )
        .unwrap()
        .stdout,
    )
    .unwrap();
    let parents =
        MilestoneHarness::git_text_at(&fixture.foreign, &["cat-file", "-p", FOREIGN_KEEPER]);
    for proof in [
        &fixture.selected_target,
        selected_context["commit"].as_str().unwrap(),
    ] {
        assert!(
            prepared["prepared_proofs"]
                .as_array()
                .unwrap()
                .contains(&json!(proof)),
            "new preparation must reach its real task keeper: {proof}"
        );
        assert!(
            parents
                .lines()
                .any(|line| line == format!("parent {proof}")),
            "actual prepared object must have a keeper parent: {proof}"
        );
    }
    let ancestry = MilestoneHarness::git_output_at(
        &fixture.foreign,
        [
            "merge-base",
            "--is-ancestor",
            &fixture.selected_target,
            fixture.context["commit"].as_str().unwrap(),
        ],
    )
    .status
    .success();
    assert!(ancestry);
    assert!(
        MilestoneHarness::git_at(&fixture.foreign, &["status", "--porcelain"])
            .unwrap()
            .stdout
            .is_empty()
    );
    for submission in &fixture.submissions {
        assert_eq!(
            submission.status,
            if submission.is_delivery {
                "completed"
            } else {
                "ready"
            },
            "gate {}",
            submission.stage
        );
    }
    let root = &fixture.root;
    let read_files = |paths: &[&str]| {
        paths
            .iter()
            .map(|path| std::fs::read(root.path(path)).ok())
            .collect::<Vec<_>>()
    };
    let source = fixture.source.clone();
    let commit = fixture.base.clone();
    let handoff_commit = fixture.commit.clone();
    let delivered_active = fixture.delivered.clone();
    let foreign_id = FOREIGN_WAITING;
    let selected_target = fixture.selected_target.clone();
    let foreign_proof = fixture.foreign_proof.clone();
    let completed_handoff = || fixture.handoff();
    root.write(
        ARCHIVE,
        &format!(
            "interrupted first archive write\n{}\n{}\n",
            fixture.foreign_proof, fixture.selected_target
        ),
    );
    root.restore_git_note(&fixture.commit, PRESERVED_NOTE.as_bytes());
    for reference in [
        "refs/forge/state/forge-mcp/forge-mcp/m-close/keep",
        "refs/forge/state/other/m-close/keep",
        "refs/forge/state/m-close/keep",
    ] {
        root.set_ref(reference, &fixture.commit);
    }
    let recovery_snapshot = root.checkpoint();
    let anchor = MilestoneHarness::git_text_at(root.root(), &["hash-object", "--stdin"]);
    let (preparation, mut unowned) = root.git_note(&anchor);
    unowned["handoff_proof_owners"]
        .as_object_mut()
        .unwrap()
        .retain(|_, owner| owner["identity"] != "forge-mcp/other/m-foreign");
    root.set_git_note(&anchor, &unowned);
    let before = read_files(&[ACTIVE, ARCHIVE]);
    let legacy = fixture.handoff().unwrap_err();
    assert!(format!("{legacy:#}").contains("HANDOFF_PROOF_OWNERSHIP_UNAVAILABLE"));
    assert_eq!(read_files(&[ACTIVE, ARCHIVE]), before);
    root.restore_git_note(&anchor, &preparation.stdout);
    root.set_git_note(&fixture.commit, &json!({"external_note": "x".repeat(2048)}));
    let oversized = fixture.handoff().unwrap_err();
    assert!(
        format!("{oversized:#}").contains("HANDOFF_PASSPORT_TOO_LARGE"),
        "unexpected oversized handoff error: {oversized:#}"
    );
    assert_eq!(read_files(&[ACTIVE, ARCHIVE]), before);
    root.restore_git_note(&fixture.commit, PRESERVED_NOTE.as_bytes());
    completed_handoff().unwrap();
    let parsed = Milestone::parse(&root.document(ARCHIVE), "forge-mcp").unwrap();
    let accepted = Milestone::parse(&fixture.delivered, "forge-mcp")
        .unwrap()
        .tasks[0]
        .receipt
        .clone()
        .unwrap();
    assert!(parsed.tasks[0]
        .receipt
        .as_ref()
        .unwrap()
        .matches_proofs(&accepted));
    assert!(!root.root().join("docs/milestones/010-close.md").exists());
    let reopened = root.store();
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
        let count: i64 = MilestoneHarness::task_rows(&reopened, table, &fixture.task_id).unwrap();
        assert_eq!(count, 0, "{table}");
    }
    let foreign_task_store = root.store_in("forge-mcp", "other");
    let foreign_task = foreign_task_store.inspect(foreign_id).unwrap();
    assert_eq!(foreign_task.task_id, foreign_id);
    let foreign_revisions: i64 =
        MilestoneHarness::task_rows(&reopened, "task_revisions", foreign_id).unwrap();
    assert_eq!(foreign_revisions, 1);
    assert_eq!(
        crate::common::sqlite::query_value::<i64>(
            &reopened.connection,
            "SELECT COUNT(*) FROM task_dependencies WHERE task_id=?1 AND satisfied=1",
            [foreign_id],
        ),
        2
    );
    let tombstone = MilestoneHarness::tombstone(&reopened, "forge-mcp/forge-mcp/m-close");
    assert_eq!(tombstone.terminal, 1);
    assert_eq!(
        tombstone.completed_tasks,
        std::slice::from_ref(&fixture.task_id)
    );
    assert_eq!(tombstone.total, 1);
    // A completed handoff remains successful after its task/gate rows are gone.
    completed_handoff().unwrap_or_else(|error| {
        panic!("post-prune handoff retry must not require live tasks: {error:#}")
    });
    // A stale manifest from an unmerged worktree must not resurrect archived tasks.
    root.write("docs/milestones/010-close.md", &source);
    let synced = root
        .manage(tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("forge-mcp".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(synced["total"], 0);
    for reference in [
        "docs/milestones/010-close.md",
        "docs/milestones/archive/010-close.md",
    ] {
        let error = root
            .manage(tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(reference.into()),
                ..Default::default()
            })
            .unwrap_err();
        assert!(format!("{error:#}").contains("STORE_SYNC_FAILED"));
    }
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
    root.remove_file("docs/milestones/010-close.md");
    let archived = root.document("docs/milestones/archive/010-close.md");
    let parsed = Milestone::parse(&archived, "forge-mcp").unwrap();
    let completed = parsed
        .tasks
        .iter()
        .find(|task| task.task_ref == "finish")
        .expect("the admitted task is preserved in the archive");
    assert_eq!(completed.status.as_deref(), Some("completed"));
    assert!(completed
        .extended_scope
        .iter()
        .any(|scope| scope == "src/extra.rs"));
    assert!(root.path("src/extra.rs").exists());
    assert!(
        completed.receipt.is_some(),
        "archive retains its typed receipt"
    );
    let meta = MilestoneHarness::frontmatter(&archived);
    assert_eq!(meta["status"], "completed");
    assert_eq!(
        meta["handoff"]["commit"].as_str(),
        Some(handoff_commit.as_str())
    );
    let (_, minutes) = MilestoneHarness::handoff_times(&meta);
    assert_eq!(
        meta["handoff"]["duration"].as_str().unwrap(),
        format!("{}h {}m", minutes / 60, minutes % 60)
    );
    let verification = &meta["handoff"]["verification"];
    assert_eq!(
        verification["command"],
        ": {scope_target} && test -x .forge/runner_fixture && .forge/runner_fixture"
    );
    assert_eq!(verification["status"], "passed");
    assert_eq!(verification["tests_passed"], 1);
    assert_eq!(verification["tests_failed"], 0);
    let passport = MilestoneHarness::yaml_json(&meta["handoff"]["passport"]);
    assert_eq!(passport["tasks_count"], 2);
    assert_eq!(passport["linter"]["status"], "unobserved");
    assert_eq!(passport["test_suite_results"]["tests_passed"], 1);
    assert!(passport["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["status"] == "completed" && task["execution_wall_seconds"].is_u64()));
    assert!(passport["ast_symbol_deltas"].is_object());
    assert_eq!(
        passport["ast_symbol_deltas"],
        json!({"added": 1, "removed": 0, "modified": 0})
    );
    assert_eq!(
        passport["verified_handoff_proof"].as_str().unwrap().len(),
        64
    );
    let (note_output, note) = root.git_note(&handoff_commit);
    assert!(note_output.status.success());
    assert!(
        note_output.stdout.len() < 2048,
        "Git note must stay below 2 KiB"
    );
    let compact = &note["milestone_passports"]["forge-mcp/forge-mcp/m-close"];
    assert!(compact.get("terminal_tasks").is_none());
    assert!(compact.get("landed_receipts").is_none());
    assert!(compact.get("tasks").is_none());
    assert_eq!(compact["tasks_count"], passport["tasks_count"]);
    assert_eq!(compact["verification"]["tests_passed"], 1);
    assert_eq!(
        note["milestone_passports"]["forge-mcp/other/m-close"]["retained"],
        true
    );
    assert_eq!(note["external_note"], "preserved");
    let proof_anchor = MilestoneHarness::git_text_at(root.root(), &["hash-object", "--stdin"]);
    let (_, owners) = root.git_note(&proof_anchor);
    let owners = owners["handoff_proof_owners"].as_object().unwrap();
    assert!(!owners
        .values()
        .any(|owner| owner["identity"] == "forge-mcp/forge-mcp/m-close"));
    assert!(owners.values().any(|owner| {
        owner["identity"] == "forge-mcp/other/m-foreign"
            && owner["proof_shas"]
                .as_array()
                .unwrap()
                .iter()
                .any(|sha| sha == &json!(selected_target))
    }));
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
        assert_eq!(root.has_ref(reference), retained);
    }
    assert!(!root
        .root()
        .join("docs/milestones/archive/010-close.md.bak")
        .exists());
    for sha in meta["handoff"]["proof_shas"].as_sequence().unwrap() {
        let reference = format!("refs/forge/evidence/{}", sha.as_str().unwrap());
        assert_eq!(
            root.has_ref(&reference),
            sha.as_str() == Some(selected_target.as_str())
                || sha.as_str() == Some(foreign_proof.as_str()),
            "active foreign prepared target owns {reference}"
        );
    }
    drop(foreign_task_store);
    drop(reopened);
    recovery_snapshot.restore(root);

    let before_active = root.file_bytes(ACTIVE);
    let crashed = handoff_with_journal_capture(
        root,
        "m-close",
        Some(&handoff_commit),
        &fixture.evidence_file,
        HandoffCrashPhase::Note,
    );
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(crashed.output.status.signal(), Some(9));
    assert!(!crashed.output.status.success());
    assert_eq!(crashed.journals.len(), 1);
    let (journal_path, journal) = crashed.journals.into_iter().next().unwrap();
    assert!(journal_path.exists(), "interrupted journal must survive");
    let prepared_archive = journal["archive_text"].as_str().unwrap();
    let prepared_meta = MilestoneHarness::frontmatter(prepared_archive);
    assert_eq!(journal["target_base"].as_str(), Some(commit.as_str()));
    assert_eq!(journal["commit"].as_str(), Some(handoff_commit.as_str()));
    assert_ne!(commit, handoff_commit);
    let prepared_deltas =
        MilestoneHarness::yaml_json(&prepared_meta["handoff"]["passport"]["ast_symbol_deltas"]);
    assert_eq!(
        prepared_deltas,
        json!({"added": 1, "removed": 0, "modified": 0}),
        "crashed handoff must record the added src/extra.rs::admitted_extension symbol"
    );
    assert_eq!(
        root.file_bytes("docs/milestones/010-close.md"),
        before_active
    );
    completed_handoff().unwrap();
    let reopened = root.store();
    let archived = root.document(ARCHIVE);
    let meta = MilestoneHarness::frontmatter(&archived);
    assert_eq!(
        MilestoneHarness::yaml_json(&meta["handoff"]["passport"]["ast_symbol_deltas"]),
        prepared_deltas
    );
    let mut recovery_failures = Vec::new();
    for scenario in [
        "post-prune",
        "torn",
        "foreign",
        "nonterminal",
        "cancelled-to-completed",
        "completed-to-cancelled",
        "missing-active-receipt",
        "altered-active-receipt",
        "missing-journal",
        "corrupt-journal",
        "altered-journal",
        "missing-archive",
        "post-unlink",
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
            | "missing-active-receipt"
            | "altered-active-receipt"
            | "missing-journal"
            | "corrupt-journal"
            | "altered-journal"
            | "post-unlink" => root.write(archive, "---\nid: m-close\nbroken: [\n"),
            "foreign" => root.write(
                archive,
                &archived.replacen("id: m-close", "id: m-foreign", 1),
            ),
            "unmerged" => root.remove_file(archive),
            "missing-archive" => root.remove_file(archive),
            "git-archive" => {
                root.remove_file(active);
                root.commit_fixture_inputs(&["docs"]);
                root.remove_file(archive);
            }
            _ => {}
        }
        if scenario == "nonterminal" {
            root.write(active, &source);
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
        if !matches!(scenario, "missing-journal" | "unmerged" | "git-archive") {
            let mut prepared = journal.clone();
            if scenario == "altered-journal" {
                prepared["archive_text"] = "tampered".into();
            }
            if scenario == "corrupt-journal" {
                MilestoneHarness::write_journal_bytes(&journal_path, b"{broken");
            } else {
                MilestoneHarness::restore_journal(&journal_path, &prepared);
            }
        }
        if scenario == "post-unlink" {
            root.remove_file(active);
        }
        let before_active = read_files(&[active]);
        let before_archive = read_files(&[archive]);
        let output = completed_handoff();
        let success = matches!(
            scenario,
            "post-prune" | "torn" | "git-archive" | "missing-archive" | "post-unlink"
        );
        if output.is_ok() != success {
            recovery_failures.push(format!("{scenario}: {output:#?}"));
            continue;
        }
        if success {
            assert!(!journal_path.exists(), "{scenario}: journal cleanup");
            assert!(!root.root().join(active).exists(), "{scenario}");
            if scenario == "torn" {
                let repaired = root.document(archive);
                let repaired = Milestone::parse(&repaired, "forge-mcp").unwrap();
                let original = Milestone::parse(&archived, "forge-mcp").unwrap();
                assert_eq!(
                    serde_json::to_value(&repaired.tasks[0].receipt).unwrap(),
                    serde_json::to_value(&original.tasks[0].receipt).unwrap()
                );
            }
        } else {
            assert_eq!(read_files(&[active]), before_active, "{scenario}");
            assert_eq!(read_files(&[archive]), before_archive, "{scenario}");
            if scenario == "unmerged" {
                assert!(format!("{:#}", output.unwrap_err()).contains("HANDOFF_ARCHIVE_INVALID"));
            }
        }
        if journal_path.exists() {
            assert_eq!(
                journal_path.parent(),
                Some(root.path(".forge").as_path()),
                "journal outside fixture .forge directory"
            );
            std::fs::remove_file(&journal_path).unwrap();
        }
        assert!(reopened.list(None, "all", None).unwrap().is_empty());
    }
    assert!(
        recovery_failures.is_empty(),
        "recovery matrix: {recovery_failures:?}"
    );
    drop(reopened);

    let mut foreign_store = TasksStore::open_project(
        &tasks::database_path(&fixture.foreign).unwrap(),
        "forge-mcp",
        "other",
    )
    .unwrap();
    foreign_store
        .delete(Some("other/m-proof:proof"), None, true, 0)
        .unwrap();
    for path in ["020-foreign.md", "021-proof.md", "022-same-id.md"] {
        fixture
            .foreign_parent
            .remove(format!("linked/docs/milestones/{path}"));
    }
    for _ in 0..2 {
        completed_handoff().unwrap();
        for sha in [&selected_target, &foreign_proof] {
            let pin = format!("refs/forge/evidence/{sha}");
            assert!(!root.has_ref(&pin));
        }
    }
}
