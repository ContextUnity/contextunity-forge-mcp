use super::*;

#[test]
fn milestone_specification_parses_typed_deferred_defects_block() {
    let spec = include_str!("../../fixtures/milestones/deferred_defects.md");
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
fn task_settings_parses_various_commit_tracking_formats() {
    for (yaml, expected) in [
        ("commit_tracking: off\n", false),
        ("commit_tracking: \"milestone_only\"\n", false),
        ("record_task_commits: false\n", false),
        ("", true),
    ] {
        let settings: contextunity_forge_mcp::engine::tasks::TaskSettings =
            serde_yaml::from_str(yaml).unwrap();
        assert_eq!(settings.records_task_commits(), expected, "{yaml:?}");
    }
}
