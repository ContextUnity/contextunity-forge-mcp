use super::*;

#[test]
fn symmetric_subtask_statuses_normalize_legacy_pending_through_storage() {
    let spec = "---\nid: m-statuses\nstatus: active\n---\n```yaml\ntask_ref: statuses\ntarget: Symmetric status lifecycle\nproof_policy: seam-test-first\nscope: [src/lib.rs]\nsubtasks:\n- subtask_ref: legacy\n  title: Legacy status\n  status: pending\n```\n";
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    assert_eq!(milestone.tasks[0].subtasks[0].status, "ready");
    let mut harness = TaskHarness::memory_with_manifest(spec, "docs/statuses.md");
    let id = harness.id.clone();
    assert_eq!(
        harness
            .store
            .subtask_add(&id, "new", "New work")
            .unwrap()
            .status,
        "ready"
    );
    for (input, expected) in [
        ("ready", "ready"),
        ("in_progress", "in_progress"),
        ("completed", "completed"),
        ("cancelled", "cancelled"),
        ("blocked", "blocked"),
        ("pending", "ready"),
    ] {
        let updated = harness
            .store
            .subtask_update(&id, "legacy", input, Some("retained evidence"))
            .unwrap();
        assert_eq!(updated.status, expected, "input {input}");
        assert_eq!(harness.store.subtask_list(&id).unwrap()[0].status, expected);
        assert_eq!(
            harness.store.inspect(&id).unwrap().spec.subtasks[0].status,
            expected
        );
        let stored = harness.persisted_subtask_status("legacy").unwrap();
        assert_eq!(stored, expected);
    }
    for invalid in ["parked", "unknown", ""] {
        assert!(harness
            .store
            .subtask_update(&id, "legacy", invalid, None)
            .unwrap_err()
            .to_string()
            .contains("TASK_SUBTASK_STATUS_INVALID"));
        assert!(Milestone::parse(
            &spec.replace("status: pending", &format!("status: '{invalid}'")),
            "forge-mcp"
        )
        .is_err());
    }
    harness.set_all_subtasks_legacy_pending().unwrap();
    assert_eq!(
        harness.store.inspect(&id).unwrap().spec.subtasks[0].status,
        "ready"
    );
    assert!(harness
        .store
        .subtask_list(&id)
        .unwrap()
        .iter()
        .all(|subtask| subtask.status == "ready"));
    let root = harness.root().to_owned();
    harness
        .store
        .sync(&milestone, "docs/statuses.md", &root)
        .unwrap();
    assert!(harness
        .store
        .inspect(&id)
        .unwrap()
        .spec
        .subtasks
        .iter()
        .all(|subtask| subtask.status == "ready"));
}
#[test]
fn cancelled_tasks_retain_history_and_do_not_unblock_dependencies() {
    let root = unconfigured_test_root();
    let source = "---\nid: m-cancelled-tasks\nstatus: active\n---\n```yaml\ntask_ref: producer\ntarget: Producer\nproof_policy: seam-test-first\nscope: [src/lib.rs]\nsubtasks:\n- subtask_ref: child\n  title: Child\n```\n```yaml\ntask_ref: consumer\ntarget: Consumer\nproof_policy: seam-test-first\nscope: [src/consumer.rs]\ndepends_on: [producer]\n```\n";
    let mut milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
    store.sync(&milestone, "docs/cancelled.md", &root).unwrap();
    let claim = store
        .claim(
            &id,
            "contract",
            "builder",
            root.to_str().unwrap(),
            Some("test-session-builder"),
        )
        .unwrap();
    store
        .subtask_update(&id, "child", "blocked", Some("Waiting on input"))
        .unwrap();
    milestone.tasks[0].status = Some("cancelled".into());
    store.sync(&milestone, "docs/cancelled.md", &root).unwrap();
    let cancelled = store.inspect(&id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert!(cancelled.claim_revision > claim.claim_revision);
    assert!(cancelled.worker_id.is_none());
    assert!(cancelled.worktree.is_none());
    assert!(cancelled.receipt.is_none());
    assert_eq!(cancelled.spec.subtasks[0].status, "blocked");
    assert_eq!(
        cancelled.spec.subtasks[0].evidence.as_deref(),
        Some("Waiting on input")
    );
    assert_eq!(store.list(None, "all", None).unwrap().len(), 2);
    assert_eq!(store.list(None, "cancelled", None).unwrap().len(), 1);
    assert_eq!(store.list(None, "blocked", None).unwrap().len(), 1);
    assert!(store
        .next(
            "worker",
            root.to_str().unwrap(),
            Some("test-session-worker"),
            |_, _| Ok(())
        )
        .unwrap()
        .is_none());
    assert!(store
        .claim(
            &id,
            "contract",
            "worker",
            root.to_str().unwrap(),
            Some("test-session-worker")
        )
        .is_err());
    assert!(store
        .subtask_update(&id, "child", "ready", None)
        .unwrap_err()
        .to_string()
        .contains("TASK_TERMINAL"));
    assert!(store
        .subtask_add(&id, "extra", "Extra")
        .unwrap_err()
        .to_string()
        .contains("TASK_TERMINAL"));
    let details = store.inspect_details(&id).unwrap();
    assert_eq!(details["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(details["attempts"][0]["ended"], true);
    let revision = cancelled.claim_revision;
    store.sync(&milestone, "docs/cancelled.md", &root).unwrap();
    assert_eq!(store.inspect(&id).unwrap().claim_revision, revision);
    milestone.tasks[0].status = Some("ready".into());
    store.sync(&milestone, "docs/cancelled.md", &root).unwrap();
    assert_eq!(store.inspect(&id).unwrap().status, "cancelled");
    assert_eq!(store.reset(&id).unwrap().status, "ready");
    store.sync(&milestone, "docs/cancelled.md", &root).unwrap();
    assert_eq!(store.inspect(&id).unwrap().status, "ready");

    let mut fresh_store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
    milestone.tasks[0].status = Some("cancelled".into());
    fresh_store
        .sync(&milestone, "docs/cancelled.md", &root)
        .unwrap();
    assert_eq!(fresh_store.inspect(&id).unwrap().status, "cancelled");
    assert!(fresh_store
        .next(
            "worker",
            root.to_str().unwrap(),
            Some("test-session-worker"),
            |_, _| Ok(())
        )
        .unwrap()
        .is_none());
}
#[test]
fn completed_cancellations_re_admit_and_restore_dependency_blockers() {
    let source = "---\nid: m-cancelled-tasks\nstatus: active\n---\n```yaml\ntask_ref: producer\ntarget: Producer\nproof_policy: seam-test-first\nscope: [src/lib.rs]\nsubtasks:\n- subtask_ref: child\n  title: Child\n```\n```yaml\ntask_ref: consumer\ntarget: Consumer\nproof_policy: seam-test-first\nscope: [src/consumer.rs]\ndepends_on: [producer]\n```\n";
    let mut completed_milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let id = completed_milestone.task_id(&completed_milestone.tasks[0]);
    let workspace = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    workspace.write("src/lib.rs", "pub fn core() {}\n");
    workspace.write("src/consumer.rs", "pub fn consumer() {}\n");
    workspace.write("src/observer.rs", "pub fn observer() {}\n");
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nmilestones: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    completed_milestone.tasks[0].status = None;
    completed_milestone.tasks[0].receipt = None;
    completed_milestone.tasks[0].proof_policy = "direct-proof".into();
    completed_milestone.tasks[0].subtasks[0].status = "completed".into();
    completed_milestone.tasks[0].subtasks[0].evidence = Some("Child work verified".into());
    let original_digest = completed_milestone
        .digest(&completed_milestone.tasks[0])
        .unwrap();
    let write_manifest = |path: &str, milestone: &Milestone| {
        let tasks = milestone
            .tasks
            .iter()
            .map(|task| format!("```yaml\n{}```\n", serde_yaml::to_string(task).unwrap()))
            .collect::<String>();
        workspace.write(
            path,
            &format!("---\nid: {}\nstatus: active\n---\n{tasks}", milestone.id),
        );
    };
    write_manifest("docs/cancelled.md", &completed_milestone);
    let db_path = workspace.root().join(".forge/tasks.sqlite");
    let mut store = TasksStore::open(&db_path).unwrap();
    store
        .sync(&completed_milestone, "docs/cancelled.md", workspace.root())
        .unwrap();
    drop(store);
    {
        let mut task = TaskHarness::attach(&workspace, &id);
        task.advance_to_completion().unwrap();
    }
    let downstream = Milestone::parse(
        "---\nid: m-downstream\nstatus: active\n---\n```yaml\ntask_ref: observer\ntarget: Observer\nproof_policy: seam-test-first\nscope: [src/observer.rs]\ndepends_on: [m-cancelled-tasks:producer]\n```\n",
        "forge-mcp",
    )
    .unwrap();
    let downstream_id = downstream.task_id(&downstream.tasks[0]);
    let snapshot = FixtureSnapshot::capture(&workspace);
    assert_eq!(
        TasksStore::open(&db_path)
            .unwrap()
            .inspect(&id)
            .unwrap()
            .status,
        "completed"
    );

    for admission in ["same-digest", "higher-revision", "fresh-import"] {
        snapshot.restore(&workspace);
        let mut store = TasksStore::open(&db_path).unwrap();
        let mut milestone = completed_milestone.clone();
        milestone.tasks[0].status = Some("cancelled".into());
        write_manifest("docs/cancelled.md", &milestone);
        let workspace_root = workspace.root().to_path_buf();
        let terminal_sync_error = store
            .sync(&milestone, "docs/cancelled.md", &workspace_root)
            .unwrap_err()
            .to_string();
        assert!(terminal_sync_error.contains("TASK_TERMINAL"));
        assert_eq!(store.inspect(&id).unwrap().status, "completed");

        write_manifest("docs/downstream.md", &downstream);
        store
            .sync(&downstream, "docs/downstream.md", &workspace_root)
            .unwrap();
        assert_eq!(store.list(None, "ready", None).unwrap().len(), 2);

        if admission == "fresh-import" {
            store.delete(Some(&id), None, true, i64::MAX).unwrap();
            assert_eq!(store.list(None, "ready", None).unwrap().len(), 2);
        } else {
            store.reset(&id).unwrap();
        }
        if admission == "higher-revision" {
            milestone.tasks[0].contract_revision += 1;
        }
        let re_admission_digest = milestone.digest(&milestone.tasks[0]).unwrap();
        if admission != "higher-revision" {
            assert_eq!(
                re_admission_digest, original_digest,
                "{admission} should re-admit the completed task at its original digest"
            );
        }
        write_manifest("docs/cancelled.md", &milestone);
        store
            .sync(&milestone, "docs/cancelled.md", &workspace_root)
            .unwrap();
        store
            .sync(&downstream, "docs/downstream.md", &workspace_root)
            .unwrap();
        assert_eq!(store.list(None, "ready", None).unwrap().len(), 0);
        assert_eq!(store.list(None, "blocked", None).unwrap().len(), 2);
        assert!(store
            .claim(
                &downstream_id,
                "contract",
                "worker",
                workspace_root.to_str().unwrap(),
                Some("test-session-worker"),
            )
            .is_err());
        assert!(store
            .next(
                "worker",
                workspace_root.to_str().unwrap(),
                Some("test-session-worker"),
                |_, _| Ok(()),
            )
            .unwrap()
            .is_none());
    }
}
