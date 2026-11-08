use super::*;

#[test]
fn completed_tasks_sync_and_inspect_without_panic() {
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn legacy() {}\n");
    // This seam inspects retained SQLite status after archived-receipt resync.
    // Default pruning is covered by the milestone handoff seam.
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nhandoff_pruning: false\n",
    );
    let spec = "---\nid: m-completed\ntitle: Completed Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: t1\ntarget: Deliver t1\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-completed.md", spec);
    commit_handoff_fixture_inputs(&root, &["forge-mcp.yaml", "docs", "src"]);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-completed.md", root.root())
        .unwrap();

    let t1_id = "forge-mcp/m-completed:t1";

    // Deliver through the active profile so the final gate records its commit.
    let mut harness = TaskHarness::attach(&root, t1_id);
    let expected_final_gate = harness.executable_gates().unwrap().len() - 1;
    let completed = harness.advance_to_completion().unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.gate, expected_final_gate);
    assert_eq!(
        completed
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(git_head(root.root()).as_str())
    );

    // 2. Handoff reconciles the delivered commit into the archived receipt before syncing it.
    let delivery_commit = git_head(root.root());
    drop(store);
    let handoff = handoff_with_fixture_evidence(
        root.root(),
        "m-completed",
        Some(&delivery_commit),
        "cargo test --test acdd completed_tasks_sync_and_inspect_without_panic",
        1,
        0,
    )
    .unwrap();
    let archive_path = handoff["path"].as_str().unwrap();
    let updated_doc = std::fs::read_to_string(root.root().join(archive_path)).unwrap();
    let milestone_with_receipt = Milestone::parse(&updated_doc, "forge-mcp").unwrap();
    assert!(completed
        .receipt
        .as_ref()
        .unwrap()
        .matches_proofs(milestone_with_receipt.tasks[0].receipt.as_ref().unwrap()));
    let mut reopened = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(reopened.inspect(t1_id).unwrap().milestone_ref, archive_path);
    assert!(!reopened.milestone_tombstoned("m-completed").unwrap());
    reopened
        .sync(&milestone_with_receipt, archive_path, root.root())
        .unwrap();
    let resynced = reopened.inspect(t1_id).unwrap();
    assert_eq!(resynced.status, "completed");
    assert_eq!(resynced.gate, expected_final_gate);
}
