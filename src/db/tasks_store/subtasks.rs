use super::*;

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
}
