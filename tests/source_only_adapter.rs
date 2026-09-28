#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use contextunity_forge_mcp::{db::reader, mcp::server::Server};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("forge_source_only_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn inventory(server: &Server) -> Value {
    server.read(|conn| Ok(json!({
        "files": reader::rows(conn, "SELECT path FROM files ORDER BY path", &[], 100)?,
        "bundled_facts": conn.query_row("SELECT count(*) FROM local_facts WHERE path LIKE '%/vendor/%' OR path LIKE '%/static/library/%'", [], |r| r.get::<_,i64>(0))?,
    }))).unwrap()
}

#[test]
fn source_only_scope_removes_bundles_without_hiding_consumer_sources() {
    let ws = Workspace::new();
    ws.write("app/main.py", "def main(): pass\n");
    ws.write("app/src/static/js/authored.js", "function authored() {}\n");
    ws.write("app/src/staticfiles/collected.js", "function collected() {}\n");
    ws.write("app/src/core/static/core/js/grid.bundle.js", "function duplicate() {}\n");
    ws.write("library/src/library/runtime.py", "def runtime(): pass\n");
    ws.write("library/frontend/src/grid.ts", "export function grid() {}\n");
    ws.write("library/frontend/src/types/generated/contracts.ts", "export interface Grid { name: string }\n");
    ws.write("library/src/library/static/library/grid.js", "function bundled() {}\n");
    ws.write("library/src/library/static/library/vendor/ag-grid-community.min.js", "function vendor() {}\n");
    ws.write("library/build/bundle.js", "function built() {}\n");
    ws.write("library/frontend/dist/bundle.js", "function distribution() {}\n");
    ws.write("library/docs/api.md", "# Library API\nAuthored documentation.\n");
    let config = "roots: ['.']\nlinked_workspaces:\n  - name: library\n    path: ../library\n    roots: [src, frontend, schema, tests]\n    doc_roots: [docs]\n";
    ws.write("app/forge-mcp.yaml", config);
    let root = ws.0.join("app");
    let server = Server::new(root.clone(), root.join(".forge/code-map.sqlite"));
    assert!(inventory(&server)["bundled_facts"].as_i64().unwrap() > 0);

    let filtered = format!("ignore: [staticfiles, grid.bundle.js]\n{config}    ignore: [static, vendor, vendors, dist, build]\n");
    ws.write("app/forge-mcp.yaml", &filtered);
    let clean = inventory(&server);
    assert_eq!(clean["bundled_facts"], 0);
    let paths: Vec<_> = clean["files"].as_array().unwrap().iter().map(|v| v["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec![
        "[library]/docs/api.md",
        "[library]/frontend/src/grid.ts",
        "[library]/frontend/src/types/generated/contracts.ts",
        "[library]/src/library/runtime.py",
        "main.py",
        "src/static/js/authored.js",
    ]);
    ws.write("library/src/library/static/library/grid.js", "function rebuilt_bundle() {}\n");
    let unchanged = inventory(&server);
    assert_eq!(unchanged["freshness"]["refresh"], "none");
    assert_eq!(unchanged["freshness"]["output_root"], clean["freshness"]["output_root"]);

    ws.write("library/frontend/src/grid.ts", "export function updated_grid() {}\n");
    let changed = inventory(&server);
    assert_eq!(changed["freshness"]["refresh"], "delta");
    assert_ne!(changed["freshness"]["output_root"], clean["freshness"]["output_root"]);
    assert_eq!(changed["files"], clean["files"]);
}
