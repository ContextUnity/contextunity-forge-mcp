use crate::cache_fixture::{entries, global_copies, CacheFixture, RebuildCause};
use crate::{cli, harness::McpTestHarness};
use contextunity_forge_mcp::engine::pipeline;
use serde_json::json;

#[test]
fn mcp_cold_and_adapter_rebuilds_keep_extractions_in_the_session_cache() {
    let workspace = CacheFixture::new();
    let directory = workspace.path(".forge/extraction-cache");
    let mut client = McpTestHarness::new(&workspace).expect("MCP initialize");
    for changed in [false, true] {
        if changed {
            workspace.clear_cache();
            workspace.write("forge-mcp.yaml", "roots: [src]\nadapter_version: changed\n");
        }
        let result = client
            .payload("code_map_overview", json!({}))
            .expect("MCP tool should succeed");
        assert_eq!(result["freshness"]["refresh"], "rebuild");
        let cached = entries(&directory);
        assert!(!cached.is_empty(), "adapter_changed={changed}");
        assert!(global_copies(&cached).is_empty(), "{cached:?}");
        assert_eq!(
            client
                .payload(
                    "code_map_search",
                    json!({"pattern":"isolated","exact":true})
                )
                .expect("MCP tool should succeed")["nodes"]["total"],
            1
        );
    }
    client.finish().expect("server must finish successfully");
}

#[test]
fn shared_server_constructor_keeps_cold_admission_in_the_workspace_cache() {
    let workspace = CacheFixture::new();
    let symbols = crate::common::mcp_client::in_process_server(&workspace)
        .read(|connection| {
            let symbols: i64 = connection.query_row(
                "SELECT count(*) FROM nodes WHERE name='isolated'",
                [],
                |row| row.get(0),
            )?;
            Ok(serde_json::json!({"symbols":symbols}))
        })
        .expect("admit real shared server");
    assert_eq!(symbols["symbols"], 1);
    assert!(workspace.path(".forge/index.sqlite").is_file());
    let cached = entries(&workspace.path(".forge/extraction-cache"));
    assert!(!cached.is_empty());
    assert!(global_copies(&cached).is_empty(), "{cached:?}");
}

#[test]
fn every_delta_rebuild_fallback_preserves_the_explicit_cache_directory() {
    for case in RebuildCause::ALL {
        let workspace = CacheFixture::new();
        workspace.prepare_fallback(case);
        let directory = workspace.directory();
        let changed = case.changed_path();
        let result = pipeline::delta_with_cache_dir(
            workspace.root(),
            &workspace.db(),
            &[changed.into()],
            &directory,
        )
        .unwrap();
        assert!(result.is_object(), "{case:?}: {result}");
        let cached = entries(&directory);
        assert!(!cached.is_empty(), "{case:?}");
        assert!(global_copies(&cached).is_empty(), "{case:?}: {cached:?}");
        assert_eq!(workspace.isolated_symbol_count().unwrap(), 1);
    }
}

#[test]
fn concurrent_workspace_delta_fallbacks_use_distinct_local_caches() {
    let first = CacheFixture::new();
    let second = CacheFixture::new();
    second.copy_source_from(&first);
    for workspace in [&first, &second] {
        workspace.build();
        workspace.clear_cache();
        workspace.write("requirements.txt", "requests>=2.1\n");
    }
    let cached = std::thread::scope(|scope| {
        let handles = [&first, &second].map(|workspace| {
            scope.spawn(move || {
                workspace.delta(&["src/main.rs"]);
                entries(&workspace.path(".forge/extraction-cache"))
            })
        });
        handles.map(|handle| handle.join().unwrap())
    });
    assert!(!cached[0].is_empty());
    assert_eq!(cached[0], cached[1]);
    assert!(global_copies(&cached[0]).is_empty());
    assert_ne!(
        first.path(".forge/extraction-cache"),
        second.path(".forge/extraction-cache")
    );
}

#[test]
fn cli_build_and_query_auto_build_capture_output_with_local_extraction_caches() {
    for args in [vec!["build"], vec!["query", "overview"]] {
        let workspace = CacheFixture::new();
        let output = cli::run_cli_json(&workspace, &args);
        crate::common::assertions::assert_command_succeeded(&output);
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap()
            .is_object());
        let cached = entries(&workspace.path(".forge/extraction-cache"));
        assert!(!cached.is_empty(), "{args:?}");
        assert!(global_copies(&cached).is_empty(), "{args:?}: {cached:?}");
        assert_eq!(workspace.isolated_symbol_count().unwrap(), 1);
    }
}

