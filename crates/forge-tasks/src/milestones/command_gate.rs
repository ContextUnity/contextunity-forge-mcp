use crate::{
    core::tasks::profile::CommandDef, db::tasks_store::GateExecutionTrace,
    engine::tasks::store_for_task,
};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::fs::File;
#[cfg(not(unix))]
use std::fs::{self, OpenOptions};
use std::{
    io::{Read, Write},
    path::{Component, Path},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Execute registered gate commands in order and persist a trace for each process.
pub(super) fn execute_command_gate(
    root: &Path,
    task_id: &str,
    gate_path: &str,
    commands: &CommandDef,
    paths: &[String],
) -> Result<Vec<GateExecutionTrace>> {
    if task_id.trim().is_empty() || gate_path.trim().is_empty() {
        bail!("TASK_COMMAND_INVALID: task_id and gate_path must not be empty");
    }
    let mut store = store_for_task(root, task_id)?;
    let task = store.inspect(task_id)?;
    let stage = gate_path
        .strip_prefix("tasks.")
        .filter(|stage| !stage.trim().is_empty())
        .context("TASK_COMMAND_INVALID: gate_path must be 'tasks.<stage>'")?;
    let profile = store.profile_for_task(&task)?;
    let cursor = crate::engine::tasks::claim::cursor_for_stage(&profile, &task.stage)?;
    if (cursor.canonical_stage_id != stage && cursor.stage_id != stage) || task.status != "in_progress" {
        bail!("TASK_STALE_CLAIM: task '{task_id}' is not actively claimed at gate '{gate_path}'");
    }
    let gate = cursor.gate;
    let registered_name = gate
        .command
        .as_deref()
        .context("TASK_COMMAND_INVALID: command gate has no registered command identity")?;
    let registered = profile
        .commands
        .get(registered_name)
        .context("TASK_COMMAND_INVALID: registered command is missing from the active profile")?;
    if registered != commands {
        bail!(
            "TASK_COMMAND_INVALID: supplied command definition does not match registered command '{registered_name}'"
        );
    }
    let commands_to_run = command_plan(registered_name, commands, paths)?;
    if commands_to_run.is_empty() {
        bail!("TASK_COMMAND_NO_MATCH: no registered command matched the supplied project paths");
    }
    let candidate_fingerprint = crate::engine::tasks::candidate_fingerprint(root)?;
    let run = store.start_execution_run(task_id, gate_path, root, &candidate_fingerprint)?;
    let mut traces = Vec::with_capacity(commands_to_run.len());
    for (command_id, command_str, timeout) in commands_to_run {
        let attempt_id = store.reserve_execution_attempt(run.run_id, &command_id)?;
        let execution = run_shell(root, &command_str, timeout);
        let (status, exit_code, stdout, stderr, duration_ms) = match execution {
            Ok(result) => (
                if result.timed_out {
                    "timeout"
                } else {
                    "completed"
                }
                .to_owned(),
                result.exit_code,
                result.stdout,
                result.stderr,
                result.duration_ms,
            ),
            Err(error) => (
                "spawn_error".to_owned(),
                None,
                Vec::new(),
                error.to_string().into_bytes(),
                0,
            ),
        };
        let mut complete_log = stdout.clone();
        if !stderr.is_empty() {
            if !complete_log.is_empty() {
                complete_log.extend_from_slice(b"\n");
            }
            complete_log.extend_from_slice(b"[stderr]\n");
            complete_log.extend_from_slice(&stderr);
        }
        if status == "timeout" {
            complete_log.extend_from_slice(
                format!(
                    "\n[process timed out after {} seconds]\n",
                    timeout.as_secs()
                )
                .as_bytes(),
            );
        }
        let log_path = persist_log(
            root,
            task_id,
            gate_path,
            &command_id,
            attempt_id,
            &complete_log,
        )?;
        let output = String::from_utf8_lossy(&complete_log).into_owned();
        let trace = GateExecutionTrace {
            milestone_id: run.milestone_id.clone(),
            subject: task_id.to_owned(),
            gate_path: gate_path.to_owned(),
            attempt_id,
            command_id,
            command_str,
            status: status.clone(),
            exit_code,
            log_path: Some(log_path),
            output_snippet: (!output.is_empty()).then_some(output),
            stdout_sha256: Some(format!("{:x}", Sha256::digest(&stdout))),
            duration_ms,
        };
        store.record_claim_bound_execution_trace(run.run_id, &trace)?;
        let succeeded = trace.status == "completed" && trace.exit_code == Some(0);
        traces.push(trace);
        if !succeeded {
            store.complete_execution_run(run.run_id)?;
            let trace = traces.last().context(
                "TASK_TRACE_PERSISTENCE_FAILED: command result disappeared after persistence",
            )?;
            bail!(
                "TASK_COMMAND_FAILED: gate '{gate_path}' command '{}' finished with status '{}' and exit code {:?}; full log: {}",
                trace.command_id,
                trace.status,
                trace.exit_code,
                trace.log_path.as_deref().unwrap_or("unavailable")
            );
        }
    }
    store.complete_execution_run(run.run_id)?;
    Ok(traces)
}

fn command_plan(
    registered_name: &str,
    commands: &CommandDef,
    paths: &[String],
) -> Result<Vec<(String, String, Duration)>> {
    match commands {
        CommandDef::Pattern(patterns) => {
            let timeout = Duration::from_secs(patterns.timeout_seconds.unwrap_or(300).max(1));
            let mut selected = Vec::new();
            for path in paths {
                validate_project_path(path)?;
                if let Some(command) = commands.command_for_path(path) {
                    selected.push((
                        format!("{registered_name}::{path}"),
                        command.to_owned(),
                        timeout,
                    ));
                }
            }
            Ok(selected)
        }
        CommandDef::Detailed {
            command,
            timeout_seconds,
            ..
        } => Ok(vec![(
            registered_name.to_owned(),
            command.clone(),
            Duration::from_secs(timeout_seconds.unwrap_or(300).max(1)),
        )]),
        CommandDef::Script(command) => Ok(vec![(
            registered_name.to_owned(),
            command.clone(),
            Duration::from_secs(300),
        )]),
    }
}

fn validate_project_path(path: &str) -> Result<()> {
    let path_ref = Path::new(path);
    if path.trim().is_empty()
        || path_ref.is_absolute()
        || path_ref
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("TASK_COMMAND_PATH_INVALID: '{path}' must be a normalized project-relative path");
    }
    Ok(())
}

