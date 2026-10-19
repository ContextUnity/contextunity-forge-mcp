use crate::common::Workspace;
use contextunity_forge_mcp::db::tasks_store::TasksStore;
use contextunity_forge_mcp::db::writer::{build, validate_code_map_target};

#[cfg(unix)]
#[test]
fn task_store_initialization_serializes_with_index_publication() {
    // A separate process exercises the same public opener while publication
    // holds the generation lock. Its environment is local to that child.
    if let Some(path) = std::env::var_os("FORGE_TEST_TASK_INITIALIZER") {
        std::fs::write(std::env::var_os("FORGE_TEST_TASK_READY").unwrap(), "ready").unwrap();
        let error = TasksStore::open(std::path::Path::new(&path))
            .err()
            .expect("published code index must not become a task store");
        assert!(error.to_string().contains("TASK_DATABASE_COLLISION"));
        return;
    }
    let workspace = Workspace::new();
    let directory = workspace.root();
    for alias in [false, true] {
        let target = directory.join(format!("published-{alias}.sqlite"));
        let input = if alias {
            std::fs::write(&target, []).unwrap();
            let link = directory.join("task-alias.sqlite");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            link
        } else {
            target.clone()
        };
        let root = directory.join(format!("source-{alias}"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.py"), "def published():\n    return 1\n").unwrap();
        let index = directory.join(format!("generation-{alias}.sqlite"));
        build(&root, &index, None).unwrap();
        let expected = std::fs::read(&index).unwrap();
        let mut lock_path = target.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .unwrap();
        lock.lock().unwrap();
        validate_code_map_target(&root, &target).unwrap();
        let ready = directory.join(format!("ready-{alias}"));
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "database_guard::task_store_initialization_serializes_with_index_publication",
                "--nocapture",
            ])
            .env("FORGE_TEST_TASK_INITIALIZER", &input)
            .env("FORGE_TEST_TASK_READY", &ready)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while !ready.exists() && started.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        let blocked = child.try_wait().unwrap().is_none();
        // Match atomic_build's final replacement while holding its real lock.
        std::fs::rename(&index, &target).unwrap();
        drop(lock);
        let output = child.wait_with_output().unwrap();
        assert!(ready.exists(), "initializer process did not start");
        assert!(blocked, "task initialization bypassed the publication lock");
        assert!(output.status.success(), "{output:?}");
        assert_eq!(std::fs::read(&target).unwrap(), expected);
    }
}

#[cfg(unix)]
#[test]
fn unreadable_task_database_is_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let ws = Workspace::new();
    let target = ws.root().join("custom_store.db");
    drop(TasksStore::open(&target).unwrap());
    let before = std::fs::read(&target).unwrap();
    let permissions = std::fs::metadata(&target).unwrap().permissions();
    for command in ["delta", "build", "query", "serve"] {
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o0)).unwrap();
        if std::fs::File::open(&target).is_ok() {
            std::fs::set_permissions(&target, permissions).unwrap();
            eprintln!("permission-denial fixture requires a user without DAC override");
            return;
        }
        let (rejected, diagnostic) = if command == "delta" {
            let result = contextunity_forge_mcp::db::writer::delta(
                ws.root(),
                &target,
                &[ws.root().join("app.py")],
            );
            (result.is_err(), format!("{result:?}"))
        } else {
            let arguments = if command == "query" {
                vec!["--db", target.to_str().unwrap(), "query", "overview"]
            } else {
                vec!["--db", target.to_str().unwrap(), command]
            };
            let output = crate::common::mcp_client::run_cli(&ws, &arguments);
            (
                !output.status.success(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        };
        std::fs::set_permissions(&target, permissions.clone()).unwrap();
        assert!(
            rejected && diagnostic.contains("TASK_DATABASE_COLLISION"),
            "{command}: {diagnostic}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), before, "{command}");
    }
}

#[test]
fn code_map_rebuild_recovers_readable_non_sqlite_cache() {
    let ws = Workspace::new();
    let target = ws.root().join("cache.db");
    for contents in [
        b"broken".as_slice(),
        b"readable invalid SQLite header bytes".as_slice(),
    ] {
        std::fs::write(&target, contents).unwrap();
        build(ws.root(), &target, None).unwrap();
        let connection = rusqlite::Connection::open(&target).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}

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
    assert!(
        !code_map.exists(),
        "rejected task open must not create its target"
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