#[test]
fn cli_delta_fallbacks_write_only_the_configured_workspace_cache() {
    for case in RebuildCause::ALL {
        let workspace = CacheFixture::new();
        workspace.prepare_fallback(case);
        let directory = workspace.directory();
        let changed = case.changed_path();
        let output = cli::run_cli_json(&workspace, &["delta", "--modified", changed]);
        crate::common::assertions::assert_command_succeeded(&output);
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap()
            .is_object());
        let cached = entries(&directory);
        assert!(!cached.is_empty(), "CLI {case:?}");
        assert!(
            global_copies(&cached).is_empty(),
            "CLI {case:?}: {cached:?}"
        );
        assert_eq!(workspace.isolated_symbol_count().unwrap(), 1);
    }
}

#[test]
fn mcp_manifest_and_source_inventory_rebuilds_keep_the_session_cache() {
    for case in ["manifest", "source-inventory"] {
        let workspace = CacheFixture::new();
        let mut client = McpTestHarness::new(&workspace).expect("MCP initialize");
        client
            .payload("code_map_overview", json!({}))
            .expect("MCP tool should succeed");
        let directory = workspace.path(".forge/extraction-cache");
        workspace.clear_cache();
        if case == "manifest" {
            workspace.write("requirements.txt", "requests>=2.1\n");
        } else {
            workspace.remove("src/main.rs");
            std::fs::create_dir(workspace.path("src/main.rs")).unwrap();
            workspace.write(
                "src/new.rs",
                &format!(
                    "pub fn refreshed() {{ let identity = {:?}; }}\n",
                    workspace.root()
                ),
            );
        }
        crate::common::mcp_client::invalidate_mcp_connection_cache(&workspace.db()).unwrap();
        let result = client
            .payload("code_map_overview", json!({}))
            .expect("MCP tool should succeed");
        assert_eq!(
            result["freshness"]["refresh"], "rebuild",
            "{case:?}: {result}"
        );
        let cached = entries(&directory);
        assert!(!cached.is_empty(), "MCP {case:?}");
        assert!(
            global_copies(&cached).is_empty(),
            "MCP {case:?}: {cached:?}"
        );
        let symbol = if case == "manifest" {
            "isolated"
        } else {
            "refreshed"
        };
        assert_eq!(
            client
                .payload("code_map_search", json!({"pattern":symbol,"exact":true}))
                .expect("MCP tool should succeed")["nodes"]["total"],
            1
        );
        if case == "source-inventory" {
            assert_eq!(
                client
                    .payload(
                        "code_map_search",
                        json!({"pattern":"isolated","exact":true})
                    )
                    .expect("MCP tool should succeed")["nodes"]["total"],
                0
            );
        }
        client.finish().expect("server must finish successfully");
    }
}

#[test]
fn an_unwritable_explicit_cache_remains_best_effort_without_global_fallback() {
    let first = CacheFixture::new();
    let mut client = McpTestHarness::new(&first).expect("MCP initialize");
    client
        .payload("code_map_overview", json!({}))
        .expect("MCP tool should succeed");
    let expected_entries = entries(&first.path(".forge/extraction-cache"));
    assert!(!expected_entries.is_empty());
    let blocked = CacheFixture::new();
    blocked.copy_source_from(&first);
    blocked.write(".forge/extraction-cache", "blocked cache\n");
    client.finish().expect("server must finish successfully");
    let mut client = McpTestHarness::new(&blocked).expect("MCP initialize");
    assert_eq!(
        client
            .payload(
                "code_map_search",
                json!({"pattern":"isolated","exact":true})
            )
            .expect("MCP tool should succeed")["nodes"]["total"],
        1
    );
    assert_eq!(
        std::fs::read(blocked.path(".forge/extraction-cache")).unwrap(),
        b"blocked cache\n"
    );
    assert!(global_copies(&expected_entries).is_empty());
    client.finish().expect("server must finish successfully");
}

#[test]
fn selected_git_worktrees_keep_the_explicit_session_cache() {
    let workspace = CacheFixture::new();
    workspace.seed_git().expect("seed real Git baseline");
    let linked = workspace.path(".worktrees/049-cache");
    workspace
        .run_git(&[
            "worktree",
            "add",
            "--detach",
            linked
                .to_str()
                .expect("worktree path should be valid UTF-8"),
        ])
        .expect("create real selected worktree");
    workspace.write(
        ".worktrees/049-cache/src/main.rs",
        &format!("pub fn selected() {{ let identity = {:?}; }}\n", linked),
    );
    let mut client = McpTestHarness::new(&workspace).expect("MCP initialize");
    let result = client
        .payload(
            "code_map_search",
            json!({"pattern":"selected","exact":true,"worktree":"49"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(result["nodes"]["total"], 1, "{result}");
    let cached = entries(&workspace.path(".forge/extraction-cache"));
    assert!(!cached.is_empty());
    assert!(global_copies(&cached).is_empty());
    assert!(!linked.join(".forge/extraction-cache").exists());
    client.finish().expect("server must finish successfully");
}
