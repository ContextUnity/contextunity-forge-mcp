use super::*;

const REMAINING_TASK: LifecycleTask = LifecycleTask {
    id: "m-default",
    path: "docs/milestones/010-default.md",
    task: "remaining",
    target: "Keep remaining work",
    subtask: "preserve-progress",
};
const RETIRED_TASK: LifecycleTask = LifecycleTask {
    id: "m-cancelled",
    path: "docs/milestones/040-cancelled.md",
    task: "retired",
    target: "Retire this task",
    subtask: "retired-subtask",
};

#[test]
fn milestone_lifecycle_transitions_and_handoff_archive_ready_manifest() {
    let root = MilestoneHarness::lifecycle_workspace();
    let archived = LifecycleTask {
        id: "m-archived-default",
        path: "docs/milestones/archive/011-archived-default.md",
        task: "archived-default",
        target: "Default archived milestone",
        subtask: "archived-subtask",
    };
    for (document, expected, archive) in [
        (&REMAINING_TASK, "planned", false),
        (&archived, "completed", true),
    ] {
        root.write(document.path, &document.source(None, None, 1));
        let synced = root.sync(document.path);
        if archive {
            assert!(format!("{:#}", synced.unwrap_err()).contains("STORE_SYNC_FAILED"));
        } else {
            assert_eq!(synced.unwrap()["tasks"].as_array().unwrap().len(), 1);
        }
        assert_eq!(root.show(document.id, archive)["status"], expected);
    }

    for (id, status, path, supported) in [
        (
            "m-completed",
            "completed",
            "docs/milestones/030-completed.md",
            true,
        ),
        (
            "m-invalid-status",
            "deferred",
            "docs/milestones/001-invalid-status.md",
            false,
        ),
    ] {
        let document = LifecycleTask {
            id,
            path,
            task: status,
            target: if supported {
                "Accept a supported lifecycle status"
            } else {
                "Reject an unsupported lifecycle status"
            },
            subtask: "status-subtask",
        };
        root.write(path, &document.source(Some(status), None, 1));
        let result = root.sync(path);
        if supported {
            assert_eq!(result.unwrap()["tasks"].as_array().unwrap().len(), 1);
        } else {
            assert!(
                result.is_err(),
                "milestone sync must reject statuses outside active, planned, completed, and cancelled: {result:?}"
            );
        }
    }

    root.write(REMAINING_TASK.path, &REMAINING_TASK.source(None, None, 1));
    root.sync(REMAINING_TASK.path).unwrap();
    let remaining_id = REMAINING_TASK.task_id();
    let progress = root
        .manage(tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(remaining_id.clone()),
            subtask_ref: Some("preserve-progress".into()),
            subtask_status: Some("in_progress".into()),
            evidence: Some("verified retained work".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(progress["subtask"]["status"], "in_progress");
    let revised = LifecycleTask {
        target: "Keep remaining work after revision",
        ..REMAINING_TASK
    };
    root.write(revised.path, &revised.source(None, None, 2));
    root.sync(revised.path).unwrap();
    let retained = root
        .store()
        .subtask_list(&remaining_id)
        .unwrap()
        .into_iter()
        .find(|subtask| subtask.subtask_ref == "preserve-progress")
        .unwrap();
    assert_eq!(retained.status, "in_progress");
    assert_eq!(retained.evidence.as_deref(), Some("verified retained work"));
}

#[test]
fn cancellation_sync_validates_reason_before_pruning_task_context() {
    let root = MilestoneHarness::lifecycle_workspace();
    root.write(REMAINING_TASK.path, &REMAINING_TASK.source(None, None, 1));
    root.write(
        RETIRED_TASK.path,
        &RETIRED_TASK.source(Some("active"), None, 1),
    );
    root.sync(REMAINING_TASK.path).unwrap();
    root.sync(RETIRED_TASK.path).unwrap();
    {
        let store = root.store();
        MilestoneHarness::seed_dependency(
            &store,
            &REMAINING_TASK.task_id(),
            &RETIRED_TASK.task_id(),
        );
        MilestoneHarness::seed_dependency(
            &store,
            &RETIRED_TASK.task_id(),
            &REMAINING_TASK.task_id(),
        );
        let scope = store.milestone_scope_ref(RETIRED_TASK.path).unwrap();
        MilestoneHarness::post_scoped_message(
            &store,
            &scope,
            Some(&RETIRED_TASK.task_id()),
            "decisions",
            "task context",
        );
        MilestoneHarness::post_scoped_message(
            &store,
            &scope,
            None,
            "decisions",
            "milestone context",
        );
    }
    let store = root.store();
    let remaining_id = REMAINING_TASK.task_id();
    let cancelled_id = RETIRED_TASK.task_id();
    let scope = store.milestone_scope_ref(RETIRED_TASK.path).unwrap();

    for reason in [None, Some("   ")] {
        root.write(
            RETIRED_TASK.path,
            &RETIRED_TASK.source(Some("cancelled"), reason, 1),
        );
        let rejected = root.sync(RETIRED_TASK.path);
        assert!(
            rejected.is_err(),
            "cancelled milestone sync must reject closure.reason={reason:?}: {rejected:?}"
        );
        assert!(store.inspect(&cancelled_id).is_ok());
        assert_eq!(
            store
                .blackboard_read(&cancelled_id, None, None)
                .unwrap()
                .len(),
            1
        );
    }

    root.write(
        RETIRED_TASK.path,
        &RETIRED_TASK.source(
            Some("cancelled"),
            Some("Replaced by the remaining milestone"),
            1,
        ),
    );
    let cancelled_sync = root.sync(RETIRED_TASK.path).unwrap();
    assert!(cancelled_sync["tasks"].as_array().unwrap().is_empty());
    assert!(store.inspect(&cancelled_id).is_err());
    assert!(store
        .blackboard_read(&cancelled_id, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(
        MilestoneHarness::dependency_satisfied(&store, &remaining_id, &cancelled_id),
        0
    );
    assert_eq!(
        crate::common::sqlite::query_value::<i64>(
            &store.connection,
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&scope],
        ),
        0
    );
    assert_eq!(
        MilestoneHarness::task_rows(&store, "task_dependencies", &cancelled_id).unwrap(),
        0
    );
    let remaining = store
        .list(None, "all", None)
        .unwrap()
        .into_iter()
        .find(|task| task.task_id == remaining_id)
        .unwrap();
    assert_eq!(remaining.status, "blocked");
}
