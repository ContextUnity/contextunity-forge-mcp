use crate::core::tasks::{Milestone, Receipt, SubtaskSpec, TaskSpec, GATES};
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
        "DROP INDEX IF EXISTS idx_task_blackboard_task_created;
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
         );
         DROP TABLE task_blackboard_v1;
         CREATE INDEX idx_task_blackboard_scope_created
             ON task_blackboard(milestone_ref,task_id,subtask_ref,created_at DESC,id DESC);
         CREATE INDEX idx_task_blackboard_task_created
             ON task_blackboard(task_id,created_at DESC,id DESC);
         UPDATE task_store_metadata SET value='2' WHERE key='schema_version';",
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
                "2" => {}
                _ => bail!("unsupported tasks schema; use recovery tooling"),
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
            INSERT OR IGNORE INTO task_store_metadata VALUES('schema_version','2');
            CREATE TABLE IF NOT EXISTS task_revisions(task_id TEXT PRIMARY KEY,revision INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks(task_id TEXT PRIMARY KEY,milestone_ref TEXT NOT NULL,descriptor TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_claims(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,worker_id TEXT NOT NULL,worktree TEXT NOT NULL,claimed_at INTEGER NOT NULL,ended INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(task_id,revision));
            CREATE TABLE IF NOT EXISTS task_gates(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,gate TEXT NOT NULL,revision INTEGER NOT NULL,state TEXT NOT NULL,evidence TEXT,PRIMARY KEY(task_id,gate,revision));
            CREATE TABLE IF NOT EXISTS task_findings(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,findings TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_scope_paths(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,path TEXT NOT NULL,base_root TEXT NOT NULL,frozen INTEGER NOT NULL,PRIMARY KEY(task_id,path));
            CREATE TABLE IF NOT EXISTS task_dependencies(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,dependency_id TEXT NOT NULL,satisfied INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(task_id,dependency_id));
            CREATE TABLE IF NOT EXISTS task_submissions(task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE,revision INTEGER NOT NULL,result TEXT NOT NULL,PRIMARY KEY(task_id,revision));
            CREATE TABLE IF NOT EXISTS task_blackboard(id INTEGER PRIMARY KEY AUTOINCREMENT,milestone_ref TEXT NOT NULL,task_id TEXT REFERENCES tasks ON DELETE CASCADE,subtask_ref TEXT,author TEXT NOT NULL,topic TEXT NOT NULL,payload TEXT NOT NULL,created_at INTEGER NOT NULL,CHECK(task_id IS NOT NULL OR subtask_ref IS NULL));
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_scope_created ON task_blackboard(milestone_ref, task_id, subtask_ref, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_task_blackboard_task_created ON task_blackboard(task_id, created_at DESC, id DESC);
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
        if version != "2" {
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
        self.assert_milestone_scope_ref(milestone_ref)?;
        if author.trim().is_empty() || topic.trim().is_empty() {
            bail!("TASK_BLACKBOARD_INVALID: author and topic are required");
        }
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
                "INSERT INTO task_blackboard(milestone_ref,task_id,subtask_ref,author,topic,payload,created_at) \
                 SELECT ?1,?2,?3,?4,?5,?6,?7 \
                 FROM tasks \
                 WHERE task_id=?2 AND milestone_ref=?8 \
                   AND json_extract(descriptor,'$.status')!='completed' \
                   AND (?3 IS NULL OR EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?2 AND subtask_ref=?3))",
                params![milestone_ref, task_id, subtask_ref, author, topic, payload, created_at, task_milestone_ref],
            )?
        } else {
            self.connection.execute(
                "INSERT INTO task_blackboard(milestone_ref,task_id,subtask_ref,author,topic,payload,created_at) \
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![milestone_ref, task_id, subtask_ref, author, topic, payload, created_at],
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
        self.assert_milestone_scope_ref(milestone_ref)?;
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
            "SELECT id,milestone_ref,task_id,subtask_ref,author,topic,created_at \
             FROM task_blackboard \
             WHERE milestone_ref=?1 \
               AND ((?2 IS NULL AND task_id IS NULL) OR \
                    (?2 IS NOT NULL AND task_id=?2 AND \
                     ((?3 IS NULL AND subtask_ref IS NULL) OR (?3 IS NOT NULL AND subtask_ref=?3)))) \
               AND (?4 IS NULL OR topic=?4) \
             ORDER BY created_at DESC,id DESC LIMIT ?5 OFFSET ?6",
        )?;
        let mut messages = statement
            .query_map(
                params![
                    milestone_ref,
                    task_id,
                    subtask_ref,
                    topic,
                    sql_limit,
                    sql_offset
                ],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        subtask_ref: row.get(3)?,
                        author: row.get(4)?,
                        topic: row.get(5)?,
                        payload: None,
                        created_at: row.get(6)?,
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
                "SELECT id,milestone_ref,task_id,subtask_ref,author,topic,payload,created_at \
                 FROM task_blackboard WHERE id=?1",
                [id],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        subtask_ref: row.get(3)?,
                        author: row.get(4)?,
                        topic: row.get(5)?,
                        payload: Some(row.get(6)?),
                        created_at: row.get(7)?,
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
        let limit = limit.map(i64::try_from).transpose()?.unwrap_or(i64::MAX);
        let mut statement = self.connection.prepare(
            "SELECT id,milestone_ref,task_id,subtask_ref,author,topic,payload,created_at FROM task_blackboard \
             WHERE task_id=?1 AND (?2 IS NULL OR topic=?2) ORDER BY created_at,id LIMIT ?3",
        )?;
        let messages = statement
            .query_map(params![task_id, topic, limit], |row| {
                Ok(BlackboardMessage {
                    id: row.get(0)?,
                    milestone_ref: row.get(1)?,
                    task_id: row.get(2)?,
                    subtask_ref: row.get(3)?,
                    author: row.get(4)?,
                    topic: row.get(5)?,
                    payload: Some(row.get(6)?),
                    created_at: row.get(7)?,
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
    /// Read the commit SHA recorded for the build/v1 gate evidence.
    pub fn build_snapshot_commit(&self, task_id: &str) -> Result<Option<String>> {
        self.assert_id(task_id)?;
        let evidence: Option<String> = self
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='build/v1' AND state='passed' ORDER BY revision DESC LIMIT 1")?
            .query_row([task_id], |r| r.get(0))
            .optional()?;
        Ok(evidence.and_then(|ev| {
            serde_json::from_str::<serde_json::Value>(&ev)
                .ok()?
                .get("commit")?
                .as_str()
                .map(str::to_owned)
        }))
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
                let inspect_cmd = commit.as_ref().map(|c| format!("git show {c}"));
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
        object.insert("stage".into(), serde_json::json!(GATES[task.gate]));
        object.insert("attempts".into(), serde_json::json!(attempts));
        object.insert("gates".into(), serde_json::json!(gates));
        let latest_snapshot = gates.iter().rev().find_map(|g| {
            g.get("commit").and_then(|c| c.as_str()).map(|commit| {
                serde_json::json!({
                    "commit": commit,
                    "inspect_cmd": format!("git show {commit}"),
                })
            })
        });
        if let Some(snapshot) = latest_snapshot {
            object.insert("latest_snapshot".into(), snapshot);
        }
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
            let mut task = Task {
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
                 WHERE p.task_id != ?1 AND t.milestone_ref = ?2",
            )?
            .query_map(params![id, task.milestone_ref], |r| {
                let owner: String = r.get(0)?;
                let p: String = r.get(1)?;
                Ok((owner, PathBuf::from(p)))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for value in paths {
            let path = crate::core::tasks::relative_path(value)?;
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
                .find(|base| path.starts_with(Path::new(base)))
                .cloned();
            let base = match matched_base {
                Some(b) => b,
                None => {
                    if path.starts_with("tests") {
                        "tests".to_string()
                    } else if path.starts_with("src") {
                        "src".to_string()
                    } else {
                        bail!(
                            "TASK_SCOPE_INVALID: path '{value}' is outside source roots and tests"
                        );
                    }
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
