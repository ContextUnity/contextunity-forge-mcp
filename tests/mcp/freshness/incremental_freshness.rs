use super::*;

#[test]
#[cfg(unix)]
fn excluded_entry_does_not_skip_unresolved_symlink_validation() {
    use std::os::unix::fs::symlink;
    let outside = Workspace::with_files(&[("file.rs", "pub fn outside() {}\n")]);
    let ws = Workspace::with_files(&[
        ("excluded.rs", "pub fn before() {}\n"),
        ("dir/file.rs", "pub fn inside() {}\n"),
    ]);
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    fs::remove_file(ws.path("excluded.rs")).unwrap();
    fs::create_dir(ws.path("excluded.rs")).unwrap();
    fs::remove_dir_all(ws.path("dir")).unwrap();
    symlink(outside.root(), ws.path("dir")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let error = snapshot(&server).unwrap_err();
    assert!(format!("{error:#}").contains("dir/file.rs"), "{error:#}");
    fs::remove_file(ws.path("dir")).unwrap();
    drop(server);
}

#[test]
#[cfg(unix)]
fn same_server_refreshes_exact_spelling_and_alias_renames() {
    use std::os::unix::fs::symlink;

    let linked = Workspace::with_files(&[
        ("Foo.rs", "pub fn linked() {}\n"),
        ("Parent/Foo.rs", "pub fn parent() {}\n"),
    ]);
    let ws = Workspace::with_files(&[
        ("Main.rs", "pub fn primary() {}\n"),
        ("case/Foo.rs", "pub fn renamed() {}\n"),
        ("unicode/caf\u{e9}.rs", "pub fn unicode() {}\n"),
    ]);
    fs::create_dir_all(ws.path(".forge")).unwrap();
    let primary_alias = ws.path(".forge/primary-alias");
    let linked_alias = ws.path(".forge/linked-alias");
    symlink(ws.root(), &primary_alias).unwrap();
    symlink(linked.root(), &linked_alias).unwrap();
    ws.write(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: library\n    path: {}\n",
            linked_alias.display()
        ),
    );
    let server = Server::new(primary_alias.clone(), ws.path(".forge/index.sqlite"))
        .with_cache_dir(ws.path(".forge/extraction-cache"));
    snapshot(&server).unwrap();

    for (old, new) in [
        ("Main.rs", "main.rs"),
        ("case/Foo.rs", "case/foo.rs"),
        ("unicode/caf\u{e9}.rs", "unicode/cafe\u{301}.rs"),
    ] {
        let before = snapshot(&server).unwrap();
        let old_parent = std::path::Path::new(old).parent().unwrap();
        let old_key = fs::read_dir(ws.path(old_parent))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter_map(|name| name.into_string().ok())
            .find(|name| name.ends_with(".rs"))
            .map(|name| old_parent.join(name).to_string_lossy().into_owned())
            .unwrap();
        assert!(before["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["path"] == old_key));
        fs::rename(ws.path(old), ws.path(new)).unwrap();
        if old == "Main.rs" {
            fs::rename(linked.path("Foo.rs"), linked.path("foo.rs")).unwrap();
        }
        let new_key = fs::read_dir(ws.path(old_parent))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter_map(|name| name.into_string().ok())
            .find(|name| name.ends_with(".rs"))
            .map(|name| old_parent.join(name).to_string_lossy().into_owned())
            .unwrap();
        invalidate_mcp_connection_cache(&server.db).unwrap();
        let after = snapshot(&server).unwrap();
        let paths: Vec<_> = after["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&new_key.as_str()), "{paths:?}");
        if old == "Main.rs" {
            assert!(paths.contains(&"[library]/foo.rs"), "{paths:?}");
        }
        if old_key != new_key {
            assert_eq!(after["freshness"]["refresh"], "delta");
        }
        if old != "Main.rs" {
            fs::remove_file(ws.path(&new_key)).unwrap();
            invalidate_mcp_connection_cache(&server.db).unwrap();
            assert!(!snapshot(&server).unwrap()["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["path"] == new_key));
        }
    }
    fs::rename(linked.path("Parent"), linked.path("parent")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = snapshot(&server).unwrap();
    assert!(after["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["path"] == "[library]/parent/Foo.rs"));
    assert_eq!(after["freshness"]["refresh"], "delta");
    let cold = ws.path(".forge/cold-linked.sqlite");
    ws.build_to(&cold);
    let conn = contextunity_forge_mcp::db::reader::open(&cold, ws.root()).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE path='[library]/parent/Foo.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let cold_root: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after["output_root"], cold_root);
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    fs::remove_file(cold).unwrap();
    drop(server);
    fs::remove_file(primary_alias).unwrap();
    fs::remove_file(linked_alias).unwrap();
}

fn rollback_base() -> (Workspace, PathBuf, String) {
    let ws = Workspace::with_files(&[("main.rs", "pub fn before() {}\n")]);
    let server = in_process_server(&ws);
    let first = snapshot(&server).unwrap();
    let database = server.db.clone();
    drop(server);
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_file_delete BEFORE DELETE ON files BEGIN SELECT RAISE(ABORT, 'forced delta rollback'); END;").unwrap();
    drop(conn);
    let root = first["output_root"].as_str().unwrap().to_owned();
    db::cache::publish_verified(&database, ws.root(), &root)
        .unwrap()
        .then_some(())
        .expect("publish base receipt");
    ws.write("main.rs", "pub fn after_() {}\n");
    (ws, database, root)
}

struct RollbackFixture<'a> {
    base: &'a Workspace,
    copy: Workspace,
}

impl RollbackFixture<'_> {
    fn root(&self) -> &std::path::Path {
        self.base.root()
    }
    fn path(&self, path: &str) -> PathBuf {
        self.copy.path(path)
    }
}

fn rollback_fixture(base: &(Workspace, PathBuf, String)) -> (RollbackFixture<'_>, PathBuf, String) {
    let ws = Workspace::new();
    fs::create_dir_all(ws.path(".forge")).unwrap();
    fs::copy(base.0.path("main.rs"), ws.path("main.rs")).unwrap();
    let database = ws.path(".forge/index.sqlite");
    fs::copy(&base.1, &database).unwrap();
    fs::copy(
        format!("{}.verified.json", base.1.display()),
        format!("{}.verified.json", database.display()),
    )
    .unwrap();
    db::cache::publish_verified(&database, base.0.root(), &base.2)
        .unwrap()
        .then_some(())
        .expect("publish copied receipt");
    (
        RollbackFixture {
            base: &base.0,
            copy: ws,
        },
        database,
        base.2.clone(),
    )
}

fn close_rollback_fixture(database: &std::path::Path) {
    let conn = rusqlite::Connection::open(database).unwrap();
    conn.execute_batch("DROP TRIGGER reject_file_delete")
        .unwrap();
    drop(conn);
}

fn database_sidecars(database: &std::path::Path) -> Vec<(PathBuf, std::io::Result<fs::Metadata>)> {
    ["-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| {
            let path = PathBuf::from(format!("{}{suffix}", database.display()));
            let metadata = fs::symlink_metadata(&path);
            (path, metadata)
        })
        .collect()
}

#[test]
fn delta_rollback_receipt_verification_matrix() {
    let base = rollback_base();
    let cases = 0..7;
    for case in cases {
        match case {
            0 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                for case in [
                    "missing",
                    "corrupt",
                    "root",
                    "database",
                    "identity",
                    "permissions",
                ] {
                    let (ws, database, root) = rollback_fixture(&base);
                    let hooks = PipelineTestHooks::delta_failure(
                        move |db| {
                            let receipt = PathBuf::from(format!("{}.verified.json", db.display()));
                            match case {
                                "missing" => fs::remove_file(receipt).unwrap(),
                                "corrupt" => fs::write(receipt, b"invalid json").unwrap(),
                                "permissions" => {
                                    use std::os::unix::fs::PermissionsExt;
                                    fs::set_permissions(receipt, fs::Permissions::from_mode(0o644))
                                        .unwrap();
                                }
                                _ => {
                                    let mut value: serde_json::Value =
                                        serde_json::from_slice(&fs::read(&receipt).unwrap())
                                            .unwrap();
                                    match case {
                                        "root" => value["root"] = json!("/wrong-root"),
                                        "database" => value["database"] = json!("/wrong-database"),
                                        "identity" => value["identity"]["main"]["size"] = json!(0),
                                        _ => unreachable!(),
                                    }
                                    fs::write(receipt, serde_json::to_vec(&value).unwrap())
                                        .unwrap();
                                }
                            }
                        },
                        |db| {
                            fs::File::options()
                                .write(true)
                                .open(db)
                                .unwrap()
                                .set_times(
                                    fs::FileTimes::new().set_modified(std::time::SystemTime::now()),
                                )
                                .unwrap();
                        },
                    );
                    let error = delta_with_test_hooks(
                        ws.root(),
                        &database,
                        &[PathBuf::from("main.rs")],
                        &ws.path(".forge/extraction-cache"),
                        &hooks,
                    )
                    .unwrap_err();
                    assert!(
                        format!("{error:#}").contains("forced delta rollback"),
                        "{case}: {error:#}"
                    );
                    if case == "permissions" {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(
                            format!("{}.verified.json", database.display()),
                            fs::Permissions::from_mode(0o600),
                        )
                        .unwrap();
                    }
                    let identity = db::cache::identity(&database).unwrap();
                    assert!(
                        db::cache::matches_with_reason(&database, ws.root(), &root, &identity)
                            .is_err(),
                        "{case}: untrusted receipt was resealed"
                    );
                    close_rollback_fixture(&database);
                    for (sidecar, metadata) in database_sidecars(&database) {
                        assert!(
                            matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                            "sidecar remains or cannot be inspected: {}",
                            sidecar.display()
                        );
                    }
                }
            }
            1 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                let (ws, database, root) = rollback_fixture(&base);
                let hooks = PipelineTestHooks::delta_failure(
                    |_| {},
                    |db| {
                        let index = IndexFixture::open(db);
                        index.tamper_function_names("tampered").unwrap();
                    },
                );
                let error = delta_with_test_hooks(
                    ws.root(),
                    &database,
                    &[PathBuf::from("main.rs")],
                    &ws.path(".forge/extraction-cache"),
                    &hooks,
                )
                .unwrap_err();
                assert!(
                    format!("{error:#}").contains("forced delta rollback"),
                    "{error:#}"
                );
                let identity = db::cache::identity(&database).unwrap();
                assert!(
                    db::cache::matches_with_reason(&database, ws.root(), &root, &identity).is_err(),
                    "tampered database received a valid receipt"
                );
                assert!(db::reader::open(&database, ws.root()).is_err());
                close_rollback_fixture(&database);
                for (sidecar, metadata) in database_sidecars(&database) {
                    assert!(
                        matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                        "sidecar remains or cannot be inspected: {}",
                        sidecar.display()
                    );
                }
            }
            2 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                let (ws, database, root) = rollback_fixture(&base);
                let before = db::cache::identity(&database).unwrap();
                let hooks = PipelineTestHooks::delta_failure(
                    |_| {},
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new()
                                    .set_modified(std::time::SystemTime::UNIX_EPOCH),
                            )
                            .unwrap();
                    },
                );
                let error = delta_with_test_hooks(
                    ws.root(),
                    &database,
                    &[PathBuf::from("main.rs")],
                    &ws.path(".forge/extraction-cache"),
                    &hooks,
                )
                .unwrap_err();
                assert!(
                    format!("{error:#}").contains("forced delta rollback"),
                    "{error:#}"
                );
                let after = db::cache::identity(&database).unwrap();
                assert_ne!(before, after, "identity drift was not exercised");
                db::cache::matches_with_reason(&database, ws.root(), &root, &after).unwrap_or_else(
                    |reason| {
                        panic!(
                            "revalidated receipt mismatch ({}): {reason:#}",
                            db::cache::describe_diff(&before, &after)
                        )
                    },
                );
                close_rollback_fixture(&database);
                for (sidecar, metadata) in database_sidecars(&database) {
                    assert!(
                        matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                        "sidecar remains or cannot be inspected: {}",
                        sidecar.display()
                    );
                }
            }
            3 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                let (ws, database, root) = rollback_fixture(&base);
                let hooks = PipelineTestHooks::delta_failure_before_publication(
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new()
                                    .set_modified(std::time::SystemTime::UNIX_EPOCH),
                            )
                            .unwrap();
                    },
                    |db| {
                        let receipt = PathBuf::from(format!("{}.verified.json", db.display()));
                        fs::remove_file(&receipt).unwrap();
                        fs::create_dir(&receipt).unwrap();
                    },
                );
                let error = delta_with_test_hooks(
                    ws.root(),
                    &database,
                    &[PathBuf::from("main.rs")],
                    &ws.path(".forge/extraction-cache"),
                    &hooks,
                )
                .unwrap_err();
                assert!(
                    format!("{error:#}").contains("forced delta rollback"),
                    "{error:#}"
                );
                let receipt = PathBuf::from(format!("{}.verified.json", database.display()));
                assert!(receipt.is_dir());
                assert_eq!(fs::read_dir(&receipt).unwrap().count(), 0);
                assert!(db::cache::matches_with_reason(
                    &database,
                    ws.root(),
                    &root,
                    &db::cache::identity(&database).unwrap()
                )
                .is_err());
                fs::remove_dir(receipt).unwrap();
                close_rollback_fixture(&database);
                for (sidecar, metadata) in database_sidecars(&database) {
                    assert!(
                        matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                        "sidecar remains or cannot be inspected: {}",
                        sidecar.display()
                    );
                }
            }
            4 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                let (ws, database, root) = rollback_fixture(&base);
                let before = db::cache::identity(&database).unwrap();
                let wal = PathBuf::from(format!("{}-wal", database.display()));
                let hooks = PipelineTestHooks::delta_failure_before_publication(
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new()
                                    .set_modified(std::time::SystemTime::UNIX_EPOCH),
                            )
                            .unwrap();
                    },
                    |db| fs::write(format!("{}-wal", db.display()), b"nonempty wal").unwrap(),
                );
                let error = delta_with_test_hooks(
                    ws.root(),
                    &database,
                    &[PathBuf::from("main.rs")],
                    &ws.path(".forge/extraction-cache"),
                    &hooks,
                )
                .unwrap_err();
                assert!(
                    format!("{error:#}").contains("forced delta rollback"),
                    "{error:#}"
                );
                assert!(fs::metadata(&wal).unwrap().len() > 0);
                let identity = db::cache::identity(&database).unwrap();
                assert!(db::cache::describe_diff(&before, &identity).contains("wal:"));
                assert!(
                    db::cache::matches_with_reason(&database, ws.root(), &root, &identity).is_err()
                );
                assert!(!PathBuf::from(format!("{}.verified.json", database.display())).exists());
                fs::remove_file(wal).unwrap();
                close_rollback_fixture(&database);
                for (sidecar, metadata) in database_sidecars(&database) {
                    assert!(
                        matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                        "sidecar remains or cannot be inspected: {}",
                        sidecar.display()
                    );
                }
            }
            5 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                let (ws, database, root) = rollback_fixture(&base);
                let hooks = PipelineTestHooks::delta_failure_during_verification(
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new()
                                    .set_modified(std::time::SystemTime::UNIX_EPOCH),
                            )
                            .unwrap()
                    },
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new().set_modified(std::time::SystemTime::now()),
                            )
                            .unwrap()
                    },
                );
                let error = delta_with_test_hooks(
                    ws.root(),
                    &database,
                    &[PathBuf::from("main.rs")],
                    &ws.path(".forge/extraction-cache"),
                    &hooks,
                )
                .unwrap_err();
                assert!(format!("{error:#}").contains("forced delta rollback"));
                let identity = db::cache::identity(&database).unwrap();
                assert!(
                    db::cache::matches_with_reason(&database, ws.root(), &root, &identity).is_err()
                );
                assert!(!PathBuf::from(format!("{}.verified.json", database.display())).exists());
                close_rollback_fixture(&database);
                for (sidecar, metadata) in database_sidecars(&database) {
                    assert!(
                        matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                        "sidecar remains or cannot be inspected: {}",
                        sidecar.display()
                    );
                }
            }
            6 => {
                use contextunity_forge_mcp::engine::pipeline::{
                    delta_with_test_hooks, PipelineTestHooks,
                };
                let (ws, database, root) = rollback_fixture(&base);
                let hooks = PipelineTestHooks::delta_failure_before_publication(
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new()
                                    .set_modified(std::time::SystemTime::UNIX_EPOCH),
                            )
                            .unwrap();
                    },
                    |db| {
                        fs::File::options()
                            .write(true)
                            .open(db)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new().set_modified(std::time::SystemTime::now()),
                            )
                            .unwrap();
                    },
                );
                let error = delta_with_test_hooks(
                    ws.root(),
                    &database,
                    &[PathBuf::from("main.rs")],
                    &ws.path(".forge/extraction-cache"),
                    &hooks,
                )
                .unwrap_err();
                assert!(
                    format!("{error:#}").contains("forced delta rollback"),
                    "{error:#}"
                );
                let identity = db::cache::identity(&database).unwrap();
                assert!(
                    db::cache::matches_with_reason(&database, ws.root(), &root, &identity).is_err()
                );
                assert!(!PathBuf::from(format!("{}.verified.json", database.display())).exists());
                close_rollback_fixture(&database);
                for (sidecar, metadata) in database_sidecars(&database) {
                    assert!(
                        matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                        "sidecar remains or cannot be inspected: {}",
                        sidecar.display()
                    );
                }
            }
            _ => unreachable!(),
        }
    }
}

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
fn missing_verified_receipt_is_recovered_on_read() {
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
}

