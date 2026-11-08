use super::*;

#[test]
fn task_subtasks_lifecycle_management_and_digest_independence() {
    let spec = "---\nid: m-subtasks\ntitle: Subtasks Test\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: core-feature\ntarget: Deliver core feature\nproof_policy: seam-test-first\nscope: [src/]\nsubtasks:\n  - subtask_ref: sub-1\n    title: Initial discovery\n    status: in_progress\n    evidence: Found relevant files\n```\n";
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let task = &milestone.tasks[0];
    assert_eq!(task.subtasks.len(), 1);
    assert_eq!(task.subtasks[0].subtask_ref, "sub-1");
    assert_eq!(task.subtasks[0].title, "Initial discovery");
    assert_eq!(task.subtasks[0].status, "in_progress");
    assert_eq!(
        task.subtasks[0].evidence.as_deref(),
        Some("Found relevant files")
    );

    // Digest independence: subtask updates or additions do not invalidate the parent task's contract revision or SHA256 digest
    let base_digest = milestone.digest(task).unwrap();
    let mut modified_task = task.clone();
    modified_task
        .subtasks
        .push(contextunity_forge_mcp::core::tasks::SubtaskSpec {
            subtask_ref: "sub-2".into(),
            title: "Additional spike".into(),
            status: "completed".into(),
            evidence: Some("Tests verified".into()),
        });
    modified_task.subtasks[0].status = "completed".into();
    let modified_digest = milestone.digest(&modified_task).unwrap();
    assert_eq!(
        base_digest, modified_digest,
        "Subtasks must not alter contract digest"
    );

    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn core() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-subtasks.md", spec);

    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-subtasks.md", root.root())
        .unwrap();

    let task_id = "forge-mcp/m-subtasks:core-feature";
    let loaded = store.inspect(task_id).unwrap();
    assert_eq!(loaded.spec.subtasks.len(), 1);
    assert_eq!(loaded.spec.subtasks[0].subtask_ref, "sub-1");

    let subtasks_table = store.subtask_list(task_id).unwrap();
    assert_eq!(subtasks_table.len(), 1);
    assert_eq!(subtasks_table[0].subtask_ref, "sub-1");
    assert_eq!(subtasks_table[0].status, "in_progress");

    // Add subtask via store
    let added = store
        .subtask_add(task_id, "sub-2", "Implement edge case")
        .unwrap();
    assert_eq!(added.subtask_ref, "sub-2");
    assert_eq!(added.status, "ready");

    // Duplicate subtask ref rejection
    let dup_err = store
        .subtask_add(task_id, "sub-2", "Duplicate")
        .unwrap_err();
    assert!(dup_err.to_string().contains("TASK_SUBTASK_DUPLICATE"));

    // Update subtask
    let updated = store
        .subtask_update(task_id, "sub-2", "completed", Some("Added unit test"))
        .unwrap();
    assert_eq!(updated.status, "completed");
    assert_eq!(updated.evidence.as_deref(), Some("Added unit test"));

    // Test engine manage commands
    drop(store);

    // List via engine
    let list_res = tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::SubtaskList,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let subtask_list = list_res["subtasks"].as_array().unwrap();
    assert_eq!(subtask_list.len(), 2);

    // Add via engine
    let add_res = tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::SubtaskAdd,
            task_id: Some(task_id.into()),
            subtask_ref: Some("sub-3".into()),
            title: Some("Documentation update".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(add_res["subtask"]["subtask_ref"], "sub-3");
    assert_eq!(add_res["subtask"]["status"], "ready");

    // Update via engine
    let update_res = tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(task_id.into()),
            subtask_ref: Some("sub-3".into()),
            subtask_status: Some("in_progress".into()),
            evidence: Some("Drafted changes".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(update_res["subtask"]["status"], "in_progress");
    assert_eq!(update_res["subtask"]["evidence"], "Drafted changes");

    // Inspect task includes subtasks
    let inspected = tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::Inspect,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(inspected["subtasks"].as_array().unwrap().len(), 3);

    // List tasks includes subtasks
    let tasks_list = tasks::list(
        root.root(),
        serde_json::from_value(json!({"status": "all"})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        tasks_list["tasks"][0]["subtasks"].as_array().unwrap().len(),
        3
    );

    // Repeated sync: verify that re-syncing from milestone doc does NOT overwrite live SQLite progress
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-subtasks.md", root.root())
        .unwrap();
    let re_inspected = store.inspect(task_id).unwrap();
    assert_eq!(re_inspected.spec.subtasks.len(), 3);
    let sub2 = re_inspected
        .spec
        .subtasks
        .iter()
        .find(|s| s.subtask_ref == "sub-2")
        .unwrap();
    assert_eq!(sub2.status, "completed");
    assert_eq!(sub2.evidence.as_deref(), Some("Added unit test"));
    let sub3 = re_inspected
        .spec
        .subtasks
        .iter()
        .find(|s| s.subtask_ref == "sub-3")
        .unwrap();
    assert_eq!(sub3.status, "in_progress");
    drop(store);

    // Deliver the task and prove subtasks are rendered into durable milestone markdown.
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .subtask_update(
            task_id,
            "sub-1",
            "completed",
            Some("Existing discovery verified"),
        )
        .unwrap();
    store
        .subtask_update(
            task_id,
            "sub-3",
            "completed",
            Some("Documentation verified"),
        )
        .unwrap();
    drop(store);

    TaskHarness::attach(&root, task_id)
        .advance_to_completion()
        .unwrap();

    // Milestone Markdown now contains durable subtasks block
    let updated_doc = std::fs::read_to_string(root.root().join("docs/010-subtasks.md")).unwrap();
    let delivered_milestone = Milestone::parse(&updated_doc, "forge-mcp").unwrap();
    let delivered_task = &delivered_milestone.tasks[0];
    assert_eq!(delivered_task.status.as_deref(), Some("completed"));
    assert_eq!(delivered_task.subtasks.len(), 3);
    assert_eq!(delivered_task.subtasks[1].subtask_ref, "sub-2");
    assert_eq!(delivered_task.subtasks[1].status, "completed");
    assert_eq!(
        delivered_task.subtasks[1].evidence.as_deref(),
        Some("Added unit test")
    );
    assert!(delivered_task
        .subtasks
        .iter()
        .all(|subtask| matches!(subtask.status.as_str(), "completed" | "cancelled")));
    assert!(delivered_task.subtasks.iter().all(|subtask| {
        subtask.status != "completed"
            || subtask
                .evidence
                .as_deref()
                .is_some_and(|evidence| !evidence.trim().is_empty())
    }));

    // Post-delivery immutability: modifying subtasks on a completed task fails with TASK_TERMINAL
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    let add_post_err = store
        .subtask_add(task_id, "sub-4", "Late addition")
        .unwrap_err();
    assert!(add_post_err.to_string().contains("TASK_TERMINAL"));
    let update_post_err = store
        .subtask_update(task_id, "sub-2", "pending", None)
        .unwrap_err();
    assert!(update_post_err.to_string().contains("TASK_TERMINAL"));

    // Reopening the completed task via tasks::reset clears terminal receipt from markdown and restores ready state in SQLite
    let reopened_task = tasks::reset(root.root(), task_id).unwrap();
    assert_eq!(reopened_task.status, "ready");
    assert_eq!(reopened_task.gate, 0);
    assert!(reopened_task.receipt.is_none());
    assert!(reopened_task.completed_at.is_none());

    // Markdown file reflects the reopened state (status: completed and receipt removed, subtasks preserved)
    let reopened_doc = std::fs::read_to_string(root.root().join("docs/010-subtasks.md")).unwrap();
    let reopened_milestone = Milestone::parse(&reopened_doc, "forge-mcp").unwrap();
    assert_eq!(reopened_milestone.tasks[0].status.as_deref(), None);
    assert!(reopened_milestone.tasks[0].receipt.is_none());
    assert_eq!(reopened_milestone.tasks[0].subtasks.len(), 3);

    // After reopening, adding and updating subtasks works cleanly
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    let sub4 = store
        .subtask_add(task_id, "sub-4", "Audit findings subtask")
        .unwrap();
    assert_eq!(sub4.subtask_ref, "sub-4");
    assert_eq!(sub4.status, "ready");

    let sub4_updated = store
        .subtask_update(task_id, "sub-4", "completed", Some("Audit verified"))
        .unwrap();
    assert_eq!(sub4_updated.status, "completed");
    assert_eq!(sub4_updated.evidence.as_deref(), Some("Audit verified"));

    // Syncing after reopen maintains the ready state without errors
    let synced = store
        .sync(&reopened_milestone, "docs/010-subtasks.md", root.root())
        .unwrap();
    assert_eq!(synced[0].status, "ready");

    // Contract re-admission: bumping contract_revision in milestone markdown syncs cleanly even if completed
    let readmitted_doc = reopened_doc.replace(
        "proof_policy: seam-test-first",
        "contract_revision: 2\nproof_policy: seam-test-first",
    );
    let readmitted_milestone = Milestone::parse(&readmitted_doc, "forge-mcp").unwrap();
    let readmitted = store
        .sync(&readmitted_milestone, "docs/010-subtasks.md", root.root())
        .unwrap();
    assert_eq!(readmitted[0].status, "ready");
    assert_eq!(readmitted[0].contract_revision, 2);
}
