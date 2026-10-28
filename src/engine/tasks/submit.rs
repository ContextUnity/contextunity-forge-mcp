use super::{lifecycle_error, store_for_task, submit_inner, Action, Submit, Value};
use crate::{
    core::{
        git::{git_command, git_stdout},
        tasks::{
            gates::Evidence,
            profile::{ExpectedFailure, ProofKind, RejectRouting},
        },
    },
    db::tasks_store::{ClaimBoundExecutionRun, ClaimBoundExecutionTrace, GateExecutionTrace},
};
use anyhow::{bail, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Compile a claim-bound proof envelope from persisted gate execution traces and Git state.
pub(super) fn compile_evidence_envelope(root: &Path, task_id: &str, stage: &str) -> Result<Value> {
    let store = store_for_task(root, task_id)?;
    let task = store.inspect(task_id)?;
    if task.status != "in_progress" || task.stage != stage {
        bail!("TASK_STALE_CLAIM: task '{task_id}' is not actively claimed at stage '{stage}'");
    }
    let worker_id = task
        .worker_id
        .as_deref()
        .filter(|worker| !worker.trim().is_empty())
        .context("TASK_STALE_CLAIM: active worker identity is missing")?;
    let worktree = task
        .worktree
        .as_deref()
        .filter(|worktree| !worktree.trim().is_empty())
        .context("TASK_STALE_CLAIM: active worktree is missing")?;
    let profile = store.profile_for_task(&task)?;
    let cursor = crate::engine::tasks::claim::cursor_for_stage(&profile, stage)?;
    let gate = &cursor.gate;
    let gate_path = &cursor.gate_path;
    let worktree_path = Path::new(worktree).canonicalize()?;
    let candidate_head = git_stdout(
        &worktree_path,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_CANDIDATE_BASELINE_INVALID",
    )?;
    if gate.proof.kind() == Some(ProofKind::Delivery) {
        let (candidate, candidate_baseline) = store
            .build_snapshot_candidate(task_id)?
            .context("TASK_SNAPSHOT_NOT_FOUND: no accepted candidate snapshot precedes delivery")?;
        let evidence = Evidence {
            task_id: task_id.to_owned(),
            stage: stage.to_owned(),
            claim_revision: task.claim_revision,
            contract_revision: task.contract_revision,
            worker_id: worker_id.to_owned(),
            worktree: worktree.to_owned(),
            commit: Some(candidate),
            candidate_baseline_head: Some(candidate_baseline),
            proof: json!({"delivery_proof": {"status": "passed"}}),
        };
        return serde_json::to_value(evidence)
            .context("TASK_AUTO_PROOF_UNAVAILABLE: evidence envelope could not be serialized");
    }
    let candidate_fingerprint = candidate_fingerprint(&worktree_path)?;
    let execution_gate_path = format!("tasks.{}", cursor.canonical_stage_id);
    let mut run = store.latest_claim_bound_execution_run(
        task_id,
        &execution_gate_path,
        task.claim_revision,
        task.contract_revision,
        worker_id,
        &worktree_path.to_string_lossy(),
    )?;
    if run.is_none() && cursor.canonical_stage_id != cursor.stage_id {
        run = store.latest_claim_bound_execution_run(
            task_id,
            &format!("tasks.{}", cursor.stage_id),
            task.claim_revision,
            task.contract_revision,
            worker_id,
            &worktree_path.to_string_lossy(),
        )?;
    }
    if run.is_none() && gate_path != &execution_gate_path {
        run = store.latest_claim_bound_execution_run(
            task_id,
            gate_path,
            task.claim_revision,
            task.contract_revision,
            worker_id,
            &worktree_path.to_string_lossy(),
        )?;
    }
    let run = run.context("TASK_AUTO_PROOF_UNAVAILABLE: no execution exists for the active claim and current candidate")?;
    if run.candidate_fingerprint != candidate_fingerprint {
        bail!("TASK_AUTO_PROOF_UNAVAILABLE: latest execution belongs to a different Git candidate");
    }
    if run.status != "completed" {
        bail!("TASK_AUTO_PROOF_UNAVAILABLE: latest execution for the active claim and candidate is incomplete");
    }
    let traces = store.claim_bound_execution_traces(run.run_id)?;
    if traces.is_empty() {
        bail!("TASK_AUTO_PROOF_UNAVAILABLE: latest execution contains no persisted command traces");
    }
    let proof = match gate.proof.kind() {
        Some(ProofKind::Contract) => contract_proof(&run, &traces, gate, &profile.commands)?,
        Some(ProofKind::Command) => {
            command_proof(&run, &traces, gate, &profile.commands, &worktree_path)?
        }
        Some(ProofKind::Review) => {
            bail!("TASK_TYPED_REVIEW_REQUIRED: review gates require reviewer-supplied contour evidence")
        }
        Some(ProofKind::SeamTest) => {
            bail!("TASK_TYPED_EVIDENCE_REQUIRED: seam-test gates require typed seam references and test counts")
        }
        Some(ProofKind::Delivery) => {
            bail!("TASK_TYPED_EVIDENCE_REQUIRED: delivery gates require an accepted delivery proof")
        }
        Some(ProofKind::None) => {
            bail!("TASK_TYPED_EVIDENCE_REQUIRED: this gate does not support inferred proof")
        }
        None => {
            bail!("TASK_TYPED_EVIDENCE_REQUIRED: schema-defined proof gates require their declared typed payload")
        }
    };
    let evidence = Evidence {
        task_id: task_id.to_owned(),
        stage: stage.to_owned(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: worker_id.to_owned(),
        worktree: worktree.to_owned(),
        commit: None,
        candidate_baseline_head: Some(candidate_head),
        proof,
    };
    serde_json::to_value(evidence)
        .context("TASK_AUTO_PROOF_UNAVAILABLE: evidence envelope could not be serialized")
}

/// Submit a task gate by compiling its proof from persisted execution traces.
pub(super) fn submit_zero_arg(
    root: &Path,
    task_id: &str,
    stage: &str,
    action: Action,
    findings: Option<Value>,
) -> Result<Value> {
    if !matches!(action, Action::Pass) {
        bail!("TASK_TYPED_EVIDENCE_REQUIRED: zero-argument rejection submissions are not supported")
    }
    let evidence = compile_evidence_envelope(root, task_id, stage)?;
    let submission = Submit {
        task_id: task_id.to_owned(),
        stage: stage.to_owned(),
        evidence,
        action,
        findings,
    };
    submit_inner(root, submission).map_err(|error| lifecycle_error(task_id, error))
}

/// Confirm an expected red contract trace is followed by a successful implementation trace.
pub(super) fn verify_red_to_green(
    red: &ClaimBoundExecutionTrace,
    green: &ClaimBoundExecutionTrace,
    expected_failure: &ExpectedFailure,
) -> Result<Value> {
    let red_trace = &red.trace;
    let green_trace = &green.trace;
    let red_code = red_trace
        .exit_code
        .context("TASK_SEAM_TRANSITION_INVALID: red trace has no process exit code")?;
    if red.run.status != "completed" || red_trace.status != "completed" || red_code == 0 {
        bail!("TASK_SEAM_TRANSITION_INVALID: red trace must be a completed nonzero process");
    }
    if !expected_failure.exit_codes.is_empty() && !expected_failure.exit_codes.contains(&red_code) {
        bail!("TASK_SEAM_TRANSITION_INVALID: red exit code {red_code} is not in the expected failure set");
    }
    if let Some(expected) = expected_failure.stderr_contains.as_deref() {
        if expected.trim().is_empty()
            || !red_trace
                .output_snippet
                .as_deref()
                .is_some_and(|output| output.contains(expected))
        {
            bail!("TASK_SEAM_TRANSITION_INVALID: red output does not contain the configured failure signature");
        }
    }
    let green_code = green_trace
        .exit_code
        .context("TASK_SEAM_TRANSITION_INVALID: green trace has no process exit code")?;
    if green.run.status != "completed" || green_trace.status != "completed" || green_code != 0 {
        bail!("TASK_SEAM_TRANSITION_INVALID: green trace must be a completed zero-exit process");
    }
    if red_trace.subject != green_trace.subject
        || red_trace.milestone_id != green_trace.milestone_id
        || red.run.subject != green.run.subject
        || red.run.milestone_id != green.run.milestone_id
    {
        bail!("TASK_SEAM_TRANSITION_INVALID: red and green traces must belong to the same task and milestone");
    }
    if red_trace.gate_path == green_trace.gate_path {
        bail!("TASK_SEAM_TRANSITION_INVALID: red and green traces must represent different lifecycle gates");
    }
    if red.trace_order >= green.trace_order {
        bail!("TASK_SEAM_TRANSITION_INVALID: red execution must precede green execution");
    }
    let red_seam = seam_reference_from_command(&red_trace.command_str)
        .context("TASK_SEAM_TRANSITION_INVALID: red command does not identify a seam test")?;
    let green_seam = seam_reference_from_command(&green_trace.command_str)
        .context("TASK_SEAM_TRANSITION_INVALID: green command does not identify a seam test")?;
    if red_seam != green_seam {
        bail!("TASK_SEAM_TRANSITION_INVALID: red and green executions must identify the same seam test");
    }
    Ok(json!({
        "verified": true,
        "red_exit_code": red_code,
        "green_exit_code": green_code,
        "task_id": red_trace.subject,
        "seam_test_ref": red_seam,
        "red_gate": red_trace.gate_path,
        "green_gate": green_trace.gate_path,
        "red_execution_order": red.trace_order,
        "green_execution_order": green.trace_order,
    }))
}

/// Resolve the destination gate for scope or public-symbol review findings.
pub(super) fn evaluate_reject_routing(
    routing: &RejectRouting,
    finding_kind: &str,
) -> Result<String> {
    let destination = match finding_kind.trim().to_ascii_lowercase().as_str() {
        "scope" | "path" | "out_of_scope" => routing.scope.as_deref(),
        "symbol" | "symbols" | "public_symbol" | "interface" => routing.symbols.as_deref(),
        other => bail!("TASK_REJECT_ROUTING_INVALID: unsupported finding kind '{other}'"),
    }
    .filter(|destination| !destination.trim().is_empty())
    .context(format!(
        "TASK_REJECT_ROUTING_UNCONFIGURED: no destination is configured for '{finding_kind}'"
    ))?;
    Ok(destination.to_owned())
}

/// Render a compact task telemetry note while preserving complete execution logs on disk.
pub(super) fn telemetry_passport_note(
    task_id: &str,
    traces: &[GateExecutionTrace],
    diff_summary: &str,
) -> Result<String> {
    if task_id.trim().is_empty() {
        bail!("TASK_TELEMETRY_NOTE_INVALID: task_id must not be empty");
    }
    let completed = traces
        .iter()
        .filter(|trace| trace.status == "completed")
        .count();
    let failed = traces
        .iter()
        .filter(|trace| trace.status != "completed" || trace.exit_code != Some(0))
        .count();
    let mut note = format!(
        "task={task_id}\ntraces={} completed={completed} failed={failed}\ndiff={diff_summary}\n",
        traces.len()
    );
    for trace in traces {
        let log = trace.log_path.as_deref().unwrap_or("-");
        note.push_str(&format!(
            "{} {} exit={} log={}\n",
            trace.gate_path,
            trace.command_id,
            trace
                .exit_code
                .map_or_else(|| "none".to_owned(), |code| code.to_string()),
            log
        ));
    }
    truncate_utf8(&mut note, 2048);
    Ok(note)
}

fn contract_proof(
    run: &ClaimBoundExecutionRun,
    traces: &[ClaimBoundExecutionTrace],
    gate: &crate::core::tasks::profile::GateDef,
    commands: &std::collections::BTreeMap<String, crate::core::tasks::profile::CommandDef>,
) -> Result<Value> {
    if run.status != "completed" {
        bail!("TASK_AUTO_PROOF_UNAVAILABLE: contract execution did not complete");
    }
    let trace = traces
        .iter()
        .rev()
        .find(|entry| {
            entry.run.run_id == run.run_id
                && entry.trace.status == "completed"
                && entry.trace.exit_code.is_some_and(|code| code != 0)
        })
        .context(
            "TASK_AUTO_PROOF_UNAVAILABLE: latest claim-bound run has no failing contract trace",
        )?;
    verify_registered_command(gate, commands, &trace.trace)?;
    let red_exit_code = trace
        .trace
        .exit_code
        .context("TASK_AUTO_PROOF_UNAVAILABLE: contract trace has no process exit code")?;
    if let Some(expected) = gate.expected_failure.as_ref() {
        if !expected.exit_codes.is_empty() && !expected.exit_codes.contains(&red_exit_code) {
            bail!("TASK_AUTO_PROOF_INVALID: contract exit code {red_exit_code} does not match the configured expected failure");
        }
        if let Some(signature) = expected.stderr_contains.as_deref() {
            if !trace
                .trace
                .output_snippet
                .as_deref()
                .is_some_and(|output| output.contains(signature))
            {
                bail!("TASK_AUTO_PROOF_INVALID: contract trace does not contain the configured failure signature");
            }
        }
    }
    let seam_test_ref = seam_reference_from_command(&trace.trace.command_str).context(
        "TASK_AUTO_PROOF_UNAVAILABLE: contract command does not identify an indexed seam test",
    )?;
    Ok(json!({
        "contract_proof": {
            "seam_test_ref": seam_test_ref,
            "red_exit_code": red_exit_code,
        }
    }))
}

fn seam_reference_from_command(command: &str) -> Option<String> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    let test_arg = tokens.iter().position(|token| *token == "--test")?;
    let target = *tokens.get(test_arg + 1)?;
    if target.is_empty() || target.starts_with('-') || target.contains('/') || target.contains('\\')
    {
        return None;
    }
    let filter = tokens
        .iter()
        .skip(test_arg + 2)
        .take_while(|token| **token != "--")
        .find(|token| !token.starts_with('-'))?;
    let parts = filter
        .split("::")
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut modules = parts.as_slice();
    let target_module = target.replace('-', "_");
    if modules
        .first()
        .is_some_and(|part| *part == target || *part == target_module.as_str())
    {
        modules = &modules[1..];
    }
    let symbol = *modules.last()?;
    let module_parts = &modules[..modules.len() - 1];
    let path = if module_parts.is_empty() {
        PathBuf::from("tests").join(format!("{target}.rs"))
    } else {
        PathBuf::from("tests")
            .join(target)
            .join(module_parts.join("/"))
            .with_extension("rs")
    };
    Some(format!("{}::{symbol}", path.to_string_lossy()))
}

fn command_proof(
    run: &ClaimBoundExecutionRun,
    traces: &[ClaimBoundExecutionTrace],
    gate: &crate::core::tasks::profile::GateDef,
    commands: &std::collections::BTreeMap<String, crate::core::tasks::profile::CommandDef>,
    worktree: &Path,
) -> Result<Value> {
    let registered_name = gate
        .command
        .as_deref()
        .filter(|name| commands.contains_key(*name))
        .context("TASK_AUTO_PROOF_UNAVAILABLE: command gate has no registered command identity")?;
    let registered = commands
        .get(registered_name)
        .context("TASK_AUTO_PROOF_UNAVAILABLE: registered gate command is missing")?;
    if run.status != "completed" || traces.is_empty() {
        bail!("TASK_AUTO_PROOF_UNAVAILABLE: latest claim-bound command execution is incomplete");
    }
    let mut log = String::new();
    let mut tests_passed = 0_u64;
    let mut tests_failed = 0_u64;
    let mut summaries = 0;
    for entry in traces {
        if entry.run.run_id != run.run_id
            || entry.trace.status != "completed"
            || entry.trace.exit_code != Some(0)
        {
            bail!("TASK_AUTO_PROOF_UNAVAILABLE: latest claim-bound execution contains an unsuccessful command");
        }
        verify_command_identity(registered_name, registered, &entry.trace)?;
        let command_log = read_trace_log(worktree, &entry.trace)?;
        if let Some((passed, failed)) = parse_test_counts(&command_log) {
            tests_passed = tests_passed
                .checked_add(passed)
                .context("TASK_AUTO_PROOF_INVALID: passed test count overflowed")?;
            tests_failed = tests_failed
                .checked_add(failed)
                .context("TASK_AUTO_PROOF_INVALID: failed test count overflowed")?;
            summaries += 1;
        }
        log.push_str(&format!("[{}]\n{}\n", entry.trace.command_id, command_log));
    }
    if summaries == 0 {
        bail!("TASK_AUTO_PROOF_UNAVAILABLE: command logs have no supported test summary");
    }
    if tests_failed != 0 {
        bail!("TASK_AUTO_PROOF_INVALID: command log reports {tests_failed} failing tests");
    }
    Ok(json!({
        "command_proof": {
            "command": registered_name,
            "exit_code": 0,
            "tests_passed": tests_passed,
            "tests_failed": tests_failed,
            "log": bounded_string(&log, 64 * 1024),
        }
    }))
}

fn verify_registered_command(
    gate: &crate::core::tasks::profile::GateDef,
    commands: &std::collections::BTreeMap<String, crate::core::tasks::profile::CommandDef>,
    trace: &GateExecutionTrace,
) -> Result<()> {
    if let Some(name) = gate.command.as_deref() {
        let command = commands
            .get(name)
            .context("TASK_AUTO_PROOF_INVALID: gate command is not registered")?;
        verify_command_identity(name, command, trace)?;
    }
    Ok(())
}

fn verify_command_identity(
    registered_name: &str,
    registered: &crate::core::tasks::profile::CommandDef,
    trace: &GateExecutionTrace,
) -> Result<()> {
    let matches = match registered {
        crate::core::tasks::profile::CommandDef::Script(command)
        | crate::core::tasks::profile::CommandDef::Detailed { command, .. } => {
            trace.command_id == registered_name && trace.command_str == *command
        }
        crate::core::tasks::profile::CommandDef::Pattern(_) => trace
            .command_id
            .strip_prefix(&format!("{registered_name}::"))
            .is_some_and(|path| {
                !path.is_empty()
                    && validate_trace_path(path)
                    && registered.command_for_path(path) == Some(trace.command_str.as_str())
            }),
    };
    if !matches {
        bail!("TASK_AUTO_PROOF_INVALID: trace command does not match registered command '{registered_name}'");
    }
    Ok(())
}

fn validate_trace_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn read_trace_log(worktree: &Path, trace: &GateExecutionTrace) -> Result<String> {
    let log_path = trace
        .log_path
        .as_deref()
        .context("TASK_AUTO_PROOF_UNAVAILABLE: command trace has no complete output log")?;
    let worktree = worktree.canonicalize()?;
    let requested = Path::new(log_path);
    let relative = if requested.is_absolute() {
        requested
            .strip_prefix(&worktree)
            .context("TASK_AUTO_PROOF_INVALID: command log is outside the claimed worktree")?
    } else {
        requested
    };
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("TASK_AUTO_PROOF_INVALID: command log path is not normalized");
    }
    let path = worktree.join(relative);
    let mut cursor = worktree.clone();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&cursor)
            .context("TASK_AUTO_PROOF_UNAVAILABLE: complete command log is missing")?;
        if metadata.file_type().is_symlink() {
            bail!("TASK_AUTO_PROOF_INVALID: command log path contains a symlink");
        }
    }
    if !fs::metadata(&path)?.is_file() || !path.canonicalize()?.starts_with(&worktree) {
        bail!("TASK_AUTO_PROOF_INVALID: complete command log is not a confined regular file");
    }
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Hash the committed tree and all non-ignored Git-visible worktree changes.
pub(super) fn candidate_fingerprint(root: &Path) -> Result<String> {
    let head = git_stdout(
        root,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_CANDIDATE_BASELINE_INVALID",
    )?;
    let diff = git_command(root, &["diff", "--binary", "HEAD", "--"])?;
    if !diff.status.success() {
        bail!("TASK_CANDIDATE_BASELINE_INVALID: could not read the candidate diff");
    }
    let status = git_command(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !status.status.success() {
        bail!("TASK_CANDIDATE_BASELINE_INVALID: could not read candidate status");
    }
    let untracked = git_command(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    if !untracked.status.success() {
        bail!("TASK_CANDIDATE_BASELINE_INVALID: could not enumerate candidate untracked files");
    }
    let mut digest = Sha256::new();
    digest.update(head.as_bytes());
    digest.update([0]);
    digest.update(&status.stdout);
    digest.update([0]);
    digest.update(&diff.stdout);
    for path_bytes in untracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        digest.update(path_bytes);
        digest.update([0]);
        let path = git_relative_path(path_bytes)?;
        let path = root.join(path);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            digest.update(
                fs::read_link(path)?
                    .as_os_str()
                    .to_string_lossy()
                    .as_bytes(),
            );
        } else if metadata.is_file() {
            digest.update(fs::read(path)?);
        } else {
            digest.update(b"non-file");
        }
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(unix)]
fn git_relative_path(bytes: &[u8]) -> Result<PathBuf> {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn git_relative_path(bytes: &[u8]) -> Result<PathBuf> {
    Ok(PathBuf::from(std::str::from_utf8(bytes)?))
}

fn parse_test_counts(log: &str) -> Option<(u64, u64)> {
    for line in log.lines().rev() {
        if !line.contains("test result:") {
            continue;
        }
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        let mut passed = None;
        let mut failed = None;
        for pair in tokens.windows(2) {
            let value = pair[0].trim_end_matches([';', ',']).parse::<u64>().ok();
            match pair[1].trim_end_matches([';', ',']) {
                "passed" => passed = value,
                "failed" => failed = value,
                _ => {}
            }
        }
        if let (Some(passed), Some(failed)) = (passed, failed) {
            return Some((passed, failed));
        }
    }
    None
}

fn bounded_string(value: &str, max_bytes: usize) -> String {
    let mut value = value.to_owned();
    truncate_utf8(&mut value, max_bytes);
    value
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}
