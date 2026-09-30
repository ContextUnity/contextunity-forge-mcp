use contextunity_forge_mcp::{
    core::debug_log,
    engine::scanner,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("forge_debug_test_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, path: &str, text: &str) {
        let target = self.0.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, text).unwrap();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ignored_names_includes_forge_mcp() {
    let ws = Workspace::new();
    let adapter = scanner::load_adapter(&ws.0, None).unwrap();
    assert!(adapter.ignored_names.contains(".forge-mcp"));
}

#[test]
fn debug_flag_parsing_and_digest_determinism() {
    let ws = Workspace::new();

    // 1. Adapter without DEBUG
    ws.write(
        "forge-mcp.yaml",
        r#"
version: 1
roots:
  - "."
"#,
    );
    let adapter_no_debug = scanner::load_adapter(&ws.0, None).unwrap();
    assert!(!adapter_no_debug.debug);

    // 2. Adapter with DEBUG: true
    ws.write(
        "forge-mcp.yaml",
        r#"
version: 1
DEBUG: true
roots:
  - "."
"#,
    );
    let adapter_debug_caps = scanner::load_adapter(&ws.0, None).unwrap();
    assert!(adapter_debug_caps.debug);

    // 3. Adapter with debug: true
    ws.write(
        "forge-mcp.yaml",
        r#"
version: 1
debug: true
roots:
  - "."
"#,
    );
    let adapter_debug_lower = scanner::load_adapter(&ws.0, None).unwrap();
    assert!(adapter_debug_lower.debug);

    // Digest must NOT change when toggling debug mode!
    assert_eq!(
        adapter_no_debug.digest,
        adapter_debug_caps.digest,
        "DEBUG: true must not change adapter digest / Merkle commitment"
    );
    assert_eq!(
        adapter_no_debug.digest,
        adapter_debug_lower.digest,
        "debug: true must not change adapter digest / Merkle commitment"
    );
}

#[test]
fn debug_logger_writes_to_forge_mcp_directory() {
    let ws = Workspace::new();
    let date_str = chrono::Local::now().format("%Y-%m-%d").to_string();
    let expected_log_file = ws.0.join(".forge-mcp").join(format!("command-log-debug--{date_str}.log"));

    assert!(!expected_log_file.exists());

    debug_log::log_command(
        &ws.0,
        "test_tool",
        Some("{\"arg\": \"value\"}"),
        "{\"result\": \"success\"}",
    );

    assert!(expected_log_file.exists(), "Log file must be created");
    let content = fs::read_to_string(&expected_log_file).unwrap();
    assert!(content.contains("COMMAND: test_tool"));
    assert!(content.contains("INPUT:\n{\"arg\": \"value\"}"));
    assert!(content.contains("OUTPUT:\n{\"result\": \"success\"}"));
}
