//! In-process MCP transport shared by integration test suites.

use crate::common::Workspace;
use contextunity_forge_mcp::mcp::{response::BoundedWriter, server::Server};
use rmcp::{
    model::{CallToolRequestParams, CallToolResponse, NumberOrString},
    service::{serve_directly, RequestContext},
    Peer, RoleServer, ServerHandler, ServiceExt,
};
use serde_json::{json, Map, Value};
use std::{
    future::Future,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

#[derive(Debug)]
pub enum HarnessError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Timeout,
    Closed,
    Server { phase: &'static str, cause: String },
    OversizedFrame { actual: usize, limit: usize },
    Rpc(Value),
    Tool(Value),
    MalformedResponse(Value),
    Unsupported { method: String },
}

impl HarnessError {
    pub fn tool_message(&self) -> Option<&str> {
        match self {
            Self::Tool(response) => response["result"]["content"][0]["text"].as_str(),
            _ => None,
        }
    }
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "MCP I/O failure: {error}"),
            Self::Json(error) => write!(f, "MCP JSON failure: {error}"),
            Self::Timeout => f.write_str("MCP response timed out"),
            Self::Closed => f.write_str("MCP transport closed"),
            Self::Server { phase, cause } => write!(f, "MCP server {phase}: {cause}"),
            Self::OversizedFrame { actual, limit } => {
                write!(f, "MCP frame is {actual} bytes; limit is {limit}")
            }
            Self::Rpc(response) => write!(f, "MCP RPC error: {response}"),
            Self::Tool(_) => write!(
                f,
                "MCP tool error: {}",
                self.tool_message().unwrap_or("missing error text")
            ),
            Self::MalformedResponse(response) => {
                write!(f, "MCP response has an invalid payload shape: {response}")
            }
            Self::Unsupported { method } => {
                write!(f, "direct MCP dispatch does not implement {method}")
            }
        }
    }
}

impl std::error::Error for HarnessError {}

pub type HarnessResult<T> = Result<T, HarnessError>;

enum Mode {
    Direct {
        server: Server,
    },
    Duplex {
        input: WriteHalf<DuplexStream>,
        output: BufReader<ReadHalf<DuplexStream>>,
        terminal: Option<Result<(), (&'static str, String)>>,
    },
}

/// Tool logic through `ServerHandler::call_tool` / `list_tools`.
/// Duplex JSON-RPC stays for handshake and cancellation contracts.
pub struct McpTestHarness {
    owned_runtime: Option<tokio::runtime::Runtime>,
    mode: Mode,
    id: u64,
    pub(crate) _workspace_owner: Option<Arc<Workspace>>,
    pub(crate) server_task: Option<tokio::task::JoinHandle<HarnessResult<()>>>,
    pub(crate) server_control:
        Option<tokio::sync::oneshot::Receiver<rmcp::service::RunningServiceCancellationToken>>,
}

struct DirectProtocol {
    _running: Mutex<rmcp::service::RunningService<RoleServer, Server>>,
    _client: Mutex<DuplexStream>,
    runtime: tokio::runtime::Runtime,
    peer: Peer<RoleServer>,
}

fn protocol() -> &'static DirectProtocol {
    static PROTOCOL: OnceLock<DirectProtocol> = OnceLock::new();
    PROTOCOL.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("direct MCP protocol runtime");
        let (client, transport) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(transport);
        let server = Server::new(std::path::PathBuf::new(), std::path::PathBuf::new());
        let (running, peer) = runtime.block_on(async {
            let running = serve_directly(server, (reader, BoundedWriter::new(writer)), None);
            let peer = running.peer().clone();
            (running, peer)
        });
        DirectProtocol {
            _running: Mutex::new(running),
            _client: Mutex::new(client),
            runtime,
            peer,
        }
    })
}

impl McpTestHarness {
    pub fn new(workspace: &Workspace) -> HarnessResult<Self> {
        static SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let session = SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::with_session(workspace, &format!("mcp-test-session-{session}"))
    }

    pub fn with_session(workspace: &Workspace, session: &str) -> HarnessResult<Self> {
        Self::with_root_and_db(workspace.root(), &workspace.db(), session)
    }

