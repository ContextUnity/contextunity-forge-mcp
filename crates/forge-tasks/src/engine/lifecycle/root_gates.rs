use super::proof_fanout::*;
use super::*;

#[derive(Debug, Clone)]
pub(super) struct PendingRootGateAction {
    pub(super) profile: std::sync::Arc<crate::tasks::profile::GateProfile>,
    pub(super) milestone_identity: String,
    pub(super) cursor: gate_graph::GateCursor,
}

#[derive(Debug)]
pub(super) struct PendingRootGateDispatch(pub(super) PendingRootGateAction);

impl std::fmt::Display for PendingRootGateDispatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MILESTONE_ROOT_GATE_READY")
    }
}

impl std::error::Error for PendingRootGateDispatch {}

fn pending_root_gate_from_transitions(
    store: &TasksStore,
    milestone_identity: &str,
    cursors: &[gate_graph::GateCursor],
    mut applicable: impl FnMut(&gate_graph::GateCursor) -> bool,
) -> Result<Option<gate_graph::GateCursor>> {
    let applicable_indices = cursors
        .iter()
        .enumerate()
        .filter_map(|(index, cursor)| applicable(cursor).then_some(index))
        .collect::<Vec<_>>();
    let Some(first_index) = applicable_indices.first().copied() else {
        return Ok(None);
    };
    let applicable_paths = applicable_indices
        .iter()
        .map(|index| cursors[*index].canonical_stage_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let is_applicable = |candidate: &gate_graph::GateCursor| {
        applicable_paths.contains(candidate.canonical_stage_id.as_str())
    };

    let path_placeholders = std::iter::repeat_n("?", applicable_indices.len())
        .collect::<Vec<_>>()
        .join(", ");
    let latest_transition_sql = format!(
        "SELECT gate_path, action, proof_payload FROM gate_transitions
         WHERE milestone_id=? AND subject='milestone' AND gate_path IN ({path_placeholders})
         ORDER BY id DESC LIMIT 1"
    );
    let mut transition_values = Vec::with_capacity(applicable_indices.len() + 1);
    transition_values.push(rusqlite::types::Value::Text(milestone_identity.to_owned()));
    transition_values.extend(
        applicable_indices
            .iter()
            .map(|index| rusqlite::types::Value::Text(cursors[*index].canonical_stage_id.clone())),
    );
    let latest: Option<(String, String, String)> = store
        .connection
        .query_row(
            &latest_transition_sql,
            rusqlite::params_from_iter(transition_values.iter()),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((last_gate_path, action, payload)) = latest else {
        return Ok(Some(cursors[first_index].clone()));
    };
    let current_index = applicable_indices
        .iter()
        .copied()
        .find(|index| cursors[*index].canonical_stage_id == last_gate_path)
        .context("MILESTONE_ROOT_GATE_TRANSITION_INVALID: latest gate path is not applicable")?;
    let payload: Value = serde_json::from_str(&payload).with_context(|| {
        format!(
            "MILESTONE_ROOT_GATE_TRANSITION_INVALID: '{last_gate_path}' has malformed proof payload"
        )
    })?;
    match action.as_str() {
        "pass" => {
            let Some(next_index) = gate_graph::resolve_pass_target(
                cursors,
                current_index,
                is_applicable,
            )?
            else {
                return Ok(None);
            };
            Ok(Some(
                cursors
                    .get(next_index)
                    .context("MILESTONE_ROOT_GATE_ROUTE_INVALID")?
                    .clone(),
            ))
        }
        "fail" => {
            let attempts: i64 = store.connection.query_row(
                "SELECT count(*) FROM gate_transitions
                 WHERE milestone_id=?1 AND subject='milestone' AND gate_path=?2",
                rusqlite::params![milestone_identity, last_gate_path],
                |row| row.get(0),
            )?;
            let attempts_used = u32::try_from(attempts)
                .context("MILESTONE_ROOT_GATE_ATTEMPT_COUNT_INVALID")?;
            if attempts_used == 0 {
                bail!("MILESTONE_ROOT_GATE_ATTEMPT_COUNT_INVALID");
            }
            let findings = payload.get("findings").filter(|findings| !findings.is_null());
            let route = gate_graph::resolve_fail_target(
                cursors,
                current_index,
                findings,
                attempts_used,
                is_applicable,
            )?;
            let Some(target) = route.target.filter(|_| !route.exhausted) else {
                return Ok(None);
            };
            let target_index = applicable_indices
                .iter()
                .copied()
                .find(|index| cursors[*index].canonical_stage_id == target.canonical_stage_id)
                .context("MILESTONE_ROOT_GATE_ROUTE_INVALID")?;
            Ok(Some(cursors[target_index].clone()))
        }
        _ => bail!(
            "MILESTONE_ROOT_GATE_TRANSITION_INVALID: '{last_gate_path}' has unsupported action '{action}'"
        ),
    }
}

fn pending_pre_task_root_gate(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
    milestone_tasks: &[Task],
    profile: std::sync::Arc<crate::tasks::profile::GateProfile>,
    scope: &[String],
    worktree: &Path,
) -> Result<PreTaskRootGateState> {
    let milestone_identity = milestone_identity_for_task(workspace, task)?;
    let Some(task_position) = gate_graph::task_container_position(&profile, Some(task))? else {
        return Ok(PreTaskRootGateState {
            milestone_identity,
            cursors: Vec::new(),
            pending: None,
        });
    };
    let cursors =
        applicable_pre_task_root_cursors(&profile, task, milestone_tasks, scope, worktree)?;
    let pending =
        pending_root_gate_from_transitions(store, &milestone_identity, &cursors, |cursor| {
            cursor.root_index < task_position
        })?;
    Ok(PreTaskRootGateState {
        milestone_identity: milestone_identity.clone(),
        cursors,
        pending: pending.map(|cursor| PendingRootGateAction {
            profile,
            milestone_identity,
            cursor,
        }),
    })
}

#[derive(Debug, Clone)]
pub(super) struct PreTaskRootGateState {
    pub(super) milestone_identity: String,
    pub(super) cursors: Vec<gate_graph::GateCursor>,
    pub(super) pending: Option<PendingRootGateAction>,
}

pub(in crate::engine) fn resolve_milestone_profile_for_task(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
) -> Result<std::sync::Arc<crate::tasks::profile::GateProfile>> {
    let manifest_path = confined_path(&workspace.root, &task.milestone_ref)?;
    let milestone_profile = if manifest_path.exists() {
        let text = std::fs::read_to_string(&manifest_path).with_context(|| {
            format!("unable to read milestone manifest '{}'", task.milestone_ref)
        })?;
        let normalized = text.replace("\r\n", "\n");
        let header = normalized
            .strip_prefix("---\n")
            .and_then(|rest| {
                rest.split_once("\n---\n")
                    .or_else(|| rest.split_once("\n---"))
            })
            .map(|(header, _)| header);
        header
            .map(serde_yaml::from_str::<serde_yaml::Value>)
            .transpose()?
            .and_then(|fm| {
                fm.get("acdd_profile")
                    .and_then(serde_yaml::Value::as_str)
                    .map(str::to_owned)
            })
    } else {
        None
    };
    store.profile_for_milestone(
        &workspace.root,
        milestone_profile.as_deref(),
        &workspace.gates,
        &[],
    )
}

pub(super) fn pre_task_root_gates_for_dispatch(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    tasks: &[Task],
    worktree: &Path,
) -> Result<std::collections::BTreeMap<String, PreTaskRootGateState>> {
    let mut by_milestone = std::collections::BTreeMap::<String, Vec<&Task>>::new();
    for task in tasks {
        by_milestone
            .entry(task.milestone_ref.clone())
            .or_default()
            .push(task);
    }

    let mut root_states = std::collections::BTreeMap::new();
    for milestone_tasks in by_milestone.values() {
        let owned_tasks = milestone_tasks
            .iter()
            .map(|task| (*task).clone())
            .collect::<Vec<_>>();
        let scope = milestone_task_scope(&owned_tasks);
        let Some(representative) = owned_tasks
            .iter()
            .find(|task| task.status != "cancelled")
            .or_else(|| owned_tasks.first())
        else {
            continue;
        };
        let profile = resolve_milestone_profile_for_task(store, workspace, representative)?;
        for task in milestone_tasks
            .iter()
            .filter(|task| matches!(task.status.as_str(), "ready" | "parked"))
        {
            let state = pending_pre_task_root_gate(
                store,
                workspace,
                task,
                &owned_tasks,
                profile.clone(),
                &scope,
                worktree,
            )?;
            root_states.insert(task.task_id.clone(), state);
        }
    }
    Ok(root_states)
}

pub(super) fn pending_post_task_root_gate(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    tasks: &[Task],
    root: &Path,
) -> Result<Option<PendingRootGateAction>> {
    let mut by_milestone = std::collections::BTreeMap::<String, Vec<&Task>>::new();
    for task in tasks {
        by_milestone
            .entry(task.milestone_ref.clone())
            .or_default()
            .push(task);
    }

    for milestone_tasks in by_milestone.values() {
        if milestone_tasks.is_empty()
            || milestone_tasks
                .iter()
                .all(|task| task.status == "cancelled")
            || milestone_tasks
                .iter()
                .any(|task| !matches!(task.status.as_str(), "completed" | "cancelled"))
        {
            continue;
        }
        let task = milestone_tasks
            .iter()
            .find(|task| task.status != "cancelled")
            .copied()
            .or_else(|| milestone_tasks.first().copied())
            .context("MILESTONE_TASK_CONTAINER_INVALID: milestone has no tasks")?;
        let profile = resolve_milestone_profile_for_task(store, workspace, task)?;
        let Some(task_position) = gate_graph::task_container_position(&profile, Some(task))? else {
            continue;
        };
        let owned_tasks = milestone_tasks
            .iter()
            .filter(|task| task.status != "cancelled")
            .map(|task| (*task).clone())
            .collect::<Vec<_>>();
        let scope = milestone_task_scope(&owned_tasks);
        let post_task_cursors = gate_graph::collect_root_cursors(&profile)?;
        let milestone_identity = milestone_identity_for_task(workspace, task)?;
        if let Some(cursor) = pending_root_gate_from_transitions(
            store,
            &milestone_identity,
            &post_task_cursors,
            |cursor| {
                cursor.root_index > task_position
                    && gate_graph::gate_condition_matches(
                        cursor.when.as_deref(),
                        &scope,
                        &owned_tasks,
                        None,
                        Some(root),
                    )
            },
        )? {
            return Ok(Some(PendingRootGateAction {
                profile,
                milestone_identity,
                cursor,
            }));
        }
    }
    Ok(None)
}

pub(super) fn root_gate_action_value(
    workspace: &workspaces::Workspace,
    action: PendingRootGateAction,
    worker: &str,
) -> Result<Value> {
    let gate = &action.cursor.gate;
    let stage = &action.cursor.canonical_stage_id;
    let role_definition = gate
        .role
        .as_deref()
        .and_then(|role| action.profile.roles.get(role));
    let mut resolved_guidance = serde_json::Map::new();
    for slot in &gate.guidance {
        if let Some(guidance) = action.profile.guidance.get(slot) {
            resolved_guidance.insert(slot.clone(), serde_json::to_value(guidance)?);
        }
    }
    let proof_schema = gate
        .proof
        .resolve_schema(&action.profile.proofs)
        .map(serde_json::to_value)
        .transpose()?;
    let command_definition = gate
        .command
        .as_deref()
        .and_then(|command| action.profile.commands.get(command));
    let policy_definition = gate
        .policy
        .as_deref()
        .and_then(|policy| action.profile.policies.get(policy));
    let workflow_guidance = json!({
        "active_stage": stage,
        "subject": "milestone",
        "role": gate.role,
        "role_definition": role_definition,
        "role_guide": role_definition.and_then(|role| role.guidance.as_deref()),
        "guidance": resolved_guidance,
        "guidance_slots": gate.guidance,
        "steps": gate.steps,
        "tools": gate.tools,
        "proof_kind": gate.proof.kind().map(|kind| format!("{kind:?}").to_ascii_lowercase()),
        "proof_schema": proof_schema,
        "command": gate.command,
        "command_definition": command_definition,
        "policy": gate.policy,
        "policy_definition": policy_definition,
        "contours": gate.contours,
    });
    workspace.envelope(json!({
        "status": "ready",
        "subject": "milestone",
        "milestone_id": action.milestone_identity,
        "stage": stage,
        "active_stage_id": stage,
        "active_gate_path": action.cursor.gate_path,
        "worker_id": worker,
        "role": gate.role,
        "guidance": resolved_guidance,
        "gate": gate,
        "workflow_guidance": workflow_guidance,
    }))
}

struct MilestoneRootContext<'a> {
    workspace: &'a workspaces::Workspace,
    store: TasksStore,
    milestone_identity: String,
    milestone_tasks: Vec<Task>,
    profile: std::sync::Arc<crate::tasks::profile::GateProfile>,
    task_position: usize,
    scope: Vec<String>,
    root_cursors: Vec<gate_graph::GateCursor>,
    all_tasks_completed: bool,
}

fn resolve_milestone_for_root_gate<'a>(
    registry: &'a Registry,
    reference: &str,
) -> Result<(&'a workspaces::Workspace, String)> {
    let trimmed = reference.trim();
    if let Ok(resolved) = registry.resolve_milestone(None, trimmed) {
        return Ok(resolved);
    }
    let segments = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [repository, project, milestone_ref] => {
            if let Ok(owner) = registry.owner(&format!("{repository}/{project}")) {
                if let Ok(resolved) =
                    registry.resolve_milestone(Some(owner.name.as_str()), milestone_ref)
                {
                    return Ok(resolved);
                }
            }
        }
        [project, milestone_ref] => {
            if let Ok(resolved) = registry.resolve_milestone(Some(project), milestone_ref) {
                return Ok(resolved);
            }
        }
        _ => {}
    }
    bail!("{}", crate::store::task_not_found_message(reference));
}

