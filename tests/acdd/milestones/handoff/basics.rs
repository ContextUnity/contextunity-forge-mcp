use super::*;

#[test]
fn handoff_preserves_blackboard_history() {
    let root = MilestoneHarness::linear("m-close");
    let evidence =
        unready_fixture_integration_evidence(root.root(), "m-close", "integration-reviewer");
    let rejected = root
        .handoff_with_evidence("m-close", &evidence)
        .unwrap_err();
    assert!(format!("{rejected:#}").contains("finish"));
    assert!(root.path(root.active_path()).exists());

    root.complete_task(CompletionMode::TaskRef("finish"))
        .unwrap();
    for path in [
        "docs/milestones/010-close.md",
        "docs/milestones/archive/010-close.md",
    ] {
        root.post_blackboard_in(
            "forge-mcp",
            "forge-mcp",
            path,
            "notes",
            "retained until handoff",
        );
    }
    root.post_blackboard_in(
        "other",
        "other",
        "docs/milestones/010-close.md",
        "notes",
        "retained until handoff",
    );
    root.handoff().unwrap();
    let store = root.store();
    let scopes = crate::common::sqlite::query_strings(
        &store.connection,
        "SELECT milestone_ref FROM task_blackboard WHERE task_id IS NULL",
        [],
    );
    assert_eq!(scopes, ["other/other/docs/milestones/010-close.md"]);
}

#[test]
fn archived_proof_list_validation_covers_valid_and_invalid_values() {
    let fixture = MilestoneHarness::configured_workspace();
    let archived = MilestoneFixtureBuilder::new("m-terminal")
        .status("cancelled")
        .without_started_at()
        .without_doc_type()
        .closure_reason("Cancelled fixture")
        .task("cancelled", &["src/"])
        .task_target("Preserve cancellation")
        .task_status("cancelled")
        .build();
    for (proof, valid) in [
        ("[]", true),
        ("['0123456789012345678901234567890123456789']", true),
        ("null", false),
        ("not-a-list", false),
        ("{}", false),
        ("[bad-object]", false),
        ("['0123456789012345678901234567890123456789', bad]", false),
        (
            "['0123456789012345678901234567890123456789012345678901234567890123']",
            false,
        ),
    ] {
        let (header, body) = archived
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap();
        fixture.write(
            "docs/milestones/archive/010-terminal.md",
            &format!("---\n{header}\nhandoff:\n  proof_shas: {proof}\n---\n{body}"),
        );
        let output = fixture.db_recover();
        assert_eq!(output.is_ok(), valid, "proof_shas {proof}: {output:#?}");
        assert_eq!(fixture.root().join(".forge/tasks.sqlite").exists(), valid);
        for name in ["tasks.sqlite", "tasks.sqlite-wal", "tasks.sqlite-shm"] {
            let path = fixture.path(format!(".forge/{name}"));
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}
