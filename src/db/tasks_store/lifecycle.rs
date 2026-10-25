use super::*;

/// One persisted command gate execution bound to an active claim and Git candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimBoundExecutionRun {
    /// Database row id used to group command traces from this run.
    pub run_id: i64,
    /// Owning milestone identifier.
    pub milestone_id: String,
    /// Task or milestone whose command gate ran.
    pub subject: String,
    /// Hierarchical gate path for the execution.
    pub gate_path: String,
    /// Task claim revision active when the run began.
    pub claim_revision: u64,
    /// Contract revision active when the run began.
    pub contract_revision: u64,
    /// Worker identity that owns the active claim.
    pub worker_id: String,
    /// Canonical claimed worktree path.
    pub worktree: String,
    /// Digest of HEAD, tracked changes, and Git-visible untracked files.
    pub candidate_fingerprint: String,
    /// Run state: `running` until all reserved attempts are persisted, then `completed`.
    pub status: String,
}

/// A persisted trace with its run provenance and global SQLite insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimBoundExecutionTrace {
    /// Claim, worktree, and candidate provenance shared by this trace's run.
    pub run: ClaimBoundExecutionRun,
    /// Persisted command result.
    pub trace: GateExecutionTrace,
    /// Monotonic row id used to prove execution order across gate paths.
    pub trace_order: i64,
}

impl TasksStore {
    /// Begin one execution run for the currently claimed task gate and Git candidate.
    pub fn start_execution_run(
        &mut self,
        task_id: &str,
        gate_path: &str,
        worktree: &Path,
        candidate_fingerprint: &str,
    ) -> Result<ClaimBoundExecutionRun> {
        self.assert_id(task_id)?;
        let stage = gate_path
            .strip_prefix("tasks.")
            .filter(|stage| !stage.trim().is_empty())
            .context("TASK_TRACE_INVALID: task execution gate path must be 'tasks.<stage>'")?;
        if candidate_fingerprint.trim().is_empty() {
            bail!("TASK_TRACE_INVALID: candidate fingerprint must not be empty");
        }
        let claimed_worktree = worktree
            .canonicalize()
            .context("TASK_STALE_CLAIM: claimed worktree is unavailable")?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let task = load(&tx, task_id)?.context(task_not_found_message(task_id))?;
        if task.status != "in_progress" || task.stage != stage {
            bail!(
                "TASK_STALE_CLAIM: task '{task_id}' is not actively claimed at gate '{gate_path}'"
            );
        }
        let worker_id = task
            .worker_id
            .as_deref()
            .filter(|worker| !worker.trim().is_empty())
            .context("TASK_STALE_CLAIM: active worker identity is missing")?;
        let task_worktree = task
            .worktree
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .context("TASK_STALE_CLAIM: active worktree is missing")?;
        let task_worktree = Path::new(task_worktree)
            .canonicalize()
            .context("TASK_STALE_CLAIM: active worktree is unavailable")?;
        if task_worktree != claimed_worktree {
            bail!("TASK_STALE_CLAIM: execution root does not match the active claim worktree");
        }
        let active_claim: Option<(String, String)> = tx
            .query_row(
                "SELECT worker_id, worktree FROM task_claims
                 WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![task_id, task.claim_revision],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (claim_worker, claim_worktree) =
            active_claim.context("TASK_STALE_CLAIM: active claim row is missing")?;
        if claim_worker != worker_id || Path::new(&claim_worktree).canonicalize()? != task_worktree
        {
            bail!("TASK_STALE_CLAIM: active claim row does not match the task descriptor");
        }
        let milestone_id = task
            .task_id
            .rsplit('/')
            .next()
            .and_then(|tail| tail.split_once(':').map(|(milestone, _)| milestone))
            .filter(|milestone| !milestone.is_empty())
            .context("TASK_TRACE_INVALID: task id does not contain a milestone id")?
            .to_owned();
        tx.execute(
            "INSERT INTO gate_execution_runs (
                milestone_id, subject, gate_path, claim_revision, contract_revision,
                worker_id, worktree, candidate_fingerprint, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'running')",
            params![
                milestone_id,
                task_id,
                gate_path,
                task.claim_revision,
                task.contract_revision,
                worker_id,
                task_worktree.to_string_lossy(),
                candidate_fingerprint,
            ],
        )?;
        let run = ClaimBoundExecutionRun {
            run_id: tx.last_insert_rowid(),
            milestone_id,
            subject: task_id.to_owned(),
            gate_path: gate_path.to_owned(),
            claim_revision: task.claim_revision,
            contract_revision: task.contract_revision,
            worker_id: worker_id.to_owned(),
            worktree: task_worktree.to_string_lossy().into_owned(),
            candidate_fingerprint: candidate_fingerprint.to_owned(),
            status: "running".into(),
        };
        tx.commit()?;
        Ok(run)
    }

