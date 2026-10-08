use crate::common::Workspace;
use contextunity_forge_mcp::db::writer;
use contextunity_forge_mcp::engine::scanner;
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn test_root_scope_guard_filesystem_root() {
    let err = scanner::check_root_scope(Path::new("/"), false);
    assert!(err.is_err(), "Filesystem root '/' must be rejected");
    assert!(err
        .unwrap_err()
        .to_string()
        .contains("refusing to index filesystem root"));

    // Allowed when allow_broad is true
    assert!(scanner::check_root_scope(Path::new("/"), true).is_ok());
}

#[test]
fn test_root_scope_guard_multi_repo_container() {
    let workspace = Workspace::new();
    // Create 2 nested repos without root manifest or root .git
    workspace.write("repo1/.git/config", "");
    workspace.write("repo1/src/lib.py", "def a(): pass");
    workspace.write("repo2/.git/config", "");
    workspace.write("repo2/src/lib.py", "def b(): pass");

    let err = scanner::check_root_scope(workspace.root(), false);
    assert!(err.is_err(), "Multi-repo container must be rejected");
    assert!(err
        .unwrap_err()
        .to_string()
        .contains("multi-repository container"));

    // Allowed if allow_broad is true
    assert!(scanner::check_root_scope(workspace.root(), true).is_ok());

    // Allowed if root has forge-mcp.yaml manifest
    workspace.write("forge-mcp.yaml", "limits:\n  allow_broad_root: true\n");
    assert!(scanner::check_root_scope(workspace.root(), false).is_ok());
}

#[test]
fn test_scanner_limits_adapter_config() {
    let workspace = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        r#"
limits:
  max_files: 50
  max_file_bytes: 1000
  max_total_bytes: 50000
  allow_broad_root: true
"#,
    );

    let adapter = scanner::load_adapter(workspace.root(), None).unwrap();
    assert_eq!(adapter.limits.max_files, 50);
    assert_eq!(adapter.limits.max_file_bytes, 1000);
    assert_eq!(adapter.limits.max_total_bytes, 50000);
    assert!(adapter.limits.allow_broad_root);
}

#[test]
fn test_scanner_limits_file_byte_limit_enforced() {
    let workspace = Workspace::new();
    workspace.write(".git/config", ""); // mark as valid repo
    workspace.write(
        "forge-mcp.yaml",
        r#"
limits:
  max_file_bytes: 100
"#,
    );
    workspace.write("src/small.py", "x = 1\n");
    workspace.write("src/large.py", &"x = 1\n".repeat(50)); // ~300 bytes > 100 bytes

    let err = scanner::scan(workspace.root(), None);
    assert!(err.is_err());
    assert!(err
        .unwrap_err()
        .to_string()
        .contains("file exceeds byte limit"));
}

#[test]
fn scanner_enforces_configured_file_count_limit() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    workspace.write(
        "forge-mcp.yaml",
        "limits:\n  max_files: 2\n  allow_broad_root: true\n",
    );
    for name in ["a.py", "b.py", "c.py"] {
        workspace.write(format!("src/{name}"), "x = 1\n");
    }

    let error = scanner::scan(workspace.root(), None).unwrap_err();
    assert!(error.to_string().contains("workspace exceeds file limit"));
}

#[test]
fn scanner_enforces_aggregate_byte_limit() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    workspace.write(
        "forge-mcp.yaml",
        "limits:\n  max_total_bytes: 6\n  allow_broad_root: true\n",
    );
    workspace.write("src/a.py", "abc\n");
    workspace.write("src/b.py", "def\n");

    let error = scanner::scan(workspace.root(), None).unwrap_err();
    assert!(error
        .to_string()
        .contains("workspace exceeds total byte limit"));
}

#[test]
fn scanner_file_count_limit_ignores_unrecognized_files() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    workspace.write(
        "forge-mcp.yaml",
        "limits:\n  max_files: 2\n  allow_broad_root: true\n",
    );
    workspace.write("src/a.py", "x = 1\n");
    workspace.write("src/b.py", "y = 2\n");
    workspace.write("notes.txt", "not indexed\n");

    let report = scanner::scan(workspace.root(), None).unwrap();
    assert_eq!(report.files, 2);
}

#[test]
fn scanner_accepts_the_default_file_boundary_and_rejects_one_byte_over() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    workspace.write(
        "forge-mcp.yaml",
        "limits:\n  max_file_bytes: 5242880\n  max_total_bytes: 6000000\n  allow_broad_root: true\n",
    );
    workspace.write_bytes(
        "src/boundary.py",
        &vec![b'x'; scanner::DEFAULT_MAX_FILE_BYTES as usize],
    );
    assert!(scanner::scan(workspace.root(), None).is_ok());

    workspace.write_bytes(
        "src/oversized.py",
        &vec![b'x'; scanner::DEFAULT_MAX_FILE_BYTES as usize + 1],
    );
    let error = scanner::scan(workspace.root(), None).unwrap_err();
    assert!(error.to_string().contains("file exceeds byte limit"));
}

#[test]
#[ignore = "creates more than 100,000 files; run only on a dedicated filesystem stress lane"]
fn scanner_rejects_a_workspace_over_the_default_file_limit() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    let source = workspace.root().join("src");
    fs::create_dir_all(&source).unwrap();
    for index in 0..=scanner::DEFAULT_MAX_FILES {
        fs::write(source.join(format!("file_{index}.py")), "x=1\n").unwrap();
    }

    let error = scanner::scan(workspace.root(), None).unwrap_err();
    assert!(error.to_string().contains("workspace exceeds file limit"));
}

