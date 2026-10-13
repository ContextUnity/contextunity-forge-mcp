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

impl Receipt {
    /// Checks whether another receipt shares the exact same verification proofs,
    /// decision, contract revision, timestamp, and rollup, ignoring the Git commit pointer.
    pub fn matches_proofs(&self, other: &Self) -> bool {
        self.contract_revision == other.contract_revision
            && self.passed_at == other.passed_at
            && self.decision == other.decision
            && self.evidence == other.evidence
            && self.review == other.review
            && self.rollup == other.rollup
    }
}

/// Check if a path matches a scope entry (exact file or directory prefix).
pub fn path_matches_scope_entry(path: &str, scope_entry: &str) -> bool {
    let path = path.trim_matches('/').replace('\\', "/");
    let entry = scope_entry.trim_matches('/').replace('\\', "/");
    if entry.is_empty() || entry == "." {
        return true;
    }
    path == entry || path.starts_with(&format!("{entry}/"))
}

/// Normalizes a repository-relative path by stripping leading/trailing slashes and './'.
pub fn normalize_repo_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim();
    let p = p.strip_prefix("./").unwrap_or(p);
    p.trim_matches('/').to_string()
}

/// Check whether a path represents an internal Forge runtime/scratch artifact
/// (e.g. SQLite databases, locks, logs, tasks runtime directory, or checkpoints).
/// Project configuration paths under `.forge/acdd/**` and `.forge/frameworks/**`
/// are protected and are NEVER considered runtime files.
pub fn is_forge_runtime_file(path: &str) -> bool {
    let norm = normalize_repo_path(path);
    if norm.starts_with(".forge/acdd/")
        || norm == ".forge/acdd"
        || norm.starts_with(".forge/frameworks/")
        || norm == ".forge/frameworks"
    {
        return false;
    }
    if norm.starts_with(".forge-mcp/") || norm == ".forge-mcp" {
        return true;
    }
    if !norm.starts_with(".forge/") {
        return false;
    }
    norm.ends_with(".sqlite")
        || norm.contains(".sqlite-")
        || norm.ends_with(".lock")
        || norm.ends_with(".log")
        || norm.ends_with(".jsonl")
        || norm == ".forge/checkpoints.json"
        || norm.starts_with(".forge/tasks/")
        || norm == ".forge/tasks"
}

/// Verify that all files changed by a commit fall within the task's scope or milestone reference.
pub fn commit_files_within_scope(
    changed_files: &[String],
    scope: &[String],
    milestone_ref: Option<&str>,
) -> bool {
    let m_norm = milestone_ref.map(normalize_repo_path);
    changed_files.iter().all(|file| {
        let f_norm = normalize_repo_path(file);
        if is_forge_runtime_file(&f_norm) {
            return true;
        }
        if let Some(m_path) = &m_norm {
            if &f_norm == m_path {
                return true;
            }
        }
        scope.iter().any(|s| path_matches_scope_entry(file, s))
    })
}