fn load_milestone_root_context<'a>(
    registry: &'a Registry,
    reference_or_id: &str,
    worktree: &Path,
) -> Result<MilestoneRootContext<'a>> {
    let (workspace, reference) = resolve_milestone_for_root_gate(registry, reference_or_id)?;
    registry.check_worktree(workspace, worktree)?;
    let full_path = confined_path(&workspace.root, &reference)?;
    let text = std::fs::read_to_string(&full_path)
        .with_context(|| format!("unable to read milestone manifest '{reference}'"))?;
    let milestone = registry
        .parse_milestone_manifest(workspace, &reference, &text)
        .with_context(|| format!("invalid milestone manifest '{reference}'"))?;
    let milestone_identity = format!(
        "{}/{}/{}",
        milestone.repository, milestone.project, milestone.id
    );
    let mut store = workspace.open(&registry.database)?;
    store.lock_checkpoint()?;
    let mut milestone_tasks = store.list(Some(&reference), "all", None)?;
    milestone_tasks.retain(|task| task.status != "cancelled");
    let representative = milestone_tasks.first();
    let profile = store.profile_for_milestone(
        &workspace.root,
        milestone.acdd_profile.as_deref(),
        &workspace.gates,
        &milestone_tasks,
    )?;
    let task_position = gate_graph::task_container_position(&profile, representative)?
        .context("MILESTONE_TASK_CONTAINER_INVALID: active profile has no tasks container")?;
    let scope = if milestone_tasks.is_empty() {
        milestone
            .tasks
            .iter()
            .flat_map(|task| task.scope.iter().cloned())
            .collect()
    } else {
        milestone_task_scope(&milestone_tasks)
    };
    let root_cursors = gate_graph::collect_root_cursors(&profile)?;
    let all_tasks_completed = !milestone_tasks.is_empty()
        && !milestone_tasks
            .iter()
            .all(|task| task.status == "cancelled")
        && milestone_tasks
            .iter()
            .all(|task| matches!(task.status.as_str(), "completed" | "cancelled"));
    Ok(MilestoneRootContext {
        workspace,
        store,
        milestone_identity,
        milestone_tasks,
        profile,
        task_position,
        scope,
        root_cursors,
        all_tasks_completed,
    })
}

