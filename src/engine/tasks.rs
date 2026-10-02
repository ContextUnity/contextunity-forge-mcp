use crate::{
    core::tasks::{confined_path, gates::Evidence, Milestone},
    db::tasks_store::TasksStore,
};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
mod workspaces;
use workspaces::Registry;

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
    /// Represents the build case.
    Build,
    /// Represents the review case.
    Review,
}
impl Stage {
    /// Performs name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Review => "review",
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
#[derive(Debug, Deserialize, JsonSchema)]
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
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Represents submit data.
pub struct Submit {
    /// The task id value.
    pub task_id: String,
    /// The stage value.
    pub stage: String,
    /// The evidence ref value.
    pub evidence_ref: String,
    /// The action value.
    pub action: Action,
    #[schemars(schema_with = "findings_schema")]
    /// Optional findings value.
    pub findings: Option<Value>,
}
fn findings_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::Map::new().into()
}
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Enumerates the supported manage action values.
pub enum ManageAction {
    /// Represents the create case.
    Create,
    /// Represents the sync case.
    Sync,
    /// Represents the inspect case.
    Inspect,
    /// Represents the delete case.
    Delete,
    /// Represents the extend scope case.
    ExtendScope,
}
#[derive(Debug, Deserialize, JsonSchema)]
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
}

#[derive(Deserialize)]
struct TaskSettings {
    #[serde(default = "default_tasks_db")]
    tasks_db: std::path::PathBuf,
    #[serde(default = "default_repository")]
    task_repository: String,
    task_project: Option<String>,
    #[serde(default)]
    linked_workspaces: Vec<workspaces::LinkedConfig>,
}
fn default_tasks_db() -> std::path::PathBuf {
    ".forge/tasks.sqlite".into()
}
fn default_repository() -> String {
    "forge-mcp".into()
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
    for workspace in registry.selected(p.repository.as_deref())? {
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
                tasks.push(workspace.envelope(serde_json::to_value(task)?)?);
            }
        }
    }
    Ok(json!({"tasks":tasks}))
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
    store.claim(&p.task_id, &p.stage, &p.worker_id, &p.worktree)?;
    workspace.envelope(store.inspect_details(&p.task_id)?)
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
    let task = store.inspect(&p.task_id)?;
    let previous: String = store
        .connection
        .query_row(
            "SELECT worktree FROM task_claims WHERE task_id=?1 ORDER BY revision DESC LIMIT 1",
            [&p.task_id],
            |r| r.get(0),
        )
        .context("TASK_STALE_SUBMISSION")?;
    let path = confined_path(
        Path::new(task.worktree.as_deref().unwrap_or(&previous)),
        &p.evidence_ref,
    )?;
    let evidence: Evidence = serde_yaml::from_str(&std::fs::read_to_string(path)?)?;
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
        ManageAction::Inspect => {
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
    };
    if !valid {
        bail!("TASK_SELECTOR_INVALID");
    }
    let registry = Registry::load(root)?;
    let workspace = match id {
        Some(id) => registry.owner(id)?,
        None => registry.select(workspace)?,
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
                let parsed = Milestone::parse_with_identity(
                    &std::fs::read_to_string(confined_path(&workspace.root, &reference)?)?,
                    &workspace.repository,
                    &workspace.project,
                )?;
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
            workspace.envelope(store.inspect_details(id.context("task_id required")?)?)
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
    }
}
