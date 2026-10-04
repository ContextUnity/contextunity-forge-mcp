#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use contextunity_forge_mcp::engine::scanner;
use contextunity_forge_mcp::mcp::server::Server;
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::reader,
};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn page(limit: usize) -> QueryOptions {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None).unwrap()
}

#[test]
fn test_multi_workspace_indexing_and_resilience() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_base =
        std::env::temp_dir().join(format!("test_forge_mw_{}_{}", std::process::id(), nonce));

    // 1. Setup primary workspace (traverse)
    let ws_traverse = temp_base.join("traverse");
    fs::create_dir_all(ws_traverse.join("src")).unwrap();
    fs::create_dir_all(ws_traverse.join("docs")).unwrap();
    fs::write(
        ws_traverse.join("src/traverse.py"),
        "def run_traverse():\n    return 'traverse'\n",
    )
    .unwrap();
    fs::write(
        ws_traverse.join("docs/traverse.md"),
        "# Traverse Specs\n## Route Planning\nTraverse plans optimal routes.\n",
    )
    .unwrap();

    // 2. Setup linked workspace 1 (commerce)
    let ws_commerce = temp_base.join("commerce");
    fs::create_dir_all(ws_commerce.join("extensions/commerce")).unwrap();
    fs::create_dir_all(ws_commerce.join("docs")).unwrap();
    fs::write(
        ws_commerce.join("extensions/commerce/snapshot.py"),
        "def revert_confirmed_bindings():\n    return 'reverted'\n",
    )
    .unwrap();
    fs::write(
        ws_commerce.join("docs/commerce.md"),
        "# Commerce Specs\n## Binding Rules\nBindings must be reversible.\n",
    )
    .unwrap();

    // 3. Setup linked workspace 2 (gridviewspec)
    let ws_grid = temp_base.join("gridviewspec");
    fs::create_dir_all(ws_grid.join("src")).unwrap();
    fs::write(
        ws_grid.join("src/grid.ts"),
        "export interface GridColumn {\n  id: string;\n  name: string;\n}\n",
    )
    .unwrap();

    // 4. Configure forge-mcp.yaml in traverse linking commerce and gridviewspec
    let adapter_content = format!(
        r#"format: forge-code-map-policy/v1
adapter_version: 1
roots:
  - src
docs:
  - docs
linked_workspaces:
  - name: commerce
    path: "{}"
    roots:
      - extensions
    docs:
      - docs
  - name: gridviewspec
    path: "{}"
    roots:
      - src
"#,
        ws_commerce.display(),
        ws_grid.display()
    );
    fs::write(ws_traverse.join("forge-mcp.yaml"), &adapter_content).unwrap();

    let db_path = ws_traverse.join(".forge/code-map.sqlite");
    let server = Server::new(ws_traverse.clone(), db_path.clone());

    // 5. Initial read: should index traverse + commerce + gridviewspec
    let overview_res = server
        .read(|conn| {
            let ov = reader::overview_with_options(
                conn,
                &reader::OverviewOptions {
                    aspects: None,
                    page: &page(100),
                },
            )?;
            Ok(ov)
        })
        .expect("read overview");

    let file_count = overview_res["counts"]["files"].as_i64().unwrap_or(0);
    // 2 files in traverse + 2 in commerce + 1 in gridviewspec = 5 files
    assert_eq!(
        file_count, 5,
        "expected 5 indexed files across 3 workspaces"
    );

    // Check symbols from all workspaces are queryable
    let node_check = server
        .read(|conn| {
            // Inspect symbol from traverse
            let tr =
                reader::inspect_paged(conn, "function:run_traverse", false, &page(100)).is_ok();
            // Inspect symbol from commerce
            let cm = reader::inspect_paged(
                conn,
                "function:revert_confirmed_bindings",
                false,
                &page(100),
            )
            .is_ok();
            // Inspect symbol from gridviewspec
            let gr = reader::inspect_paged(conn, "interface:GridColumn", false, &page(100)).is_ok();
            Ok(serde_json::json!({
                "tr": tr,
                "cm": cm,
                "gr": gr,
            }))
        })
        .expect("check symbols");
    assert!(
        node_check["tr"].as_bool().unwrap(),
        "run_traverse should be found"
    );
    assert!(
        node_check["cm"].as_bool().unwrap(),
        "revert_confirmed_bindings should be found"
    );
    assert!(
        node_check["gr"].as_bool().unwrap(),
        "GridColumn should be found"
    );

    // Check docs from both traverse and commerce are indexed
    let doc_check = server
        .read(|conn| {
            let tr_doc = reader::search_docs_with_options(
                conn,
                "Route Planning",
                &reader::DocSearchOptions {
                    doc_type: None,
                    component: None,
                    include_excerpt: false,
                    page: &page(5),
                },
            )?;
            let cm_doc = reader::search_docs_with_options(
                conn,
                "Binding Rules",
                &reader::DocSearchOptions {
                    doc_type: None,
                    component: None,
                    include_excerpt: false,
                    page: &page(5),
                },
            )?;
            Ok(serde_json::json!({
                "tr_sections": tr_doc["sections"]["items"].as_array().unwrap().len(),
                "cm_sections": cm_doc["sections"]["items"].as_array().unwrap().len(),
            }))
        })
        .expect("check docs");
    assert!(
        doc_check["tr_sections"].as_u64().unwrap() > 0,
        "traverse docs missing"
    );
    assert!(
        doc_check["cm_sections"].as_u64().unwrap() > 0,
        "commerce docs missing"
    );

    // 6. Resilience test: remove/delete gridviewspec workspace from disk!
    fs::remove_dir_all(&ws_grid).unwrap();

    // 7. Next read: server must detect missing workspace, skip it, and automatically reindex without errors!
    let overview_res2 = server
        .read(|conn| {
            let ov = reader::overview_with_options(
                conn,
                &reader::OverviewOptions {
                    aspects: None,
                    page: &page(100),
                },
            )?;
            Ok(ov)
        })
        .expect("read overview after workspace removed");

    let file_count2 = overview_res2["counts"]["files"].as_i64().unwrap_or(0);
    // 2 files in traverse + 2 in commerce = 4 files (gridviewspec safely purged)
    assert_eq!(
        file_count2, 4,
        "expected 4 indexed files after gridviewspec removed"
    );

    let node_check2 = server
        .read(|conn| {
            let tr =
                reader::inspect_paged(conn, "function:run_traverse", false, &page(100)).is_ok();
            let cm = reader::inspect_paged(
                conn,
                "function:revert_confirmed_bindings",
                false,
                &page(100),
            )
            .is_ok();
            let gr = reader::inspect_paged(conn, "interface:GridColumn", false, &page(100)).is_ok();
            Ok(serde_json::json!({
                "tr": tr,
                "cm": cm,
                "gr": gr,
            }))
        })
        .expect("check symbols after removal");
    assert!(
        node_check2["tr"].as_bool().unwrap(),
        "run_traverse should still exist"
    );
    assert!(
        node_check2["cm"].as_bool().unwrap(),
        "revert_confirmed_bindings should still exist"
    );
    assert!(
        !node_check2["gr"].as_bool().unwrap(),
        "GridColumn should be removed"
    );

    let _ = fs::remove_dir_all(&temp_base);
}

