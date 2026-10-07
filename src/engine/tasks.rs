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
    path::Path,
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
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Parameters for listing tasks in .forge/tasks.sqlite.
pub struct List {
    /// Repository or workspace name filter (e.g. 'forge-mcp', a linked workspace name, or 'all').
    pub repository: Option<String>,
    /// Filter tasks by milestone: relative file path (e.g. 'docs/milestones/030-*.md'), numeric prefix ('030'), milestone ID ('m-...'), or slug.
    pub milestone_ref: Option<String>,
    /// Filter by task status: ready (default), in_progress, blocked, completed, or all.
    pub status: Option<Status>,
    /// Filter by lifecycle stage: contract, build, review, or deliver.
    pub stage: Option<Stage>,
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
    /// Set true to return an aggregated zero-shot task context bundle (specification, guidance, blackboard history).
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
    /// Claim-bound object: task_id, stage, claim_revision, contract_revision, worker_id, worktree, commit, and proof. proof has exactly one key. contract_proof: {seam_test_ref, red_exit_code}; red_exit_code must be > 0 unless proof_policy is direct-proof or deferred-final-test. test_proof: {command, exit_code, tests_passed, tests_failed, log?}; a pass needs a nonempty command, exit_code 0, tests_passed >= 1, tests_failed 0, and log <= 64 KiB. review_proof: {decision matching the action, contours}. contours must be exactly paths, claims, concurrency, project_isolation, and administration; each has boolean applicable and nonempty evidence.
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
        for mut task in store.list(p.milestone_ref.as_deref(), "all", p.stage.map(Stage::name))? {
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
                    item["subtasks"] = json!(task.spec.subtasks);
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
        if p.bundle.unwrap_or(false) {
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
    let evidence: Evidence = serde_json::from_value(p.evidence)
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
    Ok(serde_json::to_value(store.submit(
        &p.task_id,
        &p.stage,
        &evidence,
        p.action.name(),
        p.findings.as_ref(),
    )?)?)
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
