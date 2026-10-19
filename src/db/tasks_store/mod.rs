use crate::core::tasks::{
    authority_gap_diagnostic, milestone_id_change_diagnostic, shorten_task_response_commits,
    snapshot_inspect_cmd, AuthoritySnapshot, Milestone, Receipt, SubtaskSpec, TaskSpec,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod blackboard;
mod lifecycle;
mod subtasks;

pub(crate) use blackboard::clear_blackboard;
pub use blackboard::is_canonical_blackboard_topic;

/// The retention seconds value.
pub const RETENTION_SECONDS: i64 = 14 * 24 * 60 * 60;

#[derive(Debug, Serialize)]
/// Represents delete report data.
pub struct DeleteReport {
    /// The deleted ids value.
    pub deleted_ids: Vec<String>,
    /// The count value.
    pub count: usize,
    /// The revoked claims value.
    pub revoked_claims: usize,
    /// The mode value.
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents task data.
pub struct Task {
    /// The task id value.
    pub task_id: String,
    /// The milestone ref value.
    pub milestone_ref: String,
    /// The spec value.
    pub spec: TaskSpec,
    /// The digest value.
    pub digest: String,
    /// The contract revision value.
    pub contract_revision: u64,
    /// The claim revision value.
    pub claim_revision: u64,
    /// The status value.
    pub status: String,
    /// Active declarative gate identifier persisted in the task descriptor.
    #[serde(default)]
    pub stage: String,
    /// Longest dependency distance derived during milestone sync.
    #[serde(default)]
    pub wave_index: u64,
    /// Builder affinity retained while an active scope owner blocks this task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked_worker_id: Option<String>,
    /// Original worktree retained until the parked builder resumes successfully.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked_worktree: Option<String>,
    /// First active owner blocking a parked scope extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
    /// Effective pinned profile reference copied from task or milestone metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_pin: Option<String>,
    /// Digest-bound authority values captured at the last successful sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_snapshot: Option<AuthoritySnapshot>,
    /// Scope admitted by the original milestone contract, before later extensions.
    #[serde(default)]
    pub initial_scope: Vec<String>,
    /// Runtime position derived from `stage`; never persisted.
    #[serde(skip)]
    pub gate: usize,
    /// Optional worker id value.
    pub worker_id: Option<String>,
    /// Optional worktree value.
    pub worktree: Option<String>,
    /// Optional completed at value.
    pub completed_at: Option<i64>,
    /// Optional receipt value.
    pub receipt: Option<Receipt>,
    #[serde(default)]
    /// The applicable invariants value.
    pub applicable_invariants: Vec<String>,
}

/// A milestone-, task-, or subtask-scoped collaboration message persisted in the task store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardMessage {
    /// Monotonic message identifier.
    pub id: u64,
    /// Namespace-qualified milestone path.
    pub milestone_ref: String,
    /// Owning task identifier, or null for milestone-level messages.
    pub task_id: Option<String>,
    /// Subtask identifier, or null for milestone- and task-level messages.
    pub subtask_ref: Option<String>,
    /// Optional workflow gate that should receive this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// Posting worker identifier.
    pub author: String,
    /// Message category.
    pub topic: String,
    /// Text or serialized JSON payload; absent on list responses by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// Unix creation time.
    pub created_at: i64,
}

/// Fields needed to persist a scoped blackboard message.
#[derive(Debug, Clone, Copy)]
pub struct BlackboardPostInput<'a> {
    /// Namespace-qualified milestone reference.
    pub milestone_ref: &'a str,
    /// Owning task identifier, or none for a milestone message.
    pub task_id: Option<&'a str>,
    /// Subtask identifier, if the message belongs to a subtask.
    pub subtask_ref: Option<&'a str>,
    /// Optional gate that should receive the message.
    pub gate: Option<&'a str>,
    /// Posting worker identifier.
    pub author: &'a str,
    /// Canonical blackboard category.
    pub topic: &'a str,
    /// Text or serialized JSON message.
    pub payload: &'a str,
}

/// Filters and pagination for one scoped blackboard read.
#[derive(Debug, Clone, Copy)]
pub struct BlackboardPageRequest<'a> {
    /// Namespace-qualified milestone reference.
    pub milestone_ref: &'a str,
    /// Owning task identifier, or none for a milestone page.
    pub task_id: Option<&'a str>,
    /// Subtask identifier, if the page is scoped to a subtask.
    pub subtask_ref: Option<&'a str>,
    /// Optional canonical topic filter.
    pub topic: Option<&'a str>,
    /// Optional exact gate filter.
    pub gate: Option<&'a str>,
    /// Maximum number of messages in the page.
    pub limit: Option<usize>,
    /// Number of messages to skip before the page.
    pub offset: Option<usize>,
}

/// Canonical categories accepted by the runtime blackboard API.
pub const BLACKBOARD_TOPICS: [(&str, &str); 6] = [
    ("draft", "Contract drafts and proposed changes."),
    (
        "notes",
        "Build notes, proof observations, and general context.",
    ),
    ("findings", "Review findings and verified defects."),
    (
        "blockers",
        "Issues preventing the active work from proceeding.",
    ),
    (
        "decisions",
        "Decisions and trade-offs retained in the delivery receipt.",
    ),
    (
        "deferred",
        "Deferred findings retained in the delivery receipt.",
    ),
];

/// A bounded page of blackboard message summaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardPage {
    /// Messages on this page, without payloads.
    pub messages: Vec<BlackboardMessage>,
    /// Pagination details for this result set.
    pub pagination: BlackboardPagination,
}

/// Pagination details for a blackboard message page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardPagination {
    /// Maximum number of messages requested for a page.
    pub limit: usize,
    /// Number of messages skipped before this page.
    pub offset: usize,
    /// Whether another page is available.
    pub has_more: bool,
    /// Offset to use for the next page, if one exists.
    pub next_offset: Option<usize>,
}

/// Represents tasks store data.
pub struct TasksStore {
    /// The connection value.
    pub connection: Connection,
    pub(crate) repository: String,
    pub(crate) project: String,
    /// `{repository}/{project}/` blackboard scope prefix.
    namespace: String,
    /// `{project}/` prefix matched against task ids.
    id_prefix: String,
    /// Active workflow gate profile.
    pub profile: std::sync::Arc<crate::core::tasks::profile::GateProfile>,
    workspace_root: Option<PathBuf>,
}

