use super::*;

pub(super) fn read_subtask(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubtaskSpec> {
    let status: String = row.get(2)?;
    let status = crate::core::tasks::SubtaskStatus::parse(&status).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, error.into())
    })?;
    Ok(SubtaskSpec {
        subtask_ref: row.get(0)?,
        title: row.get(1)?,
        status: status.as_str().into(),
        evidence: row.get(3)?,
    })
}

impl TasksStore {
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
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[task_id.to_owned()], false)?;
        let mut task = load(&tx, task_id)?.context("TASK_NOT_FOUND")?;
        if matches!(task.status.as_str(), "completed" | "cancelled") {
            bail!("TASK_TERMINAL: cannot add subtasks to completed or cancelled task; reopen or reset the task first (e.g. `task reset <task-id>`)");
        }
        let updated_at = now();
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO task_subtasks(task_id, subtask_ref, title, status, evidence, updated_at) VALUES(?1, ?2, ?3, 'ready', NULL, ?4)",
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
            .query_map([task_id], read_subtask)?
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
        let status = crate::core::tasks::SubtaskStatus::parse(status)?.as_str();
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[task_id.to_owned()], false)?;
        let mut task = load(&tx, task_id)?.context("TASK_NOT_FOUND")?;
        if matches!(task.status.as_str(), "completed" | "cancelled") {
            bail!("TASK_TERMINAL: cannot update subtasks of completed or cancelled task; reopen or reset the task first (e.g. `task reset <task-id>`)");
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
            .query_map([task_id], read_subtask)?
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
        let _snapshot = super::recovery::read_snapshot(&self.connection)?;
        self.assert_id(task_id)?;
        let mut stmt = self.connection.prepare(
            "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
        )?;
        let subtasks = stmt
            .query_map([task_id], read_subtask)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if subtasks.is_empty() {
            let task = self.inspect(task_id)?;
            Ok(task.spec.subtasks)
        } else {
            Ok(subtasks)
        }
    }
}
