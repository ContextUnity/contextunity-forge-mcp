use super::*;

// JSON is embedded in an MCP text block and escaped a second time. Keeping
// task JSON below 24 KiB leaves room for that expansion and protocol metadata.
const TASK_RESPONSE_BYTES: usize = 24 * 1024;

fn task_response_bytes(value: &Value) -> usize {
    value.to_string().len()
}

fn render_scope_extension_manifest(
    text: &str,
    task_ref: &str,
    scope: &[String],
    extended_scope: &[String],
) -> Result<String> {
    let mut block_start = None;
    let mut cursor = 0;
    for line in text.split_inclusive('\n') {
        if line.trim() == "```yaml" {
            block_start = Some(cursor + line.len());
        } else if line.trim() == "```" {
            if let Some(start) = block_start.take() {
                let mut block: serde_yaml::Value = serde_yaml::from_str(&text[start..cursor])?;
                if block["task_ref"].as_str() == Some(task_ref) {
                    let map = block
                        .as_mapping_mut()
                        .context("TASK_SCOPE_MANIFEST_INVALID: task block must be a mapping")?;
                    map.insert(
                        serde_yaml::Value::String("scope".into()),
                        serde_yaml::to_value(scope)?,
                    );
                    let extended_scope_key = serde_yaml::Value::String("extended_scope".into());
                    if extended_scope.is_empty() {
                        map.remove(&extended_scope_key);
                    } else {
                        map.insert(extended_scope_key, serde_yaml::to_value(extended_scope)?);
                    }
                    let yaml = serde_yaml::to_string(&block)?;
                    return Ok(format!("{}{}{}", &text[..start], yaml, &text[cursor..]));
                }
            }
        }
        cursor += line.len();
    }
    bail!("TASK_SCOPE_MANIFEST_INVALID: task block '{task_ref}' not found")
}