#[test]
fn verified_receipt_survives_rollback_and_delta_retry() {
    let (ws, database, root) = rollback_base();
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
    if let Err(reason) = db::cache::matches_with_reason(
        &database,
        ws.root(),
        root.as_str(),
        &db::cache::identity(&database).unwrap(),
    ) {
        panic!("verified receipt must still match after rollback: {reason}");
    }

    let conn = rusqlite::Connection::open(&database).unwrap();
    let old_root: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let old_name: String = conn
        .query_row("SELECT name FROM nodes WHERE name='before'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(old_root, root.as_str());
    assert_eq!(old_name, "before");
    conn.execute_batch("DROP TRIGGER reject_file_delete")
        .unwrap();
    drop(conn);
    let retried = contextunity_forge_mcp::engine::pipeline::delta_with_cache_dir(
        ws.root(),
        &database,
        &[PathBuf::from("main.rs")],
        &ws.path(".forge/extraction-cache"),
    )
    .unwrap();
    assert_ne!(retried["output_root"].as_str().unwrap(), root);
    let conn = rusqlite::Connection::open(&database).unwrap();
    let new_name: String = conn
        .query_row("SELECT name FROM nodes WHERE name='after_'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(new_name, "after_");
    drop(conn);
    for (sidecar, metadata) in database_sidecars(&database) {
        assert!(
            matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
            "sidecar remains or cannot be inspected: {}",
            sidecar.display()
        );
    }
}

#[test]
fn same_server_coexisting_spellings_match_cold_build() {
    let ws = Workspace::new();
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    for (first, second, case_sensitive) in [
        ("Foo.rs", "foo.rs", true),
        ("caf\u{e9}.rs", "cafe\u{301}.rs", false),
    ] {
        if case_sensitive && contextunity_forge_mcp::core::fs::is_case_insensitive_volume(ws.root())
        {
            eprintln!("case coexistence unavailable on case-insensitive volume");
            continue;
        }
        ws.write(first, "pub fn upper() {}\n");
        invalidate_mcp_connection_cache(&server.db).unwrap();
        snapshot(&server).unwrap();
        ws.write(second, "pub fn lower() {}\n");
        if !case_sensitive {
            let unicode_entries = fs::read_dir(ws.root())
                .unwrap()
                .map(|entry| entry.unwrap())
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("caf"))
                .count();
            if unicode_entries == 1 {
                eprintln!("Unicode coexistence unavailable on normalization-folding volume");
                continue;
            }
            assert_eq!(unicode_entries, 2);
        }
        invalidate_mcp_connection_cache(&server.db).unwrap();
        let after = snapshot(&server).unwrap();
        let paths: Vec<_> = after["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["path"].as_str().unwrap())
            .collect();
        for path in [first, second] {
            assert!(paths.contains(&path), "{paths:?}");
        }
    }
    let after = snapshot(&server).unwrap();
    let paths: Vec<_> = after["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["path"].as_str().unwrap())
        .collect();
    let cold = ws.path(".forge/cold-spellings.sqlite");
    ws.build_to(&cold);
    let conn = contextunity_forge_mcp::db::reader::open(&cold, ws.root()).unwrap();
    let mut statement = conn
        .prepare("SELECT path FROM files ORDER BY path")
        .unwrap();
    let cold_paths: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for path in paths {
        assert!(cold_paths.iter().any(|p| p == path));
    }
    drop(statement);
    let cold_root: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after["output_root"], cold_root);
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    fs::remove_file(cold).unwrap();
}

#[test]
fn reserved_path_search_uri_is_encoded_once_after_rename() {
    let ws = Workspace::with_files(&[("old.rs", "pub fn coordinate() {}\n")]);
    ws.build();
    let mut client = McpTestHarness::new(&ws).unwrap();
    fs::rename(ws.path("old.rs"), ws.path("a #%.rs")).unwrap();
    let (_, response) = client
        .call(
            "code_map_search",
            json!({"pattern":"coordinate", "format":"markdown"}),
        )
        .unwrap();
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let uri = ws.file_uri("a #%.rs", Some(1));
    assert!(uri.ends_with("/a%20%23%25.rs#L1"), "{uri}");
    assert!(text.contains(&uri), "{text}");
    drop(client);
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
    assert!(snapshot(&server).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .is_empty());
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
    let (boundary_tx, boundary_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let builder = std::thread::spawn(move || {
        use contextunity_forge_mcp::engine::pipeline::{build_with_test_hooks, PipelineTestHooks};
        let hooks = PipelineTestHooks::before_generation_lock(move |_| {
            let _ = boundary_tx.send(());
        });
        let result = build_with_test_hooks(&root, &db, None, hooks);
        let _ = done_tx.send(result);
    });
    let reached = boundary_rx.recv_timeout(Duration::from_secs(10));
    let early = done_rx.recv_timeout(Duration::from_millis(500));
    let blocked = matches!(&early, Err(mpsc::RecvTimeoutError::Timeout));
    let inode_while_reading = fs::metadata(in_process_server(&ws).db).unwrap().ino();
    release_tx.send(()).unwrap();
    assert_eq!(reader.join().unwrap().unwrap()["name"], "before");
    let finished = match early {
        Ok(result) => Some(result),
        Err(_) => done_rx.recv_timeout(Duration::from_secs(30)).ok(),
    };
    assert!(
        reached.is_ok(),
        "builder did not reach generation lock: {reached:?}"
    );
    assert!(
        finished.is_some(),
        "builder did not complete after reader release"
    );
    builder.join().unwrap();
    finished.unwrap().unwrap();
    assert!(blocked, "builder passed active reader fence");

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
