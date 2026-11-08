use super::*;

#[test]
fn milestone_resolution_prefers_active_path_components_and_propagates_list_errors() {
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\n",
    );
    for (id, title, status, task, target, path) in [
        (
            "m-active-archive-fix",
            "Active Archive Fix",
            "active",
            "active-match",
            "Active manifest",
            "docs/milestones/051-archive-fix.md",
        ),
        (
            "m-archived-archive-fix",
            "Archived Archive Fix",
            "completed",
            "archived-match",
            "Archived manifest",
            "docs/milestones/archive/051-archive-fix.md",
        ),
    ] {
        let document = MilestoneFixtureBuilder::new(id)
            .title(title)
            .status(status)
            .without_started_at()
            .task(task, &["src/"])
            .task_target(target)
            .task_proof_policy("direct-proof")
            .build();
        root.write(path, &document);
    }
    let by_stem = root.sync("051-archive-fix").unwrap();
    let by_stem_tasks = by_stem["tasks"].as_array().unwrap();
    assert_eq!(by_stem_tasks.len(), 1);
    assert_eq!(
        by_stem_tasks[0]["milestone_ref"],
        "docs/milestones/051-archive-fix.md"
    );
    let by_archive_path = root
        .sync("docs/milestones/archive/051-archive-fix.md")
        .unwrap_err();
    assert!(format!("{by_archive_path:#}").contains("STORE_SYNC_FAILED"));
    let unscoped_sync = root.manage(tasks::Manage {
        action: tasks::ManageAction::Sync,
        ..Default::default()
    });
    assert!(
        unscoped_sync.is_err(),
        "sync requires a workspace or milestone selector"
    );

    root.write("docs/milestones/README.md", "# Milestone index\n");
    assert!(root
        .list_tasks("test", "docs/milestones/README.md")
        .is_err());
    let outside = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    outside.write(
        "010-outside.md",
        &root.document("docs/milestones/051-archive-fix.md"),
    );
    std::os::unix::fs::symlink(
        outside.path("010-outside.md"),
        root.path("docs/milestones/053-symlink.md"),
    )
    .unwrap();
    for reference in ["../outside.md", "docs/milestones/053-symlink.md"] {
        assert!(
            root.list_tasks("test", reference).is_err(),
            "milestone path must remain confined to the workspace: {reference}"
        );
    }

    let missing_identity = root.list_tasks("test", "m-does-not-exist").unwrap_err();
    assert!(
        missing_identity
            .to_string()
            .contains("not found in workspace"),
        "task_list must propagate milestone resolution errors: {missing_identity}"
    );
    let malformed_spec = MilestoneFixtureBuilder::new("m-invalid-frontmatter")
        .title("Invalid frontmatter")
        .without_status()
        .build()
        .replacen("doc_type: contract\n", "doc_type: contract\nbroken: [\n", 1);
    assert!(malformed_spec.contains("broken: [\n"));
    let malformed_path = "docs/milestones/nested/052-invalid-frontmatter.md";
    root.write(malformed_path, &malformed_spec);
    let malformed_id_error = root.list_tasks("test", "m-does-not-exist").unwrap_err();
    assert!(
        malformed_id_error.to_string().contains(
            "failed to read milestone id from 'docs/milestones/nested/052-invalid-frontmatter.md'"
        ),
        "ID lookup must preserve manifest parse errors: {malformed_id_error}"
    );
    let malformed_list_error = root.list_tasks("test", malformed_path).unwrap_err();
    assert!(
        malformed_list_error
            .to_string()
            .contains(&format!("invalid milestone manifest '{malformed_path}'")),
        "task_list must propagate malformed milestone frontmatter: {malformed_list_error}"
    );
    let malformed_sync = root.sync(malformed_path);
    assert!(
        malformed_sync
            .unwrap_err()
            .to_string()
            .contains(&format!("invalid milestone manifest '{malformed_path}'")),
        "task_manage sync must reject malformed milestone frontmatter"
    );
}
