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
use std::path::Path;
mod workspaces;
use workspaces::Registry;
mod context;
use context::context_bundle;
mod blackboard;
mod gates;
mod git_snapshot;
mod lifecycle;

pub use blackboard::{blackboard, BlackboardAction, BlackboardRequest, BlackboardScope};
pub use lifecycle::{claim, manage, reopen, reset};

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
    /// Unique task identifier `{project}/{milestone_id}:{task_ref}`.
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
    /// Unique task identifier `{project}/{milestone_id}:{task_ref}`.
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
    let output = crate::core::git::git_command(root, &["rev-parse", "--git-common-dir"]).ok()?;
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
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok("unknown".into());
    };
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
                    &workspace.repository,
                    &workspace.project,
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
fn validate_authority(
    task: &crate::db::tasks_store::Task,
    root: &Path,
    repository: &str,
    project: &str,
) -> Result<()> {
    let path = confined_path(root, &task.milestone_ref)?;
    if !path.is_file() {
        bail!(
            "TASK_AUTHORITY_MISSING: milestone file '{}' is missing. Do: restore milestone file '{}'.",
            task.milestone_ref,
            task.milestone_ref
        );
    }
    let text = std::fs::read_to_string(path)?;
    let milestone = Milestone::parse_with_identity(&text, repository, project)?;
    let stored_parts = task
        .task_id
        .strip_prefix(&format!("{project}/"))
        .and_then(|suffix| suffix.split_once(':'));
    if let Some((stored_id, task_ref)) = stored_parts {
        if milestone.id != stored_id {
            bail!(
                "{}",
                crate::core::tasks::milestone_id_change_diagnostic(
                    &task.milestone_ref,
                    project,
                    stored_id,
                    &milestone.id,
                    task_ref,
                    true,
                )
            );
        }
    }
    let spec = milestone
        .tasks
        .iter()
        .find(|s| milestone.task_id(s) == task.task_id)
        .with_context(|| {
            format!(
                "TASK_AUTHORITY_MISSING: task '{}' not found in milestone document '{}'. Do: restore task '{}' in '{}'.",
                task.task_id,
                task.milestone_ref,
                task.task_id,
                task.milestone_ref
            )
        })?;
    if milestone.digest(spec)? != task.digest {
        bail!(
            "{}",
            crate::core::tasks::authority_gap_diagnostic(
                &milestone,
                spec,
                &task.task_id,
                &task.milestone_ref,
                task.contract_revision,
                task.authority_snapshot.as_ref(),
                &task.spec,
            )
        );
    }
    Ok(())
}

fn candidate_divergence_diagnostic(
    worktree: &Path,
    task_id: &str,
    scope: &[String],
    snapshot: &str,
) -> Result<String> {
    let short = &snapshot[..snapshot.len().min(7)];
    let diff_args = ["diff", "--name-status", "--no-renames", snapshot, "--"];
    let mut diff_args = diff_args.to_vec();
    diff_args.extend(scope.iter().map(String::as_str));
    let output = crate::core::git::git_command(worktree, &diff_args)?;
    if !output.status.success() {
        bail!("TASK_CANDIDATE_MISMATCH: could not inspect candidate paths");
    }
    let mut paths_with_status = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.replace('\t', " ").replace(['\r', '\n'], "\\n"))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    let untracked_args = ["ls-files", "--others", "--exclude-standard", "--"];
    let mut untracked_args = untracked_args.to_vec();
    untracked_args.extend(scope.iter().map(String::as_str));
    let output = crate::core::git::git_command(worktree, &untracked_args)?;
    if !output.status.success() {
        bail!("TASK_CANDIDATE_MISMATCH: could not inspect untracked candidate paths");
    }
    paths_with_status.extend(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|path| format!("?? {}", path.replace(['\r', '\n'], "\\n")))
            .filter(|path| path != "?? "),
    );
    let paths_with_status = if paths_with_status.is_empty() {
        "no changed scoped paths reported".to_owned()
    } else {
        paths_with_status.join(", ")
    };
    Ok(format!(
        "TASK_CANDIDATE_MISMATCH: task '{task_id}' diverged from snapshot {short}. {paths_with_status}. Do: restore those paths to snapshot {short}."
    ))
}

