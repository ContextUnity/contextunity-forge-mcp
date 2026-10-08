use crate::common::Workspace;
use contextunity_forge_mcp::{core::debug_log, engine::scanner};
use std::fs;

#[test]
fn ignored_names_includes_forge() {
    let ws = Workspace::new();
    let adapter = scanner::load_adapter(ws.root(), None).unwrap();
    assert!(adapter.ignored_names.contains(".forge"));
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
    let adapter_no_debug = scanner::load_adapter(ws.root(), None).unwrap();
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
    let adapter_debug_caps = scanner::load_adapter(ws.root(), None).unwrap();
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
    let adapter_debug_lower = scanner::load_adapter(ws.root(), None).unwrap();
    assert!(adapter_debug_lower.debug);

    // Digest must NOT change when toggling debug mode!
    assert_eq!(
        adapter_no_debug.digest, adapter_debug_caps.digest,
        "DEBUG: true must not change adapter digest / Merkle commitment"
    );
    assert_eq!(
        adapter_no_debug.digest, adapter_debug_lower.digest,
        "debug: true must not change adapter digest / Merkle commitment"
    );
}

#[test]
fn debug_logger_writes_to_forge_directory() {
    let ws = Workspace::new();
    let date_str = chrono::Local::now().format("%Y-%m-%d").to_string();
    let expected_jsonl_file =
        ws.root().join(".forge")
            .join(format!("command-log-debug--{date_str}.jsonl"));

    assert!(!expected_jsonl_file.exists());

    // 1. CLI entry
    debug_log::log_cli(
        ws.root(),
        "query overview",
        None,
        &serde_json::json!({"status": "ok"}),
    );

    // 2. MCP entry
    debug_log::log_mcp(
        ws.root(),
        "code_map_inspect",
        "{\"selector\": \"foo::bar\"}",
        &serde_json::json!({"found": true}),
    );

    assert!(
        expected_jsonl_file.exists(),
        "JSONL log file must be created"
    );

    let content = fs::read_to_string(&expected_jsonl_file).unwrap();
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 2);

    let cli_entry: serde_json::Value = serde_json::from_str(lines[0]).expect("valid jsonl line 1");
    assert_eq!(cli_entry["source"], "cli");
    assert_eq!(cli_entry["command"], "query overview");
    assert_eq!(cli_entry["output"]["status"], "ok");

    let mcp_entry: serde_json::Value = serde_json::from_str(lines[1]).expect("valid jsonl line 2");
    assert_eq!(mcp_entry["source"], "mcp");
    assert_eq!(mcp_entry["tool"], "code_map_inspect");
    assert_eq!(mcp_entry["parameters"]["selector"], "foo::bar");
    assert_eq!(mcp_entry["output"]["found"], true);
}
