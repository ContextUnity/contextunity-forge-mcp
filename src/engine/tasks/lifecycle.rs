use super::*;
use std::fs::{File, OpenOptions};

/// Performs claim.
pub fn claim(root: &Path, p: Claim) -> Result<Value> {
    let id = p.task_id.clone();
    claim_inner(root, p).map_err(|error| lifecycle_error(&id, error))
}

fn claim_inner(root: &Path, p: Claim) -> Result<Value> {
    let worktree = crate::core::tasks::claim_worktree(&p.worktree)?;
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    registry.check_worktree(workspace, &worktree)?;
    let mut store = workspace.open(&registry.database)?;
    let task = store.inspect(&p.task_id)?;
    validate_authority(&task, &worktree, &workspace.repository, &workspace.project)?;
    registry.check_scope(workspace, &task.spec.scope, Some(&worktree))?;
    let claimed = store.claim(&p.task_id, &p.stage, &p.worker_id, &p.worktree)?;
    finish_claim(root, workspace, &mut store, claimed, p.bundle)
}

fn finish_claim(
    root: &Path,
    workspace: &workspaces::Workspace,
    store: &mut TasksStore,
    claimed: Task,
    bundle: Option<bool>,
) -> Result<Value> {
    let id = &claimed.task_id;
    let worktree = Path::new(
        claimed
            .worktree
            .as_deref()
            .context("claim worktree missing")?,
    );
    let activation = (|| {
        let milestone_prefix = claimed
            .task_id
            .split_once(':')
            .context("invalid task identity")?
            .0
            .to_string()
            + ":";
        let claimed_at = store
            .earliest_claim(&milestone_prefix)?
            .context("task claim timestamp missing")?;
        crate::engine::milestones::activate_on_claim(worktree, &claimed.milestone_ref, claimed_at)
    })();
    if let Err(error) = activation {
        store
            .release_failed_claim(id, claimed.claim_revision)
            .context("failed to release claim after milestone activation error")?;
        return Err(error);
    }
    let response = (|| {
        let details = workspace.guidance_envelope(store.inspect_details(id)?)?;
        if bundle.unwrap_or(true) {
            context_bundle(root, workspace, id, details)
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
            .release_failed_claim(id, claimed.claim_revision)
            .context("failed to release claim after context error")?;
    } else {
        store.finish_resume(id, claimed.claim_revision)?;
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
    reset_inner(root, id).map_err(|error| lifecycle_error(id, error))
}

fn reset_inner(root: &Path, id: &str) -> Result<Task> {
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

fn manage_action_name(action: &ManageAction) -> &'static str {
    match action {
        ManageAction::Next => "next",
        ManageAction::Create => "create",
        ManageAction::Sync => "sync",
        ManageAction::Inspect => "inspect",
        ManageAction::Delete => "delete",
        ManageAction::ExtendScope => "extend_scope",
        ManageAction::SubtaskAdd => "subtask_add",
        ManageAction::SubtaskUpdate => "subtask_update",
        ManageAction::SubtaskList => "subtask_list",
        ManageAction::Reset => "reset",
        ManageAction::Reopen => "reopen",
        ManageAction::Context => "context",
    }
}

fn selector_invalid_fields(p: &Manage) -> Vec<&'static str> {
    let mut fields = Vec::new();
    let has_id = p
        .task_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_milestone = p
        .milestone_ref
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_task_ref = p
        .task_ref
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_workspace = p
        .workspace
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_paths = p.paths.as_ref().is_some_and(|paths| !paths.is_empty());
    let mut invalid = |field| {
        if !fields.contains(&field) {
            fields.push(field);
        }
    };
    if p.action != ManageAction::Next {
        if p.worker_id.is_some() {
            invalid("worker_id");
        }
        if p.worktree.is_some() {
            invalid("worktree");
        }
    }
    {
        match &p.action {
            ManageAction::Next => {
                if p.worker_id
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    invalid("worker_id");
                }
                if p.worktree
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    invalid("worktree");
                }
                for (field, supplied) in [
                    ("task_id", p.task_id.is_some()),
                    ("milestone_ref", p.milestone_ref.is_some()),
                    ("task_ref", p.task_ref.is_some()),
                    ("paths", p.paths.is_some()),
                    ("subtask_ref", p.subtask_ref.is_some()),
                    ("title", p.title.is_some()),
                    ("subtask_status", p.subtask_status.is_some()),
                    ("evidence", p.evidence.is_some()),
                    ("force", p.force),
                ] {
                    if supplied {
                        invalid(field);
                    }
                }
            }
            ManageAction::Create => {
                if p.task_id.is_some() {
                    invalid("task_id");
                }
                if !has_milestone {
                    invalid("milestone_ref");
                }
                if !has_task_ref {
                    invalid("task_ref");
                }
                if p.paths.is_some() {
                    invalid("paths");
                }
                if p.force {
                    invalid("force");
                }
            }
            ManageAction::Sync => {
                if p.task_id.is_some() {
                    invalid("task_id");
                }
                if p.task_ref.is_some() {
                    invalid("task_ref");
                }
                if p.paths.is_some() {
                    invalid("paths");
                }
                if p.force {
                    invalid("force");
                }
                if !has_milestone && !has_workspace {
                    invalid("milestone_ref");
                    invalid("workspace");
                } else {
                    if p.milestone_ref.is_some() && !has_milestone {
                        invalid("milestone_ref");
                    }
                    if p.workspace.is_some() && !has_workspace {
                        invalid("workspace");
                    }
                }
            }
            ManageAction::Inspect | ManageAction::Context => {
                if !has_id {
                    invalid("task_id");
                }
                if p.milestone_ref.is_some() {
                    invalid("milestone_ref");
                }
                if p.task_ref.is_some() {
                    invalid("task_ref");
                }
                if p.paths.is_some() {
                    invalid("paths");
                }
                if p.force {
                    invalid("force");
                }
                if p.workspace.is_some() {
                    invalid("workspace");
                }
            }
            ManageAction::Delete => {
                if p.task_id.is_some() == p.milestone_ref.is_some() {
                    invalid("task_id");
                    invalid("milestone_ref");
                }
                if p.task_id.is_some() && !has_id {
                    invalid("task_id");
                }
                if p.milestone_ref.is_some() && !has_milestone {
                    invalid("milestone_ref");
                }
                if p.task_ref.is_some() {
                    invalid("task_ref");
                }
                if p.paths.is_some() {
                    invalid("paths");
                }
                if p.task_id.is_some() && p.workspace.is_some() {
                    invalid("workspace");
                }
                if p.workspace.is_some() && !has_workspace {
                    invalid("workspace");
                }
            }
            ManageAction::ExtendScope => {
                if !has_id {
                    invalid("task_id");
                }
                if p.milestone_ref.is_some() {
                    invalid("milestone_ref");
                }
                if p.task_ref.is_some() {
                    invalid("task_ref");
                }
                if !has_paths {
                    invalid("paths");
                }
                if p.force {
                    invalid("force");
                }
                if p.workspace.is_some() {
                    invalid("workspace");
                }
            }
            ManageAction::SubtaskAdd => {
                if !has_id {
                    invalid("task_id");
                }
                if p.subtask_ref
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    invalid("subtask_ref");
                }
                if p.title
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    invalid("title");
                }
                if p.force {
                    invalid("force");
                }
            }
            ManageAction::SubtaskUpdate => {
                if !has_id {
                    invalid("task_id");
                }
                if p.subtask_ref
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    invalid("subtask_ref");
                }
                if p.force {
                    invalid("force");
                }
            }
            ManageAction::SubtaskList => {
                if !has_id {
                    invalid("task_id");
                }
                if p.force {
                    invalid("force");
                }
            }
            ManageAction::Reset | ManageAction::Reopen => {
                if !has_id {
                    invalid("task_id");
                }
                if p.milestone_ref.is_some() {
                    invalid("milestone_ref");
                }
                if p.task_ref.is_some() {
                    invalid("task_ref");
                }
                if p.paths.is_some() {
                    invalid("paths");
                }
                if p.force {
                    invalid("force");
                }
                if p.workspace.is_some() {
                    invalid("workspace");
                }
            }
        }
    }
    fields
}

