use crate::acdd::tasks::harness::TaskHarness;
use contextunity_forge_mcp::db::tasks_store::TasksStore;
use rusqlite::{params, types::Value as SqlValue};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub(crate) struct PersistedTaskRows(Vec<(String, Vec<Vec<SqlValue>>)>);

#[derive(Debug, PartialEq)]
pub(crate) struct SubmissionCounts {
    gates: u64,
    submissions: u64,
    revision: u64,
}

#[derive(Debug, PartialEq)]
pub(crate) struct TaskValidationState {
    pub status: String,
    pub stage: String,
    pub claim_revision: u64,
    pub worker_id: Option<String>,
    pub gates: Vec<(String, u64, String, Option<String>)>,
    pub claims: Vec<(u64, i64)>,
    pub submissions: i64,
}

pub(crate) struct BlackboardSchema {
    pub version: String,
    pub gate_column: bool,
    pub gate_indexes: i64,
}

impl TaskHarness {
    pub fn install_delivery_sql_abort(root: &Path) -> anyhow::Result<()> {
        rusqlite::Connection::open(root.join(".forge/tasks.sqlite"))?.execute_batch("CREATE TRIGGER reject_delivery_gate BEFORE INSERT ON task_gates WHEN NEW.gate='deliver' BEGIN SELECT RAISE(ABORT,'forced delivery sqlite failure'); END;")?;
        Ok(())
    }

    pub fn remove_delivery_sql_abort(root: &Path) -> anyhow::Result<()> {
        rusqlite::Connection::open(root.join(".forge/tasks.sqlite"))?
            .execute_batch("DROP TRIGGER reject_delivery_gate;")?;
        Ok(())
    }

    pub fn database_task_count(root: &Path) -> anyhow::Result<i64> {
        Ok(
            rusqlite::Connection::open(root.join(".forge/tasks.sqlite"))?.query_row(
                "SELECT COUNT(*) FROM tasks",
                [],
                |row| row.get(0),
            )?,
        )
    }

