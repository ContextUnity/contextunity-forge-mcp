use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
/// Operation to perform on the task blackboard.
pub enum BlackboardAction {
    /// Post one message.
    Post,
    /// Read a bounded page of message summaries.
    Read,
    /// Inspect one message including its payload.
    Inspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
/// Hierarchy level selected for blackboard operations.
pub enum BlackboardScope {
    /// Messages attached directly to a milestone.
    Milestone,
    /// Messages attached directly to a task.
    Task,
    /// Messages attached to one subtask.
    Subtask,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Parameters for reading, posting, or inspecting blackboard messages.
pub struct BlackboardRequest {
    /// Operation to perform: post, read, or inspect.
    pub action: BlackboardAction,
    /// Optional hierarchy scope; omitted keys resolve to one unique active context.
    pub scope: Option<BlackboardScope>,
    /// Milestone reference used by milestone scope or as a context filter.
    pub milestone_ref: Option<String>,
    /// Task identifier used by task and subtask scopes.
    pub task_id: Option<String>,
    /// Subtask reference used by subtask scope.
    pub subtask_ref: Option<String>,
    /// Message identifier required by inspect.
    pub message_id: Option<u64>,
    /// Posting worker identifier. Defaults to the active task owner or transport.
    pub author: Option<String>,
    /// Canonical topic: draft, notes, findings, blockers, decisions, or deferred; required for post.
    pub topic: Option<String>,
    /// Optional active-profile gate target for posts and gate filter for reads.
    #[serde(default)]
    pub gate: Option<String>,
    /// Text or serialized JSON message; required for post.
    pub payload: Option<String>,
    /// Maximum number of messages for read (default 10, maximum 50).
    pub limit: Option<usize>,
    /// Number of matching messages to skip for read pagination.
    pub offset: Option<usize>,
}

struct ResolvedBlackboardContext {
    store: TasksStore,
    profile: std::sync::Arc<crate::core::tasks::profile::GateProfile>,
    milestone_ref: String,
    task_id: Option<String>,
    subtask_ref: Option<String>,
}

fn active_milestones(registry: &Registry) -> Result<Vec<(&workspaces::Workspace, String)>> {
    let mut active = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for workspace in registry.selected(Some("all"))? {
        for reference in workspace.manifests()? {
            if resolve_manifest_status(&workspace.root, &reference)? == "active"
                && seen.insert((
                    workspace.repository.clone(),
                    workspace.project.clone(),
                    reference.clone(),
                ))
            {
                active.push((workspace, reference));
            }
        }
    }
    active.sort_by(|(left_workspace, left_ref), (right_workspace, right_ref)| {
        (
            &left_workspace.repository,
            &left_workspace.project,
            left_ref,
        )
            .cmp(&(
                &right_workspace.repository,
                &right_workspace.project,
                right_ref,
            ))
    });
    Ok(active)
}

fn in_progress_tasks<'a>(
    registry: &'a Registry,
    filter: Option<(&workspaces::Workspace, &str)>,
) -> Result<Vec<(&'a workspaces::Workspace, Task)>> {
    let mut tasks = Vec::new();
    for workspace in registry.selected(Some("all"))? {
        if filter.is_some_and(|(target, _)| {
            workspace.repository != target.repository || workspace.project != target.project
        }) {
            continue;
        }
        let store = workspace.open(&registry.database)?;
        let milestone = filter.map(|(_, reference)| reference);
        tasks.extend(
            store
                .list(milestone, "in_progress", None)?
                .into_iter()
                .map(|task| (workspace, task)),
        );
    }
    Ok(tasks)
}

fn unique_in_progress_task<'a>(
    registry: &'a Registry,
    filter: Option<(&workspaces::Workspace, &str)>,
) -> Result<(&'a workspaces::Workspace, Task)> {
    let mut tasks = in_progress_tasks(registry, filter)?;
    match tasks.len() {
        0 => bail!("TASK_BLACKBOARD_NO_ACTIVE_TASK"),
        1 => tasks.pop().context("active task missing"),
        _ => bail!("TASK_BLACKBOARD_AMBIGUOUS_TASK"),
    }
}

fn explicit_milestone<'a>(
    registry: &'a Registry,
    reference: &str,
) -> Result<(&'a workspaces::Workspace, String)> {
    registry.resolve_milestone(None, reference)
}