fn eligible_root_cursor(
    ctx: &MilestoneRootContext<'_>,
    worktree: &Path,
    requested_stage: &str,
) -> Result<gate_graph::GateCursor> {
    let pre_pending = pending_root_gate_from_transitions(
        &ctx.store,
        &ctx.milestone_identity,
        &ctx.root_cursors,
        |cursor| {
            cursor.root_index < ctx.task_position
                && gate_graph::gate_condition_matches(
                    cursor.when.as_deref(),
                    &ctx.scope,
                    &ctx.milestone_tasks,
                    None,
                    Some(worktree),
                )
        },
    )?;
    let post_pending = if ctx.all_tasks_completed && pre_pending.is_none() {
        pending_root_gate_from_transitions(
            &ctx.store,
            &ctx.milestone_identity,
            &ctx.root_cursors,
            |cursor| {
                cursor.root_index > ctx.task_position
                    && gate_graph::gate_condition_matches(
                        cursor.when.as_deref(),
                        &ctx.scope,
                        &ctx.milestone_tasks,
                        None,
                        Some(worktree),
                    )
            },
        )?
    } else {
        None
    };

    let stage = requested_stage.trim();
    let cursor = if stage.is_empty() {
        pre_pending.or(post_pending).with_context(|| {
            format!(
                "MILESTONE_ROOT_GATE_NOT_PENDING: milestone '{}' has no claimable root gate",
                ctx.milestone_identity
            )
        })?
    } else {
        let mut matches = ctx
            .root_cursors
            .iter()
            .filter(|cursor| {
                cursor.canonical_stage_id == stage
                    || cursor.gate.id == stage
                    || cursor.canonical_stage_id.starts_with(&format!("{stage}."))
            })
            .collect::<Vec<_>>();
        ensure!(
            !matches.is_empty(),
            "TASK_STAGE_UNKNOWN: '{stage}' is not a root gate in the active profile"
        );
        let candidate = if matches.len() == 1 {
            matches.remove(0).clone()
        } else {
            matches
                .iter()
                .copied()
                .find(|cursor| {
                    pre_pending.as_ref().is_some_and(|pending| {
                        pending.canonical_stage_id == cursor.canonical_stage_id
                    }) || post_pending.as_ref().is_some_and(|pending| {
                        pending.canonical_stage_id == cursor.canonical_stage_id
                    })
                })
                .cloned()
                .with_context(|| {
                    format!("MILESTONE_ROOT_GATE_NOT_READY: root gate '{stage}' is not pending")
                })?
        };
        ensure!(
            gate_graph::gate_condition_matches(
                candidate.when.as_deref(),
                &ctx.scope,
                &ctx.milestone_tasks,
                None,
                Some(worktree),
            ),
            "MILESTONE_ROOT_GATE_INAPPLICABLE: root gate '{}' condition is not met",
            candidate.canonical_stage_id
        );
        if candidate.root_index < ctx.task_position {
            ensure!(
                pre_pending.as_ref().is_some_and(|pending| {
                    pending.canonical_stage_id == candidate.canonical_stage_id
                }),
                "MILESTONE_PRE_TASK_GATE_PENDING: active pre-task root gate is '{}', not '{}'",
                pre_pending
                    .as_ref()
                    .map_or("none", |pending| pending.canonical_stage_id.as_str()),
                candidate.canonical_stage_id
            );
        } else if candidate.root_index > ctx.task_position {
            ensure!(
                pre_pending.is_none(),
                "MILESTONE_PRE_TASK_GATE_PENDING: pre-task root gate '{}' must pass first",
                pre_pending
                    .as_ref()
                    .map_or("unknown", |pending| pending.canonical_stage_id.as_str())
            );
            ensure!(
                ctx.all_tasks_completed,
                "MILESTONE_POST_TASK_BARRIER_OPEN: post-task root gate '{}' requires all non-cancelled milestone tasks to be completed",
                candidate.canonical_stage_id
            );
            ensure!(
                post_pending.as_ref().is_some_and(|pending| {
                    pending.canonical_stage_id == candidate.canonical_stage_id
                }),
                "MILESTONE_ROOT_GATE_NOT_READY: active post-task root gate is '{}', not '{}'",
                post_pending
                    .as_ref()
                    .map_or("none", |pending| pending.canonical_stage_id.as_str()),
                candidate.canonical_stage_id
            );
        }
        candidate
    };
    let adapter = crate::config::load_workspace_adapter(&ctx.workspace.root)
        .context("failed to resolve workspace subsystem configuration")?;
    context::validate_gate_requirements(&ctx.profile, Some(&cursor.gate), &adapter.modules)?;
    Ok(cursor)
}

