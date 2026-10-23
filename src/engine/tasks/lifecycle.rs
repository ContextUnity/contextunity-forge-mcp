use super::*;

// JSON is embedded in an MCP text block and escaped a second time. Keeping
// task JSON below 24 KiB leaves room for that expansion and protocol metadata.
const TASK_RESPONSE_BYTES: usize = 24 * 1024;

fn task_response_bytes(value: &Value) -> usize {
    value.to_string().len()
}

fn bound_task_response(mut value: Value) -> Value {
    if task_response_bytes(&value) <= TASK_RESPONSE_BYTES {
        return value;
    }
    // Remove whole fields, never partial authority text. Preserve claim identity
    // and proof templates ahead of optional context, including duplicate copies.
    fn largest_field(value: &Value, prefix: &str) -> Option<(bool, usize, String)> {
        value
            .as_object()?
            .iter()
            .filter_map(|(key, child)| {
                let path = format!("{prefix}/{}", key.replace('~', "~0").replace('/', "~1"));
                if matches!(
                    key.as_str(),
                    "context_bundle"
                        | "contract"
                        | "guidance"
                        | "workflow_guidance"
                        | "authority_snapshot"
                ) && child.as_object().is_some_and(|object| !object.is_empty())
                {
                    return largest_field(child, &path);
                }
                let optional = !matches!(
                    key.as_str(),
                    "task_id"
                        | "stage"
                        | "status"
                        | "claim_revision"
                        | "contract_revision"
                        | "worker_id"
                        | "worktree"
                        | "expected_evidence_template"
                        | "contract_seam_test"
                        | "candidate_snapshot"
                        | "latest_snapshot"
                        | "proof_schema"
                );
                Some((optional, task_response_bytes(child) + key.len(), path))
            })
            .max()
    }
    let mut omitted = Vec::new();
    let mut omitted_count = 0;
    while task_response_bytes(&value) > TASK_RESPONSE_BYTES - 4096 {
        let Some((_, _, path)) = largest_field(&value, "") else {
            break;
        };
        let (parent, field) = path.rsplit_once('/').expect("field pointer");
        let field = field.replace("~1", "/").replace("~0", "~");
        value
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .expect("field object")
            .remove(&field);
        omitted_count += 1;
        if omitted.len() < 16 && serde_json::to_string(&path).expect("JSON pointer").len() <= 128 {
            omitted.push(path);
        }
    }
    value["byte_limited"] = json!(true);
    value["omitted_fields"] = json!(omitted);
    value["omitted_field_count"] = json!(omitted_count);
    value["retrieval_hint"] = json!("Context is incomplete. Read the milestone file for full authority and use CLI task inspect <task_id> for full stored details; use task_blackboard inspect for message payloads. Do not infer missing authority from this summary.");
    value
}

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
    store.lock_checkpoint()?;
    let task = store.inspect(&p.task_id)?;
    validate_authority(&task, &worktree, workspace, &registry)?;
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
        if response
            .as_ref()
            .err()
            .is_some_and(crate::db::tasks_store::recovery::is_pending)
        {
            return response;
        }
        store
            .release_failed_claim(id, claimed.claim_revision)
            .context("failed to release claim after context error")?;
    } else {
        store.finish_resume(id, claimed.claim_revision)?;
    }
    response.map(bound_task_response)
}
/// Resets or reopens a task back to ready state at contract gate, clearing terminal receipt and updating milestone text if needed.
pub fn reset(root: &Path, id: &str) -> Result<Task> {
    reset_inner(root, id).map_err(|error| lifecycle_error(id, error))
}

