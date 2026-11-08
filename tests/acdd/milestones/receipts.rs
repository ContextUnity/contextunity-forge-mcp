use super::*;

#[test]
fn repacked_task_commits_sync_and_handoff_cleanly() {
    let milestone_content = MilestoneFixtureBuilder::new("m-repack")
        .title("Repack test")
        .without_status()
        .task("first", &["src/lib.rs"])
        .task_target("Complete first task")
        .build();
    let root = MilestoneHarness::receipt_workspace(
        "docs/milestones/010-repack.md",
        &milestone_content,
        "",
    );
    let original_profile = root.document(".forge/acdd/profile.yaml");
    let _initial_commit = root.head();

    // 1. Sync milestone
    let sync_res = root.sync("docs/milestones/010-repack.md").unwrap();
    assert_eq!(sync_res["tasks"].as_array().unwrap().len(), 1);

    // 2. Complete the task through engine
    let task_id = root.task_id("first");

    // Create an intermediate commit on the task branch before delivery modifying only src/lib.rs
    root.write("src/lib.rs", "pub fn work() -> bool { false }\n// step 1\n");
    root.commit_tracked("Intermediate commit before delivery");
    let c1_sha = root.head();

    // Deliver task with final implementation in scope
    root.write(
        "src/lib.rs",
        "pub fn work() -> bool { true }\n// completed\n",
    );
    root.complete_task(CompletionMode::TaskRef("first"))
        .unwrap();

    // 3. Verify milestone markdown document was updated on disk with the receipt
    let milestone_doc = root.document("docs/milestones/010-repack.md");
    assert!(
        milestone_doc.contains("receipt:"),
        "milestone document must have receipt written"
    );
    assert!(
        milestone_doc.contains("status: completed"),
        "milestone task must be completed"
    );

    // 4. Repack task commit via amend (reword commit message while preserving delivered scope tree)
    root.amend_message("Repacked clean task commit");

    let repacked_sha = root.head();

    // Update milestone markdown with the repacked SHA in receipt
    let (updated_receipt, updated_doc) =
        MilestoneHarness::retarget_document_receipt(&milestone_doc, "first", &repacked_sha)
            .unwrap();
    root.write("docs/milestones/010-repack.md", &updated_doc);

    // 5c. Foreign commit touching files outside task scope: sync must reject
    root.write("tests/unrelated.rs", "fn foreign() {}\n");
    root.stage_paths(&["tests/unrelated.rs"]);
    root.commit_staged("Foreign commit outside scope");
    let foreign_sha = root.head();
    // 5d. Commit modifying protected profile without scope: sync must reject
    root.write(".forge/acdd/profile.yaml", "gates: []\n");
    root.stage_forced_paths(&[".forge/acdd/profile.yaml"]);
    root.commit_staged("Modify protected profile without scope");
    let protected_sha = root.head();
    // 5f. Divergent commit from the same baseline with different content in scope: sync must reject
    let divergent_sha = {
        // Discard only the forged receipt from the preceding rejection case;
        // the divergent history must actually start at the intended baseline.
        MilestoneHarness::git_at(root.root(), &["checkout", "--force", "-q", &c1_sha]).unwrap();
        root.write(
            "src/lib.rs",
            "pub fn work() -> bool { false }\n// different code\n",
        );
        let output =
            root.commit_tracked("Divergent commit from same baseline with different scope code");
        assert!(output.status.success());
        let d_sha = root.head();
        root.select_revision(&protected_sha);
        root.write(
            "src/lib.rs",
            "pub fn work() -> bool { true }\n// completed\n",
        );
        d_sha
    };
    // Every receipt permutation reaches validation with the genuine valid
    // runner profile. A malformed active profile must not mask receipt errors.
    root.write(".forge/acdd/profile.yaml", &original_profile);
    for (scenario, commit, rejection) in [
        (
            "decision",
            None,
            "tampered receipt must be rejected by task sync",
        ),
        (
            "passed_at",
            None,
            "tampered passed_at must be rejected by task sync",
        ),
        (
            "foreign",
            Some(foreign_sha.as_str()),
            "commit outside task scope must be rejected by task sync",
        ),
        (
            "protected",
            Some(protected_sha.as_str()),
            "commit modifying protected profile must be rejected by task sync",
        ),
        (
            "ancestor",
            Some(c1_sha.as_str()),
            "obsolete ancestor commit must be rejected by task sync",
        ),
        (
            "divergent",
            Some(divergent_sha.as_str()),
            "divergent commit with different code in scope must be rejected by task sync",
        ),
    ] {
        let mut receipt = updated_receipt.clone();
        match scenario {
            "decision" => receipt.decision = "fail".into(),
            "passed_at" => receipt.passed_at = "2099-01-01T00:00:00Z".into(),
            _ => receipt.commit = commit.map(str::to_owned),
        }
        let invalid_doc =
            milestone_engine::render_task_receipt(&updated_doc, "first", &receipt, &[]).unwrap();
        root.write("docs/milestones/010-repack.md", &invalid_doc);
        let result = root.sync("docs/milestones/010-repack.md");
        assert!(result.is_err(), "{scenario}: {rejection}");
    }

    // 5g. Corrupted evidence in task_gates: sync and migrate verify must fail closed
    let valid_gate_evidence = {
        let store = root.store();
        let mut statement = store
            .connection
            .prepare(
                "SELECT gate,revision,evidence FROM task_gates WHERE task_id=?1 AND state='passed' ORDER BY revision,gate",
            )
            .unwrap();
        let rows = statement
            .query_map([task_id.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<(String, u64, String)>>>()
            .unwrap();
        rows
    };
    let store = root.open_task_store().unwrap();
    assert_eq!(
        MilestoneHarness::overwrite_passed_gate_evidence(
            &store,
            &task_id,
            "deliver",
            "{corrupted-non-json: candidate_baseline_head}",
        )
        .unwrap(),
        1
    );
    drop(store);
    root.write("docs/milestones/010-repack.md", &updated_doc);
    let corrupt_sync = root.sync("docs/milestones/010-repack.md");
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
        root.root(),
        contextunity_forge_mcp::cli::migrate::MigrateCommand::Verify {
            milestone_ref: Some("docs/milestones/010-repack.md".into()),
        },
    );
    assert!(
        corrupt_verify.is_err(),
        "migrate verify must fail closed when task_gates evidence is corrupted"
    );

    let store = root.store();
    for (gate, revision, evidence) in &valid_gate_evidence {
        store
            .connection
            .execute(
                "UPDATE task_gates SET evidence=?1 WHERE task_id=?2 AND gate=?3 AND revision=?4",
                rusqlite::params![evidence, task_id, gate, revision],
            )
            .unwrap();
    }
    drop(store);

    // Restore the valid repacked doc
    root.write("docs/milestones/010-repack.md", &updated_doc);

    // 6. Test that task sync succeeds with the repacked commit
    let sync_repack = root.sync("docs/milestones/010-repack.md").unwrap();
    assert_eq!(sync_repack["tasks"].as_array().unwrap().len(), 1);

    // Verify SQLite task receipt has repacked SHA
    let store2 = root.store();
    let inspected = store2.inspect(&task_id).unwrap();
    assert_eq!(
        inspected.receipt.unwrap().commit.as_deref(),
        Some(repacked_sha.as_str())
    );
    drop(store2);

    // 7. Verify migrate check passes without conflicts
    let verify_res = root
        .migrate("verify", Some("docs/milestones/010-repack.md"))
        .unwrap();
    assert_eq!(verify_res["conflicts"].as_array().unwrap().len(), 0);

    // Commit the milestone doc change before handoff
    root.stage_paths(&["docs/milestones/010-repack.md"]);
    root.commit_staged("Record repacked SHA in milestone");
    let doc_only_sha = root.head();

    // Merge divergent_sha into milestone branch so it becomes an ancestor of handoff_commit
    MilestoneHarness::git_at(
        root.root(),
        &[
            "merge",
            "-s",
            "ours",
            "-m",
            "Merge divergent branch for ancestry check",
            &divergent_sha,
        ],
    )
    .unwrap();

    // Migration still validates explicit pointer updates; handoff treats them as navigation.
    let mut doc_only_receipt = updated_receipt.clone();
    doc_only_receipt.commit = Some(doc_only_sha);
    let doc_only_doc =
        milestone_engine::render_task_receipt(&updated_doc, "first", &doc_only_receipt, &[])
            .unwrap();
    root.write("docs/milestones/010-repack.md", &doc_only_doc);

    // 7e. Test migrate verify rejects when markdown has foreign commit or commit not touching scope
    let verify_foreign = root.migrate("verify", Some("docs/milestones/010-repack.md"));
    assert!(
        verify_foreign.is_err(),
        "migrate verify must reject markdown receipt specifying commit that does not touch scope"
    );

    // Restore valid markdown doc with repacked SHA before handoff
    root.write("docs/milestones/010-repack.md", &updated_doc);

    // Verify migrate verify passes for the valid repacked SHA
    let verify_valid = root.migrate("verify", Some("docs/milestones/010-repack.md"));
    assert!(
        verify_valid.is_ok(),
        "migrate verify must succeed for valid repacked SHA"
    );

    // Remove the deliberately foreign migration fixtures from the final integration.
    for path in ["tests/unrelated.rs", ".forge/acdd/profile.yaml"] {
        root.remove_file(path);
    }
    root.commit_fixture_inputs(&["tests/unrelated.rs", ".forge/acdd/profile.yaml"]);
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&fixture_profile()).unwrap(),
    );
    let handoff_commit = root.head();

    // 8. Run milestone handoff
    let evidence = root.default_fixture_evidence("m-repack", "cargo test", 1, 0);
    let handoff_res = root
        .handoff_at("m-repack", Some(&handoff_commit), &evidence)
        .unwrap();
    assert_eq!(handoff_res["id"], "m-repack");

    let archived_path = root.root().join("docs/milestones/archive/010-repack.md");
    assert!(archived_path.exists());
    let archived_doc = root.document("docs/milestones/archive/010-repack.md");
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
    let milestone_content = MilestoneFixtureBuilder::new("m-direct-repack")
        .title("Direct repack test")
        .without_status()
        .task("direct", &["src/lib.rs"])
        .task_target("Complete direct task")
        .build();
    let root = MilestoneHarness::receipt_workspace(
        "docs/milestones/010-direct.md",
        &milestone_content,
        "",
    );

    // 1. Sync milestone
    root.sync("docs/milestones/010-direct.md").unwrap();

    // 2. Complete the task through engine
    root.write(
        "src/lib.rs",
        "pub fn work() -> bool { true }\n// direct delivered\n",
    );
    root.complete_task(CompletionMode::TaskRef("direct"))
        .unwrap();

    let milestone_doc = root.document("docs/milestones/010-direct.md");

    // 3. Repack commit via amend (squash / reword commit message without modifying verified scope tree)
    root.amend_message("Repacked direct task commit");

    let repacked_sha = root.head();

    // Update milestone markdown with the repacked SHA in receipt
    let (_, updated_doc) =
        MilestoneHarness::retarget_document_receipt(&milestone_doc, "direct", &repacked_sha)
            .unwrap();
    root.write("docs/milestones/010-direct.md", &updated_doc);

    // Notice: We deliberately DO NOT run task sync here!
    // SQLite still has the old delivery commit.

    // Commit the milestone doc change
    root.stage_paths(&["docs/milestones/010-direct.md"]);
    root.commit_staged("Record repacked SHA in milestone doc");

    let handoff_commit = root.head();

    // 4. Run milestone handoff: it should pick the commit from the milestone doc!
    let evidence = root.default_fixture_evidence("m-direct-repack", "cargo test", 1, 0);
    let handoff_res = root
        .handoff_at("m-direct-repack", Some(&handoff_commit), &evidence)
        .unwrap();
    assert_eq!(handoff_res["id"], "m-direct-repack");

    let archived_path = root.root().join("docs/milestones/archive/010-direct.md");
    assert!(archived_path.exists());
    let archived_doc = root.document("docs/milestones/archive/010-direct.md");
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
    let store2 = root.store();
    assert!(store2.list(None, "all", None).unwrap().is_empty());
}