fn bound_task_response(mut value: Value) -> Value {
    if task_response_bytes(&value) <= TASK_RESPONSE_BYTES {
        return value;
    }
    // Remove whole fields, never partial authority text. Preserve claim identity
    // and proof templates ahead of optional context, including duplicate copies.
    fn largest_field(value: &Value, prefix: &str) -> Option<(u8, usize, String)> {
        value
            .as_object()?
            .iter()
            .filter_map(|(key, child)| {
                let path = format!("{prefix}/{}", key.replace('~', "~0").replace('/', "~1"));
                if matches!(
                    key.as_str(),
                    "context_bundle" | "contract" | "guidance" | "workflow_guidance"
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
                let priority_tier = if path.starts_with("/context_bundle/guidance/")
                    || matches!(
                        path.as_str(),
                        "/context_bundle/blackboard"
                            | "/context_bundle/blackboard_info"
                            | "/context_bundle/linked_milestones"
                            | "/workflow_guidance/linked_milestones"
                            | "/workflow_guidance/actionable_steps"
                            | "/authority_snapshot"
                    ) {
                    3
                } else if matches!(
                    path.as_str(),
                    "/context_bundle/scope_symbols"
                        | "/context_bundle/covering_tests"
                        | "/context_bundle/external_callers"
                ) {
                    2
                } else {
                    u8::from(optional)
                };
                Some((priority_tier, task_response_bytes(child) + key.len(), path))
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
    let worktree = crate::tasks::claim_worktree(&p.worktree)?;
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    registry.check_worktree(workspace, &worktree)?;
    let mut store = workspace.open(&registry.database)?;
    store.lock_checkpoint()?;
    let mut task = store.inspect(&p.task_id)?;
    validate_authority(&task, &worktree, workspace, &registry)?;
    registry.check_scope(workspace, &task.spec.scope, Some(&worktree))?;
    context::validate_claim_gate_requirements(workspace, &task)?;
    let profile = store.profile_for_task(&task)?;
    task.worktree = Some(worktree.to_string_lossy().into_owned());
    let cursor = super::claim::claim_nested_gate_for_task(&profile, &task.stage, &p.stage, &task)?;
    let claimed = store.claim(
        &p.task_id,
        &cursor.canonical_stage_id,
        &p.worker_id,
        &p.worktree,
        p.session_fingerprint.as_deref(),
    )?;
    finish_claim(root, workspace, &mut store, claimed, cursor, p.bundle)
}

fn finish_claim(
    root: &Path,
    workspace: &workspaces::Workspace,
    store: &mut TasksStore,
    claimed: Task,
    cursor: super::claim::GateCursor,
    bundle: Option<bool>,
) -> Result<Value> {
    let id = &claimed.task_id;
    let worktree = Path::new(
        claimed
            .worktree
            .as_deref()
            .context("claim worktree missing")?,
    );
    let (milestone_id, task_breadcrumb) = super::claim::breadcrumb(id, &cursor)?;
    let namespace_prefix = format!("{}/{}/", workspace.repository, workspace.project);
    let checkpoint_milestone_id = format!("{namespace_prefix}{milestone_id}");
    let breadcrumb = format!("{namespace_prefix}{task_breadcrumb}");
    let checkpoint_subject = id
        .rsplit_once(':')
        .map(|(_, task_ref)| task_ref)
        .context("TASK_BREADCRUMB_INVALID: task reference is missing")?;
    let subagent_role = super::claim::resolve_subagent_role(&cursor);
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
    if cursor.gate.persistence.pre == Some(crate::tasks::profile::PersistenceAction::Commit) {
        let pre_commit = (|| -> Result<()> {
            let task_spec = store.inspect(id)?;
            let msg = format!(
                "pre({}): claim {}",
                cursor.canonical_stage_id, claimed.task_id
            );
            crate::git::git_scoped_commit(worktree, &task_spec.spec.scope, &msg)?;
            Ok(())
        })();
        if let Err(error) = pre_commit {
            store.release_failed_claim(id, claimed.claim_revision).ok();
            return Err(error);
        }
    }
    let (pre_snapshot_commit, pre_candidate_baseline) = if cursor.gate.persistence.pre
        == Some(crate::tasks::profile::PersistenceAction::Snapshot)
    {
        let pre_snapshot = (|| -> Result<(String, String)> {
            let task_spec = store.inspect(id)?;
            let (snapshot, baseline, mut objects) =
                crate::engine::git_snapshot::capture_scoped_snapshot(
                    worktree,
                    id,
                    &task_spec.spec.scope,
                )?
                .context("TASK_SNAPSHOT_FAILED: pre-snapshot capture requires a Git worktree")?;
            objects.retain_pending();
            Ok((snapshot, baseline))
        })();
        match pre_snapshot {
            Ok((s, b)) => (Some(s), Some(b)),
            Err(error) => {
                store.release_failed_claim(id, claimed.claim_revision).ok();
                return Err(error);
            }
        }
    } else {
        (None, None)
    };
    let response = (|| {
        let mut details = workspace.guidance_envelope(store.inspect_details(id)?)?;
        if let Some(object) = details.as_object_mut() {
            object.insert("breadcrumb".into(), json!(breadcrumb));
            object.insert("active_gate_path".into(), json!(cursor.gate_path.clone()));
            object.insert(
                "active_stage_id".into(),
                json!(cursor.canonical_stage_id.clone()),
            );
            object.insert("subagent_role".into(), json!(subagent_role));
        }
        let details = context::adapt_workspace_guidance(workspace, details)?;
        let result = if bundle.unwrap_or(true) {
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
        };
        let output = result?;
        if worktree.join(".git").exists() {
            let git_tree_sha = crate::git::git_stdout(
                worktree,
                &["rev-parse", "--verify", "HEAD^{tree}"],
                "TASK_CHECKPOINT_STATE_INVALID",
            )?;
            store.save_checkpoint(&GateCheckpoint {
                breadcrumb: breadcrumb.clone(),
                milestone_id: checkpoint_milestone_id,
                subject: checkpoint_subject.to_owned(),
                gate_path: cursor.gate_path.clone(),
                transition_id: None,
                git_tree_sha,
                state_payload: {
                    let mut payload = json!({
                        "task_id": id,
                        "stage": cursor.stage_id.clone(),
                        "gate_path": cursor.gate_path.clone(),
                        "worker_id": claimed.worker_id.clone(),
                        "claim_revision": claimed.claim_revision,
                        "contract_revision": claimed.contract_revision,
                        "worktree": claimed.worktree.clone(),
                        "subagent_role": subagent_role.clone(),
                    });
                    if let (Some(commit), Some(baseline)) =
                        (&pre_snapshot_commit, &pre_candidate_baseline)
                    {
                        if let Some(obj) = payload.as_object_mut() {
                            obj.insert("commit".into(), json!(commit));
                            obj.insert("candidate_baseline_head".into(), json!(baseline));
                        }
                    }
                    payload
                },
            })?;
        }
        Ok(output)
    })();
    if response.is_err() {
        if response
            .as_ref()
            .err()
            .is_some_and(crate::store::recovery::is_pending)
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
            if crate::store::recovery::is_pending(&error) {
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
        ManageAction::Verify => "verify",
        ManageAction::Next => "next",
        ManageAction::Create => "create",
        ManageAction::Sync => "sync",
        ManageAction::Inspect => "inspect",
        ManageAction::Delete => "delete",
        ManageAction::Snapshot => "snapshot",
        ManageAction::Rollback => "rollback",
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
    if p.action != ManageAction::Verify {
        if p.timeout_seconds.is_some() {
            invalid("timeout_seconds");
        }
        if p.test.is_some() {
            invalid("test");
        }
        if p.seam.is_some() {
            invalid("seam");
        }
    }
    if p.action != ManageAction::Next {
        if p.worker_id.is_some() {
            invalid("worker_id");
        }
        if p.worktree.is_some() {
            invalid("worktree");
        }
    }
    if !matches!(p.action, ManageAction::Snapshot | ManageAction::Rollback) && p.name.is_some() {
        invalid("name");
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
            ManageAction::Inspect | ManageAction::Context | ManageAction::Verify => {
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
            ManageAction::Snapshot | ManageAction::Rollback => {
                if p.name
                    .as_deref()
                    .is_none_or(|name| crate::tasks::valid_identity(name).is_err())
                {
                    invalid("name");
                }
                for (field, supplied) in [
                    ("task_id", p.task_id.is_some()),
                    ("milestone_ref", p.milestone_ref.is_some()),
                    ("task_ref", p.task_ref.is_some()),
                    ("paths", p.paths.is_some()),
                    ("force", p.force),
                    ("subtask_ref", p.subtask_ref.is_some()),
                    ("title", p.title.is_some()),
                    ("subtask_status", p.subtask_status.is_some()),
                    ("evidence", p.evidence.is_some()),
                ] {
                    if supplied {
                        invalid(field);
                    }
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
        ManageAction::Verify => super::verify_task_with_options(
            root,
            id.context("task_id required")?,
            p.test.as_deref(),
            p.seam.as_deref(),
            p.timeout_seconds,
        ),
        ManageAction::Next => {
            store.lock_checkpoint()?;
            let worker = p.worker_id.as_deref().context("worker_id required")?;
            let worktree = p.worktree.as_deref().context("worktree required")?;
            let claimed = store.next(
                worker,
                worktree,
                p.session_fingerprint.as_deref(),
                |task, selected| {
                    registry.check_worktree(workspace, selected)?;
                    validate_authority(task, selected, workspace, &registry)?;
                    registry.check_scope(workspace, &task.spec.scope, Some(selected))?;
                    context::validate_claim_gate_requirements(workspace, task)
                },
            )?;
            match claimed {
                Some(claimed) => {
                    let profile = store.profile_for_task(&claimed)?;
                    let cursor = super::claim::claim_nested_gate_for_task(
                        &profile,
                        &claimed.stage,
                        "",
                        &claimed,
                    )?;
                    finish_claim(root, workspace, &mut store, claimed, cursor, Some(true))
                }
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
                    bail!("{}", crate::store::task_not_found_message(&task_id));
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
                                .downcast_ref::<crate::store::MilestoneArchived>()
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
        ManageAction::Inspect => {
            let details = workspace
                .guidance_envelope(checkpoint_details(&store, id.context("task_id required")?)?)?;
            context::adapt_workspace_guidance(workspace, details)
        }
        ManageAction::Context => {
            let task_id = id.context("task_id required")?;
            let details = workspace.guidance_envelope(checkpoint_details(&store, task_id)?)?;
            context_bundle(root, workspace, task_id, details).map(bound_task_response)
        }
        ManageAction::Delete => Ok(serde_json::to_value(store.delete(
            id,
            milestone,
            p.force,
            crate::store::now(),
        )?)?),
        ManageAction::Snapshot => {
            drop(store);
            let value = super::git_snapshot::create_named_snapshot(
                &workspace.root,
                &registry.database,
                p.name.as_deref().context("name required")?,
            )?;
            workspace.envelope(value)
        }
        ManageAction::Rollback => {
            drop(store);
            let value = super::git_snapshot::rollback_named_snapshot(
                &workspace.root,
                &registry.database,
                p.name.as_deref().context("name required")?,
            )?;
            workspace.envelope(value)
        }
        ManageAction::ExtendScope => {
            let task_id = id.context("task_id required")?;
            let paths = paths.context("paths required")?;
            let task = store.inspect(task_id)?;
            registry.check_scope(workspace, paths, task.worktree.as_deref().map(Path::new))?;
            let milestone_ref = task.milestone_ref.clone();
            let milestone_path = confined_path(&workspace.root, &milestone_ref)?;
            let original_manifest = std::fs::read_to_string(&milestone_path)?;
            let task_ref = task.spec.task_ref.clone();
            Ok(serde_json::to_value(store.extend_scope_with_pre_commit(
                task_id,
                paths,
                &workspace.root,
                |updated_task| {
                    let updated_manifest = render_scope_extension_manifest(
                        &original_manifest,
                        &task_ref,
                        &updated_task.spec.scope,
                        &updated_task.spec.extended_scope,
                    )?;
                    let milestone = registry.parse_milestone_manifest(
                        workspace,
                        &milestone_ref,
                        &updated_manifest,
                    )?;
                    let manifest_task = milestone
                        .tasks
                        .iter()
                        .find(|spec| spec.task_ref == task_ref)
                        .context("TASK_SCOPE_MANIFEST_INVALID: extended task disappeared")?;
                    if manifest_task.scope != updated_task.spec.scope
                        || manifest_task.extended_scope != updated_task.spec.extended_scope
                    {
                        bail!(
                            "TASK_SCOPE_MANIFEST_INVALID: parsed scope differs from SQLite scope"
                        );
                    }
                    updated_task.digest = milestone.digest(manifest_task)?;
                    updated_task.authority_snapshot = Some(
                        crate::tasks::AuthoritySnapshot::capture(&milestone, manifest_task),
                    );
                    Ok(Some(crate::store::recovery::ManifestReplacement::new(
                        &workspace.root,
                        &milestone_ref,
                        original_manifest.as_bytes(),
                        updated_manifest.as_bytes(),
                    )?))
                },
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