    pub fn persisted_scope_path_count(store: &TasksStore, id: &str) -> anyhow::Result<i64> {
        Ok(store.connection.query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn unfrozen_scope_path_count(store: &TasksStore, id: &str) -> anyhow::Result<i64> {
        Ok(store.connection.query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1 AND frozen=0",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn claim_count(store: &TasksStore, id: &str) -> anyhow::Result<i64> {
        Ok(store.connection.query_row(
            "SELECT count(*) FROM task_claims WHERE task_id=?1",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn latest_submission_result(store: &TasksStore, id: &str) -> anyhow::Result<(u64, Value)> {
        let (revision, text): (u64, String) = store.connection.query_row("SELECT revision,result FROM task_submissions WHERE task_id=?1 ORDER BY revision DESC LIMIT 1", [id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok((revision, serde_json::from_str(&text)?))
    }

    pub fn replace_submission_result(
        store: &TasksStore,
        id: &str,
        revision: u64,
        result: &Value,
    ) -> anyhow::Result<()> {
        store.connection.execute(
            "UPDATE task_submissions SET result=?1 WHERE task_id=?2 AND revision=?3",
            params![result.to_string(), id, revision],
        )?;
        Ok(())
    }

    pub fn blackboard_schema(store: &TasksStore) -> anyhow::Result<BlackboardSchema> {
        Ok(BlackboardSchema {
            version: store.connection.query_row("SELECT value FROM task_store_metadata WHERE key='schema_version'", [], |row| row.get(0))?,
            gate_column: store.connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_blackboard') WHERE name='gate')", [], |row| row.get(0))?,
            gate_indexes: store.connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND tbl_name='task_blackboard' AND sql LIKE '%gate%'", [], |row| row.get(0))?,
        })
    }

    pub fn persisted_descriptor(
        store: &TasksStore,
        id: &str,
    ) -> anyhow::Result<contextunity_forge_mcp::db::tasks_store::Task> {
        let text: String = store.connection.query_row(
            "SELECT descriptor FROM tasks WHERE task_id=?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn task_validation_state(
        store: &TasksStore,
        id: &str,
    ) -> anyhow::Result<TaskValidationState> {
        let current = Self::persisted_descriptor(store, id)?;
        let gates = store.connection.prepare("SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY gate,revision")?.query_map([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let claims = store
            .connection
            .prepare("SELECT revision,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")?
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let submissions = store.connection.query_row(
            "SELECT count(*) FROM task_submissions WHERE task_id=?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(TaskValidationState {
            status: current.status,
            stage: current.stage,
            claim_revision: current.claim_revision,
            worker_id: current.worker_id,
            gates,
            claims,
            submissions,
        })
    }

    pub fn latest_gate_state(
        store: &TasksStore,
        id: &str,
        stage: &str,
    ) -> anyhow::Result<(String, Option<String>)> {
        Ok(store.connection.query_row("SELECT state,evidence FROM task_gates WHERE task_id=?1 AND gate=?2 ORDER BY revision DESC LIMIT 1", params![id, stage], |row| Ok((row.get(0)?, row.get(1)?)))?)
    }

    pub fn dependency_edges(store: &TasksStore) -> anyhow::Result<Vec<(String, String)>> {
        Ok(store.connection.prepare("SELECT task_id, dependency_id FROM task_dependencies ORDER BY task_id, dependency_id")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn open_claim_count(store: &TasksStore, id: &str) -> anyhow::Result<i64> {
        Ok(store.connection.query_row(
            "SELECT COUNT(*) FROM task_claims WHERE task_id=?1 AND ended=0",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn dependency_wait_count(
        store: &TasksStore,
        id: &str,
        dependency: &str,
    ) -> anyhow::Result<i64> {
        Ok(store.connection.query_row("SELECT COUNT(*) FROM task_dependencies WHERE task_id=?1 AND dependency_id=?2 AND satisfied=0", [id, dependency], |row| row.get(0))?)
    }

    pub fn blocker_note_count(store: &TasksStore, id: &str) -> anyhow::Result<i64> {
        Ok(store.connection.query_row(
            "SELECT COUNT(*) FROM task_blackboard WHERE task_id=?1 AND topic='blockers'",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn milestone_message_count(store: &TasksStore, milestone: &str) -> anyhow::Result<i64> {
        Ok(store.connection.query_row("SELECT COUNT(*) FROM task_blackboard WHERE milestone_ref = ?1 AND task_id IS NULL AND subtask_ref IS NULL", [milestone], |row| row.get(0))?)
    }

    pub fn external_method_caller_edges(
        root: &Path,
        method: (&str, &str),
        caller: (&str, &str),
    ) -> anyhow::Result<i64> {
        let sql = rusqlite::Connection::open(root.join(".forge/code-map.sqlite"))?;
        Ok(sql.query_row("SELECT COUNT(*) FROM edges edge JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary tp ON tp.path_id=target.path_id JOIN nodes caller ON caller.node_hash=edge.src_hash JOIN path_dictionary cp ON cp.path_id=caller.path_id WHERE edge.kind='calls' AND target.name=?1 AND target.kind='method' AND tp.path=?2 AND caller.name=?3 AND cp.path=?4 AND caller.is_test=0", params![method.1, method.0, caller.1, caller.0], |row| row.get(0))?)
    }

    pub fn persisted_task_rows(store: &TasksStore, id: &str) -> anyhow::Result<PersistedTaskRows> {
        let mut tables = Vec::new();
        for table in [
            "tasks",
            "task_revisions",
            "task_claims",
            "task_gates",
            "task_findings",
            "task_scope_paths",
            "task_dependencies",
            "task_subtasks",
            "task_submissions",
        ] {
            let mut query = store.connection.prepare(&format!(
                "SELECT * FROM {table} WHERE task_id=?1 ORDER BY rowid"
            ))?;
            let columns = query.column_count();
            let rows = query
                .query_map([id], |row| {
                    (0..columns)
                        .map(|column| row.get(column))
                        .collect::<rusqlite::Result<Vec<SqlValue>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tables.push((table.into(), rows));
        }
        Ok(PersistedTaskRows(tables))
    }

    pub fn persisted_submission_counts(
        store: &TasksStore,
        id: &str,
    ) -> anyhow::Result<SubmissionCounts> {
        Ok(store.connection.query_row("SELECT (SELECT COUNT(*) FROM task_gates WHERE task_id=?1),(SELECT COUNT(*) FROM task_submissions WHERE task_id=?1),(SELECT revision FROM task_revisions WHERE task_id=?1)", [id], |row| Ok(SubmissionCounts { gates: row.get(0)?, submissions: row.get(1)?, revision: row.get(2)? }))?)
    }

    pub fn seed_unenrolled_task(
        store: &TasksStore,
        id: &str,
        milestone_ref: &str,
        descriptor: &Value,
        revision: u64,
        finding: (u64, &str),
    ) -> anyhow::Result<()> {
        store.connection.execute(
            "INSERT INTO tasks(task_id,milestone_ref,descriptor) VALUES(?1,?2,?3)",
            params![id, milestone_ref, descriptor.to_string()],
        )?;
        store.connection.execute(
            "INSERT INTO task_revisions VALUES(?1,?2)",
            params![id, revision],
        )?;
        store.connection.execute(
            "INSERT INTO task_findings VALUES(?1,?2,?3)",
            params![id, finding.0, finding.1],
        )?;
        Ok(())
    }

    pub fn set_task_descriptor(
        store: &TasksStore,
        id: &str,
        descriptor: &Value,
    ) -> anyhow::Result<()> {
        store.connection.execute(
            "UPDATE tasks SET descriptor=?2 WHERE task_id=?1",
            params![id, descriptor.to_string()],
        )?;
        Ok(())
    }

    pub fn seed_task_dependency(
        store: &TasksStore,
        id: &str,
        dependency: &str,
        revision: u64,
    ) -> anyhow::Result<()> {
        store.connection.execute(
            "INSERT INTO task_dependencies VALUES(?1,?2,?3)",
            params![id, dependency, revision],
        )?;
        Ok(())
    }

    pub fn remove_task_dependencies(store: &TasksStore, id: &str) -> anyhow::Result<()> {
        store
            .connection
            .execute("DELETE FROM task_dependencies WHERE task_id=?1", [id])?;
        Ok(())
    }

    pub fn install_subtask_revision_trigger(
        store: &TasksStore,
        source: &str,
        subtask: &str,
        target: &str,
    ) -> anyhow::Result<()> {
        let source = source.replace('\'', "''");
        let subtask = subtask.replace('\'', "''");
        let target = target.replace('\'', "''");
        store.connection.execute_batch(&format!("CREATE TRIGGER undeclared_owner_delta AFTER INSERT ON task_subtasks WHEN NEW.task_id='{source}' AND NEW.subtask_ref='{subtask}' BEGIN UPDATE task_revisions SET revision=revision+1 WHERE task_id='{target}'; END;"))?;
        Ok(())
    }

    pub fn remove_subtask_revision_trigger(store: &TasksStore) -> anyhow::Result<()> {
        store
            .connection
            .execute_batch("DROP TRIGGER undeclared_owner_delta")?;
        Ok(())
    }

    pub fn gate_evidence_json(store: &TasksStore, id: &str, stage: &str) -> anyhow::Result<String> {
        Ok(store.connection.query_row("SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1", params![id, stage], |row| row.get(0))?)
    }

    pub fn set_gate_evidence_commit(
        store: &TasksStore,
        id: &str,
        stage: &str,
        commit: &str,
    ) -> anyhow::Result<()> {
        store.connection.execute("UPDATE task_gates SET evidence=json_set(evidence, '$.commit', ?1) WHERE task_id=?2 AND gate=?3 AND state='passed'", params![commit, id, stage])?;
        Ok(())
    }

    pub fn replace_gate_evidence(
        store: &TasksStore,
        id: &str,
        stage: &str,
        evidence: &str,
    ) -> anyhow::Result<()> {
        store.connection.execute(
            "UPDATE task_gates SET evidence=?1 WHERE task_id=?2 AND gate=?3 AND state='passed'",
            params![evidence, id, stage],
        )?;
        Ok(())
    }

    pub fn read_only_descriptor(root: &Path, id: &str) -> anyhow::Result<Value> {
        let sql = rusqlite::Connection::open_with_flags(
            root.join(".forge/tasks.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let descriptor: String = sql.query_row(
            "SELECT descriptor FROM tasks WHERE task_id=?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&descriptor)?)
    }
}
