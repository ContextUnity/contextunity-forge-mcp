use super::*;

#[test]
fn subtask_update_preserves_evidence_when_none_provided_and_contract_bump_resets_subtasks() {
    let spec_v1 = "---\nid: m-bump\ntitle: Bump Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: feature\ntarget: Deliver feature\nproof_policy: seam-test-first\nscope: [src/]\ncontract_revision: 1\nsubtasks:\n  - subtask_ref: sub-orig\n    title: Original subtask\n    status: pending\n```\n";
    let root = unconfigured_test_root();
    let milestone_v1 = Milestone::parse(spec_v1, "forge-mcp").unwrap();
    let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
    store
        .sync(&milestone_v1, "docs/010-bump.md", &root)
        .unwrap();
    let task_id = "forge-mcp/m-bump:feature";

    // 1. Update subtask with evidence
    let updated = store
        .subtask_update(task_id, "sub-orig", "in_progress", Some("Initial evidence"))
        .unwrap();
    assert_eq!(updated.status, "in_progress");
    assert_eq!(updated.evidence.as_deref(), Some("Initial evidence"));

    // 2. Update status only (evidence: None) - verify evidence is NOT erased
    let status_only = store
        .subtask_update(task_id, "sub-orig", "completed", None)
        .unwrap();
    assert_eq!(status_only.status, "completed");
    assert_eq!(
        status_only.evidence.as_deref(),
        Some("Initial evidence"),
        "Existing evidence must be preserved when evidence is None"
    );

    // Also check subtask_list and inspect
    let list = store.subtask_list(task_id).unwrap();
    assert_eq!(list[0].evidence.as_deref(), Some("Initial evidence"));
    let inspect = store.inspect(task_id).unwrap();
    assert_eq!(
        inspect.spec.subtasks[0].evidence.as_deref(),
        Some("Initial evidence")
    );

    // 3. Add dynamic CLI subtask
    store
        .subtask_add(task_id, "sub-dynamic", "Dynamic subtask")
        .unwrap();
    assert_eq!(store.subtask_list(task_id).unwrap().len(), 2);
    assert_eq!(store.inspect(task_id).unwrap().spec.subtasks.len(), 2);

    // 4. Contract revision bump to v2 with a new specification
    let spec_v2 = "---\nid: m-bump\ntitle: Bump Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: feature\ntarget: Deliver feature v2\nproof_policy: seam-test-first\nscope: [src/]\ncontract_revision: 2\nsubtasks:\n  - subtask_ref: sub-new\n    title: New subtask in v2\n    status: pending\n```\n";
    let milestone_v2 = Milestone::parse(spec_v2, "forge-mcp").unwrap();
    store
        .sync(&milestone_v2, "docs/010-bump.md", &root)
        .unwrap();

    // After contract bump, old subtasks (both sub-orig and sub-dynamic) must be reset,
    // and both subtask_list and inspect must return EXACTLY the same subtasks from v2!
    let list_v2 = store.subtask_list(task_id).unwrap();
    let inspect_v2 = store.inspect(task_id).unwrap();
    assert_eq!(list_v2.len(), 1);
    assert_eq!(list_v2[0].subtask_ref, "sub-new");
    assert_eq!(inspect_v2.spec.subtasks.len(), 1);
    assert_eq!(inspect_v2.spec.subtasks[0].subtask_ref, "sub-new");
}
