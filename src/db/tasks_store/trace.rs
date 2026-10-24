use super::{Connection, TasksStore};
use anyhow::{bail, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// Maximum number of UTF-8 bytes stored in a command trace output snippet.
pub const MAX_OUTPUT_SNIPPET_BYTES: usize = 2 * 1024;

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
                OR length(CAST(output_snippet AS BLOB)) <= 2048
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

impl TasksStore {
    /// Persist a command execution trace and return its SQLite row identifier.
    pub fn record_execution_trace(&self, trace: &GateExecutionTrace) -> Result<i64> {
        record_trace(&self.connection, trace)
    }

    /// Insert or replace the checkpoint associated with one turn breadcrumb.
    pub fn save_checkpoint(&self, checkpoint: &GateCheckpoint) -> Result<()> {
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
        let state_payload = serde_json::to_string(&checkpoint.state_payload)?;
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
}
