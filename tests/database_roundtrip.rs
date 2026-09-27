#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::db::{reader, writer};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_database_build_and_query_roundtrip() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("test_forge_{}_{}", std::process::id(), nonce));
    let workspace = dir.join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace failed");

    // Add a python file
    fs::write(
        workspace.join("main.py"),
        r#"
class Controller:
    def handle_request(self):
        return helper()

def helper():
    return 42
"#,
    )
    .expect("write python file failed");

    // Add a markdown file
    fs::write(
        workspace.join("README.md"),
        r#"# Workspace Documentation
## Standards
All handlers must be asynchronous and tested.
"#,
    )
    .expect("write markdown file failed");

    let db_path = dir.join("code_map.sqlite");

    // Build cold code map
    let report = writer::build(&workspace, &db_path, None).expect("cold build failed");
    assert!(report["nodes"].as_i64().unwrap_or(0) > 0);
    assert!(report["files"].as_i64().unwrap_or(0) >= 2);

    // Open reader and test overview
    let conn = reader::open(&db_path, &workspace).expect("reader open failed");
    let overview = reader::overview(&conn).expect("overview failed");
    assert!(overview["counts"][0]["files"].as_i64().unwrap_or(0) >= 2);

    // Test docs search
    let docs = reader::search_docs(&conn, "Standards", None, None, 10).expect("search docs failed");
    assert!(!docs["sections"].as_array().unwrap().is_empty());

    let _ = fs::remove_dir_all(&dir);
}