fn ensure_root_gate_independent(
    ctx: &MilestoneRootContext<'_>,
    cursor: &gate_graph::GateCursor,
    worker_id: &str,
) -> Result<()> {
    let Some(prior_ref) = cursor.gate.independent_from.as_deref() else {
        return Ok(());
    };
    let prior_stage = if let Some(found) = ctx
        .root_cursors
        .iter()
        .find(|entry| entry.canonical_stage_id == prior_ref)
    {
        found.canonical_stage_id.clone()
    } else if let Some(sibling) =
        cursor
            .canonical_stage_id
            .rsplit_once('.')
            .and_then(|(parent, _)| {
                let candidate = format!("{parent}.{prior_ref}");
                ctx.root_cursors
                    .iter()
                    .find(|entry| entry.canonical_stage_id == candidate)
            })
    {
        sibling.canonical_stage_id.clone()
    } else {
        let matches = ctx
            .root_cursors
            .iter()
            .filter(|entry| {
                entry.gate.id == prior_ref
                    || entry.canonical_stage_id.rsplit('.').next() == Some(prior_ref)
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [found] => found.canonical_stage_id.clone(),
            _ => bail!(
                "TASK_GATES_INVALID: independent_from '{prior_ref}' on root gate '{}' could not be resolved uniquely",
                cursor.canonical_stage_id
            ),
        }
    };
    let prior_transition: Option<(String, String)> = ctx
        .store
        .connection
        .query_row(
            "SELECT action, worker_id FROM gate_transitions
             WHERE milestone_id=?1 AND subject='milestone' AND gate_path=?2
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![ctx.milestone_identity, prior_stage],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((action, prior_worker)) = prior_transition else {
        // Eligible root gates whose predecessor was bypassed (for example `polish`
        // when `verify` passes directly to `milestone_review`) have no prior
        // execution worker to exclude here.
        return Ok(());
    };
    if action != "pass" {
        bail!(
            "TASK_GATES_INVALID: independent gate '{}' requires accepted pass for '{}'",
            cursor.canonical_stage_id,
            prior_stage
        );
    }
    ensure!(
        prior_worker != worker_id,
        "TASK_REVIEW_NOT_INDEPENDENT: milestone '{}'. reviewer worker '{}' matches prior worker '{}'",
        ctx.milestone_identity,
        worker_id,
        prior_worker
    );
    Ok(())
}

fn root_checkpoint_breadcrumb(milestone_identity: &str, cursor: &gate_graph::GateCursor) -> String {
    let leaf_stage = cursor
        .canonical_stage_id
        .rsplit('.')
        .next()
        .unwrap_or(&cursor.gate.id);
    format!(
        "{milestone_identity}/{}/milestone/{leaf_stage}",
        cursor.canonical_stage_id
    )
}

fn root_candidate_fingerprint(worktree: &Path, required: bool) -> Result<Option<(String, String)>> {
    if worktree.join(".git").exists() {
        let git_tree_sha = crate::git::git_stdout(
            worktree,
            &["rev-parse", "--verify", "HEAD^{tree}"],
            "TASK_CHECKPOINT_STATE_INVALID",
        )?;
        let fingerprint = super::candidate_fingerprint(worktree)?;
        Ok(Some((git_tree_sha, fingerprint)))
    } else if required {
        bail!(
            "TASK_SNAPSHOT_FAILED: root snapshot or immutable persistence requires a Git worktree"
        );
    } else {
        Ok(None)
    }
}

fn phase_root_gate_paths<'a>(
    ctx: &'a MilestoneRootContext<'_>,
    cursor: &gate_graph::GateCursor,
) -> Vec<&'a str> {
    let is_pre = cursor.root_index < ctx.task_position;
    ctx.root_cursors
        .iter()
        .filter(|candidate| (candidate.root_index < ctx.task_position) == is_pre)
        .map(|candidate| candidate.canonical_stage_id.as_str())
        .collect()
}

