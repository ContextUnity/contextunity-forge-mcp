use super::*;

#[test]
fn same_server_metadata_query_refreshes_edited_source() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn current() {\n    let value = 1;\n}\n")]);
    let server = in_process_server(&ws);
    let before = snapshot(&server).unwrap();
    assert_eq!(before["nodes"][0]["end_line"], 3);
    ws.write(
        "main.rs",
        "pub fn current() {\n    let value = 1;\n    let next = value + 1;\n}\n",
    );
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"][0]["end_line"], 4);
    assert_ne!(before["corpus_hash"], after["corpus_hash"]);
    assert_ne!(before["output_root"], after["output_root"]);
    assert_eq!(after["freshness"]["status"], "source_inventory_matched");
    assert_eq!(after["freshness"]["output_root"], after["output_root"]);
    assert_eq!(after["freshness"]["corpus_hash"], after["corpus_hash"]);
    assert_eq!(after["freshness"]["refresh"], "delta");
}

#[test]
fn verified_receipt_recovery_survives_rollback_and_rejects_tampering() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
    let first = snapshot(&server).unwrap();
    let database = server.db.clone();
    let receipt = PathBuf::from(format!("{}.verified.json", database.display()));
    assert!(receipt.exists());
    fs::remove_file(&receipt).unwrap();
    drop(server);

    let reopened = snapshot(&in_process_server(&ws)).unwrap();
    assert_eq!(reopened["corpus_hash"], first["corpus_hash"]);
    assert_eq!(reopened["freshness"]["refresh"], "none");
    assert!(
        receipt.exists(),
        "verified database should regain its receipt"
    );

    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("CREATE TRIGGER reject_file_delete BEFORE DELETE ON files BEGIN SELECT RAISE(ABORT, 'forced delta rollback'); END;")
        .unwrap();
    assert!(
        db::cache::publish_verified(&database, ws.root(), first["output_root"].as_str().unwrap())
            .unwrap()
    );
    let receipt = PathBuf::from(format!("{}.verified.json", database.display()));

    ws.write("main.rs", "pub fn after_() {}\n");
    let error = contextunity_forge_mcp::engine::pipeline::delta_with_cache_dir(
        ws.root(),
        &database,
        &[PathBuf::from("main.rs")],
        &ws.path(".forge/extraction-cache"),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("forced delta rollback"),
        "{error:#}"
    );
    assert!(
        receipt.exists(),
        "rollback must preserve the verified receipt"
    );
    assert!(db::cache::matches(
        &database,
        ws.root(),
        first["output_root"].as_str().unwrap(),
        &db::cache::identity(&database).unwrap()
    ));

    let verified = db::cache::identity(&database).unwrap();
    db::cache::invalidate(&database);
    let index = IndexFixture::open(&database);
    index.tamper_function_names("tampered").unwrap();
    drop(index);
    assert!(
        !db::cache::publish_verified_identity(
            &database,
            ws.root(),
            first["output_root"].as_str().unwrap(),
            &verified
        )
        .unwrap()
    );
    assert!(!receipt.exists());
    assert!(db::reader::open(&database, ws.root()).is_err());
}

