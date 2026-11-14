#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::common::mcp_client::StdioClient;
use crate::common::Workspace;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{symlink, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const TARGETS: [(&str, &str, &str); 4] = [
    ("Linux", "x86_64", "x86_64-unknown-linux-gnu"),
    ("Linux", "aarch64", "aarch64-unknown-linux-gnu"),
    ("Darwin", "x86_64", "x86_64-apple-darwin"),
    ("Darwin", "arm64", "aarch64-apple-darwin"),
];

#[derive(Clone, Copy, Debug)]
enum ReleaseIntegrity {
    Verified,
    Mismatch,
    MissingAssetEntry,
    Truncated,
    HttpFailure,
    Large,
    Stall,
}

fn reusable_listener(address: std::net::SocketAddrV4) -> TcpListener {
    // SAFETY: the descriptor is immediately owned by TcpListener and all pointers
    // refer to initialized values of the sizes supplied to libc.
    unsafe {
        #[cfg(target_os = "linux")]
        let socket_type = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
        #[cfg(not(target_os = "linux"))]
        let socket_type = libc::SOCK_STREAM;
        let fd = libc::socket(libc::AF_INET, socket_type, 0);
        assert!(fd >= 0, "socket: {}", std::io::Error::last_os_error());
        let listener = TcpListener::from_raw_fd(fd);
        assert_eq!(
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC),
            0,
            "close-on-exec: {}",
            std::io::Error::last_os_error()
        );
        let enabled: libc::c_int = 1;
        assert_eq!(
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                (&enabled as *const libc::c_int).cast(),
                std::mem::size_of_val(&enabled) as libc::socklen_t
            ),
            0,
            "setsockopt: {}",
            std::io::Error::last_os_error()
        );
        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = address.port().to_be();
        addr.sin_addr.s_addr = u32::from_ne_bytes(address.ip().octets());
        #[cfg(target_os = "macos")]
        {
            addr.sin_len = std::mem::size_of_val(&addr) as u8;
        }
        assert_eq!(
            libc::bind(
                fd,
                (&addr as *const libc::sockaddr_in).cast(),
                std::mem::size_of_val(&addr) as libc::socklen_t
            ),
            0,
            "bind {address}: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(
            libc::listen(fd, 16),
            0,
            "listen: {}",
            std::io::Error::last_os_error()
        );
        listener
    }
}

// An immutable native release is shared across scenarios. Only the external
// GitHub HTTP origin is replaced; executable, tar, checksums and skill sync are real.
struct ReleaseFixture {
    archive: Vec<u8>,
    binary: Vec<u8>,
    binary_digest: Vec<u8>,
}

fn release_fixture() -> &'static ReleaseFixture {
    static RELEASE: OnceLock<ReleaseFixture> = OnceLock::new();
    RELEASE.get_or_init(|| {
        let workspace = Workspace::new();
        let binary = workspace.path("contextunity-forge-mcp");
        fs::copy(env!("CARGO_BIN_EXE_contextunity-forge-mcp"), &binary).unwrap();
        // Release assets are stripped too; avoid archiving debug information.
        setup_output(Command::new("strip").arg(&binary));
        let archive = workspace.path("release.tar.gz");
        setup_output(
            Command::new("tar")
                .args(["-hczf"])
                .arg(&archive)
                .arg("-C")
                .arg(workspace.root())
                .arg("contextunity-forge-mcp")
                .arg("-C")
                .arg(env!("CARGO_MANIFEST_DIR"))
                .arg("skills"),
        );
        let binary = fs::read(binary).unwrap();
        let binary_digest = Sha256::digest(&binary).to_vec();
        ReleaseFixture {
            archive: fs::read(archive).unwrap(),
            binary,
            binary_digest,
        }
    })
}

