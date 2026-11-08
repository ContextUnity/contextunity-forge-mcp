use super::*;

#[test]
fn handoff_retains_opt_out_history_and_records_polish_verification() {
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nhandoff_pruning: false\n",
    );
    root.write("src/lib.rs", "pub fn fallback() {}\n");
    let source = MilestoneFixtureBuilder::new("m-fallback")
        .title("Fallback work")
        .without_started_at()
        .task("finish", &["src/"])
        .task_target("Finish fallback")
        .build();
    let active = "docs/milestones/020-fallback.md";
    root.write(active, &source);
    root.commit_fixture_inputs(&["forge-mcp.yaml", active]);
    let (milestone, store) = root.sync_document(active, &source);
    let task_id = milestone.task_id(&milestone.tasks[0]);
    root.complete_task(CompletionMode::Store {
        store: &store,
        task_id: &task_id,
    })
    .unwrap();
    let claim_time = store
        .earliest_claim("forge-mcp/m-fallback:")
        .unwrap()
        .unwrap();
    drop(store);

    let mut profile = fixture_profile();
    let check: contextunity_forge_mcp::core::tasks::profile::CommandDef =
        serde_yaml::from_str("command: 'test -s src/lib.rs'\noutput: {require_counts: false}\n")
            .unwrap();
    profile
        .commands
        .insert("check_full_repo".into(), check.clone());
    profile.commands.insert("project-check".into(), check);
    profile.commands.insert(
        "project-lint".into(),
        contextunity_forge_mcp::core::tasks::profile::CommandDef::Script(
            "! grep -q BAD_LINT src/lib.rs".into(),
        ),
    );
    profile.commands.insert(
        "failing-lint".into(),
        contextunity_forge_mcp::core::tasks::profile::CommandDef::Script(
            "grep -q BAD_LINT src/lib.rs".into(),
        ),
    );
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&profile).unwrap(),
    );
    let before_polish = root.head();
    root.write(
        "src/lib.rs",
        "pub fn fallback() { /* integrated source polish */ }\n",
    );
    root.commit_fixture_inputs(&["src/lib.rs"]);
    let mut evidence = root.default_fixture_evidence("m-fallback", "cargo test", 1, 0);
    let polish_commit = root.head();
    evidence.verification.project_command = Some("project-check".into());
    evidence.verification.lint_command = Some("failing-lint".into());
    assert_ne!(polish_commit, before_polish);
    let failed_lint = root.handoff_at("m-fallback", None, &evidence).unwrap_err();
    assert!(format!("{failed_lint:#}").contains("HANDOFF_VERIFICATION_FAILED"));
    assert!(root.path("docs/milestones/020-fallback.md").exists());
    assert!(!root
        .path("docs/milestones/archive/020-fallback.md")
        .exists());
    evidence.verification.lint_command = Some("project-lint".into());
    root.handoff_at("m-fallback", Some(&polish_commit), &evidence)
        .unwrap();
    let retained = root.store();
    let retained_task = retained.inspect(&task_id).unwrap();
    assert_eq!(retained_task.status, "completed");
    assert_eq!(
        retained_task.milestone_ref,
        "docs/milestones/archive/020-fallback.md"
    );
    assert!(!retained.milestone_tombstoned("m-fallback").unwrap());
    let text = root.document("docs/milestones/archive/020-fallback.md");
    let meta = MilestoneHarness::frontmatter(&text);
    assert_eq!(meta["handoff"]["passport"]["linter"]["status"], "clean");
    assert_eq!(
        meta["handoff"]["passport"]["linter"]["observation"]["exit_code"],
        0
    );
    assert_eq!(
        meta["handoff"]["passport"]["linter"]["observation"]["command"],
        "! grep -q BAD_LINT src/lib.rs"
    );
    assert_eq!(
        meta["handoff"]["passport"]["automatic_project_verification"]["selection"],
        "project-check"
    );
    let (started, minutes) = MilestoneHarness::handoff_times(&meta);
    assert_eq!(started, claim_time);
    assert_eq!(
        meta["handoff"]["duration"].as_str().unwrap(),
        format!("{}h {}m", minutes / 60, minutes % 60)
    );

    // Archive discovery precedes import even when another worktree still has
    // the unmerged active document. Archived opt-out SQL history is not rebuilt.
    let stale = root.path("stale-worktree");
    root.add_detached_worktree(&stale, &polish_commit).unwrap();
    let orphan = "refs/forge/state/forge-mcp/forge-mcp/m-fallback/orphan";
    root.set_ref(orphan, &polish_commit);
    root.remove_file(".forge/tasks.sqlite");
    root.db_recover().unwrap();
    let recovered_store = root.store();
    assert!(recovered_store.list(None, "all", None).unwrap().is_empty());
    assert!(recovered_store.milestone_tombstoned("m-fallback").unwrap());
    assert!(!root.has_ref(orphan));
    drop(recovered_store);
    let stale_sync = contextunity_forge_mcp::engine::tasks::manage(
        &stale,
        contextunity_forge_mcp::engine::tasks::Manage {
            action: contextunity_forge_mcp::engine::tasks::ManageAction::Sync,
            milestone_ref: Some("docs/milestones/020-fallback.md".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(format!("{stale_sync:#}").contains("STORE_SYNC_FAILED"));
}
