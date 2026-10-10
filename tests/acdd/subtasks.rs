use super::support::*;

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

    let root = ScopedWorkspace::new("forge_subtasks");
    root.write("src/lib.rs", "pub fn core() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-subtasks.md", spec);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();

    let task_id = "forge-mcp/forge-mcp/m-subtasks:core-feature";
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
    assert_eq!(added.status, "pending");

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

    // Invalid status rejection
    let invalid_err = store
        .subtask_update(task_id, "sub-2", "unknown_status", None)
        .unwrap_err();
    assert!(invalid_err
        .to_string()
        .contains("TASK_SUBTASK_STATUS_INVALID"));

    // Test engine manage commands
    drop(store);

    // List via engine
    let list_res = tasks::manage(
        &root.0,
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
        &root.0,
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
    assert_eq!(add_res["subtask"]["status"], "pending");

    // Update via engine
    let update_res = tasks::manage(
        &root.0,
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
        &root.0,
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
        &root.0,
        serde_json::from_value(json!({"status": "all"})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        tasks_list["tasks"][0]["subtasks"].as_array().unwrap().len(),
        3
    );

    // Repeated sync: verify that re-syncing from milestone doc does NOT overwrite live SQLite progress
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-subtasks.md", &root.0)
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

    // Deliver task through all 4 gates and prove subtasks are rendered into durable milestone markdown
    let stages = ["contract", "build", "review", "deliver"];
    for (index, stage) in stages.iter().enumerate() {
        let worker = if index >= 2 { "reviewer" } else { "builder" };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.to_string(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
        tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": serde_json::to_value(evidence(&stored)).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
    }

    // Milestone Markdown now contains durable subtasks block
    let updated_doc = std::fs::read_to_string(root.0.join("docs/010-subtasks.md")).unwrap();
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

    // Post-delivery immutability: modifying subtasks on a completed task fails with TASK_TERMINAL
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let add_post_err = store
        .subtask_add(task_id, "sub-4", "Late addition")
        .unwrap_err();
    assert!(add_post_err.to_string().contains("TASK_TERMINAL"));
    let update_post_err = store
        .subtask_update(task_id, "sub-2", "pending", None)
        .unwrap_err();
    assert!(update_post_err.to_string().contains("TASK_TERMINAL"));

    // Reopening the completed task via tasks::reset clears terminal receipt from markdown and restores ready state in SQLite
    let reopened_task = tasks::reset(&root.0, task_id).unwrap();
    assert_eq!(reopened_task.status, "ready");
    assert_eq!(reopened_task.gate, 0);
    assert!(reopened_task.receipt.is_none());
    assert!(reopened_task.completed_at.is_none());

    // Markdown file reflects the reopened state (status: completed and receipt removed, subtasks preserved)
    let reopened_doc = std::fs::read_to_string(root.0.join("docs/010-subtasks.md")).unwrap();
    let reopened_milestone = Milestone::parse(&reopened_doc, "forge-mcp").unwrap();
    assert_eq!(reopened_milestone.tasks[0].status.as_deref(), None);
    assert!(reopened_milestone.tasks[0].receipt.is_none());
    assert_eq!(reopened_milestone.tasks[0].subtasks.len(), 3);

    // After reopening, adding and updating subtasks works cleanly
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let sub4 = store
        .subtask_add(task_id, "sub-4", "Audit findings subtask")
        .unwrap();
    assert_eq!(sub4.subtask_ref, "sub-4");
    assert_eq!(sub4.status, "pending");

    let sub4_updated = store
        .subtask_update(task_id, "sub-4", "completed", Some("Audit verified"))
        .unwrap();
    assert_eq!(sub4_updated.status, "completed");
    assert_eq!(sub4_updated.evidence.as_deref(), Some("Audit verified"));

    // Syncing after reopen maintains the ready state without errors
    let synced = store
        .sync(&reopened_milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();
    assert_eq!(synced[0].status, "ready");

    // Contract re-admission: bumping contract_revision in milestone markdown syncs cleanly even if completed
    let readmitted_doc = reopened_doc.replace(
        "proof_policy: seam-test-first",
        "contract_revision: 2\nproof_policy: seam-test-first",
    );
    let readmitted_milestone = Milestone::parse(&readmitted_doc, "forge-mcp").unwrap();
    let readmitted = store
        .sync(&readmitted_milestone, "docs/010-subtasks.md", &root.0)
        .unwrap();
    assert_eq!(readmitted[0].status, "ready");
    assert_eq!(readmitted[0].contract_revision, 2);
}

#[test]
fn direct_proof_and_deferred_final_test_accept_zero_exit_code_and_seam_test_first_rejects_it() {
    let spec = "---\nid: m-policy\ntitle: Policy Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/]\n```\n```yaml\ntask_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/]\n```\n```yaml\ntask_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = ScopedWorkspace::new("forge_policy");
    root.write("src/lib.rs", "pub fn policy() {}\n");
    root.write("src/direct.rs", "pub fn direct() {}\n");
    root.write("src/deferred.rs", "pub fn deferred() {}\n");
    root.write("src/seam.rs", "pub fn seam() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = spec
        .replace(
            "task_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/]",
            "task_ref: direct\ntarget: Deliver direct\nproof_policy: direct-proof\nscope: [src/direct.rs]",
        )
        .replace(
            "task_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/]",
            "task_ref: deferred\ntarget: Deliver deferred final test\nproof_policy: deferred-final-test\nscope: [src/deferred.rs]",
        )
        .replace(
            "task_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/]",
            "task_ref: seam\ntarget: Deliver seam\nproof_policy: seam-test-first\nscope: [src/seam.rs]",
        );
    root.write("docs/010-policy.md", &spec);
    let milestone = Milestone::parse(&spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-policy.md", &root.0)
        .unwrap();

    let direct_id = "forge-mcp/forge-mcp/m-policy:direct";
    let deferred_id = "forge-mcp/forge-mcp/m-policy:deferred";
    let seam_id = "forge-mcp/forge-mcp/m-policy:seam";

    // direct-proof allows red_exit_code: 0
    store
        .claim(
            direct_id,
            "contract",
            "builder",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    let d_task = store.inspect(direct_id).unwrap();
    let mut direct_evidence = evidence(&d_task);
    direct_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/acdd/tasks.rs",
            "red_exit_code": 0
        }
    });
    let direct_submit = store.submit(direct_id, "contract", &direct_evidence, "pass", None);
    assert!(
        direct_submit.is_ok(),
        "direct-proof policy must accept exit code 0"
    );

    // deferred-final-test allows red_exit_code: 0
    store
        .claim(
            deferred_id,
            "contract",
            "builder",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    let def_task = store.inspect(deferred_id).unwrap();
    let mut def_evidence = evidence(&def_task);
    def_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/acdd/tasks.rs",
            "red_exit_code": 0
        }
    });
    let def_submit = store.submit(deferred_id, "contract", &def_evidence, "pass", None);
    assert!(
        def_submit.is_ok(),
        "deferred-final-test policy must accept exit code 0"
    );

    // seam-test-first rejects red_exit_code: 0
    store
        .claim(seam_id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    let s_task = store.inspect(seam_id).unwrap();

    // seam-test-first rejects red_exit_code: 0
    let mut seam_evidence = evidence(&s_task);
    seam_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/acdd/tasks.rs",
            "red_exit_code": 0
        }
    });
    let seam_submit_err = store
        .submit(seam_id, "contract", &seam_evidence, "pass", None)
        .unwrap_err();
    assert!(seam_submit_err
        .to_string()
        .contains("TASK_EVIDENCE_INVALID: red seam test and nonzero exit code required"));

    // seam-test-first accepts nonzero exit code
    seam_evidence.proof = json!({
        "contract_proof": {
            "seam_test_ref": "tests/acdd/tasks.rs",
            "red_exit_code": 101
        }
    });
    let seam_submit = store.submit(seam_id, "contract", &seam_evidence, "pass", None);
    assert!(
        seam_submit.is_ok(),
        "seam-test-first accepts nonzero exit code"
    );
}

