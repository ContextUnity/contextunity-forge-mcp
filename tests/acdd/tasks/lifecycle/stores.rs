use super::*;

#[test]
fn force_deletion_preserves_project_boundaries_and_revision_fencing() {
    let mut h = TaskHarness::memory();
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let root = h.root().to_owned();
    let id = h.id.clone();
    let store = &mut h.store;
    let first = store
        .claim(
            &id,
            "contract",
            "worker",
            root.as_path().to_str().unwrap(),
            Some(&TaskHarness::session_for("worker")),
        )
        .unwrap();
    assert!(store
        .delete(None, Some("docs/010-test.md"), false, 0)
        .is_err());
    assert_eq!(store.list(None, "all", None).unwrap().len(), 2);
    let deleted = store.delete(Some(&id), None, true, 0).unwrap();
    assert_eq!(deleted.revoked_claims, 1);
    assert_eq!(deleted.count, 1);
    assert!(store.list(None, "ready", None).unwrap().is_empty());
    let recreated = store
        .create(&milestone, "docs/010-test.md", root.as_path(), "first")
        .unwrap();
    assert!(recreated[0].claim_revision > first.claim_revision);
}

#[test]
fn dedicated_store_rejects_index_and_unknown_schema_without_ddl() {
    let root = TaskHarness::recovery_workspace();
    root.write("src/lib.rs", "pub fn sample() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: []\ntasks_db: .forge/tasks.sqlite\n",
    );
    let index = root.root().join("index.sqlite");
    root.build_to(&index);
    assert!(TasksStore::open(&index).is_err());
    let connection = rusqlite::Connection::open(&index).unwrap();
    let task_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type=?1 AND name=?2",
            rusqlite::params!["table", "tasks"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(task_tables, 0);

    for version in ["1", "2", "3", "4", "99"] {
        let old = root.root().join(format!("schema-{version}.sqlite"));
        let connection = rusqlite::Connection::open(&old).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE task_store_metadata(key TEXT PRIMARY KEY,value TEXT);
                 CREATE TABLE sentinel(value TEXT NOT NULL);",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_store_metadata VALUES('schema_version',?1)",
                [version],
            )
            .unwrap();
        connection
            .execute("INSERT INTO sentinel VALUES('preserve me')", [])
            .unwrap();
        assert!(
            TasksStore::open(&old).is_err(),
            "schema {version} must fail closed"
        );
        let connection = rusqlite::Connection::open(&old).unwrap();
        let stored_version: String = connection
            .query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let sentinel: String = connection
            .query_row("SELECT value FROM sentinel", [], |row| row.get(0))
            .unwrap();
        let mut statement = connection
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let tables = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        assert_eq!(stored_version, version, "schema marker {version} changed");
        assert_eq!(sentinel, "preserve me");
        assert_eq!(tables, ["sentinel", "task_store_metadata"]);

        root.write(
            "forge-mcp.yaml",
            &format!("tasks_db: schema-{version}.sqlite\n"),
        );
        let wal = std::path::PathBuf::from(format!("{}-wal", old.display()));
        let shm = std::path::PathBuf::from(format!("{}-shm", old.display()));
        std::fs::write(&wal, b"preserve refused WAL bytes").unwrap();
        std::fs::write(&shm, b"preserve refused SHM bytes").unwrap();
        let before = std::fs::read(&old).unwrap();
        let refused = TaskHarness::run_cli_for_root(root.root(), &["milestone", "db-recover"]);
        assert!(
            !refused.status.success(),
            "unsupported schema {version} must not be recovered"
        );
        assert_eq!(std::fs::read(&old).unwrap(), before);
        assert_eq!(std::fs::read(wal).unwrap(), b"preserve refused WAL bytes");
        assert_eq!(std::fs::read(shm).unwrap(), b"preserve refused SHM bytes");
    }

    for (file_name, schema, object_type, object_name, preserved_value) in [
        (
            "unversioned.sqlite",
            "CREATE TABLE old_data(value TEXT); INSERT INTO old_data VALUES('preserve me');",
            "table",
            "old_data",
            Some("preserve me"),
        ),
        (
            "unversioned-view.sqlite",
            "CREATE VIEW old_projection AS SELECT 1 AS value;",
            "view",
            "old_projection",
            None,
        ),
    ] {
        let database = root.root().join(file_name);
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute_batch(schema)
            .unwrap();
        assert!(TasksStore::open(&database).is_err(), "{file_name}");

        let connection = rusqlite::Connection::open(&database).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT type,name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
            )
            .unwrap();
        let objects = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<(String, String)>>>()
            .unwrap();
        assert_eq!(
            objects,
            vec![(object_type.to_owned(), object_name.to_owned())],
            "{file_name}"
        );
        if let Some(expected) = preserved_value {
            let stored: String = connection
                .query_row("SELECT value FROM old_data", [], |row| row.get(0))
                .unwrap();
            assert_eq!(stored, expected, "{file_name}");
        }
    }
}