    /// Atomically reserve the next attempt number for a command in an execution run.
    pub fn reserve_execution_attempt(&mut self, run_id: i64, command_id: &str) -> Result<i64> {
        if command_id.trim().is_empty() {
            bail!("TASK_TRACE_INVALID: command id must not be empty");
        }
        let repository = self.repository.clone();
        let project = self.project.clone();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let run = execution_run_row(&tx, run_id, &repository, &project)?;
        if run.status != "running" {
            bail!(
                "TASK_TRACE_INVALID: command attempts can only be reserved in a running execution"
            );
        }
        let maximum: i64 = tx.query_row(
            "SELECT COALESCE(MAX(attempt_id), 0) FROM (
                 SELECT attempt_id FROM gate_execution_traces
                 WHERE milestone_id=?1 AND subject=?2 AND gate_path=?3 AND command_id=?4
                 UNION ALL
                 SELECT attempt_id FROM gate_execution_attempts
                 WHERE milestone_id=?1 AND subject=?2 AND gate_path=?3 AND command_id=?4
             )",
            params![run.milestone_id, run.subject, run.gate_path, command_id],
            |row| row.get(0),
        )?;
        let attempt_id = maximum
            .checked_add(1)
            .context("TASK_TRACE_INVALID: command attempt counter overflowed")?;
        tx.execute(
            "INSERT INTO gate_execution_attempts (
                run_id, milestone_id, subject, gate_path, command_id, attempt_id, state
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'reserved')",
            params![
                run_id,
                run.milestone_id,
                run.subject,
                run.gate_path,
                command_id,
                attempt_id,
            ],
        )?;
        tx.commit()?;
        Ok(attempt_id)
    }

    /// Persist a trace and its provenance together, completing its reserved attempt atomically.
    pub fn record_claim_bound_execution_trace(
        &mut self,
        run_id: i64,
        trace: &GateExecutionTrace,
    ) -> Result<()> {
        let repository = self.repository.clone();
        let project = self.project.clone();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let run = execution_run_row(&tx, run_id, &repository, &project)?;
        if run.status != "running"
            || trace.milestone_id != run.milestone_id
            || trace.subject != run.subject
            || trace.gate_path != run.gate_path
        {
            bail!("TASK_TRACE_INVALID: trace does not belong to its active execution run");
        }
        let reserved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM gate_execution_attempts
             WHERE run_id=?1 AND command_id=?2 AND attempt_id=?3 AND state='reserved')",
            params![run_id, trace.command_id, trace.attempt_id],
            |row| row.get(0),
        )?;
        if !reserved {
            bail!("TASK_TRACE_INVALID: trace has no matching reserved command attempt");
        }
        let trace_id = super::trace::record_trace(&tx, trace)?;
        tx.execute(
            "UPDATE gate_execution_attempts
             SET state='completed', trace_id=?4, trace_status=?5, exit_code=?6
             WHERE run_id=?1 AND command_id=?2 AND attempt_id=?3 AND state='reserved'",
            params![
                run_id,
                trace.command_id,
                trace.attempt_id,
                trace_id,
                trace.status,
                trace.exit_code,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Mark an execution run complete only after every reserved command has a persisted trace.
    pub fn complete_execution_run(&mut self, run_id: i64) -> Result<()> {
        let repository = self.repository.clone();
        let project = self.project.clone();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let run = execution_run_row(&tx, run_id, &repository, &project)?;
        if run.status != "running" {
            bail!("TASK_TRACE_INVALID: execution run is no longer running");
        }
        let (attempts, pending): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(state != 'completed'), 0)
             FROM gate_execution_attempts WHERE run_id=?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if attempts == 0 || pending != 0 {
            bail!(
                "TASK_TRACE_INVALID: execution run has no commands or contains an unrecorded attempt"
            );
        }
        tx.execute(
            "UPDATE gate_execution_runs SET status='completed' WHERE id=?1 AND status='running'",
            [run_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Return the newest run matching the active claim and exact Git candidate fingerprint.
    pub fn latest_claim_bound_execution_run(
        &self,
        task_id: &str,
        gate_path: &str,
        claim_revision: u64,
        contract_revision: u64,
        worker_id: &str,
        worktree: &str,
    ) -> Result<Option<ClaimBoundExecutionRun>> {
        self.assert_id(task_id)?;
        self.connection
            .query_row(
                "SELECT id, milestone_id, subject, gate_path, claim_revision,
                        contract_revision, worker_id, worktree, candidate_fingerprint, status
                 FROM gate_execution_runs
                 WHERE subject=?1 AND gate_path=?2 AND claim_revision=?3
                   AND contract_revision=?4 AND worker_id=?5 AND worktree=?6
                 ORDER BY id DESC LIMIT 1",
                params![
                    task_id,
                    gate_path,
                    claim_revision,
                    contract_revision,
                    worker_id,
                    worktree,
                ],
                execution_run_from_row,
            )
            .optional()
            .context("TASK_TRACE_READ_FAILED: latest claim-bound execution could not be read")
    }

    /// Read all persisted traces belonging to one claim-bound execution run.
    pub fn claim_bound_execution_traces(
        &self,
        run_id: i64,
    ) -> Result<Vec<ClaimBoundExecutionTrace>> {
        let run = execution_run_row(&self.connection, run_id, &self.repository, &self.project)?;
        let mut statement = self.connection.prepare(
            "SELECT t.id, t.milestone_id, t.subject, t.gate_path, t.attempt_id,
                    t.command_id, t.command_str, t.status, t.exit_code, t.log_path,
                    t.output_snippet, t.stdout_sha256, t.duration_ms
             FROM gate_execution_attempts AS a
             JOIN gate_execution_traces AS t ON t.id=a.trace_id
             WHERE a.run_id=?1 AND a.state='completed'
             ORDER BY t.id",
        )?;
        let traces = statement.query_map([run_id], |row| {
            Ok(ClaimBoundExecutionTrace {
                run: run.clone(),
                trace_order: row.get(0)?,
                trace: GateExecutionTrace {
                    milestone_id: row.get(1)?,
                    subject: row.get(2)?,
                    gate_path: row.get(3)?,
                    attempt_id: row.get(4)?,
                    command_id: row.get(5)?,
                    command_str: row.get(6)?,
                    status: row.get(7)?,
                    exit_code: row.get(8)?,
                    log_path: row.get(9)?,
                    output_snippet: row.get(10)?,
                    stdout_sha256: row.get(11)?,
                    duration_ms: row.get(12)?,
                },
            })
        })?;
        traces
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("TASK_TRACE_READ_FAILED: claim-bound traces could not be decoded")
    }

    /// Read persisted command traces for one task, optionally limited to one gate path.
    pub fn execution_traces(
        &self,
        task_id: &str,
        gate_path: Option<&str>,
    ) -> Result<Vec<GateExecutionTrace>> {
        self.assert_id(task_id)?;
        let mut statement = self.connection.prepare(
            "SELECT milestone_id, subject, gate_path, attempt_id, command_id, command_str,
                    status, exit_code, log_path, output_snippet, stdout_sha256, duration_ms
             FROM gate_execution_traces
             WHERE subject=?1 AND (?2 IS NULL OR gate_path=?2)
             ORDER BY id",
        )?;
        let traces = statement.query_map(params![task_id, gate_path], |row| {
            Ok(GateExecutionTrace {
                milestone_id: row.get(0)?,
                subject: row.get(1)?,
                gate_path: row.get(2)?,
                attempt_id: row.get(3)?,
                command_id: row.get(4)?,
                command_str: row.get(5)?,
                status: row.get(6)?,
                exit_code: row.get(7)?,
                log_path: row.get(8)?,
                output_snippet: row.get(9)?,
                stdout_sha256: row.get(10)?,
                duration_ms: row.get(11)?,
            })
        })?;
        traces
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("TASK_TRACE_READ_FAILED: persisted command traces could not be decoded")
    }

    /// Releases only the claim revision whose document activation failed.
    pub fn release_failed_claim(&mut self, id: &str, revision: u64) -> Result<()> {
        self.assert_id(id)?;
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        if task.status != "in_progress" {
            bail!(
                "TASK_STALE_CLAIM: task '{id}'. status is '{}'. Do: leave the claim unchanged.",
                task.status
            );
        }
        if task.claim_revision != revision {
            bail!(
                "TASK_STALE_CLAIM: task '{id}'. active claim_revision {} != requested {revision}. Do: release claim_revision {}.",
                task.claim_revision,
                task.claim_revision
            );
        }
        if tx.execute(
            "DELETE FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
            params![id, revision],
        )? != 1
        {
            bail!(
                "TASK_STALE_CLAIM: task '{id}'. claim_revision {revision} has no open claim row. Do: claim the task again before modifying it."
            );
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
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        if task.status != "in_progress" || task.claim_revision != revision {
            bail!(
                "TASK_STALE_CLAIM: {id}. Claim changed while finishing dispatch. Do: inspect the active owner before retrying."
            );
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
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
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
            bail!(
                "TASK_WORKER_INVALID: unselected. worker_id is empty. Do: supply a worker identity."
            );
        }
        let context = ClaimContext::from_store(self);
        let requested_worktree = crate::core::tasks::claim_worktree(worktree)?;
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
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
        tx.enroll(std::slice::from_ref(&task.task_id), false)?;
        validate(&task, &selected_worktree)?;
        let claimed = context.claim(&tx, &task.task_id, &task.stage, worker, &selected_worktree)?;
        tx.commit()?;
        Ok(Some(claimed))
    }
    /// Performs reset.
    pub fn reset(&mut self, id: &str) -> Result<Task> {
        self.reset_with_pre_persist(id, || Ok(()))
    }

    pub(crate) fn reset_with_pre_persist(
        &mut self,
        id: &str,
        before_persist: impl FnOnce() -> Result<()>,
    ) -> Result<Task> {
        self.assert_id(id)?;
        let workspace_root = self.workspace_root.clone();
        let base_profile = self.profile.clone();
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
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
        before_persist()?;
        tx.commit()?;
        Ok(task)
    }
    /// Performs reopen, resetting a task back to ready state.
    pub fn reopen(&mut self, id: &str) -> Result<Task> {
        self.reset(id)
    }
    /// Performs extend scope.
    pub fn extend_scope(&mut self, id: &str, paths: &[String], root: &Path) -> Result<Task> {
        self.extend_scope_with_pre_commit(id, paths, root, |_| Ok(None))
    }

    pub(crate) fn extend_scope_with_pre_commit(
        &mut self,
        id: &str,
        paths: &[String],
        root: &Path,
        before_commit: impl FnOnce(&mut Task) -> Result<Option<super::recovery::ManifestReplacement>>,
    ) -> Result<Task> {
        self.assert_id(id)?;
        if paths.is_empty() {
            bail!("paths must be nonempty");
        }
        let repository = self.repository.clone();
        let project = self.project.clone();
        let id_prefix = self.id_prefix.clone();
        let namespace = self.namespace.clone();
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
        let mut task = load(&tx, id)?.context(task_not_found_message(id))?;
        if matches!(task.status.as_str(), "completed" | "cancelled") {
            bail!(
                "TASK_TERMINAL: cannot extend scope of completed or cancelled task; reopen or reset the task first (e.g. `task reset <task-id>`)"
            );
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
                bail!(
                    "TASK_SCOPE_PROTECTED: paths under .forge/acdd/** must be admitted in the initial task contract scope"
                );
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
                        bail!(
                            "TASK_SCOPE_CONFLICT: {id}. Path '{value}' belongs to task '{owner}'. Do: complete the scope owner before extending this task."
                        );
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
                task.spec.scope.push(normalized.clone());
                if !task.spec.extended_scope.contains(&normalized) {
                    task.spec.extended_scope.push(normalized);
                }
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
        if let Some(replacement) = before_commit(&mut task)? {
            tx.stage_manifest_replacement(replacement)?;
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
        let checkpoint = self.checkpoint_context()?;
        let mut tx = super::recovery::transaction(&mut self.connection, checkpoint)?;
        let targets: Vec<String> = tx
            .prepare("SELECT task_id FROM tasks WHERE ((?1 IS NOT NULL AND task_id=?1) OR (?2 IS NOT NULL AND milestone_ref=?2)) AND (substr(task_id,1,length(?3))=?3 OR substr(task_id,1,length(?4))=?4)")?
            .query_map(params![id, milestone, id_prefix, namespace], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|task_id| task_belongs_to(&repository, &project, task_id))
            .collect();
        tx.enroll(&targets, false)?;
        let mut changes = Vec::new();
        for id in &targets {
            if load(&tx, id)?.is_some_and(|task| task.status == "completed") {
                changes.push((id.clone(), true));
            }
        }
        tx.enroll_dependencies(&changes)?;
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

/// Insert a gate transition and return its SQLite row identifier.
pub(super) fn record_transition(
    connection: &Connection,
    transition: &GateTransition,
) -> Result<i64> {
    for (field, value) in [
        ("milestone_id", transition.milestone_id.as_str()),
        ("subject", transition.subject.as_str()),
        ("gate_path", transition.gate_path.as_str()),
        ("action", transition.action.as_str()),
        ("proof_kind", transition.proof_kind.as_str()),
        ("git_tree_sha", transition.git_tree_sha.as_str()),
        ("worker_id", transition.worker_id.as_str()),
    ] {
        if value.trim().is_empty() {
            anyhow::bail!("TASK_TRANSITION_INVALID: {field} must not be empty");
        }
    }
    let proof_payload = serde_json::to_string(&transition.proof_payload)?;
    connection.execute(
        "INSERT INTO gate_transitions (
            milestone_id, subject, gate_path, action, proof_kind, proof_payload,
            git_tree_sha, worker_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            transition.milestone_id,
            transition.subject,
            transition.gate_path,
            transition.action,
            transition.proof_kind,
            proof_payload,
            transition.git_tree_sha,
            transition.worker_id,
        ],
    )?;
    Ok(connection.last_insert_rowid())
}

impl TasksStore {
    /// Persist one task or milestone gate transition and return its SQLite row identifier.
    pub fn record_gate_transition(&self, transition: &GateTransition) -> Result<i64> {
        record_transition(&self.connection, transition)
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
            bail!(
                "TASK_WORKTREE_AFFINITY: {id}. Parked for worker '{}' in worktree '{}'. Do: resume with the preserved worker and worktree.",
                task.parked_worker_id.as_deref().unwrap_or_default(),
                task.parked_worktree.as_deref().unwrap_or_default()
            );
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
            bail!(
                "TASK_NOT_READY: task '{id}'. status is '{}' (active stage '{}'). Do: claim only ready tasks at their active stage.",
                task.status,
                task.stage
            );
        }
        let dependencies =
            unsatisfied_dependencies(tx, id, &self.repository, self.workspace_root.as_deref())?;
        if !dependencies.is_empty() {
            bail!(
                "TASK_NOT_READY: task '{id}' has unsatisfied dependencies: [{}]. Do: complete dependencies before claiming stage '{}'.",
                dependencies.join(", "),
                task.stage
            );
        }
        let task_gate = profile.index_of(&task.stage).with_context(|| {
            format!(
                "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
                task.task_id,
                task.stage,
                profile
                    .gates
                    .iter()
                    .map(|gate| gate.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
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
                    bail!(
                        "TASK_SCOPE_CONFLICT: path '{}' belongs to task '{owner}'; reopen that task instead",
                        my_path.display()
                    );
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

fn execution_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaimBoundExecutionRun> {
    Ok(ClaimBoundExecutionRun {
        run_id: row.get(0)?,
        milestone_id: row.get(1)?,
        subject: row.get(2)?,
        gate_path: row.get(3)?,
        claim_revision: row.get(4)?,
        contract_revision: row.get(5)?,
        worker_id: row.get(6)?,
        worktree: row.get(7)?,
        candidate_fingerprint: row.get(8)?,
        status: row.get(9)?,
    })
}

fn execution_run_row(
    connection: &Connection,
    run_id: i64,
    repository: &str,
    project: &str,
) -> Result<ClaimBoundExecutionRun> {
    let run = connection
        .query_row(
            "SELECT id, milestone_id, subject, gate_path, claim_revision,
                    contract_revision, worker_id, worktree, candidate_fingerprint, status
             FROM gate_execution_runs WHERE id=?1",
            [run_id],
            execution_run_from_row,
        )
        .context("TASK_TRACE_INVALID: execution run does not exist")?;
    if !super::task_belongs_to(repository, project, &run.subject) {
        bail!("TASK_PROJECT_MISMATCH");
    }
    Ok(run)
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
