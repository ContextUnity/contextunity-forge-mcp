use crate::{
    core::tasks::{confined_path, gates::Evidence, Milestone},
    db::tasks_store::{Task, TasksStore},
};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf}};
mod workspaces;
use workspaces::Registry;
mod context;
use context::context_bundle;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
/// Enumerates the supported status values.
pub enum Status {
    /// Represents the ready case.
    Ready,
    /// Represents the in progress case.
    InProgress,
    /// Represents the blocked case.
    Blocked,
    /// Represents the completed case.
    Completed,
    /// Represents the all case.
    All,
}
impl Status {
    /// Performs name.
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
/// Enumerates the supported stage values.
pub enum Stage {
    /// Represents the contract case.
    Contract,
    /// Represents the build case.
    Build,
    /// Represents the review case.
    Review,
    /// Represents the deliver case.
    Deliver,
}
impl Stage {
    /// Performs name.
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
/// Enumerates the supported action values.
pub enum Action {
    /// Represents the pass case.
    Pass,
    /// Represents the reject case.
    Reject,
}
impl Action {
    /// Performs name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Reject => "reject",
        }
    }
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Represents list data.
pub struct List {
    /// Optional repository value.
    pub repository: Option<String>,
    /// Optional milestone ref value.
    pub milestone_ref: Option<String>,
    /// Optional status value.
    pub status: Option<Status>,
    /// Optional stage value.
    pub stage: Option<Stage>,
}
#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
/// Represents claim data.
pub struct Claim {
    /// The task id value.
    pub task_id: String,
    /// The stage value.
    pub stage: String,
    /// The worker id value.
    pub worker_id: String,
    /// The worktree value.
    pub worktree: String,
    #[serde(default)]
    /// Whether to return an aggregated, zero-shot task context bundle.
    pub bundle: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Represents submit data.
pub struct Submit {
    /// The task id value.
    pub task_id: String,
    /// The stage value.
    pub stage: String,
    /// Structured evidence supplied directly by the caller.
    #[schemars(schema_with = "evidence_schema")]
    pub evidence: Value,
    /// The action value.
    pub action: Action,
    #[schemars(schema_with = "findings_schema")]
    /// Optional findings value.
    pub findings: Option<Value>,
}
fn findings_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::Map::new().into()
}
fn evidence_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::Map::from_iter([("type".into(), Value::String("object".into()))]).into()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
/// Enumerates the supported manage action values.
pub enum ManageAction {
    /// Represents the create case.
    Create,
    /// Represents the sync case.
    Sync,
    #[default]
    /// Represents the inspect case.
    Inspect,
    /// Represents the delete case.
    Delete,
    /// Represents the extend scope case.
    ExtendScope,
    /// Add an iterative subtask to an existing task.
    SubtaskAdd,
    /// Update a subtask's status and evidence.
    SubtaskUpdate,
    /// List subtasks for a task.
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
/// Represents manage data.
pub struct Manage {
    /// Optional workspace value.
    pub workspace: Option<String>,
    /// The action value.
    pub action: ManageAction,
    /// Optional task id value.
    pub task_id: Option<String>,
    /// Optional milestone ref value.
    pub milestone_ref: Option<String>,
    /// Optional task ref value.
    pub task_ref: Option<String>,
    /// Optional paths value.
    pub paths: Option<Vec<String>>,
    #[serde(default)]
    /// Whether force applies.
    pub force: bool,
    #[serde(default)]
    /// Optional subtask reference slug.
    pub subtask_ref: Option<String>,
    #[serde(default)]
    /// Optional subtask title or goal description.
    pub title: Option<String>,
    #[serde(default)]
    /// Optional subtask status: pending, in_progress, or completed.
    pub subtask_status: Option<String>,
    #[serde(default)]
    /// Optional subtask verification evidence or test command.
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
impl TaskSettings {
    fn load(root: &Path) -> Result<Self> {
        let config = std::fs::read_to_string(root.join("forge-mcp.yaml"))
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
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        if path == crate::cli::default_db(root) {
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
pub fn list(root: &Path, p: List) -> Result<Value> {
    let registry = Registry::load(root)?;
    let status = p.status.unwrap_or(Status::Ready).name();
    let mut tasks = Vec::new();
    let all_workspaces = p.repository.as_deref() == Some("all");
    let mut workspaces = serde_json::Map::new();
    let mut selected = registry.selected(p.repository.as_deref())?;
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
        selected.pop().context("task workspace missing")?.envelope(json!({"tasks":tasks}))
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
        let milestone_prefix = p.task_id.split_once(':').context("invalid task identity")?.0.to_string() + ":";
        let claimed_at = store.earliest_claim(&milestone_prefix)?.context("task claim timestamp missing")?;
        crate::engine::milestones::activate_on_claim(&worktree, &claimed.milestone_ref, claimed_at)
    })();
    if let Err(error) = activation {
        store.release_failed_claim(&p.task_id, claimed.claim_revision)
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
        store.release_failed_claim(&p.task_id, claimed.claim_revision)
            .context("failed to release claim after context error")?;
    }
    response
}
/// Performs store for task.
pub fn store_for_task(root: &Path, id: &str) -> Result<TasksStore> {
    let registry = Registry::load(root)?;
    registry.owner(id)?.open(&registry.database)
}

#[derive(Serialize, Deserialize)]
struct ResetJournal {
    task_id: String,
    milestone_ref: String,
    original_text: String,
    pid: u32,
    #[serde(default)]
    process_start: Option<u64>,
}

fn process_start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split_whitespace().nth(19)?.parse().ok()
}

fn reset_journal_path(root: &Path, id: &str) -> PathBuf {
    let digest = Sha256::digest(id.as_bytes());
    root.join(".forge/task-reset").join(format!("{}.json", hex::encode(digest)))
}

fn write_reset_journal(root: &Path, journal: &ResetJournal) -> Result<PathBuf> {
    let path = reset_journal_path(root, &journal.task_id);
    fs::create_dir_all(path.parent().context("reset journal directory missing")?)?;
    let bytes = serde_json::to_vec(journal)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)
        .context("TASK_RESET_IN_PROGRESS")?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error.into());
    }
    Ok(path)
}