    pub fn with_root_and_db(
        root: &std::path::Path,
        database: &std::path::Path,
        session: &str,
    ) -> HarnessResult<Self> {
        let server = Server::new(root.to_path_buf(), database.to_path_buf())
            .with_cache_dir(root.join(".forge/extraction-cache"))
            .with_session_fingerprint(session.to_owned());
        Ok(Self {
            owned_runtime: None,
            mode: Mode::Direct { server },
            id: 0,
            _workspace_owner: None,
            server_task: None,
            server_control: None,
        })
    }

    pub fn spawn(
        root: &std::path::Path,
        database: &std::path::Path,
        session: &str,
    ) -> HarnessResult<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(HarnessError::Io)?;
        let (client, transport) = tokio::io::duplex(1024 * 1024);
        let (reader, writer) = tokio::io::split(transport);
        let server = Server::new(root.to_path_buf(), database.to_path_buf())
            .with_cache_dir(root.join(".forge/extraction-cache"))
            .with_session_fingerprint(session.to_owned());
        let (control_sender, control_receiver) = tokio::sync::oneshot::channel();
        let server_task = runtime.spawn(async move {
            let service = server
                .serve((reader, BoundedWriter::new(writer)))
                .await
                .map_err(|error| HarnessError::Server {
                    phase: "serve",
                    cause: error.to_string(),
                })?;
            let _receiver_closed = control_sender.send(service.cancellation_token());
            match service.waiting().await {
                Ok(rmcp::service::QuitReason::Closed) => Ok(()),
                Ok(reason) => Err(HarnessError::Server {
                    phase: "waiting",
                    cause: format!("{reason:?}"),
                }),
                Err(error) => Err(HarnessError::Server {
                    phase: "waiting",
                    cause: error.to_string(),
                }),
            }
        });
        let (reader, input) = tokio::io::split(client);
        Ok(Self {
            owned_runtime: Some(runtime),
            mode: Mode::Duplex {
                input,
                output: BufReader::new(reader),
                terminal: None,
            },
            id: 0,
            _workspace_owner: None,
            server_task: Some(server_task),
            server_control: Some(control_receiver),
        })
    }

    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        match &self.owned_runtime {
            Some(runtime) => runtime.block_on(future),
            None => protocol().runtime.handle().block_on(future),
        }
    }

    pub fn finish(mut self) -> HarnessResult<()> {
        if matches!(self.mode, Mode::Direct { .. }) {
            return Ok(());
        }
        self.owned_runtime
            .as_ref()
            .expect("duplex runtime")
            .block_on(async {
                tokio::task::yield_now().await;
                let already_finished = self
                    .server_task
                    .as_ref()
                    .is_none_or(|task| task.is_finished());
                if !already_finished {
                    if let Some(receiver) = self.server_control.take() {
                        let control =
                            match tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
                                .await
                            {
                                Ok(result) => result.map(|_| ()).map_err(|error| error.to_string()),
                                Err(error) => Err(error.to_string()),
                            };
                        if let Err(cause) = control {
                            let terminal = duplex_terminal(&mut self.mode);
                            Self::await_server(&mut self.server_task, terminal, true).await?;
                            return Err(HarnessError::Server {
                                phase: "control",
                                cause,
                            });
                        }
                    }
                }
                let shutdown = if already_finished {
                    Ok(())
                } else if let Mode::Duplex { input, .. } = &mut self.mode {
                    input.shutdown().await
                } else {
                    Ok(())
                };
                let terminal = duplex_terminal(&mut self.mode);
                Self::await_server(&mut self.server_task, terminal, true).await?;
                shutdown.map_err(HarnessError::Io)?;
                if already_finished {
                    return Err(HarnessError::Server {
                        phase: "unexpected-close",
                        cause: "server ended before requested shutdown".into(),
                    });
                }
                Ok(())
            })
    }

    async fn await_server(
        task: &mut Option<tokio::task::JoinHandle<HarnessResult<()>>>,
        terminal: &mut Option<Result<(), (&'static str, String)>>,
        bounded: bool,
    ) -> HarnessResult<()> {
        if let Some(handle) = task.as_mut() {
            let joined = if bounded {
                tokio::time::timeout(std::time::Duration::from_secs(5), &mut *handle).await
            } else {
                Ok((&mut *handle).await)
            };
            let result = match joined {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => Err(HarnessError::Server {
                    phase: "join",
                    cause: error.to_string(),
                }),
                Err(_) => {
                    handle.abort();
                    let cancelled = handle.await;
                    Err(HarnessError::Server {
                        phase: "shutdown-timeout",
                        cause: format!("server did not finish in5s; abort result: {cancelled:?}"),
                    })
                }
            };
            *terminal = Some(result.map_err(|error| match error {
                HarnessError::Server { phase, cause } => (phase, cause),
                error => ("task", error.to_string()),
            }));
            *task = None;
        }
        match terminal.as_ref() {
            Some(Ok(())) => Ok(()),
            Some(Err((phase, cause))) => Err(HarnessError::Server {
                phase,
                cause: cause.clone(),
            }),
            None => Err(HarnessError::Closed),
        }
    }

    fn next_id(&mut self) -> u64 {
        self.id += 1;
        self.id
    }

    fn request_context(id: u64) -> RequestContext<RoleServer> {
        RequestContext::new(
            NumberOrString::Number(i64::try_from(id).unwrap_or(1)),
            protocol().peer.clone(),
        )
    }

    fn envelope(id: u64, result: Value) -> (usize, Value) {
        let response = json!({"jsonrpc":"2.0","id":id,"result":result});
        let frame = serde_json::to_string(&response).unwrap_or_else(|_| response.to_string());
        (frame.len() + 1, response)
    }

    fn error_envelope(id: u64, error: rmcp::ErrorData) -> (usize, Value) {
        let response = json!({"jsonrpc":"2.0","id":id,"error":error});
        let frame = serde_json::to_string(&response).unwrap_or_else(|_| response.to_string());
        (frame.len() + 1, response)
    }

    pub fn request(&mut self, method: &str, params: Value) -> HarnessResult<(usize, Value)> {
        match method {
            "tools/call" if matches!(self.mode, Mode::Direct { .. }) => {
                let name = params["name"].as_str().unwrap_or_default().to_owned();
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                self.call(&name, arguments)
            }
            "tools/list" if matches!(self.mode, Mode::Direct { .. }) => {
                let id = self.next_id();
                let context = Self::request_context(id);
                let Mode::Direct { server } = &self.mode else {
                    return Err(HarnessError::Closed);
                };
                match protocol()
                    .runtime
                    .handle()
                    .block_on(server.list_tools(None, context))
                {
                    Ok(result) => {
                        let result = serde_json::to_value(result).map_err(HarnessError::Json)?;
                        Ok(Self::envelope(id, result))
                    }
                    Err(error) => Ok(Self::error_envelope(id, error)),
                }
            }
            _ if matches!(self.mode, Mode::Direct { .. }) => Err(HarnessError::Unsupported {
                method: method.to_owned(),
            }),
            _ => {
                let id = self.next_id();
                self.write(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))?;
                self.owned_runtime
                    .as_ref()
                    .expect("duplex runtime")
                    .block_on(async {
                        tokio::time::timeout(std::time::Duration::from_secs(30), async {
                            loop {
                                let mut line = String::new();
                                let (output, terminal) = match &mut self.mode {
                                    Mode::Duplex {
                                        output, terminal, ..
                                    } => (output, terminal),
                                    Mode::Direct { .. } => return Err(HarnessError::Closed),
                                };
                                let read = tokio::select! {
                                    biased;
                                    result = async {
                                        Self::await_server(&mut self.server_task, terminal, false).await
                                    } => {
                                        result?;
                                        return Err(HarnessError::Closed);
                                    }
                                    read = output.read_line(&mut line) => read.map_err(HarnessError::Io)?,
                                };
                                if read == 0 {
                                    let terminal = duplex_terminal(&mut self.mode);
                                    Self::await_server(&mut self.server_task, terminal, true).await?;
                                    return Err(HarnessError::Closed);
                                }
                                let limit = contextunity_forge_mcp::core::response::MAX_OUTPUT_BYTES;
                                if line.len() > limit {
                                    return Err(HarnessError::OversizedFrame {
                                        actual: line.len(),
                                        limit,
                                    });
                                }
                                let response: Value =
                                    serde_json::from_str(&line).map_err(HarnessError::Json)?;
                                if response["id"] == id {
                                    tokio::task::yield_now().await;
                                    if self
                                        .server_task
                                        .as_ref()
                                        .is_some_and(|task| task.is_finished())
                                    {
                                        let terminal = duplex_terminal(&mut self.mode);
                                        Self::await_server(
                                            &mut self.server_task,
                                            terminal,
                                            true,
                                        )
                                        .await?;
                                        return Err(HarnessError::Closed);
                                    }
                                    return Ok((line.len(), response));
                                }
                            }
                        })
                        .await
                        .map_err(|_| HarnessError::Timeout)?
                    })
            }
        }
    }

    pub fn request_value(&mut self, method: &str, params: Value) -> HarnessResult<Value> {
        self.request(method, params).map(|(_, response)| response)
    }

    pub fn call(&mut self, name: &str, arguments: Value) -> HarnessResult<(usize, Value)> {
        if matches!(self.mode, Mode::Duplex { .. }) {
            return self.request("tools/call", json!({"name":name,"arguments":arguments}));
        }
        let id = self.next_id();
        let arguments = match arguments {
            Value::Null => Map::new(),
            Value::Object(arguments) => arguments,
            value => {
                let mut arguments = Map::new();
                arguments.insert("value".into(), value);
                arguments
            }
        };
        let request = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let context = Self::request_context(id);
        let Mode::Direct { server } = &self.mode else {
            return Err(HarnessError::Closed);
        };
        match protocol()
            .runtime
            .handle()
            .block_on(server.call_tool(request, context))
        {
            Ok(CallToolResponse::Complete(result)) => {
                let result = serde_json::to_value(result).map_err(HarnessError::Json)?;
                Ok(Self::envelope(id, result))
            }
            Ok(other) => {
                let text = format!("{other:?}");
                Ok(Self::envelope(
                    id,
                    json!({"isError":true,"content":[{"type":"text","text":text}]}),
                ))
            }
            Err(error) => Ok(Self::error_envelope(id, error)),
        }
    }

    pub fn payload(&mut self, name: &str, mut arguments: Value) -> HarnessResult<Value> {
        if arguments
            .as_object()
            .is_some_and(|args| !args.contains_key("format"))
        {
            arguments["format"] = json!("json");
        }
        serde_json::from_str(&self.text(name, arguments)?).map_err(HarnessError::Json)
    }

    pub fn text(&mut self, name: &str, arguments: Value) -> HarnessResult<String> {
        let (_, response) = self.call(name, arguments)?;
        if response.get("error").is_some() {
            return Err(HarnessError::Rpc(response));
        }
        if response["result"]["isError"] == true {
            return Err(HarnessError::Tool(response));
        }
        response["result"]["content"][0]["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| HarnessError::MalformedResponse(response))
    }

    pub fn notify(&mut self, method: &str, params: Value) -> HarnessResult<()> {
        match &self.mode {
            Mode::Direct { .. } => Ok(()),
            Mode::Duplex { .. } => {
                self.write(json!({"jsonrpc":"2.0","method":method,"params":params}))
            }
        }
    }

    fn write(&mut self, message: Value) -> HarnessResult<()> {
        let Mode::Duplex { input, .. } = &mut self.mode else {
            return Ok(());
        };
        self.owned_runtime
            .as_ref()
            .expect("duplex runtime")
            .block_on(async {
                input
                    .write_all(format!("{message}\n").as_bytes())
                    .await
                    .map_err(HarnessError::Io)?;
                input.flush().await.map_err(HarnessError::Io)
            })
    }
}

fn duplex_terminal(mode: &mut Mode) -> &mut Option<Result<(), (&'static str, String)>> {
    match mode {
        Mode::Duplex { terminal, .. } => terminal,
        Mode::Direct { .. } => unreachable!("duplex terminal is only used on the pipe path"),
    }
}