fn reset_inner(root: &Path, id: &str) -> Result<Task> {
    let registry = Registry::load(root)?;
    let workspace = registry.owner(id)?;
    let mut store = workspace.open(&registry.database)?;
    store.lock_checkpoint()?;
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
    match store.reset_with_pre_persist(id, || {
        crate::core::fs::atomic_replace(&milestone_path, updated.as_bytes())
    }) {
        Ok(task) => Ok(task),
        Err(error) => {
            if crate::db::tasks_store::recovery::is_pending(&error) {
                return Err(error);
            }
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
    let _observation = if matches!(
        p.action,
        ManageAction::Inspect | ManageAction::Context | ManageAction::SubtaskList
    ) {
        read_checkpoint_fence(root)?
    } else {
        None
    };
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
            store.lock_checkpoint()?;
            let worker = p.worker_id.as_deref().context("worker_id required")?;
            let worktree = p.worktree.as_deref().context("worktree required")?;
            let claimed = store.next(worker, worktree, |task, selected| {
                registry.check_worktree(workspace, selected)?;
                validate_authority(task, selected, workspace, &registry)?;
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
            let mut total = 0usize;
            let mut returned_bytes = 0usize;
            for reference in references {
                if crate::engine::milestones::is_archived_path(Path::new(&reference)) {
                    if milestone.is_none() {
                        continue;
                    }
                    bail!("STORE_SYNC_FAILED: milestone '{reference}' is archived");
                }
                let full_path = confined_path(&workspace.root, &reference)?;
                let text = std::fs::read_to_string(full_path)?;
                let parsed = registry
                    .parse_milestone_manifest(workspace, &reference, &text)
                    .with_context(|| format!("invalid milestone manifest '{reference}'"))?;
                if parsed.project != workspace.project {
                    if milestone.is_none() {
                        continue;
                    }
                    bail!("TASK_PROJECT_MISMATCH");
                }
                if store.milestone_tombstoned(&parsed.id)? {
                    if milestone.is_none() {
                        continue;
                    }
                    bail!("STORE_SYNC_FAILED: milestone '{}' is archived", parsed.id);
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
                let admission = if let Some(task_ref) = task_ref {
                    store.create(&parsed, &reference, &workspace.root, task_ref)
                } else {
                    store.sync(&parsed, &reference, &workspace.root)
                };
                let tasks = match admission {
                    Err(error)
                        if milestone.is_none()
                            && error
                                .downcast_ref::<crate::db::tasks_store::MilestoneArchived>()
                                .is_some() =>
                    {
                        continue;
                    }
                    result => result?,
                };
                for task in tasks {
                    total += 1;
                    let summary = workspace.envelope(json!({
                        "task_id": task.task_id,
                        "spec": {"task_ref": task.spec.task_ref},
                        "milestone_ref": task.milestone_ref,
                        "status": task.status,
                        "stage": task.stage,
                        "claim_revision": task.claim_revision,
                        "contract_revision": task.contract_revision,
                        "wave_index": task.wave_index,
                    }))?;
                    let bytes = task_response_bytes(&summary) + 1;
                    if result.len() == total - 1
                        && returned_bytes + bytes <= TASK_RESPONSE_BYTES - 1024
                    {
                        returned_bytes += bytes;
                        result.push(summary);
                    }
                }
            }
            let returned = result.len();
            Ok(json!({"tasks":result,"total":total,"returned":returned,
                "byte_limited":returned < total,
                "retrieval_hint":"All selected tasks were synchronized. Use task_list or CLI task list for task summaries and CLI task inspect <task_id> for full details."}))
        }
        ManageAction::Inspect => workspace
            .guidance_envelope(checkpoint_details(&store, id.context("task_id required")?)?),
        ManageAction::Context => {
            let task_id = id.context("task_id required")?;
            let details = workspace.guidance_envelope(checkpoint_details(&store, task_id)?)?;
            context_bundle(root, workspace, task_id, details).map(bound_task_response)
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

// Runtime health composes Git/files with SQL under manage's observation fence.
// The store's inspect_details remains a persisted SQL view.
fn checkpoint_details(store: &TasksStore, id: &str) -> Result<Value> {
    let mut details = store.inspect_details(id)?;
    let task = store.inspect(id)?;
    details["checkpoint_status"] = json!(if store.checkpoint_enrolled(&task)? {
        "ENROLLED"
    } else {
        "UNENROLLED"
    });
    Ok(details)
}