#[test]
fn scanner_memory_budget_has_a_defensive_floor_and_cap() {
    let available = scanner::available_memory_bytes();
    assert!(available > 0, "available memory must be positive");
    let budget = scanner::memory_budget_bytes();
    assert!(budget >= 64 * 1024 * 1024, "budget must be at least 64 MiB");
    assert!(
        budget <= 16 * 1024 * 1024 * 1024,
        "budget must not exceed 16 GB"
    );
}

#[test]
fn delta_preserves_the_configured_file_size_limit() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    workspace.write(
        "adapter.yaml",
        "roots: [src]\nlimits:\n  max_files: 10\n  max_file_bytes: 3\n  max_total_bytes: 100000\n",
    );
    workspace.write("src/a.py", "x");
    let database = workspace.root().join(".forge/code-map.sqlite");
    let adapter = workspace.root().join("adapter.yaml");
    writer::build(workspace.root(), &database, Some(&adapter)).unwrap();

    workspace.write("src/a.py", "x=1\n");
    let error =
        writer::delta(workspace.root(), &database, &[PathBuf::from("src/a.py")]).unwrap_err();
    assert!(
        error.to_string().contains("file exceeds byte limit"),
        "{error:#}"
    );
}

#[test]
fn delta_accepts_a_root_allowed_by_the_current_adapter() {
    let workspace = Workspace::new();
    workspace.write("repo-a/.git/config", "");
    workspace.write("repo-a/src/a.py", "x = 1\n");
    workspace.write("repo-b/.git/config", "");
    workspace.write("repo-b/src/b.py", "y = 2\n");
    workspace.write(
        "adapter.yaml",
        "roots: [repo-a/src, repo-b/src]\nlimits:\n  allow_broad_root: true\n",
    );
    let database = workspace.root().join(".forge/code-map.sqlite");
    let adapter = workspace.root().join("adapter.yaml");
    writer::build(workspace.root(), &database, Some(&adapter)).unwrap();

    workspace.write("repo-a/src/a.py", "x = 3\n");
    let result = writer::delta(
        workspace.root(),
        &database,
        &[PathBuf::from("repo-a/src/a.py")],
    );
    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn delta_rebuilds_stale_policy_with_its_explicit_adapter() {
    let workspace = Workspace::new();
    workspace.write("repo-a/.git/config", "");
    workspace.write("repo-a/src/a.py", "x = 1\n");
    workspace.write("repo-b/.git/config", "");
    workspace.write("repo-b/src/b.py", "y = 2\n");
    workspace.write(
        "adapter.yaml",
        "roots: [repo-a/src, repo-b/src]\nlimits:\n  allow_broad_root: true\n",
    );
    let database = workspace.root().join(".forge/code-map.sqlite");
    let adapter = workspace.root().join("adapter.yaml");
    writer::build(workspace.root(), &database, Some(&adapter)).unwrap();

    let conn = Connection::open(&database).unwrap();
    let previous_digest: String = conn
        .query_row(
            "SELECT digest FROM source_inventory WHERE path='repo-a/src/a.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let raw_policy: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='adapter'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut policy: serde_json::Value = serde_json::from_str(&raw_policy).unwrap();
    assert!(policy.as_object_mut().unwrap().remove("limits").is_some());
    conn.execute(
        "UPDATE metadata SET value=?1 WHERE key='adapter'",
        [policy.to_string()],
    )
    .unwrap();
    conn.execute(
        "UPDATE metadata SET value='13' WHERE key='schema_version'",
        [],
    )
    .unwrap();
    drop(conn);

    workspace.write("repo-a/src/a.py", "x = 3\n");
    writer::delta(
        workspace.root(),
        &database,
        &[PathBuf::from("repo-a/src/a.py")],
    )
    .unwrap();

    let conn = Connection::open(&database).unwrap();
    let current_digest: String = conn
        .query_row(
            "SELECT digest FROM source_inventory WHERE path='repo-a/src/a.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(current_digest, previous_digest);
    let version: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, scanner::ENGINE_SCHEMA_VERSION);
    let raw_policy: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='adapter'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let policy: serde_json::Value = serde_json::from_str(&raw_policy).unwrap();
    assert_eq!(policy["adapter_path"], "adapter.yaml");
    assert_eq!(policy["limits"]["allow_broad_root"], true);
}

#[test]
fn default_scope_excludes_common_generated_directories() {
    let workspace = Workspace::new();
    workspace.write("src/main.py", "def source(): pass\n");
    workspace.write("src/build_helpers.py", "def source(): pass\n");
    for directory in [
        ".git",
        ".forge",
        ".forge-mcp",
        ".venv",
        "vendor",
        "target",
        "node_modules",
        "build",
        "dist",
        "coverage",
        ".cache",
        ".next",
        ".nuxt",
        ".svelte-kit",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".gradle",
        "__pycache__",
    ] {
        workspace.write(format!("{directory}/generated.py"), "def source(): pass\n");
    }

    let report = scanner::scan(workspace.root(), None).unwrap();
    let paths: Vec<_> = report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(paths, ["src/build_helpers.py", "src/main.py"]);
}
