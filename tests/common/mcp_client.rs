use crate::common::{assertions, Workspace};
use contextunity_forge_mcp::mcp::server::Server;
use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};

/// Changes the database mtime so the server re-admits its cached connection.
/// This triggers source-inventory validation without waiting for the production TTL.
pub fn invalidate_mcp_connection_cache(database: &std::path::Path) -> std::io::Result<()> {
    let file = std::fs::File::open(database)?;
    let modified = file.metadata()?.modified()?;
    file.set_times(
        std::fs::FileTimes::new().set_modified(modified + std::time::Duration::from_secs(1)),
    )?;
    Ok(())
}

const INITIALIZE_PROTOCOL_VERSION: &str = "2025-03-26";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Constructs the real MCP server with an isolated workspace for in-process checks.
pub fn in_process_server(workspace: &Workspace) -> Server {
    Server::new(
        workspace.root().to_path_buf(),
        workspace.root().join(".forge/index.sqlite"),
    )
}

/// Runs the shipped CLI against the same isolated workspace used by test clients.
pub fn run_cli(workspace: &Workspace, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .arg("--root")
        .arg(workspace.root())
        .args(args)
        .output()
        .expect("could not run contextunity-forge-mcp")
}

/// Runs the shipped CLI in JSON mode for callers that decode stdout as JSON.
pub fn run_cli_json(workspace: &Workspace, args: &[&str]) -> Output {
    let mut json_args = Vec::with_capacity(args.len() + 2);
    json_args.extend(["--format", "json"]);
    json_args.extend_from_slice(args);
    run_cli(workspace, &json_args)
}

/// A JSON-RPC client backed by the real server binary and stdio transport.
pub struct StdioClient {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    id: u64,
}

impl StdioClient {
    /// Starts the Cargo-built server binary and completes MCP initialization.
    pub fn new(workspace: &Workspace) -> Self {
        Self::with_binary(workspace, env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
    }

    /// Starts a caller-selected server binary, preserving real process boundaries.
    pub fn with_binary(workspace: &Workspace, binary: impl AsRef<OsStr>) -> Self {
        let mut command = Command::new(binary);
        command.arg("--root").arg(workspace.root()).arg("serve");
        Self::with_command(&mut command)
    }

    /// Initializes a real stdio server launched through a process fixture.
    pub fn with_command(command: &mut Command) -> Self {
        let mut client = Self::with_pending_initialization(command);
        client.notify("notifications/initialized", json!({}));
        client
    }

    /// Receives initialize response, leaving the initialized notification pending.
    pub fn with_pending_initialization(command: &mut Command) -> Self {
        let spawn_deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut child = loop {
            match command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => break child,
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && std::time::Instant::now() < spawn_deadline =>
                {
                    // A concurrent spawn may briefly inherit the copied reload binary's writer.
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("could not start MCP server process: {error}"),
            }
        };
        let input = child
            .stdin
            .take()
            .expect("MCP server stdin should be piped");
        let stdout = child
            .stdout
            .take()
            .expect("MCP server stdout should be piped");
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || sender.send(line).is_err() {
                    break;
                }
            }
        });

        let mut client = Self {
            child,
            input,
            output,
            id: 0,
        };
        let (_, response) = client.request(
            "initialize",
            json!({
                "protocolVersion": INITIALIZE_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "context-test", "version": "1"}
            }),
        );
        assertions::assert_rpc_succeeded(&response);
        client
    }

    /// Sends one request and returns the frame size and decoded JSON-RPC response.
    pub fn request(&mut self, method: &str, params: Value) -> (usize, Value) {
        self.id += 1;
        let request_id = self.id;
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        }));

        loop {
            let line = self
                .output
                .recv_timeout(RESPONSE_TIMEOUT)
                .expect("MCP response missing or timed out");
            assert!(
                line.len() <= contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES,
                "JSON-RPC frame was {} bytes",
                line.len()
            );
            let response: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
                panic!("MCP server returned invalid JSON: {error}: {line}")
            });
            if response["id"] == request_id {
                return (line.len(), response);
            }
        }
    }

    /// Sends one request when only the decoded JSON-RPC response is needed.
    pub fn request_value(&mut self, method: &str, params: Value) -> Value {
        self.request(method, params).1
    }

    /// Calls one registered MCP tool and returns its frame size and response.
    pub fn call(&mut self, name: &str, arguments: Value) -> (usize, Value) {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    /// Calls one MCP tool and parses the JSON payload from its first text block.
    pub fn payload(&mut self, name: &str, arguments: Value) -> Value {
        let mut arguments = arguments;
        if arguments
            .as_object()
            .is_some_and(|arguments| !arguments.contains_key("format"))
        {
            arguments["format"] = json!("json");
        }
        let (_, response) = self.call(name, arguments);
        assertions::mcp_payload(&response)
    }

    /// Exposes the child handle for process-level assertions such as PID stability.
    pub fn child(&self) -> &Child {
        &self.child
    }

    /// Ends the launcher while retaining the client's open server transport.
    pub fn terminate_launcher(&mut self) {
        self.child.kill().expect("could not kill launcher");
        self.child.wait().expect("could not reap launcher");
    }

    /// Observes transport closure without closing the client's input pipe.
    pub fn wait_for_transport_close(&self, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self
                .output
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("MCP transport remained open"),
                Ok(_) => {}
            }
        }
    }

    /// Sends an MCP notification without waiting for a response.
    pub fn notify(&mut self, method: &str, params: Value) {
        self.write_message(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn write_message(&mut self, message: Value) {
        writeln!(self.input, "{message}").expect("could not write to MCP server stdin");
        self.input
            .flush()
            .expect("could not flush MCP server stdin");
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
