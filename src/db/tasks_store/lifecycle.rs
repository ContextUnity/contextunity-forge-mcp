use super::*;

impl TasksStore {
    /// Releases only the claim revision whose document activation failed.
    pub fn release_failed_claim(&mut self, id: &str, revision: u64) -> Result<()> {
        self.assert_id(id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        if task.status != "in_progress" {
            bail!(
                "TASK_STALE_CLAIM: task '{id}'. status is '{}'. Do: leave the claim unchanged.",
                task.status
            );
        }
        if task.claim_revision != revision {
            bail!("TASK_STALE_CLAIM: task '{id}'. active claim_revision {} != requested {revision}. Do: release claim_revision {}.", task.claim_revision, task.claim_revision);
        }
        if tx.execute(
            "DELETE FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
            params![id, revision],
        )? != 1
        {
            bail!("TASK_STALE_CLAIM: task '{id}'. claim_revision {revision} has no open claim row. Do: claim the task again before modifying it.");
        }
        task.claim_revision = next_revision(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        task.status = if task.parked_worker_id.is_some() {
            "parked"
        } else {
            "ready"
        }
        .into();
        save(&tx, &task)?;
        tx.commit()?;
        Ok(())
    }
    /// Clear resume metadata only after activation and context construction succeed.
    pub(crate) fn finish_resume(&mut self, id: &str, revision: u64) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        if task.status != "in_progress" || task.claim_revision != revision {
            bail!("TASK_STALE_CLAIM: {id}. Claim changed while finishing dispatch. Do: inspect the active owner before retrying.");
        }
        if task.parked_worker_id.is_some() {
            task.parked_worker_id = None;
            task.parked_worktree = None;
            task.blocked_by = None;
            save(&tx, &task)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Claim one task under the same immediate transaction used by dispatch.
    pub fn claim(&mut self, id: &str, stage: &str, worker: &str, worktree: &str) -> Result<Task> {
        self.assert_id(id)?;
        let context = ClaimContext::from_store(self);
        let worktree = crate::core::tasks::claim_worktree(worktree)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = context.claim(&tx, id, stage, worker, &worktree)?;
        tx.commit()?;
        Ok(task)
    }

    /// Select and claim eligible work without allowing a second dispatcher to race.
    pub fn next(
        &mut self,
        worker: &str,
        worktree: &str,
        mut validate: impl FnMut(&Task, &Path) -> Result<()>,
    ) -> Result<Option<Task>> {
        if worker.trim().is_empty() {
            bail!("TASK_WORKER_INVALID: unselected. worker_id is empty. Do: supply a worker identity.");
        }
        let context = ClaimContext::from_store(self);
        let requested_worktree = crate::core::tasks::claim_worktree(worktree)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let descriptors: Vec<String> = tx
            .prepare("SELECT descriptor FROM tasks WHERE substr(task_id,1,length(?1))=?1 OR substr(task_id,1,length(?2))=?2")?
            .query_map(params![context.id_prefix, context.namespace], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut candidates = Vec::new();
        for descriptor in descriptors {
            let task: Task = serde_json::from_str(&descriptor)?;
            if !task_belongs_to(&context.repository, &context.project, &task.task_id)
                || task.worker_id.is_some()
                || !matches!(task.status.as_str(), "ready" | "parked")
                || !dependencies_ready(
                    &tx,
                    &task.task_id,
                    &context.repository,
                    context.workspace_root.as_deref(),
                )?
            {
                continue;
            }
            let profile =
                resolve_task_profile(context.workspace_root.as_deref(), &context.profile, &task)?;
            let gate = profile
                .gates
                .iter()
                .find(|gate| gate.id == task.stage)
                .context("TASK_STAGE_UNKNOWN: dispatch candidate stage is not in its profile")?;
            let independent = gate
                .independent_from
                .as_deref()
                .or_else(|| matches!(task.stage.as_str(), "review" | "deliver").then_some("build"));
            if let Some(from) = independent {
                let owner: Option<String> = tx.query_row(
                    "SELECT json_extract(evidence,'$.worker_id') FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",
                    params![task.task_id, from], |row| row.get(0)).optional()?.flatten();
                if owner.as_deref() == Some(worker) {
                    continue;
                }
            }
            let priority = if task.stage == "deliver" {
                0
            } else if task.stage == "review" {
                1
            } else if task.status == "parked" {
                if task.parked_worker_id.as_deref() != Some(worker) {
                    continue;
                }
                2
            } else {
                3
            };
            if scope_locked(&tx, &task, &context.repository, &context.project)? {
                continue;
            }
            candidates.push((priority, task));
        }
        candidates.sort_by(|(left_priority, left), (right_priority, right)| {
            (left_priority, left.wave_index, &left.task_id).cmp(&(
                right_priority,
                right.wave_index,
                &right.task_id,
            ))
        });
        let Some((_, task)) = candidates.into_iter().next() else {
            tx.commit()?;
            return Ok(None);
        };
        let selected_worktree = if task.status == "parked" {
            PathBuf::from(
                task.parked_worktree
                    .as_deref()
                    .context("TASK_WORKTREE_AFFINITY: parked worktree is missing")?,
            )
        } else {
            requested_worktree
        };
        validate(&task, &selected_worktree)?;
        let claimed = context.claim(&tx, &task.task_id, &task.stage, worker, &selected_worktree)?;
        tx.commit()?;
        Ok(Some(claimed))
    }
    /// Performs reset.
    pub fn reset(&mut self, id: &str) -> Result<Task> {
        self.assert_id(id)?;
        let workspace_root = self.workspace_root.clone();
        let base_profile = self.profile.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        let profile = resolve_task_profile(workspace_root.as_deref(), &base_profile, &task)?;
        if matches!(task.status.as_str(), "completed" | "cancelled") {
            task.gate = 0;
            task.stage = profile.gate_id(0).to_owned();
            task.completed_at = None;
            task.receipt = None;
            task.spec.status = Some("ready".into());
        }
        end_claim(&tx, id)?;
        task.claim_revision = next_revision(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        task.status = "ready".into();
        task.parked_worker_id = None;
        task.parked_worktree = None;
        task.blocked_by = None;
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
        let repository = self.repository.clone();
        let project = self.project.clone();
        let id_prefix = self.id_prefix.clone();
        let namespace = self.namespace.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        if matches!(task.status.as_str(), "completed" | "cancelled") {
            bail!("TASK_TERMINAL: cannot extend scope of completed or cancelled task; reopen or reset the task first (e.g. `task reset <task-id>`)");
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
        let other_scopes: Vec<(String, PathBuf, String)> = tx
            .prepare(
                "SELECT p.task_id, p.path, json_extract(t.descriptor, '$.status') FROM task_scope_paths p \
                 JOIN tasks t ON t.task_id = p.task_id \
                 WHERE p.task_id != ?1 AND t.milestone_ref = ?2 \
                   AND (substr(p.task_id, 1, length(?3)) = ?3 OR substr(p.task_id, 1, length(?4)) = ?4) \
                    AND json_extract(t.descriptor, '$.status') NOT IN ('completed', 'cancelled') ORDER BY p.task_id",
            )?
            .query_map(params![id, task.milestone_ref, id_prefix, namespace], |r| {
                let owner: String = r.get(0)?;
                let p: String = r.get(1)?;
                Ok((owner, PathBuf::from(p), r.get(2)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let other_scopes: Vec<(String, PathBuf, String)> = other_scopes
            .into_iter()
            .filter(|(owner, _, _)| task_belongs_to(&repository, &project, owner))
            .collect();
        let mut blockers = std::collections::BTreeSet::new();
        for value in paths {
            let path = crate::core::tasks::relative_path(value)?;
            if protected_acdd_path(&path) {
                bail!("TASK_SCOPE_PROTECTED: paths under .forge/acdd/** must be admitted in the initial task contract scope");
            }
            for (owner, other_path, status) in &other_scopes {
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
                    if status == "in_progress" && task.status == "in_progress" {
                        blockers.insert(owner.clone());
                    } else {
                        bail!("TASK_SCOPE_CONFLICT: {id}. Path '{value}' belongs to task '{owner}'. Do: complete the scope owner before extending this task.");
                    }
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
        if !blockers.is_empty() {
            task.parked_worker_id = task.worker_id.take();
            task.parked_worktree = task.worktree.take();
            task.blocked_by = blockers.first().cloned();
            task.status = "parked".into();
            end_claim(&tx, id)?;
            task.claim_revision = next_revision(&tx, id)?;
            for holder in &blockers {
                tx.execute(
                    "INSERT OR IGNORE INTO task_dependencies VALUES(?1,?2,0)",
                    params![id, holder],
                )?;
            }
            tx.execute("INSERT INTO task_blackboard(milestone_ref,task_id,gate,author,topic,payload,created_at) VALUES(?1,?2,?3,?4,'blockers',?5,?6)", params![format!("{namespace}{}", task.milestone_ref), id, task.stage, task.parked_worker_id, format!("Scope extension parked behind active owners: {}. Resume in {} after their completion.", blockers.into_iter().collect::<Vec<_>>().join(", "), task.parked_worktree.as_deref().unwrap_or_default()), now()])?;
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
        let repository = self.repository.clone();
        let project = self.project.clone();
        let id_prefix = self.id_prefix.clone();
        let namespace = self.namespace.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let targets: Vec<String> = tx
            .prepare("SELECT task_id FROM tasks WHERE ((?1 IS NOT NULL AND task_id=?1) OR (?2 IS NOT NULL AND milestone_ref=?2)) AND (substr(task_id,1,length(?3))=?3 OR substr(task_id,1,length(?4))=?4)")?
            .query_map(params![id, milestone, id_prefix, namespace], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|task_id| task_belongs_to(&repository, &project, task_id))
            .collect();
        let mut revoked_claims = 0;
        for id in &targets {
            let task = load(&tx, id)?.context(task_not_found_message(id))?;
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
            let task = load(&tx, id)?.context(task_not_found_message(id))?;
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

struct ClaimContext {
    repository: String,
    project: String,
    id_prefix: String,
    namespace: String,
    workspace_root: Option<PathBuf>,
    profile: std::sync::Arc<crate::core::tasks::profile::GateProfile>,
}

impl ClaimContext {
    fn from_store(store: &TasksStore) -> Self {
        Self {
            repository: store.repository.clone(),
            project: store.project.clone(),
            id_prefix: store.id_prefix.clone(),
            namespace: store.namespace.clone(),
            workspace_root: store.workspace_root.clone(),
            profile: store.profile.clone(),
        }
    }

    fn claim(
        &self,
        tx: &Connection,
        id: &str,
        stage: &str,
        worker: &str,
        worktree: &Path,
    ) -> Result<Task> {
        if worker.trim().is_empty() {
            bail!("TASK_WORKER_INVALID: {id}. worker_id is empty. Do: supply a worker identity.");
        }
        let mut task = load(tx, id)?.context(task_not_found_message(id))?;
        let profile = resolve_task_profile(self.workspace_root.as_deref(), &self.profile, &task)?;
        if task.status == "parked"
            && (task.parked_worker_id.as_deref() != Some(worker)
                || task.parked_worktree.as_deref().map(Path::new) != Some(worktree))
        {
            bail!("TASK_WORKTREE_AFFINITY: {id}. Parked for worker '{}' in worktree '{}'. Do: resume with the preserved worker and worktree.", task.parked_worker_id.as_deref().unwrap_or_default(), task.parked_worktree.as_deref().unwrap_or_default());
        }
        if let Some(root) = self.workspace_root.as_deref() {
            verify_dirty_acdd_scope(root, &task.initial_scope)?;
            if worktree != root {
                verify_dirty_acdd_scope(worktree, &task.initial_scope)?;
            }
        }
        if task.worker_id.is_some() {
            let (worker_id, active_worktree, claimed_at): (String, String, i64) = tx.query_row(
                "SELECT worker_id,worktree,claimed_at FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![id, task.claim_revision],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            return Err(TaskAlreadyClaimed {
                task_id: task.task_id,
                worker_id,
                worktree: active_worktree,
                revision: task.claim_revision,
                claimed_at,
            }
            .into());
        }
        if task.status != "ready" && task.status != "parked" {
            bail!("TASK_NOT_READY: task '{id}'. status is '{}' (active stage '{}'). Do: claim only ready tasks at their active stage.", task.status, task.stage);
        }
        let dependencies =
            unsatisfied_dependencies(tx, id, &self.repository, self.workspace_root.as_deref())?;
        if !dependencies.is_empty() {
            bail!("TASK_NOT_READY: task '{id}' has unsatisfied dependencies: [{}]. Do: complete dependencies before claiming stage '{}'.", dependencies.join(", "), task.stage);
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
        check_stage(id, &task.stage, stage)?;
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
                   AND (substr(p.task_id, 1, length(?3)) = ?3 OR substr(p.task_id, 1, length(?4)) = ?4) \
                   AND json_extract(t.descriptor, '$.status') = 'in_progress'",
            )?
            .query_map(params![id, task.milestone_ref, self.id_prefix, self.namespace], |row| {
                Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?)))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let other_scopes: Vec<(String, PathBuf)> = other_scopes
            .into_iter()
            .filter(|(owner, _)| task_belongs_to(&self.repository, &self.project, owner))
            .collect();
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
        task.claim_revision = next_revision(tx, id)?;
        task.worker_id = Some(worker.into());
        task.worktree = Some(worktree.to_string_lossy().into_owned());
        task.status = "in_progress".into();
        tx.execute("INSERT INTO task_claims(task_id,revision,worker_id,worktree,claimed_at) VALUES(?1,?2,?3,?4,?5)",params![id,task.claim_revision,worker,task.worktree,now()])?;
        save(tx, &task)?;

        Ok(task)
    }
}

fn scope_locked(conn: &Connection, task: &Task, repository: &str, project: &str) -> Result<bool> {
    let owners: Vec<String> = conn.prepare("SELECT descriptor FROM tasks WHERE milestone_ref=?1 AND task_id!=?2 AND json_extract(descriptor,'$.status')='in_progress'")?
        .query_map(params![task.milestone_ref, task.task_id], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
    for owner in owners {
        let owner: Task = serde_json::from_str(&owner)?;
        if !task_belongs_to(repository, project, &owner.task_id) {
            continue;
        }
        if owner.spec.scope.iter().any(|left| {
            task.spec.scope.iter().any(|right| {
                crate::core::tasks::path_matches_scope_entry(left, right)
                    || crate::core::tasks::path_matches_scope_entry(right, left)
            })
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}
