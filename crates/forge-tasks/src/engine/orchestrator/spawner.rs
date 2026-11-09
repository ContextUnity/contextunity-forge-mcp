use super::types::RunnerInvocationContext;
use crate::tasks::profile::RunnerConfig;
use serde_json::Value;
use std::{fmt, io, process::Child, time::Duration};

/// Identity policy selected by the lifecycle owner, independently of role names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerSessionPolicy {
    Resume,
    Fresh,
}

/// The child pipe that produced an output chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStream {
    Stdout,
    Stderr,
}

/// An owned runner whose process group must be cleaned up before release.
#[derive(Debug)]
pub struct RunnerProcess {
    child: Child,
    session_fingerprint: String,
    cleaned_up: bool,
}

impl RunnerProcess {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn session_fingerprint(&self) -> &str {
        &self.session_fingerprint
    }
}

/// A process observation; the existing gate engine decides what happens next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerOutcome {
    pub process_id: u32,
    pub session_fingerprint: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

#[derive(Debug)]
pub enum RunnerError {
    InvalidCommand(String),
    InvalidSession(String),
    InvalidContext(String),
    Io(io::Error),
}

impl fmt::Display for RunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for RunnerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Preserve the claim's prepared role prompt and attach its complete gate context.
pub fn render_role_prompt(claim: &Value) -> Result<String, RunnerError> {
    let invalid = || RunnerError::InvalidContext("incomplete gate claim authority".into());
    let nonblank = |value: &Value| value.as_str().is_some_and(|s| !s.trim().is_empty());
    let guidance = &claim["workflow_guidance"];
    let prompt = &guidance["role_runner"]["rendered_prompt"];
    if !nonblank(&claim["task_id"])
        || !nonblank(&claim["goal"])
        || !nonblank(prompt)
        || !claim["allowed_write_scope"]
            .as_array()
            .is_some_and(|scope| scope.iter().all(nonblank))
        || !guidance["expected_evidence_template"].is_object()
        || ["byte_limited", "truncated"].iter().any(|key| {
            claim
                .get(key)
                .is_some_and(|value| value.as_bool() != Some(false))
        })
        || claim
            .get("omitted_fields")
            .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
        || claim
            .get("omitted_field_count")
            .is_some_and(|value| value.as_u64() != Some(0))
    {
        return Err(invalid());
    }
    Ok(format!(
        "{}\n\nGate context:\n{claim}",
        prompt.as_str().ok_or_else(invalid)?
    ))
}

/// Select an identity before claiming a gate; pass it back with `Resume` when spawning.
pub fn prepare_runner_session(
    identity: &str,
    policy: RunnerSessionPolicy,
) -> Result<String, RunnerError> {
    #[cfg(unix)]
    {
        unix::prepare_session(identity, policy)
    }
    #[cfg(not(unix))]
    {
        let _ = (identity, policy);
        Err(unsupported())
    }
}

/// Spawn original template arguments with literal runtime values in an isolated group.
pub fn spawn_role_runner(
    config: &RunnerConfig,
    context: &RunnerInvocationContext,
    session: RunnerSessionPolicy,
) -> Result<RunnerProcess, RunnerError> {
    #[cfg(unix)]
    {
        unix::spawn(config, context, session)
    }
    #[cfg(not(unix))]
    {
        let _ = (config, context, session);
        Err(unsupported())
    }
}

/// Drain both streams until completion or timeout, then release all owned resources.
pub fn monitor_runner_process(
    process: RunnerProcess,
    timeout: Duration,
    on_output: &mut dyn FnMut(RunnerStream, &[u8]) -> io::Result<()>,
) -> Result<RunnerOutcome, RunnerError> {
    #[cfg(unix)]
    {
        unix::monitor(process, timeout, on_output)
    }
    #[cfg(not(unix))]
    {
        let _ = (process, timeout, on_output);
        Err(unsupported())
    }
}

/// Execute a command with the same process-group supervision as role runners.
/// Commands have no agent session and do not claim role-owned gates.
#[cfg(unix)]
pub(crate) fn run_command_process(
    command: &mut std::process::Command,
    timeout: Duration,
    on_output: &mut dyn FnMut(RunnerStream, &[u8]) -> io::Result<()>,
) -> Result<RunnerOutcome, RunnerError> {
    use std::{os::unix::process::CommandExt, process::Stdio};
    if let Some(settings) = command
        .get_current_dir()
        .and_then(crate::tasks::configuration::current)
    {
        settings.configure_child(command);
    }
    let child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    monitor_runner_process(
        RunnerProcess {
            child,
            session_fingerprint: String::new(),
            cleaned_up: false,
        },
        timeout,
        on_output,
    )
}

#[cfg(not(unix))]
fn unsupported() -> RunnerError {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "role runners require POSIX process groups",
    )
    .into()
}