fn setup_output(command: &mut Command) -> Output {
    let output = command.output().expect("run fixture setup command");
    if !output.status.success() {
        panic!(
            "fixture setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output
}

struct InstallerHarness {
    workspace: Workspace,
    home: PathBuf,
    project: PathBuf,
    tools: PathBuf,
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    release_stall: Arc<AtomicBool>,
    archive: Arc<Vec<u8>>,
    integrity: Arc<Mutex<ReleaseIntegrity>>,
    server: Option<JoinHandle<()>>,
}

impl InstallerHarness {
    fn new(integrity: ReleaseIntegrity) -> Self {
        let release = release_fixture();
        let workspace = Workspace::new();
        let home = workspace.path("home with \"quotes\" and space");
        let project = workspace.path("project");
        let tools = workspace.path("tools");
        for directory in [&home, &project, &tools, &workspace.path("tmp")] {
            fs::create_dir_all(directory).unwrap();
        }
        static TOOL_PATHS: OnceLock<Vec<(&str, PathBuf)>> = OnceLock::new();
        for (tool, source) in TOOL_PATHS.get_or_init(|| {
            [
                "awk",
                "basename",
                "cat",
                "chmod",
                "cmp",
                "cp",
                "cut",
                "dirname",
                "expr",
                "find",
                "grep",
                "gzip",
                "head",
                "id",
                "ln",
                "mkdir",
                "mktemp",
                "mv",
                "od",
                "pwd",
                "readlink",
                "rm",
                "rmdir",
                "sed",
                "sh",
                "sort",
                "tail",
                "tar",
                "tr",
                "uname",
                "wc",
                "curl",
                "sha256sum",
                "shasum",
            ]
            .into_iter()
            .filter_map(|tool| {
                let output = Command::new("sh")
                    .args(["-c", "command -v \"$1\"", "sh", tool])
                    .output()
                    .unwrap();
                if output.status.success() {
                    let source = String::from_utf8(output.stdout).unwrap();
                    Some((tool, PathBuf::from(source.trim())))
                } else {
                    None
                }
            })
            .collect()
        }) {
            symlink(source, tools.join(tool)).unwrap();
        }
        // Calls to these commands leave durable evidence even if a script ignores
        // their exit status. The PATH exposes no other language runtime/toolchain.
        for tool in [
            "bash", "cargo", "rustc", "cc", "gcc", "clang", "python", "python3", "node", "nodejs",
            "perl", "ruby", "jq",
        ] {
            let path = tools.join(tool);
            fs::write(
                &path,
                "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$FORGE_FORBIDDEN_LOG\"\nexit 97\n",
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let archive = if matches!(integrity, ReleaseIntegrity::Large) {
            static LARGE_ARCHIVE: OnceLock<Vec<u8>> = OnceLock::new();
            LARGE_ARCHIVE
                .get_or_init(|| {
                    let file = workspace.path("payload");
                    let mut state = 0x1234_5678_u32;
                    let payload: Vec<u8> = (0..8 * 1024 * 1024)
                        .map(|_| {
                            state ^= state << 13;
                            state ^= state >> 17;
                            state ^= state << 5;
                            state as u8
                        })
                        .collect();
                    fs::write(file, payload).unwrap();
                    let path = workspace.path("large.tar");
                    setup_output(
                        Command::new("tar")
                            .arg("-cf")
                            .arg(&path)
                            .arg("-C")
                            .arg(workspace.root())
                            .arg("payload"),
                    );
                    fs::read(path).unwrap()
                })
                .clone()
        } else {
            release.archive.clone()
        };
        let archive = Arc::new(archive);
        let server_archive = Arc::clone(&archive);
        let listener = reusable_listener("127.0.0.1:0".parse().unwrap());
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&requests);
        let finished = Arc::clone(&stop);
        let release_stall = Arc::new(AtomicBool::new(false));
        let stall = Arc::clone(&release_stall);
        let integrity = Arc::new(Mutex::new(integrity));
        let server_integrity = Arc::clone(&integrity);
        let actual_checksum = format!("{:x}", Sha256::digest(&*archive));
        let server = thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("release HTTP origin: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut first_line = String::new();
                {
                    let mut reader = BufReader::new(&mut stream);
                    reader.read_line(&mut first_line).unwrap();
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                            break;
                        }
                    }
                }
                let path = first_line.split_whitespace().nth(1).unwrap_or_default();
                let integrity = *server_integrity.lock().unwrap();
                let checksum = if matches!(integrity, ReleaseIntegrity::Mismatch) {
                    "0".repeat(64)
                } else {
                    actual_checksum.clone()
                };
                let checksums = if matches!(integrity, ReleaseIntegrity::MissingAssetEntry) {
                    format!("{checksum}  unrelated.tar.gz\n")
                } else {
                    TARGETS
                        .iter()
                        .map(|(_, _, target)| {
                            format!("{checksum}  contextunity-forge-mcp-{target}.tar.gz\n")
                        })
                        .collect::<String>()
                };
                observed.lock().unwrap().push(path.to_owned());
                let name = path.rsplit('/').next().unwrap_or_default();
                let asset = TARGETS.iter().any(|(_, _, target)| {
                    name == format!("contextunity-forge-mcp-{target}.tar.gz")
                });
                let body: &[u8] = if name == "SHA256SUMS" {
                    checksums.as_bytes()
                } else if asset {
                    &server_archive
                } else {
                    b"unknown release asset"
                };
                let status = if (name == "SHA256SUMS" || asset)
                    && !(asset && matches!(integrity, ReleaseIntegrity::HttpFailure))
                {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let declared = body.len()
                    + usize::from(asset && matches!(integrity, ReleaseIntegrity::Truncated));
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    declared
                );
                // The valid archive is one byte short of the advertised response.
                // This isolates transport failure from checksum and tar failures.
                let delivered = body;
                if asset && matches!(integrity, ReleaseIntegrity::Stall) {
                    stream.write_all(header.as_bytes()).unwrap();
                    stream.flush().unwrap();
                    while !stall.load(Ordering::Acquire) && !finished.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(5));
                    }
                    if stall.load(Ordering::Acquire) {
                        stream
                            .write_all(delivered)
                            .expect("complete stalled archive response");
                        stream.flush().expect("flush stalled archive response");
                    }
                    continue;
                }
                let write_result = stream
                    .write_all(header.as_bytes())
                    .and_then(|()| stream.write_all(delivered))
                    .and_then(|()| stream.flush());
                if let Err(error) = write_result {
                    if status == "404 Not Found"
                        && matches!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                        )
                    {
                        eprintln!(
                            "release HTTP origin: client closed 404 connection for {path} before full write: {error}"
                        );
                    } else {
                        panic!("release HTTP origin failed writing {status} for {path}: declared={declared} attempted={} error={error}", delivered.len());
                    }
                }
            }
        });
        Self {
            workspace,
            home,
            project,
            tools,
            origin,
            requests,
            stop,
            release_stall,
            archive,
            integrity,
            server: Some(server),
        }
    }

    fn run(&self, platform: Option<(&str, &str)>) -> Output {
        if let Some((os, arch)) = platform {
            let uname = self.tools.join("uname");
            fs::remove_file(&uname).unwrap();
            fs::write(&uname, format!("#!/bin/sh\ncase \"$1\" in -s) echo '{os}';; -m) echo '{arch}';; *) exit 1;; esac\n")).unwrap();
            fs::set_permissions(uname, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let child = self.spawn();
        self.wait(child)
    }

    fn spawn(&self) -> Child {
        Command::new("/bin/sh")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .current_dir(&self.project)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.tools)
            .env("TMPDIR", self.workspace.path("tmp"))
            .env("FORGE_RELEASE_BASE_URL", &self.origin)
            .env("FORGE_FORBIDDEN_LOG", self.workspace.path("forbidden.log"))
            .env("LC_ALL", "C")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                fs::File::create(self.workspace.path("installer.stdout")).unwrap(),
            ))
            .stderr(Stdio::from(
                fs::File::create(self.workspace.path("installer.stderr")).unwrap(),
            ))
            .spawn()
            .expect("execute real POSIX installer")
    }

    fn wait(&self, child: Child) -> Output {
        self.wait_for(child, Duration::from_secs(30))
            .unwrap_or_else(|output| {
                panic!(
                    "installer deadline exceeded; requests={:?}; output={output:?}",
                    self.requests.lock().unwrap()
                )
            })
    }

    fn wait_for(&self, child: Child, timeout: Duration) -> Result<Output, Output> {
        let mut child = ChildGuard(Some(child));
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child
                .0
                .as_mut()
                .unwrap()
                .try_wait()
                .expect("poll installer")
            {
                kill_group(child.0.as_ref().unwrap()).expect("kill installer descendants");
                child.0.take();
                return Ok(self.output(status));
            }
            if Instant::now() >= deadline {
                kill_group(child.0.as_ref().unwrap())
                    .expect("kill timed-out installer process group");
                let status = child
                    .0
                    .as_mut()
                    .unwrap()
                    .wait()
                    .expect("reap timed-out installer");
                child.0.take();
                return Err(self.output(status));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn output(&self, status: std::process::ExitStatus) -> Output {
        let read = |name| {
            let mut bytes = Vec::new();
            fs::File::open(self.workspace.path(name))
                .unwrap()
                .take(1024 * 1024)
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        Output {
            status,
            stdout: read("installer.stdout"),
            stderr: read("installer.stderr"),
        }
    }

    fn installed_binary(&self) -> PathBuf {
        self.home.join(".local/bin/contextunity-forge-mcp")
    }

    fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.project.join(".mcp.json")).unwrap()).unwrap()
    }

    fn seed_binary(&self, bytes: &[u8]) {
        let path = self.installed_binary();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn try_teardown(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            server.join().map_err(|error| {
                error
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| error.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_else(|| "non-string panic".to_owned())
            })?;
        }
        Ok(())
    }
}

impl Drop for InstallerHarness {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
        } else {
            self.try_teardown()
                .unwrap_or_else(|error| panic!("release HTTP origin thread failed: {error}"));
        }
    }
}

