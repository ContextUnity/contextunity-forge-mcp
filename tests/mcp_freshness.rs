#![cfg(feature = "lang-rust")]

use contextunity_forge_mcp::{db, engine::scanner, mcp::server::Server};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge_freshness_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, source: &str) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, source).unwrap();
    }
    fn server(&self) -> Server {
        Server::new(self.0.clone(), self.0.join(".forge/index.sqlite"))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn snapshot(server: &Server) -> anyhow::Result<Value> {
    server.read(|conn| {
        Ok(json!({
            "nodes": db::reader::rows(conn, "SELECT name,path,line,end_line FROM nodes WHERE kind='function' ORDER BY path,name", &[], 100)?,
            "corpus_hash": conn.query_row("SELECT value FROM metadata WHERE key='corpus_hash'", [], |row| row.get::<_, String>(0))?,
            "output_root": conn.query_row("SELECT value FROM metadata WHERE key='output_root'", [], |row| row.get::<_, String>(0))?,
        }))
    })
}

fn wait_for_inventory_ttl() {
    std::thread::sleep(std::time::Duration::from_millis(5100));
}

#[cfg(feature = "lang-python")]
#[test]
fn same_server_refreshes_non_source_manifests_outside_source_roots() {
    let ws = Workspace::new();
    ws.write("forge-mcp.yaml", "roots: [src]\n");
    ws.write(
        "src/consumer.py",
        "import novel_library\ndef run(): return novel_library.fetch()\n",
    );
    let server = ws.server();
    let evidence = || {
        server.read(|conn| Ok(json!({
        "evidence": conn.query_row("SELECT evidence FROM resolution_coverage WHERE expression='novel_library'", [], |row| row.get::<_, String>(0))?,
        "manifest_digest": conn.query_row("SELECT value FROM metadata WHERE key='manifest_digest'", [], |row| row.get::<_, String>(0))?,
    }))).unwrap()
    };
    let before = evidence();
    assert!(!before["evidence"].as_str().unwrap().contains("manifest"));
    ws.write("requirements-dev.txt", "novel_library>=1\n");
    wait_for_inventory_ttl();
    let after = evidence();
    assert!(after["evidence"].as_str().unwrap().contains("manifest"));
    assert_ne!(before["manifest_digest"], after["manifest_digest"]);
    assert_eq!(after["freshness"]["refresh"], "rebuild");
    fs::remove_file(ws.0.join("requirements-dev.txt")).unwrap();
    wait_for_inventory_ttl();
    let removed = evidence();
    assert_eq!(removed["manifest_digest"], before["manifest_digest"]);
    assert!(!removed["evidence"].as_str().unwrap().contains("manifest"));
}

#[test]
fn same_server_metadata_query_refreshes_edited_source() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {\n    let value = 1;\n}\n");
    let server = ws.server();
    let before = snapshot(&server).unwrap();
    assert_eq!(before["nodes"][0]["end_line"], 3);
    ws.write(
        "main.rs",
        "pub fn current() {\n    let value = 1;\n    let next = value + 1;\n}\n",
    );
    wait_for_inventory_ttl();
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
    let server = ws.server();
    let first = snapshot(&server).unwrap();
    let receipt = PathBuf::from(format!("{}.verified.json", server.db.display()));
    assert!(receipt.exists());
    fs::remove_file(&receipt).unwrap();

    let reopened = snapshot(&ws.server()).unwrap();
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
    let server = ws.server();
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
        &ws.0,
        first["output_root"].as_str().unwrap(),
        &verified
    )
    .unwrap());
    assert!(!PathBuf::from(format!("{}.verified.json", database.display())).exists());
    assert!(db::reader::open(&database, &ws.0).is_err());
}

#[test]
fn failed_delta_keeps_receipt_for_rolled_back_index() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = ws.server();
    let first = snapshot(&server).unwrap();
    let database = server.db.clone();
    drop(server);
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_file_delete BEFORE DELETE ON files BEGIN SELECT RAISE(ABORT, 'forced delta rollback'); END;",
    )
    .unwrap();
    drop(conn);
    assert!(
        db::cache::publish_verified(&database, &ws.0, first["output_root"].as_str().unwrap())
            .unwrap()
    );
    let receipt = PathBuf::from(format!("{}.verified.json", database.display()));

    ws.write("main.rs", "pub fn after_() {}\n");
    let error = db::writer::delta(&ws.0, &database, &[PathBuf::from("main.rs")]).unwrap_err();
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
        &ws.0,
        first["output_root"].as_str().unwrap(),
        &db::cache::identity(&database).unwrap()
    ));
}

