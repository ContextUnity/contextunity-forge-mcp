use crate::{
    core::tasks::{
        confined_path, gates::Evidence, shorten_task_response_commits, snapshot_inspect_cmd,
        Milestone,
    },
    db::tasks_store::{BlackboardPageRequest, BlackboardPostInput, Task, TasksStore},
};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};
mod workspaces;
use workspaces::Registry;
mod context;
use context::context_bundle;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
/// Task lifecycle status filter.
pub enum Status {
    /// Tasks whose dependencies are satisfied and are ready for claim.
    Ready,
    /// Tasks currently claimed and under active execution.
    InProgress,
    /// Tasks blocked by incomplete dependencies or open blockers.
    Blocked,
    /// Completed tasks with durable receipts.
    Completed,
    /// Query tasks across all statuses.
    All,
}
impl Status {
    /// String name of the status.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::All => "all",
        }
    }
}
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
/// Gate submission decision action.
pub enum Action {
    /// Pass gate criteria and advance to the next configured stage.
    Pass,
    /// Reject gate criteria with findings.
    Reject,
}
impl Action {
    /// String name of the action.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Reject => "reject",
        }
    }
}
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
/// Milestone lifecycle status filter for task queries.
pub enum MilestoneStatusFilter {
    /// Show tasks belonging to active milestones only (default).
    Active,
    /// Show tasks belonging to planned milestones.
    Planned,
    /// Show tasks belonging to completed milestones.
    Completed,
    /// Show tasks across all milestones regardless of status.
    All,
}