fn request_archive(installer: &InstallerHarness) -> TcpStream {
    let address = installer.origin.strip_prefix("http://").unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(stream, "GET /contextunity-forge-mcp-x86_64-unknown-linux-gnu.tar.gz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    stream.flush().unwrap();
    stream
}

fn kill_group(child: &Child) -> std::io::Result<()> {
    // SAFETY: spawn creates a process group whose ID is this child's PID.
    let result = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

#[test]
fn http_harness_preserves_bytes_and_reports_disconnect() {
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Large);
    assert!(installer.archive.len() >= 4 * 1024 * 1024);
    let mut response = Vec::new();
    request_archive(&installer)
        .read_to_end(&mut response)
        .unwrap();
    let header_end = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    let header = String::from_utf8_lossy(&response[..header_end]);
    assert!(header.starts_with("HTTP/1.1 200 OK"), "{header}");
    let body = &response[header_end..];
    assert!(
        header.contains(&format!("Content-Length: {}\r\n", installer.archive.len())),
        "{header}"
    );
    assert_eq!(
        body.len(),
        installer.archive.len(),
        "archive delivery length"
    );
    assert_eq!(
        Sha256::digest(body),
        Sha256::digest(&*installer.archive),
        "archive delivery digest"
    );
    assert_eq!(body, installer.archive.as_slice());
    let teardown = installer.try_teardown();
    assert!(
        teardown.is_ok(),
        "release HTTP origin thread failed: {teardown:?}"
    );
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Large);
    let mut stream = request_archive(&installer);
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
        assert!(header.len() <= 4096, "oversized HTTP header");
    }
    assert!(header.starts_with(b"HTTP/1.1 200 OK"));
    let mut first_body_byte = [0];
    stream.read_exact(&mut first_body_byte).unwrap();
    // SAFETY: linger is initialized and the descriptor is owned by stream.
    unsafe {
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        assert_eq!(
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                (&linger as *const libc::linger).cast(),
                std::mem::size_of_val(&linger) as libc::socklen_t
            ),
            0
        );
    }
    drop(stream);
    let failure = installer
        .try_teardown()
        .expect_err("200 transfer disconnect must fail server teardown");
    assert!(
        failure.contains("200 OK")
            && failure.contains("/contextunity-forge-mcp-")
            && failure.contains("declared=")
            && failure.contains("attempted="),
        "missing transfer context: {failure}"
    );
}

#[test]
fn http_harness_releases_unused_listener_port() {
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    let address: std::net::SocketAddrV4 = installer
        .origin
        .strip_prefix("http://")
        .unwrap()
        .parse()
        .unwrap();
    assert!(installer.requests.lock().unwrap().is_empty());
    installer.try_teardown().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpListener::bind(address) {
            Ok(listener) => {
                drop(listener);
                break;
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("listener {address} did not become rebindable: {error}"),
        }
    }
}

struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = kill_group(&child);
            let _ = child.wait();
        }
    }
}