fn latest_accepted_root_snapshot(
    store: &TasksStore,
    milestone_identity: &str,
    phase_paths: &[&str],
) -> Result<Option<String>> {
    if phase_paths.is_empty() {
        return Ok(None);
    }
    let placeholders = std::iter::repeat_n("?", phase_paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT proof_payload FROM gate_transitions
         WHERE milestone_id=? AND subject='milestone' AND action='pass'
           AND gate_path IN ({placeholders})
         ORDER BY id DESC"
    );
    let mut params_vec = Vec::with_capacity(phase_paths.len() + 1);
    params_vec.push(rusqlite::types::Value::Text(milestone_identity.to_owned()));
    params_vec.extend(
        phase_paths
            .iter()
            .map(|path| rusqlite::types::Value::Text((*path).to_owned())),
    );
    let mut stmt = store.connection.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(params_vec.iter()))?;
    while let Some(row) = rows.next()? {
        let raw: String = row.get(0)?;
        if let Some(fingerprint) = serde_json::from_str::<Value>(&raw).ok().and_then(|value| {
            value
                .get("root_snapshot_fingerprint")
                .and_then(Value::as_str)
                .filter(|fp| !fp.is_empty())
                .map(str::to_owned)
        }) {
            return Ok(Some(fingerprint));
        }
    }
    Ok(None)
}

pub(super) fn claim_root_gate(root: &Path, p: Claim) -> Result<Value> {
    if p.worker_id.trim().is_empty() {
        bail!("TASK_WORKER_INVALID: worker_id must not be empty");
    }
    let worktree = crate::tasks::claim_worktree(&p.worktree)?;
    let canonical_worktree = worktree.canonicalize().with_context(|| {
        format!(
            "TASK_STALE_CLAIM: root worktree {} unavailable",
            worktree.display()
        )
    })?;
    let registry = Registry::load(root)?;
    let ctx = load_milestone_root_context(&registry, &p.task_id, &canonical_worktree)?;
    let cursor = eligible_root_cursor(&ctx, &canonical_worktree, &p.stage)?;
    ensure_root_gate_independent(&ctx, &cursor, &p.worker_id)?;
    if cursor.gate.persistence.pre == Some(crate::tasks::profile::PersistenceAction::Commit)
        && canonical_worktree.join(".git").exists()
        && !ctx.scope.is_empty()
    {
        crate::git::git_scoped_commit(
            &canonical_worktree,
            &ctx.scope,
            &format!(
                "pre({}): claim {}",
                cursor.canonical_stage_id, ctx.milestone_identity
            ),
        )?;
    }
    let needs_snapshot = cursor.gate.persistence.immutable
        || cursor.gate.persistence.pre == Some(crate::tasks::profile::PersistenceAction::Snapshot)
        || cursor.gate.persistence.post == Some(crate::tasks::profile::PersistenceAction::Snapshot);
    if let Some((git_tree_sha, fingerprint)) =
        root_candidate_fingerprint(&canonical_worktree, needs_snapshot)?
    {
        if cursor.gate.persistence.immutable {
            let phase_paths = phase_root_gate_paths(&ctx, &cursor);
            if let Some(prior_snapshot) =
                latest_accepted_root_snapshot(&ctx.store, &ctx.milestone_identity, &phase_paths)?
            {
                ensure!(
                    prior_snapshot == fingerprint,
                    "TASK_CANDIDATE_MISMATCH: milestone '{}' root gate '{}' requires an immutable candidate; working tree differs from prior snapshot",
                    ctx.milestone_identity,
                    cursor.canonical_stage_id
                );
            }
        }
        ctx.store
            .save_engine_checkpoint(&crate::store::GateCheckpoint {
                breadcrumb: root_checkpoint_breadcrumb(&ctx.milestone_identity, &cursor),
                milestone_id: ctx.milestone_identity.clone(),
                subject: "milestone".to_owned(),
                gate_path: cursor.canonical_stage_id.clone(),
                transition_id: None,
                git_tree_sha,
                state_payload: json!({
                    "subject": "milestone",
                    "milestone_id": ctx.milestone_identity,
                    "stage": cursor.canonical_stage_id,
                    "gate_path": cursor.canonical_stage_id,
                    "worker_id": p.worker_id,
                    "worktree": canonical_worktree,
                    "candidate_fingerprint": fingerprint,
                    "immutable": cursor.gate.persistence.immutable,
                }),
            })?;
    }
    let mut response = root_gate_action_value(
        ctx.workspace,
        PendingRootGateAction {
            profile: ctx.profile,
            milestone_identity: ctx.milestone_identity,
            cursor,
        },
        &p.worker_id,
    )?;
    if let Some(obj) = response.as_object_mut() {
        obj.insert("worktree".into(), json!(worktree));
    }
    Ok(response)
}

