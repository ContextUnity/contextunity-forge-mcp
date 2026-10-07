use crate::{
    core::tasks::{confined_path, gates::Evidence, Milestone},
    db::tasks_store::{Task, TasksStore},
};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
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
/// ACDD lifecycle gate stage.
pub enum Stage {
    /// Contract specification and red-seam test proof gate.
    Contract,
    /// Implementation and green-test proof gate.
    Build,
    /// Independent review against contract, invariants, and contours.
    Review,
    /// Milestone delivery and durable completion receipt generation.
    Deliver,
}
impl Stage {
    /// String name of the stage.
    pub fn name(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::Build => "build",
            Self::Review => "review",
            Self::Deliver => "deliver",
        }
    }
}
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
/// Gate submission decision action.
pub enum Action {
    /// Pass gate criteria and advance to next stage (or deliver).
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
    /// Optional message category filter for read; required for post.
    pub topic: Option<String>,
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
    /// Filter by lifecycle stage: contract, build, review, or deliver.
    pub stage: Option<Stage>,
    /// Subtask detail level: compact by default or full to include titles and evidence.
    pub detail: Option<TaskListDetail>,
}
#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
/// Parameters for claiming a task lifecycle gate.
pub struct Claim {
    /// Unique task identifier in repository/project/milestone:task_ref format.
    pub task_id: String,
    /// Lifecycle gate to claim: 'contract/v1', 'build/v1', 'review/v1', or 'deliver/v1'.
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
    /// Active lifecycle gate being submitted: 'contract/v1', 'build/v1', 'review/v1', or 'deliver/v1'.
    pub stage: String,
    /// Claim-bound object: task_id, stage, claim_revision, contract_revision, worker_id, worktree, optional commit (captured automatically on contract/build if omitted, or inherited from build on review/deliver), and proof. proof has exactly one key. contract_proof: {seam_test_ref, red_exit_code}; red_exit_code must be > 0 unless proof_policy is direct-proof or deferred-final-test. test_proof: {command, exit_code, tests_passed, tests_failed, log?}; a pass needs a nonempty command, exit_code 0, tests_passed >= 1, tests_failed 0, and log <= 64 KiB. review_proof: {decision matching the action, contours}. contours must be exactly paths, claims, concurrency, project_isolation, and administration; each has boolean applicable and nonempty evidence.
    #[schemars(schema_with = "evidence_schema")]
    pub evidence: Value,
    /// Gate submission action: 'pass' to advance to next gate, or 'reject' to fail the gate.
    pub action: Action,
    #[serde(default)]
    #[schemars(schema_with = "findings_schema")]
    /// Optional JSON findings. Omit or null when unused. A review or delivery rejection records findings and returns the task to build.
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
        for mut task in store.list(p.milestone_ref.as_deref(), "all", p.stage.map(Stage::name))? {
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
                    crate::core::tasks::GATES[task.gate]
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
            Ok(details)
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
    Ok(ResolvedBlackboardContext {
        store,
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
                    Ok(ResolvedBlackboardContext {
                        store,
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
            Ok(ResolvedBlackboardContext {
                store,
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
            Ok(ResolvedBlackboardContext {
                store,
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
            if request.topic.is_none() || request.payload.is_none() {
                bail!("TASK_BLACKBOARD_INVALID: post requires topic and payload");
            }
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
            let id = context.store.blackboard_post_scoped(
                &context.milestone_ref,
                context.task_id.as_deref(),
                context.subtask_ref.as_deref(),
                author,
                request
                    .topic
                    .as_deref()
                    .context("TASK_BLACKBOARD_INVALID: post requires topic")?,
                request
                    .payload
                    .as_deref()
                    .context("TASK_BLACKBOARD_INVALID: post requires payload")?,
            )?;
            Ok(json!({"id":id}))
        }
        BlackboardAction::Read => {
            let context = resolve_blackboard_context(&registry, &request)?;
            let page = context.store.blackboard_page(
                &context.milestone_ref,
                context.task_id.as_deref(),
                context.subtask_ref.as_deref(),
                request.topic.as_deref(),
                request.limit,
                request.offset,
            )?;
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
fn capture_scoped_snapshot(
    worktree: &Path,
    task_id: &str,
    scope: &[String],
) -> Result<Option<String>> {
    let head_out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree)
        .output();
    let _head = match head_out {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                return Ok(None);
            }
            s
        }
        _ => return Ok(None),
    };

    static SNAPSHOT_INDEX_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temp_index_path = loop {
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
                break candidate;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };

    struct TempFileGuard(PathBuf);
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = TempFileGuard(temp_index_path.clone());
    let temp_index_str = temp_index_path.to_string_lossy().to_string();

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

    let is_tracked = |p: &str| -> bool {
        std::process::Command::new("git")
            .args(["ls-files", "--error-unmatch", "--", p])
            .current_dir(worktree)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let valid_paths: Vec<&str> = scope
        .iter()
        .map(|s| s.as_str())
        .filter(|p| worktree.join(p).exists() || is_tracked(p))
        .collect();

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

    Ok(Some(commit_sha))
}

/// Performs submit.
pub fn submit(root: &Path, p: Submit) -> Result<Value> {
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    let mut store = workspace.open(&registry.database)?;
    let _receipt_lock = if p.stage.split('/').next() == Some("deliver") {
        Some(lock_receipts(&registry.database)?)
    } else {
        None
    };
    let task = store.inspect(&p.task_id)?;
    if !p.evidence.is_object() {
        bail!("TASK_EVIDENCE_INVALID: evidence must be a JSON object");
    }
    let mut evidence: Evidence = serde_json::from_value(p.evidence)
        .context("TASK_EVIDENCE_INVALID: expected a structured JSON evidence object")?;
    registry.check_worktree(workspace, Path::new(&evidence.worktree))?;
    if task.status != "completed" {
        validate_authority(&task, Path::new(&evidence.worktree))?;
        registry.check_scope(
            workspace,
            &task.spec.scope,
            Some(Path::new(&evidence.worktree)),
        )?;
    }
    let had_explicit_commit = evidence.commit.is_some();
    if evidence.commit.is_none() {
        if matches!(p.stage.as_str(), "review/v1" | "deliver/v1") {
            evidence.commit = store.build_snapshot_commit(&p.task_id)?;
        } else {
            evidence.commit = capture_scoped_snapshot(
                Path::new(&evidence.worktree),
                &p.task_id,
                &task.spec.scope,
            )?;
        }
    }
    if !had_explicit_commit && matches!(p.stage.as_str(), "review/v1" | "deliver/v1") {
        if let Some(current_snap) =
            capture_scoped_snapshot(Path::new(&evidence.worktree), &p.task_id, &task.spec.scope)?
        {
            if Some(&current_snap) != evidence.commit.as_ref() {
                bail!("TASK_CANDIDATE_MISMATCH: worktree has diverged from accepted candidate");
            }
        }
    }
    let pin_snapshot =
        p.action.name() == "pass" && matches!(p.stage.as_str(), "contract/v1" | "build/v1");
    let snapshot_worktree = evidence.worktree.clone();
    let snapshot_task_id = p.task_id.clone();
    let snapshot_commit = evidence.commit.clone();
    let task_result = store.submit_with_pre_persist(
        &p.task_id,
        &p.stage,
        &evidence,
        p.action.name(),
        p.findings.as_ref(),
        || {
            if pin_snapshot {
                if let Some(commit_sha) = snapshot_commit.as_deref() {
                    let worktree = Path::new(&snapshot_worktree);
                    let repository = std::process::Command::new("git")
                        .args(["rev-parse", "--git-dir"])
                        .current_dir(worktree)
                        .output()?;
                    if !repository.status.success() {
                        // Non-Git task workspaces have no repository ref to preserve.
                        return Ok(());
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
            Ok(())
        },
    )?;
    let mut response = serde_json::to_value(task_result)?;
    if let Some(snapshot) = &evidence.commit {
        if let Some(obj) = response.as_object_mut() {
            let inspect_cmd = format!("git show {snapshot}");
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