impl MilestoneStatusFilter {
    /// Checks if a given status string matches this filter.
    pub fn matches(&self, status: &str) -> bool {
        match self {
            Self::All => true,
            Self::Active => status == "active",
            Self::Planned => status == "planned",
            Self::Completed => status == "completed",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Controls how much subtask information task listings return.
pub enum TaskListDetail {
    /// Return subtask references and statuses only.
    #[default]
    Compact,
    /// Include subtask titles and verification evidence.
    Full,
}

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

#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
/// Parameters for listing tasks in .forge/tasks.sqlite.
pub struct List {
    /// Repository or workspace name filter (e.g. 'forge-mcp', a linked workspace name, or 'all').
    pub repository: Option<String>,
    /// Filter tasks by milestone: relative file path (e.g. 'docs/milestones/030-*.md'), numeric prefix ('030'), milestone ID ('m-...'), or slug.
    pub milestone_ref: Option<String>,
    /// Filter by milestone lifecycle status: 'active' (default), 'planned', 'completed', or 'all'. A milestone_ref without this filter selects all statuses.
    pub milestone_status: Option<MilestoneStatusFilter>,
    /// Filter by task status: ready (default), in_progress, blocked, completed, or all.
    pub status: Option<Status>,
    /// Filter by any gate identifier in the active workflow profile.
    pub stage: Option<String>,
    /// Subtask detail level: compact by default or full to include titles and evidence.
    pub detail: Option<TaskListDetail>,
}
#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
/// Parameters for claiming a task lifecycle gate.
pub struct Claim {
    /// Unique task identifier in repository/project/milestone:task_ref format.
    pub task_id: String,
    /// Lifecycle stage identifier from the task's active workflow profile.
    pub stage: String,
    /// Unique identifier of the claiming agent worker (must differ between builder and reviewer).
    pub worker_id: String,
    /// Absolute or repository-relative path of the development worktree executing this stage.
    pub worktree: String,
    #[serde(default)]
    /// Return the aggregated zero-shot task context bundle (defaults to true; set false for minimal details).
    pub bundle: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Parameters for submitting evidence to advance or reject a lifecycle gate.
pub struct Submit {
    /// Unique task identifier in repository/project/milestone:task_ref format.
    pub task_id: String,
    /// Active lifecycle stage identifier from the task's pinned workflow profile.
    pub stage: String,
    /// Claim-bound proof object containing task_id, stage, claim_revision, contract_revision, worker_id, worktree, an optional profile-defined commit reference, and exactly one proof key selected by the active gate definition.
    #[schemars(schema_with = "evidence_schema")]
    pub evidence: Value,
    /// Gate submission action: 'pass' to advance to next gate, or 'reject' to fail the gate.
    pub action: Action,
    #[serde(default)]
    #[schemars(schema_with = "findings_schema")]
    /// Optional JSON findings. Omit or null when unused. A rejected gate records findings and rewinds according to its configured reject_to stage.
    pub findings: Option<Value>,
}
fn findings_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = serde_json::Map::new();
    schema.insert(
        "description".into(),
        Value::String("Optional JSON findings. Omit or null when unused. A review or delivery rejection records findings and returns the task to build.".into()),
    );
    schema.into()
}
fn evidence_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.into()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
/// Administrative actions for task management.
pub enum ManageAction {
    /// Create a new root task specification in SQLite.
    Create,
    /// Synchronize task specifications from milestone markdown documents.
    Sync,
    #[default]
    /// Inspect details, gate history, and evidence for a specific task.
    Inspect,
    /// Delete a task from SQLite.
    Delete,
    /// Extend the allowed file scope paths for a task.
    ExtendScope,
    /// Add an iterative subtask with subtask_ref and title to an existing task.
    SubtaskAdd,
    /// Update status and evidence for an existing subtask.
    SubtaskUpdate,
    /// List all iterative subtasks for a parent task.
    SubtaskList,
    /// Reset an existing or completed task back to ready state.
    Reset,
    /// Reopen an existing or completed task back to ready state.
    Reopen,
    /// Return the unified zero-shot task context bundle.
    Context,
}
#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
/// Parameters for administrative task management in .forge/tasks.sqlite.
pub struct Manage {
    /// Workspace or repository name (defaults to primary configured workspace).
    pub workspace: Option<String>,
    /// Administrative action to perform: inspect, sync, create, delete, extend_scope, subtask_add, subtask_update, subtask_list, reset, reopen, context.
    pub action: ManageAction,
    /// Task identifier for inspect, delete, subtask operations, reset, or reopen.
    pub task_id: Option<String>,
    /// Milestone reference for sync, create, or milestone-scoped delete: relative file path (e.g. 'docs/milestones/030-*.md'), numeric prefix ('030'), milestone ID ('m-...'), or slug.
    pub milestone_ref: Option<String>,
    /// Task reference slug for task creation.
    pub task_ref: Option<String>,
    /// Allowed file scope paths for task creation or scope extension.
    pub paths: Option<Vec<String>>,
    #[serde(default)]
    /// Force execution when resetting terminal tasks or deleting.
    pub force: bool,
    #[serde(default)]
    /// Subtask reference slug for subtask_add or subtask_update (e.g. 'handle-null-return').
    pub subtask_ref: Option<String>,
    #[serde(default)]
    /// Subtask title or goal description for subtask_add.
    pub title: Option<String>,
    #[serde(default)]
    /// Subtask status for subtask_update: 'pending', 'in_progress', or 'completed'.
    pub subtask_status: Option<String>,
    #[serde(default)]
    /// Subtask verification evidence, test command, or receipt for subtask_update.
    pub evidence: Option<String>,
}

#[derive(Deserialize)]
struct TaskSettings {
    #[serde(default = "default_tasks_db")]
    tasks_db: std::path::PathBuf,
    #[serde(default = "default_repository")]
    task_repository: String,
    task_project: Option<String>,
    pub(crate) agents_guidance: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) gates: Option<String>,
    #[allow(dead_code)]
    #[serde(default = "default_milestones")]
    pub(crate) milestones: Vec<String>,
    #[allow(dead_code)]
    #[serde(default = "default_plans")]
    pub(crate) plans: Vec<String>,
    #[serde(default)]
    linked_workspaces: Vec<workspaces::LinkedConfig>,
}
fn default_tasks_db() -> std::path::PathBuf {
    ".forge/tasks.sqlite".into()
}
fn default_repository() -> String {
    "forge-mcp".into()
}
fn default_milestones() -> Vec<String> {
    vec!["docs/milestones".into()]
}
fn default_plans() -> Vec<String> {
    vec!["docs/plans".into()]
}
fn primary_worktree_root(root: &Path) -> Option<std::path::PathBuf> {
    if !root.join(".git").exists() {
        return None;
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = std::str::from_utf8(&output.stdout).ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    let common_dir = Path::new(raw);
    let common_dir = if common_dir.is_absolute() {
        common_dir.to_path_buf()
    } else {
        root.join(common_dir)
    };
    let common_dir = common_dir.canonicalize().ok().unwrap_or(common_dir);
    if common_dir.file_name() == Some(std::ffi::OsStr::new(".git")) {
        common_dir.parent().map(Path::to_path_buf)
    } else {
        Some(common_dir)
    }
}
impl TaskSettings {
    fn load(root: &Path) -> Result<Self> {
        let config = std::fs::read_to_string(root.join("forge-mcp.yaml"))
            .or_else(|_| {
                if let Some(base) = primary_worktree_root(root) {
                    std::fs::read_to_string(base.join("forge-mcp.yaml"))
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
                }
            })
            .context("task operations require forge-mcp.yaml")?;
        let settings: Self = serde_yaml::from_str(&config).context("invalid task configuration")?;
        crate::core::tasks::valid_identity(&settings.task_repository)?;
        if let Some(project) = &settings.task_project {
            crate::core::tasks::valid_identity(project)?;
        }
        Ok(settings)
    }
    fn path(&self, root: &Path) -> Result<std::path::PathBuf> {
        let path = &self.tasks_db;
        let (base, path) = if path.is_absolute() {
            (root.to_path_buf(), path.to_path_buf())
        } else {
            let base = primary_worktree_root(root).unwrap_or_else(|| root.to_path_buf());
            let full_path = base.join(path);
            (base, full_path)
        };
        if path == crate::cli::default_db(root) || path == crate::cli::default_db(&base) {
            bail!("tasks_db must be separate from code index");
        }
        Ok(path)
    }
}
/// Performs database path.
pub fn database_path(root: &Path) -> Result<std::path::PathBuf> {
    TaskSettings::load(root)?.path(root)
}
/// Performs project identity.
pub fn project_identity(root: &Path) -> Result<(String, String)> {
    let settings = TaskSettings::load(root)?;
    Ok((
        settings.task_repository.clone(),
        settings.task_project.unwrap_or(settings.task_repository),
    ))
}
/// Performs store.
pub fn store(root: &Path) -> Result<TasksStore> {
    let settings = TaskSettings::load(root)?;
    TasksStore::open_project(
        &settings.path(root)?,
        &settings.task_repository,
        settings
            .task_project
            .as_deref()
            .unwrap_or(&settings.task_repository),
    )
}
fn resolve_manifest_status(root: &Path, milestone_ref: &str) -> Result<String> {
    let path = confined_path(root, milestone_ref)?;
    let text = std::fs::read_to_string(&path)?;
    let archived = path
        .components()
        .any(|component| component.as_os_str() == "archive");
    Ok(Milestone::status_from_frontmatter(&text, archived)?.to_owned())
}

/// Performs list.
pub fn list(root: &Path, mut p: List) -> Result<Value> {
    let registry = Registry::load(root)?;
    let mut resolved_workspace = None;
    if let Some(m) = p.milestone_ref.as_deref() {
        let (workspace, resolved) = registry.resolve_milestone(p.repository.as_deref(), m)?;
        resolved_workspace = Some(workspace);
        p.milestone_ref = Some(resolved);
    }
    let status = p.status.unwrap_or(Status::Ready).name();
    let default_milestone_filter = if p.milestone_ref.is_some() {
        MilestoneStatusFilter::All
    } else {
        MilestoneStatusFilter::Active
    };
    let effective_milestone_filter = p.milestone_status.unwrap_or(default_milestone_filter);
    let detail = p.detail.unwrap_or_default();
    let mut tasks = Vec::new();
    let all_workspaces = p.repository.as_deref() == Some("all") && p.milestone_ref.is_none();
    let mut workspaces = serde_json::Map::new();
    let mut selected = match resolved_workspace {
        Some(workspace) => vec![workspace],
        None => registry.selected(p.repository.as_deref())?,
    };
    for workspace in &selected {
        let mut store = workspace.open(&registry.database)?;
        store.cleanup(crate::db::tasks_store::now())?;
        let mut milestone_status_cache: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for mut task in store.list(p.milestone_ref.as_deref(), "all", p.stage.as_deref())? {
            let milestone_status = match milestone_status_cache.get(&task.milestone_ref) {
                Some(status) => status.clone(),
                None => {
                    let status = resolve_manifest_status(&workspace.root, &task.milestone_ref)?;
                    milestone_status_cache.insert(task.milestone_ref.clone(), status.clone());
                    status
                }
            };
            if !effective_milestone_filter.matches(&milestone_status) {
                continue;
            }
            if task.status != "completed"
                && (validate_authority(
                    &task,
                    task.worktree
                        .as_deref()
                        .map(Path::new)
                        .unwrap_or(&workspace.root),
                )
                .is_err()
                    || registry
                        .check_scope(
                            workspace,
                            &task.spec.scope,
                            task.worktree.as_deref().map(Path::new),
                        )
                        .is_err())
            {
                task.status = "blocked".into();
            }
            if status == "all" || task.status == status {
                let stage = if task.status == "completed" {
                    "completed"
                } else {
                    task.stage.as_str()
                };
                let mut item = json!({
                    "task_id": task.task_id,
                    "target": task.spec.target,
                    "status": task.status,
                    "stage": stage,
                    "owner": task.worker_id,
                    "agent_type": task.spec.agent_type.as_deref().unwrap_or("worker"),
                    "rev": task.contract_revision,
                });
                if !task.spec.subtasks.is_empty() {
                    item["subtasks"] = match detail {
                        TaskListDetail::Compact => {
                            let compact: Vec<_> = task
                                .spec
                                .subtasks
                                .iter()
                                .map(|subtask| {
                                    json!({
                                        "subtask_ref": subtask.subtask_ref,
                                        "status": subtask.status,
                                    })
                                })
                                .collect();
                            json!(compact)
                        }
                        TaskListDetail::Full => serde_json::to_value(&task.spec.subtasks)?,
                    };
                }
                tasks.push(item);
            }
        }
        if all_workspaces {
            workspaces.insert(
                format!("{}/{}", workspace.repository, workspace.project),
                workspace.envelope(json!({}))?,
            );
        }
    }
    if all_workspaces {
        Ok(json!({"tasks":tasks,"workspaces":workspaces}))
    } else {
        selected
            .pop()
            .context("task workspace missing")?
            .envelope(json!({"tasks":tasks}))
    }
}
fn validate_authority(task: &crate::db::tasks_store::Task, root: &Path) -> Result<()> {
    let text = std::fs::read_to_string(confined_path(root, &task.milestone_ref)?)
        .context("TASK_AUTHORITY_MISSING")?;
    let mut identity = task.task_id.split('/');
    let repository = identity.next().context("invalid task identity")?;
    let project = identity.next().context("invalid task identity")?;
    let milestone = Milestone::parse_with_identity(&text, repository, project)?;
    let spec = milestone
        .tasks
        .iter()
        .find(|s| milestone.task_id(s) == task.task_id)
        .context("TASK_AUTHORITY_MISSING")?;
    if milestone.digest(spec)? != task.digest {
        bail!("AUTHORITY_GAP: specification changed");
    }
    Ok(())
}
/// Performs claim.
pub fn claim(root: &Path, p: Claim) -> Result<Value> {
    let worktree = crate::core::tasks::claim_worktree(&p.worktree)?;
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    registry.check_worktree(workspace, &worktree)?;
    let mut store = workspace.open(&registry.database)?;
    let task = store.inspect(&p.task_id)?;
    validate_authority(&task, &worktree)?;
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
/// Performs store for task.
pub fn store_for_task(root: &Path, id: &str) -> Result<TasksStore> {
    let registry = Registry::load(root)?;
    registry.owner(id)?.open(&registry.database)
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

fn lock_receipts(database: &Path) -> Result<File> {
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
    crate::engine::milestones::atomic_replace(&milestone_path, updated.as_bytes())?;
    match store.reset(id) {
        Ok(task) => Ok(task),
        Err(error) => {
            crate::engine::milestones::atomic_replace(&milestone_path, original.as_bytes())
                .context("TASK_RECEIPT_ROLLBACK_FAILED")?;
            Err(error)
        }
    }
}
/// Reopens a task back to ready state at contract gate.
pub fn reopen(root: &Path, id: &str) -> Result<Task> {
    reset(root, id)
}
static SNAPSHOT_INDEX_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TemporaryGitIndex(PathBuf);

impl TemporaryGitIndex {
    fn create() -> Result<Self> {
        loop {
            let sequence = SNAPSHOT_INDEX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "forge-snap-{}-{}-{sequence}.index",
                std::process::id(),
                crate::db::tasks_store::now()
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(index) => {
                    drop(index);
                    return Ok(Self(candidate));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryGitIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct GitHeadRollback {
    worktree: PathBuf,
    ref_name: String,
    baseline: String,
    landed: String,
    index: Option<GitIndexRollback>,
    armed: bool,
}

impl GitHeadRollback {
    fn rollback(&mut self) -> Result<()> {
        if !self.armed && self.index.as_ref().is_none_or(|index| !index.armed) {
            return Ok(());
        }
        let mut failures = Vec::new();
        if self.armed {
            let output = std::process::Command::new("git")
                .args(["update-ref", &self.ref_name, &self.baseline, &self.landed])
                .current_dir(&self.worktree)
                .output();
            match output {
                Ok(output) if output.status.success() => self.armed = false,
                Ok(output) => failures.push(format!(
                    "branch ref: {}",
                    String::from_utf8_lossy(&output.stderr)
                )),
                Err(error) => failures.push(format!("branch ref: {error}")),
            }
        }
        if let Some(index) = self.index.as_mut() {
            if let Err(error) = index.restore() {
                failures.push(format!("index: {error:#}"));
            }
        }
        if !failures.is_empty() {
            bail!("TASK_DELIVERY_ROLLBACK_FAILED: {}", failures.join("; "));
        }
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
        if let Some(index) = self.index.as_mut() {
            index.disarm();
        }
    }
}

impl Drop for GitHeadRollback {
    fn drop(&mut self) {
        if self.armed || self.index.as_ref().is_some_and(|index| index.armed) {
            let _ = self.rollback();
        }
    }
}

#[derive(Debug, Clone)]
struct GitIndexEntry {
    mode: String,
    object: String,
    stage: String,
    path: Vec<u8>,
}

struct GitIndexRollback {
    worktree: PathBuf,
    paths: Vec<Vec<u8>>,
    previous: Vec<GitIndexEntry>,
    object_id_len: usize,
    armed: bool,
}

impl GitIndexRollback {
    fn capture(worktree: &Path, paths: Vec<Vec<u8>>) -> Result<Self> {
        let output = std::process::Command::new("git")
            .args(["ls-files", "--stage", "-z"])
            .current_dir(worktree)
            .output()?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_INDEX_FAILED: could not read the current index: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let object_format = git_stdout(worktree, &["rev-parse", "--show-object-format"], "TASK_DELIVERY_INDEX_FAILED")?;
        let object_id_len = match object_format.as_str() {
            "sha1" => 40,
            "sha256" => 64,
            _ => bail!("TASK_DELIVERY_INDEX_FAILED: unsupported Git object format '{object_format}'"),
        };
        let changed = paths.iter().cloned().collect::<std::collections::HashSet<_>>();
        let mut previous = Vec::new();
        for record in output.stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
            let separator = record
                .iter()
                .position(|byte| *byte == b'\t')
                .context("TASK_DELIVERY_INDEX_FAILED: malformed index entry")?;
            let (header, path_with_separator) = record.split_at(separator);
            let path = &path_with_separator[1..];
            if !changed.contains(path) {
                continue;
            }
            let mut fields = header.split(|byte| *byte == b' ');
            let mode = fields.next().context("malformed index mode")?;
            let object = fields.next().context("malformed index object")?;
            let stage = fields.next().context("malformed index stage")?;
            if fields.next().is_some() {
                bail!("TASK_DELIVERY_INDEX_FAILED: malformed index entry header");
            }
            previous.push(GitIndexEntry {
                mode: String::from_utf8(mode.to_vec()).context("invalid index mode")?,
                object: String::from_utf8(object.to_vec()).context("invalid index object id")?,
                stage: String::from_utf8(stage.to_vec()).context("invalid index stage")?,
                path: path.to_vec(),
            });
        }
        Ok(Self {
            worktree: worktree.to_path_buf(),
            paths,
            previous,
            object_id_len,
            armed: true,
        })
    }

    fn apply(&mut self, commit: &str) -> Result<()> {
        if self.paths.is_empty() {
            self.armed = false;
            return Ok(());
        }
        let mut pathspecs = Vec::new();
        for path in &self.paths {
            pathspecs.extend_from_slice(b":(literal)");
            pathspecs.extend_from_slice(path);
            pathspecs.push(0);
        }
        let mut child = std::process::Command::new("git")
            .args([
                "reset",
                "--quiet",
                commit,
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ])
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .as_mut()
            .context("TASK_DELIVERY_INDEX_FAILED: Git reset stdin is unavailable")?
            .write_all(&pathspecs)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_INDEX_FAILED: could not reconcile delivery paths: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let zero = "0".repeat(self.object_id_len);
        let mut input = Vec::new();
        for path in &self.paths {
            for stage in 0..=3 {
                input.extend_from_slice(format!("0 {zero} {stage}\t").as_bytes());
                input.extend_from_slice(path);
                input.push(0);
            }
        }
        for entry in &self.previous {
            input.extend_from_slice(
                format!("{} {} {}\t", entry.mode, entry.object, entry.stage).as_bytes(),
            );
            input.extend_from_slice(&entry.path);
            input.push(0);
        }
        let mut child = std::process::Command::new("git")
            .args(["update-index", "-z", "--index-info"])
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .as_mut()
            .context("TASK_DELIVERY_ROLLBACK_FAILED: Git index stdin is unavailable")?
            .write_all(&input)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_ROLLBACK_FAILED: could not restore index entries: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        self.armed = false;
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for GitIndexRollback {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.restore();
        }
    }
}

fn git_stdout(worktree: &Path, args: &[&str], error_code: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(worktree)
        .output()?;
    if !output.status.success() {
        bail!("{error_code}: {}", String::from_utf8_lossy(&output.stderr));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        bail!("{error_code}: empty Git response");
    }
    Ok(value)
}

fn run_git_input(
    worktree: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: &[u8],
    error_code: &str,
) -> Result<()> {
    let mut command = std::process::Command::new("git");
    command
        .args(args)
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn()?;
    child
        .stdin
        .as_mut()
        .context("Git command stdin is unavailable")?
        .write_all(input)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!("{error_code}: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn add_candidate_entries_to_index(
    worktree: &Path,
    temp_index: &Path,
    candidate: &str,
) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["ls-tree", "-r", "-z", "--full-tree", candidate])
        .current_dir(worktree)
        .output()?;
    if !output.status.success() {
        bail!(
            "TASK_CANDIDATE_INVALID: could not read candidate entries: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut entries = Vec::new();
    for record in output.stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("TASK_CANDIDATE_INVALID: malformed candidate tree entry")?;
        let (header, path_with_separator) = record.split_at(separator);
        let path = &path_with_separator[1..];
        let fields = header.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("TASK_CANDIDATE_INVALID: malformed candidate tree entry header");
        }
        entries.extend_from_slice(fields[0]);
        entries.push(b' ');
        entries.extend_from_slice(fields[2]);
        entries.extend_from_slice(b" 0\t");
        entries.extend_from_slice(path);
        entries.push(0);
    }
    if !entries.is_empty() {
        run_git_input(
            worktree,
            &["update-index", "--add", "-z", "--index-info"],
            Some(temp_index),
            &entries,
            "TASK_DELIVERY_COMMIT_FAILED: could not overlay candidate tree",
        )?;
    }
    Ok(())
}

fn delivery_changed_paths(worktree: &Path, baseline: &str, landed: &str) -> Result<Vec<Vec<u8>>> {
    let output = std::process::Command::new("git")
        .args([
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            "-z",
            baseline,
            landed,
        ])
        .current_dir(worktree)
        .output()?;
    if !output.status.success() {
        bail!(
            "TASK_DELIVERY_INDEX_FAILED: could not list delivery paths: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

fn commit_tree_and_parent(worktree: &Path, commit: &str) -> Result<(String, String)> {
    if !(40..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("TASK_SNAPSHOT_INVALID: commit must be a full hexadecimal object id");
    }
    let details = git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_SNAPSHOT_INVALID",
    )?;
    let fields = details.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 {
        bail!("TASK_SNAPSHOT_INVALID: candidate commit must have exactly one baseline parent");
    }
    let treeish = format!("{commit}^{{tree}}");
    let tree = git_stdout(worktree, &["rev-parse", &treeish], "TASK_SNAPSHOT_INVALID")?;
    Ok((tree, fields[1].to_owned()))
}

fn candidate_tree(worktree: &Path, commit: &str) -> Result<String> {
    if !(40..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("TASK_SNAPSHOT_INVALID: commit must be a full hexadecimal object id");
    }
    let details = git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_SNAPSHOT_INVALID",
    )?;
    if details.split_whitespace().count() != 1 {
        bail!("TASK_CANDIDATE_INVALID: scoped candidate must be a parentless snapshot commit");
    }
    let treeish = format!("{commit}^{{tree}}");
    git_stdout(worktree, &["rev-parse", &treeish], "TASK_SNAPSHOT_INVALID")
}

fn commit_candidate_snapshot(
    worktree: &Path,
    task: &Task,
    candidate: &str,
    candidate_baseline: &str,
    scope: &[String],
    receipt_path: &Path,
) -> Result<(String, Option<GitHeadRollback>)> {
    candidate_tree(worktree, candidate)?;
    let ref_name = std::process::Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(worktree)
        .output()?;
    let ref_name = if ref_name.status.success() {
        String::from_utf8_lossy(&ref_name.stdout).trim().to_owned()
    } else {
        "HEAD".to_owned()
    };
    if ref_name != "HEAD" {
        let valid_ref = std::process::Command::new("git")
            .args(["check-ref-format", &ref_name])
            .current_dir(worktree)
            .output()?;
        if !valid_ref.status.success() {
            bail!("TASK_DELIVERY_COMMIT_FAILED: current branch ref is invalid");
        }
    }
    let temp_index = TemporaryGitIndex::create()?;
    let temp_index_str = temp_index.path().to_string_lossy().to_string();
    let baseline_index = std::process::Command::new("git")
        .args(["read-tree", candidate_baseline])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !baseline_index.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not initialize baseline delivery index: {}",
            String::from_utf8_lossy(&baseline_index.stderr)
        );
    }
    let mut scope_paths = scope.iter().map(String::as_str).collect::<Vec<_>>();
    if scope_paths.is_empty() {
        scope_paths.push(".");
    }
    let mut remove_scoped = std::process::Command::new("git");
    remove_scoped
        .args(["rm", "--cached", "--quiet", "-r", "--ignore-unmatch", "--"])
        .args(&scope_paths)
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree);
    let removed = remove_scoped.output()?;
    if !removed.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not remove baseline scope entries: {}",
            String::from_utf8_lossy(&removed.stderr)
        );
    }
    add_candidate_entries_to_index(worktree, temp_index.path(), candidate)?;
    let receipt_path = receipt_path
        .strip_prefix(worktree)
        .context("TASK_DELIVERY_COMMIT_INVALID: receipt path is outside the worktree")?
        .to_string_lossy()
        .replace('\\', "/");
    let staged_receipt = std::process::Command::new("git")
        .args(["add", "--force", "--", &receipt_path])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !staged_receipt.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not stage the milestone receipt: {}",
            String::from_utf8_lossy(&staged_receipt.stderr)
        );
    }
    let write_tree = std::process::Command::new("git")
        .arg("write-tree")
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !write_tree.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not write delivery tree: {}",
            String::from_utf8_lossy(&write_tree.stderr)
        );
    }
    let expected_tree = String::from_utf8_lossy(&write_tree.stdout).trim().to_owned();
    let head = git_stdout(worktree, &["rev-parse", "--verify", "HEAD"], "TASK_CANDIDATE_BASELINE_MISMATCH")?;
    if head != candidate_baseline {
        let (head_tree, head_parent) = commit_tree_and_parent(worktree, &head)
            .unwrap_or_else(|_| (String::new(), String::new()));
        if head_tree == expected_tree && head_parent == candidate_baseline {
            return Ok((head, None));
        }
        bail!("TASK_CANDIDATE_BASELINE_MISMATCH: HEAD '{head}' does not match candidate baseline '{candidate_baseline}'");
    }
    let mut message = format!(
        "forge-task: {}\n\nTarget: {}\n\nCompleted subtasks:",
        single_line_commit_text(&task.task_id),
        single_line_commit_text(&task.spec.target),
    );
    let mut completed_subtasks = 0;
    for subtask in task
        .spec
        .subtasks
        .iter()
        .filter(|subtask| subtask.status == "completed")
    {
        message.push_str("\n- ");
        message.push_str(&single_line_commit_text(&subtask.subtask_ref));
        message.push_str(": ");
        message.push_str(&single_line_commit_text(&subtask.title));
        completed_subtasks += 1;
    }
    if completed_subtasks == 0 {
        message.push_str(" none");
    }
    let committed = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", &message])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !committed.status.success() {
        if let Ok(head) = git_stdout(worktree, &["rev-parse", "--verify", "HEAD"], "TASK_DELIVERY_COMMIT_FAILED") {
            if let Ok((tree, parent)) = commit_tree_and_parent(worktree, &head) {
                if tree == expected_tree && parent == candidate_baseline {
                    let mut rollback = GitHeadRollback {
                        worktree: worktree.to_path_buf(),
                        ref_name,
                        baseline: candidate_baseline.to_owned(),
                        landed: head,
                        index: None,
                        armed: true,
                    };
                    rollback.rollback()?;
                }
            }
        }
        bail!(
            "TASK_DELIVERY_HOOK_REJECTED: {}",
            String::from_utf8_lossy(&committed.stderr)
        );
    }
    let landed = git_stdout(worktree, &["rev-parse", "--verify", "HEAD"], "TASK_DELIVERY_COMMIT_FAILED")?;
    let mut rollback = GitHeadRollback {
        worktree: worktree.to_path_buf(),
        ref_name,
        baseline: candidate_baseline.to_owned(),
        landed: landed.clone(),
        index: None,
        armed: true,
    };
    let (landed_tree, landed_parent) = match commit_tree_and_parent(worktree, &landed) {
        Ok(details) => details,
        Err(error) => {
            rollback.rollback()?;
            return Err(error);
        }
    };
    if landed_tree != expected_tree || landed_parent != candidate_baseline {
        rollback.rollback()?;
        bail!("TASK_DELIVERY_COMMIT_INVALID: hooks or concurrent changes altered the candidate tree or parent");
    }
    let changed_paths = match delivery_changed_paths(worktree, candidate_baseline, &landed) {
        Ok(paths) => paths,
        Err(error) => {
            rollback.rollback()?;
            return Err(error);
        }
    };
    let mut index_rollback = match GitIndexRollback::capture(worktree, changed_paths) {
        Ok(index) => index,
        Err(error) => {
            rollback.rollback()?;
            return Err(error);
        }
    };
    if let Err(error) = index_rollback.apply(&landed) {
        index_rollback.restore().context("TASK_DELIVERY_ROLLBACK_FAILED")?;
        rollback.rollback()?;
        return Err(error);
    }
    rollback.index = Some(index_rollback);
    Ok((landed, Some(rollback)))
}

