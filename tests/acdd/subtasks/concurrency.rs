use super::*;

#[test]
fn concurrent_subtask_add_preserves_all_subtasks_in_sqlite_and_descriptor() {
    let spec = "---\nid: m-concurrent\ntitle: Concurrent Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: worker\ntarget: Deliver worker\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn work() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-concurrent.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-concurrent.md", root.root())
        .unwrap();
    let task_id = "forge-mcp/m-concurrent:worker";

    let db_path = root.root().join(".forge/tasks.sqlite");
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