/// Performs store for task.
pub fn store_for_task(root: &Path, id: &str) -> Result<TasksStore> {
    let registry = Registry::load(root)?;
    registry.owner(id)?.open(&registry.database)
}

/// Performs submit.
pub fn submit(root: &Path, p: Submit) -> Result<Value> {
    let registry = Registry::load(root)?;
    let workspace = registry.owner(&p.task_id)?;
    let mut store = workspace.open(&registry.database)?;
    let observed_task = store.inspect(&p.task_id)?;
    let profile = store.profile_for_task(&observed_task)?;
    let gate_index = profile.index_of(&p.stage).with_context(|| {
        format!(
            "TASK_STAGE_UNKNOWN: '{}' is not in the active profile; choose one of: {}",
            p.stage,
            profile
                .gates
                .iter()
                .map(|gate| gate.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let gate = &profile.gates[gate_index];
    let is_delivery = gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Delivery);
    let _receipt_lock = if is_delivery {
        Some(lifecycle::lock_receipts(&registry.database)?)
    } else {
        None
    };
    let task = store.inspect(&p.task_id)?;
    if task.stage != observed_task.stage || task.profile_pin != observed_task.profile_pin {
        bail!("TASK_STALE_SUBMISSION: task '{}'. stage or profile_pin changed concurrently. Do: retry submission.", p.task_id);
    }
    if !p.evidence.is_object() {
        bail!("TASK_EVIDENCE_INVALID: task '{}'. evidence is not a valid JSON object. Do: pass evidence as a JSON object.", p.task_id);
    }
    let mut evidence: Evidence = serde_json::from_value(p.evidence)
        .context(format!("TASK_EVIDENCE_INVALID: task '{}'. evidence JSON does not match the evidence object. Do: pass the evidence fields required by stage '{}'.", p.task_id, p.stage))?;
    // This value is derived from the accepted snapshot, never from caller-supplied evidence.
    evidence.candidate_baseline_head = None;
    registry.check_worktree(workspace, Path::new(&evidence.worktree))?;
    if task.status != "completed" {
        validate_authority(
            &task,
            Path::new(&evidence.worktree),
            &workspace.repository,
            &workspace.project,
        )?;
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
        let (snapshot, baseline) =
            git_snapshot::capture_scoped_snapshot(worktree, &p.task_id, &task.spec.scope)?
                .context("TASK_SNAPSHOT_FAILED: snapshot capture requires a Git worktree")?;
        if explicit_commit
            .as_ref()
            .is_some_and(|commit| commit != &snapshot)
        {
            let supplied = explicit_commit.as_deref().unwrap_or("missing");
            bail!("TASK_CANDIDATE_MISMATCH: task '{}'. supplied commit {supplied} != scoped snapshot {snapshot}. Do: resubmit with commit {snapshot}.", p.task_id);
        }
        evidence.commit = Some(snapshot);
        evidence.candidate_baseline_head = Some(baseline);
        candidate_commit = evidence.commit.clone();
    } else if needs_candidate {
        let (candidate, candidate_baseline) = store.build_snapshot_candidate(&p.task_id)?.context(
            "TASK_SNAPSHOT_NOT_FOUND: no accepted candidate snapshot precedes this gate",
        )?;
        candidate_commit = Some(candidate.clone());
        evidence.candidate_baseline_head = Some(candidate_baseline.clone());
        if is_delivery && gate.auto_commit && p.action.name() == "pass" {
            if task.status == "completed" {
                let delivered = task
                    .receipt
                    .as_ref()
                    .and_then(|receipt| receipt.commit.as_deref())
                    .context("TASK_RECEIPT_INVALID: completed task has no delivery commit")?;
                if explicit_commit
                    .as_deref()
                    .is_some_and(|commit| commit != delivered)
                {
                    let supplied = explicit_commit.as_deref().unwrap_or("missing");
                    bail!("TASK_CANDIDATE_MISMATCH: task '{}'. supplied commit {supplied} != accepted receipt commit {delivered}. Do: resubmit with commit {delivered}.", p.task_id);
                }
                evidence.commit = Some(delivered.to_owned());
            } else {
                let head = crate::core::git::git_stdout(
                    worktree,
                    &["rev-parse", "--verify", "HEAD"],
                    "TASK_CANDIDATE_BASELINE_MISMATCH",
                )?;
                if head != candidate_baseline {
                    bail!("TASK_CANDIDATE_BASELINE_MISMATCH: HEAD '{head}' does not match candidate baseline '{candidate_baseline}'");
                }
                let (current, _) =
                    git_snapshot::capture_scoped_snapshot(worktree, &p.task_id, &task.spec.scope)?
                        .context(
                            "TASK_SNAPSHOT_FAILED: snapshot capture requires a Git worktree",
                        )?;
                if current != candidate {
                    bail!(
                        "{}",
                        candidate_divergence_diagnostic(
                            worktree,
                            &p.task_id,
                            &task.spec.scope,
                            &candidate
                        )?
                    );
                }
                evidence.commit = Some(candidate.clone());
            }
        } else {
            if explicit_commit
                .as_ref()
                .is_some_and(|commit| commit != &candidate)
            {
                let supplied = explicit_commit.as_deref().unwrap_or("missing");
                bail!("TASK_CANDIDATE_MISMATCH: task '{}'. supplied commit {supplied} != candidate snapshot {candidate}. Do: resubmit with commit {candidate}.", p.task_id);
            }
            evidence.commit = Some(candidate.clone());
            if matches!(
                gate.proof.kind(),
                Some(
                    crate::core::tasks::profile::ProofKind::Review
                        | crate::core::tasks::profile::ProofKind::Delivery
                )
            ) {
                let (current, _) =
                    git_snapshot::capture_scoped_snapshot(worktree, &p.task_id, &task.spec.scope)?
                        .context(
                            "TASK_SNAPSHOT_FAILED: snapshot capture requires a Git worktree",
                        )?;
                if current != candidate {
                    bail!(
                        "{}",
                        candidate_divergence_diagnostic(
                            worktree,
                            &p.task_id,
                            &task.spec.scope,
                            &candidate
                        )?
                    );
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
    let delivery_needs_commit =
        is_delivery && gate.auto_commit && p.action.name() == "pass" && task.status != "completed";
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
                    let repository =
                        crate::core::git::git_command(worktree, &["rev-parse", "--git-dir"])?;
                    if !repository.status.success() {
                        // Non-Git task workspaces have no repository ref to preserve.
                        return Ok(None);
                    }
                    let obj_exists =
                        crate::core::git::git_command(worktree, &["cat-file", "-e", commit_sha])?;
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
                    let update_ref = crate::core::git::git_command(
                        worktree,
                        &["update-ref", &ref_name, commit_sha],
                    )?;
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
                let (landed, rollback) = git_snapshot::commit_candidate_snapshot(
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
                rollback
                    .rollback()
                    .context("TASK_DELIVERY_ROLLBACK_FAILED")?;
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
#[cfg(test)]
mod git_snapshot_contract_tests {
    use super::git_snapshot::TemporaryGitIndex;

    #[test]
    fn temporary_git_index_is_removed_when_dropped() {
        let path = {
            let index = TemporaryGitIndex::create().expect("create temporary git index");
            let path = index.path().to_path_buf();
            assert!(path.exists());
            path
        };

        assert!(!path.exists());
    }
}