#[test]
fn task_store_v7_schema_rejects_stale_versions_fail_closed_and_allows_reader_during_write() {
    let root = TaskHarness::recovery_workspace();
    let current = root.root().join("current.sqlite");
    drop(TasksStore::open(&current).expect("empty DB should initialize as schema v7"));
    let reopened = TasksStore::open(&current).expect("schema v7 should remain supported");
    let schema_version: String = reopened
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, "7");
    reopened
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .unwrap();
    drop(TasksStore::open(&current).expect("v7 open must not acquire the writer lock"));
    reopened.connection.execute_batch("ROLLBACK").unwrap();
    drop(reopened);

    for stale_version in ["5", "6"] {
        let conn = rusqlite::Connection::open(&current).unwrap();
        conn.execute(
            "UPDATE task_store_metadata SET value=?1 WHERE key='schema_version'",
            [stale_version],
        )
        .unwrap();
        drop(conn);
        let err = match TasksStore::open(&current) {
            Err(err) => err.to_string(),
            Ok(_) => panic!("stale schema version {stale_version} must fail closed"),
        };
        assert!(
            err.contains("unsupported tasks schema; use recovery tooling"),
            "unexpected error for version {stale_version}: {err}"
        );
    }
}

#[test]
fn task_store_incompatible_trace_schema_fails_closed_without_dropping_tables() {
    let root = TaskHarness::recovery_workspace();
    let database = root.root().join("legacy-trace.sqlite");
    drop(TasksStore::open(&database).expect("fresh DB initializes at schema v7"));

    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "DROP TABLE command_execution_proofs;
             DROP TABLE gate_execution_traces;
             CREATE TABLE gate_execution_traces (id INTEGER PRIMARY KEY);
             CREATE TABLE command_execution_proofs (id INTEGER PRIMARY KEY);
             UPDATE task_store_metadata SET value = '6' WHERE key = 'schema_version';",
        )
        .unwrap();
    drop(connection);

    let err = match TasksStore::open(&database) {
        Err(err) => err.to_string(),
        Ok(_) => panic!("expected fail-closed schema error"),
    };
    assert!(
        err.contains("unsupported tasks schema; use recovery tooling"),
        "expected fail-closed schema error, got: {err}"
    );

    let reopened = rusqlite::Connection::open(&database).unwrap();
    for table in ["command_execution_proofs", "gate_execution_traces"] {
        let exists: bool = reopened
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            exists,
            "{table} must not be dropped when schema migration fails"
        );
    }
}

#[test]
fn completed_task_reopen_and_mcp_manage_action_lifecycle() {
    let mut h = TaskHarness::disk();
    h.advance_to_completion().unwrap();
    let id = h.id.clone();
    let task_id = id.as_str();
    let root = h.root().to_owned();
    let store = &mut h.store;
    let completed_task = store.inspect(task_id).unwrap();
    assert_eq!(completed_task.status, "completed");
    assert!(completed_task.completed_at.is_some());
    assert!(completed_task.receipt.is_some());
    let milestone_path = root.join("docs/010-test.md");
    let completed_text = std::fs::read_to_string(&milestone_path).unwrap();
    let add_err = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap_err();
    assert!(add_err.to_string().contains("TASK_TERMINAL"));
    assert!(add_err.to_string().contains("reopen or reset"));
    let held_path = root.join("docs/010-test-held.md");
    std::fs::rename(&milestone_path, &held_path).unwrap();
    assert!(tasks::reopen(&root, task_id).is_err());
    assert_eq!(store.inspect(task_id).unwrap().status, "completed");
    std::fs::rename(&held_path, &milestone_path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        completed_text
    );
    let reopen_res = TaskHarness::manage_task(&root, task_id, tasks::ManageAction::Reopen).unwrap();
    assert_eq!(reopen_res["status"], "ready");
    assert_eq!(reopen_res["stage"], "contract");
    assert!(reopen_res["receipt"].is_null());
    assert!(reopen_res["completed_at"].is_null());
    let reopened_text = std::fs::read_to_string(&milestone_path).unwrap();
    assert!(!reopened_text.contains("status: completed"));
    assert!(!reopened_text.contains("receipt:"));
    let sub = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap();
    assert_eq!(sub.subtask_ref, "sub-audit");
    assert_eq!(sub.status, "ready");
    let re_claimed = TaskHarness::claim_at(&root, task_id, "contract", "auditor").unwrap();
    assert_eq!(re_claimed["status"], "in_progress");
    let reset_res = TaskHarness::manage_task(&root, task_id, tasks::ManageAction::Reset).unwrap();
    assert_eq!(reset_res["status"], "ready");
}
