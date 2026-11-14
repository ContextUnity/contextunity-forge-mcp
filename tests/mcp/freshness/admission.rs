use super::*;

#[test]
fn worktree_churn_keeps_one_registration_for_the_shared_slot() {
    let first = Workspace::with_files(&[("main.rs", "pub fn first() {}\n")]);
    let server = in_process_server(&first);
    snapshot(&server).unwrap();
    let mut workspaces = Vec::new();
    for _ in 0..5 {
        let workspace = Workspace::with_files(&[("main.rs", "pub fn other() {}\n")]);
        let mut rebound = server.clone();
        rebound.root = workspace.root().to_path_buf();
        rebound.db = workspace.root().join(".forge/index.sqlite");
        snapshot(&rebound).unwrap();
        assert_eq!(server.reader_registration_count().unwrap(), 1);
        workspaces.push(workspace);
    }
}

#[test]
fn receipt_failures_report_reason_and_read_recovers() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let db_path = in_process_server(&ws).db;
    ws.build_to(&db_path);
    let receipt = PathBuf::from(format!("{}.verified.json", db_path.display()));
    for publication_failure in [false, true] {
        if publication_failure {
            if receipt.exists() {
                fs::remove_file(&receipt).unwrap();
            }
            fs::create_dir(&receipt).unwrap();
        } else {
            fs::write(&receipt, b"invalid json").unwrap();
        }
        let response = snapshot(&in_process_server(&ws)).unwrap();
        let diagnostics = response["diagnostics"].to_string();
        assert!(
            diagnostics.contains(if publication_failure {
                "verified receipt publication failed"
            } else {
                "verified receipt unavailable"
            }),
            "{diagnostics}"
        );
        assert!(
            diagnostics.contains(&db_path.display().to_string()),
            "{diagnostics}"
        );
        assert!(
            diagnostics.contains(if publication_failure {
                "os error"
            } else {
                "expected value"
            }),
            "{diagnostics}"
        );
        assert_eq!(response["nodes"][0]["name"], "before");
        if publication_failure {
            fs::remove_dir(&receipt).unwrap();
        }
    }
}

#[test]
fn warm_queries_reuse_the_same_sqlite_connection() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
    server
        .read(|conn| {
            conn.busy_timeout(Duration::from_millis(1234))?;
            Ok(json!({}))
        })
        .unwrap();
    let response = server
        .read(|conn| {
            let timeout: i64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
            Ok(json!({"timeout": timeout}))
        })
        .unwrap();
    assert_eq!(response["timeout"], 1234);
}

#[test]
fn query_failure_preserves_error_when_rollback_also_fails() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
    let error = server
        .read(|conn| {
            conn.execute_batch("ROLLBACK")?;
            anyhow::bail!("intentional query failure")
        })
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("intentional query failure"), "{message}");
    assert!(message.contains("query rollback failed"), "{message}");
    assert_eq!(snapshot(&server).unwrap()["nodes"][0]["name"], "before");
    snapshot(&server).unwrap();
    assert!(server
        .read(|_| anyhow::bail!("intentional query failure"))
        .is_err());
    snapshot(&server).unwrap();
    let err = server
        .read(|_| {
            IndexFixture::open(&server.db).tamper_function_names("tampered")?;
            Ok(json!({"should_not_escape": true}))
        })
        .unwrap_err();
    assert!(err.to_string().contains("generation changed"));
    assert!(snapshot(&server).is_err());
}

#[test]
fn foreign_and_corrupt_indexes_remain_rejected_without_replacement() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn original() {}\n")]);
    let other = Workspace::with_files(&[("src/main.rs", "pub fn original() {}\n")]);
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let foreign = Server::new(other.root().to_path_buf(), server.db.clone())
        .with_cache_dir(other.path(".forge/extraction-cache"));
    assert!(snapshot(&foreign)
        .unwrap_err()
        .to_string()
        .contains("different workspace"));
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());
    let mut cached_foreign = server.clone();
    cached_foreign.root = other.root().to_path_buf();
    assert!(snapshot(&cached_foreign)
        .unwrap_err()
        .to_string()
        .contains("different workspace"));
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());

    let index = IndexFixture::open(&server.db);
    index.outdated(MetadataVersion::Schema);
    index.checkpoint();
    drop(index);
    let damaged = IndexFixture::corrupt_file(&server.db, IndexCorruption::Freelist);
    assert!(snapshot(&server).is_err());
    assert_eq!(fs::read(&server.db).unwrap(), damaged);

    drop(foreign);
    drop(cached_foreign);
    drop(server);
    fs::write(ws.path(".forge/index.sqlite"), b"corrupt database").unwrap();
    assert!(snapshot(&in_process_server(&ws)).is_err());
    assert_eq!(
        fs::read(ws.path(".forge/index.sqlite")).unwrap(),
        b"corrupt database"
    );

    let server = in_process_server(&other);
    snapshot(&server).unwrap();
    let db_path = server.db.clone();
    drop(server);
    let index = IndexFixture::open(&db_path);
    index.tamper_function_names("tampered").unwrap();
    index.checkpoint();
    drop(index);
    let identity = db::cache::identity(&db_path).unwrap();
    other.write("forge-mcp.yaml", "roots:\n  - src\n");
    assert!(snapshot(&in_process_server(&other)).is_err());
    assert_eq!(identity, db::cache::identity(&db_path).unwrap());
}

