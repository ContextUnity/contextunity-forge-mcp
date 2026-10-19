use crate::common::Workspace;
use contextunity_forge_mcp::db::tasks_store::TasksStore;
use contextunity_forge_mcp::db::writer::{build, validate_code_map_target};

#[test]
fn code_map_target_rejects_tasks_database_by_name_and_schema() {
    let ws = Workspace::new();
    let tasks_sqlite = ws.root().join(".forge/tasks.sqlite");

    // 1. Rejects by filename pattern even if file doesn't exist yet
    let err = validate_code_map_target(ws.root(), &tasks_sqlite).unwrap_err();
    assert!(
        err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected TASK_DATABASE_COLLISION, got: {err}"
    );

    // 2. Build also fails closed when given a tasks.sqlite output
    let build_err = build(ws.root(), &tasks_sqlite, None).unwrap_err();
    assert!(
        build_err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected TASK_DATABASE_COLLISION on build, got: {build_err}"
    );
    assert!(
        !tasks_sqlite.exists(),
        "rejected build must not create its target"
    );

    // 3. Rejects existing task database even if named arbitrarily (e.g. custom_store.db)
    let custom_task_db = ws.root().join("custom_store.db");
    {
        let conn = rusqlite::Connection::open(&custom_task_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (task_id TEXT PRIMARY KEY, milestone_ref TEXT, descriptor TEXT);
             CREATE TABLE task_gates (task_id TEXT, gate TEXT, revision INTEGER, state TEXT);",
        )
        .unwrap();
    }
    let custom_err = validate_code_map_target(ws.root(), &custom_task_db).unwrap_err();
    assert!(
        custom_err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected collision error on custom task database, got: {custom_err}"
    );
    let before = std::fs::read(&custom_task_db).unwrap();
    let err = build(ws.root(), &custom_task_db, None).unwrap_err();
    assert!(err.to_string().contains("TASK_DATABASE_COLLISION"));
    assert_eq!(std::fs::read(&custom_task_db).unwrap(), before);
}

#[test]
fn tasks_store_rejects_code_map_database_by_name_and_schema() {
    let ws = Workspace::new();
    let code_map = ws.root().join(".forge/code-map.sqlite");

    // 1. Rejects by filename pattern
    let err = match TasksStore::open(&code_map) {
        Err(e) => e,
        Ok(_) => panic!("expected TasksStore::open(code-map.sqlite) to fail"),
    };
    assert!(
        err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected TASK_DATABASE_COLLISION on TasksStore::open(code-map.sqlite), got: {err}"
    );

    // 2. Rejects existing code map database even if named arbitrarily
    let custom_code_map = ws.root().join("custom_index.db");
    {
        let conn = rusqlite::Connection::open(&custom_code_map).unwrap();
        conn.execute_batch(
            "CREATE TABLE nodes (id TEXT PRIMARY KEY, name TEXT);
             CREATE TABLE edges (src TEXT, dst TEXT);",
        )
        .unwrap();
    }
    let store_err = match TasksStore::open(&custom_code_map) {
        Err(e) => e,
        Ok(_) => panic!("expected TasksStore::open(custom_code_map) to fail"),
    };
    assert!(
        store_err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected collision error on opening code map as tasks store, got: {store_err}"
    );
}

#[test]
fn code_map_target_fails_closed_on_locked_sqlite_file() {
    let ws = Workspace::new();
    let locked_db = ws.root().join("locked_store.db");
    let conn = rusqlite::Connection::open(&locked_db).unwrap();
    conn.execute_batch("CREATE TABLE arbitrary (id INTEGER); BEGIN EXCLUSIVE;")
        .unwrap();

    // When a SQLite database is locked by another transaction, validate_code_map_target MUST fail closed
    let err = validate_code_map_target(ws.root(), &locked_db).unwrap_err();
    assert!(
        err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected fail-closed collision rejection on locked sqlite database, got: {err}"
    );
}

#[cfg(unix)]
#[test]
fn code_map_target_rejects_fifo_without_blocking() {
    let ws = Workspace::new();
    let fifo = ws.root().join("test.fifo");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());

    let err = validate_code_map_target(ws.root(), &fifo).unwrap_err();
    assert!(
        err.to_string().contains("regular file"),
        "expected regular file rejection on FIFO, got: {err}"
    );
}

#[test]
fn code_map_target_rejects_non_canonical_and_configured_tasks_db() {
    let ws = Workspace::new();
    // 1. Configure tasks_db as .forge/state.sqlite
    let config = "task_repository: repo\ntasks_db: .forge/state.sqlite\n";
    std::fs::write(ws.root().join("forge-mcp.yaml"), config).unwrap();

    // Non-canonical path referencing configured tasks_db before it exists
    let non_canonical = ws.root().join(".forge/sub/../state.sqlite");
    let err = validate_code_map_target(ws.root(), &non_canonical).unwrap_err();
    assert!(
        err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected TASK_DATABASE_COLLISION on non-canonical tasks_db path, got: {err}"
    );

    // 2. Configure tasks_db as .forge/code-map.sqlite (invalid collision with default code map)
    let bad_config = "task_repository: repo\ntasks_db: .forge/code-map.sqlite\n";
    std::fs::write(ws.root().join("forge-mcp.yaml"), bad_config).unwrap();

    let code_map = ws.root().join(".forge/code-map.sqlite");
    let err = validate_code_map_target(ws.root(), &code_map).unwrap_err();
    assert!(
        err.to_string().contains("TASK_DATABASE_COLLISION"),
        "expected TASK_DATABASE_COLLISION on colliding code-map.sqlite, got: {err}"
    );

    // Invalid task identity must not disable protection of a custom tasks_db.
    std::fs::write(
        ws.root().join("forge-mcp.yaml"),
        "task_repository: repo\ntask_project: invalid/name\ntasks_db: .forge/state.sqlite\n",
    )
    .unwrap();
    let custom = ws.root().join(".forge/state.sqlite");
    let err = build(ws.root(), &custom, None).unwrap_err();
    assert!(
        err.to_string().contains("TASK_DATABASE_COLLISION"),
        "{err:#}"
    );
    assert!(!custom.exists());
}