impl Drop for RunnerProcess {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = self.cleanup();
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        fs::File,
        io::Read,
        os::{fd::AsRawFd, unix::process::CommandExt},
        process::{Command, ExitStatus, Stdio},
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        thread::{self, JoinHandle},
        time::Instant,
    };

    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    // Tokenize trusted syntax before interpolation so inserted values cannot alter argv.
    fn command_words(template: &str) -> Result<Vec<String>, RunnerError> {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quote = None;
        let mut started = false;
        let mut chars = template.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\0' {
                return Err(RunnerError::InvalidCommand(
                    "NUL in command template".into(),
                ));
            }
            match (quote, ch) {
                (Some('\''), '\'') | (Some('"'), '"') => quote = None,
                (Some('\''), _) => word.push(ch),
                (_, '\\') => {
                    let next = chars
                        .next()
                        .ok_or_else(|| RunnerError::InvalidCommand("trailing escape".into()))?;
                    if quote == Some('"') && !matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                        word.push('\\');
                    }
                    if next != '\n' {
                        word.push(next);
                        started = true;
                    }
                }
                (None, '\'' | '"') => {
                    quote = Some(ch);
                    started = true;
                }
                (None, ch) if ch.is_whitespace() => {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                _ => {
                    word.push(ch);
                    started = true;
                }
            }
        }
        if quote.is_some() {
            return Err(RunnerError::InvalidCommand("unclosed command quote".into()));
        }
        if started {
            words.push(word);
        }
        if words.first().is_none_or(String::is_empty) {
            return Err(RunnerError::InvalidCommand("missing executable".into()));
        }
        Ok(words)
    }

    pub(super) fn spawn(
        config: &RunnerConfig,
        context: &RunnerInvocationContext,
        session: RunnerSessionPolicy,
    ) -> Result<RunnerProcess, RunnerError> {
        let words = command_words(&config.runner)?;
        let mut context = context.clone();
        context.session_id = prepare_session(&context.session_id, session)?;
        let argv: Vec<String> = words
            .into_iter()
            .map(|runner| {
                context.render_runner(&RunnerConfig {
                    runner,
                    prompt_template: None,
                })
            })
            .collect();
        if argv[0].is_empty() || argv.iter().any(|word| word.contains('\0')) {
            return Err(RunnerError::InvalidCommand(
                "invalid executable or NUL in argument".into(),
            ));
        }
        let mut command = Command::new(&argv[0]);
        if let Some(settings) = crate::tasks::configuration::current(&context.worktree) {
            settings.configure_child(&mut command);
        }
        let child = command
            .args(&argv[1..])
            .current_dir(&context.worktree)
            .env("FORGE_AGENT_SESSION_ID", &context.session_id)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        Ok(RunnerProcess {
            child,
            session_fingerprint: context.session_id,
            cleaned_up: false,
        })
    }

    pub(super) fn prepare_session(
        identity: &str,
        policy: RunnerSessionPolicy,
    ) -> Result<String, RunnerError> {
        match policy {
            RunnerSessionPolicy::Resume => {
                if identity.trim().is_empty() || identity.contains('\0') {
                    return Err(RunnerError::InvalidSession(
                        "resume requires a nonblank session identity without NUL".into(),
                    ));
                }
                Ok(identity.to_owned())
            }
            RunnerSessionPolicy::Fresh => {
                let mut entropy = [0; 32];
                File::open("/dev/urandom")?.read_exact(&mut entropy)?;
                Ok(format!("forge-runner-{}", hex::encode(entropy)))
            }
        }
    }

    impl RunnerProcess {
        // Kill the group even after leader exit: descendants may still own output pipes.
        pub(super) fn cleanup(&mut self) -> io::Result<ExitStatus> {
            if self.cleaned_up {
                return self.child.wait();
            }
            let group_error = loop {
                // SAFETY: child pid is positive and identifies the dedicated process group.
                if unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) } == 0 {
                    break None;
                }
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break (error.raw_os_error() != Some(libc::ESRCH)).then_some(error);
            };
            // The directly owned child may have moved itself out of its original
            // group. Its unreaped pid is still ours; never signal its new group.
            if let Err(error) = self.child.kill() {
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error);
                }
            }
            let status = loop {
                match self.child.wait() {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    result => break result?,
                }
            };
            self.cleaned_up = true;
            group_error.map_or(Ok(status), Err)
        }

        fn has_exited(&self) -> io::Result<bool> {
            loop {
                // SAFETY: zero is valid siginfo storage. waitid writes only to this
                // initialized value and observes our owned child without reaping it.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        self.child.id(),
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result == 0 {
                    // SAFETY: waitid initialized the child-status fields above.
                    return Ok(unsafe { info.si_pid() } != 0);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }

    #[derive(Debug)]
    struct CleanupFailure {
        primary: io::Error,
        cleanup: io::Error,
    }

    impl fmt::Display for CleanupFailure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}; runner cleanup also failed: {}",
                self.primary, self.cleanup
            )
        }
    }

    impl std::error::Error for CleanupFailure {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.primary)
        }
    }

    enum PipeEvent {
        Data(RunnerStream, Vec<u8>),
        End(io::Result<()>),
    }

    // Nonblocking readers plus cancellation keep joins bounded even if a descendant
    // escapes the group or a logging callback fails while the bounded channel is full.
    struct Readers {
        cancel: Arc<AtomicBool>,
        handles: Vec<JoinHandle<()>>,
    }

    impl Readers {
        fn start<R: Read + AsRawFd + Send + 'static>(
            &mut self,
            mut pipe: R,
            stream: RunnerStream,
            sender: mpsc::SyncSender<PipeEvent>,
        ) -> io::Result<()> {
            // SAFETY: the owned pipe descriptor stays alive for both fcntl calls.
            let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
            if flags == -1
                || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                    == -1
            {
                return Err(io::Error::last_os_error());
            }
            let cancel = Arc::clone(&self.cancel);
            let handle = thread::Builder::new()
                .name(format!("role-runner-{stream:?}"))
                .spawn(move || {
                    let send = |mut event| loop {
                        if cancel.load(Ordering::Acquire) {
                            return false;
                        }
                        match sender.try_send(event) {
                            Ok(()) => return true,
                            Err(mpsc::TrySendError::Disconnected(_)) => return false,
                            Err(mpsc::TrySendError::Full(returned)) => {
                                event = returned;
                                thread::sleep(POLL_INTERVAL);
                            }
                        }
                    };
                    let mut buffer = [0; 8192];
                    while !cancel.load(Ordering::Acquire) {
                        match pipe.read(&mut buffer) {
                            Ok(0) => {
                                send(PipeEvent::End(Ok(())));
                                break;
                            }
                            Ok(size) => {
                                if !send(PipeEvent::Data(stream, buffer[..size].to_vec())) {
                                    break;
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                thread::sleep(POLL_INTERVAL)
                            }
                            Err(error) => {
                                send(PipeEvent::End(Err(error)));
                                break;
                            }
                        }
                    }
                })?;
            self.handles.push(handle);
            Ok(())
        }
    }

    impl Drop for Readers {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Release);
            for handle in self.handles.drain(..) {
                let _ = handle.join();
            }
        }
    }

    pub(super) fn monitor(
        mut process: RunnerProcess,
        timeout: Duration,
        on_output: &mut dyn FnMut(RunnerStream, &[u8]) -> io::Result<()>,
    ) -> Result<RunnerOutcome, RunnerError> {
        let started = Instant::now();
        let (sender, receiver) = mpsc::sync_channel(8);
        let mut readers = Readers {
            cancel: Arc::new(AtomicBool::new(false)),
            handles: Vec::new(),
        };
        let missing = || io::Error::other("runner output pipe missing");
        readers.start(
            process.child.stdout.take().ok_or_else(missing)?,
            RunnerStream::Stdout,
            sender.clone(),
        )?;
        readers.start(
            process.child.stderr.take().ok_or_else(missing)?,
            RunnerStream::Stderr,
            sender,
        )?;
        let mut exited = false;
        let mut ended = 0;
        let mut timed_out = false;
        let result: io::Result<()> = (|| {
            loop {
                if !exited {
                    exited = process.has_exited()?;
                }
                if exited && ended == 2 {
                    break;
                }
                let Some(remaining) = timeout
                    .checked_sub(started.elapsed())
                    .filter(|d| !d.is_zero())
                else {
                    timed_out = true;
                    break;
                };
                match receiver.recv_timeout(remaining.min(POLL_INTERVAL)) {
                    Ok(PipeEvent::Data(stream, bytes)) => on_output(stream, &bytes)?,
                    Ok(PipeEvent::End(result)) => {
                        result?;
                        ended += 1;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) if ended == 2 => {
                        thread::sleep(remaining.min(POLL_INTERVAL))
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(io::Error::other(
                            "runner output reader stopped unexpectedly",
                        ))
                    }
                }
            }
            Ok(())
        })();
        let cleanup = process.cleanup();
        // Readers stop after timeout/error rather than waiting forever for inherited pipes.
        drop(readers);
        let status = match (result, cleanup) {
            (Ok(()), Ok(status)) => status,
            (Err(primary), Err(cleanup)) => {
                return Err(io::Error::other(CleanupFailure { primary, cleanup }).into())
            }
            (Err(error), _) | (_, Err(error)) => return Err(error.into()),
        };
        Ok(RunnerOutcome {
            process_id: process.id(),
            session_fingerprint: process.session_fingerprint.clone(),
            exit_code: status.code(),
            timed_out,
        })
    }
}
