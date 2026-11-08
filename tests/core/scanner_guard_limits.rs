use crate::common::Workspace;
use contextunity_forge_mcp::db::writer;
use contextunity_forge_mcp::engine::scanner;
use rusqlite::Connection;
use std::{
    fs, io,
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
fn scanner_enforces_configured_file_count_and_byte_limits() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");

    enum Expected {
        Error(&'static str),
        SuccessFiles(usize),
    }

    let cases = [
        (
            "configured file count limit",
            "limits:\n  max_files: 2\n  allow_broad_root: true\n",
            &[
                ("src/a.py", "x = 1\n"),
                ("src/b.py", "x = 1\n"),
                ("src/c.py", "x = 1\n"),
            ][..],
            Expected::Error("workspace exceeds file limit"),
        ),
        (
            "aggregate byte limit",
            "limits:\n  max_total_bytes: 6\n  allow_broad_root: true\n",
            &[("src/a.py", "abc\n"), ("src/b.py", "def\n")][..],
            Expected::Error("workspace exceeds total byte limit"),
        ),
        (
            "file count limit ignores unrecognized files",
            "limits:\n  max_files: 2\n  allow_broad_root: true\n",
            &[
                ("src/a.py", "x = 1\n"),
                ("src/b.py", "y = 2\n"),
                ("notes.txt", "not indexed\n"),
            ][..],
            Expected::SuccessFiles(2),
        ),
    ];

    for (name, config, files, expected) in cases {
        let _ = std::fs::remove_dir_all(workspace.path("src"));
        let _ = std::fs::remove_file(workspace.path("notes.txt"));
        workspace.write("forge-mcp.yaml", config);
        for &(file_path, content) in files {
            workspace.write(file_path, content);
        }

        match (scanner::scan(workspace.root(), None), expected) {
            (Err(err), Expected::Error(expected_msg)) => {
                assert!(
                    err.to_string().contains(expected_msg),
                    "{name}: expected error containing '{expected_msg}', got: {err}"
                );
            }
            (Ok(report), Expected::SuccessFiles(expected_count)) => {
                assert_eq!(
                    report.files, expected_count,
                    "{name}: expected {expected_count} files, got {}",
                    report.files
                );
            }
            (result, _) => {
                panic!("{name}: unexpected result: {result:?}");
            }
        }
    }
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
fn scanner_rejects_a_workspace_over_the_default_file_limit() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    let source = workspace.root().join("src");
    fs::create_dir_all(&source).unwrap();
    let mut created_paths = 0;
    for group_start in (0..=scanner::DEFAULT_MAX_FILES).step_by(100) {
        let group_end = (group_start + 100).min(scanner::DEFAULT_MAX_FILES + 1);
        let seed = workspace
            .root()
            .join(format!(".file-limit-seed-{group_start}.data"));
        fs::write(&seed, "x=1\n").unwrap();
        for index in group_start..group_end {
            // Distinct directory entries share fixture bytes in bounded groups.
            let path = source.join(format!("file_{index}.py"));
            match fs::hard_link(&seed, &path) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                    ) =>
                {
                    fs::write(path, "x=1\n").unwrap();
                }
                Err(error) => panic!("create distinct scanner fixture path: {error}"),
            }
            created_paths += 1;
        }
    }

    assert_eq!(scanner::DEFAULT_MAX_FILES, 100_000);
    assert_eq!(created_paths, 100_001);
    assert_eq!(created_paths, scanner::DEFAULT_MAX_FILES + 1);
    let error = scanner::scan(workspace.root(), None)
        .map(|report| report.files)
        .expect_err("one path beyond the default limit must be rejected");
    assert_eq!(
        error.to_string(),
        "workspace exceeds file limit (100001 > max_files 100000)"
    );
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
fn scanner_memory_helpers_reexport_the_core_system_contract() {
    let core_available: fn() -> u64 = contextunity_forge_mcp::core::sys::available_memory_bytes;
    let scanner_available: fn() -> u64 = scanner::available_memory_bytes;
    assert!(
        std::ptr::fn_addr_eq(scanner_available, core_available),
        "scanner must re-export the canonical core memory helper"
    );

    let core_budget: fn() -> u64 = contextunity_forge_mcp::core::sys::memory_budget_bytes;
    let scanner_budget: fn() -> u64 = scanner::memory_budget_bytes;
    assert!(
        std::ptr::fn_addr_eq(scanner_budget, core_budget),
        "scanner must re-export the canonical core budget helper"
    );

    let available = scanner_available();
    assert!(available > 0, "available memory must be positive");
    let budget = scanner_budget();
    assert!(
        (64 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&budget),
        "budget must retain its 64 MiB floor and 16 GiB cap"
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
