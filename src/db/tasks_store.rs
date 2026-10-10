use crate::core::tasks::{
    shorten_task_response_commits, snapshot_inspect_cmd, Milestone, Receipt, SubtaskSpec, TaskSpec,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    /// Effective pinned profile reference copied from task or milestone metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_pin: Option<String>,
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

/// Return whether a topic is one of the canonical runtime categories.
pub fn is_canonical_blackboard_topic(topic: &str) -> bool {
    BLACKBOARD_TOPICS
        .iter()
        .any(|(canonical, _)| *canonical == topic)
}

fn validate_blackboard_topic(topic: &str) -> Result<()> {
    if is_canonical_blackboard_topic(topic) {
        Ok(())
    } else {
        let allowed = BLACKBOARD_TOPICS
            .iter()
            .map(|(canonical, _)| *canonical)
            .collect::<Vec<_>>()
            .join(", ");
        bail!("TASK_BLACKBOARD_TOPIC_INVALID: use one of the canonical topics: {allowed}");
    }
}

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
    namespace: String,
    /// Active workflow gate profile.
    pub profile: std::sync::Arc<crate::core::tasks::profile::GateProfile>,
    workspace_root: Option<PathBuf>,
}

#[derive(Debug)]
/// Represents task already claimed data.
pub struct TaskAlreadyClaimed;
impl std::fmt::Display for TaskAlreadyClaimed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TASK_ALREADY_CLAIMED")
    }
}
impl std::error::Error for TaskAlreadyClaimed {}

fn legacy_blackboard_namespace(task_id: &str) -> Result<String> {
    let mut parts = task_id.split('/');
    let repository = parts.next().unwrap_or_default();
    let project = parts.next().unwrap_or_default();
    let task = parts.next().unwrap_or_default();
    if repository.is_empty() || project.is_empty() || task.is_empty() || parts.next().is_some() {
        bail!("TASK_BLACKBOARD_MIGRATION_INVALID: malformed task identifier '{task_id}'");
    }
    let (milestone, task_ref) = task.split_once(':').with_context(|| {
        format!("TASK_BLACKBOARD_MIGRATION_INVALID: malformed task identifier '{task_id}'")
    })?;
    for identity in [repository, project, milestone, task_ref] {
        crate::core::tasks::valid_identity(identity).with_context(|| {
            format!("TASK_BLACKBOARD_MIGRATION_INVALID: malformed task identifier '{task_id}'")
        })?;
    }
    Ok(format!("{repository}/{project}/"))
}

fn migrate_blackboard_v1(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: String = tx.query_row(
        "SELECT value FROM task_store_metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version == "2" {
        tx.commit()?;
        return Ok(());
    }
    if version != "1" {
        bail!("unsupported tasks schema; use recovery tooling");
    }
    tx.execute_batch(
        "DROP INDEX IF EXISTS idx_task_blackboard_scope_created;
         DROP INDEX IF EXISTS idx_task_blackboard_task_created;
         ALTER TABLE task_blackboard RENAME TO task_blackboard_v1;
         CREATE TABLE task_blackboard(
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             milestone_ref TEXT NOT NULL,
             task_id TEXT REFERENCES tasks ON DELETE CASCADE,
             subtask_ref TEXT,
             author TEXT NOT NULL,
             topic TEXT NOT NULL,
             payload TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             CHECK(task_id IS NOT NULL OR subtask_ref IS NULL)
         );",
    )?;
    let legacy_rows = {
        let mut statement = tx.prepare(
            "SELECT b.id,b.task_id,b.author,b.topic,b.payload,b.created_at,t.milestone_ref \
             FROM task_blackboard_v1 b LEFT JOIN tasks t ON t.task_id=b.task_id \
             ORDER BY b.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (id, task_id, author, topic, payload, created_at, task_milestone_ref) in legacy_rows {
        let task_id = task_id.with_context(|| {
            format!("TASK_BLACKBOARD_MIGRATION_INVALID: message {id} has no task identifier")
        })?;
        let namespace = legacy_blackboard_namespace(&task_id)?;
        let milestone_ref = task_milestone_ref.with_context(|| {
            format!(
                "TASK_BLACKBOARD_MIGRATION_INVALID: message {id} refers to missing task '{task_id}'"
            )
        })?;
        let normalized_path = crate::core::tasks::relative_path(&milestone_ref)
            .with_context(|| {
                format!(
                    "TASK_BLACKBOARD_MIGRATION_INVALID: task '{task_id}' has an invalid milestone path"
                )
            })?;
        let milestone_ref = format!("{namespace}{}", normalized_path.to_string_lossy());
        tx.execute(
            "INSERT INTO task_blackboard(id,milestone_ref,task_id,subtask_ref,author,topic,payload,created_at) \
             VALUES(?1,?2,?3,NULL,?4,?5,?6,?7)",
            params![id, milestone_ref, task_id, author, topic, payload, created_at],
        )?;
    }
    tx.execute_batch(
        "DROP TABLE task_blackboard_v1;
         CREATE INDEX idx_task_blackboard_scope_created
             ON task_blackboard(milestone_ref,task_id,subtask_ref,created_at DESC,id DESC);
         CREATE INDEX idx_task_blackboard_task_created
             ON task_blackboard(task_id,created_at DESC,id DESC);
         UPDATE task_store_metadata SET value='2' WHERE key='schema_version';",
    )?;
    tx.commit()?;
    Ok(())
}

fn legacy_stage(index: u64) -> Result<&'static str> {
    ["contract", "build", "review", "deliver"]
        .get(index as usize)
        .copied()
        .context("unsupported legacy task gate index")
}

fn migrate_task_value(value: &mut serde_json::Value) -> Result<()> {
    let Some(object) = value.as_object_mut() else { return Ok(()); };
    if let Some(task) = object.get_mut("task") { migrate_task_value(task)?; }
    let is_task = object.contains_key("task_id") && object.contains_key("spec");
    if is_task {
        if let Some(stage) = object.get("stage").and_then(serde_json::Value::as_str) {
            let normalized = stage.strip_suffix("/v1").unwrap_or(stage).to_owned();
            object.insert("stage".into(), normalized.into());
        } else if let Some(index) = object.get("gate").and_then(serde_json::Value::as_u64) {
            object.insert("stage".into(), legacy_stage(index)?.into());
        } else {
            bail!("task schema v2 record has neither a stage nor a gate index");
        }
        object.remove("gate");
    } else if object.contains_key("claim_revision") && object.contains_key("proof") {
        if let Some(stage) = object.get("stage").and_then(serde_json::Value::as_str) {
            let normalized = stage.strip_suffix("/v1").unwrap_or(stage).to_owned();
            object.insert("stage".into(), normalized.into());
        }
    }
    if let Some(legacy) = object.remove("test_proof") {
        if object.contains_key("command_proof") { bail!("duplicate task proof during schema migration"); }
        object.insert("command_proof".into(), legacy);
    }
    for child in object.values_mut() { migrate_task_value(child)?; }
    Ok(())
}

