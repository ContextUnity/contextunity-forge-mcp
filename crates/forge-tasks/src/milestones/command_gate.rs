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
    execute_command_gate_with_timeout(root, task_id, gate_path, commands, paths, None)
}

pub(crate) fn execute_command_gate_with_timeout(
    root: &Path,
    task_id: &str,
    gate_path: &str,
    commands: &CommandDef,
    paths: &[String],
    timeout_seconds: Option<u64>,
) -> Result<Vec<GateExecutionTrace>> {
    if task_id.trim().is_empty() || gate_path.trim().is_empty() {
        bail!("TASK_COMMAND_INVALID: task_id and gate_path must not be empty");
    }
    let mut store = store_for_task(root, task_id)?;
    let task = store.inspect(task_id)?;
    let worktree = task
        .worktree
        .as_deref()
        .context("TASK_STALE_CLAIM: claimed worktree is missing")?;
    let root_path = Path::new(worktree).canonicalize()?;
    let root = root_path.as_path();
    let stage = gate_path
        .strip_prefix("tasks.")
        .filter(|stage| !stage.trim().is_empty())
        .context("TASK_COMMAND_INVALID: gate_path must be 'tasks.<stage>'")?;
    let profile = store.profile_for_task(&task)?;
    if timeout_seconds
        .is_some_and(|value| value == 0 || value > profile.limits.max_command_timeout_seconds)
    {
        bail!("TASK_COMMAND_TIMEOUT_INVALID: timeout_seconds exceeds profile.limits.max_command_timeout_seconds ({}) or is zero", profile.limits.max_command_timeout_seconds);
    }
    let cursor = crate::engine::tasks::claim::cursor_for_stage(&profile, &task.stage)?;
    if (cursor.canonical_stage_id != stage && cursor.stage_id != stage)
        || task.status != "in_progress"
    {
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
    let commands_to_run = resolve_command_plan(root, registered_name, commands, paths, &profile)?;
    if commands_to_run.is_empty() {
        bail!("TASK_COMMAND_NO_MATCH: no registered command matched the supplied project paths");
    }
    let candidate_fingerprint = crate::engine::tasks::candidate_fingerprint(root)?;
    let run = store.start_execution_run(task_id, gate_path, root, &candidate_fingerprint)?;
    let mut traces = Vec::with_capacity(commands_to_run.len());
    for (command_id, command_str, timeout) in commands_to_run {
        let timeout = timeout_seconds.map(Duration::from_secs).unwrap_or(timeout);
        let mut violations = scope_violations(root, &task.spec)?;
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
        violations.extend(scope_violations(root, &task.spec)?);
        let output = String::from_utf8_lossy(&complete_log).into_owned();
        let parsed = parse_command_output(&output, &command_str, commands.output())?;
        let stderr_text = String::from_utf8_lossy(&stderr);
        let compilation_error = stderr_text.contains("could not compile")
            || stderr_text.contains("error[E")
            || stderr_text.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("error:") && !line.starts_with("error: test failed")
            });
        let expected = gate.expected_failure.as_ref().is_some_and(|expected| {
            status == "completed"
                && !compilation_error
                && exit_code.is_some_and(|code| {
                    code != 0
                        && (expected.exit_codes.is_empty() || expected.exit_codes.contains(&code))
                })
                && expected.stderr_contains.as_ref().is_none_or(|signature| {
                    !signature.trim().is_empty() && output.contains(signature)
                })
        });
        let succeeded = status == "completed"
            && if gate.proof.kind() == Some(crate::tasks::profile::ProofKind::Contract) {
                expected
                    || (exit_code == Some(0)
                        && profile
                            .contract_zero_exit_policies()
                            .iter()
                            .any(|policy| policy == &task.spec.proof_policy))
            } else {
                exit_code == Some(0)
            };
        let budget = profile.limits.diagnostic_inline_bytes;
        let mut diagnostic = if succeeded {
            format!(
                "PASS {command_id}: exit={} passed={} failed={} ({duration_ms} ms)",
                exit_code.unwrap_or_default(),
                parsed.passed.unwrap_or(0),
                parsed.failed.unwrap_or(0)
            )
        } else {
            parsed.diagnostic.clone()
        };
        if !violations.is_empty() {
            diagnostic = format!(
                "SCOPE_VIOLATION_WARNING: {}\n{diagnostic}",
                violations.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        }
        if status == "timeout" {
            diagnostic = format!("TIMEOUT: Hints: task_manage(action: \"verify\", task_id: \"{task_id}\", timeout_seconds: {})\n{diagnostic}", timeout.as_secs().saturating_mul(2).min(profile.limits.max_command_timeout_seconds));
        }
        let log_path = if !succeeded && diagnostic.len() > budget {
            let relative = persist_log(
                root,
                task_id,
                gate_path,
                &command_id,
                attempt_id,
                &complete_log,
            )?;
            let head_end = output_boundary(&diagnostic, budget * 3 / 10);
            let mut tail_start = diagnostic.len().saturating_sub(budget / 5);
            while !diagnostic.is_char_boundary(tail_start) {
                tail_start += 1;
            }
            diagnostic = format!(
                "{}\n[… output truncated …]\n{}",
                &diagnostic[..head_end],
                &diagnostic[tail_start..]
            );
            diagnostic.push_str(&format!(
                "\nFull log: file://{}",
                root.join(&relative).display()
            ));
            Some(relative)
        } else {
            None
        };
        let trace = GateExecutionTrace {
            milestone_id: run.milestone_id.clone(),
            subject: task_id.to_owned(),
            gate_path: gate_path.to_owned(),
            attempt_id,
            command_id,
            command_str,
            status: status.clone(),
            exit_code,
            log_path,
            output_snippet: Some(diagnostic),
            stdout_sha256: Some(format!("{:x}", Sha256::digest(&stdout))),
            duration_ms,
        };
        let seam_ref = parsed.seam_ref;
        let passed_tests = passed_test_references(
            root,
            &output,
            &trace.command_id,
            &trace.command_str,
            commands.output(),
        )
        .unwrap_or_default();
        let facts = crate::store::CommandExecutionProof {
            passed: parsed.passed,
            failed: parsed.failed,
            seam_ref,
            passed_tests,
            output_spec: commands.output().cloned(),
            scope_violations: violations.into_iter().collect(),
            expected_signature: expected.then(|| {
                gate.expected_failure
                    .as_ref()
                    .and_then(|failure| failure.stderr_contains.clone())
                    .unwrap_or_else(|| "no compiler error diagnostic".into())
            }),
        };
        store.record_execution_trace_with_proof(run.run_id, &trace, Some(&facts))?;
        traces.push(trace);
        if !succeeded {
            store.complete_execution_run(run.run_id)?;
            let trace = traces.last().context(
                "TASK_TRACE_PERSISTENCE_FAILED: command result disappeared after persistence",
            )?;
            bail!(
                "TASK_COMMAND_FAILED: gate '{gate_path}' command '{}' finished with status '{}' and exit code {:?}; {}",
                trace.command_id,
                trace.status,
                trace.exit_code,
                trace.output_snippet.as_deref().unwrap_or("no diagnostic")
            );
        }
    }
    store.complete_execution_run(run.run_id)?;
    Ok(traces)
}

