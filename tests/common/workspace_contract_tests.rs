use contextunity_forge_mcp::core::models::stable_hash64;

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
    let _ = stable_hash64("workspace_boundary");
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root_cargo = manifest_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&root_cargo).expect("root Cargo.toml must exist");
    let doc: toml::Value = content.parse().expect("root Cargo.toml must be valid toml");

    // 1. Cargo workspace root in Cargo.toml declares members:
    //    ["crates/forge-core", "crates/forge-index", "crates/forge-tasks", "."]
    let workspace = doc.get("workspace").expect("Cargo.toml must declare [workspace]");
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
    let core_doc: toml::Value = core_content
        .parse()
        .expect("parse forge-core Cargo.toml");
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
    let index_doc: toml::Value = index_content
        .parse()
        .expect("parse forge-index Cargo.toml");
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
    let tasks_doc: toml::Value = tasks_content
        .parse()
        .expect("parse forge-tasks Cargo.toml");
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