/// Validate that a commit modifies files within task scope, actually touches task scope,
/// and is a valid progression from baseline and any accepted delivery commit.
pub fn validate_commit_for_task(
    worktree: &Path,
    commit: &str,
    scope: &[String],
    milestone_ref: Option<&str>,
    accepted_delivery: Option<&str>,
    baseline: Option<&str>,
) -> Result<()> {
    if !crate::core::git::git_commit_exists(worktree, commit)? {
        bail!(
            "TASK_COMMIT_NOT_FOUND: commit '{commit}' is not available in workspace Git repository"
        );
    }
    let changed = crate::core::git::git_commit_changed_files(worktree, commit)?;
    if changed.is_empty() {
        bail!("TASK_COMMIT_EMPTY: commit '{commit}' contains no changed files");
    }
    if !commit_files_within_scope(&changed, scope, milestone_ref) {
        bail!("TASK_COMMIT_OUTSIDE_SCOPE: commit '{commit}' changes files outside task scope");
    }
    let touches_scope = changed
        .iter()
        .any(|f| scope.iter().any(|s| path_matches_scope_entry(f, s)));
    if !touches_scope {
        bail!("TASK_COMMIT_IGNORES_SCOPE: commit '{commit}' does not touch task scope");
    }
    if let Some(accepted) = accepted_delivery {
        if commit != accepted {
            if crate::core::git::git_ancestor(worktree, commit, accepted)? {
                bail!("TASK_COMMIT_OBSOLETE: commit '{commit}' is an ancestor of accepted delivery '{accepted}'");
            }
            if crate::core::git::git_scoped_tree_changed(worktree, accepted, commit, scope)? {
                bail!("TASK_COMMIT_TREE_MISMATCH: commit '{commit}' scoped tree differs from accepted delivery '{accepted}'");
            }
        }
    }
    if let Some(base) = baseline {
        if commit == base || crate::core::git::git_ancestor(worktree, commit, base)? {
            bail!(
                "TASK_COMMIT_OBSOLETE: commit '{commit}' does not advance beyond baseline '{base}'"
            );
        }
        if commit != accepted_delivery.unwrap_or("")
            && !crate::core::git::git_ancestor(worktree, base, commit)?
        {
            bail!("TASK_COMMIT_UNRELATED: commit '{commit}' does not descend from task baseline '{base}'");
        }
    }
    Ok(())
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

/// Digest-bound authority fields captured when a task is admitted.
///
/// This snapshot exists only to explain a later digest mismatch. It does not
/// participate in `Milestone::digest`, whose serialized representation is a
/// durable compatibility boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySnapshot {
    /// The repository frontmatter value.
    pub repository: String,
    /// The project frontmatter value.
    pub project: String,
    /// The milestone id frontmatter value.
    pub milestone_id: String,
    /// The milestone invariants frontmatter value.
    pub milestone_invariants: Vec<String>,
    /// The milestone owners frontmatter value.
    pub milestone_owners: Vec<String>,
    /// The milestone dependencies frontmatter value.
    pub milestone_depends_on: Vec<String>,
    /// The optional milestone ACDD profile frontmatter value.
    pub milestone_acdd_profile: Option<String>,
    /// The task reference field.
    pub task_ref: String,
    /// The task target field.
    pub task_target: String,
    /// The optional task agent type field.
    pub task_agent_type: Option<String>,
    /// The optional task ACDD profile field.
    pub task_acdd_profile: Option<String>,
    /// The task proof policy field.
    pub task_proof_policy: String,
    /// The task scope field.
    pub task_scope: Vec<String>,
    /// The task scope roots field.
    pub task_scope_roots: Vec<String>,
    /// The task dependencies field.
    pub task_depends_on: Vec<String>,
    /// The task invariants field.
    pub task_invariants: Vec<String>,
}

impl AuthoritySnapshot {
    /// Capture the digest-bound values represented by one milestone/task pair.
    pub fn capture(milestone: &Milestone, task: &TaskSpec) -> Self {
        Self {
            repository: milestone.repository.clone(),
            project: milestone.project.clone(),
            milestone_id: milestone.id.clone(),
            milestone_invariants: milestone.invariants.clone(),
            milestone_owners: milestone.owners.clone(),
            milestone_depends_on: milestone.depends_on.clone(),
            milestone_acdd_profile: milestone.acdd_profile.clone(),
            task_ref: task.task_ref.clone(),
            task_target: task.target.clone(),
            task_agent_type: task.agent_type.clone(),
            task_acdd_profile: task.acdd_profile.clone(),
            task_proof_policy: task.proof_policy.clone(),
            task_scope: task.scope.clone(),
            task_scope_roots: task.scope_roots.clone(),
            task_depends_on: task.depends_on.clone(),
            task_invariants: task.invariants.clone(),
        }
    }
}