#[test]
fn test_linked_workspace_toggle_enabled() {
    let temp_base = std::env::temp_dir().join(format!("forge_test_enabled_{}", std::process::id()));
    let ws_main = temp_base.join("main");
    let ws_sibling = temp_base.join("sibling");
    fs::create_dir_all(ws_main.join("src")).unwrap();
    fs::create_dir_all(ws_sibling.join("src")).unwrap();
    fs::write(ws_main.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(ws_sibling.join("src/lib.rs"), "pub fn sibling() {}\n").unwrap();

    // 1. enabled: false -> should not be included in linked_workspaces
    let adapter_disabled = format!(
        r#"roots:
  - src
linked_workspaces:
  - name: sibling
    path: "{}"
    enabled: false
"#,
        ws_sibling.display()
    );
    fs::write(ws_main.join("forge-mcp.yaml"), &adapter_disabled).unwrap();
    let adapter = scanner::load_adapter(&ws_main, None).unwrap();
    assert!(
        adapter.linked_workspaces.is_empty(),
        "disabled linked workspace must be skipped"
    );

    // 2. enabled: true -> should be included
    let adapter_enabled = format!(
        r#"roots:
  - src
linked_workspaces:
  - name: sibling
    path: "{}"
    enabled: true
"#,
        ws_sibling.display()
    );
    fs::write(ws_main.join("forge-mcp.yaml"), &adapter_enabled).unwrap();
    let adapter = scanner::load_adapter(&ws_main, None).unwrap();
    assert_eq!(adapter.linked_workspaces.len(), 1);
    assert_eq!(adapter.linked_workspaces[0].name, "sibling");

    // 3. enabled omitted -> defaults to true
    let adapter_default = format!(
        r#"roots:
  - src
linked_workspaces:
  - name: sibling
    path: "{}"
"#,
        ws_sibling.display()
    );
    fs::write(ws_main.join("forge-mcp.yaml"), &adapter_default).unwrap();
    let adapter = scanner::load_adapter(&ws_main, None).unwrap();
    assert_eq!(adapter.linked_workspaces.len(), 1);
    assert_eq!(adapter.linked_workspaces[0].name, "sibling");

    let _ = fs::remove_dir_all(&temp_base);
}