#[test]
fn record_task_commits_false_archives_commitless_markdown_receipt_and_syncs_cleanly() {
    let milestone_content = MilestoneFixtureBuilder::new("m-commitless")
        .title("Commitless test")
        .without_status()
        .task("work", &["src/lib.rs"])
        .task_target("Complete work task")
        .build();
    let root = MilestoneHarness::receipt_workspace(
        "docs/milestones/010-commitless.md",
        &milestone_content,
        "record_task_commits: false\n",
    );

    // 1. Sync milestone
    root.sync("docs/milestones/010-commitless.md").unwrap();

    // 2. Complete task through engine with implementation in scope
    root.write(
        "src/lib.rs",
        "pub fn work() -> bool { true }\n// implementation\n",
    );
    root.complete_task(CompletionMode::TaskRef("work")).unwrap();

    let handoff_commit = root.head();

    // 3. Run milestone handoff
    let evidence = root.default_fixture_evidence("m-commitless", "cargo test", 1, 0);
    let handoff_res = root
        .handoff_at("m-commitless", Some(&handoff_commit), &evidence)
        .unwrap();
    assert_eq!(handoff_res["id"], "m-commitless");

    // 4. Verify the archived document has a commitless receipt (no commit SHA in markdown)
    let archived_path = root
        .root()
        .join("docs/milestones/archive/010-commitless.md");
    assert!(archived_path.exists());
    let archived_doc = root.document("docs/milestones/archive/010-commitless.md");
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
    let store2 = root.store();
    assert!(store2.list(None, "all", None).unwrap().is_empty());

    // 6. Explicit archive sync fails closed even for a valid commitless receipt.
    let sync_res = root.sync("docs/milestones/archive/010-commitless.md");
    assert!(format!("{:#}", sync_res.unwrap_err()).contains("STORE_SYNC_FAILED"));

    // 7. Rejected archive sync does not resurrect SQLite history.
    let store3 = root.store();
    assert!(store3.list(None, "all", None).unwrap().is_empty());
    drop(store3);

    // 8. Verify migrate verify succeeds with 0 conflicts on the commitless archive
    let migrate_res = contextunity_forge_mcp::cli::migrate::run(
        root.root(),
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
    let milestone_content = MilestoneFixtureBuilder::new("m-squash")
        .title("Squash test")
        .without_status()
        .task("work", &["src/lib.rs", "docs/notes.md"])
        .task_target("Complete work task")
        .build();
    let root = MilestoneHarness::receipt_workspace(
        "docs/milestones/010-squash.md",
        &milestone_content,
        "",
    );

    // 1. Sync milestone
    root.sync("docs/milestones/010-squash.md").unwrap();

    // 2. Complete task through engine
    root.complete_task(CompletionMode::TaskRef("work")).unwrap();

    // Rewriting the full history preserves content, not the original baseline.
    let delivery_sha = root.head();
    root.create_orphan_branch("rewritten-history");
    root.commit_staged("Squashed release commit containing task implementation");

    let squashed_sha = root.head();
    assert_ne!(delivery_sha, squashed_sha);

    // 4. Create an additional commit on top (e.g. docs commit)
    root.write("docs/notes.md", "release documentation\n");
    root.stage_paths(&["docs/notes.md"]);
    root.commit_staged("Documentation commit on top of squash");
    let tip_sha = root.head();
    assert_ne!(squashed_sha, tip_sha);

    // 5. Handoff at tip succeeds and correctly resolves squashed_sha (introducing commit), NOT tip_sha!
    let evidence = root.default_fixture_evidence("m-squash", "cargo test", 1, 0);
    let handoff_res = root
        .handoff_at("m-squash", Some(&tip_sha), &evidence)
        .unwrap();
    assert_eq!(handoff_res["id"], "m-squash");

    let store2 = root.store();
    assert!(store2.list(None, "all", None).unwrap().is_empty());
    let archive = root.document("docs/milestones/archive/010-squash.md");
    let archived = Milestone::parse(&archive, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|r| r.commit.as_deref()),
        Some(squashed_sha.as_str()),
        "handoff must record the squashed commit that introduced the scoped changes, not the tip commit on top"
    );
}
