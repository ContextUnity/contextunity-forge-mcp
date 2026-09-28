use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("forge_cli_root_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("sample.py"), "def sample():\n    return 1\n").unwrap();
        Self(path)
    }

    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn build_uses_global_root_when_positional_root_is_omitted() {
    let workspace = Workspace::new();
    let root = workspace.0.to_str().unwrap();
    let result = cli(&["--root", root, "build"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(workspace.db().exists());
    let inspect = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(String::from_utf8_lossy(&inspect.stdout).contains("sample.py"));
}

#[test]
fn positional_root_build_remains_supported() {
    let workspace = Workspace::new();
    let result = cli(&["build", workspace.0.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(workspace.db().exists());
}

#[test]
fn conflicting_roots_are_rejected_for_build_scan_and_delta() {
    let first = Workspace::new();
    let second = Workspace::new();
    for command in ["build", "scan", "delta"] {
        let mut args = vec![
            "--root",
            first.0.to_str().unwrap(),
            command,
            second.0.to_str().unwrap(),
        ];
        if command == "delta" {
            args.push("sample.py");
        }
        let result = cli(&args);
        assert!(
            !result.status.success(),
            "{command} accepted conflicting roots"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("conflicting workspace roots"));
    }
    assert!(!first.db().exists());
    assert!(!second.db().exists());
}

#[test]
fn scan_uses_global_root_when_positional_root_is_omitted() {
    let workspace = Workspace::new();
    let result = cli(&["--root", workspace.0.to_str().unwrap(), "scan"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("sample.py"));
}

#[test]
fn delta_uses_global_root_with_named_modified_file() {
    let workspace = Workspace::new();
    let root = workspace.0.to_str().unwrap();
    let build = cli(&["--root", root, "build"]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    fs::write(
        workspace.0.join("sample.py"),
        "def updated():\n    return 2\n",
    )
    .unwrap();
    let delta = cli(&["--root", root, "delta", "--modified", "sample.py"]);
    assert!(
        delta.status.success(),
        "{}",
        String::from_utf8_lossy(&delta.stderr)
    );
    let inspect = cli(&["--root", root, "query", "inspect", "sample.py:updated"]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(String::from_utf8_lossy(&inspect.stdout).contains("updated"));
}
