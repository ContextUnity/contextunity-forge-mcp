use super::*;

impl TasksStore {
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
        let task_gate =
            profile.index_of(&task.stage).with_context(|| {
                format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        )
            })?;
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
