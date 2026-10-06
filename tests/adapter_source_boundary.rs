#![cfg(feature = "lang-yaml")]

use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::scanner,
    mcp::server::Server,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge_adapter_boundary_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/index.sqlite")
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn response_only_default_config_is_outside_source_inventory() {
    let workspace = Workspace::new();
    workspace.write("settings.yaml", "service: enabled\n");
    workspace.write("nested/forge-mcp.yaml", "service: nested\n");
    let server = Server::new(workspace.0.clone(), workspace.db());
    let read = || {
        server.read(|conn| Ok(json!({"files": reader::rows(conn, "SELECT path FROM files ORDER BY path", &[], 20)?}))).unwrap()
    };
    let before = read();
    let before_bytes = fs::read(workspace.db()).unwrap();
    for content in [Some("response: {page_size: 1}\n"), Some("{}\n"), None] {
        if let Some(content) = content {
            workspace.write("forge-mcp.yaml", content);
        } else {
            fs::remove_file(workspace.0.join("forge-mcp.yaml")).unwrap();
        }
        let current = read();
        assert_eq!(current["files"], before["files"]);
        assert_eq!(current["freshness"]["refresh"], "none");
        assert_eq!(
            current["freshness"]["output_root"],
            before["freshness"]["output_root"]
        );
        assert_eq!(fs::read(workspace.db()).unwrap(), before_bytes);
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
    let config = workspace.0.join("src/custom.yaml");
    let configuration = |size| {
        format!("roots: [src]\nresponse: {{page_size: {size}}}\nlinked_workspaces:\n  - name: library\n    path: {}\n", linked.0.display())
    };
    workspace.write("src/custom.yaml", &configuration(1));
    writer::build(&workspace.0, &workspace.db(), Some(&config)).unwrap();
    workspace.write("src/custom.yaml", &configuration(2));
    workspace.write("src/settings.yaml", "service: after\n");
    let delta = writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("src/settings.yaml")],
    )
    .unwrap();
    assert_eq!(delta["changed_files"], 1, "{delta}");
    let cold_path = workspace.0.join(".forge/cold.sqlite");
    writer::build(&workspace.0, &cold_path, Some(&config)).unwrap();
    let incremental = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_path, &workspace.0).unwrap();
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
    let adapter = scanner::load_adapter(&workspace.0, Some(&config)).unwrap();
    assert_eq!(adapter.adapter_path, Some(config));
}
