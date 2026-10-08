use super::support::*;

#[test]
fn blackboard_persists_task_scoped_messages_in_chronological_order() {
    let (root, store, milestone) = fixture();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let first_id = store
        .blackboard_post(&first, "author-a", "contract_draft", "red test")
        .unwrap();
    let second_id = store
        .blackboard_post(&first, "author-b", "architectural_notes", "use WAL")
        .unwrap();
    store
        .blackboard_post(&second, "author-c", "contract_draft", "other task")
        .unwrap();
    assert!(second_id > first_id);
    let all = store.blackboard_read(&first, None, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, first_id);
    assert_eq!(all[0].author, "author-a");
    assert_eq!(all[1].id, second_id);
    assert!(all[0].created_at <= all[1].created_at);
    assert_eq!(
        store
            .blackboard_read(&first, Some("architectural_notes"), Some(1))
            .unwrap()[0]
            .payload
            .as_deref(),
        Some("use WAL")
    );
    assert_eq!(store.blackboard_clear(&first).unwrap(), 2);
    assert!(store
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 1);
    let index: String = store.connection.query_row(
        "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='task_blackboard' AND sql LIKE '%task_id, created_at%'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(index, "idx_task_blackboard_task_created");
    drop(root);
}
#[test]
fn blackboard_schema_v1_upgrade_keeps_task_store_operational() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    store
        .connection
        .execute_batch(
            "DROP TABLE task_blackboard;
             CREATE TABLE task_blackboard(
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,
                 author TEXT NOT NULL,
                 topic TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             INSERT INTO task_blackboard(id,task_id,author,topic,payload,created_at)
             VALUES(73,'forge-mcp/forge-mcp/m-test:first','legacy-worker','build_proof','preserved payload',1234);
             UPDATE task_store_metadata SET value='1' WHERE key='schema_version';",
        )
        .unwrap();
    drop(store);

    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(reopened.inspect(&task_id).unwrap().status, "ready");
    let schema_version: String = reopened
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, "2");
    let task_id_nullable: i64 = reopened
        .connection
        .query_row(
            "SELECT \"notnull\" FROM pragma_table_info('task_blackboard') WHERE name='task_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(task_id_nullable, 0);
}

#[test]
fn blackboard_scope_resolution_isolates_milestone_task_and_subtask_messages() {
    let root = ScopedWorkspace::new("forge_blackboard_hierarchy");
    let manifest = "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\nstatus: active\n---\n# Blackboard\n```yaml\ntask_ref: first\ntarget: Coordinate task agents\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Coordinate another task\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    root.write("docs/milestones/010-blackboard.md", manifest);
    let milestone = Milestone::parse(manifest, "forge-mcp").unwrap();
    let database = root.0.join(".forge/tasks.sqlite");
    let mut store = TasksStore::open(&database).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-blackboard.md", &root.0)
        .unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    store
        .claim(&first, "contract", "worker-one", root.0.to_str().unwrap())
        .unwrap();
    store
        .subtask_add(&first, "active-slice", "Resolve the active subtask")
        .unwrap();
    store
        .subtask_update(&first, "active-slice", "in_progress", None)
        .unwrap();

    let call = |args| tasks::blackboard(&root.0, serde_json::from_value(args).unwrap(), "test");
    let milestone_post = call(json!({
        "action":"post","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md",
        "topic":"architecture","payload":"milestone coordination"
    }))
    .unwrap();
    let milestone_message_id = milestone_post["id"].as_u64().unwrap();
    let task_post = call(json!({
        "action":"post","task_id":first,
        "topic":"build_proof","payload":"task coordination"
    }))
    .unwrap();
    let task_message_id = task_post["id"].as_u64().unwrap();
    let subtask_post = call(json!({
        "action":"post","scope":"subtask",
        "topic":"architectural_seam","payload":"subtask coordination"
    }))
    .unwrap();
    let subtask_message_id = subtask_post["id"].as_u64().unwrap();

    let milestone_page = call(json!({
        "action":"read","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md"
    }))
    .unwrap();
    assert_eq!(milestone_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(milestone_page["messages"][0]["id"], milestone_message_id);
    assert_eq!(milestone_page["messages"][0]["task_id"], Value::Null);
    assert!(milestone_page["messages"][0].get("payload").is_none());

    let task_page = call(json!({"action":"read","limit":100})).unwrap();
    assert_eq!(task_page["pagination"]["limit"], 50);
    assert_eq!(task_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(task_page["messages"][0]["id"], task_message_id);
    assert_eq!(task_page["messages"][0]["task_id"], first);
    assert_eq!(task_page["messages"][0]["subtask_ref"], Value::Null);
    assert!(task_page["messages"][0].get("payload").is_none());

    let subtask_page = call(json!({"action":"read","scope":"subtask"})).unwrap();
    assert_eq!(subtask_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(subtask_page["messages"][0]["id"], subtask_message_id);
    assert_eq!(subtask_page["messages"][0]["task_id"], first);
    assert_eq!(subtask_page["messages"][0]["subtask_ref"], "active-slice");

    let inspected = call(json!({"action":"inspect","message_id":subtask_message_id})).unwrap();
    assert_eq!(inspected["message"]["payload"], "subtask coordination");

    store
        .claim(&second, "contract", "worker-two", root.0.to_str().unwrap())
        .unwrap();
    let ambiguous = call(json!({"action":"read","scope":"task"})).unwrap_err();
    assert!(ambiguous
        .to_string()
        .contains("TASK_BLACKBOARD_AMBIGUOUS_TASK"));
    let still_isolated = call(json!({
        "action":"read","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md"
    }))
    .unwrap();
    assert_eq!(still_isolated["messages"][0]["id"], milestone_message_id);
}

#[test]
fn blackboard_automatic_scope_fails_closed_without_one_active_context() {
    for (name, manifests, expected) in [
        (
            "forge_blackboard_no_active_context",
            vec![("010-planned.md", "planned")],
            "TASK_BLACKBOARD_NO_ACTIVE_CONTEXT",
        ),
        (
            "forge_blackboard_ambiguous_milestones",
            vec![("010-first.md", "active"), ("020-second.md", "active")],
            "TASK_BLACKBOARD_AMBIGUOUS_MILESTONE",
        ),
    ] {
        let root = ScopedWorkspace::new(name);
        root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
        for (filename, status) in manifests {
            root.write(
                &format!("docs/milestones/{filename}"),
                &format!("---\nid: m-{filename}\ntitle: {filename}\ndoc_type: contract\nstatus: {status}\n---\n"),
            );
        }
        let request: tasks::BlackboardRequest =
            serde_json::from_value(json!({"action":"read"})).unwrap();
        let error = tasks::blackboard(&root.0, request, "test").unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn blackboard_concurrent_connections_persist_and_cascade() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let database = root.0.join(".forge/tasks.sqlite");
    let workers: Vec<_> = (0..2)
        .map(|worker| {
            let database = database.clone();
            let task_id = task_id.clone();
            std::thread::spawn(move || {
                let connection = TasksStore::open(&database).unwrap();
                for index in 0..20 {
                    connection
                        .blackboard_post(
                            &task_id,
                            &format!("worker-{worker}"),
                            "build_proof",
                            &index.to_string(),
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    drop(store);
    let reopened = TasksStore::open(&database).unwrap();
    assert_eq!(
        reopened
            .blackboard_read(&task_id, None, None)
            .unwrap()
            .len(),
        40
    );
    reopened
        .connection
        .execute("DELETE FROM tasks WHERE task_id=?1", [&task_id])
        .unwrap();
    assert!(reopened
        .blackboard_read(&task_id, None, None)
        .unwrap()
        .is_empty());
}

static BLACKBOARD_BUSY_SIGNAL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> =
    std::sync::OnceLock::new();
static BLACKBOARD_BUSY_NOTIFIED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn signal_blackboard_write_contention(_: i32) -> bool {
    if !BLACKBOARD_BUSY_NOTIFIED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        if let Some(signal) = BLACKBOARD_BUSY_SIGNAL.get() {
            let _ = signal.send(());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(1));
    true
}

#[test]
fn blackboard_post_fails_closed_when_task_completes_during_write() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let task = store.inspect(&task_id).unwrap();
    let milestone_ref = store.milestone_scope_ref(&task.milestone_ref).unwrap();
    drop(store);

    let database = root.0.join(".forge/tasks.sqlite");
    let mut completion = TasksStore::open(&database).unwrap();
    let poster = TasksStore::open(&database).unwrap();
    let (busy_tx, busy_rx) = std::sync::mpsc::channel();
    BLACKBOARD_BUSY_NOTIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(BLACKBOARD_BUSY_SIGNAL.set(busy_tx).is_ok());
    poster
        .connection
        .busy_handler(Some(signal_blackboard_write_contention))
        .unwrap();

    let transition = completion
        .connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transition
        .execute(
            "UPDATE tasks SET descriptor=json_set(descriptor,'$.status','completed') WHERE task_id=?1",
            [&task_id],
        )
        .unwrap();
    let post = std::thread::spawn(move || {
        poster.blackboard_post_scoped(
            &milestone_ref,
            Some(&task_id),
            None,
            "worker",
            "build_proof",
            "post races task completion",
        )
    });

    busy_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("post reached SQLite write contention");
    transition.commit().unwrap();
    let error = post.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("TASK_TERMINAL"), "{error}");
}

#[test]
fn cancelled_milestone_blackboard_pruning_is_project_scoped() {
    let root = ScopedWorkspace::new("forge_cancelled_milestone_project_isolation");
    let database_path = root.0.join(".forge/tasks.sqlite");
    let initialized = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    drop(initialized);
    let db = rusqlite::Connection::open(&database_path).unwrap();
    db.execute_batch(
        "ALTER TABLE task_blackboard RENAME TO task_blackboard_tasks;
         CREATE TABLE task_blackboard(
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             milestone_ref TEXT NOT NULL,
             task_id TEXT REFERENCES tasks ON DELETE CASCADE,
             subtask_ref TEXT,
             author TEXT NOT NULL,
             topic TEXT NOT NULL,
             payload TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         DROP TABLE task_blackboard_tasks;",
    )
    .unwrap();
    drop(db);

    let path = "docs/milestones/040-cancelled.md";
    let primary = TasksStore::open_project(&database_path, "primary", "app").unwrap();
    let linked = TasksStore::open_project(&database_path, "linked", "app").unwrap();
    let primary_ref = primary.milestone_scope_ref(path).unwrap();
    let linked_ref = linked.milestone_scope_ref(path).unwrap();
    assert_ne!(primary_ref, linked_ref);
    primary
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'a','note','primary',1)",
            [&primary_ref],
        )
        .unwrap();
    primary
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref,author,topic,payload,created_at) VALUES(?1,'b','note','linked',2)",
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