fn active_milestone(registry: &Registry) -> Result<(&workspaces::Workspace, String)> {
    let mut milestones = active_milestones(registry)?;
    match milestones.len() {
        0 => bail!("TASK_BLACKBOARD_NO_ACTIVE_CONTEXT"),
        1 => milestones.pop().context("active milestone missing"),
        _ => bail!("TASK_BLACKBOARD_AMBIGUOUS_MILESTONE"),
    }
}

fn resolved_context_for_milestone(
    registry: &Registry,
    workspace: &workspaces::Workspace,
    reference: &str,
) -> Result<ResolvedBlackboardContext> {
    let store = workspace.open(&registry.database)?;
    let milestone_ref = store.milestone_scope_ref(reference)?;
    let full_path = confined_path(&workspace.root, reference)?;
    let text = std::fs::read_to_string(full_path)
        .with_context(|| format!("unable to read milestone manifest '{reference}'"))?;
    let inferred = crate::core::tasks::infer_project_from_path(Path::new(reference));
    let milestone = Milestone::parse_with_identity(
        &text,
        &workspace.repository,
        inferred.as_deref().unwrap_or(&workspace.project),
    )
    .with_context(|| format!("invalid milestone manifest '{reference}'"))?;
    let profile = match milestone.acdd_profile.as_deref() {
        Some(pin) => {
            crate::core::tasks::profile::load_pinned_over(&workspace.root, pin, &workspace.gates)?
        }
        None => workspace.gates.clone(),
    };
    Ok(ResolvedBlackboardContext {
        store,
        profile,
        milestone_ref,
        task_id: None,
        subtask_ref: None,
    })
}

fn task_matches_milestone(
    registry: &Registry,
    task_workspace: &workspaces::Workspace,
    task: &Task,
    reference: &str,
) -> Result<()> {
    let (milestone_workspace, milestone_reference) = explicit_milestone(registry, reference)?;
    if milestone_workspace.repository != task_workspace.repository
        || milestone_workspace.project != task_workspace.project
        || milestone_reference != task.milestone_ref
    {
        bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
    }
    Ok(())
}

fn resolve_blackboard_context(
    registry: &Registry,
    request: &BlackboardRequest,
) -> Result<ResolvedBlackboardContext> {
    let inferred_scope = request.scope.or_else(|| {
        if request.subtask_ref.is_some() {
            Some(BlackboardScope::Subtask)
        } else if request.task_id.is_some() {
            Some(BlackboardScope::Task)
        } else if request.milestone_ref.is_some() {
            Some(BlackboardScope::Milestone)
        } else {
            None
        }
    });

    match inferred_scope {
        None => {
            let mut tasks = in_progress_tasks(registry, None)?;
            match tasks.len() {
                1 => {
                    let (workspace, task) = tasks.pop().context("active task missing")?;
                    let store = workspace.open(&registry.database)?;
                    let milestone_ref = store.milestone_scope_ref(&task.milestone_ref)?;
                    let profile = store.profile.clone();
                    Ok(ResolvedBlackboardContext {
                        store,
                        profile,
                        milestone_ref,
                        task_id: Some(task.task_id),
                        subtask_ref: None,
                    })
                }
                count if count > 1 => bail!("TASK_BLACKBOARD_AMBIGUOUS_TASK"),
                _ => {
                    let (workspace, reference) = active_milestone(registry)?;
                    resolved_context_for_milestone(registry, workspace, &reference)
                }
            }
        }
        Some(BlackboardScope::Milestone) => {
            if request.task_id.is_some() || request.subtask_ref.is_some() {
                bail!("TASK_BLACKBOARD_SCOPE_INVALID: milestone scope does not accept task keys");
            }
            let (workspace, reference) = match request.milestone_ref.as_deref() {
                Some(reference) => explicit_milestone(registry, reference)?,
                None => active_milestone(registry)?,
            };
            resolved_context_for_milestone(registry, workspace, &reference)
        }
        Some(BlackboardScope::Task) => {
            if request.subtask_ref.is_some() {
                bail!("TASK_BLACKBOARD_SCOPE_INVALID: task scope does not accept subtask_ref");
            }
            let (workspace, task) = match request.task_id.as_deref() {
                Some(task_id) => {
                    let workspace = registry.owner(task_id)?;
                    let store = workspace.open(&registry.database)?;
                    let task = store.inspect(task_id)?;
                    (workspace, task)
                }
                None => {
                    let filter = request
                        .milestone_ref
                        .as_deref()
                        .map(|reference| explicit_milestone(registry, reference))
                        .transpose()?;
                    unique_in_progress_task(
                        registry,
                        filter
                            .as_ref()
                            .map(|(workspace, reference)| (*workspace, reference.as_str())),
                    )?
                }
            };
            if let Some(reference) = request.milestone_ref.as_deref() {
                task_matches_milestone(registry, workspace, &task, reference)?;
            }
            let store = workspace.open(&registry.database)?;
            let milestone_ref = store.milestone_scope_ref(&task.milestone_ref)?;
            let profile = store.profile.clone();
            Ok(ResolvedBlackboardContext {
                store,
                profile,
                milestone_ref,
                task_id: Some(task.task_id),
                subtask_ref: None,
            })
        }
        Some(BlackboardScope::Subtask) => {
            let filter = request
                .milestone_ref
                .as_deref()
                .map(|reference| explicit_milestone(registry, reference))
                .transpose()?;
            let (workspace, task) = match request.task_id.as_deref() {
                Some(task_id) => {
                    let workspace = registry.owner(task_id)?;
                    let store = workspace.open(&registry.database)?;
                    let task = store.inspect(task_id)?;
                    (workspace, task)
                }
                None => unique_in_progress_task(
                    registry,
                    filter
                        .as_ref()
                        .map(|(workspace, reference)| (*workspace, reference.as_str())),
                )?,
            };
            if let Some(reference) = request.milestone_ref.as_deref() {
                task_matches_milestone(registry, workspace, &task, reference)?;
            }
            let subtask_ref = match request.subtask_ref.clone() {
                Some(reference) if !reference.trim().is_empty() => reference,
                Some(_) => bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED"),
                None => {
                    let mut subtasks = task
                        .spec
                        .subtasks
                        .iter()
                        .filter(|subtask| subtask.status == "in_progress")
                        .map(|subtask| subtask.subtask_ref.clone());
                    let first = subtasks
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("TASK_BLACKBOARD_SUBTASK_REQUIRED"))?;
                    if subtasks.next().is_some() {
                        bail!("TASK_BLACKBOARD_AMBIGUOUS_SUBTASK");
                    }
                    first
                }
            };
            let store = workspace.open(&registry.database)?;
            let milestone_ref = store.milestone_scope_ref(&task.milestone_ref)?;
            let profile = store.profile.clone();
            Ok(ResolvedBlackboardContext {
                store,
                profile,
                milestone_ref,
                task_id: Some(task.task_id),
                subtask_ref: Some(subtask_ref),
            })
        }
    }
}