/// Performs manage.
pub fn manage(root: &Path, mut p: Manage) -> Result<Value> {
    let id = p
        .task_id
        .clone()
        .or_else(|| p.milestone_ref.clone())
        .unwrap_or_else(|| "unselected".into());
    manage_inner(root, &mut p).map_err(|error| lifecycle_error(&id, error))
}

fn manage_inner(root: &Path, p: &mut Manage) -> Result<Value> {
    let id = p.task_id.as_deref();
    let milestone = p.milestone_ref.as_deref();
    let task_ref = p.task_ref.as_deref();
    let paths = p.paths.as_deref();
    let workspace = p.workspace.as_deref();
    let invalid_fields = selector_invalid_fields(p);
    if !invalid_fields.is_empty() {
        let action = manage_action_name(&p.action);
        bail!("TASK_SELECTOR_INVALID: action '{action}'. missing or invalid fields: [{}]. Do: pass required fields for '{action}'.", invalid_fields.join(", "));
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
        ManageAction::Next => {
            let worker = p.worker_id.as_deref().context("worker_id required")?;
            let worktree = p.worktree.as_deref().context("worktree required")?;
            let claimed = store.next(worker, worktree, |task, selected| {
                registry.check_worktree(workspace, selected)?;
                validate_authority(task, selected, &workspace.repository, &workspace.project)?;
                registry.check_scope(workspace, &task.spec.scope, Some(selected))
            })?;
            match claimed {
                Some(claimed) => finish_claim(root, workspace, &mut store, claimed, Some(true)),
                None => workspace.envelope(json!({"status":"idle","worker_id":worker})),
            }
        }
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
                    let task_id = format!(
                        "{}/{}:{}",
                        parsed.project,
                        parsed.id,
                        task_ref.unwrap_or_default()
                    );
                    bail!(
                        "{}",
                        crate::db::tasks_store::task_not_found_message(&task_id)
                    );
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
