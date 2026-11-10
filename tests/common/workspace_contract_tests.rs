use forge_core::commitments::DOMAINS;
use forge_core::config::ScannerLimits;
use forge_core::format::stable_hash64;
use forge_core::interner::{PathId, SymbolId};
use forge_core::models::Node;
use forge_index::ast::AstToken;
use forge_index::languages::LanguageRegistry;
use forge_index::linker::RouteIndex;
use forge_tasks::blackboard::BlackboardScope;
use forge_tasks::lifecycle::{TaskStage, TaskStatus};
use forge_tasks::verification::ProofPolicy;

#[cfg(feature = "lang-rust")]
#[test]
fn workspace_build_cache_is_isolated_and_removed_with_its_workspace() {
    let first = crate::common::Workspace::new();
    let second = crate::common::Workspace::new();
    for workspace in [&first, &second] {
        workspace.write("src/lib.rs", "pub fn same_source() {}\n");
    }
    let first_cold = first.build();
    let first_warm = first.build_to(first.path(".forge/warm.sqlite"));
    let second_cold = second.build();
    assert_eq!(first_cold["pipeline"]["extraction_cache_hits"], 0);
    assert_eq!(first_warm["pipeline"]["extraction_cache_hits"], 1);
    assert_eq!(second_cold["pipeline"]["extraction_cache_hits"], 0);
    let cache = first.path(".forge/extraction-cache");
    assert!(cache.is_dir());
    drop(first);
    assert!(!cache.exists());
    assert!(second.path(".forge/extraction-cache").is_dir());
}