#[test]
fn same_server_refreshes_add_edit_rename_and_delete() {
    let ws = Workspace::with_files(&[("first.rs", "pub fn first() {}\n")]);
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    ws.write("added.rs", "pub fn added() {}\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let added = snapshot(&server).unwrap();
    assert_eq!(added["nodes"].as_array().unwrap().len(), 2);

    let before = added.clone();
    let path = ws.path("first.rs");
    let old_modified = fs::metadata(&path).unwrap().modified().unwrap();
    ws.write("first.rs", "pub fn after_() {}\n");
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old_modified))
        .unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let edited = snapshot(&server).unwrap();
    let edited_first = edited["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["path"] == "first.rs")
        .unwrap();
    assert_eq!(edited_first["name"], "after_");
    assert_ne!(before["corpus_hash"], edited["corpus_hash"]);
    assert_ne!(before["output_root"], edited["output_root"]);

    fs::rename(ws.path("added.rs"), ws.path("renamed.rs")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let renamed = snapshot(&server).unwrap();
    assert_eq!(renamed["nodes"][1]["path"], "renamed.rs");
    fs::remove_file(ws.path("first.rs")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let deleted = snapshot(&server).unwrap();
    assert_eq!(deleted["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(deleted["nodes"][0]["name"], "added");
}

#[test]
fn linked_source_edits_and_renames_refresh_in_same_server() {
    let linked = Workspace::with_files(&[("lib.rs", "pub fn before() {}\n")]);
    let ws = Workspace::with_files(&[(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: library\n    path: {}\n",
            linked.root().display()
        ),
    )]);
    let server = in_process_server(&ws);
    let before = snapshot(&server).unwrap();
    assert_eq!(before["nodes"][0]["path"], "[library]/lib.rs");
    linked.write("lib.rs", "pub fn changed() {}\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    assert_eq!(snapshot(&server).unwrap()["nodes"][0]["name"], "changed");
    fs::rename(linked.path("lib.rs"), linked.path("next.rs")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    assert_eq!(
        snapshot(&server).unwrap()["nodes"][0]["path"],
        "[library]/next.rs"
    );
    fs::remove_file(linked.path("next.rs")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    assert!(
        snapshot(&server).unwrap()["nodes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn external_delta_invalidates_cached_connection_without_another_write() {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    ws.write("main.rs", "pub fn after() {}\n");
    let delta = contextunity_forge_mcp::engine::pipeline::delta_with_cache_dir(
        ws.root(),
        &server.db,
        &[PathBuf::from("main.rs")],
        &ws.path(".forge/extraction-cache"),
    )
    .unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"][0]["name"], "after");
    assert_eq!(after["output_root"], delta["output_root"]);
    assert_eq!(after["freshness"]["refresh"], "none");
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());
}

#[test]
#[cfg(unix)]
fn cold_rebuild_releases_idle_mcp_readers_before_replacing_database() {
    use std::os::unix::fs::MetadataExt;

    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let first = in_process_server(&ws);
    let second = in_process_server(&ws);
    assert_eq!(snapshot(&first).unwrap()["nodes"][0]["name"], "before");
    assert_eq!(snapshot(&second).unwrap()["nodes"][0]["name"], "before");
    assert!(first.connection.lock().unwrap().is_some());
    assert!(second.connection.lock().unwrap().is_some());
    let inode = fs::metadata(&first.db).unwrap().ino();

    ws.write("main.rs", "pub fn after() {}\n");
    let report = ws.build_to(&first.db);

    assert_ne!(fs::metadata(&first.db).unwrap().ino(), inode);
    assert_eq!(report["verification_cache"], true);
    assert_eq!(snapshot(&first).unwrap()["nodes"][0]["name"], "after");
    assert_eq!(snapshot(&second).unwrap()["nodes"][0]["name"], "after");
    let conn = rusqlite::Connection::open(&first.db).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
#[cfg(unix)]
fn cold_rebuild_waits_for_active_mcp_reader_before_rename() {
    use std::os::unix::fs::MetadataExt;

    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let inode = fs::metadata(&server.db).unwrap().ino();

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        server.read(|conn| {
            let name = crate::sqlite_fixture::first_function_name(conn)?;
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(json!({"name": name}))
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    ws.write("main.rs", "pub fn after() {}\n");

    let root = ws.root().to_path_buf();
    let db = in_process_server(&ws).db;
    let builder = std::thread::spawn(move || {
        contextunity_forge_mcp::engine::pipeline::build_with_cache_dir(
            &root,
            &db,
            None,
            &root.join(".forge/extraction-cache"),
        )
    });
    std::thread::sleep(Duration::from_millis(500));
    let inode_while_reading = fs::metadata(in_process_server(&ws).db).unwrap().ino();
    release_tx.send(()).unwrap();
    assert_eq!(reader.join().unwrap().unwrap()["name"], "before");
    builder.join().unwrap().unwrap();

    assert_eq!(inode_while_reading, inode);
    assert_ne!(
        fs::metadata(in_process_server(&ws).db).unwrap().ino(),
        inode
    );
    assert_eq!(
        snapshot(&in_process_server(&ws)).unwrap()["nodes"][0]["name"],
        "after"
    );
}

#[test]
fn unchanged_warm_reads_and_metadata_only_touch_do_not_rewrite_database() {
    let mut sources = Vec::new();
    for i in 0..64 {
        sources.push((
            format!("src/file_{i}.rs"),
            format!("pub fn value_{i}() {{}}\n"),
        ));
    }
    let files: Vec<_> = sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let ws = Workspace::with_files(&files);
    let server = in_process_server(&ws);
    let before = snapshot(&server).unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let bytes = fs::read(&server.db).unwrap();
    let mut elapsed = Vec::new();
    for _ in 0..5 {
        let current = snapshot(&server).unwrap();
        assert_eq!(current["corpus_hash"], before["corpus_hash"]);
        assert_eq!(current["freshness"]["refresh"], "none");
        assert_eq!(current["freshness"]["files_checked"], 64);
        assert_eq!(current["freshness"]["inventory_scan_ms"], 0.0);
        elapsed.push(current["freshness"]["inventory_scan_ms"].as_f64().unwrap());
    }
    fs::File::options()
        .write(true)
        .open(ws.path("src/file_0.rs"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(SystemTime::now()))
        .unwrap();
    let touched = snapshot(&server).unwrap();
    assert_eq!(touched["output_root"], before["output_root"]);
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());
    assert_eq!(bytes, fs::read(&server.db).unwrap());
    eprintln!("64-file warm source inventory scans (ms): {elapsed:?}");
}