fn validate_blackboard_request(request: &BlackboardRequest) -> Result<()> {
    match request.action {
        BlackboardAction::Post => {
            if request.message_id.is_some() || request.limit.is_some() || request.offset.is_some() {
                bail!("TASK_BLACKBOARD_INVALID: post rejects message_id, limit, and offset");
            }
        }
        BlackboardAction::Read => {
            if request.author.is_some() || request.payload.is_some() || request.message_id.is_some()
            {
                bail!("TASK_BLACKBOARD_INVALID: read rejects author, payload, and message_id");
            }
        }
        BlackboardAction::Inspect => {
            if request.message_id.is_none() {
                bail!("TASK_BLACKBOARD_INVALID: inspect requires message_id");
            }
            if request.scope.is_some()
                || request.milestone_ref.is_some()
                || request.task_id.is_some()
                || request.subtask_ref.is_some()
                || request.author.is_some()
                || request.topic.is_some()
                || request.gate.is_some()
                || request.payload.is_some()
                || request.limit.is_some()
                || request.offset.is_some()
            {
                bail!("TASK_BLACKBOARD_INVALID: inspect accepts only message_id");
            }
        }
    }
    if request.offset.is_some() && request.action != BlackboardAction::Read {
        bail!("TASK_BLACKBOARD_INVALID: offset is only valid for read");
    }
    Ok(())
}

fn blackboard_profile(
    context: &ResolvedBlackboardContext,
) -> Result<std::sync::Arc<crate::core::tasks::profile::GateProfile>> {
    match context.task_id.as_deref() {
        Some(task_id) => {
            let task = context.store.inspect(task_id)?;
            context.store.profile_for_task(&task)
        }
        None => Ok(context.profile.clone()),
    }
}

