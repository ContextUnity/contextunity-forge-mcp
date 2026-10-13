use super::*;
use std::fs::{File, OpenOptions};

/// Performs claim.
pub fn claim(root: &Path, p: Claim) -> Result<Value> {
    let worktree = crate::core::tasks::claim_worktree(&p.worktree)?;
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    registry.check_worktree(workspace, &worktree)?;
    let mut store = workspace.open(&registry.database)?;
    let task = store.inspect(&p.task_id)?;
    validate_authority(&task, &worktree, &workspace.repository, &workspace.project)?;
    registry.check_scope(workspace, &task.spec.scope, Some(&worktree))?;
    let claimed = store.claim(&p.task_id, &p.stage, &p.worker_id, &p.worktree)?;
    let activation = (|| {
        let milestone_prefix = p
            .task_id
            .split_once(':')
            .context("invalid task identity")?
            .0
            .to_string()
            + ":";
        let claimed_at = store
            .earliest_claim(&milestone_prefix)?
            .context("task claim timestamp missing")?;
        crate::engine::milestones::activate_on_claim(&worktree, &claimed.milestone_ref, claimed_at)
    })();
    if let Err(error) = activation {
        store
            .release_failed_claim(&p.task_id, claimed.claim_revision)
            .context("failed to release claim after milestone activation error")?;
        return Err(error);
    }
    let response = (|| {
        let details = workspace.guidance_envelope(store.inspect_details(&p.task_id)?)?;
        if p.bundle.unwrap_or(true) {
            context_bundle(root, workspace, &p.task_id, details)
        } else {
            let mut minimal = details;
            if let Some(obj) = minimal.as_object_mut() {
                let depends_on = obj
                    .get("spec")
                    .and_then(|s| s.get("depends_on"))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let subtasks = obj
                    .get("subtasks")
                    .cloned()
                    .or_else(|| obj.get("spec").and_then(|s| s.get("subtasks")).cloned())
                    .unwrap_or_else(|| serde_json::json!([]));
                obj.remove("gates");
                obj.remove("attempts");
                obj.remove("findings");
                obj.remove("receipt");
                obj.remove("spec");
                obj.insert("depends_on".into(), depends_on);
                obj.insert("subtasks".into(), subtasks);
            }
            Ok(minimal)
        }
    })();
    if response.is_err() {
        store
            .release_failed_claim(&p.task_id, claimed.claim_revision)
            .context("failed to release claim after context error")?;
    }
    response
}
pub(super) fn lock_receipts(database: &Path) -> Result<File> {
    let lock_path = database.with_extension("receipt.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    file.lock()?;
    Ok(file)
}

/// Resets or reopens a task back to ready state at contract gate, clearing terminal receipt and updating milestone text if needed.
pub fn reset(root: &Path, id: &str) -> Result<Task> {
    let registry = Registry::load(root)?;
    let workspace = registry.owner(id)?;
    let mut store = workspace.open(&registry.database)?;
    let _receipt_lock = lock_receipts(&registry.database)?;
    let previous = store.inspect(id)?;
    if previous.status != "completed" {
        return store.reset(id);
    }
    let milestone_path = confined_path(&workspace.root, &previous.milestone_ref)?;
    let original = std::fs::read_to_string(&milestone_path)?;
    let task_ref = id
        .rsplit_once(':')
        .map(|(_, reference)| reference)
        .unwrap_or(id);
    let updated = crate::engine::milestones::clear_task_receipt(&original, task_ref)?;
    crate::core::fs::atomic_replace(&milestone_path, updated.as_bytes())?;
    match store.reset(id) {
        Ok(task) => Ok(task),
        Err(error) => {
            crate::core::fs::atomic_replace(&milestone_path, original.as_bytes())
                .context("TASK_RECEIPT_ROLLBACK_FAILED")?;
            Err(error)
        }
    }
}

/// Reopens a task back to ready state at contract gate.
pub fn reopen(root: &Path, id: &str) -> Result<Task> {
    reset(root, id)
}
/// Performs manage.
pub fn manage(root: &Path, mut p: Manage) -> Result<Value> {
    let id = p.task_id.as_deref();
    let milestone = p.milestone_ref.as_deref();
    let task_ref = p.task_ref.as_deref();
    let paths = p.paths.as_deref();
    let workspace = p.workspace.as_deref();
    let valid = match p.action {
        ManageAction::Create => {
            id.is_none() && milestone.is_some() && task_ref.is_some() && paths.is_none() && !p.force
        }
        ManageAction::Sync => {
            id.is_none()
                && task_ref.is_none()
                && paths.is_none()
                && !p.force
                && (milestone.is_some() || workspace.is_some())
        }
        ManageAction::Inspect | ManageAction::Context => {
            id.is_some()
                && milestone.is_none()
                && task_ref.is_none()
                && paths.is_none()
                && !p.force
                && workspace.is_none()
        }
        ManageAction::Delete => {
            id.is_some() != milestone.is_some()
                && task_ref.is_none()
                && paths.is_none()
                && (id.is_none() || workspace.is_none())
        }
        ManageAction::ExtendScope => {
            id.is_some()
                && milestone.is_none()
                && task_ref.is_none()
                && paths.is_some_and(|p| !p.is_empty())
                && !p.force
                && workspace.is_none()
        }
        ManageAction::SubtaskAdd => {
            id.is_some() && p.subtask_ref.is_some() && p.title.is_some() && !p.force
        }
        ManageAction::SubtaskUpdate => id.is_some() && p.subtask_ref.is_some() && !p.force,
        ManageAction::SubtaskList => id.is_some() && !p.force,
        ManageAction::Reset | ManageAction::Reopen => {
            id.is_some()
                && milestone.is_none()
                && task_ref.is_none()
                && paths.is_none()
                && !p.force
                && workspace.is_none()
        }
    };
    if !valid {
        bail!("TASK_SELECTOR_INVALID");
    }
    let registry = Registry::load(root)?;
    let (workspace, resolved_milestone) = if let Some(id) = id {
        (registry.owner(id)?, None)
    } else if let Some(m) = milestone {
        let (ws, resolved) = registry.resolve_milestone(workspace, m)?;
        (ws, Some(resolved))
    } else {
        (registry.select(workspace)?, None)
    };
    if let Some(resolved) = resolved_milestone {
        p.milestone_ref = Some(resolved);
    }
    let milestone = p.milestone_ref.as_deref();
    let mut store = workspace.open(&registry.database)?;
    match p.action {
        ManageAction::Create | ManageAction::Sync => {
            let references = if let Some(reference) = milestone {
                vec![reference.to_owned()]
            } else {
                workspace.manifests()?
            };
            let mut result = Vec::new();
            for reference in references {
                let rel_path = Path::new(&reference);
                let inferred = crate::core::tasks::infer_project_from_path(rel_path);
                let target_project = inferred.as_deref().unwrap_or(&workspace.project);
                let parsed = Milestone::parse_with_identity(
                    &std::fs::read_to_string(confined_path(&workspace.root, &reference)?)?,
                    &workspace.repository,
                    target_project,
                )?;
                if parsed.project != workspace.project {
                    if milestone.is_none() {
                        continue;
                    }
                    bail!("TASK_PROJECT_MISMATCH");
                }
                if parsed.status.as_deref() == Some("cancelled") {
                    store.prune_cancelled(&reference)?;
                    continue;
                }
                if task_ref.is_some_and(|r| !parsed.tasks.iter().any(|t| t.task_ref == r)) {
                    bail!("TASK_NOT_FOUND");
                }
                for spec in parsed
                    .tasks
                    .iter()
                    .filter(|spec| task_ref.is_none_or(|task_ref| spec.task_ref == task_ref))
                {
                    registry.check_scope(workspace, &spec.scope, None)?;
                }
                let tasks = if let Some(task_ref) = task_ref {
                    store.create(&parsed, &reference, &workspace.root, task_ref)?
                } else {
                    store.sync(&parsed, &reference, &workspace.root)?
                };
                for task in tasks {
                    result.push(workspace.envelope(serde_json::to_value(task)?)?);
                }
            }
            Ok(json!({"tasks":result}))
        }
        ManageAction::Inspect => {
            workspace.guidance_envelope(store.inspect_details(id.context("task_id required")?)?)
        }
        ManageAction::Context => {
            let task_id = id.context("task_id required")?;
            let details = workspace.guidance_envelope(store.inspect_details(task_id)?)?;
            context_bundle(root, workspace, task_id, details)
        }
        ManageAction::Delete => Ok(serde_json::to_value(store.delete(
            id,
            milestone,
            p.force,
            crate::db::tasks_store::now(),
        )?)?),
        ManageAction::ExtendScope => {
            let task = store.inspect(id.context("task_id required")?)?;
            registry.check_scope(
                workspace,
                paths.context("paths required")?,
                task.worktree.as_deref().map(Path::new),
            )?;
            Ok(serde_json::to_value(store.extend_scope(
                id.context("task_id required")?,
                paths.context("paths required")?,
                &workspace.root,
            )?)?)
        }
        ManageAction::SubtaskAdd => {
            let task_id = id.context("task_id required")?;
            let subtask_ref = p.subtask_ref.as_deref().context("subtask_ref required")?;
            let title = p.title.as_deref().context("title required")?;
            let subtask = store.subtask_add(task_id, subtask_ref, title)?;
            workspace.envelope(json!({
                "task_id": task_id,
                "subtask": subtask,
            }))
        }
        ManageAction::SubtaskUpdate => {
            let task_id = id.context("task_id required")?;
            let subtask_ref = p.subtask_ref.as_deref().context("subtask_ref required")?;
            let status = p.subtask_status.as_deref().unwrap_or("completed");
            let subtask =
                store.subtask_update(task_id, subtask_ref, status, p.evidence.as_deref())?;
            workspace.envelope(json!({
                "task_id": task_id,
                "subtask": subtask,
            }))
        }
        ManageAction::SubtaskList => {
            let task_id = id.context("task_id required")?;
            let subtasks = store.subtask_list(task_id)?;
            workspace.envelope(json!({
                "task_id": task_id,
                "subtasks": subtasks,
            }))
        }
        ManageAction::Reset | ManageAction::Reopen => {
            let task_id = id.context("task_id required")?;
            let task = reset(root, task_id)?;
            workspace.envelope(serde_json::to_value(task)?)
        }
    }
}
