use contextunity_forge_mcp::engine::scanner;
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_scanner_scope_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "def source(): pass\n").unwrap();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_scope_excludes_common_generated_directories() {
    let workspace = Workspace::new();
    workspace.write("src/main.py");
    workspace.write("src/build_helpers.py");
    for directory in [
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
        workspace.write(&format!("{directory}/generated.py"));
    }

    let report = scanner::scan(&workspace.0, None).unwrap();
    let paths: Vec<_> = report.entries.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(paths, ["src/build_helpers.py", "src/main.py"]);
}
