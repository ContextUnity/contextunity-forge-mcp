use super::{
    spawner::{
        monitor_runner_process, prepare_runner_session, render_role_prompt, spawn_role_runner,
        RunnerSessionPolicy,
    },
    types::{OrchestratorEvent, OrchestratorState, RunnerInvocationContext},
};
use crate::{
    engine::{self, context, Action, Claim, Registry},
    store::TasksStore,
    tasks::profile::{GateDriver, ProofKind, RunnerConfig},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::{io::Write, path::Path, path::PathBuf, time::Duration};

/// Execution settings for one task in an already selected worktree.
#[derive(Debug, Clone)]
pub struct TaskRunOptions {
    /// Worktree passed through the existing task claim boundary.
    pub worktree: PathBuf,
    /// Coordinator identity used for mechanical gates.
    pub worker_id: String,
    /// Genuine host session used by the coordinator, separate from role sessions.
    pub session_fingerprint: String,
    /// Maximum lifetime of a role process, including stream drainage.
    pub runner_timeout: Duration,
}

/// Advance a task through its active profile using the existing task engine.
pub fn run_task_lifecycle(
    root: &Path,
    task_id: &str,
    options: &TaskRunOptions,
    observer: &mut dyn FnMut(OrchestratorEvent),
) -> Result<Value> {
    if options.worker_id.trim().is_empty()
        || options.session_fingerprint.trim().is_empty()
        || options.session_fingerprint == crate::store::UNKNOWN_SESSION_FINGERPRINT
        || options.runner_timeout.is_zero()
    {
        bail!("TASK_RUN_OPTIONS_INVALID: worker, genuine coordinator session and positive runner timeout are required");
    }
    let worktree = options.worktree.canonicalize()?;
    let registry = Registry::load(root)?;
    let workspace = registry.owner(task_id)?;
    registry.check_worktree(workspace, &worktree)?;
    let mut observations = OrchestratorState::default();
    loop {
        let store = registry.open(workspace)?;
        let task = store.inspect(task_id)?;
        if task.status == "completed" {
            task.receipt
                .as_ref()
                .context("TASK_RECEIPT_MISSING: completed task has no durable receipt")?;
            return Ok(serde_json::to_value(task)?);
        }
        let gate = store.resolve_gate_for_task(task_id, &worktree.to_string_lossy(), None)?;
        let profile = store.profile_for_task(&task)?;
        let mechanical = gate.proof.kind() == Some(ProofKind::Delivery)
            || gate.driver() == Some(GateDriver::Command);
        let (worker, session, runner) = if mechanical {
            (
                options.worker_id.clone(),
                options.session_fingerprint.clone(),
                None,
            )
        } else {
            if gate.driver() != Some(GateDriver::Agent) {
                bail!(
                    "TASK_RUNNER_UNCONFIGURED: gate '{}' has no executable driver",
                    gate.id
                );
            }
            let role_id = gate
                .role
                .as_deref()
                .context("TASK_RUNNER_UNCONFIGURED: gate role missing")?;
            let role = profile
                .roles
                .get(role_id)
                .context("TASK_RUNNER_UNCONFIGURED: role definition missing")?;
            let config = RunnerConfig::from_role(role)
                .context("TASK_RUNNER_UNCONFIGURED: role requires a runner")?;
            let details = workspace.guidance_envelope(store.inspect_details(task_id)?)?;
            let retained = if role.effective_reuse_on_reject() {
                handle_gate_rejection(&store, task_id, &gate.id, &details)?
            } else {
                None
            };
            let (worker, session) = match retained {
                Some(identity) => identity,
                None => {
                    let session = prepare_runner_session("", RunnerSessionPolicy::Fresh)?;
                    (
                        format!("{}:{}:{session}", options.worker_id, gate.id),
                        session,
                    )
                }
            };
            (
                worker,
                session,
                Some((role_id.to_owned(), role.clone(), config)),
            )
        };

        if task.status == "in_progress" {
            // An already claimed mechanical gate is resumable only by its exact
            // owner. Role processes are never relaunched over an active claim.
            let active: Option<(String, String, Option<String>)> = store.connection.query_row(
                "SELECT worker_id, worktree, session_fingerprint FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![task_id, task.claim_revision], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional()?;
            if !mechanical
                || !active.is_some_and(|(w, tree, fingerprint)| {
                    w == worker
                        && fingerprint.as_deref() == Some(&session)
                        && Path::new(&tree).canonicalize().ok().as_deref()
                            == Some(worktree.as_path())
                })
            {
                bail!("TASK_ALREADY_CLAIMED: task '{task_id}' retains an active claim; do not relaunch or release its worker");
            }
        } else {
            // Claim runs the normal authority, condition, independence and checkpoint checks.
            engine::claim(
                root,
                Claim {
                    session_fingerprint: Some(session.clone()),
                    task_id: task_id.to_owned(),
                    stage: gate.id.clone(),
                    worker_id: worker.clone(),
                    worktree: worktree.to_string_lossy().into_owned(),
                    bundle: Some(true),
                },
            )?;
        }
        let claimed = store.inspect(task_id)?;
        observer(observations.record_claim(task_id, &claimed.stage, &worker));
        if let Some((role_id, role, config)) = runner {
            // Rebuild through the canonical internal producer, before transport
            // byte limits. Never clear omission flags from a truncated envelope.
            let details = workspace.guidance_envelope(store.inspect_details(task_id)?)?;
            let details = context::adapt_workspace_guidance(workspace, details)?;
            let complete = context::context_bundle(root, workspace, task_id, details)?;
            let model = role.models.first();
            let invocation = RunnerInvocationContext {
                task_id: task_id.to_owned(),
                stage: claimed.stage.clone(),
                role: role_id,
                model: model.map(|m| m.model.clone()).unwrap_or_default(),
                reasoning: model.and_then(|m| m.reasoning.clone()).unwrap_or_default(),
                session_id: session.clone(),
                prompt: render_role_prompt(&complete)?,
                worktree: worktree.clone(),
            };
            drop(store);
            let process = spawn_role_runner(&config, &invocation, RunnerSessionPolicy::Resume)?;
            observer(observations.record_runner_start(
                task_id,
                &claimed.stage,
                &worker,
                process.session_fingerprint(),
            ));
            let outcome =
                monitor_runner_process(process, options.runner_timeout, &mut |_, bytes| {
                    std::io::stderr().write_all(bytes)
                })?;
            observer(OrchestratorEvent::RunnerExited {
                task_id: task_id.to_owned(),
                stage: claimed.stage.clone(),
                worker_id: worker.clone(),
                exit_code: outcome.exit_code,
                timed_out: outcome.timed_out,
            });
            let store = registry.open(workspace)?;
            let accepted: Option<String> = store
                .connection
                .query_row(
                    "SELECT state FROM task_gates WHERE task_id=?1 AND gate=?2 AND revision=?3",
                    params![task_id, claimed.stage, claimed.claim_revision],
                    |row| row.get(0),
                )
                .optional()?;
            if accepted.as_deref() == Some("rejected") {
                let raw: String = store.connection.query_row(
                    "SELECT findings FROM task_findings WHERE task_id=?1 AND revision=?2",
                    params![task_id, claimed.claim_revision],
                    |row| row.get(0),
                )?;
                observer(OrchestratorEvent::GateRejected {
                    task_id: task_id.to_owned(),
                    stage: claimed.stage.clone(),
                    worker_id: worker,
                    findings: serde_json::from_str(&raw)?,
                });
            }
            if outcome.timed_out || outcome.exit_code != Some(0) {
                bail!(
                    "TASK_RUNNER_FAILED: gate '{}' exit={:?} timed_out={}",
                    claimed.stage,
                    outcome.exit_code,
                    outcome.timed_out
                );
            }
            if !matches!(accepted.as_deref(), Some("passed" | "rejected")) {
                bail!(
                    "TASK_RUNNER_NO_PROGRESS: gate '{}' has no accepted submission for claim {}",
                    claimed.stage,
                    claimed.claim_revision
                );
            }
            if store.inspect(task_id)?.status == "exhausted" {
                bail!(
                    "TASK_ATTEMPTS_EXHAUSTED: gate '{}' exhausted its configured attempts",
                    claimed.stage
                );
            }
        } else {
            drop(store);
            // The existing compiler performs observed command verification or
            // derives delivery proof from the immutable accepted candidate.
            engine::submit_zero_arg(
                root,
                task_id,
                &claimed.stage,
                Action::Pass,
                None,
                Some(&session),
            )?;
            if gate.proof.kind() == Some(ProofKind::Delivery) {
                let accepted = registry.open(workspace)?.inspect(task_id)?;
                let receipt = accepted
                    .receipt
                    .as_ref()
                    .context("TASK_RECEIPT_MISSING: delivery produced no receipt")?;
                observer(OrchestratorEvent::TaskDelivered {
                    task_id: task_id.to_owned(),
                    stage: claimed.stage,
                    worker_id: worker,
                    receipt_id: receipt.commit.clone().unwrap_or_else(|| task_id.to_owned()),
                });
            }
        }
    }
}

// The guidance producer resolves the canonical reject routing. Recover the
// accepted target claim rather than guessing identities from role spellings.
fn handle_gate_rejection(
    store: &TasksStore,
    task_id: &str,
    stage: &str,
    details: &Value,
) -> Result<Option<(String, String)>> {
    let rejection = &details["workflow_guidance"]["rejection"];
    if rejection.is_null() {
        return Ok(None);
    }
    let worker = rejection["worker_id"]
        .as_str()
        .context("TASK_RESUME_IDENTITY_MISSING: rejected target has no accepted worker")?;
    let gates = details["gates"]
        .as_array()
        .context("TASK_RESUME_IDENTITY_MISSING: gate history missing")?;
    let record = gates
        .iter()
        .rev()
        .find(|entry| entry["stage"] == stage && entry["state"] == "passed")
        .context("TASK_RESUME_IDENTITY_MISSING: rejected target has no accepted claim")?;
    let revision = record["claim_revision"]
        .as_u64()
        .context("TASK_RESUME_IDENTITY_MISSING: accepted claim revision missing")?;
    let (claim_worker, fingerprint): (String, Option<String>) = store.connection.query_row(
        "SELECT worker_id, session_fingerprint FROM task_claims WHERE task_id=?1 AND revision=?2",
        params![task_id, revision],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let fingerprint = fingerprint
        .filter(|s| !s.trim().is_empty() && s != crate::store::UNKNOWN_SESSION_FINGERPRINT)
        .context("TASK_RESUME_IDENTITY_MISSING: accepted target session unknown")?;
    if claim_worker != worker {
        bail!("TASK_RESUME_IDENTITY_MISSING: target worker differs from accepted claim");
    }
    Ok(Some((claim_worker, fingerprint)))
}
