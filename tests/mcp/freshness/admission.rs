use super::*;

#[test]
fn failed_query_keeps_next_read_usable_and_database_changes_are_fenced() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
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
