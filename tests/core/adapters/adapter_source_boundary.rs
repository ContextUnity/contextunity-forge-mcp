#![cfg(feature = "lang-yaml")]

use crate::common::Workspace;
use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::scanner,
    mcp::server::Server,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

#[test]
fn response_only_default_config_is_outside_source_inventory() {
    let workspace = Workspace::new();
    workspace.write("settings.yaml", "service: enabled\n");
    workspace.write("nested/forge-mcp.yaml", "service: nested\n");
    let database = workspace.path(".forge/index.sqlite");
    let server = Server::new(workspace.root().to_path_buf(), database.clone());
    let read = || {
        server.read(|conn| Ok(json!({"files": reader::rows(conn, "SELECT path FROM files ORDER BY path", &[], 20)?}))).unwrap()
    };
    let before = read();
    let before_bytes = fs::read(&database).unwrap();
    for content in [Some("response: {page_size: 1}\n"), Some("{}\n"), None] {
        if let Some(content) = content {
            workspace.write("forge-mcp.yaml", content);
        } else {
            fs::remove_file(workspace.path("forge-mcp.yaml")).unwrap();
        }
        let current = read();
        assert_eq!(current["files"], before["files"]);
        assert_eq!(current["freshness"]["refresh"], "none");
        assert_eq!(
            current["freshness"]["output_root"],
            before["freshness"]["output_root"]
        );
        assert_eq!(fs::read(&database).unwrap(), before_bytes);
    }
    assert_eq!(
        before["files"],
        json!([{"path":"nested/forge-mcp.yaml"},{"path":"settings.yaml"}])
    );
}

#[test]
fn custom_adapter_exclusion_survives_delta_and_linked_configs_remain_sources() {
    let workspace = Workspace::new();
    let linked = Workspace::new();
    linked.write("forge-mcp.yaml", "library: indexed\n");
    workspace.write("outside.yaml", "outside: excluded\n");
    workspace.write("src/settings.yaml", "service: before\n");
    let config = workspace.path("src/custom.yaml");
    let configuration = |size| {
        format!("roots: [src]\nresponse: {{page_size: {size}}}\nlinked_workspaces:\n  - name: library\n    path: {}\n", linked.root().display())
    };
    workspace.write("src/custom.yaml", &configuration(1));
    let database = workspace.path(".forge/index.sqlite");
    writer::build(workspace.root(), &database, Some(&config)).unwrap();
    workspace.write("src/custom.yaml", &configuration(2));
    workspace.write("src/settings.yaml", "service: after\n");
    let delta = writer::delta(
        workspace.root(),
        &database,
        &[PathBuf::from("src/settings.yaml")],
    )
    .unwrap();
    assert_eq!(delta["changed_files"], 1, "{delta}");
    let cold_path = workspace.path(".forge/cold.sqlite");
    writer::build(workspace.root(), &cold_path, Some(&config)).unwrap();
    let incremental = reader::open(&database, workspace.root()).unwrap();
    let cold = reader::open(&cold_path, workspace.root()).unwrap();
    for query in [
        "SELECT path,digest FROM files ORDER BY path",
        "SELECT n.id,n.kind,n.name,n.qualname,(SELECT path FROM path_dictionary WHERE path_id=n.path_id) AS path,n.line,n.end_line,n.details FROM nodes n ORDER BY n.id",
        "SELECT * FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
    ] {
        assert_eq!(
            reader::rows(&incremental, query, &[], 100).unwrap(),
            reader::rows(&cold, query, &[], 100).unwrap(),
            "{query}"
        );
    }
    let paths: Vec<Value> = reader::rows(
        &incremental,
        "SELECT path FROM files ORDER BY path",
        &[],
        20,
    )
    .unwrap();
    assert_eq!(
        json!(paths),
        json!([{"path":"[library]/forge-mcp.yaml"},{"path":"src/settings.yaml"}])
    );
    let adapter = scanner::load_adapter(workspace.root(), Some(&config)).unwrap();
    assert_eq!(adapter.adapter_path, Some(config));
}

#[cfg(feature = "lang-rust")]
#[test]
fn delta_framework_manifest_parse_error_fails_closed_before_publication() {
    let workspace = Workspace::new();
    workspace.write("forge-mcp.yaml", "roots: [src]\n");
    workspace.write("src/main.rs", "pub fn previous() -> u32 { 1 }\n");
    let database = workspace.path(".forge/index.sqlite");
    writer::build(workspace.root(), &database, None).unwrap();

    let indexed_source_state = |conn: &rusqlite::Connection| {
        let inventory_digest: String = conn
            .query_row(
                "SELECT digest FROM source_inventory WHERE path='src/main.rs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let file_state: (String, Option<String>, i64) = conn
            .query_row(
                "SELECT digest,language,size FROM files WHERE path='src/main.rs'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let local_fact_digest: String = conn
            .query_row(
                "SELECT source_digest FROM local_facts WHERE path='src/main.rs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let node_names: Vec<String> = conn
            .prepare("SELECT n.name FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='src/main.rs' ORDER BY n.name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        (inventory_digest, file_state, local_fact_digest, node_names)
    };
    let (before_root, before_source_state) = {
        let conn = reader::open(&database, workspace.root()).unwrap();
        let output_root: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='output_root'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let source_state = indexed_source_state(&conn);
        assert!(source_state.3.iter().any(|name| name == "previous"));
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM files WHERE path='.forge/frameworks/acme.toml'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "the framework manifest is outside the configured source inventory"
        );
        (output_root, source_state)
    };
    let before_bytes = fs::read(&database).unwrap();

    workspace.write(".forge/frameworks/acme.toml", "receivers = [\n");
    workspace.write_bytes("src/main.rs", b"\xff");
    let result = writer::delta(
        workspace.root(),
        &database,
        &[PathBuf::from("src/main.rs")],
    );
    let error = match result {
        Err(error) => error,
        Ok(summary) => panic!(
            "delta published a changed source despite malformed .forge/frameworks/acme.toml: {summary}"
        ),
    };
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains(".forge/frameworks/acme.toml"),
        "the error must identify the malformed framework manifest: {error_text}"
    );

    assert_eq!(fs::read(&database).unwrap(), before_bytes);
    let conn = reader::open(&database, workspace.root()).unwrap();
    let after_root: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after_root, before_root);
    let after_source_state = indexed_source_state(&conn);
    assert_eq!(after_source_state, before_source_state);
    assert!(after_source_state.3.iter().any(|name| name == "previous"));
}
