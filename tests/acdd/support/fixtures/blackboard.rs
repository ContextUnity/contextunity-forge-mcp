use super::*;

pub(crate) fn file_backed_blackboard_fixture() -> (crate::common::Workspace, TasksStore, Milestone)
{
    let root = crate::common::Workspace::new();
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.path(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-test.md", root.root())
        .unwrap();
    (root, store, milestone)
}

pub(crate) fn sync_blackboard_tasks(
    root: &crate::common::Workspace,
    path: &str,
    milestone: &Milestone,
) -> (TasksStore, String, String) {
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store.sync(milestone, path, root.root()).unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    (store, first, second)
}

#[test]
fn cancelled_milestone_blackboard_pruning_is_project_scoped() {
    let root = crate::common::Workspace::new();
    let database_path = root.root().join(".forge/tasks.sqlite");
    let initialized = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    drop(initialized);

    let path = "docs/milestones/040-cancelled.md";
    let primary = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    let linked = TasksStore::open_project(&database_path, "linked", "app").unwrap();
    let primary_ref = primary.milestone_scope_ref(path).unwrap();
    let linked_ref = linked.milestone_scope_ref(path).unwrap();
    assert_ne!(primary_ref, linked_ref);
    primary
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'a','notes','primary',1)",
            [&primary_ref],
        )
        .unwrap();
    primary
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'b','notes','linked',2)",
            [&linked_ref],
        )
        .unwrap();

    let mut selected = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    assert_eq!(selected.prune_cancelled(path).unwrap(), 0);
    let selected_rows: i64 = primary
        .connection
        .query_row(
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&primary_ref],
            |row| row.get(0),
        )
        .unwrap();
    let linked_rows: i64 = primary
        .connection
        .query_row(
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&linked_ref],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(selected_rows, 0);
    assert_eq!(linked_rows, 1);
}