#[test]
fn same_server_refreshes_add_delete_and_rename() {
    let ws = Workspace::new();
    ws.write("first.rs", "pub fn first() {}\n");
    let server = ws.server();
    snapshot(&server).unwrap();
    ws.write("added.rs", "pub fn added() {}\n");
    wait_for_inventory_ttl();
    let added = snapshot(&server).unwrap();
    assert_eq!(added["nodes"].as_array().unwrap().len(), 2);
    fs::rename(ws.0.join("added.rs"), ws.0.join("renamed.rs")).unwrap();
    wait_for_inventory_ttl();
    let renamed = snapshot(&server).unwrap();
    assert_eq!(renamed["nodes"][1]["path"], "renamed.rs");
    fs::remove_file(ws.0.join("first.rs")).unwrap();
    wait_for_inventory_ttl();
    let deleted = snapshot(&server).unwrap();
    assert_eq!(deleted["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(deleted["nodes"][0]["name"], "added");
}

#[test]
fn restored_mtime_and_same_length_edit_refreshes_digest() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = ws.server();
    let before = snapshot(&server).unwrap();
    let path = ws.0.join("main.rs");
    let old_modified = fs::metadata(&path).unwrap().modified().unwrap();
    ws.write("main.rs", "pub fn after_() {}\n");
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old_modified))
        .unwrap();
    wait_for_inventory_ttl();
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
            linked.0.display()
        ),
    );
    let server = ws.server();
    let before = snapshot(&server).unwrap();
    assert_eq!(before["nodes"][0]["path"], "[library]/lib.rs");
    linked.write("lib.rs", "pub fn changed() {}\n");
    wait_for_inventory_ttl();
    assert_eq!(snapshot(&server).unwrap()["nodes"][0]["name"], "changed");
    fs::rename(linked.0.join("lib.rs"), linked.0.join("next.rs")).unwrap();
    wait_for_inventory_ttl();
    assert_eq!(
        snapshot(&server).unwrap()["nodes"][0]["path"],
        "[library]/next.rs"
    );
    fs::remove_file(linked.0.join("next.rs")).unwrap();
    wait_for_inventory_ttl();
    assert!(snapshot(&server).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn external_delta_invalidates_cached_connection_without_another_write() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn before() {}\n");
    let server = ws.server();
    snapshot(&server).unwrap();
    ws.write("main.rs", "pub fn after() {}\n");
    let delta = db::writer::delta(&ws.0, &server.db, &[PathBuf::from("main.rs")]).unwrap();
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
    let first = ws.server();
    let second = ws.server();
    assert_eq!(snapshot(&first).unwrap()["nodes"][0]["name"], "before");
    assert_eq!(snapshot(&second).unwrap()["nodes"][0]["name"], "before");
    assert!(first.connection.lock().unwrap().is_some());
    assert!(second.connection.lock().unwrap().is_some());
    let inode = fs::metadata(&first.db).unwrap().ino();

    ws.write("main.rs", "pub fn after() {}\n");
    let report = db::writer::build(&ws.0, &first.db, None).unwrap();

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
    let server = ws.server();
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

    let root = ws.0.clone();
    let db = ws.server().db;
    let builder = std::thread::spawn(move || db::writer::build(&root, &db, None));
    std::thread::sleep(Duration::from_millis(500));
    let inode_while_reading = fs::metadata(ws.server().db).unwrap().ino();
    release_tx.send(()).unwrap();
    assert_eq!(reader.join().unwrap().unwrap()["name"], "before");
    builder.join().unwrap().unwrap();

    assert_eq!(inode_while_reading, inode);
    assert_ne!(fs::metadata(ws.server().db).unwrap().ino(), inode);
    assert_eq!(snapshot(&ws.server()).unwrap()["nodes"][0]["name"], "after");
}

#[test]
fn unchanged_warm_reads_and_metadata_only_touch_do_not_rewrite_database() {
    let ws = Workspace::new();
    for i in 0..64 {
        ws.write(
            &format!("src/file_{i}.rs"),
            &format!("pub fn value_{i}() {{}}\n"),
        );
    }
    let server = ws.server();
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
        .open(ws.0.join("src/file_0.rs"))
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
    let server = ws.server();
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
    let server = ws.server();
    snapshot(&server).unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let foreign = Server::new(other.0.clone(), server.db.clone());
    assert!(snapshot(&foreign)
        .unwrap_err()
        .to_string()
        .contains("different workspace"));
    assert_eq!(identity, db::cache::identity(&server.db).unwrap());
    drop(server);
    fs::write(ws.0.join(".forge/index.sqlite"), b"corrupt database").unwrap();
    assert!(snapshot(&ws.server()).is_err());
    assert_eq!(
        fs::read(ws.0.join(".forge/index.sqlite")).unwrap(),
        b"corrupt database"
    );
}

