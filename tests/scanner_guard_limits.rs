use contextunity_forge_mcp::engine::scanner;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge_guard_test_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn write_bytes(&self, path: &str, content: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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

    let err = scanner::check_root_scope(&workspace.0, false);
    assert!(err.is_err(), "Multi-repo container must be rejected");
    assert!(err
        .unwrap_err()
        .to_string()
        .contains("multi-repository container"));

    // Allowed if allow_broad is true
    assert!(scanner::check_root_scope(&workspace.0, true).is_ok());

    // Allowed if root has forge-mcp.yaml manifest
    workspace.write("forge-mcp.yaml", "limits:\n  allow_broad_root: true\n");
    assert!(scanner::check_root_scope(&workspace.0, false).is_ok());
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

    let adapter = scanner::load_adapter(&workspace.0, None).unwrap();
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

    let err = scanner::scan(&workspace.0, None);
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
        workspace.write(&format!("src/{name}"), "x = 1\n");
    }

    let error = scanner::scan(&workspace.0, None).unwrap_err();
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

    let error = scanner::scan(&workspace.0, None).unwrap_err();
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

    let report = scanner::scan(&workspace.0, None).unwrap();
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
    assert!(scanner::scan(&workspace.0, None).is_ok());

    workspace.write_bytes(
        "src/oversized.py",
        &vec![b'x'; scanner::DEFAULT_MAX_FILE_BYTES as usize + 1],
    );
    let error = scanner::scan(&workspace.0, None).unwrap_err();
    assert!(error.to_string().contains("file exceeds byte limit"));
}

#[test]
#[ignore = "creates more than 100,000 files; run only on a dedicated filesystem stress lane"]
fn scanner_rejects_a_workspace_over_the_default_file_limit() {
    let workspace = Workspace::new();
    workspace.write(".git/config", "");
    let source = workspace.0.join("src");
    fs::create_dir_all(&source).unwrap();
    for index in 0..=scanner::DEFAULT_MAX_FILES {
        fs::write(source.join(format!("file_{index}.py")), "x=1\n").unwrap();
    }

    let error = scanner::scan(&workspace.0, None).unwrap_err();
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
