use crate::core::tasks::{Milestone, Receipt, SubtaskSpec, TaskSpec, GATES};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    /// The gate value.
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

/// A task-scoped collaboration message persisted in the task store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardMessage {
    /// Monotonic message identifier.
    pub id: u64,
    /// Owning task identifier.
    pub task_id: String,
    /// Posting worker identifier.
    pub author: String,
    /// Message category.
    pub topic: String,
    /// Text or serialized JSON payload.
    pub payload: String,
    /// Unix creation time.
    pub created_at: i64,
}

/// Represents tasks store data.
pub struct TasksStore {
    /// The connection value.
    pub connection: Connection,
    namespace: String,
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
            if version != "1" {
                bail!("unsupported tasks schema; use recovery tooling");
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
            INSERT OR IGNORE INTO task_store_metadata VALUES('schema_version','1');
            CREATE TABLE IF NOT EXISTS task_revisions(task_id TEXT PRIMARY KEY,revision INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks(task_id TEXT PRIMARY KEY,milestone_ref TEXT NOT NULL,descriptor TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_claims(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,worker_id TEXT NOT NULL,worktree TEXT NOT NULL,claimed_at INTEGER NOT NULL,ended INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(task_id,revision));
            CREATE TABLE IF NOT EXISTS task_gates(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,gate TEXT NOT NULL,revision INTEGER NOT NULL,state TEXT NOT NULL,evidence TEXT,PRIMARY KEY(task_id,gate,revision));
            CREATE TABLE IF NOT EXISTS task_findings(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,findings TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_scope_paths(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,path TEXT NOT NULL,base_root TEXT NOT NULL,frozen INTEGER NOT NULL,PRIMARY KEY(task_id,path));
            CREATE TABLE IF NOT EXISTS task_dependencies(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,dependency_id TEXT NOT NULL,satisfied INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(task_id,dependency_id));
            CREATE TABLE IF NOT EXISTS task_submissions(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,result TEXT NOT NULL,PRIMARY KEY(task_id,revision));
            CREATE TABLE IF NOT EXISTS task_blackboard(id INTEGER PRIMARY KEY AUTOINCREMENT,task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,author TEXT NOT NULL,topic TEXT NOT NULL,payload TEXT NOT NULL,created_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_task_created ON task_blackboard(task_id, created_at);
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
        if version != "1" {
            bail!("unsupported tasks schema; use recovery tooling");
        }
        Ok(Self {
            connection,
            namespace: format!("{repository}/{project}/"),
        })
    }
    pub(crate) fn assert_id(&self, id: &str) -> Result<()> {
        if !id.starts_with(&self.namespace) {
            bail!("TASK_PROJECT_MISMATCH");
        }
        Ok(())
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
        if author.trim().is_empty() || topic.trim().is_empty() {
            bail!("TASK_BLACKBOARD_INVALID: author and topic are required");
        }
        let inserted = self.connection.execute(
            "INSERT INTO task_blackboard(task_id,author,topic,payload,created_at) \
             SELECT task_id,?2,?3,?4,?5 FROM tasks \
             WHERE task_id=?1 AND json_extract(descriptor,'$.status')!='completed'",
            params![task_id, author, topic, payload, now()],
        )?;
        if inserted == 0 {
            if self.inspect(task_id)?.status == "completed" {
                bail!("TASK_TERMINAL");
            }
            bail!("TASK_BLACKBOARD_INVALID: task cannot accept messages");
        }
        Ok(u64::try_from(self.connection.last_insert_rowid())?)
    }
    /// Read messages in insertion order with an optional topic and result limit.
    pub fn blackboard_read(
        &self,
        task_id: &str,
        topic: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<BlackboardMessage>> {
        self.assert_id(task_id)?;
        let limit = limit.map(i64::try_from).transpose()?.unwrap_or(i64::MAX);
        let mut statement = self.connection.prepare(
            "SELECT id,task_id,author,topic,payload,created_at FROM task_blackboard \
             WHERE task_id=?1 AND (?2 IS NULL OR topic=?2) ORDER BY created_at,id LIMIT ?3",
        )?;
        let messages = statement
            .query_map(params![task_id, topic, limit], |row| {
                Ok(BlackboardMessage {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    author: row.get(2)?,
                    topic: row.get(3)?,
                    payload: row.get(4)?,
                    created_at: row.get(5)?,
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
    /// Performs inspect.
    pub fn inspect(&self, id: &str) -> Result<Task> {
        self.assert_id(id)?;
        load(&self.connection, id)?.context("TASK_NOT_FOUND")
    }
    /// Performs inspect details.
    pub fn inspect_details(&self, id: &str) -> Result<serde_json::Value> {
        let _snapshot = self.connection.unchecked_transaction()?;
        let task = self.inspect(id)?;
        let attempts:Vec<serde_json::Value>=self.connection.prepare("SELECT revision,worker_id,worktree,claimed_at,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")?.query_map([id],|r|Ok(serde_json::json!({"claim_revision":r.get::<_,u64>(0)?,"worker_id":r.get::<_,String>(1)?,"worktree":r.get::<_,String>(2)?,"claimed_at":r.get::<_,i64>(3)?,"ended":r.get::<_,bool>(4)?})))?.collect::<std::result::Result<_,_>>()?;
        let gates:Vec<serde_json::Value>=self.connection.prepare("SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY revision,gate")?.query_map([id],|r|Ok(serde_json::json!({"stage":r.get::<_,String>(0)?,"claim_revision":r.get::<_,u64>(1)?,"state":r.get::<_,String>(2)?,"evidence":r.get::<_,Option<String>>(3)?})))?.collect::<std::result::Result<_,_>>()?;
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
        object.insert("stage".into(), serde_json::json!(GATES[task.gate]));
        object.insert("attempts".into(), serde_json::json!(attempts));
        object.insert("gates".into(), serde_json::json!(gates));
        object.insert("findings".into(), serde_json::json!(findings));
        object.insert("gate_states".into(),serde_json::json!(GATES.iter().enumerate().map(|(i,name)|serde_json::json!({"stage":name,"state":if i<task.gate || task.status=="completed" {"passed"} else if i==task.gate && task.status=="in_progress" {"in_progress"} else {"pending"}})).collect::<Vec<_>>()));
        object.insert("subtasks".into(), serde_json::json!(task.spec.subtasks));
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
    fn sync_selected(
        &mut self,
        milestone: &Milestone,
        reference: &str,
        root: &Path,
        task_ref: Option<&str>,
    ) -> Result<Vec<Task>> {
        if format!("{}/{}/", milestone.repository, milestone.project) != self.namespace {
            bail!("TASK_PROJECT_MISMATCH");
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
            if let Some(mut existing) = load(&tx, &id)? {
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
                tx.execute("DELETE FROM task_subtasks WHERE task_id=?1", [&id])?;
            }
            let completed = spec.status.as_deref() == Some("completed");
            if completed {
                crate::core::tasks::gates::validate_durable_receipt(
                    spec.receipt
                        .as_ref()
                        .context("TASK_RECEIPT_INVALID: missing receipt")?,
                    spec.contract_revision,
                )?;
            }
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
                    gate: 3,
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
            let task = Task {
                task_id: id.clone(),
                milestone_ref: reference.into(),
                spec: spec.clone(),
                digest,
                contract_revision: spec.contract_revision,
                claim_revision: revision,
                status: if completed { "completed" } else { "ready" }.into(),
                gate: if completed { 3 } else { 0 },
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
                     ON CONFLICT(task_id, subtask_ref) DO UPDATE SET title=excluded.title, status=excluded.status, evidence=COALESCE(excluded.evidence, task_subtasks.evidence), updated_at=excluded.updated_at",
                    params![id, sub.subtask_ref, sub.title, sub.status, sub.evidence, now()],
                )?;
            }
            for scope in &spec.scope {
                let path = crate::core::tasks::relative_path(scope)?;
                crate::core::tasks::confined_path(root, scope)?;
                let base = if scope.ends_with('/') || root.join(&path).is_dir() {
                    path.clone()
                } else {
                    path.parent().context("scope needs parent")?.to_path_buf()
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
        for item in stmt.query_map(params![milestone, self.namespace], |r| {
            r.get::<_, String>(0)
        })? {
            let mut task: Task = serde_json::from_str(&item?)?;
            if task.status == "ready" && !dependencies_ready(&self.connection, &task.task_id)? {
                task.status = "blocked".into();
            }
            if status != "all" && task.status != status {
                continue;
            }
            if stage.is_some_and(|s| GATES[task.gate].split('/').next() != Some(s)) {
                continue;
            }
            result.push(task);
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        if task.worker_id.is_some() {
            return Err(TaskAlreadyClaimed.into());
        }
        if task.status != "ready" || !dependencies_ready(&tx, id)? {
            bail!("TASK_NOT_READY");
        }
        check_stage(&task, stage)?;
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        if task.status == "completed" {
            task.gate = 0;
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
            params![id, GATES[task.gate], task.claim_revision],
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        if task.status == "completed" {
            bail!("TASK_TERMINAL: cannot extend scope of completed task; reopen or reset the task first (e.g. `task reset <task-id>`)");
        }
        let worktree = task.worktree.as_deref().map(Path::new).unwrap_or(root);
        let bases: Vec<String> = tx
            .prepare(
                "SELECT DISTINCT base_root FROM task_scope_paths WHERE task_id=?1 AND frozen=1",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for value in paths {
            let path = crate::core::tasks::relative_path(value)?;
            let base = bases
                .iter()
                .find(|base| path.starts_with(Path::new(base)))
                .context("TASK_SCOPE_INVALID: outside frozen roots")?;
            for root in [root, worktree] {
                let resolved = crate::core::tasks::confined_path(root, value)?;
                let resolved_base = if base.is_empty() {
                    root.canonicalize()?
                } else {
                    crate::core::tasks::confined_path(root, base)?
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
    value
        .map(|v| serde_json::from_str(&v).map_err(Into::into))
        .transpose()
}
pub(crate) fn save(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute("INSERT INTO tasks VALUES(?1,?2,?3) ON CONFLICT(task_id) DO UPDATE SET milestone_ref=excluded.milestone_ref,descriptor=excluded.descriptor",params![task.task_id,task.milestone_ref,serde_json::to_string(task)?])?;
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
pub(crate) fn check_stage(task: &Task, stage: &str) -> Result<()> {
    if GATES[task.gate] != stage && GATES[task.gate].split('/').next() != Some(stage) {
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
