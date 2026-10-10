use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};
/// Implements gates support.
pub mod gates;
/// Declarative ACDD profile module.
pub mod profile;

/// The gates value.
pub const GATES: [&str; 4] = ["contract", "build", "review", "deliver"];

/// Format the agent-facing command for inspecting a candidate snapshot.
/// Durable evidence and receipts retain the full commit ID.
pub fn snapshot_inspect_cmd(commit: &str) -> String {
    format!("git show {}", short_candidate_commit(commit))
}

/// Abbreviate a candidate SHA for task responses shown to agents.
pub fn short_candidate_commit(commit: &str) -> String {
    if commit.len() >= 7 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        commit[..7].to_owned()
    } else {
        commit.to_owned()
    }
}

/// Shorten candidate IDs in agent-visible task responses without changing stored evidence.
pub fn shorten_task_response_commits(value: &mut serde_json::Value) {
    fn shorten(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(serde_json::Value::String(commit)) = object.get_mut("commit") {
                    *commit = short_candidate_commit(commit);
                }
                if let Some(serde_json::Value::String(evidence)) = object.get_mut("evidence") {
                    if let Ok(mut parsed) = serde_json::from_str(evidence) {
                        shorten(&mut parsed);
                        if let Ok(encoded) = serde_json::to_string(&parsed) {
                            *evidence = encoded;
                        }
                    }
                }
                for child in object.values_mut() {
                    shorten(child);
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(shorten),
            _ => {}
        }
    }

    shorten(value);
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
/// Represents receipt data.
pub struct Receipt {
    /// The optional commit value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
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
    /// Durable task context retained after the blackboard is pruned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollup: Option<ReceiptRollup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Durable outcomes copied from the task store at delivery.
pub struct ReceiptRollup {
    /// Invariants admitted for the delivered task.
    pub verified_invariants: Vec<String>,
    /// Durable task blackboard outcomes; new decision and deferred entries retain a topic prefix.
    pub architectural_notes: Vec<String>,
    /// Compact decision across the five review contours.
    pub review_summary: ReviewSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Accepted review decision and contour dispositions.
pub struct ReviewSummary {
    /// Overall independent review decision.
    pub decision: String,
    /// Each contour is accepted or not applicable.
    pub contours: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
/// Represents an iterative subtask under a parent task contract.
pub struct SubtaskSpec {
    /// The unique subtask reference slug within the task.
    pub subtask_ref: String,
    /// Description of the subtask goal.
    pub title: String,
    #[serde(default = "default_subtask_status")]
    /// Subtask status: pending, in_progress, or completed.
    pub status: String,
    #[serde(default)]
    /// Optional verification evidence or test command.
    pub evidence: Option<String>,
}

fn default_subtask_status() -> String {
    "pending".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
/// Represents an out-of-scope defect or deferred review finding recorded under tasks.
pub struct DeferredDefect {
    /// Unique defect or finding reference slug (e.g. 'DEFECT-001' or 'parser-multiline-attr').
    pub id: String,
    /// Source of discovery: e.g. 'review_findings', 'task_blackboard', or worker identifier.
    pub source: String,
    /// Summary of the defect or uncovered edge case.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Targeted path or module boundary.
    pub path: Option<String>,
    #[serde(default = "default_defect_disposition")]
    /// Disposition: 'deferred', 'subsequent_milestone', or 'rejected'.
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Additional context, reproduction notes, or rationale.
    pub notes: Option<String>,
}

fn default_defect_disposition() -> String {
    "deferred".into()
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
    /// Optional pinned ACDD profile override for this task (`path:sha256_prefix`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acdd_profile: Option<String>,
    /// The proof policy value.
    pub proof_policy: String,
    /// The scope value.
    pub scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// The pre-admitted boundary roots within which scope may be extended.
    pub scope_roots: Vec<String>,
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
    #[serde(default)]
    /// Optional iterative subtasks detailing and deepening this task.
    pub subtasks: Vec<SubtaskSpec>,
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
    /// Optional pinned ACDD profile override for every task in the milestone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acdd_profile: Option<String>,
    /// The invariants value.
    pub invariants: Vec<String>,
    /// The owners value.
    pub owners: Vec<String>,
    /// The depends on value.
    pub depends_on: Vec<String>,
    /// The tasks value.
    pub tasks: Vec<TaskSpec>,
    /// Optional milestone status (active, planned, completed, cancelled).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    /// Optional deferred out-of-scope defects or review findings recorded under tasks.
    pub deferred_defects: Vec<DeferredDefect>,
}

impl Milestone {
    /// Resolves a manifest's lifecycle status, applying the location-based default.
    pub fn status_from_frontmatter(text: &str, archived: bool) -> Result<&'static str> {
        let text = text.replace("\r\n", "\n");
        let body = text
            .strip_prefix("---\n")
            .context("milestone requires YAML frontmatter")?;
        let (header, _) = body
            .split_once("\n---\n")
            .context("unterminated frontmatter")?;
        let meta: serde_yaml::Value = serde_yaml::from_str(header)?;
        meta.as_mapping()
            .context("milestone frontmatter must be a mapping")?;
        let id = meta
            .get("id")
            .and_then(serde_yaml::Value::as_str)
            .context("milestone requires id")?;
        valid_identity(id)?;
        Ok(
            parse_lifecycle_status(&meta)?.unwrap_or(if archived {
                "completed"
            } else {
                "planned"
            }),
        )
    }

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
        let status = parse_lifecycle_status(&meta)?.map(str::to_owned);
        let acdd_profile = field("acdd_profile");
        if let Some(reference) = &acdd_profile {
            profile::parse_profile_reference(reference, true)?;
        }
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
        let mut deferred_defects: Vec<DeferredDefect> = meta
            .get("deferred_defects")
            .map(|v| serde_yaml::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        for defect in &deferred_defects {
            valid_identity(&defect.id)?;
            if defect.title.trim().is_empty() {
                bail!("deferred defect requires a non-empty title");
            }
            if let Some(path) = &defect.path {
                relative_path(path)?;
            }
        }
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
            if let Some(defects_val) = value.get("deferred_defects") {
                let parsed: Vec<DeferredDefect> = serde_yaml::from_value(defects_val.clone())?;
                for defect in &parsed {
                    valid_identity(&defect.id)?;
                    if defect.title.trim().is_empty() {
                        bail!("deferred defect requires a non-empty title");
                    }
                    if let Some(path) = &defect.path {
                        relative_path(path)?;
                    }
                }
                deferred_defects.extend(parsed);
                continue;
            }
            if value.get("task_ref").is_none() {
                continue;
            }
            let task: TaskSpec = serde_yaml::from_value(value)?;
            valid_identity(&task.task_ref)?;
            if let Some(reference) = &task.acdd_profile {
                profile::parse_profile_reference(reference, true)?;
            }
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
            for root in &task.scope_roots {
                relative_path(root)?;
            }
            if !task.scope_roots.is_empty() {
                for path in &task.scope {
                    let rel = relative_path(path)?;
                    let inside = task.scope_roots.iter().any(|r| {
                        if let Ok(rel_root) = relative_path(r) {
                            rel.starts_with(&rel_root)
                        } else {
                            false
                        }
                    });
                    if !inside {
                        bail!("TASK_SCOPE_INVALID: path '{path}' is outside scope_roots");
                    }
                }
            }
            if !ids.insert(task.task_ref.clone()) {
                bail!("duplicate task_ref");
            }
            let mut subtask_ids = BTreeSet::new();
            for subtask in &task.subtasks {
                valid_identity(&subtask.subtask_ref)?;
                if subtask.title.trim().is_empty() {
                    bail!("subtask requires a non-empty title");
                }
                if !matches!(
                    subtask.status.as_str(),
                    "pending" | "in_progress" | "completed"
                ) {
                    bail!("invalid subtask status; must be pending, in_progress, or completed");
                }
                if !subtask_ids.insert(subtask.subtask_ref.clone()) {
                    bail!("duplicate subtask_ref: {}", subtask.subtask_ref);
                }
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
            status,
            repository,
            project,
            acdd_profile,
            invariants,
            owners,
            depends_on,
            tasks,
            deferred_defects,
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
        spec.subtasks = Vec::new();
        let bytes = match self.acdd_profile.as_ref() {
            Some(profile) => serde_json::to_vec(&(
                &self.repository,
                &self.project,
                &self.id,
                &self.invariants,
                &self.owners,
                &self.depends_on,
                profile,
                spec,
            ))?,
            None => serde_json::to_vec(&(
                &self.repository,
                &self.project,
                &self.id,
                &self.invariants,
                &self.owners,
                &self.depends_on,
                spec,
            ))?,
        };
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

fn parse_lifecycle_status(meta: &serde_yaml::Value) -> Result<Option<&'static str>> {
    let Some(value) = meta.get("status") else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .context("MILESTONE_STATUS_INVALID: status must be a string")?;
    let status = match value {
        "active" => "active",
        "planned" => "planned",
        "completed" => "completed",
        "cancelled" => "cancelled",
        _ => bail!("MILESTONE_STATUS_INVALID: expected active, planned, completed, or cancelled"),
    };
    if status == "cancelled" {
        let reason = meta
            .get("closure")
            .and_then(|closure| closure.get("reason"))
            .and_then(serde_yaml::Value::as_str)
            .context(
                "MILESTONE_CLOSURE_REASON_REQUIRED: cancelled milestones require closure.reason",
            )?;
        if reason.trim().is_empty() {
            bail!("MILESTONE_CLOSURE_REASON_REQUIRED: closure.reason cannot be empty");
        }
    }
    Ok(Some(status))
}

pub(crate) fn valid_identity(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.~".contains(c))
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

/// Infers project name from a relative milestone or plan path structurally without arbitrary folder name whitelists.
///
/// In any project structure (monorepo, nested services, plugins, arbitrary folders):
/// - `docs/milestones/010.md` or `docs/plans/010.md` -> None (root project)
/// - `milestones/010.md` or `plans/010.md` -> None (root project)
/// - `<subproject>/docs/milestones/...` -> Some("<subproject>")
/// - `packages/api/docs/milestones/...` -> Some("packages.api")
///
/// Works universally regardless of how directories are named (extensions, packages, backend, custom_dir, etc.).
pub fn infer_project_from_path(path: &Path) -> Option<String> {
    let parts: Vec<_> = path.iter().map(|s| s.to_string_lossy()).collect();
    for (i, part) in parts.iter().enumerate() {
        if part == "milestones" || part == "plans" {
            let candidate_idx = if i > 0 && parts[i - 1] == "docs" {
                if i >= 2 {
                    Some(i - 2)
                } else {
                    None
                }
            } else if i > 0 {
                Some(i - 1)
            } else {
                None
            };

            if let Some(idx) = candidate_idx {
                let owners = &parts[..=idx];
                if owners
                    .iter()
                    .any(|part| part.is_empty() || part.as_ref() == "." || part.starts_with('.'))
                {
                    return None;
                }
                return Some(
                    owners
                        .iter()
                        .map(|part| {
                            let mut encoded = String::new();
                            for ch in part.chars() {
                                match ch {
                                    'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => {
                                        encoded.push(ch)
                                    }
                                    '.' => encoded.push_str("~d"),
                                    '~' => encoded.push_str("~~"),
                                    _ => encoded.push_str(&format!("~u{}~", ch as u32)),
                                }
                            }
                            encoded
                        })
                        .collect::<Vec<_>>()
                        .join("."),
                );
            }
        }
    }
    None
}