pub(super) fn wait_for_fixture_pid(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(pid) = fs::read_to_string(path)
            .ok()
            .and_then(|contents| contents.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 0)
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "fixture PID was not ready: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn fixture_stopped(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: signal zero only probes this test's fixture PID.
        let gone = unsafe { libc::kill(pid, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if gone || Instant::now() >= deadline {
            return gone;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn installer_early_exit_kills_process_group_descendant() {
    let workspace = Workspace::new();
    let pid_file = workspace.path("installer-descendant.pid");
    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 30 & echo $! > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"; exit 0")
        .arg("fixture")
        .arg(&pid_file)
        .process_group(0)
        .spawn()
        .unwrap();
    let parent = child.id() as i32;
    let mut child = ChildGuard(Some(child));
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
            kill_group(child.0.as_ref().unwrap()).unwrap();
            child.0.take();
            break status;
        }
        assert!(Instant::now() < deadline, "fixture did not exit");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    let descendant = wait_for_fixture_pid(&pid_file);
    assert!(
        fixture_stopped(parent),
        "fixture process {parent} survived teardown"
    );
    assert!(
        fixture_stopped(descendant),
        "fixture process {descendant} survived teardown"
    );
}

#[test]
fn installer_deadline_kills_descendants_holding_output() {
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    let curl = installer.tools.join("curl");
    fs::remove_file(&curl).unwrap();
    fs::write(
        &curl,
        "#!/bin/sh\n(/bin/sleep 1; printf 'survived\\n' >> \"$FORGE_FORBIDDEN_LOG\") &\nprintf '%s\\n' \"$!\" > \"$FORGE_FORBIDDEN_LOG\"\nwait\n",
    )
    .unwrap();
    fs::set_permissions(curl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = ChildGuard(Some(installer.spawn()));
    let ready = installer.workspace.path("forbidden.log");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(ready.exists(), "download descendant did not start");
    let descendant = wait_for_fixture_pid(&ready);
    let start = Instant::now();
    let output = installer
        .wait_for(child.0.take().unwrap(), Duration::from_millis(200))
        .expect_err("stalled child should time out");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "deadline did not bound collection: {output:?}"
    );
    assert!(!output.status.success());
    assert!(
        fixture_stopped(descendant),
        "download descendant survived process-group cleanup"
    );
    assert!(
        !fs::read_to_string(ready).unwrap().contains("survived"),
        "download descendant survived process-group cleanup"
    );
    let teardown = installer.try_teardown();
    assert!(
        teardown.is_ok(),
        "release HTTP origin thread failed: {teardown:?}"
    );
}

#[test]
fn termination_during_download_cleans_up_and_preserves_installation() {
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Stall);
    installer.seed_binary(b"existing binary");
    let skill = installer
        .home
        .join(".agents/skills/contextunity-forge/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, b"existing skill").unwrap();
    let config = installer.project.join(".mcp.json");
    fs::write(&config, b"{\"user\":true}\n").unwrap();
    let mut child = ChildGuard(Some(installer.spawn()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while installer.requests.lock().unwrap().is_empty() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !installer.requests.lock().unwrap().is_empty(),
        "installer did not request archive"
    );
    // SAFETY: the PID belongs to our live child, which ChildGuard reaps on failure.
    assert_eq!(
        unsafe { libc::kill(child.0.as_ref().unwrap().id() as i32, libc::SIGTERM) },
        0
    );
    installer.release_stall.store(true, Ordering::Release);
    let output = installer.wait(child.0.take().unwrap());
    let context = format!(
        "requests={:?}; status={}; stdout={}; stderr={}",
        installer.requests.lock().unwrap(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected TERM trap exit: {context}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("curl:"),
        "download failed rather than being interrupted: {context}"
    );
    assert_eq!(
        installer.requests.lock().unwrap().len(),
        1,
        "installer continued after TERM: {context}"
    );
    assert_eq!(
        fs::read(installer.installed_binary()).unwrap(),
        b"existing binary",
        "{context}"
    );
    assert_eq!(fs::read(skill).unwrap(), b"existing skill", "{context}");
    assert_eq!(fs::read(config).unwrap(), b"{\"user\":true}\n", "{context}");
    assert_eq!(
        fs::read_dir(installer.workspace.path("tmp"))
            .unwrap()
            .count(),
        0,
        "{context}"
    );
    assert_eq!(
        fs::read_dir(installer.installed_binary().parent().unwrap())
            .unwrap()
            .count(),
        1,
        "{context}"
    );
    assert_eq!(
        fs::read_dir(&installer.project).unwrap().count(),
        1,
        "{context}"
    );
    let teardown = installer.try_teardown();
    assert!(
        teardown.is_ok(),
        "release HTTP origin thread failed: {teardown:?}"
    );
}

#[test]
fn verified_release_installs_real_binary_and_companion_skills_without_runtimes() {
    let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    for agent in [".claude", ".codex", ".gemini", ".cursor"] {
        fs::create_dir(installer.home.join(agent)).unwrap();
    }
    let output = installer.run(None);
    assert!(
        output.status.success(),
        "assertion failed: verified release installation failed: {output:?}"
    );
    let binary = installer.installed_binary();
    assert!(
        fs::read(&binary).unwrap() == release_fixture().binary,
        "installed binary differs from the verified release executable"
    );
    assert_ne!(
        fs::metadata(&binary).unwrap().permissions().mode() & 0o111,
        0
    );
    let version = Command::new(&binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        version.stdout,
        Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--version")
            .output()
            .unwrap()
            .stdout
    );
    let expected =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/contextunity-forge/SKILL.md"))
            .unwrap();
    for directory in [
        ".agents/skills",
        ".claude/skills",
        ".codex/skills",
        ".gemini/config/skills",
        ".cursor/skills",
    ] {
        assert_eq!(
            fs::read(
                installer
                    .home
                    .join(directory)
                    .join("contextunity-forge/SKILL.md")
            )
            .unwrap(),
            expected,
            "real companion sync to {directory}"
        );
    }
    assert!(
        !installer.workspace.path("forbidden.log").exists(),
        "installer invoked a forbidden runtime/compiler"
    );
    assert_eq!(
        installer.config()["mcpServers"]["contextunity-forge"]["type"],
        "stdio"
    );
    assert_eq!(
        installer.config()["mcpServers"]["contextunity-forge"]["command"],
        binary.to_str().unwrap()
    );
    let messages = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        messages.contains("PATH"),
        "missing bin-directory PATH warning: {messages}"
    );
}

#[test]
fn platform_detection_selects_each_agreed_release_asset() {
    for (os, arch, target) in TARGETS {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        let output = installer.run(Some((os, arch)));
        assert!(
            output.status.success(),
            "{os}/{arch} installation: {output:?}"
        );
        let expected = format!("/contextunity-forge-mcp-{target}.tar.gz");
        assert!(
            installer
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|path| path == &expected),
            "wrong archive for {os}/{arch}"
        );
        assert_eq!(
            Sha256::digest(fs::read(installer.installed_binary()).unwrap()).to_vec(),
            release_fixture().binary_digest
        );
    }
}

#[test]
fn unsupported_platform_is_rejected_before_download_or_installation() {
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("install.sh")
            .is_file(),
        "installer entrypoint is absent"
    );
    for platform in [("FreeBSD", "x86_64"), ("Linux", "ppc64le")] {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        installer.seed_binary(b"existing binary");
        let output = installer.run(Some(platform));
        assert!(
            !output.status.success(),
            "unsupported {platform:?} accepted"
        );
        assert!(installer.requests.lock().unwrap().is_empty());
        assert_eq!(
            fs::read(installer.installed_binary()).unwrap(),
            b"existing binary"
        );
    }
}

#[test]
fn incomplete_and_failed_http_downloads_preserve_installation_and_cleanup() {
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Mismatch);
    installer.seed_binary(b"existing binary");
    let skill = installer
        .home
        .join(".agents/skills/contextunity-forge/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, b"existing skill").unwrap();
    let custom = installer.home.join(".agents/skills/custom/SKILL.md");
    fs::create_dir_all(custom.parent().unwrap()).unwrap();
    fs::write(&custom, b"user skill").unwrap();
    let config = installer.project.join(".mcp.json");
    for integrity in [
        ReleaseIntegrity::Mismatch,
        ReleaseIntegrity::MissingAssetEntry,
        ReleaseIntegrity::Truncated,
        ReleaseIntegrity::HttpFailure,
    ] {
        *installer.integrity.lock().unwrap() = integrity;
        installer.requests.lock().unwrap().clear();
        let checksum_case = matches!(
            integrity,
            ReleaseIntegrity::Mismatch | ReleaseIntegrity::MissingAssetEntry
        );
        let before: &[u8] = if checksum_case {
            b"{\"mcpServers\":{\"user\":{\"command\":\"keep\"}}}\n"
        } else {
            b"{\"user\":true}\n"
        };
        fs::write(&config, before).unwrap();
        let output = installer.run(None);
        let context = format!(
            "{integrity:?}; requests={:?}; status={}; stdout={}; stderr={}",
            installer.requests.lock().unwrap(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success(),
            "download unexpectedly succeeded: {context}"
        );
        if checksum_case {
            assert!(!output.status.success(), "corrupt release was accepted");
            assert!(
                installer
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|path| path.ends_with(".tar.gz")),
                "rejection must follow a real release download: {output:?}"
            );
            assert_eq!(fs::read(&custom).unwrap(), b"user skill");
        }
        assert_eq!(
            fs::read(installer.installed_binary()).unwrap(),
            b"existing binary",
            "{context}"
        );
        assert_eq!(fs::read(&skill).unwrap(), b"existing skill", "{context}");
        assert_eq!(fs::read(&config).unwrap(), before, "{context}");
        assert_eq!(
            fs::read_dir(installer.workspace.path("tmp"))
                .unwrap()
                .count(),
            0,
            "{context}"
        );
        assert_eq!(
            fs::read_dir(installer.installed_binary().parent().unwrap())
                .unwrap()
                .count(),
            1,
            "{context}"
        );
        assert_eq!(
            fs::read_dir(&installer.project).unwrap().count(),
            1,
            "{context}"
        );
    }
    let teardown = installer.try_teardown();
    assert!(
        teardown.is_ok(),
        "release HTTP origin thread failed: {teardown:?}"
    );
}

#[test]
fn shasum_only_path_installs_verified_release() {
    let mut installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    let sha256sum = installer.tools.join("sha256sum");
    if sha256sum.exists() {
        let target = fs::read_link(&sha256sum).unwrap();
        fs::remove_file(&sha256sum).unwrap();
        let shasum = installer.tools.join("shasum");
        if shasum.exists() {
            fs::remove_file(&shasum).unwrap();
        }
        fs::write(
            &shasum,
            format!("#!/bin/sh\nshift 2\nexec '{}' \"$@\"\n", target.display()),
        )
        .unwrap();
        fs::set_permissions(shasum, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(!sha256sum.exists());
    assert!(installer.tools.join("shasum").exists());
    let output = installer.run(None);
    assert!(
        output.status.success(),
        "shasum install: requests={:?}; output={output:?}",
        installer.requests.lock().unwrap()
    );
    assert_eq!(
        fs::read(installer.installed_binary()).unwrap(),
        release_fixture().binary
    );
    let teardown = installer.try_teardown();
    assert!(
        teardown.is_ok(),
        "release HTTP origin thread failed: {teardown:?}"
    );
}

#[test]
fn nonfile_binary_destinations_preserve_configuration_and_skills() {
    for aliased in [false, true] {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        let binary = installer.installed_binary();
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        let target = if aliased {
            let target = installer.workspace.path("outside-bin");
            fs::create_dir(&target).unwrap();
            symlink(&target, &binary).unwrap();
            target
        } else {
            fs::create_dir(&binary).unwrap();
            binary.clone()
        };
        fs::write(target.join("keep"), b"destination data").unwrap();
        let skill = installer
            .home
            .join(".agents/skills/contextunity-forge/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, b"old skill").unwrap();
        let config = installer.project.join(".mcp.json");
        let before = b"{\"user\":\"preserve\"}\n";
        fs::write(&config, before).unwrap();

        let output = installer.run(None);

        assert!(
            !output.status.success(),
            "nonfile destination accepted: {output:?}"
        );
        assert_eq!(fs::read(config).unwrap(), before);
        assert_eq!(fs::read(skill).unwrap(), b"old skill");
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"destination data");
        assert_eq!(
            fs::read_dir(&target).unwrap().count(),
            1,
            "published into destination directory"
        );
        assert_eq!(
            fs::read_dir(binary.parent().unwrap()).unwrap().count(),
            1,
            "leaked binary stage"
        );
        assert_eq!(
            fs::symlink_metadata(binary)
                .unwrap()
                .file_type()
                .is_symlink(),
            aliased
        );
    }
}

#[test]
fn aliased_agent_skill_root_preserves_the_canonical_hub_without_rsync() {
    let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    assert!(!installer.tools.join("rsync").exists());
    let hub = installer.home.join(".agents/skills");
    fs::create_dir_all(hub.join("user-skill")).unwrap();
    fs::write(hub.join("user-skill/SKILL.md"), b"user skill").unwrap();
    fs::create_dir(installer.home.join(".codex")).unwrap();
    symlink(&hub, installer.home.join(".codex/skills")).unwrap();

    let output = installer.run(None);

    assert!(
        output.status.success(),
        "aliased skill-root install: {output:?}"
    );
    assert_eq!(
        fs::read(hub.join("user-skill/SKILL.md")).unwrap(),
        b"user skill"
    );
    assert_eq!(
        fs::read(hub.join("contextunity-forge/SKILL.md")).unwrap(),
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/contextunity-forge/SKILL.md"))
            .unwrap()
    );
    assert!(installer.home.join(".codex/skills").is_symlink());
    assert!(!hub.join("contextunity-forge/contextunity-forge").exists());
}

#[test]
fn client_setup_preserves_nested_existing_json_and_user_fields() {
    for source in [
        json!({"inputs":[{"id":"token", "default":"braces { } and \\\"quoted\\\" values"}], "mcpSe\rvers":"unrelated user field", "unicode":"Привіт 🦀"}).to_string(),
        json!({"inputs":{"mcpServers":{"nested":"keep"}}, "mcpServers":{
            "other":{"command":"user-server", "env":{"TOKEN":"a}b\\c\"d"}, "args":["{x}",null,42,true]},
            "contextunity-forge":{"command":"/obsolete/forge", "type":"stdio", "cwd":"/user/project", "env":{"FORGE_CUSTOM":"keep"}, "args":["--root","/user/project"]}
        }}).to_string(),
        r#"{"mcp\u0053ervers":{"oth\u0065r":{"command":"keep","env":{"GREETING":"\u041f\u0440\u0438\u0432\u0456\u0442"}}}}"#.to_owned(),
    ] {
        let existing: Value = serde_json::from_str(&source).unwrap();
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        fs::write(
            installer.project.join(".mcp.json"),
            source,
        )
        .unwrap();
        let output = installer.run(None);
        assert!(output.status.success(), "client setup: {output:?}");
        let mut actual = installer.config();
        let forge = &mut actual["mcpServers"]["contextunity-forge"];
        assert_eq!(
            forge["command"],
            installer.installed_binary().to_str().unwrap()
        );
        assert_eq!(forge["type"], "stdio");
        if let Some(prior) = existing["mcpServers"].get("contextunity-forge") {
            forge["command"] = prior["command"].clone();
        } else {
            actual["mcpServers"]
                .as_object_mut()
                .unwrap()
                .remove("contextunity-forge");
            if existing.get("mcpServers").is_none() {
                actual.as_object_mut().unwrap().remove("mcpServers");
            }
        }
        assert_eq!(
            actual, existing,
            "installer changed user-authored JSON fields"
        );
        assert!(!installer.workspace.path("forbidden.log").exists());
    }
}

#[test]
fn invalid_client_json_is_preserved_without_overwrite() {
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("install.sh")
            .is_file(),
        "installer entrypoint is absent"
    );
    for malformed in [
        "{\"mcpServers\":",
        "[]",
        "{\"mcpServers\":[]}",
        "{\"mcpServers\":null}",
        "{} trailing",
    ] {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        fs::write(installer.project.join(".mcp.json"), malformed).unwrap();
        let output = installer.run(None);
        assert!(
            !output.status.success(),
            "invalid client JSON was accepted: {malformed}"
        );
        assert_eq!(
            fs::read_to_string(installer.project.join(".mcp.json")).unwrap(),
            malformed
        );
    }
}

#[test]
fn invalid_client_utf8_preserves_all_installation_state() {
    for invalid in [
        &[0xff][..],
        &[0xc0, 0xaf],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0x80],
        &[0xe2, 0x82],
        &[0xe2, 0x28, 0xa1],
        &[0xf5, 0x80, 0x80, 0x80],
        &[0xf0, 0x80, 0x80, 0x80],
    ] {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        installer.seed_binary(b"existing binary");
        let skill = installer
            .home
            .join(".agents/skills/contextunity-forge/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, b"old skill").unwrap();
        let mut malformed = b"{\"user\":\"".to_vec();
        malformed.extend_from_slice(invalid);
        malformed.extend_from_slice(b"\"}\n");
        let config = installer.project.join(".mcp.json");
        fs::write(&config, &malformed).unwrap();

        let output = installer.run(None);

        assert!(
            !output.status.success(),
            "invalid UTF8 accepted ({invalid:x?}): {output:?}"
        );
        assert_eq!(fs::read(config).unwrap(), malformed);
        assert_eq!(
            fs::read(installer.installed_binary()).unwrap(),
            b"existing binary"
        );
        assert_eq!(fs::read(skill).unwrap(), b"old skill");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn update_reexecutes_the_installed_server_and_preserves_project_configuration() {
    use std::{os::unix::fs::MetadataExt, time::Instant};
    let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    installer.seed_binary(&release_fixture().binary);
    for skill in ["contextunity-forge", "user-skill"] {
        fs::create_dir_all(installer.home.join(".agents/skills").join(skill)).unwrap();
        fs::write(
            installer
                .home
                .join(".agents/skills")
                .join(skill)
                .join("SKILL.md"),
            "old skill content",
        )
        .unwrap();
    }
    let binary = installer.installed_binary();
    fs::create_dir(installer.project.join("src")).unwrap();
    fs::write(
        installer.project.join("forge-mcp.yaml"),
        "roots: [src]\nignore: [.forge]\nresponse:\n  page_size: 7\n",
    )
    .unwrap();
    let config_before = fs::read(installer.project.join("forge-mcp.yaml")).unwrap();
    let mut command = Command::new(&binary);
    command
        .arg("--root")
        .arg(&installer.project)
        .arg("serve")
        .env("HOME", &installer.home);
    let mut server = StdioClient::with_command(&mut command).unwrap();
    let (_, before) = server.request("tools/list", json!({})).unwrap();
    assert!(before.get("result").is_some());
    let original_inode = fs::metadata(&binary).unwrap().ino();
    let output = installer.run(None);
    assert!(output.status.success(), "update failed: {output:?}");
    let installed_inode = fs::metadata(&binary).unwrap().ino();
    assert_ne!(
        installed_inode, original_inode,
        "binary must be atomically replaced"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let proc_executable = PathBuf::from(format!("/proc/{}/exe", server.child().id()));
    while fs::metadata(&proc_executable).unwrap().ino() != installed_inode
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::metadata(&proc_executable).unwrap().ino(),
        installed_inode,
        "installer did not reload the running installed server"
    );
    let (_, after) = server.request("tools/list", json!({})).unwrap();
    assert!(
        after.get("result").is_some(),
        "MCP session did not survive update: {after}"
    );
    assert_eq!(
        fs::read(installer.project.join("forge-mcp.yaml")).unwrap(),
        config_before
    );
    assert_eq!(
        fs::read(
            installer
                .home
                .join(".agents/skills/contextunity-forge/SKILL.md")
        )
        .unwrap(),
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/contextunity-forge/SKILL.md"))
            .unwrap()
    );
    assert_eq!(
        fs::read_to_string(installer.home.join(".agents/skills/user-skill/SKILL.md")).unwrap(),
        "old skill content"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn update_reexecutes_default_configured_server_without_signaling_other_commands() {
    use std::{
        io::Read,
        os::{
            fd::{AsRawFd, OwnedFd},
            unix::{fs::MetadataExt, net::UnixStream},
        },
        process::Child,
        time::Instant,
    };

    // A blocked/stopped CLI must be killed and reaped even when the red assertion
    // unwinds. MCP processes already have the canonical StdioClient cleanup.
    struct CommandCleanup(Child);
    impl Drop for CommandCleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    let installed = installer.run(None);
    assert!(
        installed.status.success(),
        "initial installation: {installed:?}"
    );
    let config = installer.config();
    let entry = &config["mcpServers"]["contextunity-forge"];
    let args: Vec<&str> = entry
        .get("args")
        .map(|args| {
            args.as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        args.is_empty(),
        "this seam proves the supported no-subcommand MCP invocation"
    );
    let binary = PathBuf::from(entry["command"].as_str().unwrap());
    let mut default_command = Command::new(&binary);
    default_command
        .args(&args)
        .current_dir(&installer.project)
        .env_clear()
        .env("HOME", &installer.home);
    let mut default_server = StdioClient::with_command(&mut default_command).unwrap();
    let (_, initialized) = default_server.request("tools/list", json!({})).unwrap();
    assert!(initialized.get("result").is_some());
    let server_pid = default_server.child().id();
    let old_inode = fs::metadata(&binary).unwrap().ino();

    // Keep a genuine non-server command alive in write(2), with a positional
    // argument exactly equal to "serve". A literal-token selector must not
    // mistake this read operation for an MCP server.
    fs::write(
        installer.project.join("serve"),
        "bounded reader payload ".repeat(1800),
    )
    .unwrap();
    let (mut command_output, child_output) = UnixStream::pair().unwrap();
    command_output
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let capacity: libc::c_int = 4096;
    // SAFETY: child_output is a live socket; capacity is a valid integer socket option.
    let configured = unsafe {
        libc::setsockopt(
            child_output.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&capacity as *const libc::c_int).cast(),
            std::mem::size_of_val(&capacity) as libc::socklen_t,
        )
    };
    assert_eq!(configured, 0, "configure bounded CLI stdout socket");
    let child_output: OwnedFd = child_output.into();
    let mut nonserver = CommandCleanup(
        Command::new(&binary)
            .args(["--format", "json", "read", "serve", "--mode", "raw"])
            .current_dir(&installer.project)
            .env_clear()
            .env("HOME", &installer.home)
            .stdout(Stdio::from(child_output))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut first_byte = [0];
    command_output.read_exact(&mut first_byte).unwrap();
    assert_eq!(
        first_byte,
        [b'{'],
        "real read command started serializing its result"
    );
    assert!(
        nonserver.0.try_wait().unwrap().is_none(),
        "read command must remain blocked on stdout"
    );

    let foreign_binary = installer.workspace.path("foreign-forge");
    fs::copy(&binary, &foreign_binary).unwrap();
    let mut foreign_command = Command::new(&foreign_binary);
    foreign_command
        .arg("serve")
        .current_dir(&installer.project)
        .env_clear()
        .env("HOME", &installer.home);
    let mut foreign_server = StdioClient::with_command(&mut foreign_command).unwrap();
    let excluded_pids = [nonserver.0.id(), foreign_server.child().id()];
    // Stop only owned children after they have entered their real production
    // paths. A wrongly delivered SIGHUP then remains observable in /proc, even
    // if an MCP server could otherwise reexec the same unchanged foreign image.
    for pid in excluded_pids {
        // SAFETY: pid belongs to a live child retained by this test's cleanup owner.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGSTOP) }, 0);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            if status
                .lines()
                .any(|line| line.starts_with("State:") && line.contains("T (stopped)"))
            {
                break;
            }
            assert!(Instant::now() < deadline, "owned child {pid} did not stop");
            thread::sleep(Duration::from_millis(5));
        }
    }

    let updated = installer.run(None);
    assert!(updated.status.success(), "update failed: {updated:?}");
    let replacement_inode = fs::metadata(&binary).unwrap().ino();
    assert_ne!(
        replacement_inode, old_inode,
        "installer must replace the running executable atomically"
    );
    let running_executable = PathBuf::from(format!("/proc/{server_pid}/exe"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::metadata(&running_executable).unwrap().ino() != replacement_inode
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::metadata(&running_executable).unwrap().ino(),
        replacement_inode,
        "assertion failed: default configured MCP server did not exec the installed replacement"
    );
    let (_, listed) = default_server.request("tools/list", json!({})).unwrap();
    assert!(
        listed.get("result").is_some(),
        "default MCP stream did not survive update: {listed}"
    );
    assert_eq!(default_server.child().id(), server_pid);

    for pid in excluded_pids {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        let pending = status
            .lines()
            .filter_map(|line| {
                let (field, value) = line.split_once(':')?;
                matches!(field, "SigPnd" | "ShdPnd")
                    .then(|| u64::from_str_radix(value.trim(), 16).unwrap())
            })
            .fold(0, |mask, pending| mask | pending);
        assert_eq!(
            pending & (1 << (libc::SIGHUP - 1)),
            0,
            "assertion failed: reload signaled excluded child {pid}"
        );
        // SAFETY: the owned child is still stopped and will be reaped by its cleanup owner.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGCONT) }, 0);
    }
    let mut remaining_output = Vec::new();
    command_output.read_to_end(&mut remaining_output).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = nonserver.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "non-server command did not terminate"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.success(),
        "non-server command did not finish normally"
    );
    let (_, foreign_list) = foreign_server.request("tools/list", json!({})).unwrap();
    assert!(
        foreign_list.get("result").is_some(),
        "foreign MCP session was disturbed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn update_reexecutes_installed_image_retained_by_hardlink() {
    use std::{os::unix::fs::MetadataExt, time::Instant};

    {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        let initial = installer.run(None);
        assert!(
            initial.status.success(),
            "initial installation: {initial:?}"
        );
        let config = installer.config();
        let entry = &config["mcpServers"]["contextunity-forge"];
        let binary = PathBuf::from(entry["command"].as_str().unwrap());
        let args = entry
            .get("args")
            .map(|args| {
                args.as_array()
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert!(
            args.is_empty(),
            "use the actual generated default server invocation"
        );
        let retained = installer.workspace.path("retained-installed-image");
        fs::hard_link(&binary, &retained).unwrap();
        let original_inode = fs::metadata(&binary).unwrap().ino();
        assert_eq!(fs::metadata(&binary).unwrap().nlink(), 2);
        let mut command = Command::new(&binary);
        command
            .args(&args)
            .current_dir(&installer.project)
            .env_clear()
            .env("HOME", &installer.home);
        let mut server = StdioClient::with_command(&mut command).unwrap();
        let (_, initialized) = server.request("tools/list", json!({})).unwrap();
        assert!(initialized.get("result").is_some());
        let pid = server.child().id();
        let proc_executable = PathBuf::from(format!("/proc/{pid}/exe"));

        // Both processes will have the same /proc exe text after replacement,
        // but the literal filename identifies only the foreign image's inode.
        let literal = binary.with_file_name("contextunity-forge-mcp (deleted)");
        let mut foreign = {
            fs::copy(&binary, &literal).unwrap();
            assert_ne!(fs::metadata(&literal).unwrap().ino(), original_inode);
            let mut command = Command::new(&literal);
            command
                .arg("serve")
                .current_dir(&installer.project)
                .env_clear()
                .env("HOME", &installer.home);
            let child = StdioClient::with_command(&mut command).unwrap();
            let pid = child.child().id();
            // SAFETY: the PID belongs to this owned initialized MCP child.
            assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGSTOP) }, 0);
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
                if status
                    .lines()
                    .any(|line| line.starts_with("State:") && line.contains("T (stopped)"))
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "owned foreign process did not stop"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Some(child)
        };

        let updated = installer.run(None);

        assert!(
            updated.status.success(),
            "hardlinked installation update: {updated:?}"
        );
        let replacement_inode = fs::metadata(&binary).unwrap().ino();
        assert_ne!(replacement_inode, original_inode);
        assert_eq!(fs::metadata(&retained).unwrap().ino(), original_inode);
        assert_eq!(fs::metadata(&retained).unwrap().nlink(), 1);
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::metadata(&proc_executable).unwrap().ino() != replacement_inode
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            fs::metadata(&proc_executable).unwrap().ino(),
            replacement_inode,
            "active installed image retained by a hardlink did not reload: {updated:?}"
        );
        let (_, after) = server.request("tools/list", json!({})).unwrap();
        assert!(after.get("result").is_some());
        assert_eq!(server.child().id(), pid);

        if let Some(foreign) = &mut foreign {
            let pid = foreign.child().id();
            let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            let pending = status
                .lines()
                .filter_map(|line| {
                    let (field, value) = line.split_once(':')?;
                    matches!(field, "SigPnd" | "ShdPnd")
                        .then(|| u64::from_str_radix(value.trim(), 16).unwrap())
                })
                .fold(0, |mask, pending| mask | pending);
            assert_eq!(
                pending & (1 << (libc::SIGHUP - 1)),
                0,
                "reload signaled foreign literal-path collision"
            );
            assert_eq!(fs::read_link(format!("/proc/{pid}/exe")).unwrap(), literal);
            // SAFETY: this is the same owned stopped MCP child retained above.
            assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGCONT) }, 0);
            let (_, listed) = foreign.request("tools/list", json!({})).unwrap();
            assert!(listed.get("result").is_some());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn reload_excludes_foreign_executable_with_literal_deleted_suffix() {
    use std::{os::unix::fs::MetadataExt, time::Instant};

    #[derive(Debug)]
    enum Topology {
        Linked,
        Hardlinked,
        Unlinked,
    }

    for topology in [Topology::Linked, Topology::Hardlinked, Topology::Unlinked] {
        let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
        installer.seed_binary(&release_fixture().binary);
        let binary = installer.installed_binary();
        let foreign = binary.with_file_name("contextunity-forge-mcp (deleted)");
        fs::copy(&binary, &foreign).unwrap();
        let mut command = Command::new(&foreign);
        command
            .arg("serve")
            .current_dir(&installer.project)
            .env_clear()
            .env("HOME", &installer.home);
        let mut server = StdioClient::with_command(&mut command).unwrap();
        let (_, before) = server.request("tools/list", json!({})).unwrap();
        assert!(before.get("result").is_some());
        let pid = server.child().id();
        let executable = PathBuf::from(format!("/proc/{pid}/exe"));
        let links = match topology {
            Topology::Linked => 1,
            Topology::Hardlinked => {
                fs::hard_link(&foreign, installer.workspace.path("retained-foreign-image"))
                    .unwrap();
                2
            }
            Topology::Unlinked => {
                fs::remove_file(&foreign).unwrap();
                0
            }
        };
        assert_eq!(fs::metadata(&executable).unwrap().nlink(), links);
        let expected_path = if matches!(topology, Topology::Unlinked) {
            binary.with_file_name("contextunity-forge-mcp (deleted) (deleted)")
        } else {
            foreign.clone()
        };
        assert_eq!(fs::read_link(&executable).unwrap(), expected_path);
        // SAFETY: this PID is a live owned StdioClient child; only its own
        // pending signal mask is observed while stopped, then it is resumed.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGSTOP) }, 0);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            if status
                .lines()
                .any(|line| line.starts_with("State:") && line.contains("T (stopped)"))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned foreign server did not stop"
            );
            thread::sleep(Duration::from_millis(5));
        }

        let output = Command::new(&binary)
            .args(["--format", "json", "reload"])
            .current_dir(&installer.project)
            .env_clear()
            .env("HOME", &installer.home)
            .output()
            .unwrap();

        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        let pending = status
            .lines()
            .filter_map(|line| {
                let (field, value) = line.split_once(':')?;
                matches!(field, "SigPnd" | "ShdPnd")
                    .then(|| u64::from_str_radix(value.trim(), 16).unwrap())
            })
            .fold(0, |mask, pending| mask | pending);
        assert_eq!(
            pending & (1 << (libc::SIGHUP - 1)),
            0,
            "reload signaled a foreign {topology:?} literal-suffix executable: {output:?}"
        );
        assert!(
            !output.status.success(),
            "only foreign servers exist: {output:?}"
        );
        assert_eq!(fs::read_link(&executable).unwrap(), expected_path);
        // SAFETY: the same owned child is still stopped and retained for cleanup.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGCONT) }, 0);
        let (_, after) = server.request("tools/list", json!({})).unwrap();
        assert!(after.get("result").is_some());
        assert_eq!(server.child().id(), pid);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_update_executes_default_and_explicit_servers_with_confined_discovery() {
    use std::{
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{fs::MetadataExt, net::UnixStream},
        },
        process::Child,
        ptr,
    };

    struct CommandCleanup(Child);
    impl Drop for CommandCleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    let initial = installer.run(None);
    assert!(
        initial.status.success(),
        "initial installation: {initial:?}"
    );
    let config = installer.config();
    let entry = &config["mcpServers"]["contextunity-forge"];
    let binary = PathBuf::from(entry["command"].as_str().unwrap());
    let retained = installer.workspace.path("retained-installed-image");
    let args = entry
        .get("args")
        .map(|args| {
            args.as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        args.is_empty(),
        "exercise the actual generated default invocation"
    );
    let mut default_command = Command::new(&binary);
    default_command
        .args(&args)
        .current_dir(&installer.project)
        .env_clear()
        .env("HOME", &installer.home);
    let mut default_server = StdioClient::with_command(&mut default_command).unwrap();
    let mut explicit_command = Command::new(&binary);
    explicit_command
        .arg("--root")
        .arg(&installer.project)
        .arg("serve")
        .current_dir(&installer.project)
        .env_clear()
        .env("HOME", &installer.home);
    let mut explicit_server = StdioClient::with_command(&mut explicit_command).unwrap();
    let foreign_binary = binary.with_file_name("contextunity-forge-mcp (deleted)");
    fs::copy(&binary, &foreign_binary).unwrap();
    let mut foreign_command = Command::new(&foreign_binary);
    foreign_command
        .arg("serve")
        .current_dir(&installer.project)
        .env_clear()
        .env("HOME", &installer.home);
    let mut foreign_server = StdioClient::with_command(&mut foreign_command).unwrap();

    // Keep a real non-server invocation alive with an argument named serve.
    fs::write(
        installer.project.join("serve"),
        "bounded reader payload ".repeat(1800),
    )
    .unwrap();
    let (mut command_output, child_output) = UnixStream::pair().unwrap();
    command_output
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let capacity: libc::c_int = 4096;
    // SAFETY: child_output is a live socket and capacity is a correctly sized option.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                child_output.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&capacity as *const libc::c_int).cast(),
                std::mem::size_of_val(&capacity) as libc::socklen_t,
            )
        },
        0
    );
    let child_output: OwnedFd = child_output.into();
    let mut nonserver = CommandCleanup(
        Command::new(&binary)
            .args(["--format", "json", "read", "serve", "--mode", "raw"])
            .current_dir(&installer.project)
            .env_clear()
            .env("HOME", &installer.home)
            .stdout(Stdio::from(child_output))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut first = [0];
    command_output.read_exact(&mut first).unwrap();
    assert_eq!(first, [b'{']);
    assert!(nonserver.0.try_wait().unwrap().is_none());

    let pids = [
        default_server.child().id(),
        explicit_server.child().id(),
        foreign_server.child().id(),
    ];
    for retain_old_image in [false, true] {
        if retain_old_image {
            fs::hard_link(&binary, &retained).unwrap();
        }
        let mut observers = Vec::new();
        for pid in pids {
            // SAFETY: kqueue takes no pointers and returns a new descriptor on success.
            let fd = unsafe { libc::kqueue() };
            assert!(
                fd >= 0,
                "create native process observer: {}",
                std::io::Error::last_os_error()
            );
            // SAFETY: fd is newly owned by this test and transferred to OwnedFd once.
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            let change = libc::kevent {
                ident: pid as libc::uintptr_t,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD | libc::EV_CLEAR,
                fflags: libc::NOTE_EXEC | libc::NOTE_EXIT,
                data: 0,
                udata: ptr::null_mut(),
            };
            // SAFETY: change is a live event; no output array is requested.
            assert_eq!(
                unsafe {
                    libc::kevent(fd.as_raw_fd(), &change, 1, ptr::null_mut(), 0, ptr::null())
                },
                0
            );
            observers.push(fd);
        }
        let original_inode = fs::metadata(&binary).unwrap().ino();
        let updated = installer.run(None);
        assert!(updated.status.success(), "macOS update: {updated:?}");
        assert_ne!(fs::metadata(&binary).unwrap().ino(), original_inode);
        if retain_old_image {
            assert_eq!(fs::metadata(&retained).unwrap().ino(), original_inode);
        }
        for (index, observer) in observers.iter().take(2).enumerate() {
            let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
            let timeout = libc::timespec {
                tv_sec: 10,
                tv_nsec: 0,
            };
            // SAFETY: event is writable storage for one event and timeout is initialized.
            assert_eq!(
                unsafe {
                    libc::kevent(
                        observer.as_raw_fd(),
                        ptr::null(),
                        0,
                        event.as_mut_ptr(),
                        1,
                        &timeout,
                    )
                },
                1,
                "native server did not exec replacement"
            );
            // SAFETY: kevent reported one initialized event.
            let event = unsafe { event.assume_init() };
            let ident = event.ident;
            let flags = event.fflags;
            assert_eq!(ident, pids[index] as libc::uintptr_t);
            assert_ne!(flags & libc::NOTE_EXEC, 0);
            assert_eq!(
                flags & libc::NOTE_EXIT,
                0,
                "MCP process exited instead of reexec"
            );
        }
        for server in [
            &mut default_server,
            &mut explicit_server,
            &mut foreign_server,
        ] {
            let (_, listed) = server.request("tools/list", json!({})).unwrap();
            assert!(
                listed.get("result").is_some(),
                "MCP stream did not survive: {listed}"
            );
        }
        assert_eq!(default_server.child().id(), pids[0]);
        assert_eq!(explicit_server.child().id(), pids[1]);
        let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 250_000_000,
        };
        // SAFETY: event has space for one event and all passed pointers are live.
        assert_eq!(
            unsafe {
                libc::kevent(
                    observers[2].as_raw_fd(),
                    ptr::null(),
                    0,
                    event.as_mut_ptr(),
                    1,
                    &timeout,
                )
            },
            0,
            "reload disturbed foreign executable"
        );
    }
    let mut remaining = Vec::new();
    command_output.read_to_end(&mut remaining).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = nonserver.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "non-server command did not terminate"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "reload signaled non-server command");
}

#[test]
fn installer_flags_support_nightly_latest_and_help_options() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");

    // --help prints usage and exits 0
    let help = Command::new("/bin/sh")
        .arg(&script)
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("Usage:"));
    assert!(help_stdout.contains("--latest"));
    assert!(help_stdout.contains("--nightly"));

    // Unknown options fail fast with exit code 1
    let unknown = Command::new("/bin/sh")
        .arg(&script)
        .arg("--unknown-flag")
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(1));
    let unknown_stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(unknown_stderr.contains("Unknown option: --unknown-flag"));

    // --nightly and --latest run successfully when base URL is provided
    let installer = InstallerHarness::new(ReleaseIntegrity::Verified);
    let nightly_run = Command::new("/bin/sh")
        .arg(&script)
        .arg("--nightly")
        .current_dir(&installer.project)
        .env_clear()
        .env("HOME", &installer.home)
        .env("TMPDIR", installer.workspace.path("tmp"))
        .env("PATH", &installer.tools)
        .env("FORGE_RELEASE_BASE_URL", &installer.origin)
        .output()
        .unwrap();
    assert!(
        nightly_run.status.success(),
        "install.sh --nightly failed: {}",
        String::from_utf8_lossy(&nightly_run.stderr)
    );
}
