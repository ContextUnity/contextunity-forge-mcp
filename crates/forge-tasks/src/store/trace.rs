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
    let schema: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='gate_execution_traces'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let expected = format!("<= {MAX_OUTPUT_SNIPPET_BYTES}");
    if !proof_exists || !schema.as_ref().is_some_and(|s| s.contains(&expected)) {
        bail!("unsupported tasks schema; use recovery tooling");
    }
    Ok(())
}

/// Maximum number of UTF-8 bytes stored in a command trace output snippet.
pub const MAX_OUTPUT_SNIPPET_BYTES: usize = forge_core::response::MAX_OUTPUT_BYTES;

/// Proof-critical facts extracted from complete command output before presentation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandExecutionProof {
    pub passed: Option<u64>,
    pub failed: Option<u64>,
    pub seam_ref: Option<String>,
    #[serde(default)]
    pub seam_verified: bool,
    #[serde(default)]
    pub passed_tests: std::collections::BTreeSet<String>,
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

/// Create the gate transition, command trace, and checkpoint tables.
pub(super) fn migrate(connection: &Connection) -> Result<()> {
    let expected = format!("<= {MAX_OUTPUT_SNIPPET_BYTES}");
    let schema: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='gate_execution_traces'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = schema {
        if !existing.contains(&expected) {
            bail!("unsupported tasks schema; use recovery tooling");
        }
    }

    connection.execute_batch(&format!(
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
                OR length(CAST(output_snippet AS BLOB)) <= {MAX_OUTPUT_SNIPPET_BYTES}
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
        );

        CREATE TABLE IF NOT EXISTS command_execution_proofs (
            run_id INTEGER NOT NULL REFERENCES gate_execution_runs(id) ON DELETE CASCADE,
            command_id TEXT NOT NULL,
            attempt_id INTEGER NOT NULL,
            payload TEXT NOT NULL,
            PRIMARY KEY(run_id, command_id, attempt_id)
        );"
    ))?;
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
        "DELETE FROM gate_transitions WHERE milestone_id=?1
         AND id NOT IN (SELECT transition_id FROM gate_checkpoints WHERE milestone_id=?1 AND transition_id IS NOT NULL)",
        [milestone_identity],
    )?;
    Ok(())
}

impl TasksStore {
    /// Record accepted root-gate proof slices for their individual milestone tasks.
    pub fn record_task_proof_advances(
        &self,
        source: &GateTransition,
        task_slices: &[(String, serde_json::Value)],
    ) -> Result<usize> {
        if source.subject != "milestone" || source.action != "pass" {
            bail!("TASK_PROOF_FANOUT_INVALID: source must be a passed milestone transition");
        }
        if !source.milestone_id.starts_with(&self.namespace) {
            bail!("TASK_PROOF_FANOUT_SCOPE: milestone is outside this project namespace");
        }
        for (field, value) in [
            ("milestone_id", source.milestone_id.as_str()),
            ("gate_path", source.gate_path.as_str()),
            ("proof_kind", source.proof_kind.as_str()),
            ("git_tree_sha", source.git_tree_sha.as_str()),
            ("worker_id", source.worker_id.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("TASK_PROOF_FANOUT_INVALID: {field} must not be empty");
            }
        }

        let transaction = self.connection.unchecked_transaction()?;
        let mut inserted = 0;
        for (subject, payload) in task_slices {
            if subject.trim().is_empty() || !subject.starts_with(&self.id_prefix) {
                bail!("TASK_PROOF_FANOUT_SCOPE: task subject is outside this project namespace");
            }
            let proof_payload = serde_json::to_string(payload)?;
            let latest: Option<(String, String, String, String)> = transaction
                .query_row(
                    "SELECT action, proof_kind, proof_payload, git_tree_sha
                 FROM gate_transitions
                 WHERE milestone_id=?1 AND subject=?2 AND gate_path=?3
                 ORDER BY id DESC LIMIT 1",
                    params![source.milestone_id, subject, source.gate_path,],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;
            if latest.is_some_and(|(action, proof_kind, previous_payload, git_tree_sha)| {
                action == "advance"
                    && proof_kind == source.proof_kind
                    && previous_payload == proof_payload
                    && git_tree_sha == source.git_tree_sha
            }) {
                continue;
            }
            transaction.execute(
                "INSERT INTO gate_transitions (
                    milestone_id, subject, gate_path, action, proof_kind, proof_payload,
                    git_tree_sha, worker_id
                 ) VALUES (?1, ?2, ?3, 'advance', ?4, ?5, ?6, ?7)",
                params![
                    source.milestone_id,
                    subject,
                    source.gate_path,
                    source.proof_kind,
                    proof_payload,
                    source.git_tree_sha,
                    source.worker_id,
                ],
            )?;
            inserted += 1;
        }
        transaction.commit()?;
        Ok(inserted)
    }

