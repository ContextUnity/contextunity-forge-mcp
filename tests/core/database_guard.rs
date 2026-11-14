use crate::common::Workspace;
use contextunity_forge_mcp::db::tasks_store::TasksStore;
use contextunity_forge_mcp::db::writer::{build, validate_code_map_target};

#[test]
fn stage_cleanup_and_failure_recovery() {
    use contextunity_forge_mcp::{
        db::cache,
        engine::pipeline::{build_with_test_hooks, PipelineTestHooks},
    };
    use std::sync::{Arc, Mutex};
    let ws = Workspace::new();
    std::fs::create_dir_all(ws.path("src")).unwrap();
    std::fs::write(ws.path("src/unit_00.rs"), "fn original() {}\n").unwrap();
    let db = ws.path("index.sqlite");
    let old = build(ws.root(), &db, None).unwrap();
    let before = std::fs::read(&db).unwrap();
    let receipt = ws.path("index.sqlite.verified.json");
    std::fs::write(ws.path("src/unit_00.rs"), "fn changed() {}\n").unwrap();
    let unrelated = ws.path("unrelated.txt");
    std::fs::write(&unrelated, b"preserve").unwrap();
    for case in ["publication", "consumer", "obstructed"] {
        std::fs::write(&db, &before).unwrap();
        let conn = contextunity_forge_mcp::db::reader::open(&db, ws.root()).unwrap();
        contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
        drop(conn);
        let seal = old["output_root"].as_str().unwrap();
        assert!(cache::publish_verified(&db, ws.root(), seal).unwrap());
        assert!(cache::matches(
            &db,
            ws.root(),
            seal,
            &cache::identity(&db).unwrap()
        ));
        let receipt_before = std::fs::read(&receipt).unwrap();
        let recorded = Arc::new(Mutex::new(None));
        let hook_recorded = Arc::clone(&recorded);
        let obstruct = move |stage: &std::path::Path| {
            let displaced = stage.with_extension("displaced");
            std::fs::rename(stage, &displaced).unwrap();
            std::fs::write(stage, b"obstruction").unwrap();
            *hook_recorded.lock().unwrap() = Some((stage.to_owned(), displaced));
        };
        let hooks = match case {
            "publication" => PipelineTestHooks::before_cleanup(obstruct),
            "consumer" => PipelineTestHooks::fail_consumer(),
            _ => PipelineTestHooks::fail_consumer_before_cleanup(obstruct),
        };
        let result = build_with_test_hooks(ws.root(), &db, None, hooks);
        if case == "consumer" {
            assert!(result.is_err());
            assert_eq!(std::fs::read(&db).unwrap(), before);
            assert_eq!(std::fs::read(&receipt).unwrap(), receipt_before);
            assert!(cache::matches(
                &db,
                ws.root(),
                seal,
                &cache::identity(&db).unwrap()
            ));
        } else {
            let (stage, displaced) = recorded.lock().unwrap().take().unwrap();
            if case == "publication" {
                let report = result.unwrap();
                let cleanup = report["cleanup_error"]
                    .as_str()
                    .expect("cleanup error must be reported");
                assert!(cleanup.contains(&stage.display().to_string()), "{cleanup}");
                assert_ne!(report["output_root"], old["output_root"]);
                assert_eq!(report["verification_cache"], true);
                let seal = report["output_root"].as_str().unwrap();
                assert!(cache::matches(
                    &db,
                    ws.root(),
                    seal,
                    &cache::identity(&db).unwrap()
                ));
            } else {
                let diagnostic = format!("{:#}", result.unwrap_err());
                assert!(
                    diagnostic.contains("stage cleanup also failed"),
                    "{diagnostic}"
                );
                assert!(
                    diagnostic.contains(&stage.display().to_string()),
                    "{diagnostic}"
                );
                assert!(
                    diagnostic.contains("injected pipeline SQLite consumer failure"),
                    "{diagnostic}"
                );
                assert_eq!(std::fs::read(&stage).unwrap(), b"obstruction");
                assert_eq!(std::fs::read(&db).unwrap(), before);
                assert_eq!(std::fs::read(&receipt).unwrap(), receipt_before);
                assert!(cache::matches(
                    &db,
                    ws.root(),
                    seal,
                    &cache::identity(&db).unwrap()
                ));
            }
            assert_eq!(std::fs::read(&unrelated).unwrap(), b"preserve");
            std::fs::remove_file(&stage).unwrap();
            std::fs::rename(&displaced, &stage).unwrap();
            std::fs::remove_dir_all(&stage).unwrap();
        }
        let recovered = build(ws.root(), &db, None).unwrap();
        let conn = contextunity_forge_mcp::db::reader::open(&db, ws.root()).unwrap();
        contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
        drop(conn);
        assert!(cache::matches(
            &db,
            ws.root(),
            recovered["output_root"].as_str().unwrap(),
            &cache::identity(&db).unwrap()
        ));
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"preserve");
        assert!(!std::fs::read_dir(ws.root()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".forge-stage-")));
    }
}

