use super::{Connection, TasksStore};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

const CHECKPOINT_PAYLOAD_KEY: &str = "_forge_checkpoint_payload_v1";

pub(super) fn ensure_command_output_schema(connection: &Connection) -> Result<()> {
    let proof_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='command_execution_proofs')",
        [],
        |row| row.get(0),
    )?;
    let schema: String = connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE name='gate_execution_traces'",
        [],
        |row| row.get(0),
    )?;
    if proof_exists && !schema.contains("<= 2048") {
        return Ok(());
    }
    connection.execute_batch("SAVEPOINT command_output_upgrade")?;
    match migrate(connection) {
        Ok(()) => {
            connection.execute_batch("RELEASE command_output_upgrade")?;
            Ok(())
        }
        Err(error) => {
            connection.execute_batch(
                "ROLLBACK TO command_output_upgrade; RELEASE command_output_upgrade",
            )?;
            Err(error)
        }
    }
}

/// Maximum number of UTF-8 bytes stored in a command trace output snippet.
pub const MAX_OUTPUT_SNIPPET_BYTES: usize = 64 * 1024;

/// Proof-critical facts extracted from complete command output before presentation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandExecutionProof {
    pub passed: Option<u64>,
    pub failed: Option<u64>,
    pub seam_ref: Option<String>,
    pub output_spec: Option<crate::tasks::profile::CommandOutputSpec>,
    pub expected_signature: Option<String>,
    pub scope_violations: Vec<String>,
}

/// A durable record of one task or milestone gate transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateTransition {
    /// Milestone that owns the transition.
    pub milestone_id: String,
    /// Task or milestone whose gate changed.
    pub subject: String,
    /// Hierarchical gate path, such as `tasks.build`.
    pub gate_path: String,
    /// Transition action, such as `advance`, `reject`, or `reopen`.
    pub action: String,
    /// Kind of evidence recorded for the transition.
    pub proof_kind: String,
    /// Typed proof data or failure metadata.
    pub proof_payload: serde_json::Value,
    /// Git tree associated with the transition.
    pub git_tree_sha: String,
    /// Worker that performed or recorded the transition.
    pub worker_id: String,
}

/// A persisted trace for one command executed by a task or milestone gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateExecutionTrace {
    /// Milestone that owns the execution.
    pub milestone_id: String,
    /// Task or milestone whose gate ran the command.
    pub subject: String,
    /// Hierarchical gate path, such as `tasks.build`.
    pub gate_path: String,
    /// Attempt number for this gate command.
    pub attempt_id: i64,
    /// Stable command identifier from the command catalog.
    pub command_id: String,
    /// Expanded command string that was executed.
    pub command_str: String,
    /// Execution outcome, such as `completed`, `timeout`, or `spawn_error`.
    pub status: String,
    /// Process exit code, if the process produced one.
    pub exit_code: Option<i32>,
    /// Optional path to the complete command log.
    pub log_path: Option<String>,
    /// Bounded output excerpt for immediate task guidance.
    pub output_snippet: Option<String>,
    /// SHA-256 digest of stdout, when available.
    pub stdout_sha256: Option<String>,
    /// Command duration in milliseconds.
    pub duration_ms: i64,
}

/// A durable task or milestone checkpoint keyed by its turn breadcrumb.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateCheckpoint {
    /// Unique turn breadcrumb for this checkpoint.
    pub breadcrumb: String,
    /// Milestone that owns the checkpoint.
    pub milestone_id: String,
    /// Task or milestone whose gate state was captured.
    pub subject: String,
    /// Hierarchical gate path, such as `tasks.build`.
    pub gate_path: String,
    /// Transition that produced the checkpoint, if one exists.
    pub transition_id: Option<i64>,
    /// Git tree associated with the checkpoint.
    pub git_tree_sha: String,
    /// Serialized state captured at this breadcrumb.
    pub state_payload: serde_json::Value,
}