    /// List the latest accepted task-proof advance for each requested root gate path.
    pub fn latest_task_proof_advances(
        &self,
        milestone_identity: &str,
        task_id: &str,
        gate_paths: &[String],
    ) -> Result<Vec<GateTransition>> {
        if gate_paths.is_empty() {
            return Ok(Vec::new());
        }
        if !milestone_identity.starts_with(&self.namespace) || !task_id.starts_with(&self.id_prefix)
        {
            bail!("TASK_PROOF_CONTEXT_SCOPE: task proof lookup is outside this project namespace");
        }
        let placeholders = std::iter::repeat_n("?", gate_paths.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT milestone_id, subject, gate_path, action, proof_kind,
                    proof_payload, git_tree_sha, worker_id
             FROM gate_transitions
             WHERE milestone_id=? AND subject=? AND action='advance'
               AND gate_path IN ({placeholders})
             ORDER BY id DESC"
        );
        let mut values = Vec::with_capacity(gate_paths.len() + 2);
        values.push(rusqlite::types::Value::Text(milestone_identity.to_owned()));
        values.push(rusqlite::types::Value::Text(task_id.to_owned()));
        values.extend(gate_paths.iter().cloned().map(rusqlite::types::Value::Text));
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(
            rusqlite::params_from_iter(values.iter()),
            transition_from_row,
        )?;
        let mut latest_by_path = std::collections::BTreeMap::new();
        for transition in rows {
            let transition = transition?;
            latest_by_path
                .entry(transition.gate_path.clone())
                .or_insert(transition);
        }
        Ok(gate_paths
            .iter()
            .filter_map(|path| latest_by_path.remove(path))
            .collect())
    }

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
            bail!("TASK_CHECKPOINT_OWNED: milestone checkpoints are engine-owned");
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
                &milestone_id,
                &subject,
                &gate_path,
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
        self.save_checkpoint_validated(checkpoint, false)
    }

    pub(crate) fn save_engine_checkpoint(&self, checkpoint: &GateCheckpoint) -> Result<()> {
        self.save_checkpoint_validated(checkpoint, true)
    }

    fn save_checkpoint_validated(
        &self,
        checkpoint: &GateCheckpoint,
        allow_engine_owned: bool,
    ) -> Result<()> {
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
        if !allow_engine_owned
            && checkpoint.transition_id.is_none()
            && is_engine_owned_checkpoint(&subject, &gate_path)
        {
            bail!("TASK_CHECKPOINT_OWNED: milestone checkpoints are engine-owned");
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
            bail!("TASK_CHECKPOINT_OWNED: milestone checkpoints are engine-owned");
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
    if breadcrumb.is_empty()
        || breadcrumb.len() > forge_core::config::MAX_METADATA_BYTES
        || breadcrumb.contains('\0')
    {
        bail!(
            "TASK_CHECKPOINT_INVALID: breadcrumb must contain 1..={} bytes",
            forge_core::config::MAX_METADATA_BYTES
        );
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

fn is_engine_owned_checkpoint(subject: &str, _gate_path: &str) -> bool {
    subject == "milestone"
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

#[cfg(test)]
mod task_proof_advance_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepted_task_proof_advances_are_idempotent_and_track_new_snapshots() -> Result<()> {
        let store = TasksStore::open_in_memory("repository", "project")?;
        let milestone_id = store.milestone_scope_ref("milestone")?;
        let task_id = "project/milestone:contract";
        let payload = json!({
            "goal": "preserve the task scoped proof",
            "cited_invariants": ["INV-TEST"],
        });
        let mut source = GateTransition {
            milestone_id: milestone_id.clone(),
            subject: "milestone".into(),
            gate_path: "blueprint_synthesis.synthesize".into(),
            action: "pass".into(),
            proof_kind: "blueprint".into(),
            proof_payload: json!({}),
            git_tree_sha: "tree-one".into(),
            worker_id: "architect".into(),
        };

        assert_eq!(
            store.record_task_proof_advances(&source, &[(task_id.into(), payload.clone())])?,
            1
        );
        assert_eq!(
            store.record_task_proof_advances(&source, &[(task_id.into(), payload.clone())])?,
            0
        );

        source.git_tree_sha = "tree-two".into();
        assert_eq!(
            store.record_task_proof_advances(&source, &[(task_id.into(), payload.clone())])?,
            1
        );
        let replacement = json!({"goal": "temporary replacement"});
        assert_eq!(
            store.record_task_proof_advances(&source, &[(task_id.into(), replacement.clone())])?,
            1
        );
        assert_eq!(
            store.record_task_proof_advances(&source, &[(task_id.into(), payload.clone())])?,
            1,
            "A -> B -> A must append a new latest A row even when A exists historically"
        );
        let rows = store.latest_task_proof_advances(
            &milestone_id,
            task_id,
            &[source.gate_path.clone()],
        )?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].git_tree_sha, "tree-two");
        assert_eq!(rows[0].proof_payload, payload);
        Ok(())
    }
}