#[test]
fn a_cached_connection_does_not_admit_another_workspace_with_the_same_adapter_digest() {
    let ws = Workspace::new();
    let other = Workspace::new();
    ws.write("main.rs", "pub fn original() {}\n");
    let server = ws.server();
    snapshot(&server).unwrap();
    let identity = db::cache::identity(&server.db).unwrap();
    let mut foreign = server.clone();
    foreign.root = other.0.clone();
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
    let server = ws.server();
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
        let database = ws.0.join(".forge/code-map.sqlite");
        db::writer::build(&ws.0, &database, None).unwrap();

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
        let response = client.request(
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
    let database = empty.0.join(".forge/code-map.sqlite");
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::write(&database, []).unwrap();
    let before = fs::read(&database).unwrap();
    let mut client = StdioClient::new(&empty);
    let response = client.request(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact"}}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(fs::read(&database).unwrap(), before);

    let legacy = Workspace::new();
    legacy.write("main.rs", "pub fn current() {}\n");
    let database = legacy.0.join(".forge/code-map.sqlite");
    db::writer::build(&legacy.0, &database, None).unwrap();
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("DROP TABLE metadata")
        .unwrap();

    let mut client = StdioClient::new(&legacy);
    let response = client.request(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact"}}),
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let overview: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(overview["freshness"]["refresh"], "rebuild");
    assert!(db::reader::open(&database, &legacy.0).is_ok());
}

#[test]
#[cfg(unix)]
fn symlink_database_is_rejected_without_touching_target() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn original() {}\n");
    let server = ws.server();
    snapshot(&server).unwrap();
    let original = server.db.clone();
    let alias = ws.0.join("alias.sqlite");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let identity = db::cache::identity(&original).unwrap();
    assert!(snapshot(&Server::new(ws.0.clone(), alias)).is_err());
    assert_eq!(identity, db::cache::identity(&original).unwrap());
}

#[test]
fn newly_created_configured_root_is_admitted() {
    let ws = Workspace::new();
    ws.write("forge-mcp.yaml", "roots:\n  - src\n");
    let server = ws.server();
    assert!(snapshot(&server).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .is_empty());
    ws.write("src/created.rs", "pub fn created() {}\n");
    wait_for_inventory_ttl();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"][0]["name"], "created");
    assert_eq!(after["freshness"]["refresh"], "rebuild");
}

#[test]
fn changed_ignore_file_removes_existing_source_from_inventory() {
    let ws = Workspace::new();
    fs::create_dir(ws.0.join(".git")).unwrap();
    ws.write("visible.rs", "pub fn visible() {}\n");
    ws.write("ignored.rs", "pub fn ignored() {}\n");
    let server = ws.server();
    assert_eq!(
        snapshot(&server).unwrap()["nodes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    ws.write(".gitignore", "ignored.rs\n");
    wait_for_inventory_ttl();
    let after = snapshot(&server).unwrap();
    assert_eq!(after["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(after["nodes"][0]["name"], "visible");
    assert_eq!(after["freshness"]["refresh"], "rebuild");
}

#[test]
fn changed_adapter_does_not_replace_a_corrupt_current_index() {
    let ws = Workspace::new();
    ws.write("src/main.rs", "pub fn original() {}\n");
    let server = ws.server();
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
    assert!(snapshot(&ws.server()).is_err());
    assert_eq!(identity, db::cache::identity(&db_path).unwrap());
}

struct StdioClient {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    id: usize,
}

#[test]
fn mcp_document_retrieval_and_explain_output_controls() {
    let ws = Workspace::new();
    ws.write(
        "main.rs",
        "pub fn target() {}\npub fn caller() { target(); }\n",
    );
    ws.write(
        "README.md",
        "# Contract\n\nThe `target` function preserves the documented contract.\n",
    );
    let mut client = StdioClient::new(&ws);
    let document = client.document(json!({"path_or_id":"README.md"}));
    assert!(document["sections"]["items"][0]["content"]
        .as_str()
        .unwrap()
        .contains("documented contract"));
    let outline = client.document(json!({"path_or_id":"README.md","detail":"compact"}));
    assert!(outline["sections"]["items"][0].get("content").is_none());
    assert_eq!(document["sections"]["total"], outline["sections"]["total"]);
    let mut explain = |arguments: Value| {
        let response = client.request(
            "tools/call",
            json!({"name":"code_map_explain","arguments":arguments}),
        );
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str::<Value>(response["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap()
    };
    let with_docs = explain(json!({"selector":"target"}));
    assert!(with_docs["documents"]["total"].as_u64().unwrap() > 0);
    let without_docs = explain(json!({"selector":"target","show_doc":false}));
    assert!(without_docs.get("documents").is_none() || without_docs["documents"]["total"] == 0);
    assert_eq!(with_docs["coverage"], without_docs["coverage"]);
    assert_eq!(with_docs["incoming"], without_docs["incoming"]);
    for (alias, canonical, omitted) in [
        ("inbound", "incoming", "outgoing"),
        ("outbound", "outgoing", "incoming"),
    ] {
        let alias_result = explain(json!({"selector":"target","direction":alias}));
        let canonical_result = explain(json!({"selector":"target","direction":canonical}));
        assert_eq!(alias_result["direction"], canonical);
        assert_eq!(alias_result[canonical], canonical_result[canonical]);
        assert_eq!(alias_result[omitted]["omitted"], true);
    }
}
impl StdioClient {
    fn new(ws: &Workspace) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", ws.0.to_str().unwrap(), "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || sender.send(line).is_err() {
                break;
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
        };
        let init = client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"freshness-test","version":"1"}}));
        assert!(init.get("error").is_none(), "{init}");
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client.input.flush().unwrap();
        client
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let line = self
                .output
                .recv_timeout(Duration::from_secs(30))
                .expect("MCP response missing or timed out");
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == self.id {
                return response;
            }
        }
    }
    fn document(&mut self, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({"name":"get_doc","arguments":arguments}),
        );
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn all_mcp_tools_admit_large_inventory_snapshot_without_raising_query_limit() {
    let ws = Workspace::new();
    ws.write("main.rs", "pub fn current() {}\n");
    ws.write("docs/intro.md", "# Intro\n\nCurrent documentation.\n");
    let database = ws.0.join(".forge/code-map.sqlite");
    db::writer::build(&ws.0, &database, None).unwrap();

    let conn = rusqlite::Connection::open(&database).unwrap();
    let inventory: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='inventory_snapshot'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let padded = format!("{inventory}{}", " ".repeat(8 * 1024 * 1024));
    conn.execute(
        "UPDATE metadata SET value=?1 WHERE key='inventory_snapshot'",
        [&padded],
    )
    .unwrap();
    contextunity_forge_mcp::core::commitments::seal(&conn).unwrap();
    drop(conn);

    let mut client = StdioClient::new(&ws);
    let calls = [
        ("code_map_overview", json!({"detail":"compact","limit":1})),
        ("code_map_inspect", json!({"selector":"main.rs:current"})),
        ("get_code_snippet", json!({"selector":"main.rs:current"})),
        (
            "code_map_search",
            json!({"pattern":"current","detail":"compact","limit":10}),
        ),
        ("code_map_tests", json!({"selector":"main.rs:current"})),
        (
            "code_map_impact",
            json!({"selector":"main.rs:current","depth":1}),
        ),
        ("code_map_explain", json!({"selector":"main.rs:current"})),
        ("code_map_query", json!({"operation":"overview"})),
        ("code_map_analyze", json!({"target":""})),
        (
            "code_map_prove_removal",
            json!({"selector":"main.rs:current"}),
        ),
        (
            "ast_grep_search",
            json!({"pattern":"pub fn current() {}","language":"rust","path":"main.rs"}),
        ),
        ("search_docs", json!({"query":"documentation"})),
        ("get_doc", json!({"path_or_id":"docs/intro.md"})),
        ("forge_guide", json!({"topic":"query"})),
        ("session_checkpoint", json!({"action":"list"})),
    ];
    for (name, arguments) in calls {
        let response = client.request("tools/call", json!({"name":name,"arguments":arguments}));
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        if name == "get_code_snippet" {
            let snippet: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert!(snippet.get("source").is_some(), "{snippet}");
            assert!(snippet.get("coverage").is_none(), "{snippet}");
            assert!(snippet.get("documents").is_none(), "{snippet}");
        }
        if name == "code_map_overview" {
            let overview: Value =
                serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(overview["counts"]["files"], 2);
            assert!(overview["freshness"].get("files_checked").is_none() || overview["freshness"]["files_checked"] == 2);
        }
    }

    ws.write("main.rs", "pub fn current() { let value = 1; }\n");
    wait_for_inventory_ttl();
    let response = client.request(
        "tools/call",
        json!({"name":"code_map_overview","arguments":{"detail":"compact","limit":1}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    let refreshed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(refreshed["freshness"]["refresh"], "delta");

    let server = Server::new(ws.0.clone(), database);
    server
        .read(|conn| {
            assert_eq!(
                conn.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH),
                8 * 1024 * 1024
            );
            Ok(Value::Null)
        })
        .unwrap();
}

#[test]
fn stdio_response_only_adapter_transitions_preserve_facts_and_continuations() {
    let ws = Workspace::new();
    ws.write(
        "src/README.md",
        "# A\none\n# B\ntwo\n# C\nthree\n# D\nfour\n",
    );
    ws.write("src/helper.md", "# Helper\ninside\n");
    ws.write("outside.md", "# Outside\noutside\n");
    let mut client = StdioClient::new(&ws);
    let before = client.document(json!({"path_or_id":"src/README.md"}));
    let database = ws.0.join(".forge/code-map.sqlite");
    let identity = db::cache::identity(&database).unwrap();
    let database_hash = || scanner::digest(&fs::read(&database).unwrap());
    let before_hash = database_hash();
    let generation = before["sections"]["generation"].clone();
    assert!(generation.is_string(), "{before}");
    assert_eq!(before["freshness"]["files_checked"], 3);
    let default_limit = before["sections"]["limit"].as_u64().unwrap();
    for (configuration, limit) in [
        (Some("response: {page_size: 1}\n"), 1),
        (Some("response: {page_size: 2}\n"), 2),
        (None, default_limit),
        (Some("{}\n"), default_limit),
        (None, default_limit),
    ] {
        if let Some(configuration) = configuration {
            ws.write("forge-mcp.yaml", configuration);
        } else {
            fs::remove_file(ws.0.join("forge-mcp.yaml")).unwrap();
        }
        let current = client.document(json!({"path_or_id":"src/README.md"}));
        assert_eq!(current["sections"]["limit"], limit);
        assert_eq!(
            current["sections"]["items"].as_array().unwrap().len(),
            limit.min(4) as usize
        );
        assert!(
            current["freshness"].get("refresh").is_none() || current["freshness"]["refresh"] == "none",
            "{configuration:?}: {current}"
        );
        assert_eq!(current["sections"]["generation"], generation);
        for key in ["corpus_hash", "output_root"] {
            if let Some(expected) = before["freshness"].get(key) {
                if let Some(actual) = current["freshness"].get(key) {
                    assert_eq!(actual, expected);
                }
            }
        }
        let continued = client.document(
            json!({"path_or_id":"src/README.md","offset":1,"limit":1,"generation":generation}),
        );
        assert_eq!(continued["sections"]["offset"], 1);
        assert_eq!(continued["sections"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(continued["sections"]["generation"], generation);
        assert_eq!(database_hash(), before_hash);
        assert_eq!(db::cache::identity(&database).unwrap(), identity);
    }
    for invalid in [
        "response: {page_size: 0}\n",
        "response: [\n",
        "response: {page_size: 2}\nroots: ['..']\n",
    ] {
        ws.write("forge-mcp.yaml", invalid);
        let error = client.request(
            "tools/call",
            json!({"name":"get_doc","arguments":{"path_or_id":"src/README.md"}}),
        );
        assert_eq!(error["result"]["isError"], true, "{invalid}: {error}");
        assert_eq!(database_hash(), before_hash);
        assert_eq!(db::cache::identity(&database).unwrap(), identity);
    }
    ws.write("forge-mcp.yaml", "response: {page_size: 1}\n");
    let recovered =
        client.document(json!({"path_or_id":"src/README.md","offset":1,"generation":generation}));
    assert!(
        recovered["freshness"].get("refresh").is_none() || recovered["freshness"]["refresh"] == "none"
    );
    assert_eq!(recovered["sections"]["generation"], generation);
    assert_eq!(database_hash(), before_hash);

    let mut previous_generation = generation;
    for (settings, files) in [
        ("roots: [src]\n", 2),
        ("roots: [src]\nignore: [helper.md]\n", 1),
        ("roots: [src]\nignore: [helper.md]\nadapter_version: 2\n", 1),
    ] {
        ws.write(
            "forge-mcp.yaml",
            &format!("{settings}response: {{page_size: 1}}\n"),
        );
        let changed = client.document(json!({"path_or_id":"src/README.md"}));
        assert_eq!(
            changed["freshness"]["refresh"], "rebuild",
            "{settings}: {changed}"
        );
        assert_eq!(changed["freshness"]["files_checked"], files);
        assert_eq!(changed["sections"]["limit"], 1);
        assert_ne!(changed["sections"]["generation"], previous_generation);
        previous_generation = changed["sections"]["generation"].clone();
    }
}