#[test]
fn mcp_rebuild_admission_preserves_corruption_and_rebuilds_valid_legacy_index() {
    for (mismatch, corruption) in [
        ("schema", IndexCorruption::Freelist),
        ("schema", IndexCorruption::DocumentSearchData),
        ("semantics", IndexCorruption::DocumentSearchData),
        ("adapter", IndexCorruption::Freelist),
    ] {
        let ws = Workspace::with_files(&[
            ("main.rs", "pub fn current() {}\n"),
            ("README.md", "# Current\nIndexed documentation.\n"),
        ]);
        let database = ws.db();
        ws.build_to(&database);

        if mismatch == "adapter" {
            ws.write("forge-mcp.yaml", "adapter_version: 2\n");
        } else {
            let version = match mismatch {
                "schema" => MetadataVersion::Schema,
                "semantics" => MetadataVersion::Semantics,
                _ => unreachable!(),
            };
            IndexFixture::open(&database).outdated(version);
        }

        let fault_label = format!("{mismatch}/{corruption:?}");
        let before = IndexFixture::corrupt_file(&database, corruption);

        let mut client = McpTestHarness::new(&ws).expect("MCP initialize");
        let response = client
            .request_value(
                "tools/call",
                json!({"name":"code_map_overview","arguments":{"detail":"compact"}}),
            )
            .expect("MCP transport");
        assert_eq!(
            response["result"]["isError"], true,
            "{fault_label}: {response}"
        );
        assert_eq!(fs::read(&database).unwrap(), before, "{fault_label}");
        client.finish().expect("server must finish successfully");
    }

    let empty = Workspace::with_files(&[("main.rs", "pub fn current() {}\n")]);
    let database = empty.db();
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::write(&database, []).unwrap();
    let before = fs::read(&database).unwrap();
    let mut client = McpTestHarness::new(&empty).expect("MCP initialize");
    let response = client
        .request_value(
            "tools/call",
            json!({"name":"code_map_overview","arguments":{"detail":"compact","format":"json"}}),
        )
        .expect("MCP transport");
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(fs::read(&database).unwrap(), before);

    let legacy = Workspace::with_files(&[("main.rs", "pub fn current() {}\n")]);
    let database = legacy.db();
    legacy.build_to(&database);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("DROP TABLE metadata")
        .unwrap();

    client.finish().expect("server must finish successfully");
    let mut client = McpTestHarness::new(&legacy).expect("MCP initialize");
    let response = client
        .request_value(
            "tools/call",
            json!({"name":"code_map_overview","arguments":{"detail":"compact","format":"json"}}),
        )
        .expect("MCP transport");
    assert_ne!(response["result"]["isError"], true, "{response}");
    let overview: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(overview["freshness"]["refresh"], "rebuild");
    assert!(db::reader::open(&database, legacy.root()).is_ok());
    client.finish().expect("server must finish successfully");
}

#[test]
#[cfg(unix)]
fn symlink_database_is_rejected_without_touching_target() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn original() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let original = server.db.clone();
    let alias = ws.path("alias.sqlite");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let identity = db::cache::identity(&original).unwrap();
    let aliased = Server::new(ws.root().to_path_buf(), alias)
        .with_cache_dir(ws.path(".forge/extraction-cache"));
    assert!(snapshot(&aliased).is_err());
    assert_eq!(identity, db::cache::identity(&original).unwrap());
}

#[test]
fn newly_created_configured_root_is_admitted() {
    let ws = Workspace::with_files(&[("forge-mcp.yaml", "roots:\n  - src\n")]);
    let server = in_process_server(&ws);
    assert!(snapshot(&server).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .is_empty());
    ws.write("src/created.rs", "pub fn created() {}\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"][0]["name"], "created");
    assert_eq!(after["freshness"]["refresh"], "rebuild");
}

#[test]
fn changed_ignore_file_removes_existing_source_from_inventory() {
    let ws = Workspace::with_gitdir(&[
        ("visible.rs", "pub fn visible() {}\n"),
        ("ignored.rs", "pub fn ignored() {}\n"),
    ])
    .expect("workspace with .git directory");
    let server = in_process_server(&ws);
    assert_eq!(
        snapshot(&server).unwrap()["nodes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    ws.write(".gitignore", "ignored.rs\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(after["nodes"][0]["name"], "visible");
    assert_eq!(after["freshness"]["refresh"], "rebuild");
}