fn single_line_commit_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn capture_scoped_snapshot(
    worktree: &Path,
    task_id: &str,
    scope: &[String],
) -> Result<Option<(String, String)>> {
    let head_out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree)
        .output();
    let head = match head_out {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                return Ok(None);
            }
            s
        }
        _ => return Ok(None),
    };

    let temp_index = TemporaryGitIndex::create()?;
    let temp_index_str = temp_index.path().to_string_lossy().to_string();

    // Keep unrelated branch content out of this task's candidate tree.
    let read_tree = std::process::Command::new("git")
        .args(["read-tree", "--empty"])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !read_tree.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: git read-tree failed: {}",
            String::from_utf8_lossy(&read_tree.stderr)
        );
    }

    let mut valid_paths = Vec::with_capacity(scope.len());
    for path in scope {
        match std::fs::symlink_metadata(worktree.join(path)) {
            Ok(_) => valid_paths.push(path.as_str()),
            // The temporary index is empty, so a deleted scoped path is
            // already absent from the candidate tree and must not be passed
            // to `git add` as a pathspec.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("TASK_SNAPSHOT_FAILED: could not inspect scoped path {path}")
                });
            }
        }
    }

    if !valid_paths.is_empty() {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["add", "-A", "--"]);
        cmd.args(&valid_paths);
        cmd.env("GIT_INDEX_FILE", &temp_index_str);
        cmd.current_dir(worktree);
        let out = cmd.output()?;
        if !out.status.success() {
            bail!(
                "TASK_SNAPSHOT_FAILED: git add failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    } else if scope.is_empty() {
        let out = std::process::Command::new("git")
            .args(["add", "-A"])
            .env("GIT_INDEX_FILE", &temp_index_str)
            .current_dir(worktree)
            .output()?;
        if !out.status.success() {
            bail!(
                "TASK_SNAPSHOT_FAILED: git add -A failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    let write_tree_out = std::process::Command::new("git")
        .args(["write-tree"])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !write_tree_out.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: git write-tree failed: {}",
            String::from_utf8_lossy(&write_tree_out.stderr)
        );
    }
    let tree_sha = String::from_utf8_lossy(&write_tree_out.stdout)
        .trim()
        .to_string();
    if tree_sha.is_empty() {
        bail!("TASK_SNAPSHOT_FAILED: empty tree SHA from write-tree");
    }

    let commit_msg = format!("forge-snapshot: {task_id}");
    let commit_out = std::process::Command::new("git")
        .args(["commit-tree", &tree_sha, "-m", &commit_msg])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .env("GIT_AUTHOR_NAME", "forge-agent")
        .env("GIT_AUTHOR_EMAIL", "forge@contextunity.local")
        .env("GIT_COMMITTER_NAME", "forge-agent")
        .env("GIT_COMMITTER_EMAIL", "forge@contextunity.local")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .current_dir(worktree)
        .output()?;
    if !commit_out.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: git commit-tree failed: {}",
            String::from_utf8_lossy(&commit_out.stderr)
        );
    }
    let commit_sha = String::from_utf8_lossy(&commit_out.stdout)
        .trim()
        .to_string();
    if !(7..=64).contains(&commit_sha.len()) {
        bail!("TASK_SNAPSHOT_FAILED: invalid commit SHA length: {commit_sha}");
    }

    Ok(Some((commit_sha, head)))
}