struct ShellResult {
    timed_out: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    duration_ms: i64,
}

fn run_shell(root: &Path, command_str: &str, timeout: Duration) -> Result<ShellResult> {
    let mut command = shell_command(command_str);
    command
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("TASK_COMMAND_SPAWN_FAILED: {command_str}"))?;
    let stdout = child
        .stdout
        .take()
        .context("TASK_COMMAND_SPAWN_FAILED: child stdout pipe is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("TASK_COMMAND_SPAWN_FAILED: child stderr pipe is unavailable")?;
    let (stdout_sender, stdout_receiver) = mpsc::channel();
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_sender.send(read_all(stdout));
    });
    thread::spawn(move || {
        let _ = stderr_sender.send(read_all(stderr));
    });
    let process_id = child.id();
    let deadline = started
        .checked_add(timeout)
        .context("TASK_COMMAND_TIMEOUT_INVALID: timeout exceeds the supported process lifetime")?;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            terminate_process_tree(&mut child);
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = match stdout_receiver.recv_timeout(Duration::from_millis(250)) {
        Ok(output) => output?,
        Err(_) => {
            terminate_process_group(process_id);
            bail!(
                "TASK_COMMAND_OUTPUT_TIMEOUT: stdout pipe did not close after process completion"
            );
        }
    };
    let stderr = match stderr_receiver.recv_timeout(Duration::from_millis(250)) {
        Ok(output) => output?,
        Err(_) => {
            terminate_process_group(process_id);
            bail!(
                "TASK_COMMAND_OUTPUT_TIMEOUT: stderr pipe did not close after process completion"
            );
        }
    };
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    Ok(ShellResult {
        timed_out,
        exit_code: status.code(),
        stdout,
        stderr,
        duration_ms,
    })
}

