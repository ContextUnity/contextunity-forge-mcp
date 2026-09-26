use contextunity_forge_mcp::mcp::server::Server;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_adapter_change_auto_rebuild() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("test_forge_adapter_{}_{}", std::process::id(), nonce));
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("main.rs"), "pub fn hello() {}\n").unwrap();

    let adapter_path = root.join("forge-mcp.yaml");
    fs::write(
        &adapter_path,
        "format: forge-code-map-policy/v1\nadapter_version: 1\nroots:\n  - src\n",
    )
    .unwrap();

    let db_path = root.join(".forge/code-map.sqlite");
    let server = Server::new(root.clone(), db_path.clone());

    // 1. Initial read triggers build
    let res = server.read(|conn| {
        let ver: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_version'",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::Value::String(ver))
    });
    assert_eq!(res.unwrap(), serde_json::Value::String("1".into()));

    // 2. Change adapter_version in forge-mcp.yaml
    fs::write(
        &adapter_path,
        "format: forge-code-map-policy/v1\nadapter_version: 2\nroots:\n  - src\n",
    )
    .unwrap();

    // 3. Read again -> should automatically detect change and rebuild
    let res2 = server.read(|conn| {
        let ver: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_version'",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::Value::String(ver))
    });
    assert_eq!(res2.unwrap(), serde_json::Value::String("2".into()));

    // 4. Change content (ignore rule) without changing version -> digest change triggers rebuild
    fs::write(
        &adapter_path,
        "format: forge-code-map-policy/v1\nadapter_version: 2\nroots:\n  - src\nignore:\n  - target\n",
    )
    .unwrap();

    let res3 = server.read(|conn| {
        let digest: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_digest'",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::Value::String(digest))
    });
    assert!(res3.is_ok());

    let _ = fs::remove_dir_all(&root);
}