/// Performs submit.
pub fn submit(root: &Path, p: Submit) -> Result<Value> {
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    let mut store = workspace.open(&registry.database)?;
    let observed_task = store.inspect(&p.task_id)?;
    let profile = store.profile_for_task(&observed_task)?;
    let gate_index = profile.index_of(&p.stage).with_context(|| format!(
        "TASK_STAGE_UNKNOWN: '{}' is not in the active profile; choose one of: {}",
        p.stage,
        profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
    ))?;
    let gate = &profile.gates[gate_index];
    let is_delivery = gate.proof.kind()
        == Some(crate::core::tasks::profile::ProofKind::Delivery);
    let _receipt_lock = if is_delivery {
        Some(lock_receipts(&registry.database)?)
    } else {
        None
    };
    let task = store.inspect(&p.task_id)?;
    if task.stage != observed_task.stage || task.profile_pin != observed_task.profile_pin {
        bail!("TASK_STALE_SUBMISSION");
    }
    if !p.evidence.is_object() {
        bail!("TASK_EVIDENCE_INVALID: evidence must be a JSON object");
    }
    let mut evidence: Evidence = serde_json::from_value(p.evidence)
        .context("TASK_EVIDENCE_INVALID: expected a structured JSON evidence object")?;
    // This value is derived from the accepted snapshot, never from caller-supplied evidence.
    evidence.candidate_baseline_head = None;
    registry.check_worktree(workspace, Path::new(&evidence.worktree))?;
    if task.status != "completed" {
        validate_authority(&task, Path::new(&evidence.worktree))?;
        registry.check_scope(
            workspace,
            &task.spec.scope,
            Some(Path::new(&evidence.worktree)),
        )?;
    }
    let worktree = Path::new(&evidence.worktree);
    let explicit_commit = evidence.commit.clone();
    let prior_snapshot = profile.gates[..gate_index]
        .iter()
        .rev()
        .find(|prior| prior.sha_snapshot);
    let needs_candidate = !gate.sha_snapshot && prior_snapshot.is_some();
    let mut candidate_commit = None;
    let mut delivery_rollback = None;
    if gate.sha_snapshot && p.action.name() == "pass" {
        let (snapshot, baseline) = capture_scoped_snapshot(worktree, &p.task_id, &task.spec.scope)?
            .context("TASK_SNAPSHOT_FAILED: snapshot capture requires a Git worktree")?;
        if explicit_commit.as_ref().is_some_and(|commit| commit != &snapshot) {
            bail!("TASK_CANDIDATE_MISMATCH: supplied commit does not match the scoped snapshot");
        }
        evidence.commit = Some(snapshot);
        evidence.candidate_baseline_head = Some(baseline);
        candidate_commit = evidence.commit.clone();
    } else if needs_candidate {
        let (candidate, candidate_baseline) = store
            .build_snapshot_candidate(&p.task_id)?
            .context("TASK_SNAPSHOT_NOT_FOUND: no accepted candidate snapshot precedes this gate")?;
        candidate_commit = Some(candidate.clone());
        evidence.candidate_baseline_head = Some(candidate_baseline.clone());
        if is_delivery && gate.auto_commit && p.action.name() == "pass" {
            if task.status == "completed" {
                let delivered = task.receipt.as_ref()
                    .and_then(|receipt| receipt.commit.as_deref())
                    .context("TASK_RECEIPT_INVALID: completed task has no delivery commit")?;
                if explicit_commit.as_deref().is_some_and(|commit| commit != delivered) {
                    bail!("TASK_CANDIDATE_MISMATCH: supplied delivery commit differs from the accepted receipt");
                }
                evidence.commit = Some(delivered.to_owned());
            } else {
                let head = git_stdout(
                    worktree,
                    &["rev-parse", "--verify", "HEAD"],
                    "TASK_CANDIDATE_BASELINE_MISMATCH",
                )?;
                if head != candidate_baseline {
                    bail!("TASK_CANDIDATE_BASELINE_MISMATCH: HEAD '{head}' does not match candidate baseline '{candidate_baseline}'");
                }
                let (current, _) = capture_scoped_snapshot(worktree, &p.task_id, &task.spec.scope)?
                    .context("TASK_SNAPSHOT_FAILED: snapshot capture requires a Git worktree")?;
                if current != candidate {
                    bail!("TASK_CANDIDATE_MISMATCH: worktree has diverged from accepted candidate");
                }
                evidence.commit = Some(candidate.clone());
            }
        } else {
            if explicit_commit.as_ref().is_some_and(|commit| commit != &candidate) {
                bail!("TASK_CANDIDATE_MISMATCH: supplied commit does not match accepted candidate");
            }
            evidence.commit = Some(candidate.clone());
            if matches!(gate.proof.kind(), Some(crate::core::tasks::profile::ProofKind::Review | crate::core::tasks::profile::ProofKind::Delivery)) {
                let (current, _) = capture_scoped_snapshot(worktree, &p.task_id, &task.spec.scope)?
                    .context("TASK_SNAPSHOT_FAILED: snapshot capture requires a Git worktree")?;
                if current != candidate {
                    bail!("TASK_CANDIDATE_MISMATCH: worktree has diverged from accepted candidate");
                }
            }
        }
    }
    let pin_snapshot = p.action.name() == "pass" && gate.sha_snapshot;
    let snapshot_worktree = evidence.worktree.clone();
    let snapshot_task_id = p.task_id.clone();
    let snapshot_scope = task.spec.scope.clone();
    let snapshot_commit = candidate_commit.clone().or_else(|| evidence.commit.clone());
    let snapshot_baseline = evidence.candidate_baseline_head.clone();
    let delivery_needs_commit = is_delivery
        && gate.auto_commit
        && p.action.name() == "pass"
        && task.status != "completed";
    let snapshot_milestone_ref = task.milestone_ref.clone();
    let task_result = store.submit_with_pre_persist(
        &p.task_id,
        &p.stage,
        &mut evidence,
        p.action.name(),
        p.findings.as_ref(),
        |delivery_task| {
            if pin_snapshot {
                if let Some(commit_sha) = snapshot_commit.as_deref() {
                    let worktree = Path::new(&snapshot_worktree);
                    let repository = std::process::Command::new("git")
                        .args(["rev-parse", "--git-dir"])
                        .current_dir(worktree)
                        .output()?;
                    if !repository.status.success() {
                        // Non-Git task workspaces have no repository ref to preserve.
                        return Ok(None);
                    }
                    let obj_exists = std::process::Command::new("git")
                        .args(["cat-file", "-e", commit_sha])
                        .current_dir(worktree)
                        .output()?;
                    if !obj_exists.status.success() {
                        bail!(
                            "TASK_SNAPSHOT_REF_FAILED: candidate object {commit_sha} is unavailable: {}",
                            String::from_utf8_lossy(&obj_exists.stderr)
                        );
                    }
                    let clean_ref_path = if let Some((prefix, task_ref)) = snapshot_task_id.split_once(':') {
                        let clean_prefix = prefix.replace(['\\', ' ', ':'], "_");
                        let clean_task_ref = task_ref.replace(['\\', ' ', ':'], "_");
                        format!("{clean_prefix}/{clean_task_ref}")
                    } else {
                        snapshot_task_id.replace([':', '\\', ' '], "_")
                    };
                    let ref_name = format!("refs/forge/snapshots/{clean_ref_path}");
                    let update_ref = std::process::Command::new("git")
                        .args(["update-ref", &ref_name, commit_sha])
                        .current_dir(worktree)
                        .output()?;
                    if !update_ref.status.success() {
                        bail!(
                            "TASK_SNAPSHOT_REF_FAILED: git update-ref failed: {}",
                            String::from_utf8_lossy(&update_ref.stderr)
                        );
                    }
                }
            }
            if delivery_needs_commit {
                let candidate = snapshot_commit.as_deref()
                    .context("TASK_SNAPSHOT_NOT_FOUND: delivery candidate is missing")?;
                let receipt_path = Path::new(&snapshot_worktree).join(&snapshot_milestone_ref);
                let candidate_baseline = snapshot_baseline
                    .as_deref()
                    .context("TASK_CANDIDATE_BASELINE_INVALID: candidate baseline is missing")?;
                let (landed, rollback) = commit_candidate_snapshot(
                    Path::new(&snapshot_worktree),
                    delivery_task,
                    candidate,
                    candidate_baseline,
                    &snapshot_scope,
                    &receipt_path,
                )?;
                delivery_rollback = rollback;
                return Ok(Some(landed));
            }
            Ok(None)
        },
    );
    let task_result = match task_result {
        Ok(task) => {
            if let Some(rollback) = delivery_rollback.as_mut() {
                rollback.disarm();
            }
            task
        }
        Err(error) => {
            if let Some(rollback) = delivery_rollback.as_mut() {
                rollback.rollback().context("TASK_DELIVERY_ROLLBACK_FAILED")?;
            }
            return Err(error);
        }
    };
    let mut response = serde_json::to_value(task_result)?;
    if let Some(snapshot) = &evidence.commit {
        if let Some(obj) = response.as_object_mut() {
            let inspect_cmd = snapshot_inspect_cmd(snapshot);
            obj.insert(
                "snapshot".into(),
                serde_json::json!({
                    "commit": snapshot,
                    "stage": p.stage,
                    "scoped_files": task.spec.scope,
                    "inspect_cmd": inspect_cmd,
                }),
            );
        }
    }
    shorten_task_response_commits(&mut response);
    Ok(response)
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
