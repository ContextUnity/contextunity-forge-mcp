use super::*;
use contextunity_forge_mcp::engine::pipeline::{build, delta};
use std::{path::PathBuf, time::SystemTime};

#[test]
fn same_server_refreshes_non_source_manifests_outside_source_roots() {
    let ws = Workspace::new();
    ws.write("forge-mcp.yaml", "roots: [src]\n");
    ws.write(
        "src/consumer.py",
        "import novel_library\ndef run(): return novel_library.fetch()\n",
    );
    let server = in_process_server(&ws);
    let evidence = || {
        server.read(|conn| Ok(json!({
        "evidence": conn.query_row("SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='novel_library'", [], |row| row.get::<_, String>(0))?,
        "manifest_digest": conn.query_row("SELECT value FROM metadata WHERE key='manifest_digest'", [], |row| row.get::<_, String>(0))?,
    }))).unwrap()
    };
    let before = evidence();
    assert!(!before["evidence"].as_str().unwrap().contains("manifest"));
    ws.write("requirements-dev.txt", "novel_library>=1\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = evidence();
    assert!(after["evidence"].as_str().unwrap().contains("manifest"));
    assert_ne!(before["manifest_digest"], after["manifest_digest"]);
    assert_eq!(after["freshness"]["refresh"], "rebuild");
    fs::remove_file(ws.path("requirements-dev.txt")).unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let removed = evidence();
    assert_eq!(removed["manifest_digest"], before["manifest_digest"]);
    assert!(!removed["evidence"].as_str().unwrap().contains("manifest"));
    let mut missing = Vec::new();
    let packages = Workspace::new();
    let linked_packages = Workspace::new();
    for (path, content) in [
        (
            "rust/Cargo.toml",
            "[package]\nname = 'manifest-cargo-package'\nversion = '0.1.0'\ndescription = 'Declared crate description'\n",
        ),
        (
            "python/pyproject.toml",
            "[project]\nname = 'manifest-python-package'\nversion = '0.1.0'\ndescription = 'Declared Python description'\n",
        ),
        (
            "poetry/pyproject.toml",
            "[tool.poetry]\nname = 'manifest-poetry-package'\nversion = '0.1.0'\ndescription = 'Declared Poetry description'\n",
        ),
        (
            "javascript/package.json",
            "{\"name\":\"manifest-npm-workspace\",\"workspaces\":[\"packages/*\"]}",
        ),
        (
            "javascript/packages/child/package.json",
            "{\"name\":\"manifest-npm-child\",\"description\":\"Declared npm description\"}",
        ),
        (
            "pnpm/package.json",
            "{\"name\":\"manifest-pnpm-workspace\"}",
        ),
        ("pnpm/pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
        (
            "pnpm/packages/child/package.json",
            "{\"name\":\"manifest-pnpm-child\"}",
        ),
        (
            "yarn/package.json",
            "{\"name\":\"manifest-yarn-workspace\",\"workspaces\":[\"packages/*\"]}",
        ),
        ("yarn/yarn.lock", "# yarn lockfile v1\n"),
        (
            "yarn/packages/child/package.json",
            "{\"name\":\"manifest-yarn-child\"}",
        ),
        (
            "go/go.mod",
            "module example.org/manifest-go-module\n\ngo 1.22\n",
        ),
        ("directory-only/unit.py", "def directory_marker(): pass\n"),
        ("broken/Cargo.toml", "this is not valid TOML = [\n"),
        (
            "unsupported/pyproject.toml",
            "[tool.black]\nline-length = 88\n",
        ),
        (
            "configured-ignored/Cargo.toml",
            "[package]\nname='configured-ignored-package'\nversion='0.1.0'\n",
        ),
        (
            "gitignored/Cargo.toml",
            "[package]\nname='gitignored-package'\nversion='0.1.0'\n",
        ),
        (
            "docs/guide.md",
            "---\ntitle: Discovery documentation hub\ndoc_type: guide\n---\n# Discovery documentation hub\nDeclared guide.\n",
        ),
        (
            "docs/milestones/010-discovery.md",
            "---\nid: m-discovery-fixture\ntitle: Discovery active milestone\ndoc_type: contract\nstatus: active\n---\n# Discovery active milestone\n",
        ),
    ] {
        packages.write(path, content);
    }
    linked_packages.write(
        "weird+%#1/pyproject.toml",
        "[project]\nname = 'linked-manifest-package'\n",
    );
    packages.write(".gitignore", "gitignored/\n");
    packages.write(
        "forge-mcp.yaml",
        &format!(
            "milestones: [docs/milestones]\nignore: [configured-ignored]\nlinked_workspaces:\n  - name: library\n    path: '{}'\n    roots: [.]\n",
            linked_packages.root().display()
        ),
    );
    drop(packages.build());
    // Scanner and milestone composition belong upstream of the DB reader.
    let mut client = StdioClient::new(&packages);
    let hub = client.payload("get_doc", json!({"path_or_id":"docs/guide.md"}));
    let hub_line = hub["sections"]["items"][0]["line"].as_u64().unwrap();
    let overview = client.payload("code_map_overview", json!({"limit":100}));
    assert_eq!(
        overview["discovery"]["active_milestones"]["availability"],
        "available"
    );
    let milestone = overview["discovery"]["active_milestones"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|milestone| milestone["id"] == "m-discovery-fixture")
        .expect("canonical active milestone row");
    assert_eq!(milestone["status"], "active");
    assert_eq!(milestone["completion"], "0/0");
    let (_, response) = client.call(
        "code_map_overview",
        serde_json::json!({"limit":100,"format":"markdown"}),
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let package_markdown = response["result"]["content"][0]["text"].as_str().unwrap();
    for expected in [
        "manifest-cargo-package",
        "manifest-python-package",
        "manifest-poetry-package",
        "manifest-npm-workspace",
        "manifest-npm-child",
        "manifest-pnpm-workspace",
        "manifest-pnpm-child",
        "manifest-yarn-workspace",
        "manifest-yarn-child",
        "example.org/manifest-go-module",
        "linked-manifest-package",
        "Discovery documentation hub",
        "Discovery active milestone",
        "m-discovery-fixture",
        "0/0 complete",
    ] {
        if !package_markdown.contains(expected) {
            missing.push(format!(
                "overview must supply actual admitted manifest/hub identity {expected:?}"
            ));
        }
    }
    for (path, line) in [("docs/guide.md", Some(hub_line)), ("rust", None)] {
        let coordinate = format!(
            "file://{}{}",
            packages.root().join(path).display(),
            line.map_or_else(String::new, |line| format!("#L{line}"))
        );
        if !package_markdown.contains(&coordinate) {
            missing.push(format!(
                "overview must resolve the actual coordinate {coordinate}"
            ));
        }
    }
    assert!(!package_markdown.contains("configured-ignored-package"));
    assert!(!package_markdown.contains("gitignored-package"));
    let directory_section = package_markdown
        .split("## Directory components")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("overview directory components section");
    assert!(directory_section.contains("directory-only"));
    let manifest_section = package_markdown
        .split("## Manifest components")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("overview manifest components section");
    assert!(!manifest_section.contains("directory-only"));
    assert!(!package_markdown.contains("directory_marker"));
    assert!(
        package_markdown.contains("malformed_manifest"),
        "{package_markdown}"
    );
    assert!(
        package_markdown.contains("unsupported_manifest_shape"),
        "{package_markdown}"
    );
    assert!(
        package_markdown.contains(&format!(
            "file://{}/weird%2B%25%231/pyproject.toml",
            linked_packages.root().display()
        )),
        "linked manifest URI must resolve against its owning root: {package_markdown}"
    );
    let full_manifest_page = &overview["discovery"]["manifest_components"];
    let linked_manifest = full_manifest_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "linked-manifest-package")
        .expect("linked package identity comes from its admitted manifest");
    assert_eq!(linked_manifest["workspace"], "library");
    assert_eq!(linked_manifest["root"], "weird+%#1");
    assert_eq!(linked_manifest["manifest_path"], "weird+%#1/pyproject.toml");

    let (_, scoped_overview_response) = client.call(
        "code_map_overview",
        serde_json::json!({"aspects":["manifest_components"],"limit":1,"format":"markdown"}),
    );
    assert_ne!(scoped_overview_response["result"]["isError"], true);
    let scoped_overview = scoped_overview_response["result"]["content"][0]["text"]
        .as_str()
        .expect("aspect-scoped overview Markdown");
    let manifest_continuation = scoped_overview
        .lines()
        .find(|line| line.starts_with("Continue manifest components "))
        .expect("bounded manifest page continuation");
    assert!(
        manifest_continuation.contains("aspects=[\"manifest_components\"]")
            && manifest_continuation.contains(&format!(
                "workspace_root={}",
                packages.root().display()
            )),
        "overview continuation must retain its aspect selection and workspace: {manifest_continuation}"
    );

    let first_page = client.payload("code_map_overview", json!({"limit":1}));
    let components = &first_page["discovery"]["manifest_components"];
    assert_eq!(components["availability"], "available");
    assert_eq!(components["items"].as_array().unwrap().len(), 1);
    assert_eq!(components["limit"], 1);
    assert!(components["has_more"].as_bool().unwrap());
    assert_eq!(components["next_offset"], 1);
    assert_eq!(components["diagnostics_available"], true);
    assert_eq!(components["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(components["diagnostic_total"], 2);
    assert_eq!(components["diagnostics_truncated"], true);
    let next_page = client.payload(
        "code_map_overview",
        json!({"limit":1,"offset":1,"generation":components["generation"]}),
    );
    let next_components = &next_page["discovery"]["manifest_components"];
    assert_eq!(next_components["total"], components["total"]);
    assert_eq!(next_components["offset"], 1);
    assert_ne!(next_components["items"][0], components["items"][0]);

    let bounded = Workspace::new();
    bounded.write("forge-mcp.yaml", "roots: [src]\nlimits:\n  max_files: 1\n");
    bounded.write("src/app.py", "def app(): pass\n");
    bounded.write("src/a/package.json", "{\"name\":\"bounded-a\"}");
    bounded.write("src/b/package.json", "{\"name\":\"bounded-b\"}");
    drop(bounded.build());
    let mut bounded_client = StdioClient::new(&bounded);
    let bounded_overview = bounded_client.payload("code_map_overview", json!({"limit":10}));
    assert_eq!(
        bounded_overview["discovery"]["manifest_components"]["availability"],
        "unavailable"
    );
    assert_eq!(
        bounded_overview["discovery"]["manifest_components"]["cause"],
        "manifest_file_count_limit"
    );
    assert!(
        missing.is_empty(),
        "discovery composition contract: {}\n\n{package_markdown}",
        missing.join("\n"),
    );
}

#[test]
fn manifest_changes_trigger_deterministic_rebuild_in_both_admit_and_delta() {
    let ws = Workspace::new();
    ws.write("forge-mcp.yaml", "roots: [src]\n");
    ws.write("requirements.txt", "requests>=2.0\n");
    ws.write("unindexed/requirements.txt", "unindexed_library>=1\n");
    ws.write("src/app.py", "import requests\n");

    let db = ws.path(".forge/index.sqlite");
    build(ws.root(), &db, None).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    let initial_digest: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='manifest_digest'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);

    let server = in_process_server(&ws);
    let unchanged = server.read(|_| Ok(json!({}))).unwrap();
    assert_eq!(unchanged["freshness"]["refresh"], "none");
    ws.write("unindexed/requirements.txt", "unindexed_library>=2\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let outside_roots = server.read(|_| Ok(json!({}))).unwrap();
    assert_eq!(outside_roots["freshness"]["refresh"], "none");

    // 1. Change manifest on disk and verify Server::admit performs rebuild
    ws.write("requirements.txt", "requests>=2.1\n");
    let server = in_process_server(&ws);
    let after_admit = server
        .read(|c| {
            Ok(json!({
                "manifest_digest": c.query_row("SELECT value FROM metadata WHERE key='manifest_digest'", [], |r| r.get::<_, String>(0))?
            }))
        })
        .unwrap();
    let admit_digest = after_admit["manifest_digest"].as_str().unwrap();
    assert_ne!(admit_digest, initial_digest);

    // 2. Change manifest on disk and verify delta performs rebuild
    ws.write("requirements.txt", "requests>=2.2\n");
    let delta_res = delta(ws.root(), &db, &[PathBuf::from("src/app.py")]).unwrap();
    assert!(delta_res.is_object());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let after_delta: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='manifest_digest'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(after_delta, admit_digest);
    assert_ne!(after_delta, initial_digest);
}

#[test]
fn same_server_metadata_query_refreshes_edited_source() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {\n    let value = 1;\n}\n");
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
fn verified_reader_restores_missing_receipt_for_unchanged_index() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {}\n");
    let server = in_process_server(&ws);
    let first = snapshot(&server).unwrap();
    let receipt = PathBuf::from(format!("{}.verified.json", server.db.display()));
    assert!(receipt.exists());
    fs::remove_file(&receipt).unwrap();

    let reopened = snapshot(&in_process_server(&ws)).unwrap();
    assert_eq!(reopened["corpus_hash"], first["corpus_hash"]);
    assert_eq!(reopened["freshness"]["refresh"], "none");
    assert!(
        receipt.exists(),
        "verified database should regain its receipt"
    );
}

#[test]
fn receipt_recovery_rejects_database_changed_after_verification() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {}\n");
    let server = in_process_server(&ws);
    let first = snapshot(&server).unwrap();
    let database = server.db.clone();
    drop(server);
    let verified = db::cache::identity(&database).unwrap();
    db::cache::invalidate(&database);
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute("UPDATE nodes SET name='tampered' WHERE kind='function'", [])
        .unwrap();
    drop(conn);
    assert!(!db::cache::publish_verified_identity(
        &database,
        ws.root(),
        first["output_root"].as_str().unwrap(),
        &verified
    )
    .unwrap());
    assert!(!PathBuf::from(format!("{}.verified.json", database.display())).exists());
    assert!(db::reader::open(&database, ws.root()).is_err());
}

#[test]
fn failed_delta_keeps_receipt_for_rolled_back_index() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = in_process_server(&ws);
    let first = snapshot(&server).unwrap();
    let database = server.db.clone();
    drop(server);
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_file_delete BEFORE DELETE ON files BEGIN SELECT RAISE(ABORT, 'forced delta rollback'); END;",
    )
    .unwrap();
    drop(conn);
    assert!(db::cache::publish_verified(
        &database,
        ws.root(),
        first["output_root"].as_str().unwrap()
    )
    .unwrap());
    let receipt = PathBuf::from(format!("{}.verified.json", database.display()));

    ws.write("main.rs", "pub fn after_() {}\n");
    let error = db::writer::delta(ws.root(), &database, &[PathBuf::from("main.rs")]).unwrap_err();
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
}