fn blackboard_schema_hint(
    profile: &crate::core::tasks::profile::GateProfile,
    issue: &str,
) -> String {
    let topics = crate::db::tasks_store::BLACKBOARD_TOPICS
        .iter()
        .map(|(topic, description)| format!("{topic}: {description}"))
        .collect::<Vec<_>>()
        .join("; ");
    let gates = profile
        .gates
        .iter()
        .map(|gate| gate.id.as_str())
        .collect::<Vec<_>>();
    let example_gate = gates.first().copied().unwrap_or("contract");
    let example = json!({
        "action": "post",
        "scope": "task",
        "gate": example_gate,
        "topic": "draft",
        "payload": "Record the contract decision"
    });
    format!(
        "TASK_BLACKBOARD_SCHEMA_INVALID: {issue}. Canonical topics: {topics}. Active profile gates: {}. example request: {}",
        if gates.is_empty() { "(none)".into() } else { gates.join(", ") },
        serde_json::to_string(&example).unwrap_or_else(|_| "{}".into()),
    )
}

fn validate_blackboard_schema(
    request: &BlackboardRequest,
    profile: &crate::core::tasks::profile::GateProfile,
) -> Result<()> {
    if request.action == BlackboardAction::Post && request.payload.is_none() {
        bail!(
            "{}",
            blackboard_schema_hint(profile, "post requires a payload")
        );
    }
    if request.action != BlackboardAction::Inspect {
        if request.action == BlackboardAction::Post && request.topic.is_none() {
            bail!(
                "{}",
                blackboard_schema_hint(profile, "post requires a topic")
            );
        }
        if let Some(topic) = request.topic.as_deref() {
            if !crate::db::tasks_store::is_canonical_blackboard_topic(topic) {
                bail!(
                    "{}",
                    blackboard_schema_hint(profile, &format!("topic '{topic}' is not canonical"))
                );
            }
        }
        if let Some(gate) = request.gate.as_deref() {
            if !profile.gates.iter().any(|definition| definition.id == gate) {
                bail!(
                    "{}",
                    blackboard_schema_hint(
                        profile,
                        &format!("gate '{gate}' is not in the active task profile")
                    )
                );
            }
        }
    }
    Ok(())
}

/// Resolve blackboard hierarchy context once and serve the same operation to MCP and CLI.
pub fn blackboard(root: &Path, request: BlackboardRequest, transport: &str) -> Result<Value> {
    validate_blackboard_request(&request)?;
    let registry = Registry::load(root)?;
    match request.action {
        BlackboardAction::Inspect => {
            let message_id = request
                .message_id
                .context("TASK_BLACKBOARD_INVALID: message_id is required")?;
            for workspace in registry.selected(Some("all"))? {
                let store = workspace.open(&registry.database)?;
                if let Some(message) = store.blackboard_inspect(message_id)? {
                    return Ok(json!({"message":message}));
                }
            }
            bail!("TASK_BLACKBOARD_NOT_FOUND")
        }
        BlackboardAction::Post => {
            let context = resolve_blackboard_context(&registry, &request)?;
            let profile = blackboard_profile(&context)?;
            validate_blackboard_schema(&request, &profile)?;
            let task_author = context
                .task_id
                .as_deref()
                .map(|task_id| context.store.inspect(task_id))
                .transpose()?
                .and_then(|task| task.worker_id);
            let author = request
                .author
                .as_deref()
                .or(task_author.as_deref())
                .unwrap_or(transport);
            let id = context
                .store
                .blackboard_post_scoped_with_gate(BlackboardPostInput {
                    milestone_ref: &context.milestone_ref,
                    task_id: context.task_id.as_deref(),
                    subtask_ref: context.subtask_ref.as_deref(),
                    gate: request.gate.as_deref(),
                    author,
                    topic: request
                        .topic
                        .as_deref()
                        .context("TASK_BLACKBOARD_INVALID: post requires topic")?,
                    payload: request
                        .payload
                        .as_deref()
                        .context("TASK_BLACKBOARD_INVALID: post requires payload")?,
                })?;
            Ok(json!({"id":id}))
        }
        BlackboardAction::Read => {
            let context = resolve_blackboard_context(&registry, &request)?;
            let profile = blackboard_profile(&context)?;
            validate_blackboard_schema(&request, &profile)?;
            let page = context
                .store
                .blackboard_page_for_gate(BlackboardPageRequest {
                    milestone_ref: &context.milestone_ref,
                    task_id: context.task_id.as_deref(),
                    subtask_ref: context.subtask_ref.as_deref(),
                    topic: request.topic.as_deref(),
                    gate: request.gate.as_deref(),
                    limit: request.limit,
                    offset: request.offset,
                })?;
            Ok(json!({"messages":page.messages,"pagination":page.pagination}))
        }
    }
}
