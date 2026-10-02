use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};
/// Implements gates support.
pub mod gates;

/// The gates value.
pub const GATES: [&str; 5] = [
    "design/v1",
    "contract/v1",
    "build/v1",
    "review/v1",
    "handoff/v1",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
/// Represents receipt data.
pub struct Receipt {
    /// The commit value.
    pub commit: String,
    /// The contract revision value.
    pub contract_revision: u64,
    /// The passed at value.
    pub passed_at: String,
    /// The evidence value.
    pub evidence: serde_json::Value,
    /// The review value.
    pub review: serde_json::Value,
    /// The decision value.
    pub decision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Represents task spec data.
pub struct TaskSpec {
    /// The task ref value.
    pub task_ref: String,
    /// The target value.
    pub target: String,
    /// Optional agent specialization requested by the task contract.
    #[serde(default)]
    pub agent_type: Option<String>,
    /// The proof policy value.
    pub proof_policy: String,
    /// The scope value.
    pub scope: Vec<String>,
    #[serde(default = "initial_revision")]
    /// The contract revision value.
    pub contract_revision: u64,
    #[serde(default)]
    /// The depends on value.
    pub depends_on: Vec<String>,
    #[serde(default)]
    /// The invariants value.
    pub invariants: Vec<String>,
    #[serde(default)]
    /// Optional status value.
    pub status: Option<String>,
    #[serde(default)]
    /// Optional receipt value.
    pub receipt: Option<Receipt>,
}
fn initial_revision() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents milestone data.
pub struct Milestone {
    /// The id value.
    pub id: String,
    /// The repository value.
    pub repository: String,
    /// The project value.
    pub project: String,
    /// The invariants value.
    pub invariants: Vec<String>,
    /// The owners value.
    pub owners: Vec<String>,
    /// The depends on value.
    pub depends_on: Vec<String>,
    /// The tasks value.
    pub tasks: Vec<TaskSpec>,
}

impl Milestone {
    /// Performs parse.
    pub fn parse(text: &str, repository: &str) -> Result<Self> {
        Self::parse_specification(text, repository, None)
    }
    /// Parses with identity.
    pub fn parse_with_identity(text: &str, repository: &str, project: &str) -> Result<Self> {
        Self::parse_specification(text, repository, Some(project))
    }
    fn parse_specification(text: &str, repository: &str, project: Option<&str>) -> Result<Self> {
        let text = text.replace("\r\n", "\n");
        let body = text
            .strip_prefix("---\n")
            .context("milestone requires YAML frontmatter")?;
        let (header, body) = body
            .split_once("\n---\n")
            .context("unterminated frontmatter")?;
        let meta: serde_yaml::Value = serde_yaml::from_str(header)?;
        let field = |key: &str| meta[key].as_str().map(str::to_owned);
        let id = field("id").context("milestone requires id")?;
        let repository = field("repository").unwrap_or_else(|| repository.into());
        let project = field("project").unwrap_or_else(|| project.unwrap_or(&repository).into());
        for value in [&id, &repository, &project] {
            valid_identity(value)?;
        }
        let invariants = meta
            .get("invariants")
            .map(|v| serde_yaml::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        let owners = meta
            .get("owners")
            .map(|v| serde_yaml::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        let depends_on = meta
            .get("depends_on")
            .map(|v| serde_yaml::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        let mut tasks = Vec::new();
        let mut ids = BTreeSet::new();
        let mut lines = body.lines();
        while let Some(line) = lines.next() {
            if line.trim() != "```yaml" {
                continue;
            }
            let mut yaml = String::new();
            let mut closed = false;
            for line in lines.by_ref() {
                if line.trim() == "```" {
                    closed = true;
                    break;
                }
                yaml.push_str(line);
                yaml.push('\n');
            }
            if !closed {
                bail!("unterminated YAML block");
            }
            let value: serde_yaml::Value = serde_yaml::from_str(&yaml)?;
            if value.get("task_ref").is_none() {
                continue;
            }
            let task: TaskSpec = serde_yaml::from_value(value)?;
            valid_identity(&task.task_ref)?;
            if task.target.trim().is_empty()
                || task.scope.is_empty()
                || task.proof_policy.trim().is_empty()
            {
                bail!("incomplete task specification");
            }
            if task.contract_revision == 0 {
                bail!("contract_revision must be positive");
            }
            for path in &task.scope {
                relative_path(path)?;
            }
            if !ids.insert(task.task_ref.clone()) {
                bail!("duplicate task_ref");
            }
            tasks.push(task);
        }
        for task in &tasks {
            for dependency in &task.depends_on {
                if dependency == &task.task_ref || !ids.contains(dependency) {
                    bail!("invalid local task dependency");
                }
            }
        }
        fn visit<'a>(
            id: &'a str,
            tasks: &'a [TaskSpec],
            active: &mut BTreeSet<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Result<()> {
            if done.contains(id) {
                return Ok(());
            }
            if !active.insert(id) {
                bail!("cyclic task dependencies");
            }
            for dep in &tasks
                .iter()
                .find(|t| t.task_ref == id)
                .context("missing dependency")?
                .depends_on
            {
                visit(dep, tasks, active, done)?;
            }
            active.remove(id);
            done.insert(id);
            Ok(())
        }
        let mut done = BTreeSet::new();
        for task in &tasks {
            visit(&task.task_ref, &tasks, &mut BTreeSet::new(), &mut done)?;
        }
        Ok(Self {
            id,
            repository,
            project,
            invariants,
            owners,
            depends_on,
            tasks,
        })
    }
    /// Performs task id.
    pub fn task_id(&self, task: &TaskSpec) -> String {
        format!(
            "{}/{}/{}:{}",
            self.repository, self.project, self.id, task.task_ref
        )
    }
    /// Performs digest.
    pub fn digest(&self, task: &TaskSpec) -> Result<String> {
        let mut spec = task.clone();
        spec.receipt = None;
        spec.status = None;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
            &self.repository,
            &self.project,
            &self.id,
            &self.invariants,
            &self.owners,
            &self.depends_on,
            spec,
        ))?)))
    }
}

pub(crate) fn valid_identity(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        bail!("invalid identity");
    }
    Ok(())
}

/// Performs relative path.
pub fn relative_path(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("TASK_SCOPE_INVALID: expected relative path without traversal");
    }
    let normalized: PathBuf = path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .collect();
    if normalized.as_os_str().is_empty() {
        bail!("TASK_SCOPE_INVALID: empty normalized path");
    }
    Ok(normalized)
}

pub(crate) fn claim_worktree(value: &str) -> Result<PathBuf> {
    let path = Path::new(value)
        .canonicalize()
        .context("WORKTREE_NOT_FOUND: worktree directory must exist before claim")?;
    if !path.is_dir() {
        bail!("WORKTREE_NOT_FOUND: worktree directory must exist before claim");
    }
    Ok(path)
}

/// Performs confined path.
pub fn confined_path(root: &Path, value: &str) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let relative = relative_path(value)?;
    let path = root.join(relative);
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        if std::fs::symlink_metadata(ancestor).is_ok() {
            bail!("TASK_SCOPE_INVALID: dangling symlink");
        }
        ancestor = ancestor.parent().context("invalid path")?;
    }
    let suffix = path.strip_prefix(ancestor)?;
    let resolved = if suffix.as_os_str().is_empty() {
        ancestor.canonicalize()?
    } else {
        ancestor.canonicalize()?.join(suffix)
    };
    if !resolved.starts_with(&root) {
        bail!("TASK_SCOPE_INVALID: symlink escape");
    }
    Ok(resolved)
}