pub(super) fn verify_root_gate_with_options(
    root: &Path,
    milestone_ref: &str,
    worktree: Option<&str>,
    worker_id: Option<&str>,
    timeout_seconds: Option<u64>,
) -> Result<Value> {
    let resolved_worktree = worktree
        .map(crate::tasks::claim_worktree)
        .transpose()?
        .unwrap_or_else(|| root.to_path_buf());
    let canonical_worktree = resolved_worktree.canonicalize()?;
    let registry = Registry::load(root)?;
    let mut ctx = load_milestone_root_context(&registry, milestone_ref, &canonical_worktree)?;
    let cursor = eligible_root_cursor(&ctx, &canonical_worktree, "")?;
    if !matches!(
        cursor.gate.proof.kind(),
        Some(ProofKind::Command | ProofKind::None)
    ) {
        bail!(
            "TASK_VERIFY_UNSUPPORTED: root gate '{}' uses proof kind '{}' without executable command verification",
            cursor.canonical_stage_id,
            gate_proof_kind(&cursor.gate)
        );
    }
    let command_name = cursor.gate.command.as_deref().with_context(|| {
        format!(
            "MILESTONE_COMMAND_MISSING: root gate '{}' has no bound command",
            cursor.canonical_stage_id
        )
    })?;
    let command_def = ctx
        .profile
        .commands
        .get(command_name)
        .with_context(|| {
            format!(
                "MILESTONE_COMMAND_MISSING: command '{command_name}' is not registered in active profile"
            )
        })?
        .clone();
    if let Some(timeout) = timeout_seconds {
        if timeout == 0 || timeout > ctx.profile.limits.max_command_timeout_seconds {
            bail!(
                "TASK_VERIFY_INVALID: timeout_seconds must be within 1..={}",
                ctx.profile.limits.max_command_timeout_seconds
            );
        }
    }
    let plan = crate::milestones::resolve_command_plan(
        &canonical_worktree,
        command_name,
        &command_def,
        &ctx.scope,
        &ctx.profile,
    )?;
    if plan.is_empty() {
        bail!(
            "MILESTONE_COMMAND_MISSING: no command matched root gate '{}'",
            cursor.canonical_stage_id
        );
    }
    let fingerprint = super::candidate_fingerprint(&canonical_worktree)?;
    let worker = worker_id.unwrap_or("verify");
    let run = ctx.store.start_root_execution_run(
        &ctx.milestone_identity,
        &cursor.canonical_stage_id,
        worker,
        &canonical_worktree,
        &fingerprint,
    )?;
    let mut overall_passed = true;
    for (command_id, command_str, default_timeout) in plan {
        let attempt_id = ctx
            .store
            .reserve_execution_attempt(run.run_id, &command_id)?;
        let timeout = timeout_seconds
            .map(std::time::Duration::from_secs)
            .unwrap_or(default_timeout);
        let observation = crate::milestones::command_gate::observe_command(
            &canonical_worktree,
            &command_str,
            timeout,
            command_def.output(),
        )?;
        let timed_out = observation["timed_out"] == true;
        let exit_code = observation["exit_code"].as_i64().map(|value| value as i32);
        let passed = exit_code == Some(0) && !timed_out;
        overall_passed &= passed;
        let trace = crate::store::GateExecutionTrace {
            milestone_id: ctx.milestone_identity.clone(),
            subject: "milestone".into(),
            gate_path: cursor.canonical_stage_id.clone(),
            attempt_id,
            command_id,
            command_str,
            status: if timed_out {
                "timeout".into()
            } else {
                "completed".into()
            },
            exit_code,
            log_path: None,
            output_snippet: observation["diagnostic"].as_str().map(str::to_owned),
            stdout_sha256: None,
            duration_ms: observation["duration_ms"].as_i64().unwrap_or(0),
        };
        let facts = crate::store::CommandExecutionProof {
            passed: observation["tests_passed"].as_u64(),
            failed: observation["tests_failed"].as_u64(),
            seam_ref: None,
            seam_verified: false,
            passed_tests: std::collections::BTreeSet::new(),
            output_spec: command_def.output().cloned(),
            expected_signature: None,
            scope_violations: Vec::new(),
        };
        ctx.store
            .record_execution_trace_with_proof(run.run_id, &trace, Some(&facts))?;
        if !overall_passed {
            break;
        }
    }
    ctx.store.complete_execution_run(run.run_id)?;
    let run = ctx
        .store
        .latest_root_execution_run(
            &ctx.milestone_identity,
            &cursor.canonical_stage_id,
            &canonical_worktree.to_string_lossy(),
        )?
        .context("TASK_AUTO_PROOF_UNAVAILABLE: completed root execution run missing")?;
    let traces = ctx.store.claim_bound_execution_traces(run.run_id)?;
    let proof = if cursor.gate.proof.kind() == Some(ProofKind::Command) {
        Some(super::evidence_compiler::command_proof(
            &run,
            &traces,
            &cursor.gate,
            &ctx.profile,
            &canonical_worktree,
            &ctx.store,
        )?)
    } else {
        None
    };
    let last = traces.last().map(|entry| &entry.trace);
    Ok(json!({
        "milestone": ctx.milestone_identity,
        "stage": cursor.canonical_stage_id,
        "passed": overall_passed,
        "exit_code": last.and_then(|trace| trace.exit_code),
        "proof": proof,
        "output": last.and_then(|trace| trace.output_snippet.clone()),
    }))
}