/// Create or upgrade the gate transition, command trace, and checkpoint tables.
pub(super) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS gate_transitions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            milestone_id TEXT NOT NULL,
            subject TEXT NOT NULL,
            gate_path TEXT NOT NULL,
            action TEXT NOT NULL,
            proof_kind TEXT NOT NULL,
            proof_payload JSON NOT NULL,
            git_tree_sha TEXT NOT NULL,
            worker_id TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS gate_execution_traces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            milestone_id TEXT NOT NULL,
            subject TEXT NOT NULL,
            gate_path TEXT NOT NULL,
            attempt_id INTEGER NOT NULL DEFAULT 1,
            command_id TEXT NOT NULL,
            command_str TEXT NOT NULL,
            status TEXT NOT NULL,
            exit_code INTEGER,
            log_path TEXT,
            output_snippet TEXT,
            stdout_sha256 TEXT,
            duration_ms INTEGER NOT NULL,
            executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(milestone_id, subject, gate_path, attempt_id, command_id),
            CHECK (
                output_snippet IS NULL
                OR length(CAST(output_snippet AS BLOB)) <= 65536
            )
        );

        CREATE TABLE IF NOT EXISTS gate_checkpoints (
            breadcrumb TEXT PRIMARY KEY,
            milestone_id TEXT NOT NULL,
            subject TEXT NOT NULL,
            gate_path TEXT NOT NULL,
            transition_id INTEGER REFERENCES gate_transitions(id),
            git_tree_sha TEXT NOT NULL,
            state_payload JSON NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );",
    )?;
    let schema: String = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE name='gate_execution_traces'",
        [],
        |row| row.get(0),
    )?;
    if schema.contains("<= 2048") {
        connection.execute_batch(
            "SAVEPOINT trace_snippet_upgrade;
            ALTER TABLE gate_execution_traces RENAME TO gate_execution_traces_old;",
        )?;
        let result = (|| -> Result<()> {
            connection.execute_batch(&schema.replace("<= 2048", "<= 65536"))?;
            connection.execute_batch(
                "INSERT INTO gate_execution_traces SELECT * FROM gate_execution_traces_old;
                DROP TABLE gate_execution_traces_old;
                RELEASE trace_snippet_upgrade;",
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            connection.execute_batch(
                "ROLLBACK TO trace_snippet_upgrade; RELEASE trace_snippet_upgrade;",
            )?;
            return Err(error);
        }
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS command_execution_proofs (
            run_id INTEGER NOT NULL REFERENCES gate_execution_runs(id) ON DELETE CASCADE,
            command_id TEXT NOT NULL,
            attempt_id INTEGER NOT NULL,
            payload TEXT NOT NULL,
            PRIMARY KEY(run_id, command_id, attempt_id)
        );",
    )?;
    Ok(())
}

/// Insert a gate execution trace and return its SQLite row identifier.
pub(super) fn record_trace(connection: &Connection, trace: &GateExecutionTrace) -> Result<i64> {
    for (field, value) in [
        ("milestone_id", trace.milestone_id.as_str()),
        ("subject", trace.subject.as_str()),
        ("gate_path", trace.gate_path.as_str()),
        ("command_id", trace.command_id.as_str()),
        ("command_str", trace.command_str.as_str()),
        ("status", trace.status.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("TASK_TRACE_INVALID: {field} must not be empty");
        }
    }
    if trace.attempt_id < 1 {
        bail!("TASK_TRACE_INVALID: attempt_id must be greater than zero");
    }
    if trace.duration_ms < 0 {
        bail!("TASK_TRACE_INVALID: duration_ms must not be negative");
    }

    let output_snippet = trace.output_snippet.as_deref().map(bounded_output_snippet);
    connection.execute(
        "INSERT INTO gate_execution_traces (
            milestone_id, subject, gate_path, attempt_id, command_id, command_str,
            status, exit_code, log_path, output_snippet, stdout_sha256, duration_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            trace.milestone_id,
            trace.subject,
            trace.gate_path,
            trace.attempt_id,
            trace.command_id,
            trace.command_str,
            trace.status,
            trace.exit_code,
            trace.log_path,
            output_snippet,
            trace.stdout_sha256,
            trace.duration_ms,
        ],
    )?;
    Ok(connection.last_insert_rowid())
}

fn bounded_output_snippet(output: &str) -> &str {
    if output.len() <= MAX_OUTPUT_SNIPPET_BYTES {
        return output;
    }
    let mut end = MAX_OUTPUT_SNIPPET_BYTES;
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    &output[..end]
}

/// Remove the durable gate telemetry owned by one fully qualified milestone identity.
///
/// Checkpoints reference transitions, so they must be removed before their transition
/// rows. Exact equality keeps milestone IDs containing SQL wildcard characters literal.
pub(super) fn prune_milestone_gate_records(
    connection: &Connection,
    milestone_identity: &str,
) -> Result<()> {
    connection.execute(
        "DELETE FROM gate_checkpoints WHERE milestone_id=?1",
        [milestone_identity],
    )?;
    connection.execute(
        "DELETE FROM gate_execution_traces WHERE milestone_id=?1",
        [milestone_identity],
    )?;
    connection.execute(
        "DELETE FROM gate_transitions WHERE milestone_id=?1",
        [milestone_identity],
    )?;
    Ok(())
}

