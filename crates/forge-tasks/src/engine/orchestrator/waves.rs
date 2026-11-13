use super::{
    role::RoleObservations,
    task_runner::{run_task_lifecycle, TaskRunOptions},
    types::OrchestratorEvent,
    workflow::{run_gate_graph, GateDecision},
};
use crate::{
    engine::{self, gate_graph, Registry},
    milestones,
    tasks::{
        profile::{GateDriver, ProofKind},
        Milestone,
    },
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::Path,
};

/// Dispatch policy for one milestone runner invocation.
pub struct MilestoneRunOptions {
    pub task: TaskRunOptions,
    pub in_place: bool,
    pub target_base: String,
}

/// Run validated task waves through the existing task lifecycle, then root gates.
pub fn run_milestone_waves(
    root: &Path,
    selector: &str,
    options: &MilestoneRunOptions,
    observer: &mut dyn FnMut(OrchestratorEvent),
) -> Result<Value> {
    ensure!(
        !options.target_base.trim().is_empty(),
        "MILESTONE_TARGET_BASE_REQUIRED"
    );
    let root = root.canonicalize()?;
    let registry = Registry::load(&root)?;
    let (workspace, reference) = registry.resolve_milestone(None, selector)?;
    ensure!(
        workspace.root == root,
        "MILESTONE_RUN_WORKSPACE_REQUIRED: run in the owning workspace"
    );
    let text = fs::read_to_string(root.join(&reference))?;
    let milestone = registry.parse_milestone_manifest(workspace, &reference, &text)?;
    let waves = milestones::validate_wave_dag(&milestone)?;
    let mut sync = engine::Manage {
        action: engine::ManageAction::Sync,
        ..Default::default()
    };
    sync.milestone_ref = Some(reference.clone());
    engine::manage(&root, sync)?;
    let store = registry.open(workspace)?;
    let synchronized_tasks = store.list(Some(&reference), "all", None)?;
    let active_tasks = synchronized_tasks
        .iter()
        .filter(|task| task.status != "cancelled")
        .cloned()
        .collect::<Vec<_>>();
    let task = active_tasks
        .first()
        .or_else(|| synchronized_tasks.first())
        .context("MILESTONE_TASK_CONTAINER_INVALID: milestone has no tasks")?;
    let profile = store.profile_for_milestone(
        &root,
        milestone.acdd_profile.as_deref(),
        &workspace.gates,
        &synchronized_tasks,
    )?;
    let cursors = gate_graph::collect_root_cursors(&profile)?;
    let task_position = gate_graph::task_container_position(&profile, Some(task))?
        .context("MILESTONE_TASK_CONTAINER_INVALID: no task lifecycle container applies")?;
    if !options.in_place {
        ensure!(clean_worktree(&root)?, "MILESTONE_ISOLATION_DIRTY: isolated dispatch requires a clean integrated milestone checkout");
    }
    let identity = format!(
        "{}/{}/{}",
        milestone.repository, milestone.project, milestone.id
    );
    let scope = milestone
        .tasks
        .iter()
        .flat_map(|task| task.scope.iter().cloned())
        .collect::<Vec<_>>();
    let pre_task_required = cursors
        .iter()
        .filter(|cursor| {
            cursor.root_index < task_position
                && gate_graph::gate_condition_matches(
                    cursor.when.as_deref(),
                    &scope,
                    &active_tasks,
                    None,
                    Some(&root),
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut tasks_done = false;
    let mut verification: Option<Value> = None;
    let mut review: Option<(milestones::HandoffBinding, milestones::IntegrationReview)> = None;
    let mut roles = RoleObservations::default();
    let mut handoff: Option<Value> = None;
    let profile_digest = digest(profile.as_ref())?;
    let mut authority = milestone.clone();
    authority.status = None;
    for task in &mut authority.tasks {
        task.status = None;
        task.receipt = None;
    }
    let authority_digest = digest(&authority)?;
    // Replay observations through the canonical router. These are existing transition
    // records, not a second cursor or task registry. A changed profile/authority or
    // candidate invalidates the remaining prefix and requires fresh observations.
    let mut history = {
        let store = registry.open(workspace)?;
        let mut query = store.connection.prepare("SELECT id, gate_path, proof_payload FROM gate_transitions WHERE milestone_id=?1 AND subject='milestone' AND proof_kind='orchestrator' ORDER BY id")?;
        let rows = query
            .query_map([&identity], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (id, gate, payload) = row?;
                Ok((id, (gate, serde_json::from_str::<Value>(&payload)?)))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        // The latest observation links to the exact prefix actually used, so a
        // refreshed suffix cannot be hidden behind its obsolete audit records.
        let mut route = VecDeque::new();
        let mut next = rows.last_key_value().map(|(&id, _)| id);
        while let Some(id) = next {
            let (gate, saved) = rows
                .get(&id)
                .context("MILESTONE_CHECKPOINT_INVALID: missing predecessor observation")?;
            route.push_front((id, gate.clone(), saved.clone()));
            next = match saved.get("previous_observation") {
                Some(Value::Null) => None,
                Some(value) => Some(value.as_i64().filter(|previous| *previous > 0 && *previous < id).context("MILESTONE_CHECKPOINT_INVALID: predecessor must be an earlier observation")?),
                // Unlinked records follow their chronological start boundary.
                None if saved["run_start"] == true => None,
                None => rows.range(..id).next_back().map(|(&id, _)| id),
            };
        }
        route
    };
    let mut previous_observation = None;
    let outcomes = run_gate_graph(&root, &scope, &active_tasks, &cursors, |cursor, attempt| {
        roles.note_executed(&cursor.canonical_stage_id);
        if !tasks_done && cursor.root_index > task_position {
            gate_graph::ensure_root_gate_transitions_passed(
                &registry.open(workspace)?.connection,
                &identity,
                &pre_task_required,
            )?;
            dispatch_waves(&root, &milestone, &waves, options, observer)?;
            tasks_done = true;
        }
        let binding = if tasks_done {
            milestones::handoff_context(&root, selector, &options.target_base)?["binding"].clone()
        } else {
            json!({"candidate_fingerprint":engine::candidate_fingerprint(&root)?})
        };
        let cached = history
            .front()
            .filter(|(_, gate, saved)| {
                gate == &cursor.canonical_stage_id
                    && saved["profile_digest"] == profile_digest
                    && saved["authority_digest"] == authority_digest
                    && saved["binding"] == binding
            })
            .cloned();
        if cached.is_some() {
            history.pop_front();
        } else {
            history.clear();
        }
        if cursor.gate.receipt || cursor.gate.proof.kind() == Some(ProofKind::Delivery) {
            ensure!(tasks_done, "MILESTONE_TASKS_INCOMPLETE");
            let current = milestones::handoff_context(&root, selector, &options.target_base)?;
            let observed = verification.as_ref().context(
                "HANDOFF_VERIFICATION_REQUIRED: no accepted root verification was observed",
            )?;
            ensure!(
                observed["binding"] == current["binding"],
                "HANDOFF_EVIDENCE_STALE: verification is not bound to final content"
            );
            let (binding, accepted_review) = review.as_ref().context("HANDOFF_REVIEW_REQUIRED: profile reached handoff without current-content independent integration review")?;
            ensure!(
                serde_json::to_value(binding)? == current["binding"],
                "HANDOFF_EVIDENCE_STALE: review is not bound to final content"
            );
            let check = &observed["verification"];
            let evidence = milestones::IntegrationEvidence {
                binding: binding.clone(),
                target_base: options.target_base.clone(),
                review: accepted_review.clone(),
                verification: milestones::IntegrationVerification {
                    command: check["command"]
                        .as_str()
                        .context("HANDOFF_VERIFICATION_INVALID")?
                        .into(),
                    exit_code: i32::try_from(
                        check["exit_code"]
                            .as_i64()
                            .context("HANDOFF_VERIFICATION_INVALID")?,
                    )?,
                    tests_passed: check["tests_passed"]
                        .as_u64()
                        .context("HANDOFF_VERIFICATION_INVALID")?,
                    tests_failed: check["tests_failed"]
                        .as_u64()
                        .context("HANDOFF_VERIFICATION_INVALID")?,
                    lint_command: None,
                    project_command: Some(
                        check["command_id"]
                            .as_str()
                            .context("HANDOFF_VERIFICATION_INVALID")?
                            .into(),
                    ),
                },
            };
            let result = milestones::handoff(&root, selector, None, &evidence)?;
            handoff = Some(result.clone());
            return Ok(GateDecision {
                passed: true,
                evidence: result,
                findings: None,
            });
        }
        let mut replayed = cached.is_some();
        let mut decision = if let Some((id, _, saved)) = cached {
            previous_observation = Some(id);
            let decision = GateDecision {
                passed: saved["passed"]
                    .as_bool()
                    .context("MILESTONE_CHECKPOINT_INVALID")?,
                evidence: saved["evidence"].clone(),
                findings: saved
                    .get("findings")
                    .filter(|value| !value.is_null())
                    .cloned(),
            };
            roles.restore(&cursor.canonical_stage_id, &decision);
            if cursor.gate.driver() == Some(GateDriver::Command) && tasks_done && decision.passed {
                verification = Some(decision.evidence.clone());
            }
            decision
        } else if cursor.gate.driver() == Some(GateDriver::Command) && tasks_done {
            let result = milestones::execute_wave_verification(
                &root,
                selector,
                &options.target_base,
                &cursor.canonical_stage_id,
            )?;
            let passed = result["status"] == "passed";
            if passed {
                verification = Some(result.clone());
            } else {
                verification = None;
            }
            GateDecision {
                passed,
                evidence: result,
                findings: None,
            }
        } else if cursor.gate.driver() == Some(GateDriver::Command) {
            let name = cursor
                .gate
                .command
                .as_deref()
                .context("MILESTONE_COMMAND_MISSING")?;
            let command = profile
                .commands
                .get(name)
                .context("MILESTONE_COMMAND_MISSING")?;
            let plan = milestones::resolve_command_plan(&root, name, command, &scope, &profile)?;
            ensure!(!plan.is_empty(), "MILESTONE_COMMAND_MISSING");
            let mut observations = Vec::new();
            let mut passed = true;
            for (_, command_str, timeout) in plan {
                let result = milestones::command_gate::observe_command(
                    &root,
                    &command_str,
                    timeout,
                    command.output(),
                )?;
                passed &= result["exit_code"] == 0 && result["timed_out"] == false;
                observations.push(result);
                if !passed {
                    break;
                }
            }
            GateDecision {
                passed,
                evidence: json!(observations),
                findings: None,
            }
        } else {
            ensure!(
                cursor.gate.role.is_some() || cursor.gate.command.is_some(),
                "MILESTONE_COMMAND_MISSING: gate '{}' has no bound command",
                cursor.canonical_stage_id
            );
            if tasks_done {
                let context = milestones::handoff_context(&root, selector, &options.target_base)?;
                let binding: milestones::HandoffBinding =
                    serde_json::from_value(context["binding"].clone())?;
                let store = registry.open(workspace)?;
                let mut forbidden = Vec::new();
                for builder in &binding.builders {
                    let mut statement = store.connection.prepare("SELECT DISTINCT c.session_fingerprint FROM task_claims c JOIN tasks t ON t.task_id=c.task_id WHERE c.worker_id=?1 AND t.milestone_ref=?2")?;
                    let sessions = statement
                        .query_map(rusqlite::params![builder, reference], |row| {
                            row.get::<_, Option<String>>(0)
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    ensure!(!sessions.is_empty() && sessions.iter().all(|session| session.as_ref().is_some_and(|session| !session.is_empty() && session != crate::store::UNKNOWN_SESSION_FINGERPRINT)), "TASK_REVIEW_SESSION_UNKNOWN: accepted milestone builder session is unavailable");
                    forbidden.extend(sessions.into_iter().flatten());
                }
                roles.set_subject_context(context, forbidden);
            } else {
                roles.set_subject_context(
                    json!({"manifest_path":reference,"manifest":milestone,"source":text}),
                    Vec::new(),
                );
            }
            roles.execute(
                &root,
                &identity,
                &format!("Complete milestone {}", milestone.id),
                &scope,
                cursor,
                &profile,
            )?
        };
        if decision.passed && !tasks_done && cursor.root_index < task_position {
            let proof_validation = registry.open(workspace).and_then(|store| {
                super::super::lifecycle::validate_pre_task_root_proof(
                    &store,
                    workspace,
                    task,
                    &decision.evidence,
                )
            });
            if let Err(error) = proof_validation {
                decision = GateDecision {
                    passed: false,
                    evidence: decision.evidence,
                    findings: Some(json!({
                        "code": "MILESTONE_TASK_PROOF_INVALID",
                        "message": format!("{error:#}"),
                    })),
                };
                roles.restore(&cursor.canonical_stage_id, &decision);
                // A replayed pass with invalid task coverage must be superseded
                // by a durable failure so the configured repair route can run.
                replayed = false;
            }
        }
        if decision.passed && !decision.evidence["integration_review"].is_null() {
            ensure!(cursor.gate.independent_from.is_some(), "HANDOFF_REVIEW_NOT_INDEPENDENT: integration review needs a declared independent gate");
            review = Some((
                serde_json::from_value(decision.evidence["binding"].clone())?,
                serde_json::from_value(decision.evidence["integration_review"].clone())?,
            ));
        } else if !decision.passed && cursor.gate.independent_from.is_some() {
            review = None;
        }
        if replayed {
            return Ok(decision);
        }
        let store = registry.open(workspace)?;
        let id = store.record_gate_transition(&crate::store::GateTransition {
            milestone_id:identity.clone(), subject:"milestone".into(), gate_path:cursor.canonical_stage_id.clone(),
            action:if decision.passed { "pass" } else { "fail" }.into(), proof_kind:"orchestrator".into(),
            proof_payload:json!({"previous_observation":previous_observation,"profile_digest":profile_digest,"authority_digest":authority_digest,"binding":if tasks_done { milestones::handoff_context(&root, selector, &options.target_base)?["binding"].clone() } else {json!({"candidate_fingerprint":engine::candidate_fingerprint(&root)?})},"passed":decision.passed,"attempt":attempt,"evidence":decision.evidence,"findings":decision.findings}),
            git_tree_sha:crate::git::git_stdout(&root, &["rev-parse", "HEAD"], "MILESTONE_HEAD_REQUIRED")?,
            worker_id:decision.evidence["session_id"].as_str().unwrap_or(&options.task.worker_id).to_owned(),
        })?;
        if decision.passed && !tasks_done && cursor.root_index < task_position {
            super::super::lifecycle::fan_out_pre_task_proofs_if_ready(
                &store, workspace, task, &profile, &root,
            )?;
        }
        previous_observation = Some(id);
        Ok(decision)
    })?;
    if !tasks_done {
        gate_graph::ensure_root_gate_transitions_passed(
            &registry.open(workspace)?.connection,
            &identity,
            &pre_task_required,
        )?;
        dispatch_waves(&root, &milestone, &waves, options, observer)?;
    }
    let result: Value = handoff
        .take()
        .context("MILESTONE_HANDOFF_REQUIRED: root graph completed without accepted handoff")?;
    Ok(json!({"status":"completed","milestone":milestone.id,"outcomes":outcomes,"handoff":result}))
}

fn digest(value: &(impl serde::Serialize + ?Sized)) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn dispatch_waves(
    root: &Path,
    milestone: &Milestone,
    waves: &[Vec<String>],
    options: &MilestoneRunOptions,
    observer: &mut dyn FnMut(OrchestratorEvent),
) -> Result<()> {
    let registry = Registry::load(root)?;
    for (wave_index, wave) in waves.iter().enumerate() {
        let ids = wave
            .iter()
            .map(|task| format!("{}/{}:{task}", milestone.project, milestone.id))
            .collect::<Vec<_>>();
        observer(OrchestratorEvent::WaveStarted {
            wave_index,
            task_ids: ids.clone(),
        });
        // In-place writers and isolated integration are both serial. Every task
        // starts from the integrated predecessor content, never a stale wave base.
        for id in &ids {
            let workspace = registry.owner(id)?;
            let store = registry.open(workspace)?;
            let task = store.inspect(id)?;
            if task.status == "cancelled" {
                continue;
            }
            if task.status == "completed" {
                require_integrated_delivery(root, &task, options.in_place)?;
                continue;
            }
            ensure!(options.in_place || task.status != "in_progress", "TASK_ALREADY_CLAIMED: isolated task '{id}' retains an active claim in {:?}; recover it before provisioning another worktree", task.worktree);
            for dependency in &task.spec.depends_on {
                if let Some(dependency) = milestone
                    .tasks
                    .iter()
                    .find(|task| &task.task_ref == dependency)
                {
                    let prior = store.inspect(&format!(
                        "{}/{}:{}",
                        milestone.project, milestone.id, dependency.task_ref
                    ))?;
                    ensure!(
                        prior.status == "completed" && prior.receipt.is_some(),
                        "TASK_DEPENDENCY_INCOMPLETE: {}",
                        prior.task_id
                    );
                    require_integrated_delivery(root, &prior, options.in_place)?;
                }
            }
            drop(store);
            if options.in_place {
                let mut task_options = options.task.clone();
                task_options.worktree = root.to_owned();
                run_task_lifecycle(root, id, &task_options, observer)?;
            } else {
                run_isolated_task(root, id, options, observer)?;
            }
            let accepted = registry.open(workspace)?.inspect(id)?;
            ensure!(
                accepted.status == "completed" && accepted.receipt.is_some(),
                "MILESTONE_WAVE_INCOMPLETE: {id}"
            );
            require_integrated_delivery(root, &accepted, options.in_place)?;
        }
        for id in &ids {
            let workspace = registry.owner(id)?;
            let accepted = registry.open(workspace)?.inspect(id)?;
            ensure!(
                accepted.status == "cancelled"
                    || (accepted.status == "completed" && accepted.receipt.is_some()),
                "MILESTONE_WAVE_INCOMPLETE: {id}"
            );
            if accepted.status == "completed" {
                require_integrated_delivery(root, &accepted, options.in_place)?;
            }
        }
        observer(OrchestratorEvent::WaveCompleted {
            wave_index,
            task_ids: ids,
        });
    }
    Ok(())
}

fn require_integrated_delivery(
    root: &Path,
    task: &crate::store::Task,
    in_place: bool,
) -> Result<()> {
    let receipt = task.receipt.as_ref().context("TASK_RECEIPT_MISSING")?;
    if !in_place || receipt.commit.is_some() {
        let commit = receipt.commit.as_deref().context("MILESTONE_DELIVERY_NOT_INTEGRATED: isolated delivery has no commit; preserve and reconcile its worktree before resuming")?;
        ensure!(
            crate::git::git_ancestor(root, commit, "HEAD")?,
            "MILESTONE_DELIVERY_NOT_INTEGRATED: task '{}' delivery {} is absent from integrated HEAD; reconcile the accepted delivery from {:?} before resuming; worktree and proofs are preserved",
            task.task_id, commit, task.worktree
        );
    }
    Ok(())
}

fn run_isolated_task(
    root: &Path,
    id: &str,
    options: &MilestoneRunOptions,
    observer: &mut dyn FnMut(OrchestratorEvent),
) -> Result<()> {
    let token = format!(
        "{:x}",
        Sha256::digest(format!(
            "{id}\n{}\n{}",
            options.task.session_fingerprint,
            chrono::Utc::now()
                .timestamp_nanos_opt()
                .context("MILESTONE_CLOCK_INVALID")?
        ))
    );
    let branch = format!("forge-wave-{}", &token[..24]);
    let path = crate::fs::checked_child(root, Path::new(&format!(".worktrees/{branch}")))?;
    ensure!(
        !path.exists(),
        "MILESTONE_WORKTREE_EXISTS: refusing to acquire a foreign worktree"
    );
    fs::create_dir_all(path.parent().context("MILESTONE_WORKTREE_PARENT_INVALID")?)?;
    let path_text = path.to_str().context("MILESTONE_WORKTREE_PATH_INVALID")?;
    git_success(
        root,
        &["worktree", "add", "-b", &branch, "--", path_text, "HEAD"],
    )?;
    let mut task_options = options.task.clone();
    task_options.worktree = path.clone();
    // Failure intentionally preserves this owned worktree and its branch for recovery.
    let mut run = || run_task_lifecycle(&path, id, &task_options, observer);
    let result = match crate::tasks::configuration::current(root) {
        Some(settings) => settings.rebase(&path).run(run),
        None => run(),
    };
    result.with_context(|| {
        format!(
            "MILESTONE_TASK_FAILED: recoverable worktree {}",
            path.display()
        )
    })?;
    let commit = crate::git::git_stdout(
        &path,
        &["rev-parse", "HEAD"],
        "MILESTONE_INTEGRATION_FAILED",
    )?;
    ensure!(
        clean_worktree(&path)?,
        "MILESTONE_INTEGRATION_DIRTY: preserve worktree {}",
        path.display()
    );
    git_success(root, &["merge", "--ff-only", "--", &commit])?;
    git_success(root, &["worktree", "remove", "--", path_text])?;
    git_success(root, &["branch", "-d", "--", &branch])?;
    Ok(())
}

fn git_success(root: &Path, arguments: &[&str]) -> Result<()> {
    let output = crate::git::git_command(root, arguments)?;
    ensure!(
        output.status.success(),
        "MILESTONE_GIT_FAILED: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn clean_worktree(root: &Path) -> Result<bool> {
    let output = crate::git::git_command(root, &["status", "--porcelain"])?;
    ensure!(output.status.success(), "MILESTONE_STATUS_FAILED");
    Ok(output.stdout.is_empty())
}