#[test]
fn output_sidecar_hazards_preserve_generation() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    let ws = Workspace::new();
    let db = ws.path("index.sqlite");
    build(ws.root(), &db, None).unwrap();
    let before = std::fs::read(&db).unwrap();
    for suffix in ["directory", "-wal", "-shm", "-journal"] {
        let directory = suffix == "directory";
        if !cfg!(unix) && !directory {
            continue;
        }
        let suffix = if suffix == "directory" {
            "-wal"
        } else {
            suffix
        };
        let sidecar = ws.path(format!("index.sqlite{suffix}"));

        if directory {
            std::fs::create_dir(&sidecar).unwrap();
            std::fs::write(sidecar.join("unrelated"), "keep").unwrap();
        } else {
            #[cfg(unix)]
            symlink(ws.path("missing"), &sidecar).unwrap();
        }
        let error = build(ws.root(), &db, None).unwrap_err();
        assert!(format!("{error:#}").contains(&sidecar.display().to_string()));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        if directory {
            assert_eq!(std::fs::read(sidecar.join("unrelated")).unwrap(), b"keep");
            std::fs::remove_dir_all(sidecar).unwrap();
        } else {
            std::fs::remove_file(sidecar).unwrap();
        }
    }
    build(ws.root(), &db, None).unwrap();
    assert!(!std::fs::read_dir(ws.root()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".forge-stage-")));
}

#[cfg(unix)]
#[test]
fn staged_sidecar_cleanup_reports_all_errors_and_removes_dangling_symlink() {
    use contextunity_forge_mcp::engine::pipeline::{build_with_test_hooks, PipelineTestHooks};
    use std::os::unix::fs::symlink;
    use std::sync::{Arc, Mutex};
    let ws = Workspace::new();
    let db = ws.path("index.sqlite");
    let stage_path = Arc::new(Mutex::new(None));
    let recorded = Arc::clone(&stage_path);
    let hooks = PipelineTestHooks::before_cleanup(move |stage| {
        for suffix in ["-wal", "-shm"] {
            std::fs::create_dir(stage.join(format!("generation.sqlite{suffix}"))).unwrap();
        }
        symlink(
            stage.join("missing"),
            stage.join("generation.sqlite-journal"),
        )
        .unwrap();
        *recorded.lock().unwrap() = Some(stage.to_owned());
    });
    let report = build_with_test_hooks(ws.root(), &db, None, hooks).unwrap();
    let stage = stage_path.lock().unwrap().take().unwrap();
    let error = report["cleanup_error"].as_str().unwrap();
    for suffix in ["-wal", "-shm"] {
        assert!(
            error.contains(
                &stage
                    .join(format!("generation.sqlite{suffix}"))
                    .display()
                    .to_string()
            ),
            "{error}"
        );
    }
    assert!(!error.contains("generation.sqlite-journal"), "{error}");
    assert!(!stage.exists());
}

#[test]
fn checkpoint_rejects_active_reader_and_succeeds_after_release() {
    let ws = Workspace::new();
    let path = ws.path("checkpoint.sqlite");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode=WAL; CREATE TABLE t (v INTEGER); INSERT INTO t VALUES (1);",
        )
        .unwrap();
    let reader = rusqlite::Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN; SELECT * FROM t;").unwrap();
    writer.execute_batch("INSERT INTO t VALUES (2);").unwrap();
    for truncate in [false, true] {
        let error = contextunity_forge_mcp::db::cache::checkpoint(&writer, truncate).unwrap_err();
        assert!(error.to_string().contains("incomplete"), "{error:#}");
    }
    reader.execute_batch("ROLLBACK").unwrap();
    drop(reader);
    contextunity_forge_mcp::db::cache::checkpoint(&writer, true).unwrap();
}

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
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let real_repo = ws.path("private_var/repo");
        std::fs::create_dir_all(&real_repo).unwrap();
        std::fs::write(
            real_repo.join("forge-mcp.yaml"),
            "task_repository: repo\ntasks_db: .forge/state.sqlite\n",
        )
        .unwrap();
        let alias_repo = ws.path("var_repo");
        symlink(&real_repo, &alias_repo).unwrap();

        let canonical_root = real_repo.canonicalize().unwrap();
        let alias_target = alias_repo.join(".forge/state.sqlite");
        let err = validate_code_map_target(&canonical_root, &alias_target).unwrap_err();
        assert!(
            err.to_string().contains("TASK_DATABASE_COLLISION"),
            "expected TASK_DATABASE_COLLISION across canonical/alias roots, got: {err:#}"
        );
        assert!(!real_repo.join(".forge/state.sqlite").exists());
    }
}