/// Reconciles an interrupted Markdown-first reset before any task operation observes its state.
pub(super) fn recover_pending_resets(root: &Path, database: &Path) -> Result<()> {
    let directory = root.join(".forge/task-reset");
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let bytes = match fs::read(entry.path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let journal: ResetJournal = serde_json::from_slice(&bytes)?;
        if entry.path() != reset_journal_path(root, &journal.task_id) {
            bail!("TASK_RESET_JOURNAL_INVALID: task identity mismatch");
        }
        let owner_is_live = match journal.process_start {
            Some(start) => process_start_ticks(journal.pid) == Some(start),
            None => Path::new(&format!("/proc/{}", journal.pid)).exists(),
        };
        if owner_is_live {
            bail!("TASK_RESET_IN_PROGRESS: retry after receipt synchronization");
        }
        let mut identity = journal.task_id.split('/');
        let repository = identity.next().context("invalid reset journal task identity")?;
        let project = identity.next().context("invalid reset journal task identity")?;
        let store = TasksStore::open_project(database, repository, project)?;
        let task = store.inspect(&journal.task_id)?;
        if task.milestone_ref != journal.milestone_ref {
            bail!("TASK_RESET_JOURNAL_INVALID: milestone reference changed");
        }
        match task.status.as_str() {
            "completed" => {
                let path = confined_path(root, &journal.milestone_ref)?;
                crate::engine::milestones::atomic_replace(&path, journal.original_text.as_bytes())?;
            }
            "ready" => {}
            _ => bail!("TASK_RESET_JOURNAL_INVALID: unexpected task status"),
        }
        fs::remove_file(entry.path())?;
    }
    Ok(())
}
/// Resets or reopens a task back to ready state at contract gate, clearing terminal receipt and updating milestone text if needed.
pub fn reset(root: &Path, id: &str) -> Result<Task> {
    let registry = Registry::load(root)?;
    let workspace = registry.owner(id)?;
    let mut store = workspace.open(&registry.database)?;
    let mut original = None;
    let result = store.reset_with(id, |previous| {
        if previous.status != "completed" {
            return Ok(());
        }
        let milestone_path = crate::core::tasks::confined_path(&workspace.root, &previous.milestone_ref)?;
        let text = std::fs::read_to_string(&milestone_path)?;
        let task_ref = id.rsplit_once(':').map(|(_, r)| r).unwrap_or(id);
        let updated = crate::engine::milestones::clear_task_receipt(&text, task_ref)?;
        let journal = write_reset_journal(&workspace.root, &ResetJournal {
            task_id: id.to_owned(),
            milestone_ref: previous.milestone_ref.clone(),
            original_text: text.clone(),
            pid: std::process::id(),
            process_start: process_start_ticks(std::process::id()),
        })?;
        if let Err(error) = crate::engine::milestones::atomic_replace(&milestone_path, updated.as_bytes()) {
            fs::remove_file(&journal)?;
            return Err(error);
        }
        original = Some((milestone_path, text, journal));
        Ok(())
    });
    match result {
        Ok(task) => {
            if let Some((_, _, journal)) = original {
                fs::remove_file(journal)?;
            }
            Ok(task)
        }
        Err(error) => {
            if let Some((path, text, journal)) = original {
                crate::engine::milestones::atomic_replace(&path, text.as_bytes())
                    .context("TASK_RECEIPT_ROLLBACK_FAILED")?;
                fs::remove_file(journal)?;
            }
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
pub fn manage(root: &Path, p: Manage) -> Result<Value> {
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
                && (milestone.is_some() || workspace.is_some())
                && task_ref.is_none()
                && paths.is_none()
                && !p.force
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
        ManageAction::SubtaskUpdate => {
            id.is_some() && p.subtask_ref.is_some() && !p.force
        }
        ManageAction::SubtaskList => {
            id.is_some() && !p.force
        }
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
    let workspace = match id {
        Some(id) => registry.owner(id)?,
        None => {
            if let Some(ws) = workspace {
                registry.select(Some(ws))?
            } else if let Some(m) = milestone {
                let primary = registry.select(None)?;
                let inferred = crate::core::tasks::infer_project_from_path(Path::new(m));
                let text = std::fs::read_to_string(confined_path(&primary.root, m)?)?;
                let parsed = Milestone::parse_with_identity(
                    &text,
                    &primary.repository,
                    inferred.as_deref().unwrap_or(&primary.project),
                )?;
                registry.select(Some(&parsed.project))?
            } else {
                registry.select(None)?
            }
        }
    };
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
            let subtask = store.subtask_update(task_id, subtask_ref, status, p.evidence.as_deref())?;
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
