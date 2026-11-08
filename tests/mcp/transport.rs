#[cfg(unix)]
#[test]
fn stdio_transport_smoke() {
    use crate::common::mcp_client::{StdioClient, StdioError, TransportEnd};
    use serde_json::{json, Value};
    use std::{
        fs, io,
        os::unix::process::ExitStatusExt,
        path::{Path, PathBuf},
        process::Command,
        time::{Duration, Instant},
    };

    let isolated_server_binary = |workspace: &crate::common::Workspace| -> io::Result<PathBuf> {
        let binary = workspace.path(".forge/server");
        fs::create_dir_all(binary.parent().unwrap())?;
        let shipped = env!("CARGO_BIN_EXE_contextunity-forge-mcp");
        fs::hard_link(shipped, &binary).or_else(|_| fs::copy(shipped, &binary).map(|_| ()))?;
        Ok(binary)
    };
    let replace_binary = |workspace: &crate::common::Workspace, binary: &Path| -> io::Result<()> {
        let replacement = workspace.path(".forge/server-next");
        fs::copy(binary, &replacement)?;
        fs::rename(replacement, binary)
    };

    let mut command = Command::new("sh");
    command.args(["-c", "printf '\\377'; IFS= read -r _"]);
    let error = match StdioClient::with_pending_initialization(&mut command) {
        Ok(_) => panic!("invalid UTF-8 on stdout must fail initialization"),
        Err(error) => error,
    };
    assert!(
        matches!(
            &error,
            StdioError::Io(error) if error.kind() == std::io::ErrorKind::InvalidData
        ),
        "expected invalid-data reader error, got {error:?}"
    );

    let mut command = Command::new("sh");
    command.args(["-c", "IFS= read -r _"]);
    let mut client = StdioClient::with_dropped_reader_for_test(&mut command)
        .expect("real child pipe should initialize");
    let error = client
        .request("tools/list", json!({}))
        .expect_err("lost reader channel must fail the request");
    assert!(
        matches!(error, StdioError::ReaderDisconnected),
        "reader loss was misclassified: {error:?}"
    );

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;

        let workspace = crate::common::Workspace::with_files(&[]);
        let binary = isolated_server_binary(&workspace).expect("copy isolated server image");
        let mut client = StdioClient::with_binary(&workspace, &binary)
            .expect("real reload session should initialize");
        let pid = client.child().id();
        replace_binary(&workspace, &binary).expect("replace server image");
        let new_inode = fs::metadata(&binary)
            .expect("replacement image metadata")
            .ino();
        let reload = Command::new(&binary)
            .args(["--format", "json", "reload"])
            .output()
            .expect("run reload CLI");
        assert!(
            reload.status.success(),
            "{}",
            String::from_utf8_lossy(&reload.stderr)
        );
        let response: Value = serde_json::from_slice(&reload.stdout).unwrap();
        assert_eq!(response["signaled_pids"], json!([pid]));
        let proc_exe = PathBuf::from(format!("/proc/{pid}/exe"));
        let deadline = Instant::now() + Duration::from_secs(10);
        let observed = loop {
            let observed = fs::metadata(&proc_exe)
                .expect("observe server executable")
                .ino();
            if observed == new_inode || Instant::now() >= deadline {
                break observed;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(observed, new_inode, "server did not exec the replacement");
        let listed = client
            .request_value("tools/list", json!({}))
            .expect("real tools/list transport");
        assert!(listed.get("error").is_none(), "{listed}");
        let tools = listed["result"]["tools"].as_array().expect("{listed}");
        assert!(
            tools.iter().any(|tool| tool["name"] == "task_blackboard"),
            "{listed}"
        );
        assert_eq!(client.child().id(), pid);
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ServerState {
        Absent,
        Zombie,
        Running,
    }
    struct ParentCleanup {
        pid: i32,
        armed: bool,
    }
    impl Drop for ParentCleanup {
        fn drop(&mut self) {
            if self.armed {
                // SAFETY: pid is the real server launched by this test.
                unsafe { libc::kill(self.pid, libc::SIGKILL) };
            }
        }
    }

    let cases = [
        ("pre-initialize", false, false, false),
        ("initialized", true, false, false),
        ("alive-parent-obsolete-idle-timeout", true, false, true),
    ]
    .into_iter()
    .chain(cfg!(target_os = "linux").then_some((
        "parent-loss-during-reload",
        true,
        true,
        false,
    )));
    for (case, initialized, reload, inactivity) in cases {
        let workspace = crate::common::Workspace::with_files(&[]);
        let binary = if reload {
            isolated_server_binary(&workspace).expect("copy isolated server image")
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        };
        let pid_file = workspace.path("server.pid");
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "\"$1\" --root \"$2\" serve <&0 & echo $! >\"$3\"; wait",
                "parent-fixture",
            ])
            .arg(&binary)
            .arg(workspace.root())
            .arg(&pid_file);
        if inactivity {
            command.env("CONTEXTUNITY_FORGE_MCP_IDLE_TIMEOUT", "1s");
        }
        let mut client = if initialized {
            StdioClient::with_command(&mut command).expect("initialized parent session")
        } else {
            StdioClient::with_pending_initialization(&mut command)
                .expect("pre-initialize parent session")
        };
        let server_pid: i32 = fs::read_to_string(pid_file)
            .expect("read server pid")
            .trim()
            .parse()
            .expect("parse server pid");
        let mut cleanup = ParentCleanup {
            pid: server_pid,
            armed: true,
        };
        let signal = |signal| {
            // SAFETY: server_pid is the real server launched by this test.
            unsafe { libc::kill(server_pid, signal) }
        };

        if inactivity {
            std::thread::sleep(Duration::from_secs(3));
        }
        if initialized {
            let listed = client
                .request_value("tools/list", json!({}))
                .expect("real tools/list transport");
            assert!(listed["result"]["tools"].is_array(), "{case}: {listed}");
        }
        if reload {
            replace_binary(&workspace, &binary).expect("replace and freeze server");
            let signal_result = signal(libc::SIGSTOP);
            let deadline = Instant::now() + Duration::from_secs(2);
            let stopped = loop {
                let stat = fs::read_to_string(format!("/proc/{server_pid}/stat"))
                    .expect("read stopped server proc stat");
                let stopped = stat
                    .rsplit_once(") ")
                    .is_some_and(|(_, tail)| tail.starts_with('T'));
                if stopped || Instant::now() >= deadline {
                    break stopped;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            assert_eq!(signal_result, 0, "{case}");
            assert!(stopped, "{case}: server did not stop");
        }
        let launcher = client.terminate_launcher().expect("kill and reap launcher");
        assert_eq!(launcher.signal(), Some(libc::SIGKILL), "{case}: {launcher}");
        if reload {
            assert_eq!(
                [signal(libc::SIGHUP), signal(libc::SIGCONT)],
                [0, 0],
                "{case}"
            );
        }
        assert_eq!(
            client
                .wait_for_transport_close(Duration::from_secs(10))
                .expect("observe real pipe closure"),
            TransportEnd::CleanEof,
            "{case}: MCP transport remained open"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let server_state = loop {
            #[cfg(target_os = "linux")]
            let observed = match fs::read_to_string(format!("/proc/{server_pid}/stat")) {
                Ok(stat) => {
                    let (_, tail) = stat
                        .rsplit_once(") ")
                        .expect("server proc stat should include its command terminator");
                    if tail.starts_with('Z') {
                        ServerState::Zombie
                    } else {
                        ServerState::Running
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => ServerState::Absent,
                Err(error) => panic!("read server proc stat: {error}"),
            };
            #[cfg(not(target_os = "linux"))]
            let observed = if signal(0) == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    ServerState::Absent
                } else {
                    panic!("inspect server process: {error}");
                }
            } else {
                ServerState::Running
            };
            if observed != ServerState::Running || Instant::now() >= deadline {
                cleanup.armed = observed == ServerState::Running;
                break observed;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            matches!(server_state, ServerState::Absent | ServerState::Zombie),
            "{case}: server closed stdout but kept running: {server_state:?}"
        );
    }
}