#[derive(Debug)]
/// Represents task already claimed data.
pub struct TaskAlreadyClaimed {
    /// Identifier of the task that already has an active claim.
    pub task_id: String,
    /// Worker identifier recorded by the active claim.
    pub worker_id: String,
    /// Worktree recorded by the active claim.
    pub worktree: String,
    /// Revision recorded by the active claim.
    pub revision: u64,
    /// Unix timestamp when the active claim was opened.
    pub claimed_at: i64,
}
impl std::fmt::Display for TaskAlreadyClaimed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TASK_ALREADY_CLAIMED: {}. Claimed by worker '{}' in worktree '{}' (revision {}, claimed at {}). Do: submit the active claim from worker '{}' before opening another.",
            self.task_id,
            self.worker_id,
            self.worktree,
            self.revision,
            self.claimed_at,
            self.worker_id
        )
    }
}
impl std::error::Error for TaskAlreadyClaimed {}

pub(crate) fn task_not_found_message(id: &str) -> String {
    format!("TASK_NOT_FOUND: task '{id}' not found in tasks database. Do: verify task id and sync milestone.")
}

fn stored_task_id_for_milestone(
    connection: &Connection,
    milestone_ref: &str,
    repository: &str,
    project: &str,
) -> Result<Option<String>> {
    let project_prefix = format!("{project}/");
    let namespace_prefix = format!("{repository}/{project}/");
    Ok(connection
        .query_row(
            "SELECT task_id FROM tasks \
             WHERE milestone_ref=?1 AND ( \
                 (json_extract(descriptor, '$.authority_snapshot.repository')=?2 \
                     AND json_extract(descriptor, '$.authority_snapshot.project')=?3) \
                 OR (json_extract(descriptor, '$.authority_snapshot.repository') IS NULL \
                     AND (substr(task_id, 1, length(?4))=?4 OR substr(task_id, 1, length(?5))=?5)) \
             ) ORDER BY task_id LIMIT 1",
            params![
                milestone_ref,
                repository,
                project,
                project_prefix,
                namespace_prefix
            ],
            |row| row.get(0),
        )
        .optional()?)
}

fn stored_task_parts<'a>(
    task_id: &'a str,
    repository: &str,
    project: &str,
) -> Option<(&'a str, &'a str)> {
    task_id
        .strip_prefix(&format!("{repository}/{project}/"))
        .or_else(|| task_id.strip_prefix(&format!("{project}/")))?
        .split_once(':')
}

fn protected_acdd_path(path: &Path) -> bool {
    path.starts_with(Path::new(".forge/acdd"))
}