#[test]
fn same_server_refreshes_add_delete_and_rename() {
    let ws = Workspace::new();
    ws.write("first.rs", "pub fn first() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    ws.write("added.rs", "pub fn added() {}\n");
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let added = snapshot(&server).unwrap();
    assert_eq!(added["nodes"].as_array().unwrap().len(), 2);
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
fn restored_mtime_and_same_length_edit_refreshes_digest() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = in_process_server(&ws);
    let before = snapshot(&server).unwrap();
    let path = ws.path("main.rs");
    let old_modified = fs::metadata(&path).unwrap().modified().unwrap();
    ws.write("main.rs", "pub fn after_() {}\n");
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old_modified))
        .unwrap();
    invalidate_mcp_connection_cache(&server.db).unwrap();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"][0]["name"], "after_");
    assert_ne!(before["corpus_hash"], after["corpus_hash"]);
    assert_ne!(before["output_root"], after["output_root"]);
}

#[test]
fn linked_source_edits_and_renames_refresh_in_same_server() {
    let ws = Workspace::new();
    let linked = Workspace::new();
    linked.write("lib.rs", "pub fn before() {}\n");
    ws.write(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: library\n    path: {}\n",
            linked.root().display()
        ),
    );
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
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    ws.write("main.rs", "pub fn after() {}\n");
    let delta = db::writer::delta(ws.root(), &server.db, &[PathBuf::from("main.rs")]).unwrap();
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

    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let first = in_process_server(&ws);
    let second = in_process_server(&ws);
    assert_eq!(snapshot(&first).unwrap()["nodes"][0]["name"], "before");
    assert_eq!(snapshot(&second).unwrap()["nodes"][0]["name"], "before");
    assert!(first.connection.lock().unwrap().is_some());
    assert!(second.connection.lock().unwrap().is_some());
    let inode = fs::metadata(&first.db).unwrap().ino();

    ws.write("main.rs", "pub fn after() {}\n");
    let report = db::writer::build(ws.root(), &first.db, None).unwrap();

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

    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let inode = fs::metadata(&server.db).unwrap().ino();

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        server.read(|conn| {
            let name: String =
                conn.query_row("SELECT name FROM nodes WHERE kind='function'", [], |row| {
                    row.get(0)
                })?;
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(json!({"name": name}))
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    ws.write("main.rs", "pub fn after() {}\n");

    let root = ws.root().to_path_buf();
    let db = in_process_server(&ws).db;
    let builder = std::thread::spawn(move || db::writer::build(&root, &db, None));
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
    let ws = Workspace::new();
    for i in 0..64 {
        ws.write(
            format!("src/file_{i}.rs"),
            &format!("pub fn value_{i}() {{}}\n"),
        );
    }
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

#[test]
fn failed_query_keeps_next_read_usable_and_database_changes_are_fenced() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    assert!(server
        .read(|_| anyhow::bail!("intentional query failure"))
        .is_err());
    snapshot(&server).unwrap();
    let err = server
        .read(|_| {
            let conn = rusqlite::Connection::open(&server.db)?;
            conn.execute("UPDATE nodes SET name='tampered' WHERE kind='function'", [])?;
            Ok(json!({"should_not_escape": true}))
        })
        .unwrap_err();
    assert!(err.to_string().contains("generation changed"));
    assert!(snapshot(&server).is_err());
}

#[test]
fn foreign_and_corrupt_databases_remain_rejected_without_replacement() {
    let ws = Workspace::new();
    let other = Workspace::new();
    ws.write("main.rs", "pub fn original() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let foreign = Server::new(other.root().to_path_buf(), server.db.clone());
    assert!(snapshot(&foreign)
        .unwrap_err()
        .to_string()
        .contains("different workspace"));
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());
    drop(server);
    fs::write(ws.path(".forge/index.sqlite"), b"corrupt database").unwrap();
    assert!(snapshot(&in_process_server(&ws)).is_err());
    assert_eq!(
        fs::read(ws.path(".forge/index.sqlite")).unwrap(),
        b"corrupt database"
    );
}

#[test]
fn a_cached_connection_does_not_admit_another_workspace_with_the_same_adapter_digest() {
    let ws = Workspace::new();
    let other = Workspace::new();
    ws.write("main.rs", "pub fn original() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let mut foreign = server.clone();
    foreign.root = other.root().to_path_buf();
    assert!(snapshot(&foreign)
        .unwrap_err()
        .to_string()
        .contains("different workspace"));
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());
}

#[test]
fn cached_header_admission_still_rejects_corrupt_schema_replacement() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn original() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let conn = rusqlite::Connection::open(&server.db).unwrap();
    conn.execute(
        "UPDATE metadata SET value='0' WHERE key='schema_version'",
        [],
    )
    .unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(conn);
    let mut damaged = fs::read(&server.db).unwrap();
    damaged[36..40].copy_from_slice(&1_u32.to_be_bytes());
    fs::write(&server.db, &damaged).unwrap();
    assert!(snapshot(&server).is_err());
    assert_eq!(fs::read(&server.db).unwrap(), damaged);
}