pub(in crate::engine) fn submit_root_gate(root: &Path, p: Submit) -> Result<Value> {
    let evidence_obj = p.evidence.as_object().with_context(|| {
        format!(
            "TASK_EVIDENCE_INVALID: milestone '{}'. evidence is not a valid JSON object.",
            p.task_id
        )
    })?;
    let worker_id = evidence_obj
        .get("worker_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| {
            format!(
                "TASK_EVIDENCE_INVALID: milestone '{}'. evidence.worker_id is required.",
                p.task_id
            )
        })?
        .to_owned();
    let raw_worktree = evidence_obj
        .get("worktree")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| {
            format!(
                "TASK_EVIDENCE_INVALID: milestone '{}'. evidence.worktree is required.",
                p.task_id
            )
        })?
        .to_owned();
    let proof_value = evidence_obj.get("proof").cloned().with_context(|| {
        format!(
            "TASK_EVIDENCE_INVALID: milestone '{}'. evidence.proof is required.",
            p.task_id
        )
    })?;
    if proof_value.is_null() {
        bail!(
            "TASK_EVIDENCE_INVALID: milestone '{}' for stage '{}'. proof is null.",
            p.task_id,
            p.stage
        );
    }

    let worktree = crate::tasks::claim_worktree(&raw_worktree)?;
    let registry = Registry::load(root)?;
    let ctx = load_milestone_root_context(&registry, &p.task_id, &worktree)?;
    if p.stage.trim().is_empty() {
        bail!("TASK_STAGE_UNKNOWN: root gate submit requires a non-empty stage");
    }
    let cursor = eligible_root_cursor(&ctx, &worktree, &p.stage)?;
    ensure_root_gate_independent(&ctx, &cursor, &worker_id)?;

    let passed = p.action.name() == "pass";
    let proof_policy = ctx
        .milestone_tasks
        .first()
        .map_or("strict_production", |task| task.spec.proof_policy.as_str());
    crate::tasks::gates::validate_gate_proof(
        &proof_value,
        &cursor.gate,
        &ctx.profile,
        passed,
        proof_policy,
    )?;
    if passed {
        match cursor.gate.proof.kind() {
            Some(ProofKind::Contract | ProofKind::SeamTest) => {
                bail!(
                    "TASK_PROOF_UNOBSERVED: root gate '{}' cannot pass '{}' without observed verification",
                    cursor.canonical_stage_id,
                    gate_proof_kind(&cursor.gate)
                );
            }
            Some(ProofKind::Command) => {
                let canonical_worktree = worktree.canonicalize().with_context(|| {
                    format!(
                        "TASK_STALE_CLAIM: root worktree {} unavailable",
                        worktree.display()
                    )
                })?;
                let fingerprint = super::candidate_fingerprint(&canonical_worktree)?;
                let run = ctx
                    .store
                    .latest_root_execution_run(
                        &ctx.milestone_identity,
                        &cursor.canonical_stage_id,
                        &canonical_worktree.to_string_lossy(),
                    )?
                    .with_context(|| {
                        format!(
                            "TASK_PROOF_UNOBSERVED: root command gate '{}' requires observed verification via task verify",
                            cursor.canonical_stage_id
                        )
                    })?;
                if run.status != "completed" || run.candidate_fingerprint != fingerprint {
                    bail!(
                        "TASK_PROOF_UNOBSERVED: root command gate '{}' verification is incomplete or stale for current tree",
                        cursor.canonical_stage_id
                    );
                }
                let traces = ctx.store.claim_bound_execution_traces(run.run_id)?;
                let observed_envelope = super::evidence_compiler::command_proof(
                    &run,
                    &traces,
                    &cursor.gate,
                    &ctx.profile,
                    &canonical_worktree,
                    &ctx.store,
                )?;
                let supplied = proof_value
                    .get("command_proof")
                    .context("TASK_EVIDENCE_INVALID: command_proof is required")?;
                let actual = observed_envelope
                    .get("command_proof")
                    .context("TASK_AUTO_PROOF_UNAVAILABLE: observed command_proof missing")?;
                super::gates::submit::validate_observed_proof_fields(
                    "command_proof",
                    supplied,
                    actual,
                )?;
            }
            _ => {}
        }
    }

    let mut payload_map = serde_json::Map::new();
    payload_map.insert("evidence".into(), p.evidence.clone());
    if let Some(findings) = &p.findings {
        payload_map.insert("findings".into(), findings.clone());
    }

    let canonical_worktree = worktree.canonicalize().with_context(|| {
        format!(
            "TASK_STALE_CLAIM: root worktree {} unavailable",
            worktree.display()
        )
    })?;
    let needs_snapshot = cursor.gate.persistence.immutable
        || cursor.gate.persistence.post == Some(crate::tasks::profile::PersistenceAction::Snapshot);
    let current_snapshot = if passed {
        root_candidate_fingerprint(&canonical_worktree, needs_snapshot)?
    } else {
        None
    };
    if passed && cursor.gate.persistence.immutable {
        let (_, current_fp) = current_snapshot
            .as_ref()
            .context("TASK_SNAPSHOT_FAILED: immutable root gate requires a Git worktree")?;
        let breadcrumb = root_checkpoint_breadcrumb(&ctx.milestone_identity, &cursor);
        let checkpoint = ctx.store.get_checkpoint(&breadcrumb)?.with_context(|| {
            format!(
                "TASK_CANDIDATE_MISMATCH: milestone '{}' root gate '{}' requires an active claim snapshot; claim the gate before submitting",
                ctx.milestone_identity,
                cursor.canonical_stage_id
            )
        })?;
        let claimed_fp = checkpoint
            .state_payload
            .get("candidate_fingerprint")
            .and_then(Value::as_str)
            .with_context(|| {
                format!(
                    "TASK_CANDIDATE_MISMATCH: milestone '{}' root gate '{}' claim checkpoint has no candidate fingerprint",
                    ctx.milestone_identity,
                    cursor.canonical_stage_id
                )
            })?;
        ensure!(
            claimed_fp == current_fp,
            "TASK_CANDIDATE_MISMATCH: milestone '{}' root gate '{}' requires an immutable candidate; working tree differs from claimed snapshot",
            ctx.milestone_identity,
            cursor.canonical_stage_id
        );
        let phase_paths = phase_root_gate_paths(&ctx, &cursor);
        if let Some(prior_snapshot) =
            latest_accepted_root_snapshot(&ctx.store, &ctx.milestone_identity, &phase_paths)?
        {
            ensure!(
                prior_snapshot == *current_fp,
                "TASK_CANDIDATE_MISMATCH: milestone '{}' root gate '{}' requires an immutable candidate; working tree differs from prior snapshot",
                ctx.milestone_identity,
                cursor.canonical_stage_id
            );
        }
    }
    if passed
        && cursor.gate.persistence.post == Some(crate::tasks::profile::PersistenceAction::Snapshot)
    {
        if let Some((_, ref fingerprint)) = current_snapshot {
            payload_map.insert("root_snapshot_fingerprint".into(), json!(fingerprint));
        }
    }
    let proof_payload = Value::Object(payload_map);

    let representative_task = ctx
        .milestone_tasks
        .iter()
        .find(|task| task.status != "cancelled")
        .or_else(|| ctx.milestone_tasks.first());
    if passed && cursor.root_index < ctx.task_position {
        if let Some(task) = representative_task {
            validate_pre_task_root_proof(&ctx.store, ctx.workspace, task, &proof_payload)?;
        }
    }

    let git_tree_sha = current_snapshot
        .as_ref()
        .map(|(sha, _)| sha.clone())
        .unwrap_or_else(|| {
            if canonical_worktree.join(".git").exists() {
                crate::git::git_stdout(
                    &canonical_worktree,
                    &["rev-parse", "--verify", "HEAD^{tree}"],
                    "TASK_CHECKPOINT_STATE_INVALID",
                )
                .unwrap_or_else(|_| "unknown".to_owned())
            } else {
                "non-git".to_owned()
            }
        });
    let transition_id = ctx
        .store
        .record_gate_transition(&crate::store::GateTransition {
            milestone_id: ctx.milestone_identity.clone(),
            subject: "milestone".to_owned(),
            gate_path: cursor.canonical_stage_id.clone(),
            action: if passed { "pass" } else { "fail" }.to_owned(),
            proof_kind: gate_proof_kind(&cursor.gate),
            proof_payload,
            git_tree_sha: git_tree_sha.clone(),
            worker_id: worker_id.clone(),
        })?;
    if let Some((_, fingerprint)) = current_snapshot {
        ctx.store
            .save_engine_checkpoint(&crate::store::GateCheckpoint {
                breadcrumb: root_checkpoint_breadcrumb(&ctx.milestone_identity, &cursor),
                milestone_id: ctx.milestone_identity.clone(),
                subject: "milestone".to_owned(),
                gate_path: cursor.canonical_stage_id.clone(),
                transition_id: Some(transition_id),
                git_tree_sha,
                state_payload: json!({
                    "subject": "milestone",
                    "milestone_id": ctx.milestone_identity,
                    "stage": cursor.canonical_stage_id,
                    "gate_path": cursor.canonical_stage_id,
                    "worker_id": worker_id,
                    "candidate_fingerprint": fingerprint,
                    "immutable": cursor.gate.persistence.immutable,
                }),
            })?;
    }

    if passed && cursor.root_index < ctx.task_position {
        if let Some(task) = representative_task {
            fan_out_pre_task_proofs_if_ready(
                &ctx.store,
                ctx.workspace,
                task,
                &ctx.profile,
                &worktree,
            )?;
        }
    }

    ctx.workspace.envelope(json!({
        "status": if passed { "accepted" } else { "rejected" },
        "subject": "milestone",
        "milestone_id": ctx.milestone_identity,
        "stage": cursor.canonical_stage_id,
        "active_stage_id": cursor.canonical_stage_id,
        "gate_path": cursor.canonical_stage_id,
        "action": if passed { "pass" } else { "fail" },
        "transition_id": transition_id,
        "worker_id": worker_id,
    }))
}

pub(super) fn ensure_pre_task_root_gates_passed(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
    profile: &crate::tasks::profile::GateProfile,
) -> Result<()> {
    let milestone_tasks = store.list(Some(&task.milestone_ref), "all", None)?;
    let scope = milestone_task_scope(&milestone_tasks);
    let worktree = task
        .worktree
        .as_deref()
        .map(Path::new)
        .unwrap_or(&workspace.root);
    let cursors =
        applicable_pre_task_root_cursors(profile, task, &milestone_tasks, &scope, worktree)?;
    let milestone_identity = milestone_identity_for_task(workspace, task)?;
    gate_graph::ensure_root_gate_transitions_passed(
        &store.connection,
        &milestone_identity,
        &cursors,
    )?;
    fan_out_pre_task_proofs_if_ready(store, workspace, task, profile, worktree)?;
    Ok(())
}
