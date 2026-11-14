use crate::common::Workspace;
use contextunity_forge_mcp::{core::debug_log, engine::scanner};
use std::fs;
use std::process::Command;

struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
    let expected_jsonl_file = ws
        .root()
        .join(".forge")
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
    let root = ws.root().to_path_buf();
    fs::remove_dir_all(&root).unwrap();
    assert!(!root.exists());
}

#[test]
fn debug_log_obstructions_report_errors_without_breaking_requests() {
    use std::io::{Read, Write};
    #[cfg(unix)]
    use std::os::fd::OwnedFd;
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;

    for source in ["cli", "mcp"] {
        for obstruction in ["directory", "file"] {
            for broken_stderr in [false, true] {
                if broken_stderr && !cfg!(unix) {
                    continue;
                }
                let ws = Workspace::new();
                ws.write(
                    "forge-mcp.yaml",
                    "version: 1\ndebug: true\nroots:\n  - '.'\n",
                );
                ws.write(
                    "secret-request-content",
                    "version: 1\ndebug: true\nroots:\n  - '.'\n",
                );
                let directory = ws.path(".forge");
                let destination = if obstruction == "directory" {
                    fs::write(&directory, "obstruction").unwrap();
                    directory.clone()
                } else {
                    fs::create_dir(&directory).unwrap();
                    let date = chrono::Local::now().format("%Y-%m-%d");
                    let file = directory.join(format!("command-log-debug--{date}.jsonl"));
                    fs::create_dir(&file).unwrap();
                    file
                };
                let (operation, reason) = if obstruction == "directory" {
                    (
                        "create directory",
                        fs::create_dir_all(&directory).unwrap_err().to_string(),
                    )
                } else {
                    (
                        "open",
                        fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&destination)
                            .unwrap_err()
                            .to_string(),
                    )
                };
                let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
                command
                    .arg("--root")
                    .arg(ws.root())
                    .arg("--db")
                    .arg(ws.path("index.sqlite"))
                    .arg("--format")
                    .arg("json")
                    .env("FORGE_EXTRACTION_CACHE_DIR", ws.path("cache"))
                    .stdout(Stdio::piped());
                if source == "cli" {
                    command
                        .arg("--config")
                        .arg("secret-request-content")
                        .arg("scan");
                } else {
                    command.arg("serve").stdin(Stdio::piped());
                }
                if broken_stderr {
                    #[cfg(unix)]
                    {
                        let (reader, writer) = UnixStream::pair().unwrap();
                        drop(reader);
                        command.stderr(Stdio::from(OwnedFd::from(writer)));
                    }
                } else {
                    command.stderr(Stdio::piped());
                }
                let mut child = ChildGuard(command.spawn().unwrap());
                let mut stdout_pipe = child.0.stdout.take().unwrap();
                let stdout_reader = std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    stdout_pipe.read_to_end(&mut bytes).unwrap();
                    bytes
                });
                let stderr_reader = child.0.stderr.take().map(|mut pipe| {
                    std::thread::spawn(move || {
                        let mut bytes = Vec::new();
                        pipe.read_to_end(&mut bytes).unwrap();
                        bytes
                    })
                });
                if source == "mcp" {
                    let mut input = child.0.stdin.take().unwrap();
                    writeln!(input, "{}", serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).unwrap();
                    writeln!(
                        input,
                        "{}",
                        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
                    )
                    .unwrap();
                    writeln!(input, "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"forge_guide\",\"arguments\":{{\"topic\":\"query\",\"secret\":\"secret-request-content\"}}}}}}").unwrap();
                }
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                let status = loop {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        break status;
                    }
                    assert!(std::time::Instant::now() < deadline, "{source} timed out");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                };
                let stdout = String::from_utf8(stdout_reader.join().unwrap()).unwrap();
                let stderr = String::from_utf8(
                    stderr_reader
                        .map(|reader| reader.join().unwrap())
                        .unwrap_or_default(),
                )
                .unwrap();
                assert!(status.success(), "{source}: {stdout}\n{stderr}");
                let response: serde_json::Value =
                    serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
                if source == "mcp" {
                    assert_eq!(response["id"], 1);
                    assert!(response.get("result").is_some(), "{response}");
                    assert_ne!(response["result"]["isError"], true, "{response}");
                }
                if !broken_stderr {
                    assert!(stderr.contains(operation), "{stderr}");
                    assert!(
                        stderr.contains(&destination.display().to_string()),
                        "{stderr}"
                    );
                    assert!(stderr.contains(&reason), "{stderr}");
                    assert!(!stderr.contains("secret-request-content"));
                }
                if obstruction == "directory" {
                    fs::remove_file(&directory).unwrap();
                } else {
                    fs::remove_dir(&destination).unwrap();
                    fs::remove_dir_all(&directory).unwrap();
                }
                let root = ws.root().to_path_buf();
                fs::remove_dir_all(&root).unwrap();
                assert!(!root.exists());
            }
        }
    }
}