pub fn resolve_command_plan(
    root: &Path,
    registered_name: &str,
    commands: &CommandDef,
    paths: &[String],
    profile: &crate::tasks::profile::GateProfile,
) -> Result<Vec<(String, String, Duration)>> {
    let paths = expanded_command_paths(root, paths)?;
    match commands {
        CommandDef::Pattern(patterns) => {
            let timeout = Duration::from_secs(patterns.timeout_seconds.unwrap_or(300).max(1));
            let mut selected = Vec::new();
            for path in &paths {
                validate_project_path(path)?;
                if let Some(command) = commands.command_for_path(path) {
                    let command = expand_command_args(
                        command,
                        profile
                            .scope_args_for_command_path(commands, path)
                            .as_deref(),
                    );
                    if selected.iter().any(|(_, existing, _)| *existing == command) {
                        continue;
                    }
                    selected.push((format!("{registered_name}::{path}"), command, timeout));
                }
            }
            Ok(selected)
        }
        CommandDef::Detailed { command, .. } | CommandDef::Script(command) => {
            let timeout = match commands {
                CommandDef::Detailed {
                    timeout_seconds, ..
                } => timeout_seconds.unwrap_or(300),
                _ => 300,
            };
            let timeout = Duration::from_secs(timeout.max(1));
            let args = paths
                .iter()
                .filter_map(|path| {
                    profile
                        .scope_args_for_command_path(commands, path)
                        .map(|args| (args, path))
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            if args.is_empty() {
                return Ok(vec![(registered_name.to_owned(), command.clone(), timeout)]);
            }
            Ok(args
                .into_iter()
                .map(|(args, path)| {
                    (
                        format!("{registered_name}::{path}"),
                        expand_command_args(command, Some(&args)),
                        timeout,
                    )
                })
                .collect())
        }
    }
}

pub(crate) fn expand_command_args(command: &str, args: Option<&str>) -> String {
    match args {
        Some(args) if command.contains("{scope_target}") => command.replace("{scope_target}", args),
        Some(args) => format!("{command} {args}"),
        None => command.to_owned(),
    }
}

fn expanded_command_paths(root: &Path, paths: &[String]) -> Result<Vec<String>> {
    let mut selected = std::collections::BTreeSet::new();
    for path in paths {
        validate_project_path(path.trim_end_matches('/'))?;
        if root.join(path).is_dir() || path.contains(['*', '?', '{']) {
            let output = crate::core::git::git_command(
                root,
                &[
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ],
            )?;
            if !output.status.success() {
                bail!("TASK_COMMAND_PATH_INVALID: could not expand task paths");
            }
            for candidate in output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
            {
                let candidate = String::from_utf8_lossy(candidate).into_owned();
                if candidate.starts_with(&format!("{}/", path.trim_end_matches('/')))
                    || crate::tasks::profile::glob_pattern_matches(path, &candidate)
                {
                    selected.insert(candidate);
                }
            }
        } else {
            selected.insert(path.clone());
        }
    }
    Ok(selected.into_iter().collect())
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParsedCommandOutput {
    pub passed: Option<u64>,
    pub failed: Option<u64>,
    pub seam_ref: Option<String>,
    pub diagnostic: String,
}

pub(crate) fn parse_command_output(
    output: &str,
    command: &str,
    spec: Option<&crate::tasks::profile::CommandOutputSpec>,
) -> Result<ParsedCommandOutput> {
    let count = |output: &str, pattern: &str| -> Result<Option<u64>> {
        let regex = regex::Regex::new(pattern)?;
        let mut total = None::<u64>;
        for captures in regex.captures_iter(output) {
            let value = captures
                .get(1)
                .context("TASK_COMMAND_OUTPUT_INVALID: count regex requires capture group 1")?
                .as_str()
                .parse::<u64>()
                .context("TASK_COMMAND_OUTPUT_INVALID: test count is not an unsigned integer")?;
            total = Some(
                total
                    .unwrap_or(0_u64)
                    .checked_add(value)
                    .context("TASK_COMMAND_OUTPUT_INVALID: test count overflow")?,
            );
        }
        Ok(total)
    };
    let counts = spec.and_then(|spec| spec.counts.as_ref());
    let passed = count(
        output,
        counts.map_or(r"(?m)\b(\d+) passed\b", |counts| counts.passed.as_str()),
    )?;
    let failed = count(
        output,
        counts.map_or(r"(?m)\b(\d+) failed\b", |counts| counts.failed.as_str()),
    )?;
    let failed = failed.or_else(|| (counts.is_some() && passed.is_some()).then_some(0));
    let noise = spec
        .map(|spec| &spec.noise)
        .into_iter()
        .flatten()
        .map(|pattern| regex::Regex::new(pattern))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let errors = spec
        .map(|spec| &spec.errors)
        .into_iter()
        .flatten()
        .map(|pattern| regex::Regex::new(pattern))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let lines = output
        .lines()
        .filter(|line| {
            errors.iter().any(|pattern| pattern.is_match(line))
                || !noise.iter().any(|pattern| pattern.is_match(line))
        })
        .collect::<Vec<_>>();
    let diagnostic = lines
        .iter()
        .filter(|line| errors.iter().any(|pattern| pattern.is_match(line)))
        .chain(
            lines
                .iter()
                .filter(|line| !errors.iter().any(|pattern| pattern.is_match(line))),
        )
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let seam_ref = spec
        .and_then(|spec| spec.seam_ref_pattern.as_ref())
        .map(|pattern| -> Result<Option<String>> {
            Ok(regex::Regex::new(pattern)?
                .captures(command)
                .and_then(|captures| captures.get(1))
                .map(|value| value.as_str().to_owned()))
        })
        .transpose()?
        .flatten()
        .or_else(|| {
            let re = regex::Regex::new(r#"(?:--seam-ref\s+|--seam-ref=)(\S+)"#).ok()?;
            re.captures(command)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_owned())
        });
    Ok(ParsedCommandOutput {
        passed,
        failed,
        seam_ref,
        diagnostic,
    })
}

fn passed_test_references(
    root: &Path,
    output: &str,
    trace_command_id: &str,
    command_str: &str,
    spec: Option<&crate::tasks::profile::CommandOutputSpec>,
) -> Result<std::collections::BTreeSet<String>> {
    let Some(spec) = spec else {
        return Ok(Default::default());
    };
    let Some(tests_spec) = &spec.tests else {
        return Ok(Default::default());
    };

    let passed_re = regex::Regex::new(&tests_spec.passed)?;
    let failed_re = tests_spec
        .failed
        .as_ref()
        .map(|pattern| regex::Regex::new(pattern))
        .transpose()?;
    let ignored_re = tests_spec
        .ignored
        .as_ref()
        .map(|pattern| regex::Regex::new(pattern))
        .transpose()?;
    let target_re = tests_spec
        .target
        .as_ref()
        .map(|pattern| regex::Regex::new(pattern))
        .transpose()?;

    let scoped_target = trace_command_id
        .split_once("::")
        .map(|(_, path)| path.trim_start_matches('/'));

    let mut current_target = scoped_target.map(|s| s.to_string());
    let mut targets_seen = std::collections::BTreeSet::new();
    let mut passed = std::collections::BTreeSet::new();
    let mut failed = std::collections::BTreeSet::new();
    let mut ignored = std::collections::BTreeSet::new();

    for line in output.lines() {
        if let Some(target_re) = &target_re {
            if let Some(captures) = target_re.captures(line) {
                if let Some(m) = captures.get(1) {
                    let target_str = m.as_str().trim();
                    let normalized = if let Ok(rel) = Path::new(target_str).strip_prefix(root) {
                        rel.to_string_lossy().into_owned()
                    } else {
                        target_str.trim_start_matches('/').to_string()
                    };
                    targets_seen.insert(normalized.clone());
                    current_target = Some(normalized);
                }
            }
        }

        if let Some(captures) = passed_re.captures(line) {
            if let Some(m) = captures.get(1) {
                let test_id = m.as_str().trim();
                let qualified = qualify_test_reference(test_id, current_target.as_deref());
                passed.insert(qualified);
            }
        }

        if let Some(failed_re) = &failed_re {
            if let Some(captures) = failed_re.captures(line) {
                if let Some(m) = captures.get(1) {
                    let test_id = m.as_str().trim();
                    let qualified = qualify_test_reference(test_id, current_target.as_deref());
                    failed.insert(qualified);
                }
            }
        }

        if let Some(ignored_re) = &ignored_re {
            if let Some(captures) = ignored_re.captures(line) {
                let test_id = captures
                    .get(1)
                    .map(|m| m.as_str().trim())
                    .unwrap_or_else(|| line.trim());
                let qualified = qualify_test_reference(test_id, current_target.as_deref());
                ignored.insert(qualified);
            }
        }
    }

    let mut canonical_targets = std::collections::BTreeSet::new();
    for target in &targets_seen {
        if targets_seen
            .iter()
            .any(|other| other != target && other.ends_with(&format!("/{target}")))
        {
            continue;
        }
        canonical_targets.insert(target.clone());
    }

    if canonical_targets.len() == 1 {
        let single_target = canonical_targets.iter().next().unwrap();
        passed = passed
            .into_iter()
            .map(|id| qualify_test_reference(&id, Some(single_target)))
            .collect();
    } else if current_target.is_none() {
        if let Some(target_re) = &target_re {
            if let Some(captures) = target_re.captures(command_str) {
                if let Some(m) = captures.get(1) {
                    let target_str = m.as_str().trim();
                    let normalized = if let Ok(rel) = Path::new(target_str).strip_prefix(root) {
                        rel.to_string_lossy().into_owned()
                    } else {
                        target_str.trim_start_matches('/').to_string()
                    };
                    let re_qualified: std::collections::BTreeSet<String> = passed
                        .into_iter()
                        .map(|id| qualify_test_reference(&id, Some(&normalized)))
                        .collect();
                    passed = re_qualified;
                }
            }
        }
    }

    for test in failed.iter().chain(&ignored) {
        passed.remove(test);
    }

    let mut validated = std::collections::BTreeSet::new();
    for reference in passed {
        if let Some((target, _)) = reference.split_once("::") {
            if target.contains('/') || Path::new(target).extension().is_some() {
                validate_project_path(target)?;
            }
        }
        validated.insert(reference);
    }

    Ok(validated)
}

fn qualify_test_reference(test_id: &str, target: Option<&str>) -> String {
    if test_id.contains('/') || test_id.contains('\\') {
        test_id.to_string()
    } else if let Some(target) = target {
        if let Some(stripped) = test_id.strip_prefix(&format!("{target}::")) {
            format!("{target}::{stripped}")
        } else {
            format!("{target}::{test_id}")
        }
    } else {
        test_id.to_string()
    }
}

fn output_boundary(value: &str, limit: usize) -> usize {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn scope_violations(
    root: &Path,
    spec: &crate::tasks::TaskSpec,
) -> Result<std::collections::BTreeSet<String>> {
    let output = crate::core::git::git_command(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !output.status.success() {
        bail!("TASK_COMMAND_SCOPE_CHECK_FAILED: git status failed");
    }
    let mut paths = std::collections::BTreeSet::new();
    let mut entries = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let mut candidates = vec![String::from_utf8_lossy(&entry[3..]).into_owned()];
        if entry[..2].iter().any(|byte| *byte == b'R' || *byte == b'C') {
            if let Some(source) = entries.next() {
                candidates.push(String::from_utf8_lossy(source).into_owned());
            }
        }
        for path in candidates {
            if !spec.scope.iter().chain(&spec.extended_scope).any(|scope| {
                path == *scope
                    || path.starts_with(&format!("{}/", scope.trim_end_matches('/')))
                    || crate::tasks::profile::glob_pattern_matches(scope, &path)
            }) {
                paths.insert(path);
            }
        }
    }
    Ok(paths)
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

#[cfg(test)]
mod command_output_tests {
    use super::parse_command_output;
    use crate::tasks::profile::{CommandCountsSpec, CommandOutputSpec};

    fn output_spec() -> CommandOutputSpec {
        CommandOutputSpec {
            counts: Some(CommandCountsSpec {
                passed: r"(?m)^passed=(.+)$".into(),
                failed: r"(?m)^failed=(.+)$".into(),
            }),
            require_counts: true,
            noise: Vec::new(),
            errors: Vec::new(),
            seam_ref_pattern: None,
            tests: None,
        }
    }

    #[test]
    fn malformed_count_is_rejected() {
        let error = parse_command_output("passed=unknown\nfailed=0", "test", Some(&output_spec()))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("test count is not an unsigned integer"));
    }

    #[test]
    fn overflowing_count_total_is_rejected() {
        let error = parse_command_output(
            "passed=18446744073709551615\npassed=1\nfailed=0",
            "test",
            Some(&output_spec()),
        )
        .unwrap_err();

        assert!(error.to_string().contains("test count overflow"));
    }

    #[test]
    fn configured_pass_count_without_failed_match_means_zero_failures() {
        let parsed = parse_command_output("passed=3", "test", Some(&output_spec())).unwrap();

        assert_eq!(parsed.passed, Some(3));
        assert_eq!(parsed.failed, Some(0));
    }
}