#[test]
fn concurrent_subtask_add_preserves_all_subtasks_in_sqlite_and_descriptor() {
    let spec = "---\nid: m-concurrent\ntitle: Concurrent Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: worker\ntarget: Deliver worker\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = ScopedWorkspace::new("forge_concurrent_subtasks");
    root.write("src/lib.rs", "pub fn work() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-concurrent.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-concurrent.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/forge-mcp/m-concurrent:worker";

    let db_path = root.0.join(".forge/tasks.sqlite");
    let t1 = {
        let db_path = db_path.clone();
        let task_id = task_id.to_string();
        std::thread::spawn(move || {
            let mut s = TasksStore::open(&db_path).unwrap();
            for i in 0..10 {
                s.subtask_add(&task_id, &format!("thread1-sub-{}", i), "Thread 1 subtask")
                    .unwrap();
            }
        })
    };
    let t2 = {
        let db_path = db_path.clone();
        let task_id = task_id.to_string();
        std::thread::spawn(move || {
            let mut s = TasksStore::open(&db_path).unwrap();
            for i in 0..10 {
                s.subtask_add(&task_id, &format!("thread2-sub-{}", i), "Thread 2 subtask")
                    .unwrap();
            }
        })
    };
    t1.join().unwrap();
    t2.join().unwrap();

    let fresh = TasksStore::open(&db_path).unwrap();
    let list = fresh.subtask_list(task_id).unwrap();
    assert_eq!(
        list.len(),
        20,
        "All 20 subtasks from both threads must exist in subtask_list"
    );
    let inspected = fresh.inspect(task_id).unwrap();
    assert_eq!(
        inspected.spec.subtasks.len(),
        20,
        "All 20 subtasks must exist in tasks.descriptor"
    );
}

#[test]
fn subtask_update_preserves_evidence_when_none_provided_and_contract_bump_resets_subtasks() {
    let spec_v1 = "---\nid: m-bump\ntitle: Bump Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: feature\ntarget: Deliver feature\nproof_policy: seam-test-first\nscope: [src/]\ncontract_revision: 1\nsubtasks:\n  - subtask_ref: sub-orig\n    title: Original subtask\n    status: pending\n```\n";
    let root = ScopedWorkspace::new("forge_subtasks_bump");
    root.write("src/lib.rs", "pub fn bump() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-bump.md", spec_v1);
    let milestone_v1 = Milestone::parse(spec_v1, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone_v1, "docs/010-bump.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/forge-mcp/m-bump:feature";

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
    root.write("docs/010-bump.md", spec_v2);
    let milestone_v2 = Milestone::parse(spec_v2, "forge-mcp").unwrap();
    store
        .sync(&milestone_v2, "docs/010-bump.md", &root.0)
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

#[test]
fn completed_tasks_sync_and_inspect_without_panic() {
    let root = ScopedWorkspace::new("forge_completed_task_sync");
    root.write("src/lib.rs", "pub fn legacy() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-completed\ntitle: Completed Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: t1\ntarget: Deliver t1\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-completed.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-completed.md", &root.0)
        .unwrap();

    let t1_id = "forge-mcp/forge-mcp/m-completed:t1";

    // 1. Deliver t1 through the engine so the final gate validates and records its commit.
    for (i, stage) in GATES.iter().enumerate() {
        let worker = if i >= 2 { "reviewer" } else { "builder" };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: t1_id.into(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                bundle: Some(false),
            },
        )
        .unwrap();
        let t1_task = store.inspect(t1_id).unwrap();
        let submission: tasks::Submit = serde_json::from_value(json!({
            "task_id": t1_id,
            "stage": stage,
            "evidence": evidence(&t1_task),
            "action": "pass"
        }))
        .unwrap();
        tasks::submit(&root.0, submission).unwrap();
    }
    let completed = store.inspect(t1_id).unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.gate, 3);
    assert_eq!(
        completed
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(git_head(&root.0).as_str())
    );

    // 2. Handoff reconciles the delivered commit into the archived receipt before syncing it.
    let delivery_commit = git_head(&root.0);
    drop(store);
    let handoff = contextunity_forge_mcp::engine::milestones::handoff(
        &root.0,
        "m-completed",
        Some(&delivery_commit),
        "cargo test --test acdd completed_tasks_sync_and_inspect_without_panic",
        1,
        0,
    )
    .unwrap();
    let archive_path = handoff["path"].as_str().unwrap();
    let updated_doc = std::fs::read_to_string(root.0.join(archive_path)).unwrap();
    let milestone_with_receipt = Milestone::parse(&updated_doc, "forge-mcp").unwrap();
    let mut reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    reopened
        .sync(&milestone_with_receipt, archive_path, &root.0)
        .unwrap();
    let resynced = reopened.inspect(t1_id).unwrap();
    assert_eq!(resynced.status, "completed");
    assert_eq!(resynced.gate, 3);
}