fn verify_dirty_acdd_scope(root: &Path, initial_scope: &[String]) -> Result<()> {
    if !root.join(".git").exists() {
        return Ok(());
    }
    let output = crate::core::git::git_command(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".forge/acdd",
        ],
    )
    .context("TASK_SCOPE_VIOLATION: unable to inspect protected ACDD paths")?;
    if !output.status.success() {
        bail!(
            "TASK_SCOPE_VIOLATION: unable to inspect protected ACDD paths: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for entry in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| entry.len() > 3)
    {
        let path = std::str::from_utf8(&entry[3..])
            .context("TASK_SCOPE_VIOLATION: invalid path in Git status")?;
        let dirty = Path::new(path);
        if protected_acdd_path(dirty)
            && !initial_scope
                .iter()
                .any(|scope| dirty.starts_with(Path::new(scope)))
        {
            bail!("TASK_SCOPE_VIOLATION: dirty protected path '{path}' must be present in the task's initial contract scope");
        }
    }
    Ok(())
}

fn resolve_task_profile(
    workspace_root: Option<&Path>,
    base_profile: &std::sync::Arc<crate::core::tasks::profile::GateProfile>,
    task: &Task,
) -> Result<std::sync::Arc<crate::core::tasks::profile::GateProfile>> {
    match task.profile_pin.as_deref() {
        Some(reference) => {
            let root = workspace_root.context(
                "TASK_PROFILE_RESOLUTION_FAILED: workspace root is unavailable for pinned profile",
            )?;
            crate::core::tasks::profile::load_pinned_over(root, reference, base_profile)
        }
        None => Ok(base_profile.clone()),
    }
}

/// Query the latest passed candidate snapshot baseline for a task from `task_gates`.
pub fn task_candidate_baseline(connection: &Connection, task_id: &str) -> Result<Option<String>> {
    let mut stmt = connection.prepare(
        "SELECT evidence FROM task_gates WHERE task_id=?1 AND state='passed' AND evidence LIKE '%candidate_baseline_head%' ORDER BY revision DESC LIMIT 1",
    )?;
    let evidence: Option<String> = stmt.query_row(params![task_id], |r| r.get(0)).optional()?;
    if let Some(ev) = evidence {
        let val: serde_json::Value = serde_json::from_str(&ev).with_context(|| {
            format!(
                "TASK_GATE_EVIDENCE_CORRUPTED: failed to parse evidence JSON for task '{task_id}'"
            )
        })?;
        let base = val
            .get("candidate_baseline_head")
            .and_then(|v| v.as_str())
            .with_context(|| format!("TASK_GATE_EVIDENCE_CORRUPTED: missing 'candidate_baseline_head' in evidence for task '{task_id}'"))?;
        return Ok(Some(base.to_owned()));
    }
    Ok(None)
}

fn task_belongs_to(repository: &str, project: &str, id: &str) -> bool {
    let Some((head, _)) = id.split_once(':') else {
        return false;
    };
    let segments: Vec<&str> = head
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    match segments.as_slice() {
        [project_name, _] => *project_name == project,
        [repository_name, project_name, _] => {
            *repository_name == repository && *project_name == project
        }
        _ => false,
    }
}
impl TasksStore {
    /// Performs open.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_project(path, "forge-mcp", "forge-mcp")
    }
    /// Performs open project.
    pub fn open_project(path: &Path, repository: &str, project: &str) -> Result<Self> {
        crate::core::tasks::valid_identity(repository)?;
        crate::core::tasks::valid_identity(project)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA busy_timeout=5000")?;
        let known:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='task_store_metadata')",[],|r|r.get(0))?;
        if known {
            let version: String = connection.query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )?;
            if version != "5" {
                bail!("unsupported tasks schema; use recovery tooling");
            }
        } else {
            let populated: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%')",
                [],
                |row| row.get(0),
            )?;
            if populated {
                bail!("tasks_db must be a dedicated task store");
            }
        }
        connection.execute_batch(
            "PRAGMA synchronous=NORMAL; PRAGMA mmap_size=268435456; PRAGMA foreign_keys=ON;",
        )?;
        // A supported initialized store needs only connection-local settings. Even
        // INSERT OR IGNORE takes the writer lock, blocking otherwise independent readers.
        if !known {
            connection.execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value INTEGER NOT NULL);
            INSERT OR IGNORE INTO meta VALUES('generation',0);
            CREATE TABLE IF NOT EXISTS task_store_metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            INSERT OR IGNORE INTO task_store_metadata VALUES('schema_version','5');
            CREATE TABLE IF NOT EXISTS task_revisions(task_id TEXT PRIMARY KEY,revision INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks(task_id TEXT PRIMARY KEY,milestone_ref TEXT NOT NULL,descriptor TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_claims(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,worker_id TEXT NOT NULL,worktree TEXT NOT NULL,claimed_at INTEGER NOT NULL,ended INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(task_id,revision));
            CREATE TABLE IF NOT EXISTS task_gates(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,gate TEXT NOT NULL,revision INTEGER NOT NULL,state TEXT NOT NULL,evidence TEXT,PRIMARY KEY(task_id,gate,revision));
            CREATE TABLE IF NOT EXISTS task_findings(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,findings TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_scope_paths(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,path TEXT NOT NULL,base_root TEXT NOT NULL,frozen INTEGER NOT NULL,PRIMARY KEY(task_id,path));
            CREATE TABLE IF NOT EXISTS task_dependencies(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,dependency_id TEXT NOT NULL,satisfied INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(task_id,dependency_id));
            CREATE TABLE IF NOT EXISTS task_submissions(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,result TEXT NOT NULL,PRIMARY KEY(task_id,revision));
            CREATE TABLE IF NOT EXISTS task_blackboard(id INTEGER PRIMARY KEY AUTOINCREMENT,milestone_ref TEXT NOT NULL,task_id TEXT REFERENCES tasks ON DELETE CASCADE,subtask_ref TEXT,gate TEXT,author TEXT NOT NULL,topic TEXT NOT NULL,payload TEXT NOT NULL,created_at INTEGER NOT NULL,CHECK(task_id IS NOT NULL OR subtask_ref IS NULL));
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_scope_created ON task_blackboard(milestone_ref, task_id, subtask_ref, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_task_created ON task_blackboard(task_id, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_gate_scope_created ON task_blackboard(milestone_ref, gate, task_id, subtask_ref, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_task_gate_created ON task_blackboard(task_id, gate, created_at DESC, id DESC);
            CREATE TABLE IF NOT EXISTS task_subtasks(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,subtask_ref TEXT NOT NULL,title TEXT NOT NULL,status TEXT NOT NULL,evidence TEXT,updated_at INTEGER NOT NULL,PRIMARY KEY(task_id,subtask_ref));
            CREATE INDEX IF NOT EXISTS idx_task_subtasks_task ON task_subtasks(task_id);
            CREATE TRIGGER IF NOT EXISTS task_insert_generation AFTER INSERT ON tasks BEGIN UPDATE meta SET value=value+1 WHERE key='generation'; END;
            CREATE TRIGGER IF NOT EXISTS task_update_generation AFTER UPDATE ON tasks BEGIN UPDATE meta SET value=value+1 WHERE key='generation'; END;
            CREATE TRIGGER IF NOT EXISTS task_delete_generation AFTER DELETE ON tasks BEGIN UPDATE meta SET value=value+1 WHERE key='generation'; END;
            COMMIT;")?;
        }
        let version: String = connection.query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        if version != "5" {
            bail!("unsupported tasks schema; use recovery tooling");
        }
        Ok(Self {
            connection,
            repository: repository.to_owned(),
            project: project.to_owned(),
            namespace: format!("{repository}/{project}/"),
            id_prefix: format!("{project}/"),
            profile: crate::core::tasks::profile::compiled(),
            workspace_root: None,
        })
    }
    pub(crate) fn set_profile(
        &mut self,
        profile: std::sync::Arc<crate::core::tasks::profile::GateProfile>,
    ) {
        self.profile = profile;
    }
    pub(crate) fn set_workspace_root(&mut self, root: &Path) {
        self.workspace_root = Some(root.to_path_buf());
    }
    /// Resolve and validate the profile pinned to one task, falling back to the workspace profile.
    pub(crate) fn profile_for_task(
        &self,
        task: &Task,
    ) -> Result<std::sync::Arc<crate::core::tasks::profile::GateProfile>> {
        resolve_task_profile(self.workspace_root.as_deref(), &self.profile, task)
    }
    fn set_runtime_gate(
        &self,
        task: &mut Task,
        profile: &crate::core::tasks::profile::GateProfile,
    ) -> Result<()> {
        task.gate =
            profile.index_of(&task.stage).with_context(|| {
                format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        )
            })?;
        Ok(())
    }
    pub(crate) fn assert_id(&self, id: &str) -> Result<()> {
        if !self.task_id_in_project(id) {
            bail!("TASK_PROJECT_MISMATCH");
        }
        Ok(())
    }
    /// Accept `{project}/{milestone}:{task}` and legacy `{repository}/{project}/{milestone}:{task}`.
    fn task_id_in_project(&self, id: &str) -> bool {
        task_belongs_to(&self.repository, &self.project, id)
    }
    /// Qualify a relative milestone path for use as a shared-database blackboard key.
    pub fn milestone_scope_ref(&self, reference: &str) -> Result<String> {
        let reference = crate::core::tasks::relative_path(reference)?;
        Ok(format!("{}{}", self.namespace, reference.to_string_lossy()))
    }
    /// Read the candidate commit and its captured branch baseline from the latest snapshot gate.
    pub fn build_snapshot_candidate(&self, task_id: &str) -> Result<Option<(String, String)>> {
        self.assert_id(task_id)?;
        let task = self.inspect(task_id)?;
        let profile = self.profile_for_task(&task)?;
        let stage_index = profile.index_of(&task.stage).with_context(|| {
            format!(
                "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'",
                task.task_id, task.stage
            )
        })?;
        let snapshot_gate = profile.gates[..stage_index]
            .iter()
            .rev()
            .find(|gate| gate.sha_snapshot)
            .context(
                "TASK_SNAPSHOT_NOT_FOUND: no accepted candidate snapshot precedes this gate",
            )?;
        let evidence: Option<String> = self
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1")?
            .query_row(params![task_id, snapshot_gate.id], |r| r.get(0))
            .optional()?;
        Ok(evidence.and_then(|ev| {
            let evidence = serde_json::from_str::<serde_json::Value>(&ev).ok()?;
            Some((
                evidence.get("commit")?.as_str()?.to_owned(),
                evidence
                    .get("candidate_baseline_head")?
                    .as_str()?
                    .to_owned(),
            ))
        }))
    }
    /// Read the candidate SHA recorded for the latest preceding snapshot gate.
    pub fn build_snapshot_commit(&self, task_id: &str) -> Result<Option<String>> {
        Ok(self
            .build_snapshot_candidate(task_id)?
            .map(|(commit, _)| commit))
    }
    /// Performs inspect.
    pub fn inspect(&self, id: &str) -> Result<Task> {
        self.assert_id(id)?;
        let mut task = load(&self.connection, id)?.context(task_not_found_message(id))?;
        let profile = self.profile_for_task(&task)?;
        self.set_runtime_gate(&mut task, &profile)?;
        Ok(task)
    }
    /// Performs inspect details.
    pub fn inspect_details(&self, id: &str) -> Result<serde_json::Value> {
        let _snapshot = self.connection.unchecked_transaction()?;
        let mut task = self.inspect(id)?;
        let profile = self.profile_for_task(&task)?;
        self.set_runtime_gate(&mut task, &profile)?;
        let attempts:Vec<serde_json::Value>=self.connection.prepare("SELECT revision,worker_id,worktree,claimed_at,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")?.query_map([id],|r|Ok(serde_json::json!({"claim_revision":r.get::<_,u64>(0)?,"worker_id":r.get::<_,String>(1)?,"worktree":r.get::<_,String>(2)?,"claimed_at":r.get::<_,i64>(3)?,"ended":r.get::<_,bool>(4)?})))?.collect::<std::result::Result<_,_>>()?;
        let gates: Vec<serde_json::Value> = self
            .connection
            .prepare(
                "SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY revision,gate",
            )?
            .query_map([id], |r| {
                let gate_name: String = r.get(0)?;
                let revision: u64 = r.get(1)?;
                let state: String = r.get(2)?;
                let evidence_str: Option<String> = r.get(3)?;
                let commit = evidence_str.as_deref().and_then(|s| {
                    serde_json::from_str::<serde_json::Value>(s)
                        .ok()
                        .and_then(|v| {
                            v.get("commit").and_then(|c| c.as_str().map(String::from))
                        })
                });
                let inspect_cmd = commit.as_ref().map(|c| snapshot_inspect_cmd(c));
                Ok(serde_json::json!({
                    "stage": gate_name,
                    "claim_revision": revision,
                    "state": state,
                    "evidence": evidence_str,
                    "commit": commit,
                    "inspect_cmd": inspect_cmd,
                }))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let findings:Vec<serde_json::Value>=self.connection.prepare("SELECT revision,findings FROM task_findings WHERE task_id=?1 ORDER BY revision")?.query_map([id],|r|Ok(serde_json::json!({"claim_revision":r.get::<_,u64>(0)?,"findings":r.get::<_,String>(1)?})))?.collect::<std::result::Result<_,_>>()?;
        let mut value = serde_json::to_value(&task)?;
        let object = value
            .as_object_mut()
            .context("task envelope must be an object")?;
        object.insert("goal".into(), serde_json::json!(task.spec.target));
        object.insert(
            "allowed_write_scope".into(),
            serde_json::json!(task.spec.scope),
        );
        object.insert(
            "proof_policy".into(),
            serde_json::json!(task.spec.proof_policy),
        );
        object.insert("stage".into(), serde_json::json!(task.stage));
        object.insert("attempts".into(), serde_json::json!(attempts));
        object.insert("gates".into(), serde_json::json!(gates));
        let latest_snapshot = profile
            .gates
            .iter()
            .rev()
            .filter(|gate| gate.sha_snapshot)
            .find_map(|snapshot_gate| {
                let accepted = gates.iter().rev().find(|record| {
                    record.get("stage").and_then(serde_json::Value::as_str)
                        == Some(snapshot_gate.id.as_str())
                        && record.get("state").and_then(serde_json::Value::as_str) == Some("passed")
                })?;
                let commit = accepted.get("commit")?.as_str()?;
                Some(serde_json::json!({
                    "commit": commit,
                    "inspect_cmd": snapshot_inspect_cmd(commit),
                }))
            });
        if let Some(snapshot) = latest_snapshot {
            object.insert("latest_snapshot".into(), snapshot);
        }
        object.insert("findings".into(), serde_json::json!(findings));
        object.insert("gate_states".into(),serde_json::json!(profile.gates.iter().map(|gate| gate.id.as_str()).enumerate().map(|(i,name)|serde_json::json!({"stage":name,"state":if i<task.gate || task.status=="completed" {"passed"} else if i==task.gate && task.status=="in_progress" {"in_progress"} else {"pending"}})).collect::<Vec<_>>()));
        object.insert("subtasks".into(), serde_json::json!(task.spec.subtasks));
        shorten_task_response_commits(&mut value);
        Ok(value)
    }
    /// Performs sync.
    pub fn sync(
        &mut self,
        milestone: &Milestone,
        reference: &str,
        root: &Path,
    ) -> Result<Vec<Task>> {
        self.sync_selected(milestone, reference, root, None)
    }
    /// Performs create.
    pub fn create(
        &mut self,
        milestone: &Milestone,
        reference: &str,
        root: &Path,
        task_ref: &str,
    ) -> Result<Vec<Task>> {
        self.sync_selected(milestone, reference, root, Some(task_ref))
    }
    /// Remove all persisted state for a cancelled milestone while retaining incoming blockers.
    pub fn prune_cancelled(&mut self, reference: &str) -> Result<usize> {
        let reference = crate::core::tasks::relative_path(reference)?;
        let relative_reference = reference.to_string_lossy();
        let milestone_scope_ref = self.milestone_scope_ref(&relative_reference)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let columns = {
            let mut statement = tx.prepare("PRAGMA table_info(task_blackboard)")?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            columns
        };
        if columns.iter().any(|column| column == "milestone_ref") {
            tx.execute(
                "DELETE FROM task_blackboard WHERE milestone_ref=?1",
                [&milestone_scope_ref],
            )?;
        }

        tx.execute(
            "DELETE FROM task_dependencies WHERE task_id IN \
             (SELECT task_id FROM tasks WHERE milestone_ref=?1 AND (substr(task_id,1,length(?2))=?2 OR substr(task_id,1,length(?3))=?3))",
            params![relative_reference.as_ref(), self.id_prefix, self.namespace],
        )?;
        let deleted = tx.execute(
            "DELETE FROM tasks WHERE milestone_ref=?1 AND (substr(task_id,1,length(?2))=?2 OR substr(task_id,1,length(?3))=?3)",
            params![relative_reference.as_ref(), self.id_prefix, self.namespace],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    fn is_repack_receipt_update(
        root: &Path,
        existing: &Option<Receipt>,
        imported: &Option<Receipt>,
        scope: &[String],
        milestone_ref: &str,
        baseline: Option<&str>,
    ) -> Result<bool> {
        let (Some(existing), Some(imported)) = (existing.as_ref(), imported.as_ref()) else {
            return Ok(false);
        };
        if !existing.matches_proofs(imported) {
            return Ok(false);
        }
        let Some(commit) = imported.commit.as_deref() else {
            return Ok(false);
        };
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(false);
        }
        Ok(crate::core::tasks::validate_commit_for_task(
            root,
            commit,
            scope,
            Some(milestone_ref),
            existing.commit.as_deref(),
            baseline,
        )
        .is_ok())
    }

    fn sync_selected(
        &mut self,
        milestone: &Milestone,
        reference: &str,
        root: &Path,
        task_ref: Option<&str>,
    ) -> Result<Vec<Task>> {
        self.set_workspace_root(root);
        if format!("{}/{}/", milestone.repository, milestone.project) != self.namespace {
            bail!("TASK_PROJECT_MISMATCH");
        }
        if milestone.status.as_deref().is_some_and(|status| {
            !matches!(status, "active" | "planned" | "completed" | "cancelled")
        }) {
            bail!("MILESTONE_STATUS_INVALID: expected active, planned, completed, or cancelled");
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(stored_task_id) =
            stored_task_id_for_milestone(&tx, reference, &milestone.repository, &milestone.project)?
        {
            if let Some((stored_id, task_ref)) =
                stored_task_parts(&stored_task_id, &milestone.repository, &milestone.project)
            {
                if milestone.id != stored_id {
                    bail!(
                        "{}",
                        milestone_id_change_diagnostic(
                            reference,
                            &milestone.project,
                            stored_id,
                            &milestone.id,
                            task_ref,
                            false,
                        )
                    );
                }
            }
        }
        if milestone.status.as_deref() == Some("cancelled") {
            drop(tx);
            self.prune_cancelled(reference)?;
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for spec in &milestone.tasks {
            if task_ref.is_some_and(|r| spec.task_ref != r) {
                continue;
            }
            let id = milestone.task_id(spec);
            let digest = milestone.digest(spec)?;
            let profile_pin = spec
                .acdd_profile
                .clone()
                .or_else(|| milestone.acdd_profile.clone());
            let task_profile = match profile_pin.as_deref() {
                Some(reference) => {
                    crate::core::tasks::profile::load_pinned_over(root, reference, &self.profile)?
                }
                None => self.profile.clone(),
            };
            if task_profile.gates.is_empty() {
                bail!("ACDD_PROFILE_INVALID: gates must not be empty");
            }
            if let Some(mut existing) = load(&tx, &id)? {
                if existing.digest == digest {
                    if spec.status.as_deref() == Some("completed") {
                        if existing.status != "completed" {
                            bail!("TASK_RECEIPT_INVALID: imported receipt differs from accepted completion");
                        }
                        if existing.receipt != spec.receipt {
                            let matches_proofs = existing
                                .receipt
                                .as_ref()
                                .zip(spec.receipt.as_ref())
                                .map(|(e, s)| e.matches_proofs(s))
                                .unwrap_or(false);
                            let doc_is_commitless = spec
                                .receipt
                                .as_ref()
                                .and_then(|r| r.commit.as_ref())
                                .is_none();
                            if matches_proofs && doc_is_commitless {
                                // The markdown receipt is commitless and matches all accepted proofs.
                                // Retain the existing SQLite receipt (with its commit) and proceed.
                            } else {
                                let baseline = task_candidate_baseline(&tx, &id)?;
                                if !Self::is_repack_receipt_update(
                                    root,
                                    &existing.receipt,
                                    &spec.receipt,
                                    &existing.spec.scope,
                                    &existing.milestone_ref,
                                    baseline.as_deref(),
                                )? {
                                    bail!("TASK_RECEIPT_INVALID: imported receipt differs from accepted completion");
                                }
                                existing.receipt = spec.receipt.clone();
                                existing.spec.receipt = spec.receipt.clone();
                            }
                        }
                    }
                    for sub in &spec.subtasks {
                        tx.execute(
                            "INSERT INTO task_subtasks(task_id, subtask_ref, title, status, evidence, updated_at) \
                             VALUES(?1, ?2, ?3, ?4, ?5, ?6) \
                             ON CONFLICT(task_id, subtask_ref) DO UPDATE SET title=excluded.title, updated_at=excluded.updated_at",
                            params![id, sub.subtask_ref, sub.title, sub.status, sub.evidence, now()],
                        )?;
                    }
                    let mut stmt = tx.prepare(
                        "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
                    )?;
                    let current_subtasks = stmt
                        .query_map([&id], |r| {
                            Ok(SubtaskSpec {
                                subtask_ref: r.get(0)?,
                                title: r.get(1)?,
                                status: r.get(2)?,
                                evidence: r.get(3)?,
                            })
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    existing.spec.subtasks = current_subtasks;
                    existing.milestone_ref = reference.into();
                    if existing.authority_snapshot.is_none() {
                        existing.authority_snapshot =
                            Some(AuthoritySnapshot::capture(milestone, spec));
                    }
                    save(&tx, &existing)?;
                    result.push(existing);
                    continue;
                }
                if spec.status.as_deref() == Some("completed")
                    || spec.contract_revision <= existing.contract_revision
                {
                    bail!(
                        "{}",
                        authority_gap_diagnostic(
                            milestone,
                            spec,
                            &id,
                            reference,
                            existing.contract_revision,
                            existing.authority_snapshot.as_ref(),
                            &existing.spec,
                        )
                    );
                }
                end_claim(&tx, &id)?;
                tx.execute("DELETE FROM task_scope_paths WHERE task_id=?1", [&id])?;
                tx.execute("DELETE FROM task_dependencies WHERE task_id=?1", [&id])?;
                let mut existing_subtasks = {
                    let mut stmt = tx.prepare(
                        "SELECT subtask_ref FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
                    )?;
                    let existing = stmt
                        .query_map([&id], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    existing
                };
                if existing_subtasks.is_empty() {
                    for subtask in &existing.spec.subtasks {
                        tx.execute(
                            "INSERT OR IGNORE INTO task_subtasks(task_id, subtask_ref, title, status, evidence, updated_at) \
                             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                            params![
                                id,
                                subtask.subtask_ref,
                                subtask.title,
                                subtask.status,
                                subtask.evidence,
                                now()
                            ],
                        )?;
                        existing_subtasks.push(subtask.subtask_ref.clone());
                    }
                }
                for subtask_ref in existing_subtasks {
                    if !spec
                        .subtasks
                        .iter()
                        .any(|subtask| subtask.subtask_ref == subtask_ref)
                    {
                        tx.execute(
                            "DELETE FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2",
                            params![id, subtask_ref],
                        )?;
                    }
                }
            }
            let completed = spec.status.as_deref() == Some("completed");
            if completed && !reference.split('/').any(|part| part == "archive") {
                crate::core::tasks::gates::validate_durable_receipt_with_profile(
                    spec.receipt
                        .as_ref()
                        .context("TASK_RECEIPT_INVALID: missing receipt")?,
                    spec.contract_revision,
                    &task_profile,
                )?;
            }
            let first_gate = task_profile.gate_id(0).to_owned();
            let last_index = task_profile.gates.len().saturating_sub(1);
            let last_gate = task_profile.gate_id(last_index).to_owned();
            let archived: Option<u64> = tx
                .query_row(
                    "SELECT revision FROM task_revisions WHERE task_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()?;
            if completed && archived.is_some() {
                result.push(Task {
                    task_id: id,
                    milestone_ref: reference.into(),
                    spec: spec.clone(),
                    digest,
                    contract_revision: spec.contract_revision,
                    claim_revision: archived.unwrap_or(0),
                    status: "completed".into(),
                    stage: last_gate.clone(),
                    wave_index: 0,
                    parked_worker_id: None,
                    parked_worktree: None,
                    blocked_by: None,
                    profile_pin: profile_pin.clone(),
                    authority_snapshot: Some(AuthoritySnapshot::capture(milestone, spec)),
                    initial_scope: spec.scope.clone(),
                    gate: last_index,
                    worker_id: None,
                    worktree: None,
                    completed_at: None,
                    receipt: spec.receipt.clone(),
                    applicable_invariants: milestone
                        .invariants
                        .iter()
                        .chain(&spec.invariants)
                        .cloned()
                        .collect(),
                });
                continue;
            }
            let revision = next_revision(&tx, &id)?;
            let mut task = Task {
                task_id: id.clone(),
                milestone_ref: reference.into(),
                spec: spec.clone(),
                digest,
                contract_revision: spec.contract_revision,
                claim_revision: revision,
                status: if completed { "completed" } else { "ready" }.into(),
                stage: if completed { last_gate } else { first_gate },
                wave_index: 0,
                parked_worker_id: None,
                parked_worktree: None,
                blocked_by: None,
                profile_pin: profile_pin.clone(),
                authority_snapshot: Some(AuthoritySnapshot::capture(milestone, spec)),
                initial_scope: spec.scope.clone(),
                gate: if completed { last_index } else { 0 },
                worker_id: None,
                worktree: None,
                completed_at: completed.then(now),
                receipt: spec.receipt.clone(),
                applicable_invariants: milestone
                    .invariants
                    .iter()
                    .chain(&spec.invariants)
                    .cloned()
                    .collect(),
            };
            save(&tx, &task)?;
            for sub in &spec.subtasks {
                tx.execute(
                    "INSERT INTO task_subtasks(task_id, subtask_ref, title, status, evidence, updated_at) \
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT(task_id, subtask_ref) DO UPDATE SET title=excluded.title, updated_at=excluded.updated_at",
                    params![id, sub.subtask_ref, sub.title, sub.status, sub.evidence, now()],
                )?;
            }
            {
                let mut stmt = tx.prepare(
                    "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
                )?;
                task.spec.subtasks = stmt
                    .query_map([&id], |row| {
                        Ok(SubtaskSpec {
                            subtask_ref: row.get(0)?,
                            title: row.get(1)?,
                            status: row.get(2)?,
                            evidence: row.get(3)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
            save(&tx, &task)?;
            for scope_root in &spec.scope_roots {
                crate::core::tasks::confined_path(root, scope_root)?;
            }
            for scope in &spec.scope {
                let path = crate::core::tasks::relative_path(scope)?;
                crate::core::tasks::confined_path(root, scope)?;
                let base = if !spec.scope_roots.is_empty() {
                    let matched = spec.scope_roots.iter().find(|sr| {
                        if let Ok(sr_rel) = crate::core::tasks::relative_path(sr) {
                            path.starts_with(&sr_rel)
                        } else {
                            false
                        }
                    });
                    if let Some(sr) = matched {
                        crate::core::tasks::relative_path(sr)?
                    } else {
                        bail!("TASK_SCOPE_INVALID: path '{scope}' is outside scope_roots");
                    }
                } else if scope.ends_with('/') || root.join(&path).is_dir() {
                    path.clone()
                } else {
                    let parent = path.parent().context("scope needs parent")?;
                    if parent.as_os_str().is_empty() {
                        path.clone()
                    } else {
                        parent.to_path_buf()
                    }
                };
                tx.execute(
                    "INSERT OR IGNORE INTO task_scope_paths VALUES(?1,?2,?3,1)",
                    params![id, path.to_string_lossy(), base.to_string_lossy()],
                )?;
            }
            for dep in &spec.depends_on {
                if milestone.tasks.iter().any(|t| t.task_ref == *dep) {
                    let dependency_id = format!("{}/{}:{}", milestone.project, milestone.id, dep);
                    let satisfied = milestone
                        .tasks
                        .iter()
                        .any(|t| t.task_ref == *dep && t.status.as_deref() == Some("completed"));
                    tx.execute(
                        "INSERT INTO task_dependencies VALUES(?1,?2,?3)",
                        params![id, dependency_id, satisfied],
                    )?;
                } else if dep.contains(':') {
                    let dependency_id = if dep.contains('/') {
                        dep.clone()
                    } else {
                        format!("{}/{}", milestone.project, dep)
                    };
                    let satisfied: bool = tx
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM tasks WHERE task_id=?1 AND json_extract(descriptor, '$.status')='completed')",
                            params![dependency_id],
                            |r| r.get(0),
                        )
                        .unwrap_or(false);
                    tx.execute(
                        "INSERT INTO task_dependencies VALUES(?1,?2,?3)",
                        params![id, dependency_id, satisfied],
                    )?;
                } else {
                    let prefix1 = format!("{}/{}:%", milestone.project, dep);
                    let prefix2 = format!("{}:%", dep);
                    let mut dep_tasks: Vec<(String, bool)> = Vec::new();
                    {
                        let mut stmt = tx.prepare(
                            "SELECT task_id, json_extract(descriptor, '$.status')='completed' FROM tasks WHERE task_id LIKE ?1 OR task_id LIKE ?2"
                        )?;
                        let rows = stmt.query_map(params![prefix1, prefix2], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
                        })?;
                        for r in rows {
                            dep_tasks.push(r?);
                        }
                    }
                    if dep_tasks.is_empty() {
                        let dependency_id = format!("{}/{}:__milestone__", milestone.project, dep);
                        tx.execute(
                            "INSERT INTO task_dependencies VALUES(?1,?2,0)",
                            params![id, dependency_id],
                        )?;
                    } else {
                        for (dep_task_id, satisfied) in dep_tasks {
                            tx.execute(
                                "INSERT INTO task_dependencies VALUES(?1,?2,?3)",
                                params![id, dep_task_id, satisfied],
                            )?;
                        }
                    }
                }
            }
            result.push(task);
        }
        for (index, predecessor) in milestone.tasks.iter().enumerate() {
            let predecessor_id = milestone.task_id(predecessor);
            let Some(owner) = load(&tx, &predecessor_id)? else {
                continue;
            };
            for successor in &milestone.tasks[index + 1..] {
                let successor_id = milestone.task_id(successor);
                if load(&tx, &successor_id)?.is_none() {
                    continue;
                }
                if predecessor.scope.iter().any(|left| {
                    successor.scope.iter().any(|right| {
                        crate::core::tasks::path_matches_scope_entry(left, right)
                            || crate::core::tasks::path_matches_scope_entry(right, left)
                    })
                }) {
                    tx.execute(
                        "INSERT OR IGNORE INTO task_dependencies VALUES(?1,?2,?3)",
                        params![successor_id, predecessor_id, owner.status == "completed"],
                    )?;
                }
            }
        }
        let mut waves = std::collections::BTreeMap::new();
        for task in &mut result {
            task.wave_index = dependency_wave(&tx, &task.task_id, &mut waves, &mut Vec::new())?;
            if load(&tx, &task.task_id)?.is_some() {
                save(&tx, task)?;
            }
        }
        tx.commit()?;
        Ok(result)
    }
    /// Performs list.
    pub fn list(
        &self,
        milestone: Option<&str>,
        status: &str,
        stage: Option<&str>,
    ) -> Result<Vec<Task>> {
        let _snapshot = self.connection.unchecked_transaction()?;
        let mut stmt = self.connection.prepare("SELECT descriptor FROM tasks WHERE (?1 IS NULL OR milestone_ref=?1) AND (substr(task_id,1,length(?2))=?2 OR substr(task_id,1,length(?3))=?3) ORDER BY milestone_ref,task_id")?;
        let mut result = Vec::new();
        let mut known_stage =
            stage.is_none_or(|requested| self.profile.index_of(requested).is_some());
        for item in stmt.query_map(params![milestone, self.id_prefix, self.namespace], |r| {
            r.get::<_, String>(0)
        })? {
            let mut task: Task = serde_json::from_str(&item?)?;
            if !self.task_id_in_project(&task.task_id) {
                continue;
            }
            let profile = self.profile_for_task(&task)?;
            self.set_runtime_gate(&mut task, &profile)?;
            if let Some(requested) = stage {
                known_stage |= profile.index_of(requested).is_some();
            }
            if task.status == "ready" && !dependencies_ready(&self.connection, &task.task_id)? {
                task.status = "blocked".into();
            }
            if status != "all" && task.status != status {
                continue;
            }
            if stage.is_some_and(|s| task.stage != s) {
                continue;
            }
            result.push(task);
        }
        if let Some(stage) = stage {
            if !known_stage {
                bail!(
                    "TASK_STAGE_UNKNOWN: '{stage}' is not in an active profile; choose one of: {}",
                    self.profile
                        .gates
                        .iter()
                        .map(|gate| gate.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Ok(result)
    }
    /// Returns the first accepted claim time for one milestone.
    pub fn earliest_claim(&self, milestone_prefix: &str) -> Result<Option<i64>> {
        Ok(self.connection.query_row(
            "SELECT MIN(c.claimed_at) FROM task_claims c JOIN tasks t ON t.task_id=c.task_id WHERE substr(t.task_id,1,length(?1))=?1 AND substr(t.task_id,1,length(?2))=?2",
            params![milestone_prefix, self.id_prefix], |row| row.get(0))?)
    }
    /// Moves a completed milestone's task references after verifying its exact task set.
    pub fn relocate_milestone(
        &mut self,
        milestone_prefix: &str,
        old_ref: &str,
        new_ref: &str,
        expected_ids: &[String],
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stmt = tx.prepare("SELECT descriptor FROM tasks WHERE substr(task_id,1,length(?1))=?1 AND substr(task_id,1,length(?2))=?2")?;
        let tasks: Vec<String> = stmt
            .query_map(params![milestone_prefix, self.id_prefix], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);
        let mut found_ids = Vec::new();
        for descriptor in tasks {
            let mut task: Task = serde_json::from_str(&descriptor)?;
            if task.milestone_ref != old_ref || task.status != "completed" {
                bail!("milestone task state changed during handoff");
            }
            found_ids.push(task.task_id.clone());
            task.milestone_ref = new_ref.into();
            save(&tx, &task)?;
        }
        found_ids.sort();
        let mut expected = expected_ids.to_vec();
        expected.sort();
        if found_ids != expected {
            bail!("milestone task set changed during handoff");
        }
        tx.commit()?;
        Ok(())
    }
    /// Relocate a completed milestone while replacing every task receipt with its landed commit.
    pub fn handoff_milestone(
        &mut self,
        milestone_prefix: &str,
        _old_ref: &str,
        new_ref: &str,
        expected_ids: &[String],
        receipts: &std::collections::BTreeMap<String, Receipt>,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let descriptors: Vec<String> = tx
            .prepare("SELECT descriptor FROM tasks WHERE substr(task_id,1,length(?1))=?1 AND substr(task_id,1,length(?2))=?2")?
            .query_map(params![milestone_prefix, self.id_prefix], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut found_ids = Vec::new();
        for descriptor in descriptors {
            let mut task: Task = serde_json::from_str(&descriptor)?;
            if task.status != "completed" {
                bail!("milestone task state changed during handoff");
            }
            let receipt = receipts
                .get(&task.task_id)
                .context("TASK_RECEIPT_INVALID: handoff receipt is missing for a completed task")?;
            if receipt.decision != "pass"
                || receipt.commit.as_ref().is_none_or(|commit| {
                    !matches!(commit.len(), 40 | 64)
                        || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            {
                bail!("TASK_RECEIPT_INVALID: handoff receipt must contain a landed Git SHA");
            }
            task.receipt = Some(receipt.clone());
            task.spec.receipt = Some(receipt.clone());
            task.milestone_ref = new_ref.into();
            found_ids.push(task.task_id.clone());
            save(&tx, &task)?;
        }
        found_ids.sort();
        let mut expected = expected_ids.to_vec();
        expected.sort();
        let mut receipt_ids = receipts.keys().cloned().collect::<Vec<_>>();
        receipt_ids.sort();
        if found_ids != expected || receipt_ids != expected {
            bail!("milestone task or receipt set changed during handoff");
        }
        tx.commit()?;
        Ok(())
    }
}
pub(crate) fn load(conn: &Connection, id: &str) -> Result<Option<Task>> {
    let value: Option<String> = conn
        .query_row("SELECT descriptor FROM tasks WHERE task_id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    value
        .map(|v| {
            let mut task: Task = serde_json::from_str(&v)?;
            task.gate = crate::core::tasks::profile::compiled()
                .index_of(&task.stage)
                .unwrap_or(usize::MAX);
            Ok(task)
        })
        .transpose()
}
pub(crate) fn save(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks(task_id,milestone_ref,descriptor) VALUES(?1,?2,?3) \
         ON CONFLICT(task_id) DO UPDATE SET milestone_ref=excluded.milestone_ref,descriptor=excluded.descriptor",
        params![task.task_id, task.milestone_ref, serde_json::to_string(task)?],
    )?;
    Ok(())
}

fn dependency_wave(
    conn: &Connection,
    id: &str,
    waves: &mut std::collections::BTreeMap<String, u64>,
    visiting: &mut Vec<String>,
) -> Result<u64> {
    if let Some(wave) = waves.get(id) {
        return Ok(*wave);
    }
    if visiting.iter().any(|ancestor| ancestor == id) {
        bail!("TASK_DEPENDENCY_CYCLE: {id}. Derived scope dependencies form a cycle. Do: order overlapping owners before their consumers.");
    }
    visiting.push(id.to_owned());
    let dependencies: Vec<String> = conn
        .prepare("SELECT dependency_id FROM task_dependencies WHERE task_id=?1")?
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut wave = 0;
    for dependency in dependencies {
        wave = wave.max(dependency_wave(conn, &dependency, waves, visiting)? + 1);
    }
    visiting.pop();
    waves.insert(id.to_owned(), wave);
    Ok(wave)
}
pub(crate) fn next_revision(conn: &Connection, id: &str) -> Result<u64> {
    Ok(conn.query_row("INSERT INTO task_revisions VALUES(?1,1) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1 RETURNING revision",[id],|r|r.get(0))?)
}
pub(crate) fn end_claim(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE task_claims SET ended=1 WHERE task_id=?1 AND ended=0",
        [id],
    )?;
    Ok(())
}
pub(crate) fn check_stage(task_id: &str, active_gate: &str, stage: &str) -> Result<()> {
    if active_gate != stage {
        bail!("TASK_STAGE_INVALID: task '{task_id}'. submitted stage '{stage}' does not match active stage '{active_gate}'. Do: submit for active stage '{active_gate}'.");
    }
    Ok(())
}
pub(crate) fn unsatisfied_dependencies(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT d.dependency_id FROM task_dependencies d \
         LEFT JOIN tasks t ON t.task_id=d.dependency_id \
         WHERE d.task_id=?1 AND d.satisfied=0 \
           AND (t.task_id IS NULL OR json_extract(t.descriptor,'$.status')!='completed') \
         ORDER BY d.dependency_id",
    )?;
    let dependencies = statement
        .query_map([id], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(dependencies)
}
fn dependencies_ready(conn: &Connection, id: &str) -> Result<bool> {
    Ok(unsatisfied_dependencies(conn, id)?.is_empty())
}
/// Performs now.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialized_task_store_opens_during_a_write_transaction() {
        let directory = std::env::temp_dir().join(format!(
            "forge-task-reader-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("tasks.sqlite");
        let mut writer = TasksStore::open(&path).unwrap();
        let tx = writer
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let opened = TasksStore::open(&path);
        tx.rollback().unwrap();
        let failure = opened.err();
        drop(writer);
        std::fs::remove_dir_all(directory).unwrap();
        assert!(
            failure.is_none(),
            "initialized store open requires no writer lock: {failure:?}"
        );
    }
}
