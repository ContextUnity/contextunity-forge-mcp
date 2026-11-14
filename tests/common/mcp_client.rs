use crate::common::Workspace;
use contextunity_forge_mcp::mcp::server::Server;
use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    io::{Read, Write},
    os::unix::process::CommandExt,
    process::{Child, ChildStdin, Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
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
    .with_cache_dir(workspace.path(".forge/extraction-cache"))
}

/// Runs the shipped CLI against the same isolated workspace used by test clients.
pub fn run_cli(workspace: &Workspace, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .arg("--root")
        .arg(workspace.root())
        .env(
            "FORGE_EXTRACTION_CACHE_DIR",
            workspace.path(".forge/extraction-cache"),
        )
        .args(args)
        .output()
        .expect("could not run contextunity-forge-mcp")
}

#[derive(Debug)]
pub enum StdioError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Timeout,
    Eof,
    ReaderDisconnected,
    OversizedFrame { actual: usize, limit: usize },
    Rpc(Value),
}

impl std::fmt::Display for StdioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "stdio I/O: {e}"),
            Self::Json(e) => write!(f, "stdio JSON: {e}"),
            Self::Timeout => f.write_str("stdio timeout"),
            Self::Eof => f.write_str("stdio clean EOF before response"),
            Self::ReaderDisconnected => f.write_str("stdio reader ended without EOF observation"),
            Self::OversizedFrame { actual, limit } => {
                write!(f, "stdio frame {actual} exceeds {limit} bytes")
            }
            Self::Rpc(response) => write!(f, "stdio RPC error: {response}"),
        }
    }
}
impl std::error::Error for StdioError {}
impl From<std::io::Error> for StdioError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for StdioError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
pub type StdioResult<T> = Result<T, StdioError>;

enum ReadEvent {
    Frame(String),
    Eof,
    Io(std::io::Error),
    Oversized(usize),
}

fn kill_group(child: &mut Child) {
    // SAFETY: each child is started as leader of its own process group.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransportEnd {
    CleanEof,
    TimedOut,
}

/// A JSON-RPC client backed by the real server binary and stdio transport.
pub struct StdioClient {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<ReadEvent>,
    reader: Option<std::thread::JoinHandle<()>>,
    reader_stop: Arc<AtomicBool>,
    id: u64,
}

impl StdioClient {
    pub fn with_binary(workspace: &Workspace, binary: impl AsRef<OsStr>) -> StdioResult<Self> {
        let mut command = Command::new(binary);
        command
            .arg("--root")
            .arg(workspace.root())
            .arg("serve")
            .env(
                "FORGE_EXTRACTION_CACHE_DIR",
                workspace.path(".forge/extraction-cache"),
            );
        Self::with_command(&mut command)
    }

    pub fn with_command(command: &mut Command) -> StdioResult<Self> {
        let mut client = Self::with_pending_initialization(command)?;
        client.notify("notifications/initialized", json!({}))?;
        Ok(client)
    }