fn set_migrated_initial_scope(value: &mut serde_json::Value, task_id: &str, scope: &[String]) {
    match value {
        serde_json::Value::Object(object) => {
            if object.get("task_id").and_then(serde_json::Value::as_str) == Some(task_id)
                && object.contains_key("spec")
            {
                object.insert("initial_scope".into(), serde_json::json!(scope));
            }
            for child in object.values_mut() {
                set_migrated_initial_scope(child, task_id, scope);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|child| set_migrated_initial_scope(child, task_id, scope)),
        _ => {}
    }
}

fn protected_acdd_path(path: &Path) -> bool {
    path.starts_with(Path::new(".forge/acdd"))
}

fn verify_dirty_acdd_scope(root: &Path, initial_scope: &[String]) -> Result<()> {
    if !root.join(".git").exists() {
        return Ok(());
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".forge/acdd"])
        .output()
        .context("TASK_SCOPE_VIOLATION: unable to inspect protected ACDD paths")?;
    if !output.status.success() {
        bail!("TASK_SCOPE_VIOLATION: unable to inspect protected ACDD paths: {}", String::from_utf8_lossy(&output.stderr));
    }
    for entry in output.stdout.split(|byte| *byte == 0).filter(|entry| entry.len() > 3) {
        let path = std::str::from_utf8(&entry[3..])
            .context("TASK_SCOPE_VIOLATION: invalid path in Git status")?;
        let dirty = Path::new(path);
        if protected_acdd_path(dirty)
            && !initial_scope.iter().any(|scope| dirty.starts_with(Path::new(scope)))
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

/// Reconcile one pre-v4 digest only when the active specification reproduces its stored authority.
fn reconcile_pre_v4_task_digest(
    connection: &Connection,
    task_id: &str,
    milestone: &Milestone,
    spec: &TaskSpec,
    digest: &str,
    existing: &mut Task,
) -> Result<()> {
    let (digest_version, stored_descriptor): (i64, String) = connection.query_row(
        "SELECT digest_version, descriptor FROM tasks WHERE task_id=?1",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if digest_version != 0 {
        return Ok(());
    }

    let previous_digest = milestone.pre_v4_digest(spec)?;
    if existing.contract_revision != spec.contract_revision
        || (existing.digest != previous_digest && existing.digest != digest)
    {
        return Ok(());
    }

    let mut descriptor: serde_json::Value = serde_json::from_str(&stored_descriptor)?;
    if descriptor.get("task_id").and_then(serde_json::Value::as_str) != Some(task_id)
        || descriptor.get("digest").and_then(serde_json::Value::as_str)
            != Some(existing.digest.as_str())
        || descriptor
            .get("contract_revision")
            .and_then(serde_json::Value::as_u64)
            != Some(existing.contract_revision)
    {
        bail!("TASK_DIGEST_MIGRATION_CONFLICT: persisted task authority changed");
    }

    if existing.digest != digest {
        descriptor["digest"] = digest.into();
    }
    let next_descriptor = serde_json::to_string(&descriptor)?;
    // Exact descriptor matching fences the digest, contract revision, claim state, and every
    // other persisted field while this immediate transaction updates the migration marker.
    let changed = connection.execute(
        "UPDATE tasks SET descriptor=?2,digest_version=1 \
         WHERE task_id=?1 AND digest_version=0 AND descriptor=?3",
        params![task_id, next_descriptor, stored_descriptor],
    )?;
    if changed != 1 {
        bail!("TASK_DIGEST_MIGRATION_CONFLICT: task row changed during migration");
    }
    existing.digest = digest.to_owned();
    Ok(())
}

/// Migrate all v2 runtime task records to string stages and the closed command proof key.
fn migrate_tasks_v2_to_v3(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: String = tx.query_row(
        "SELECT value FROM task_store_metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version == "3" { tx.commit()?; return Ok(()); }
    if version != "2" { bail!("unsupported tasks schema; use recovery tooling"); }

    let descriptors: Vec<(String, String)> = tx.prepare("SELECT task_id, descriptor FROM tasks")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, text) in descriptors {
        let mut value: serde_json::Value = serde_json::from_str(&text)?;
        migrate_task_value(&mut value)?;
        let initial_scope = tx.prepare("SELECT path FROM task_scope_paths WHERE task_id=?1 AND frozen=1 ORDER BY path")?
            .query_map([&id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        set_migrated_initial_scope(&mut value, &id, &initial_scope);
        tx.execute("UPDATE tasks SET descriptor=?2 WHERE task_id=?1", params![id, serde_json::to_string(&value)?])?;
    }

    let submissions: Vec<(String, u64, String)> = tx.prepare("SELECT task_id, revision, result FROM task_submissions")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, revision, text) in submissions {
        let mut value: serde_json::Value = serde_json::from_str(&text)?;
        migrate_task_value(&mut value)?;
        let initial_scope = tx.prepare("SELECT path FROM task_scope_paths WHERE task_id=?1 AND frozen=1 ORDER BY path")?
            .query_map([&id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        set_migrated_initial_scope(&mut value, &id, &initial_scope);
        tx.execute("UPDATE task_submissions SET result=?3 WHERE task_id=?1 AND revision=?2", params![id, revision, serde_json::to_string(&value)?])?;
    }

    let gate_rows: Vec<(String, String, u64, Option<String>)> = tx
        .prepare("SELECT task_id, gate, revision, evidence FROM task_gates")?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    for (id, gate, revision, evidence) in gate_rows {
        let normalized_gate = gate.strip_suffix("/v1").unwrap_or(&gate);
        let migrated_evidence = evidence
            .map(|text| -> Result<String> {
                let mut value: serde_json::Value = serde_json::from_str(&text)?;
                migrate_task_value(&mut value)?;
                Ok(serde_json::to_string(&value)?)
            })
            .transpose()?;
        tx.execute(
            "UPDATE task_gates SET gate=?4, evidence=?5 WHERE task_id=?1 AND gate=?2 AND revision=?3",
            params![id, gate, revision, normalized_gate, migrated_evidence],
        )?;
    }
    tx.execute("UPDATE task_store_metadata SET value='3' WHERE key='schema_version'", [])?;
    tx.commit()?;
    Ok(())
}

/// Mark existing v3 task digests for the one-time authority-preserving format migration.
fn migrate_task_digests_v3_to_v4(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: String = tx.query_row(
        "SELECT value FROM task_store_metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version == "4" {
        tx.commit()?;
        return Ok(());
    }
    if version != "3" {
        bail!("unsupported tasks schema; use recovery tooling");
    }

    let has_digest_version = {
        let mut statement = tx.prepare("PRAGMA table_info(tasks)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        columns.iter().any(|column| column == "digest_version")
    };
    if has_digest_version {
        tx.execute("UPDATE tasks SET digest_version=0", [])?;
    } else {
        tx.execute(
            "ALTER TABLE tasks ADD COLUMN digest_version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    tx.execute(
        "UPDATE task_store_metadata SET value='4' WHERE key='schema_version'",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// Add gate targeting and normalize historical blackboard topics once at schema v5.
fn migrate_blackboard_v4_to_v5(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: String = tx.query_row(
        "SELECT value FROM task_store_metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version == "5" {
        tx.commit()?;
        return Ok(());
    }
    if version != "4" {
        bail!("unsupported tasks schema; use recovery tooling");
    }

    tx.execute("ALTER TABLE task_blackboard ADD COLUMN gate TEXT", [])?;
    tx.execute_batch(
        "CREATE INDEX idx_task_blackboard_gate_scope_created
             ON task_blackboard(milestone_ref, gate, task_id, subtask_ref, created_at DESC, id DESC);
         CREATE INDEX idx_task_blackboard_task_gate_created
             ON task_blackboard(task_id, gate, created_at DESC, id DESC);
         UPDATE task_blackboard
         SET topic = CASE topic
             WHEN 'contract_draft' THEN 'draft'
             WHEN 'contract_findings' THEN 'findings'
             WHEN 'build_proof' THEN 'notes'
             WHEN 'architectural_notes' THEN 'decisions'
             WHEN 'draft' THEN 'draft'
             WHEN 'notes' THEN 'notes'
             WHEN 'findings' THEN 'findings'
             WHEN 'blockers' THEN 'blockers'
             WHEN 'decisions' THEN 'decisions'
             WHEN 'deferred' THEN 'deferred'
             ELSE 'notes'
         END;
         UPDATE task_store_metadata SET value='5' WHERE key='schema_version';",
    )?;
    tx.commit()?;
    Ok(())
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
        let mut connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA busy_timeout=5000")?;
        let known:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='task_store_metadata')",[],|r|r.get(0))?;
        if known {
            let version: String = connection.query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )?;
            match version.as_str() {
                "1" => migrate_blackboard_v1(&mut connection)?,
                "2" | "3" | "4" | "5" => {}
                _ => bail!("unsupported tasks schema; use recovery tooling"),
            }
            let version: String = connection.query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )?;
            if version == "2" { migrate_tasks_v2_to_v3(&mut connection)?; }
            let version: String = connection.query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )?;
            if version == "3" { migrate_task_digests_v3_to_v4(&mut connection)?; }
            let version: String = connection.query_row(
                "SELECT value FROM task_store_metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )?;
            if version == "4" {
                migrate_blackboard_v4_to_v5(&mut connection)?;
            }
        } else {
            let populated:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%')",[],|r|r.get(0))?;
            if populated {
                bail!("tasks_db must be a dedicated task store");
            }
        }
        connection.execute_batch("PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA mmap_size=268435456; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value INTEGER NOT NULL);
            INSERT OR IGNORE INTO meta VALUES('generation',0);
            CREATE TABLE IF NOT EXISTS task_store_metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            INSERT OR IGNORE INTO task_store_metadata VALUES('schema_version','5');
            CREATE TABLE IF NOT EXISTS task_revisions(task_id TEXT PRIMARY KEY,revision INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks(task_id TEXT PRIMARY KEY,milestone_ref TEXT NOT NULL,descriptor TEXT NOT NULL,digest_version INTEGER NOT NULL DEFAULT 1);
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
            CREATE TRIGGER IF NOT EXISTS task_delete_generation AFTER DELETE ON tasks BEGIN UPDATE meta SET value=value+1 WHERE key='generation'; END;")?;
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
            namespace: format!("{repository}/{project}/"),
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
        task.gate = profile.index_of(&task.stage).with_context(|| format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        ))?;
        Ok(())
    }
    pub(crate) fn assert_id(&self, id: &str) -> Result<()> {
        if !id.starts_with(&self.namespace) {
            bail!("TASK_PROJECT_MISMATCH");
        }
        Ok(())
    }
    /// Qualify a relative milestone path for use as a shared-database blackboard key.
    pub fn milestone_scope_ref(&self, reference: &str) -> Result<String> {
        let reference = crate::core::tasks::relative_path(reference)?;
        Ok(format!("{}{}", self.namespace, reference.to_string_lossy()))
    }
    /// Persist a message for one task and return its SQLite identifier.
    pub fn blackboard_post(
        &self,
        task_id: &str,
        author: &str,
        topic: &str,
        payload: &str,
    ) -> Result<u64> {
        self.assert_id(task_id)?;
        let task = self.inspect(task_id)?;
        let milestone_ref = self.milestone_scope_ref(&task.milestone_ref)?;
        self.blackboard_post_scoped(&milestone_ref, Some(task_id), None, author, topic, payload)
    }

    /// Persist a message at its resolved hierarchy level and return its SQLite identifier.
    pub fn blackboard_post_scoped(
        &self,
        milestone_ref: &str,
        task_id: Option<&str>,
        subtask_ref: Option<&str>,
        author: &str,
        topic: &str,
        payload: &str,
    ) -> Result<u64> {
        self.blackboard_post_scoped_with_gate(BlackboardPostInput {
            milestone_ref,
            task_id,
            subtask_ref,
            gate: None,
            author,
            topic,
            payload,
        })
    }

    /// Persist a message targeted to an optional workflow gate.
    pub fn blackboard_post_scoped_with_gate(&self, post: BlackboardPostInput<'_>) -> Result<u64> {
        let BlackboardPostInput {
            milestone_ref,
            task_id,
            subtask_ref,
            gate,
            author,
            topic,
            payload,
        } = post;
        self.assert_milestone_scope_ref(milestone_ref)?;
        if author.trim().is_empty() {
            bail!("TASK_BLACKBOARD_INVALID: author is required");
        }
        validate_blackboard_topic(topic)?;
        let task_milestone_ref = if let Some(task_id) = task_id {
            self.assert_id(task_id)?;
            let task = self.inspect(task_id)?;
            if self.milestone_scope_ref(&task.milestone_ref)? != milestone_ref {
                bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
            }
            if task.status == "completed" {
                bail!("TASK_TERMINAL");
            }
            if let Some(subtask_ref) = subtask_ref {
                if subtask_ref.trim().is_empty() {
                    bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
                }
                let exists: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2)",
                    params![task_id, subtask_ref],
                    |row| row.get(0),
                )?;
                if !exists {
                    bail!("TASK_SUBTASK_NOT_FOUND");
                }
            }
            Some(task.milestone_ref)
        } else if subtask_ref.is_some() {
            bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
        } else {
            None
        };
        let created_at = now();
        let inserted = if let (Some(task_id), Some(task_milestone_ref)) =
            (task_id, task_milestone_ref.as_deref())
        {
            self.connection.execute(
                "INSERT INTO task_blackboard(milestone_ref,task_id,subtask_ref,gate,author,topic,payload,created_at) \
                 SELECT ?1,?2,?3,?4,?5,?6,?7,?8 \
                 FROM tasks \
                 WHERE task_id=?2 AND milestone_ref=?9 \
                   AND json_extract(descriptor,'$.status')!='completed' \
                   AND (?3 IS NULL OR EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?2 AND subtask_ref=?3))",
                params![milestone_ref, task_id, subtask_ref, gate, author, topic, payload, created_at, task_milestone_ref],
            )?
        } else {
            self.connection.execute(
                "INSERT INTO task_blackboard(milestone_ref,task_id,subtask_ref,gate,author,topic,payload,created_at) \
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![milestone_ref, task_id, subtask_ref, gate, author, topic, payload, created_at],
            )?
        };
        if inserted == 0 {
            if let Some(task_id) = task_id {
                let task = self.inspect(task_id)?;
                if self.milestone_scope_ref(&task.milestone_ref)? != milestone_ref {
                    bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
                }
                if task.status == "completed" {
                    bail!("TASK_TERMINAL");
                }
                if let Some(subtask_ref) = subtask_ref {
                    let exists: bool = self.connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2)",
                        params![task_id, subtask_ref],
                        |row| row.get(0),
                    )?;
                    if !exists {
                        bail!("TASK_SUBTASK_NOT_FOUND");
                    }
                }
            }
            bail!("TASK_BLACKBOARD_STATE_CHANGED");
        }
        Ok(u64::try_from(self.connection.last_insert_rowid())?)
    }

    fn assert_milestone_scope_ref(&self, reference: &str) -> Result<()> {
        let Some(relative) = reference.strip_prefix(&self.namespace) else {
            bail!("TASK_PROJECT_MISMATCH");
        };
        if relative.is_empty() {
            bail!("TASK_BLACKBOARD_INVALID: milestone_ref is required");
        }
        crate::core::tasks::relative_path(relative)?;
        Ok(())
    }

    /// Read one bounded page of payload-free messages for a resolved hierarchy scope.
    pub fn blackboard_page(
        &self,
        milestone_ref: &str,
        task_id: Option<&str>,
        subtask_ref: Option<&str>,
        topic: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<BlackboardPage> {
        self.blackboard_page_for_gate(BlackboardPageRequest {
            milestone_ref,
            task_id,
            subtask_ref,
            topic,
            gate: None,
            limit,
            offset,
        })
    }

    /// Read one bounded page, optionally selecting messages for one workflow gate.
    pub fn blackboard_page_for_gate(
        &self,
        request: BlackboardPageRequest<'_>,
    ) -> Result<BlackboardPage> {
        let BlackboardPageRequest {
            milestone_ref,
            task_id,
            subtask_ref,
            topic,
            gate,
            limit,
            offset,
        } = request;
        self.assert_milestone_scope_ref(milestone_ref)?;
        if let Some(topic) = topic {
            validate_blackboard_topic(topic)?;
        }
        if let Some(task_id) = task_id {
            self.assert_id(task_id)?;
            let task = self.inspect(task_id)?;
            if self.milestone_scope_ref(&task.milestone_ref)? != milestone_ref {
                bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
            }
            if let Some(subtask_ref) = subtask_ref {
                let exists: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2)",
                    params![task_id, subtask_ref],
                    |row| row.get(0),
                )?;
                if !exists {
                    bail!("TASK_SUBTASK_NOT_FOUND");
                }
            }
        } else if subtask_ref.is_some() {
            bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
        }
        if subtask_ref.is_some_and(|reference| reference.trim().is_empty()) {
            bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
        }
        let limit = limit.unwrap_or(10);
        if limit == 0 {
            bail!("TASK_BLACKBOARD_INVALID: limit must be greater than zero");
        }
        let limit = limit.min(50);
        let offset = offset.unwrap_or(0);
        let sql_limit = i64::try_from(limit.checked_add(1).context("blackboard limit overflow")?)?;
        let sql_offset = i64::try_from(offset)?;
        let mut statement = self.connection.prepare(
            "SELECT id,milestone_ref,task_id,subtask_ref,gate,author,topic,created_at \
             FROM task_blackboard \
             WHERE milestone_ref=?1 \
               AND ((?2 IS NULL AND task_id IS NULL) OR \
                    (?2 IS NOT NULL AND task_id=?2 AND \
                     ((?3 IS NULL AND subtask_ref IS NULL) OR (?3 IS NOT NULL AND subtask_ref=?3)))) \
               AND (?4 IS NULL OR topic=?4) \
               AND (?5 IS NULL OR gate=?5) \
             ORDER BY created_at DESC,id DESC LIMIT ?6 OFFSET ?7",
        )?;
        let mut messages = statement
            .query_map(
                params![
                    milestone_ref,
                    task_id,
                    subtask_ref,
                    topic,
                    gate,
                    sql_limit,
                    sql_offset
                ],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        subtask_ref: row.get(3)?,
                        gate: row.get(4)?,
                        author: row.get(5)?,
                        topic: row.get(6)?,
                        payload: None,
                        created_at: row.get(7)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = messages.len() > limit;
        if has_more {
            messages.pop();
        }
        Ok(BlackboardPage {
            messages,
            pagination: BlackboardPagination {
                limit,
                offset,
                has_more,
                next_offset: has_more.then(|| offset.saturating_add(limit)),
            },
        })
    }

    /// Read one message, including its payload, if it belongs to this configured project.
    pub fn blackboard_inspect(&self, id: u64) -> Result<Option<BlackboardMessage>> {
        let message = self
            .connection
            .query_row(
                "SELECT id,milestone_ref,task_id,subtask_ref,gate,author,topic,payload,created_at \
                 FROM task_blackboard WHERE id=?1",
                [id],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        subtask_ref: row.get(3)?,
                        gate: row.get(4)?,
                        author: row.get(5)?,
                        topic: row.get(6)?,
                        payload: Some(row.get(7)?),
                        created_at: row.get(8)?,
                    })
                },
            )
            .optional()?;
        let Some(message) = message else {
            return Ok(None);
        };
        if !message.milestone_ref.starts_with(&self.namespace)
            || message
                .task_id
                .as_deref()
                .is_some_and(|task_id| !task_id.starts_with(&self.namespace))
        {
            return Ok(None);
        }
        Ok(Some(message))
    }

    /// Read messages in insertion order with an optional topic and result limit.
    pub fn blackboard_read(
        &self,
        task_id: &str,
        topic: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<BlackboardMessage>> {
        self.assert_id(task_id)?;
        if let Some(topic) = topic {
            validate_blackboard_topic(topic)?;
        }
        let limit = limit.map(i64::try_from).transpose()?.unwrap_or(i64::MAX);
        let mut statement = self.connection.prepare(
            "SELECT id,milestone_ref,task_id,subtask_ref,gate,author,topic,payload,created_at FROM task_blackboard \
             WHERE task_id=?1 AND (?2 IS NULL OR topic=?2) ORDER BY created_at,id LIMIT ?3",
        )?;
        let messages = statement
            .query_map(params![task_id, topic, limit], |row| {
                Ok(BlackboardMessage {
                    id: row.get(0)?,
                    milestone_ref: row.get(1)?,
                    task_id: row.get(2)?,
                    subtask_ref: row.get(3)?,
                    gate: row.get(4)?,
                    author: row.get(5)?,
                    topic: row.get(6)?,
                    payload: Some(row.get(7)?),
                    created_at: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(messages)
    }
    /// Delete all transient messages for one task.
    pub fn blackboard_clear(&self, task_id: &str) -> Result<usize> {
        self.assert_id(task_id)?;
        clear_blackboard(&self.connection, task_id)
    }
    /// Read the candidate commit and its captured branch baseline from the latest snapshot gate.
    pub fn build_snapshot_candidate(&self, task_id: &str) -> Result<Option<(String, String)>> {
        self.assert_id(task_id)?;
        let task = self.inspect(task_id)?;
        let profile = self.profile_for_task(&task)?;
        let stage_index = profile.index_of(&task.stage).with_context(|| format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'",
            task.task_id, task.stage
        ))?;
        let snapshot_gate = profile.gates[..stage_index]
            .iter()
            .rev()
            .find(|gate| gate.sha_snapshot)
            .context("TASK_SNAPSHOT_NOT_FOUND: no accepted candidate snapshot precedes this gate")?;
        let evidence: Option<String> = self
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1")?
            .query_row(params![task_id, snapshot_gate.id], |r| r.get(0))
            .optional()?;
        Ok(evidence.and_then(|ev| {
            let evidence = serde_json::from_str::<serde_json::Value>(&ev).ok()?;
            Some((
                evidence.get("commit")?.as_str()?.to_owned(),
                evidence.get("candidate_baseline_head")?.as_str()?.to_owned(),
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
        let mut task = load(&self.connection, id)?.context("TASK_NOT_FOUND")?;
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
                        && record.get("state").and_then(serde_json::Value::as_str)
                            == Some("passed")
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
    /// Add a subtask to an existing task.
    pub fn subtask_add(
        &mut self,
        task_id: &str,
        subtask_ref: &str,
        title: &str,
    ) -> Result<SubtaskSpec> {
        self.assert_id(task_id)?;
        crate::core::tasks::valid_identity(subtask_ref)?;
        let title = title.trim();
        if title.is_empty() {
            bail!("TASK_SUBTASK_TITLE_EMPTY: subtask title cannot be empty");
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut task = load(&tx, task_id)?.context("TASK_NOT_FOUND")?;
        if task.status == "completed" {
            bail!("TASK_TERMINAL: cannot add subtasks to completed task; reopen or reset the task first (e.g. `task reset <task-id>`)");
        }
        let updated_at = now();
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO task_subtasks(task_id, subtask_ref, title, status, evidence, updated_at) VALUES(?1, ?2, ?3, 'pending', NULL, ?4)",
            params![task_id, subtask_ref, title, updated_at],
        )?;
        if inserted == 0 {
            bail!(
                "TASK_SUBTASK_DUPLICATE: duplicate subtask_ref: {}",
                subtask_ref
            );
        }
        let mut stmt = tx.prepare(
            "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
        )?;
        let subtasks = stmt
            .query_map([task_id], |r| {
                Ok(SubtaskSpec {
                    subtask_ref: r.get(0)?,
                    title: r.get(1)?,
                    status: r.get(2)?,
                    evidence: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let result = subtasks
            .iter()
            .find(|s| s.subtask_ref == subtask_ref)
            .cloned()
            .context("subtask missing after insert")?;
        task.spec.subtasks = subtasks;
        drop(stmt);
        save(&tx, &task)?;
        tx.commit()?;
        Ok(result)
    }
    /// Update a subtask's status and evidence.
    pub fn subtask_update(
        &mut self,
        task_id: &str,
        subtask_ref: &str,
        status: &str,
        evidence: Option<&str>,
    ) -> Result<SubtaskSpec> {
        self.assert_id(task_id)?;
        if !matches!(status, "pending" | "in_progress" | "completed") {
            bail!("TASK_SUBTASK_STATUS_INVALID: invalid subtask status; must be pending, in_progress, or completed");
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut task = load(&tx, task_id)?.context("TASK_NOT_FOUND")?;
        if task.status == "completed" {
            bail!("TASK_TERMINAL: cannot update subtasks of completed task; reopen or reset the task first (e.g. `task reset <task-id>`)");
        }
        let updated_at = now();
        let updated = match evidence {
            Some(ev) => {
                let trimmed = ev.trim();
                let ev_value = if trimmed.is_empty() { None } else { Some(trimmed) };
                tx.execute(
                    "UPDATE task_subtasks SET status=?1, evidence=?2, updated_at=?3 WHERE task_id=?4 AND subtask_ref=?5",
                    params![status, ev_value, updated_at, task_id, subtask_ref],
                )?
            }
            None => {
                tx.execute(
                    "UPDATE task_subtasks SET status=?1, updated_at=?2 WHERE task_id=?3 AND subtask_ref=?4",
                    params![status, updated_at, task_id, subtask_ref],
                )?
            }
        };
        if updated == 0 {
            bail!("TASK_SUBTASK_NOT_FOUND: subtask '{subtask_ref}' not found");
        }
        let mut stmt = tx.prepare(
            "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
        )?;
        let subtasks = stmt
            .query_map([task_id], |r| {
                Ok(SubtaskSpec {
                    subtask_ref: r.get(0)?,
                    title: r.get(1)?,
                    status: r.get(2)?,
                    evidence: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let result = subtasks
            .iter()
            .find(|s| s.subtask_ref == subtask_ref)
            .cloned()
            .context("subtask missing after update")?;
        task.spec.subtasks = subtasks;
        drop(stmt);
        save(&tx, &task)?;
        tx.commit()?;
        Ok(result)
    }
    /// List all subtasks for a task.
    pub fn subtask_list(&self, task_id: &str) -> Result<Vec<SubtaskSpec>> {
        self.assert_id(task_id)?;
        let mut stmt = self.connection.prepare(
            "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
        )?;
        let subtasks = stmt
            .query_map([task_id], |r| {
                Ok(SubtaskSpec {
                    subtask_ref: r.get(0)?,
                    title: r.get(1)?,
                    status: r.get(2)?,
                    evidence: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if subtasks.is_empty() {
            let task = self.inspect(task_id)?;
            Ok(task.spec.subtasks)
        } else {
            Ok(subtasks)
        }
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
             (SELECT task_id FROM tasks WHERE milestone_ref=?1 AND substr(task_id,1,length(?2))=?2)",
            params![relative_reference.as_ref(), self.namespace],
        )?;
        let deleted = tx.execute(
            "DELETE FROM tasks WHERE milestone_ref=?1 AND substr(task_id,1,length(?2))=?2",
            params![relative_reference.as_ref(), self.namespace],
        )?;
        tx.commit()?;
        Ok(deleted)
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
        if milestone.status.as_deref() == Some("cancelled") {
            self.prune_cancelled(reference)?;
            return Ok(Vec::new());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut result = Vec::new();
        for spec in &milestone.tasks {
            if task_ref.is_some_and(|r| spec.task_ref != r) {
                continue;
            }
            let id = milestone.task_id(spec);
            let digest = milestone.digest(spec)?;
            let profile_pin = spec.acdd_profile.clone().or_else(|| milestone.acdd_profile.clone());
            let task_profile = match profile_pin.as_deref() {
                Some(reference) => crate::core::tasks::profile::load_pinned_over(
                    root,
                    reference,
                    &self.profile,
                )?,
                None => self.profile.clone(),
            };
            if task_profile.gates.is_empty() {
                bail!("ACDD_PROFILE_INVALID: gates must not be empty");
            }
            if let Some(mut existing) = load(&tx, &id)? {
                reconcile_pre_v4_task_digest(
                    &tx,
                    &id,
                    milestone,
                    spec,
                    &digest,
                    &mut existing,
                )?;
                if existing.digest == digest {
                    if spec.status.as_deref() == Some("completed")
                        && (existing.status != "completed" || existing.receipt != spec.receipt)
                    {
                        bail!("TASK_RECEIPT_INVALID: imported receipt differs from accepted completion");
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
                    if existing.status == "completed" {
                        existing.milestone_ref = reference.into();
                    }
                    save(&tx, &existing)?;
                    result.push(existing);
                    continue;
                }
                if spec.status.as_deref() == Some("completed")
                    || spec.contract_revision <= existing.contract_revision
                {
                    bail!("AUTHORITY_GAP: changed specification requires contract re-admission");
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
                    profile_pin: profile_pin.clone(),
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
                profile_pin: profile_pin.clone(),
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
            tx.execute(
                "UPDATE tasks SET digest_version=1 WHERE task_id=?1",
                [&id],
            )?;
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
                let dependency_id = format!(
                    "{}/{}/{}:{}",
                    milestone.repository, milestone.project, milestone.id, dep
                );
                tx.execute(
                    "INSERT INTO task_dependencies VALUES(?1,?2,?3)",
                    params![id, dependency_id,milestone.tasks.iter().any(|t|t.task_ref == *dep && t.status.as_deref()==Some("completed"))],
                )?;
            }
            result.push(task);
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
        let mut stmt = self.connection.prepare("SELECT descriptor FROM tasks WHERE (?1 IS NULL OR milestone_ref=?1) AND substr(task_id,1,length(?2))=?2 ORDER BY milestone_ref,task_id")?;
        let mut result = Vec::new();
        let mut known_stage = stage.is_none_or(|requested| self.profile.index_of(requested).is_some());
        for item in stmt.query_map(params![milestone, self.namespace], |r| {
            r.get::<_, String>(0)
        })? {
            let mut task: Task = serde_json::from_str(&item?)?;
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
                bail!("TASK_STAGE_UNKNOWN: '{stage}' is not in an active profile; choose one of: {}",
                    self.profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        Ok(result)
    }
    /// Returns the first accepted claim time for one milestone.
    pub fn earliest_claim(&self, milestone_prefix: &str) -> Result<Option<i64>> {
        Ok(self.connection.query_row(
            "SELECT MIN(c.claimed_at) FROM task_claims c JOIN tasks t ON t.task_id=c.task_id WHERE substr(t.task_id,1,length(?1))=?1 AND substr(t.task_id,1,length(?2))=?2",
            params![milestone_prefix, self.namespace], |row| row.get(0))?)
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
            .query_map(params![milestone_prefix, self.namespace], |row| row.get(0))?
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
        old_ref: &str,
        new_ref: &str,
        expected_ids: &[String],
        receipts: &std::collections::BTreeMap<String, Receipt>,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let descriptors: Vec<String> = tx
            .prepare("SELECT descriptor FROM tasks WHERE substr(task_id,1,length(?1))=?1 AND substr(task_id,1,length(?2))=?2")?
            .query_map(params![milestone_prefix, self.namespace], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut found_ids = Vec::new();
        for descriptor in descriptors {
            let mut task: Task = serde_json::from_str(&descriptor)?;
            if task.milestone_ref != old_ref || task.status != "completed" {
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
    /// Releases only the claim revision whose document activation failed.
    pub fn release_failed_claim(&mut self, id: &str, revision: u64) -> Result<()> {
        self.assert_id(id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        if task.status != "in_progress" || task.claim_revision != revision {
            bail!("TASK_STALE_CLAIM");
        }
        if tx.execute(
            "DELETE FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
            params![id, revision],
        )? != 1
        {
            bail!("TASK_STALE_CLAIM");
        }
        task.claim_revision = next_revision(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        task.status = "ready".into();
        save(&tx, &task)?;
        tx.commit()?;
        Ok(())
    }
    /// Performs claim.
    pub fn claim(&mut self, id: &str, stage: &str, worker: &str, worktree: &str) -> Result<Task> {
        self.assert_id(id)?;
        if worker.trim().is_empty() {
            bail!("worker_id must be nonempty");
        }
        let worktree = crate::core::tasks::claim_worktree(worktree)?;
        let namespace = self.namespace.clone();
        let workspace_root = self.workspace_root.clone();
        let base_profile = self.profile.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        let profile = resolve_task_profile(workspace_root.as_deref(), &base_profile, &task)?;
        if let Some(root) = self.workspace_root.as_deref() {
            verify_dirty_acdd_scope(root, &task.initial_scope)?;
            if worktree != root {
                verify_dirty_acdd_scope(&worktree, &task.initial_scope)?;
            }
        }
        if task.worker_id.is_some() {
            return Err(TaskAlreadyClaimed.into());
        }
        if task.status != "ready" || !dependencies_ready(&tx, id)? {
            bail!("TASK_NOT_READY");
        }
        let task_gate = profile.index_of(&task.stage).with_context(|| format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        ))?;
        check_stage(&task.stage, stage)?;
        task.gate = task_gate;
        let my_scope_paths: Vec<PathBuf> = tx
            .prepare("SELECT path FROM task_scope_paths WHERE task_id=?1")?
            .query_map([id], |row| row.get::<_, String>(0).map(PathBuf::from))?
            .collect::<std::result::Result<_, _>>()?;
        let other_scopes: Vec<(String, PathBuf)> = tx
            .prepare(
                "SELECT p.task_id, p.path FROM task_scope_paths p \
                 JOIN tasks t ON t.task_id = p.task_id \
                 WHERE p.task_id != ?1 AND t.milestone_ref = ?2 \
                   AND substr(p.task_id, 1, length(?3)) = ?3 \
                   AND json_extract(t.descriptor, '$.status') != 'completed' \
                   AND NOT EXISTS (SELECT 1 FROM task_dependencies d \
                     LEFT JOIN tasks dependency ON dependency.task_id = d.dependency_id \
                     WHERE d.task_id = t.task_id AND d.satisfied = 0 \
                       AND (dependency.task_id IS NULL OR json_extract(dependency.descriptor, '$.status') != 'completed'))",
            )?
            .query_map(params![id, task.milestone_ref, namespace], |row| {
                Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?)))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for my_path in &my_scope_paths {
            for (owner, other_path) in &other_scopes {
                if my_path == other_path
                    || my_path.starts_with(other_path)
                    || other_path.starts_with(my_path)
                {
                    bail!("TASK_SCOPE_CONFLICT: path '{}' belongs to task '{owner}'; reopen that task instead", my_path.display());
                }
            }
        }
        task.claim_revision = next_revision(&tx, id)?;
        task.worker_id = Some(worker.into());
        task.worktree = Some(worktree.to_string_lossy().into_owned());
        task.status = "in_progress".into();
        tx.execute("INSERT INTO task_claims(task_id,revision,worker_id,worktree,claimed_at) VALUES(?1,?2,?3,?4,?5)",params![id,task.claim_revision,worker,task.worktree,now()])?;
        save(&tx, &task)?;
        tx.commit()?;
        Ok(task)
    }
    /// Performs reset.
    pub fn reset(&mut self, id: &str) -> Result<Task> {
        self.assert_id(id)?;
        let workspace_root = self.workspace_root.clone();
        let base_profile = self.profile.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        let profile = resolve_task_profile(workspace_root.as_deref(), &base_profile, &task)?;
        if task.status == "completed" {
            task.gate = 0;
            task.stage = profile.gate_id(0).to_owned();
            task.completed_at = None;
            task.receipt = None;
        }
        end_claim(&tx, id)?;
        task.claim_revision = next_revision(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        task.status = "ready".into();
        tx.execute(
            "INSERT INTO task_gates VALUES(?1,?2,?3,'pending',NULL)",
            params![id, profile.gate_id(task.gate), task.claim_revision],
        )?;
        save(&tx, &task)?;
        tx.commit()?;
        Ok(task)
    }
    /// Performs reopen, resetting a task back to ready state.
    pub fn reopen(&mut self, id: &str) -> Result<Task> {
        self.reset(id)
    }
    /// Performs extend scope.
    pub fn extend_scope(&mut self, id: &str, paths: &[String], root: &Path) -> Result<Task> {
        self.assert_id(id)?;
        if paths.is_empty() {
            bail!("paths must be nonempty");
        }
        let namespace = self.namespace.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        if task.status == "completed" {
            bail!("TASK_TERMINAL: cannot extend scope of completed task; reopen or reset the task first (e.g. `task reset <task-id>`)");
        }
        let worktree = task.worktree.as_deref().map(Path::new).unwrap_or(root);
        let bases: Vec<String> = if !task.spec.scope_roots.is_empty() {
            task.spec
                .scope_roots
                .iter()
                .map(|r| {
                    let rel = crate::core::tasks::relative_path(r)?;
                    Ok(rel.to_string_lossy().into_owned())
                })
                .collect::<Result<Vec<String>>>()?
        } else {
            tx.prepare(
                "SELECT DISTINCT base_root FROM task_scope_paths WHERE task_id=?1 AND frozen=1",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
        };
        let my_paths: Vec<PathBuf> = tx
            .prepare("SELECT path FROM task_scope_paths WHERE task_id=?1")?
            .query_map([id], |r| {
                let p: String = r.get(0)?;
                Ok(PathBuf::from(p))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let other_scopes: Vec<(String, PathBuf)> = tx
            .prepare(
                "SELECT p.task_id, p.path FROM task_scope_paths p \
                 JOIN tasks t ON t.task_id = p.task_id \
                 WHERE p.task_id != ?1 AND t.milestone_ref = ?2 \
                   AND substr(p.task_id, 1, length(?3)) = ?3 \
                   AND json_extract(t.descriptor, '$.status') != 'completed'",
            )?
            .query_map(params![id, task.milestone_ref, namespace], |r| {
                let owner: String = r.get(0)?;
                let p: String = r.get(1)?;
                Ok((owner, PathBuf::from(p)))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for value in paths {
            let path = crate::core::tasks::relative_path(value)?;
            if protected_acdd_path(&path) {
                bail!("TASK_SCOPE_PROTECTED: paths under .forge/acdd/** must be admitted in the initial task contract scope");
            }
            for (owner, other_path) in &other_scopes {
                let is_conflict = if path == *other_path {
                    true
                } else if path.starts_with(other_path) {
                    !my_paths.iter().any(|my| {
                        path.starts_with(my) && (my == other_path || my.starts_with(other_path))
                    })
                } else if other_path.starts_with(&path) {
                    !my_paths.iter().any(|my| other_path.starts_with(my))
                } else {
                    false
                };
                if is_conflict {
                    bail!("TASK_SCOPE_CONFLICT: path '{value}' belongs to task '{owner}'; reopen that task instead");
                }
            }
            let matched_base = bases
                .iter()
                .find(|base| !base.is_empty() && path.starts_with(Path::new(base)))
                .cloned();
            let base = match matched_base {
                Some(b) => b,
                None => {
                    bail!("TASK_SCOPE_INVALID: path '{value}' is outside admitted scope roots");
                }
            };
            for root in [root, worktree] {
                let resolved = crate::core::tasks::confined_path(root, value)?;
                let resolved_base = if base.is_empty() {
                    root.canonicalize()?
                } else {
                    crate::core::tasks::confined_path(root, &base)?
                };
                if !resolved.starts_with(&resolved_base) {
                    bail!("TASK_SCOPE_INVALID: symlink leaves frozen module root");
                }
            }
            tx.execute(
                "INSERT OR IGNORE INTO task_scope_paths VALUES(?1,?2,?3,0)",
                params![id, path.to_string_lossy(), base],
            )?;
            let normalized = path.to_string_lossy().into_owned();
            if !task.spec.scope.contains(&normalized) {
                task.spec.scope.push(normalized);
            }
        }
        save(&tx, &task)?;
        tx.commit()?;
        Ok(task)
    }
    /// Performs delete.
    pub fn delete(
        &mut self,
        id: Option<&str>,
        milestone: Option<&str>,
        force: bool,
        at: i64,
    ) -> Result<DeleteReport> {
        if id.is_some() == milestone.is_some() {
            bail!("delete requires exactly one selector");
        }
        if let Some(id) = id {
            self.assert_id(id)?;
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let targets: Vec<String> = tx.prepare("SELECT task_id FROM tasks WHERE ((?1 IS NOT NULL AND task_id=?1) OR (?2 IS NOT NULL AND milestone_ref=?2)) AND substr(task_id,1,length(?3))=?3")?.query_map(params![id,milestone,self.namespace],|r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
        let mut revoked_claims = 0;
        for id in &targets {
            let task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
            if !force
                && (task.status != "completed"
                    || task.worker_id.is_some()
                    || task
                        .completed_at
                        .is_none_or(|time| at - time <= RETENTION_SECONDS))
            {
                bail!("TASK_DELETE_INELIGIBLE");
            }
            revoked_claims += usize::from(task.worker_id.is_some());
        }
        for id in &targets {
            let task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
            if task.status == "completed" {
                tx.execute(
                    "UPDATE task_dependencies SET satisfied=1 WHERE dependency_id=?1",
                    [id],
                )?;
            }
            next_revision(&tx, id)?;
            tx.execute("DELETE FROM tasks WHERE task_id=?1", [id])?;
        }
        tx.commit()?;
        Ok(DeleteReport {
            count: targets.len(),
            deleted_ids: targets,
            revoked_claims,
            mode: if force { "force" } else { "standard" }.into(),
        })
    }
    /// Performs cleanup.
    pub fn cleanup(&mut self, at: i64) -> Result<Vec<String>> {
        let ids: Vec<String> = self
            .list(None, "completed", None)?
            .into_iter()
            .filter(|t| {
                t.completed_at
                    .is_some_and(|time| at - time > RETENTION_SECONDS)
            })
            .map(|t| t.task_id)
            .collect();
        let mut deleted = Vec::new();
        for id in ids {
            deleted.extend(self.delete(Some(&id), None, false, at)?.deleted_ids);
        }
        Ok(deleted)
    }
}
pub(crate) fn clear_blackboard(connection: &Connection, task_id: &str) -> Result<usize> {
    Ok(connection.execute("DELETE FROM task_blackboard WHERE task_id=?1", [task_id])?)
}

pub(crate) fn load(conn: &Connection, id: &str) -> Result<Option<Task>> {
    let value: Option<String> = conn
        .query_row("SELECT descriptor FROM tasks WHERE task_id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    value.map(|v| {
        let mut task: Task = serde_json::from_str(&v)?;
        task.gate = crate::core::tasks::profile::compiled().index_of(&task.stage).unwrap_or(usize::MAX);
        Ok(task)
    }).transpose()
}
pub(crate) fn save(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks(task_id,milestone_ref,descriptor) VALUES(?1,?2,?3) \
         ON CONFLICT(task_id) DO UPDATE SET milestone_ref=excluded.milestone_ref,descriptor=excluded.descriptor",
        params![task.task_id, task.milestone_ref, serde_json::to_string(task)?],
    )?;
    Ok(())
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
pub(crate) fn check_stage(active_gate: &str, stage: &str) -> Result<()> {
    if active_gate != stage {
        bail!("TASK_STAGE_INVALID");
    }
    Ok(())
}
fn dependencies_ready(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM task_dependencies d LEFT JOIN tasks t ON t.task_id=d.dependency_id WHERE d.task_id=?1 AND d.satisfied=0 AND (t.task_id IS NULL OR json_extract(t.descriptor,'$.status')!='completed'))",[id],|r|r.get(0))?)
}
/// Performs now.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