#[cfg(unix)]
fn shell_command(command: &str) -> Command {
    let mut process = Command::new("sh");
    process.arg("-c").arg(command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    process
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut process = Command::new("cmd");
    process.arg("/C").arg(command);
    process
}

fn terminate_process_tree(child: &mut Child) {
    terminate_process_group(child.id());
    let _ = child.kill();
}

#[cfg(unix)]
fn terminate_process_group(process_id: u32) {
    if let Ok(process_id) = i32::try_from(process_id) {
        // Every command shell owns a dedicated process group, so descendants are
        // terminated together when a timeout or inherited pipe is detected.
        unsafe {
            libc::kill(-process_id, libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
fn terminate_process_group(process_id: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn read_all(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    Ok(output)
}

pub(super) fn persist_log(
    root: &Path,
    task_id: &str,
    gate_path: &str,
    command_id: &str,
    attempt_id: i64,
    output: &[u8],
) -> Result<String> {
    let task_key = digest_key(task_id);
    let gate_key = digest_key(gate_path);
    let command_key = digest_key(command_id);
    let relative = Path::new(".forge/task-traces")
        .join(task_key)
        .join(gate_key)
        .join(format!("{attempt_id}-{command_key}.log"));
    create_confined_log(root, &relative, output)?;
    Ok(relative.to_string_lossy().into_owned())
}

#[cfg(unix)]
fn create_confined_log(root: &Path, relative: &Path, output: &[u8]) -> Result<()> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::ffi::OsStrExt,
        },
    };

    let canonical_root = root
        .canonicalize()
        .context("TASK_TRACE_LOG_FAILED: worktree root is unavailable")?;
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("TASK_TRACE_LOG_FAILED: log path is not a confined relative path");
    }
    let root_name = CString::new(canonical_root.as_os_str().as_bytes())
        .context("TASK_TRACE_LOG_FAILED: worktree root contains a NUL byte")?;
    let root_fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(std::io::Error::last_os_error())
            .context("TASK_TRACE_LOG_FAILED: could not securely open worktree root");
    }
    let mut parent = unsafe { OwnedFd::from_raw_fd(root_fd) };
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let name = CString::new(component.as_os_str().as_bytes())
            .context("TASK_TRACE_LOG_FAILED: log path component contains a NUL byte")?;
        if components.peek().is_none() {
            let descriptor = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error()).context(
                    "TASK_TRACE_LOG_FAILED: log already exists or final path is a symlink",
                );
            }
            let mut file = File::from(unsafe { OwnedFd::from_raw_fd(descriptor) });
            file.write_all(output)
                .context("TASK_TRACE_LOG_FAILED: could not persist complete command output")?;
            file.sync_all()
                .context("TASK_TRACE_LOG_FAILED: could not flush complete command output")?;
            return Ok(());
        }

        let mut descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if descriptor < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        {
            let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
            if created < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error).context(
                        "TASK_TRACE_LOG_FAILED: could not create a confined log directory",
                    );
                }
            }
            descriptor = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
        }
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error())
                .context("TASK_TRACE_LOG_FAILED: parent is not a confined non-symlink directory");
        }
        parent = unsafe { OwnedFd::from_raw_fd(descriptor) };
    }
    bail!("TASK_TRACE_LOG_FAILED: log path has no final filename")
}

#[cfg(not(unix))]
fn create_confined_log(root: &Path, relative: &Path, output: &[u8]) -> Result<()> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("TASK_TRACE_LOG_FAILED: log path is not a confined relative path");
    }
    let canonical_root = root
        .canonicalize()
        .context("TASK_TRACE_LOG_FAILED: worktree root is unavailable")?;
    let parent = relative
        .parent()
        .context("TASK_TRACE_LOG_FAILED: log path has no parent directory")?;
    let mut cursor = canonical_root.clone();
    for component in parent.components() {
        cursor.push(component.as_os_str());
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => bail!("TASK_TRACE_LOG_FAILED: parent is not a non-symlink directory"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&cursor)?;
            }
            Err(error) => return Err(error.into()),
        }
        if !cursor.canonicalize()?.starts_with(&canonical_root) {
            bail!("TASK_TRACE_LOG_FAILED: log directory escapes the task worktree");
        }
    }
    let path = canonical_root.join(relative);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .context("TASK_TRACE_LOG_FAILED: log already exists or final path is a symlink")?;
    file.write_all(output)?;
    file.sync_all()?;
    Ok(())
}

fn digest_key(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
