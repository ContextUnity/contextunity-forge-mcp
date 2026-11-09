#![cfg(target_os = "linux")]

use crate::common::Workspace;
use forge_tasks::{
    engine::orchestrator::{
        spawner::{
            monitor_runner_process, render_role_prompt, spawn_role_runner, RunnerError,
            RunnerOutcome, RunnerProcess, RunnerSessionPolicy, RunnerStream,
        },
        types::RunnerInvocationContext,
    },
    tasks::profile::{validate_profile_document, RunnerConfig},
};
use serde_json::{json, Value};
use std::{
    fs, io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn runner_config(mode: &str) -> RunnerConfig {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/acdd/orchestrator/runner_fixture.py"
    )
    .replace('\'', "'\\''");
    let runner = format!(
        "python3 '{fixture}' {mode} {{prompt}} \"{{model}}\" '{{reasoning}}' {{session_id}} '{{worktree}}'"
    );
    let profile = validate_profile_document(
        &serde_yaml::to_string(&json!({
            "agents": {"fixture": {"models": ["test-model:high"], "runner": runner}}
        }))
        .unwrap(),
    )
    .unwrap();
    RunnerConfig::from_role(&profile.roles["fixture"]).unwrap()
}

fn invocation(worktree: &Path, prompt: &str) -> RunnerInvocationContext {
    RunnerInvocationContext {
        task_id: "fixture/milestone:runner".into(),
        stage: "build".into(),
        role: "custom-role-without-name-heuristics".into(),
        model: "model {reasoning}".into(),
        reasoning: "reasoning {model}".into(),
        session_id: "retained-builder-session".into(),
        prompt: prompt.into(),
        worktree: worktree.into(),
    }
}

fn capture(
    process: RunnerProcess,
    timeout: Duration,
) -> Result<(RunnerOutcome, Vec<u8>, Vec<u8>), RunnerError> {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let outcome = monitor_runner_process(process, timeout, &mut |stream, bytes| {
        match stream {
            RunnerStream::Stdout => stdout.extend_from_slice(bytes),
            RunnerStream::Stderr => stderr.extend_from_slice(bytes),
        }
        Ok(())
    })?;
    Ok((outcome, stdout, stderr))
}

fn wait_for_descendant(path: &Path) -> io::Result<i32> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(contents) = fs::read_to_string(path) {
            if let Ok(pid) = contents.parse() {
                return Ok(pid);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "child readiness"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_process_exit(pid: i32) -> io::Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let running = match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(record) => !record
                .rsplit_once(')')
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing proc stat state")
                })?
                .1
                .trim_start()
                .starts_with('Z'),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if !running {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn prompt_claim() -> Value {
    json!({
        "task_id": "fixture/milestone:runner",
        "stage": "build",
        "goal": "Preserve inserted {prompt} literally",
        "allowed_write_scope": ["src/runner.rs"],
        "applicable_invariants": ["No parallel lifecycle authority"],
        "workflow_guidance": {
            "role_runner": {"rendered_prompt": "Already prepared: inserted {model} remains literal."},
            "expected_evidence_template": {"proof": {"command_proof": {"exit_code": 0}}},
            "rejection": {"findings": {"summary": "Repair stream cleanup"}}
        }
    })
}

#[test]
fn runner_preserves_arguments_and_uses_an_isolated_process_group() {
    let root = Workspace::new();
    let worktree = root.path("work tree");
    fs::create_dir(&worktree).unwrap();
    let config = runner_config("inspect");
    for prompt in [
        "",
        "two words",
        "quotes '\" \\ newline\nUnicode: λ {session_id}",
        "$(touch injected); `touch injected`; & | > injected",
    ] {
        let context = invocation(&worktree, prompt);
        let launched = spawn_role_runner(&config, &context, RunnerSessionPolicy::Resume);
        assert!(
            launched.is_ok(),
            "assertion failed: configured runner must spawn: {launched:?}"
        );
        let process = launched.unwrap();
        let pid = process.id();
        let started_session = process.session_fingerprint().to_owned();
        let (outcome, stdout, stderr) = capture(process, Duration::from_secs(5)).unwrap();
        let observed: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(
            observed["argv"],
            json!([
                prompt,
                context.model,
                context.reasoning,
                context.session_id,
                context.worktree.to_string_lossy()
            ])
        );
        assert_eq!(observed["cwd"], json!(context.worktree.to_string_lossy()));
        assert_eq!(observed["pid"], pid);
        assert_eq!(observed["pgid"], pid);
        assert_ne!(observed["pgid"], unsafe { libc::getpgrp() });
        assert_eq!(outcome.process_id, pid);
        assert_eq!(outcome.session_fingerprint, started_session);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out);
        assert!(stderr.is_empty());
        assert!(!worktree.join("injected").exists());
    }
}