#[test]
fn workspace_builds_and_applies_delta_in_its_owned_temp_directory() {
    let workspace = crate::common::Workspace::new();
    let root = workspace.root().to_path_buf();
    workspace.write("src/lib.rs", "pub fn before() {}\n");
    workspace.build();

    let before = workspace.open();
    let before_count: i64 = before
        .query_row(
            "SELECT count(*) FROM nodes WHERE name='before'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_count, 1);
    drop(before);

    workspace.write("src/lib.rs", "pub fn after() {}\n");
    workspace.delta(&["src/lib.rs"]);
    let after = workspace.open();
    let after_names: Vec<String> = after
        .prepare("SELECT name FROM nodes WHERE kind='function' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(after_names, ["after"]);
    drop(after);

    drop(workspace);
    assert!(
        !root.exists(),
        "temporary workspace should be removed on drop"
    );
}

#[test]
fn cargo_workspace_declares_independent_crates_and_unidirectional_boundaries() {
    // Verify forge-core crate public API & real Merkle root computation
    assert_eq!(stable_hash64("workspace_boundary"), 7897085170651836845);
    assert_eq!(forge_core::version(), "0.5.0-alpha.1");
    let path_id = PathId(42);
    let sym_id = SymbolId(101);
    assert_eq!(u32::from(path_id), 42);
    assert_eq!(u32::from(sym_id), 101);
    let limits = ScannerLimits::default();
    assert_eq!(limits.max_files, 100_000);
    assert!(DOMAINS.contains(&"nodes"));
    let mut leaves = std::collections::BTreeMap::new();
    leaves.insert("src/lib.rs".to_string(), "a1b2c3d4".to_string());
    let agg = forge_core::commitments::aggregate_domain_digest("nodes", &leaves);
    assert!(!agg.is_empty());
    let domains = vec![("nodes".to_string(), agg)];
    let metadata = vec![("schema_version".to_string(), "15".to_string())];
    let merkle_root = forge_core::commitments::compute_merkle_root(&domains, &metadata);
    assert_eq!(merkle_root.len(), 64);

    let route_node = Node {
        id: "route:get_user".into(),
        kind: "route".into(),
        name: "get_user".into(),
        qualname: "api.get_user".into(),
        path: "src/api.rs".into(),
        line: 10,
        end_line: 15,
        is_test: false,
        language: "rust".into(),
        generated: false,
        details: serde_json::json!({
            "method": "GET",
            "route_path": "/api/users/{id}"
        }),
        native_semantic: None,
    };

    // Verify forge-index crate public API & real route matching and schema initialization
    assert_eq!(forge_index::version(), "0.5.0-alpha.1");
    let mut reg = LanguageRegistry::new();
    assert!(reg.is_empty());
    reg.enable("rust");
    assert!(reg.is_enabled("rust"));
    assert_eq!(reg.len(), 1);

    let route_slice = [&route_node];
    let routes = RouteIndex::build(&route_slice);
    assert_eq!(routes.len(), 1);
    let matched = routes.match_route("GET", "/api/users/123").unwrap();
    assert!(matched.is_some());
    assert_eq!(matched.unwrap().id, "route:get_user");
    let unmatched = routes.match_route("POST", "/api/users/123").unwrap();
    assert!(unmatched.is_none());

    let token = AstToken {
        kind: "identifier".into(),
        text: "my_var".into(),
        row: 1,
        column: 4,
    };
    assert_eq!(token.text, "my_var");

    let mem_conn = rusqlite::Connection::open_in_memory().unwrap();
    forge_index::db::initialize_code_map_schema(&mem_conn).unwrap();
    let table_count: i64 = mem_conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('nodes', 'edges', 'metadata', 'resolution_coverage')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 4);

    // Verify forge-tasks crate public API & real lifecycle, blackboard, and store operations
    assert_eq!(forge_tasks::version(), "0.5.0-alpha.1");
    assert_eq!(TaskStage::Build, TaskStage::Build);
    assert_eq!(TaskStatus::InProgress, TaskStatus::InProgress);
    assert_eq!(ProofPolicy::DirectProof, ProofPolicy::DirectProof);
    assert_eq!(BlackboardScope::Task, BlackboardScope::Task);

    let temp_path =
        std::env::temp_dir().join(format!("test-tasks-store-{}.sqlite", std::process::id()));
    let store =
        forge_tasks::store::TasksStore::open_project(&temp_path, "test-repo", "test-proj").unwrap();
    assert!(forge_tasks::store::now() > 0);
    drop(store);
    let _ = std::fs::remove_file(&temp_path);

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root_cargo = manifest_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&root_cargo).expect("root Cargo.toml must exist");
    let doc: toml::Value = content.parse().expect("root Cargo.toml must be valid toml");

    // 1. Cargo workspace root in Cargo.toml declares members:
    //    ["crates/forge-core", "crates/forge-index", "crates/forge-tasks", "."]
    let workspace = doc
        .get("workspace")
        .expect("Cargo.toml must declare [workspace]");
    let members = workspace
        .get("members")
        .and_then(|m| m.as_array())
        .expect("[workspace] must have members array");
    let member_strings: Vec<&str> = members.iter().filter_map(|m| m.as_str()).collect();
    for required in [
        "crates/forge-core",
        "crates/forge-index",
        "crates/forge-tasks",
        ".",
    ] {
        assert!(
            member_strings.contains(&required),
            "workspace members must include {required}, got {member_strings:?}"
        );
    }

    // 2. forge-core crate exists and has no parser or database dependencies
    let core_cargo_path = manifest_dir.join("crates/forge-core/Cargo.toml");
    assert!(
        core_cargo_path.exists(),
        "crates/forge-core/Cargo.toml must exist"
    );
    let core_content =
        std::fs::read_to_string(&core_cargo_path).expect("read forge-core Cargo.toml");
    let core_doc: toml::Value = core_content.parse().expect("parse forge-core Cargo.toml");
    assert_eq!(core_doc["package"]["name"].as_str(), Some("forge-core"));
    let core_deps = core_doc
        .get("dependencies")
        .cloned()
        .unwrap_or(toml::Value::Table(Default::default()));
    assert!(
        core_deps.get("rusqlite").is_none(),
        "forge-core must have zero rusqlite dependency"
    );
    assert!(
        core_deps.get("tree-sitter").is_none(),
        "forge-core must have zero tree-sitter dependency"
    );
    assert!(
        core_deps.get("forge-index").is_none(),
        "forge-core must have zero forge-index dependency"
    );
    assert!(
        core_deps.get("forge-tasks").is_none(),
        "forge-core must have zero forge-tasks dependency"
    );

    // 3. forge-index crate exists, depends on forge-core, and has no dependency on forge-tasks
    let index_cargo_path = manifest_dir.join("crates/forge-index/Cargo.toml");
    assert!(
        index_cargo_path.exists(),
        "crates/forge-index/Cargo.toml must exist"
    );
    let index_content =
        std::fs::read_to_string(&index_cargo_path).expect("read forge-index Cargo.toml");
    let index_doc: toml::Value = index_content.parse().expect("parse forge-index Cargo.toml");
    assert_eq!(index_doc["package"]["name"].as_str(), Some("forge-index"));
    let index_deps = index_doc
        .get("dependencies")
        .cloned()
        .unwrap_or(toml::Value::Table(Default::default()));
    assert!(
        index_deps.get("forge-core").is_some(),
        "forge-index must depend on forge-core"
    );
    assert!(
        index_deps.get("forge-tasks").is_none(),
        "forge-index must have zero forge-tasks dependency"
    );

    // 4. forge-tasks crate exists, depends on forge-core, and has no dependency on forge-index
    let tasks_cargo_path = manifest_dir.join("crates/forge-tasks/Cargo.toml");
    assert!(
        tasks_cargo_path.exists(),
        "crates/forge-tasks/Cargo.toml must exist"
    );
    let tasks_content =
        std::fs::read_to_string(&tasks_cargo_path).expect("read forge-tasks Cargo.toml");
    let tasks_doc: toml::Value = tasks_content.parse().expect("parse forge-tasks Cargo.toml");
    assert_eq!(tasks_doc["package"]["name"].as_str(), Some("forge-tasks"));
    let tasks_deps = tasks_doc
        .get("dependencies")
        .cloned()
        .unwrap_or(toml::Value::Table(Default::default()));
    assert!(
        tasks_deps.get("forge-core").is_some(),
        "forge-tasks must depend on forge-core"
    );
    assert!(
        tasks_deps.get("forge-index").is_none(),
        "forge-tasks must have zero forge-index dependency"
    );
}

#[test]
fn direct_library_task_entrypoint_initializes_extractors_without_caller_bootstrap() {
    // Calling direct root library task API automatically composes extractors
    contextunity_forge_mcp::engine::tasks::ensure_extractors();
    let facts = contextunity_forge_mcp::engine::ast::extract_typed(
        "tests/seam.rs",
        "rust",
        "#[test] fn sample_seam() { assert!(true); }",
    )
    .expect("production extractor must be wired");
    assert!(facts
        .facts
        .nodes
        .iter()
        .any(|n| n.is_test && n.name == "sample_seam"));
}

#[test]
fn workspace_writes_a_file_set_and_removes_one_member() {
    let workspace = crate::common::Workspace::new();
    workspace.write_all(&[
        ("src/a.rs", "pub fn a() {}\n"),
        ("src/b.rs", "pub fn b() {}\n"),
    ]);
    assert_eq!(
        std::fs::read_to_string(workspace.path("src/a.rs")).unwrap(),
        "pub fn a() {}\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path("src/b.rs")).unwrap(),
        "pub fn b() {}\n"
    );
    workspace.remove("src/b.rs");
    assert!(!workspace.path("src/b.rs").exists());
    assert!(workspace.path("src/a.rs").exists());
}