impl TasksStore {
    /// Persist a command execution trace and return its SQLite row identifier.
    pub fn record_execution_trace(&self, trace: &GateExecutionTrace) -> Result<i64> {
        record_trace(&self.connection, trace)
    }

    /// Save an on-demand state payload using its full turn breadcrumb as the checkpoint key.
    pub fn save_checkpoint_payload(
        &self,
        breadcrumb: &str,
        git_tree_sha: &str,
        state_payload: serde_json::Value,
    ) -> Result<()> {
        let (milestone_id, gate_path, subject) = checkpoint_identity(&self.namespace, breadcrumb)?;
        if is_engine_owned_checkpoint(&subject, &gate_path) {
            bail!("TASK_CHECKPOINT_OWNED: milestone verification checkpoints are engine-owned");
        }
        if state_payload.get("task_id").is_some() || state_payload.get("claim_revision").is_some() {
            bail!("TASK_CHECKPOINT_OWNED: on-demand checkpoints cannot define task lifecycle identity");
        }
        for (field, value) in [
            ("breadcrumb", breadcrumb),
            ("milestone_id", milestone_id.as_str()),
            ("subject", subject.as_str()),
            ("gate_path", gate_path.as_str()),
            ("git_tree_sha", git_tree_sha),
        ] {
            if value.trim().is_empty() {
                bail!("TASK_CHECKPOINT_INVALID: {field} must not be empty");
            }
        }
        let mut payload_envelope = serde_json::Map::new();
        payload_envelope.insert(CHECKPOINT_PAYLOAD_KEY.to_owned(), state_payload);
        let state_payload_str =
            serde_json::to_string(&serde_json::Value::Object(payload_envelope))?;
        let changed = self.connection.execute(
            "INSERT INTO gate_checkpoints (
                breadcrumb, milestone_id, subject, gate_path, transition_id,
                git_tree_sha, state_payload, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(breadcrumb) DO UPDATE SET
                git_tree_sha=excluded.git_tree_sha,
                state_payload=excluded.state_payload,
                updated_at=CURRENT_TIMESTAMP
             WHERE gate_checkpoints.transition_id IS NULL
               AND json_extract(gate_checkpoints.state_payload, '$._forge_checkpoint_payload_v1.task_id') IS NULL
               AND json_extract(gate_checkpoints.state_payload, '$.task_id') IS NULL",
            params![
                breadcrumb,
                milestone_id,
                subject,
                gate_path,
                git_tree_sha,
                state_payload_str,
            ],
        )?;
        if changed == 0 {
            let row: Option<(Option<i64>, Option<String>)> = self
                .connection
                .query_row(
                    "SELECT transition_id,
                            COALESCE(
                                json_extract(state_payload, '$._forge_checkpoint_payload_v1.task_id'),
                                json_extract(state_payload, '$.task_id')
                            )
                     FROM gate_checkpoints WHERE breadcrumb=?1 AND milestone_id=?2",
                    params![breadcrumb, milestone_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((transition_id, task_id)) = row {
                if transition_id.is_some() {
                    bail!("TASK_CHECKPOINT_OWNED: lifecycle checkpoint is owned by its gate transition");
                }
                if task_id.is_some() {
                    bail!(
                        "TASK_CHECKPOINT_OWNED: lifecycle checkpoint is owned by active task claim"
                    );
                }
            }
        }
        Ok(())
    }

    /// Insert or replace the checkpoint associated with one turn breadcrumb.
    pub fn save_checkpoint(&self, checkpoint: &GateCheckpoint) -> Result<()> {
        let (milestone_id, gate_path, subject) =
            checkpoint_identity(&self.namespace, &checkpoint.breadcrumb)?;
        if checkpoint.milestone_id != milestone_id
            || checkpoint.gate_path != gate_path
            || checkpoint.subject != subject
        {
            bail!(
                "TASK_CHECKPOINT_INVALID: breadcrumb identity does not match checkpoint metadata"
            );
        }
        if checkpoint.transition_id.is_none() && is_engine_owned_checkpoint(&subject, &gate_path) {
            bail!("TASK_CHECKPOINT_OWNED: milestone verification checkpoints are engine-owned");
        }
        for (field, value) in [
            ("breadcrumb", checkpoint.breadcrumb.as_str()),
            ("milestone_id", checkpoint.milestone_id.as_str()),
            ("subject", checkpoint.subject.as_str()),
            ("gate_path", checkpoint.gate_path.as_str()),
            ("git_tree_sha", checkpoint.git_tree_sha.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("TASK_CHECKPOINT_INVALID: {field} must not be empty");
            }
        }
        let mut payload_envelope = serde_json::Map::new();
        payload_envelope.insert(
            CHECKPOINT_PAYLOAD_KEY.to_owned(),
            checkpoint.state_payload.clone(),
        );
        let state_payload = serde_json::to_string(&serde_json::Value::Object(payload_envelope))?;
        self.connection.execute(
            "INSERT INTO gate_checkpoints (
                breadcrumb, milestone_id, subject, gate_path, transition_id,
                git_tree_sha, state_payload
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(breadcrumb) DO UPDATE SET
                milestone_id=excluded.milestone_id,
                subject=excluded.subject,
                gate_path=excluded.gate_path,
                transition_id=excluded.transition_id,
                git_tree_sha=excluded.git_tree_sha,
                state_payload=excluded.state_payload,
                updated_at=CURRENT_TIMESTAMP",
            params![
                checkpoint.breadcrumb,
                checkpoint.milestone_id,
                checkpoint.subject,
                checkpoint.gate_path,
                checkpoint.transition_id,
                checkpoint.git_tree_sha,
                state_payload,
            ],
        )?;
        Ok(())
    }

    /// Read one checkpoint in this repository/project namespace by its full breadcrumb.
    pub fn get_checkpoint(&self, breadcrumb: &str) -> Result<Option<GateCheckpoint>> {
        let (milestone_id, _, _) = checkpoint_identity(&self.namespace, breadcrumb)?;
        self.connection
            .query_row(
                "SELECT breadcrumb, milestone_id, subject, gate_path, transition_id,
                        git_tree_sha, state_payload
                 FROM gate_checkpoints
                 WHERE breadcrumb=?1 AND milestone_id=?2",
                params![breadcrumb, milestone_id],
                checkpoint_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Read one gate transition that belongs to the expected milestone identity.
    pub fn get_gate_transition(
        &self,
        transition_id: i64,
        milestone_identity: &str,
    ) -> Result<Option<GateTransition>> {
        if !milestone_identity.starts_with(&self.namespace) {
            bail!("TASK_CHECKPOINT_SCOPE: transition identity is outside this project namespace");
        }
        self.connection
            .query_row(
                "SELECT milestone_id, subject, gate_path, action, proof_kind,
                        proof_payload, git_tree_sha, worker_id
                 FROM gate_transitions
                 WHERE id=?1 AND milestone_id=?2",
                params![transition_id, milestone_identity],
                transition_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// List checkpoints owned by this repository/project namespace, ordered by breadcrumb.
    pub fn list_checkpoints(&self) -> Result<Vec<GateCheckpoint>> {
        let mut statement = self.connection.prepare(
            "SELECT breadcrumb, milestone_id, subject, gate_path, transition_id,
                    git_tree_sha, state_payload
             FROM gate_checkpoints
             WHERE substr(milestone_id, 1, length(?1))=?1
             ORDER BY breadcrumb",
        )?;
        let checkpoints = statement
            .query_map([self.namespace.as_str()], checkpoint_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(checkpoints)
    }

    /// Delete one checkpoint in this repository/project namespace by its full breadcrumb.
    pub fn delete_checkpoint(&self, breadcrumb: &str) -> Result<bool> {
        let (milestone_id, gate_path, subject) = checkpoint_identity(&self.namespace, breadcrumb)?;
        if is_engine_owned_checkpoint(&subject, &gate_path) {
            bail!("TASK_CHECKPOINT_OWNED: milestone verification checkpoints are engine-owned");
        }
        let changed = self.connection.execute(
            "DELETE FROM gate_checkpoints
             WHERE breadcrumb=?1 AND milestone_id=?2
               AND transition_id IS NULL
               AND json_extract(state_payload, '$._forge_checkpoint_payload_v1.task_id') IS NULL
               AND json_extract(state_payload, '$.task_id') IS NULL",
            params![breadcrumb, milestone_id],
        )?;
        if changed > 0 {
            return Ok(true);
        }
        let row: Option<(Option<i64>, Option<String>)> = self
            .connection
            .query_row(
                "SELECT transition_id,
                        COALESCE(
                            json_extract(state_payload, '$._forge_checkpoint_payload_v1.task_id'),
                            json_extract(state_payload, '$.task_id')
                        )
                 FROM gate_checkpoints
                 WHERE breadcrumb=?1 AND milestone_id=?2",
                params![breadcrumb, milestone_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((transition_id, task_id)) = row {
            if transition_id.is_some() {
                bail!(
                    "TASK_CHECKPOINT_OWNED: lifecycle checkpoint is owned by its gate transition"
                );
            }
            if task_id.is_some() {
                bail!("TASK_CHECKPOINT_OWNED: lifecycle checkpoint is owned by active task claim");
            }
        }
        Ok(false)
    }
}

fn checkpoint_identity(namespace: &str, breadcrumb: &str) -> Result<(String, String, String)> {
    if breadcrumb.is_empty() || breadcrumb.len() > 2048 || breadcrumb.contains('\0') {
        bail!("TASK_CHECKPOINT_INVALID: breadcrumb must contain 1..=2048 bytes");
    }
    let mut components = breadcrumb.rsplitn(4, '/');
    let stage = components.next().unwrap_or_default();
    let subject = components.next().unwrap_or_default();
    let gate_path = components.next().unwrap_or_default();
    let milestone_id = components.next().unwrap_or_default();
    if [stage, subject, gate_path, milestone_id]
        .iter()
        .any(|component| component.trim().is_empty())
        || milestone_id
            .split('/')
            .any(|component| component.is_empty())
        || !milestone_id.starts_with(namespace)
    {
        bail!("TASK_CHECKPOINT_SCOPE: breadcrumb must be a full locator in this project namespace");
    }
    if [stage, subject].iter().any(|component| {
        !component
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_-".contains(ch))
    }) || gate_path.split('.').any(|segment| {
        segment.is_empty()
            || !segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "_-".contains(ch))
    }) || milestone_id
        .split('/')
        .any(|segment| segment == "." || segment == ".." || segment.contains('\\'))
    {
        bail!("TASK_CHECKPOINT_INVALID: breadcrumb contains an invalid path component");
    }
    Ok((
        milestone_id.to_owned(),
        gate_path.to_owned(),
        subject.to_owned(),
    ))
}

fn is_engine_owned_checkpoint(subject: &str, gate_path: &str) -> bool {
    subject == "milestone" && gate_path == "milestone_verification"
}

fn transition_from_row(row: &Row<'_>) -> rusqlite::Result<GateTransition> {
    let proof_payload: String = row.get(5)?;
    let proof_payload = serde_json::from_str(&proof_payload).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(GateTransition {
        milestone_id: row.get(0)?,
        subject: row.get(1)?,
        gate_path: row.get(2)?,
        action: row.get(3)?,
        proof_kind: row.get(4)?,
        proof_payload,
        git_tree_sha: row.get(6)?,
        worker_id: row.get(7)?,
    })
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<GateCheckpoint> {
    let state_payload = checkpoint_payload_from_ref(row.get_ref(6)?)?;
    Ok(GateCheckpoint {
        breadcrumb: row.get(0)?,
        milestone_id: row.get(1)?,
        subject: row.get(2)?,
        gate_path: row.get(3)?,
        transition_id: row.get(4)?,
        git_tree_sha: row.get(5)?,
        state_payload,
    })
}

fn checkpoint_payload_from_ref(
    value: rusqlite::types::ValueRef<'_>,
) -> rusqlite::Result<serde_json::Value> {
    use rusqlite::types::ValueRef;

    let data_type = value.data_type();
    let stored = match value {
        ValueRef::Integer(value) => serde_json::json!(value),
        ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Real,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "checkpoint REAL payload is not finite",
                    )),
                )
            })?,
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            serde_json::from_slice(bytes).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(6, data_type, Box::new(error))
            })?
        }
        ValueRef::Null => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Null,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "checkpoint payload is NULL",
                )),
            ));
        }
    };

    if let serde_json::Value::Object(object) = &stored {
        if object.len() == 1 {
            if let Some(payload) = object.get(CHECKPOINT_PAYLOAD_KEY) {
                return Ok(payload.clone());
            }
        }
    }
    Ok(stored)
}