#[test]
fn runner_rejects_malformed_command_templates() {
    let root = Workspace::new();
    let context = invocation(root.root(), "data");
    for runner in [
        "",
        "   ",
        "''",
        "\"\"",
        "python3 'unterminated",
        "python3 \"unterminated",
        "python3 trailing\\",
        "python3 nul\0argument",
    ] {
        let config = RunnerConfig {
            runner: runner.into(),
            prompt_template: None,
        };
        let result = spawn_role_runner(&config, &context, RunnerSessionPolicy::Resume);
        assert!(
            matches!(result, Err(RunnerError::InvalidCommand(_))),
            "{result:?}"
        );
    }
}

#[test]
fn runner_tokenizes_original_quotes_escapes_and_empty_arguments() {
    let root = Workspace::new();
    let mut config = runner_config("inspect");
    config
        .runner
        .push_str(r#" '' "" joined' words' escaped\ space "back\slash" "quote\"end" '#'"#);
    let process = spawn_role_runner(
        &config,
        &invocation(root.root(), "literal {model}"),
        RunnerSessionPolicy::Resume,
    )
    .unwrap();
    let (outcome, stdout, _) = capture(process, Duration::from_secs(5)).unwrap();
    let observed: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(
        observed["argv"].as_array().unwrap()[5..],
        json!([
            "",
            "",
            "joined words",
            "escaped space",
            "back\\slash",
            "quote\"end",
            "#"
        ])
        .as_array()
        .unwrap()[..]
    );
}

#[test]
fn runner_rejects_blank_resume_session() {
    let root = Workspace::new();
    let mut context = invocation(root.root(), "data");
    for session in ["", " \t\n"] {
        context.session_id = session.into();
        let result = spawn_role_runner(
            &runner_config("inspect"),
            &context,
            RunnerSessionPolicy::Resume,
        );
        assert!(
            matches!(result, Err(RunnerError::InvalidSession(_))),
            "{result:?}"
        );
    }
}

#[test]
fn runner_reports_spawn_failure() {
    let root = Workspace::new();
    let config = RunnerConfig {
        runner: "/no/such/forge-role-runner {prompt}".into(),
        prompt_template: None,
    };
    let result = spawn_role_runner(
        &config,
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    );
    assert!(
        matches!(result, Err(RunnerError::Io(ref error)) if error.kind() == io::ErrorKind::NotFound),
        "{result:?}"
    );
}

#[test]
fn monitor_delivers_both_streams_before_child_exit() {
    let root = Workspace::new();
    let acknowledgment = root.path("streams-drained");
    let context = invocation(root.root(), acknowledgment.to_str().unwrap());
    let launched = spawn_role_runner(
        &runner_config("streams"),
        &context,
        RunnerSessionPolicy::Resume,
    );
    assert!(launched.is_ok(), "runner must launch: {launched:?}");
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let result = monitor_runner_process(
        launched.unwrap(),
        Duration::from_secs(5),
        &mut |stream, bytes| {
            match stream {
                RunnerStream::Stdout => stdout.extend_from_slice(bytes),
                RunnerStream::Stderr => stderr.extend_from_slice(bytes),
            }
            if stdout.len() == 262_144 && stderr.len() == 262_144 {
                fs::write(&acknowledgment, b"drained while child is alive")?;
            }
            Ok(())
        },
    );
    assert!(result.is_ok(), "both streams must drain: {result:?}");
    let outcome = result.unwrap();
    assert!(!outcome.timed_out);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(stdout, vec![b'o'; 262_144]);
    assert_eq!(stderr, vec![b'e'; 262_144]);
    assert!(acknowledgment.exists());
}

#[test]
fn monitor_times_out_and_reaps_the_owned_process_group() {
    let root = Workspace::new();
    let ready = root.path("descendant.pid");
    let context = invocation(root.root(), ready.to_str().unwrap());
    let launched = spawn_role_runner(
        &runner_config("hang"),
        &context,
        RunnerSessionPolicy::Resume,
    );
    assert!(launched.is_ok(), "runner must launch: {launched:?}");
    let process = launched.unwrap();
    let pid = process.id();
    let descendant = wait_for_descendant(&ready).unwrap();
    let (outcome, _, _) = capture(process, Duration::from_millis(250)).unwrap();
    assert!(outcome.timed_out);
    assert_eq!(outcome.exit_code, None);
    assert!(wait_for_process_exit(descendant).unwrap());
    let mut status = 0;
    let waited = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
    assert_eq!(waited, -1);
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn monitor_bounds_descendant_pipes_after_leader_exit() {
    let root = Workspace::new();
    let ready = root.path("orphan.pid");
    let context = invocation(root.root(), ready.to_str().unwrap());
    let launched = spawn_role_runner(
        &runner_config("orphan"),
        &context,
        RunnerSessionPolicy::Resume,
    );
    assert!(launched.is_ok(), "runner must launch: {launched:?}");
    let process = launched.unwrap();
    let descendant = wait_for_descendant(&ready).unwrap();
    let (outcome, _, _) = capture(process, Duration::from_millis(250)).unwrap();
    assert!(outcome.timed_out);
    assert!(wait_for_process_exit(descendant).unwrap());
}

#[test]
fn monitor_cleans_up_when_output_sink_fails() {
    let root = Workspace::new();
    let launched = spawn_role_runner(
        &runner_config("wait"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    );
    assert!(launched.is_ok(), "runner must launch: {launched:?}");
    let process = launched.unwrap();
    let pid = process.id();
    let result = monitor_runner_process(process, Duration::from_secs(5), &mut |_, _| {
        Err(io::Error::other("log sink unavailable"))
    });
    assert!(matches!(result, Err(RunnerError::Io(_))), "{result:?}");
    let mut status = 0;
    let waited = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
    assert_eq!(waited, -1);
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn dropping_runner_reaps_its_child() {
    let root = Workspace::new();
    let launched = spawn_role_runner(
        &runner_config("wait"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    );
    assert!(launched.is_ok(), "runner must launch: {launched:?}");
    let process = launched.unwrap();
    let pid = process.id();
    drop(process);
    let mut status = 0;
    let waited = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
    assert_eq!(waited, -1);
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn runner_resumes_builder_identity_and_isolates_fresh_sessions() {
    let root = Workspace::new();
    let context = invocation(root.root(), "data");
    let parent_identity = std::env::var_os("FORGE_AGENT_SESSION_ID");
    let mut observed_sessions = Vec::new();
    for policy in [
        RunnerSessionPolicy::Resume,
        RunnerSessionPolicy::Fresh,
        RunnerSessionPolicy::Fresh,
    ] {
        let launched = spawn_role_runner(&runner_config("inspect"), &context, policy);
        assert!(launched.is_ok(), "runner must launch: {launched:?}");
        let (outcome, stdout, _) = capture(launched.unwrap(), Duration::from_secs(5)).unwrap();
        let observed: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(observed["forge_session"], outcome.session_fingerprint);
        assert_eq!(observed["argv"][3], outcome.session_fingerprint);
        assert!(!outcome.session_fingerprint.trim().is_empty());
        if policy == RunnerSessionPolicy::Resume {
            assert_eq!(outcome.session_fingerprint, context.session_id);
        } else {
            assert_ne!(outcome.session_fingerprint, context.session_id);
            assert_ne!(
                Some(std::ffi::OsString::from(&outcome.session_fingerprint)),
                parent_identity
            );
            assert!(!observed_sessions.contains(&outcome.session_fingerprint));
        }
        observed_sessions.push(outcome.session_fingerprint);
        assert_eq!(std::env::var_os("FORGE_AGENT_SESSION_ID"), parent_identity);
    }
}

#[test]
fn monitor_preserves_nonzero_exit_status() {
    let root = Workspace::new();
    let launched = spawn_role_runner(
        &runner_config("exit"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    );
    assert!(launched.is_ok(), "runner must launch: {launched:?}");
    let (outcome, _, _) = capture(launched.unwrap(), Duration::from_secs(5)).unwrap();
    assert_eq!(outcome.exit_code, Some(7));
    assert!(!outcome.timed_out);
}

#[test]
fn monitor_preserves_raw_non_utf8_output_without_newlines() {
    let root = Workspace::new();
    let process = spawn_role_runner(
        &runner_config("binary"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    )
    .unwrap();
    let (outcome, stdout, stderr) = capture(process, Duration::from_secs(5)).unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(stdout, (0..=255).collect::<Vec<u8>>());
    assert_eq!(stderr, (0..=255).rev().collect::<Vec<u8>>());
}

#[test]
fn monitor_reports_signal_exit_without_a_timeout() {
    let root = Workspace::new();
    let process = spawn_role_runner(
        &runner_config("signal"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    )
    .unwrap();
    let (outcome, _, _) = capture(process, Duration::from_secs(5)).unwrap();
    assert_eq!(outcome.exit_code, None);
    assert!(!outcome.timed_out);
}

#[test]
fn monitor_checks_deadline_while_output_remains_ready() {
    let root = Workspace::new();
    let process = spawn_role_runner(
        &runner_config("flood"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    )
    .unwrap();
    let pid = process.id();
    let mut bytes_seen = 0;
    let outcome = monitor_runner_process(process, Duration::from_millis(250), &mut |_, bytes| {
        bytes_seen += bytes.len();
        Ok(())
    })
    .unwrap();
    assert!(outcome.timed_out);
    assert!(bytes_seen > 0);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn monitor_reaps_owned_child_after_it_leaves_the_original_group() {
    let root = Workspace::new();
    let process = spawn_role_runner(
        &runner_config("move"),
        &invocation(root.root(), "data"),
        RunnerSessionPolicy::Resume,
    )
    .unwrap();
    let pid = process.id();
    let (outcome, stdout, _) = capture(process, Duration::from_millis(250)).unwrap();
    assert!(outcome.timed_out);
    assert_eq!(stdout, b"moved\n");
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn role_prompt_preserves_prepared_text_and_complete_gate_context() {
    let claim = prompt_claim();
    let rendered = render_role_prompt(&claim);
    assert!(rendered.is_ok(), "prepared claim must render: {rendered:?}");
    let rendered = rendered.unwrap();
    let (prepared, context) = rendered.split_once("\n\nGate context:\n").unwrap();
    assert_eq!(
        prepared,
        claim["workflow_guidance"]["role_runner"]["rendered_prompt"]
    );
    assert_eq!(serde_json::from_str::<Value>(context).unwrap(), claim);
}

#[test]
fn role_prompt_rejects_incomplete_claim_authority() {
    for missing in [
        "task_id",
        "goal",
        "allowed_write_scope",
        "expected_evidence_template",
        "rendered_prompt",
    ] {
        let mut claim = prompt_claim();
        match missing {
            "expected_evidence_template" => {
                claim["workflow_guidance"]
                    .as_object_mut()
                    .unwrap()
                    .remove(missing);
            }
            "rendered_prompt" => {
                claim["workflow_guidance"]["role_runner"][missing] = Value::Null;
            }
            _ => {
                claim.as_object_mut().unwrap().remove(missing);
            }
        }
        let result = render_role_prompt(&claim);
        assert!(
            matches!(result, Err(RunnerError::InvalidContext(_))),
            "missing {missing}: {result:?}"
        );
    }
    for (pointer, value) in [
        ("/task_id", json!(" ")),
        ("/goal", json!(false)),
        ("/allowed_write_scope", json!([null])),
        ("/allowed_write_scope", json!({})),
        ("/workflow_guidance/expected_evidence_template", json!([])),
        ("/workflow_guidance/role_runner/rendered_prompt", json!("")),
        ("/byte_limited", json!(true)),
        ("/truncated", json!(true)),
        ("/omitted_fields", json!(["/context_bundle"])),
        ("/omitted_field_count", json!(1)),
    ] {
        let mut claim = prompt_claim();
        if let Some(field) = claim.pointer_mut(pointer) {
            *field = value;
        } else {
            claim[pointer.trim_start_matches('/')] = value;
        }
        assert!(
            matches!(
                render_role_prompt(&claim),
                Err(RunnerError::InvalidContext(_))
            ),
            "invalid {pointer}"
        );
    }
}