/// Format the milestone identity diagnostic used before task lookup/admission.
pub fn milestone_id_change_diagnostic(
    milestone_ref: &str,
    project: &str,
    stored_id: &str,
    file_id: &str,
    task_ref: &str,
    task_cannot_be_resolved: bool,
) -> String {
    let ending = if task_cannot_be_resolved {
        "task cannot be resolved"
    } else {
        "sync would admit them as new tasks"
    };
    format!(
        "AUTHORITY_GAP: milestone '{milestone_ref}'. milestone id changed from '{stored_id}' to '{file_id}'. Do: revert milestone id in {milestone_ref}. Task ids are {project}/{file_id}:{task_ref}; {ending}."
    )
}

/// Explain a digest mismatch by comparing admitted values with the current
/// milestone and task specification. The digest calculation itself stays
/// untouched and this helper never attributes `task.contract_revision`.
pub fn authority_gap_diagnostic(
    milestone: &Milestone,
    task: &TaskSpec,
    task_id: &str,
    milestone_ref: &str,
    stored_revision: u64,
    previous: Option<&AuthoritySnapshot>,
    previous_task: &TaskSpec,
) -> String {
    let current = AuthoritySnapshot::capture(milestone, task);
    let legacy = previous.is_none();
    let baseline = previous.cloned().unwrap_or_else(|| {
        let mut baseline = current.clone();
        baseline.task_ref = previous_task.task_ref.clone();
        baseline.task_target = previous_task.target.clone();
        baseline.task_agent_type = previous_task.agent_type.clone();
        baseline.task_acdd_profile = previous_task.acdd_profile.clone();
        baseline.task_proof_policy = previous_task.proof_policy.clone();
        baseline.task_scope = previous_task.scope.clone();
        baseline.task_scope_roots = previous_task.scope_roots.clone();
        baseline.task_depends_on = previous_task.depends_on.clone();
        baseline.task_invariants = previous_task.invariants.clone();
        baseline
    });

    let mut frontmatter_fields = Vec::new();
    if current.repository != baseline.repository {
        frontmatter_fields.push("milestone.repository");
    }
    if current.project != baseline.project {
        frontmatter_fields.push("milestone.project");
    }
    if current.milestone_invariants != baseline.milestone_invariants {
        frontmatter_fields.push("milestone.invariants");
    }
    if current.milestone_owners != baseline.milestone_owners {
        frontmatter_fields.push("milestone.owners");
    }
    if current.milestone_depends_on != baseline.milestone_depends_on {
        frontmatter_fields.push("milestone.depends_on");
    }
    if current.milestone_acdd_profile != baseline.milestone_acdd_profile {
        frontmatter_fields.push("milestone.acdd_profile");
    }
    if current.milestone_id != baseline.milestone_id {
        frontmatter_fields.push("milestone.id");
    }

    let mut task_fields = Vec::new();
    if current.task_target != baseline.task_target {
        task_fields.push("task.target");
    }
    if current.task_proof_policy != baseline.task_proof_policy {
        task_fields.push("task.proof_policy");
    }
    if current.task_scope != baseline.task_scope {
        task_fields.push("task.scope");
    }
    if current.task_scope_roots != baseline.task_scope_roots {
        task_fields.push("task.scope_roots");
    }
    if current.task_invariants != baseline.task_invariants {
        task_fields.push("task.invariants");
    }
    if current.task_depends_on != baseline.task_depends_on {
        task_fields.push("task.depends_on");
    }
    if current.task_acdd_profile != baseline.task_acdd_profile {
        task_fields.push("task.acdd_profile");
    }
    if current.task_ref != baseline.task_ref {
        task_fields.push("task.task_ref");
    }
    if current.task_agent_type != baseline.task_agent_type {
        task_fields.push("task.agent_type");
    }

    // Descriptors written before the authority snapshot was introduced cannot
    // recover the old owners/dependency/profile values. Attribute that legacy
    // gap to the complete milestone header group so the diagnostic still gives
    // an actionable repair path instead of reverting to an opaque code.
    if legacy && frontmatter_fields.is_empty() {
        frontmatter_fields.extend([
            "milestone.invariants",
            "milestone.owners",
            "milestone.depends_on",
            "milestone.acdd_profile",
        ]);
    }

    let higher_revision = task.contract_revision > stored_revision;
    let revision_facts = if higher_revision {
        format!(
            "file revision {} > stored {stored_revision} (unsynced)",
            task.contract_revision
        )
    } else {
        format!(
            "file revision {} <= stored {stored_revision}",
            task.contract_revision
        )
    };
    let frontmatter = frontmatter_fields.join(", ");
    let task_spec = task_fields.join(", ");
    let facts = match (frontmatter_fields.is_empty(), task_fields.is_empty()) {
        (false, false) => format!(
            "{revision_facts}; frontmatter fields [{frontmatter}], task fields [{task_spec}]"
        ),
        (false, true) => format!("{revision_facts}; digest fields [{frontmatter}]"),
        (true, false) => format!("{revision_facts}; digest fields [{task_spec}]"),
        (true, true) => format!("{revision_facts}; no attributed digest fields"),
    };
    let has_milestone_invariants = frontmatter_fields.contains(&"milestone.invariants");
    let action = if higher_revision {
        match (frontmatter_fields.is_empty(), task_fields.is_empty()) {
            (false, true) if has_milestone_invariants => format!(
                "revert [{frontmatter}] in {milestone_ref}, declare task invariants inside task.invariants, and run contextunity-forge-mcp task sync"
            ),
            (false, true) => format!(
                "revert [{frontmatter}] in {milestone_ref} and run contextunity-forge-mcp task sync"
            ),
            (true, false) => "run contextunity-forge-mcp task sync".to_owned(),
            (false, false) if has_milestone_invariants => format!(
                "revert [{frontmatter}] in {milestone_ref}, declare task invariants inside task.invariants, and run contextunity-forge-mcp task sync"
            ),
            (false, false) => format!(
                "revert [{frontmatter}] in {milestone_ref} and run contextunity-forge-mcp task sync"
            ),
            (true, true) => "run contextunity-forge-mcp task sync".to_owned(),
        }
    } else {
        match (frontmatter_fields.is_empty(), task_fields.is_empty()) {
            (false, true) if has_milestone_invariants => format!(
                "revert [{frontmatter}] in {milestone_ref}. Declare task invariants inside task.invariants"
            ),
            (false, true) => format!("revert [{frontmatter}] in {milestone_ref}"),
            (true, false) => format!(
                "set contract_revision above {stored_revision} and run contextunity-forge-mcp task sync"
            ),
            (false, false) if has_milestone_invariants => format!(
                "revert [{frontmatter}] in {milestone_ref}, declare task invariants inside task.invariants, set contract_revision above {stored_revision}, and run contextunity-forge-mcp task sync"
            ),
            (false, false) => format!(
                "revert [{frontmatter}] in {milestone_ref}, set contract_revision above {stored_revision}, and run contextunity-forge-mcp task sync"
            ),
            (true, true) => format!(
                "set contract_revision above {stored_revision} and run contextunity-forge-mcp task sync"
            ),
        }
    };
    format!("AUTHORITY_GAP: task '{task_id}'. {facts}. Do: {action}.")
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
        let depends_on: Vec<String> = meta
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
                if dependency == &task.task_ref {
                    bail!("invalid local task dependency");
                }
                if ids.contains(dependency) {
                    continue;
                }
                if let Some((milestone_or_proj, task_part)) = dependency.split_once(':') {
                    if task_part.trim().is_empty() {
                        bail!("invalid external task dependency");
                    }
                    valid_identity(task_part)?;
                    for segment in milestone_or_proj.split('/') {
                        valid_identity(segment)?;
                    }
                } else if depends_on.contains(dependency) {
                    valid_identity(dependency)?;
                } else {
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
            if let Some(task) = tasks.iter().find(|t| t.task_ref == id) {
                for dep in &task.depends_on {
                    visit(dep, tasks, active, done)?;
                }
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
    /// Task id `{project}/{milestone_id}:{task_ref}`.
    /// The workspace repository is not part of the id.
    pub fn task_id(&self, task: &TaskSpec) -> String {
        format!("{}/{}:{}", self.project, self.id, task.task_ref)
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