#[test]
fn stdio_rebuild_admission_preserves_corruption_and_rebuilds_valid_legacy_index() {
    for (mismatch, corruption) in [
        ("schema", "freelist"),
        ("schema", "fts_data"),
        ("semantics", "fts_data"),
        ("adapter", "freelist"),
    ] {
        let ws = Workspace::new();
        ws.write("main.rs", "pub fn current() {}\n");
        ws.write("README.md", "# Current\nIndexed documentation.\n");
        let database = ws.db();
        db::writer::build(ws.root(), &database, None).unwrap();

        if mismatch == "adapter" {
            ws.write("forge-mcp.yaml", "adapter_version: 2\n");
        } else {
            let conn = rusqlite::Connection::open(&database).unwrap();
            let key = match mismatch {
                "schema" => "schema_version",
                "semantics" => "index_semantics_version",
                _ => unreachable!(),
            };
            conn.execute("UPDATE metadata SET value='0' WHERE key=?1", [key])
                .unwrap();
        }

        let mut damaged = fs::read(&database).unwrap();
        if corruption == "freelist" {
            damaged[36..40].copy_from_slice(&1_u32.to_be_bytes());
        } else {
            let conn = rusqlite::Connection::open(&database).unwrap();
            let page_size = conn
                .query_row("PRAGMA page_size", [], |row| row.get::<_, usize>(0))
                .unwrap();
            let root_page = conn
                .query_row(
                    "SELECT rootpage FROM sqlite_schema WHERE name='doc_search_data'",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .unwrap();
            damaged[(root_page - 1) * page_size] = 0xff;
        }
        fs::write(&database, &damaged).unwrap();
        let before = fs::read(&database).unwrap();

        let mut client = StdioClient::new(&ws);
        let response = client.request_value(
            "tools/call",
            json!({"name":"code_map_overview","arguments":{"detail":"compact"}}),
        );
        assert_eq!(
            response["result"]["isError"], true,
            "{mismatch}/{corruption}: {response}"
        );
        assert_eq!(
            fs::read(&database).unwrap(),
            before,
            "{mismatch}/{corruption}"
        );
    }

    let empty = Workspace::new();
    empty.write("main.rs", "pub fn current() {}\n");
    let database = empty.db();
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::write(&database, []).unwrap();
    let before = fs::read(&database).unwrap();
    let mut client = StdioClient::new(&empty);
    let response = client.request_value(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact","format":"json"}}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(fs::read(&database).unwrap(), before);

    let legacy = Workspace::new();
    legacy.write("main.rs", "pub fn current() {}\n");
    let database = legacy.db();
    db::writer::build(legacy.root(), &database, None).unwrap();
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("DROP TABLE metadata")
        .unwrap();

    let mut client = StdioClient::new(&legacy);
    let response = client.request_value(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact","format":"json"}}),
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let overview: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(overview["freshness"]["refresh"], "rebuild");
    assert!(db::reader::open(&database, legacy.root()).is_ok());
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
    assert!(snapshot(&Server::new(ws.root().to_path_buf(), alias)).is_err());
    assert_eq!(identity, db::cache::identity(&original).unwrap());
}

#[test]
fn newly_created_configured_root_is_admitted() {
    let ws = Workspace::new();
    ws.write("forge-mcp.yaml", "roots:\n  - src\n");
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
    let ws = Workspace::new();
    fs::create_dir(ws.path(".git")).unwrap();
    ws.write("visible.rs", "pub fn visible() {}\n");
    ws.write("ignored.rs", "pub fn ignored() {}\n");
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

#[test]
fn changed_adapter_does_not_replace_a_corrupt_current_index() {
    let ws = Workspace::new();
    ws.write("src/main.rs", "pub fn original() {}\n");
    let server = in_process_server(&ws);
    snapshot(&server).unwrap();
    let db_path = server.db.clone();
    drop(server);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("UPDATE nodes SET name='tampered' WHERE kind='function'", [])
        .unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(conn);
    let identity = db::cache::identity(&db_path).unwrap();
    ws.write("forge-mcp.yaml", "roots:\n  - src\n");
    assert!(snapshot(&in_process_server(&ws)).is_err());
    assert_eq!(identity, db::cache::identity(&db_path).unwrap());
}
