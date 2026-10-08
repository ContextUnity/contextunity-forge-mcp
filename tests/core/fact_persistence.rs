use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::models::Facts,
    db::{reader, writer},
};
use rusqlite::Connection;
use std::path::PathBuf;

#[cfg(feature = "lang-python")]
#[test]
fn navigation_storage_preserves_compressed_analysis_and_delta_calls() {
    let ws = Workspace::new();
    ws.write("service.py", "class Service:\n    def execute(self):\n        \"\"\"Execute the request.\"\"\"\n        return 42\n");
    ws.write("caller.py", "from service import Service\n\ndef run():\n    service = Service()\n    return service.execute()\n");
    let db_path = ws.root().join(".forge/code-map.sqlite");
    writer::build(ws.root(), &db_path, None).unwrap();
    let conn = reader::open(&db_path, ws.root()).unwrap();
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path='caller.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let facts: Facts =
        serde_json::from_slice(&zstd::stream::decode_all(blob.as_slice()).unwrap()).unwrap();
    let caller = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    assert!(caller.details.get("value_flow").is_some());
    let stored: String = conn
        .query_row(
            "SELECT details FROM nodes WHERE id=?1",
            [&caller.id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["signature"], caller.details["signature"]);
    let method: String = conn
        .query_row(
            "SELECT details FROM nodes WHERE name='execute'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let method: serde_json::Value = serde_json::from_str(&method).unwrap();
    assert!(method["signature"].as_str().unwrap().contains("execute"));
    assert!(method["doc"]
        .as_str()
        .unwrap()
        .contains("Execute the request"));
    assert_eq!(method["receiver_name"], "self");
    drop(conn);
    ws.write("service.py", "class Service:\n    def execute(self):\n        \"\"\"Execute the updated request.\"\"\"\n        return 43\n");
    writer::delta(ws.root(), &db_path, &[PathBuf::from("service.py")]).unwrap();
    let conn = reader::open(&db_path, ws.root()).unwrap();
    let calls: i64 = conn.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='run' AND d.name='execute' AND e.kind='calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        calls, 1,
        "cached complete local analysis supports receiver resolution during delta"
    );
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    let root: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "UPDATE local_facts SET facts_blob=?1 WHERE path='caller.py'",
        [&blob[..blob.len() / 2]],
    )
    .unwrap();
    drop(conn);
    ws.write(
        "service.py",
        "class Service:\n    def execute(self):\n        return 44\n",
    );
    assert!(
        writer::delta(ws.root(), &db_path, &[PathBuf::from("service.py")]).is_err(),
        "truncated durable facts fail closed before graph publication"
    );
    let conn = Connection::open(&db_path).unwrap();
    let retained_root: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_root, root);
    let retained_calls: i64 = conn.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='run' AND d.name='execute' AND e.kind='calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(retained_calls, calls);
}