    pub fn with_pending_initialization(command: &mut Command) -> StdioResult<Self> {
        let spawn_deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut child = loop {
            match command
                .process_group(0)
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
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        };
        let (Some(input), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            kill_group(&mut child);
            return Err(std::io::Error::other("server stdio is not piped").into());
        };
        use std::os::fd::AsRawFd;
        // SAFETY: stdout owns a live descriptor; fcntl reads and updates its flags.
        let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            let error = std::io::Error::last_os_error();
            kill_group(&mut child);
            return Err(error.into());
        }
        let (sender, output) = mpsc::channel();
        let reader_stop = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&reader_stop);
        let reader = std::thread::spawn(move || {
            let mut reader = stdout;
            let mut pending = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let count = match reader.read(&mut chunk) {
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(error) => {
                        let _ = sender.send(ReadEvent::Io(error));
                        break;
                    }
                };
                for &byte in &chunk[..count] {
                    pending.push(byte);
                    if pending.len() > contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES {
                        let _ = sender.send(ReadEvent::Oversized(pending.len()));
                        return;
                    }
                    if byte == b'\n' {
                        let event = match String::from_utf8(std::mem::take(&mut pending)) {
                            Ok(line) => ReadEvent::Frame(line),
                            Err(error) => ReadEvent::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                error,
                            )),
                        };
                        let terminal = !matches!(event, ReadEvent::Frame(_));
                        if sender.send(event).is_err() || terminal {
                            return;
                        }
                    }
                }
                if count == 0 {
                    let event = if pending.is_empty() {
                        ReadEvent::Eof
                    } else {
                        ReadEvent::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unterminated stdout frame",
                        ))
                    };
                    let _ = sender.send(event);
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            reader: Some(reader),
            reader_stop,
            id: 0,
        };
        let (_, response) = client.request(
            "initialize",
            json!({
                "protocolVersion": INITIALIZE_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "context-test", "version": "1"}
            }),
        )?;
        if response.get("error").is_some() {
            return Err(StdioError::Rpc(response));
        }
        Ok(client)
    }

    #[cfg(test)]
    pub fn with_dropped_reader_for_test(command: &mut Command) -> StdioResult<Self> {
        let mut child = command
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = match child.stdin.take() {
            Some(input) => input,
            None => {
                kill_group(&mut child);
                return Err(std::io::Error::other("server stdin is not piped").into());
            }
        };
        match child.stdout.take() {
            Some(stdout) => drop(stdout),
            None => {
                kill_group(&mut child);
                return Err(std::io::Error::other("server stdout is not piped").into());
            }
        }
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || drop(sender))
            .join()
            .map_err(|_| std::io::Error::other("test reader thread panicked"))?;
        Ok(Self {
            child,
            input,
            output,
            reader: None,
            reader_stop: Arc::new(AtomicBool::new(false)),
            id: 0,
        })
    }

    pub fn request(&mut self, method: &str, params: Value) -> StdioResult<(usize, Value)> {
        self.id += 1;
        let request_id = self.id;
        self.write_message(
            json!({"jsonrpc":"2.0", "id":request_id, "method":method, "params":params}),
        )?;
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let line = match self
                .output
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(ReadEvent::Frame(line)) => line,
                Ok(ReadEvent::Eof) => return Err(StdioError::Eof),
                Ok(ReadEvent::Io(error)) => return Err(error.into()),
                Ok(ReadEvent::Oversized(actual)) => {
                    return Err(StdioError::OversizedFrame {
                        actual,
                        limit: contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES,
                    })
                }
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(StdioError::Timeout),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(StdioError::ReaderDisconnected);
                }
            };
            let limit = contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES;
            if line.len() > limit {
                return Err(StdioError::OversizedFrame {
                    actual: line.len(),
                    limit,
                });
            }
            let response: Value = serde_json::from_str(&line)?;
            if response["id"] == request_id {
                return Ok((line.len(), response));
            }
        }
    }

    pub fn request_value(&mut self, method: &str, params: Value) -> StdioResult<Value> {
        self.request(method, params).map(|(_, response)| response)
    }

    pub fn child(&self) -> &Child {
        &self.child
    }

    pub fn terminate_launcher(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.kill()?;
        self.child.wait()
    }

    pub fn wait_for_transport_close(&self, timeout: Duration) -> StdioResult<TransportEnd> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self
                .output
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(ReadEvent::Eof) => return Ok(TransportEnd::CleanEof),
                Ok(ReadEvent::Io(error)) => return Err(error.into()),
                Ok(ReadEvent::Oversized(actual)) => {
                    return Err(StdioError::OversizedFrame {
                        actual,
                        limit: contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES,
                    })
                }
                Ok(ReadEvent::Frame(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(TransportEnd::TimedOut),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(StdioError::ReaderDisconnected);
                }
            }
        }
    }

    pub fn notify(&mut self, method: &str, params: Value) -> std::io::Result<()> {
        self.write_message(json!({"jsonrpc":"2.0", "method":method, "params":params}))
    }

    fn write_message(&mut self, message: Value) -> std::io::Result<()> {
        writeln!(self.input, "{message}")?;
        self.input.flush()
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        self.reader_stop.store(true, Ordering::Release);
        kill_group(&mut self.child);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
