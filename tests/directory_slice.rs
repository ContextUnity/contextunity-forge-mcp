#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::{reader, traversal, writer},
};
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_directory_slice_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, source: &str) {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }

    fn build(&self) -> Connection {
        let db = self.0.join(".forge/code-map.sqlite");
        writer::build(&self.0, &db, None).unwrap();
        reader::open(&db, &self.0).unwrap()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options(limit: usize) -> QueryOptions {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None).unwrap()
}

#[test]
fn directory_suffix_lists_real_nodes_and_exact_file_keeps_graph_traversal() {
    let ws = Workspace::new();
    let file = "extensions/commerce/src/contextunity/commerce/ingestion/worker.py";
    ws.write(file, "def publish():\n    return 1\n");
    ws.write(
        "extensions/commerce/src/contextunity/commerce/connectors/horoshop/client.py",
        "def fetch(): pass\n",
    );
    ws.write(
        "extensions/commerce/src/contextunity/commerce/pim/views/products/editor/views.py",
        "def edit(): pass\n",
    );
    let conn = ws.build();
    let result = traversal::query_paged(
        &conn,
        "slice",
        Some("contextunity/commerce/ingestion"),
        1,
        &options(20),
    )
    .unwrap();
    assert_eq!(result["scope"], "directory_members");
    assert_eq!(
        result["selector"]["path"],
        "extensions/commerce/src/contextunity/commerce/ingestion"
    );
    assert_eq!(result["requested_depth"], 1);
    assert_eq!(result["graph_depth_applied"], 0);
    assert!(result["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["path"] == file));
    for (selector, expected) in [
        (
            "contextunity/commerce/connectors/horoshop",
            "extensions/commerce/src/contextunity/commerce/connectors/horoshop",
        ),
        (
            "contextunity/commerce/pim/views/products/editor",
            "extensions/commerce/src/contextunity/commerce/pim/views/products/editor",
        ),
    ] {
        let scoped =
            traversal::query_paged(&conn, "slice", Some(selector), 1, &options(20)).unwrap();
        assert_eq!(scoped["selector"]["path"], expected);
        assert_eq!(scoped["nodes"]["total"], 2);
    }
    let exact = traversal::query_paged(&conn, "slice", Some(file), 1, &options(20)).unwrap();
    assert_eq!(exact["depth"], 1);
    assert!(exact.get("scope").is_none());
}

#[test]
fn duplicate_directory_suffix_requires_full_path_and_unknown_path_suggests_indexed_file() {
    let ws = Workspace::new();
    ws.write(
        "extensions/commerce/src/contextunity/commerce/ingestion/a.py",
        "def a(): pass\n",
    );
    ws.write(
        "services/api/src/contextunity/commerce/ingestion/b.py",
        "def b(): pass\n",
    );
    let conn = ws.build();
    let error = traversal::query_paged(
        &conn,
        "slice",
        Some("contextunity/commerce/ingestion"),
        1,
        &options(20),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("ambiguous directory selector"), "{error}");
    assert!(
        error.contains("extensions/commerce/src/contextunity/commerce/ingestion"),
        "{error}"
    );
    assert!(
        error.contains("services/api/src/contextunity/commerce/ingestion"),
        "{error}"
    );
    let exact = traversal::query_paged(
        &conn,
        "slice",
        Some("extensions/commerce/src/contextunity/commerce/ingestion"),
        1,
        &options(20),
    )
    .unwrap();
    assert_eq!(exact["nodes"]["total"], 2);
    let unknown = traversal::query_paged(
        &conn,
        "slice",
        Some("contextunity/commerce/ingest"),
        1,
        &options(20),
    )
    .unwrap_err()
    .to_string();
    assert!(unknown.contains("indexed path suggestions"), "{unknown}");
    assert!(unknown.contains("ingestion/a.py"), "{unknown}");
}

#[test]
fn large_directory_is_paged_without_graph_expansion() {
    let ws = Workspace::new();
    let mut code = String::new();
    for index in 0..1100 {
        code.push_str(&format!("def item_{index:04}(): pass\n"));
    }
    ws.write("pkg/large/items.py", &code);
    let conn = ws.build();
    let started = Instant::now();
    let first = traversal::query_paged(&conn, "slice", Some("pkg/large"), 16, &options(1)).unwrap();
    assert_eq!(first["nodes"]["total"], 1101);
    assert_eq!(first["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["nodes"]["has_more"], true);
    assert_eq!(first["graph_depth_applied"], 0);
    println!(
        "directory_slice_large_query_ms={:.3}",
        started.elapsed().as_secs_f64() * 1000.0
    );
}
