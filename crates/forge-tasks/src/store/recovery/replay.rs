use super::*;

impl TasksStore {
    pub(crate) fn checkpoint_homes(&self) -> Result<Vec<PathBuf>> {
        Ok(self.connection.prepare("SELECT DISTINCT value FROM task_store_metadata WHERE key=?1 OR key LIKE 'checkpoint_home:%'")?
            .query_map([ROOT], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(PathBuf::from).filter(|p| p.exists()).collect())
    }

    pub(crate) fn set_obsolete_checkpoint_owners(&mut self, owners: BTreeSet<String>) {
        self.obsolete_checkpoint_owners = owners;
    }

    /// Prunes orphaned Git checkpoint refs and stale SQLite metadata pointing to deleted worktrees.
    pub fn cleanup_orphaned_checkpoints(&mut self, root: &Path) -> Result<Vec<String>> {
        let output = git_command(
            root,
            &["for-each-ref", "--format=%(refname)", "refs/forge/state/"],
        )?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        let mut pruned = Vec::new();
        for reference in String::from_utf8(output.stdout)?.lines() {
            if let Ok(show_output) =
                git_command(root, &["show", &format!("{reference}:checkpoint.json")])
            {
                if let Ok(checkpoint) =
                    serde_json::from_slice::<ActiveCheckpoint>(&show_output.stdout)
                {
                    if !checkpoint.workspace.exists() {
                        let _ = git_command(root, &["update-ref", "-d", reference]);
                        pruned.push(reference.to_owned());
                    }
                }
            }
        }
        let homes: Vec<(String, PathBuf)> = self
            .connection
            .prepare(
                "SELECT key, value FROM task_store_metadata WHERE key LIKE 'checkpoint_home:%'",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    PathBuf::from(row.get::<_, String>(1)?),
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (key, path) in homes {
            if !path.exists() {
                let _ = self
                    .connection
                    .execute("DELETE FROM task_store_metadata WHERE key=?1", [&key]);
            }
        }
        Ok(pruned)
    }
    pub(crate) fn checkpoint_enrolled(&self, task: &Task) -> Result<bool> {
        let Some(authority) = &task.authority_snapshot else {
            return Ok(false);
        };
        let root = match &self.workspace_root {
            Some(root) => root.clone(),
            None => match self
                .connection
                .query_row(
                    "SELECT value FROM task_store_metadata WHERE key=?1",
                    [ROOT],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            {
                Some(root) => PathBuf::from(root),
                None => return Ok(false),
            },
        };
        let home: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM task_store_metadata WHERE key=?1",
                [format!("checkpoint_home:{}", task.task_id)],
                |row| row.get(0),
            )
            .optional()?;
        let workspace = home
            .map(PathBuf::from)
            .or_else(|| task.worktree.as_ref().map(PathBuf::from))
            .unwrap_or(root);
        if validate_owner(task, &workspace, false).is_err() {
            return Ok(false);
        }
        let reference = format!(
            "refs/forge/state/{}/{}/{}/{}",
            authority.repository, authority.project, authority.milestone_id, authority.task_ref
        );
        let Ok(state) = read_checkpoint(&workspace, &reference) else {
            return Ok(false);
        };
        Ok(state.intent.is_none()
            && serde_json::to_value(&state.task)? == serde_json::to_value(Some(task))?)
    }

    /// Report incomplete healthy-first coverage without dropping or rewriting foreign rows.
    pub fn unenrolled_checkpoints(&self) -> Result<Vec<String>> {
        let _snapshot = read_snapshot(&self.connection)?;
        let rows = self
            .connection
            .prepare("SELECT task_id,descriptor FROM tasks ORDER BY task_id")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut unenrolled = Vec::new();
        for (id, descriptor) in rows {
            match serde_json::from_str::<Task>(&descriptor) {
                Ok(task) if self.checkpoint_enrolled(&task)? => (),
                _ => unenrolled.push(id),
            }
        }
        Ok(unenrolled)
    }

    pub(crate) fn set_preparation_owners(&mut self, owners: Vec<PreparationOwner>) {
        self.preparation_owners = owners;
    }
    /// Locks the managed operation through its file/SQL/Git effects, using the existing fence.
    pub(crate) fn lock_checkpoint(&mut self) -> Result<()> {
        if self.checkpoint_lock.is_none() {
            self.checkpoint_lock = Some(lock_database(&self.database_path)?);
        }
        Ok(())
    }
    pub(crate) fn checkpoint_context(&mut self) -> Result<Option<CheckpointContext>> {
        if self.recovery_stage || self.database_path == Path::new(":memory:") {
            return Ok(None);
        }
        let root = self
            .workspace_root
            .clone()
            .or_else(|| {
                self.connection
                    .query_row(
                        "SELECT value FROM task_store_metadata WHERE key=?1",
                        [ROOT],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()
                    .ok()
                    .flatten()
                    .map(PathBuf::from)
                    .filter(|p| p.exists())
            })
            .or_else(|| {
                let parent = self.database_path.parent()?;
                let output = git_command(parent, &["rev-parse", "--show-toplevel"]).ok()?;
                output
                    .status
                    .success()
                    .then(|| {
                        String::from_utf8(output.stdout)
                            .ok()
                            .map(|root| PathBuf::from(root.trim()))
                    })
                    .flatten()
                    .filter(|p| p.exists())
            });
        let Some(root) = root else {
            return Ok(None);
        };
        if !git_command(&root, &["rev-parse", "--git-dir"])?
            .status
            .success()
        {
            return Ok(None);
        }
        let fence = match &self.checkpoint_lock {
            Some(fence) => fence.clone(),
            None => lock_database(&self.database_path)?,
        };
        // A pending operation is reconciled explicitly, never replayed as a new SQL mutation.
        ensure_observable(&self.connection)?;
        ensure_no_intents_except(&root, &self.obsolete_checkpoint_owners)?;
        let homes = self.checkpoint_homes()?;
        for home in homes {
            if home.exists() {
                ensure_no_intents_except(&home, &self.obsolete_checkpoint_owners)?;
            }
        }
        Ok(Some(CheckpointContext {
            root,
            database: self.database_path.clone(),
            _fence: fence,
            preparations: self.preparation_owners.clone(),
        }))
    }
    /// Enrolls healthy existing active task state before retiring legacy pins.
    pub fn enroll_checkpoints(&mut self) -> Result<()> {
        let context = self.checkpoint_context()?;
        let mut owners = BTreeMap::<(String, String, String), Vec<String>>::new();
        if let Some(context) = &context {
            let descriptors = self
                .connection
                .prepare("SELECT descriptor FROM tasks ORDER BY task_id")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for descriptor in descriptors {
                let task: Task = serde_json::from_str(&descriptor)?;
                // Enrollment follows current manifest ownership, not physical SQL membership.
                if let Some(authority) = task.authority_snapshot.as_ref().filter(|authority| {
                    authority.repository == self.repository && authority.project == self.project
                }) {
                    let workspace = task_home(&self.connection, &task, context)?;
                    let reference = format!(
                        "refs/forge/state/{}/{}/{}/{}",
                        authority.repository,
                        authority.project,
                        authority.milestone_id,
                        authority.task_ref
                    );
                    let previous = read_checkpoint(&workspace, &reference).ok();
                    let preparation_changed = context.preparations.iter().any(|owner| {
                        owner.identity
                            == format!(
                                "{}/{}/{}",
                                authority.repository, authority.project, authority.milestone_id
                            )
                            && previous
                                .as_ref()
                                .is_none_or(|state| !owner.proofs.is_subset(&state.prepared_proofs))
                    });
                    if validate_owner(&task, &workspace, false).is_ok()
                        && (!self.checkpoint_enrolled(&task)? || preparation_changed)
                    {
                        owners
                            .entry((
                                authority.repository.clone(),
                                authority.project.clone(),
                                authority.milestone_id.clone(),
                            ))
                            .or_default()
                            .push(task.task_id);
                    }
                }
            }
        }
        for ids in owners.into_values() {
            let mut tx = transaction(&mut self.connection, context.clone())?;
            tx.enroll(&ids, false)?;
            tx.commit()?;
        }
        Ok(())
    }
    /// Completes publication of an already committed SQL operation without repeating it.
    pub fn reconcile_checkpoints(&mut self) -> Result<bool> {
        let _fence = match &self.checkpoint_lock {
            Some(fence) => fence.clone(),
            None => lock_database(&self.database_path)?,
        };
        let pending: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM task_store_metadata WHERE key=?1",
                [PENDING],
                |r| r.get(0),
            )
            .optional()?;
        let Some(pending) = pending else {
            return Ok(false);
        };
        let publication: Publication = serde_json::from_str(&pending)?;
        publication
            .publish()
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        self.connection
            .execute("DELETE FROM task_store_metadata WHERE key=?1", [PENDING])
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        Ok(true)
    }

    /// Rewind orphaned intents and release their interrupted claims using healthy SQL.
    pub(crate) fn recover_orphaned_intents(&mut self, roots: &[PathBuf]) -> Result<usize> {
        self.recover_intents(roots, None)
    }

    /// Reconcile only operations anchored by a validated handoff journal's task owners.
    pub(crate) fn recover_handoff_intents(
        &mut self,
        roots: &[PathBuf],
        tasks: &[Task],
    ) -> Result<usize> {
        let owners = tasks
            .iter()
            .map(|task| Ok((task.task_id.clone(), serde_json::to_value(Some(task))?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        self.recover_intents(roots, Some(&owners))
    }

    fn recover_intents(
        &mut self,
        roots: &[PathBuf],
        owners: Option<&BTreeMap<String, serde_json::Value>>,
    ) -> Result<usize> {
        let _fence = match &self.checkpoint_lock {
            Some(fence) => fence.clone(),
            None => lock_database(&self.database_path)?,
        };
        ensure_observable(&self.connection)?;
        let mut homes = roots.to_vec();
        homes.extend(self.checkpoint_homes()?);
        let mut visited = BTreeSet::new();
        let mut orphaned = Vec::new();
        for home in homes.into_iter().filter(|home| home.exists()) {
            let common = git_command(
                &home,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )?;
            if !common.status.success() {
                continue;
            }
            let common = String::from_utf8(common.stdout)?;
            if !visited.insert(Path::new(common.trim()).canonicalize()?) {
                continue;
            }
            let refs = git_command(
                &home,
                &["for-each-ref", "--format=%(refname)", "refs/forge/state/"],
            )?;
            anyhow::ensure!(
                refs.status.success(),
                "TASK_RECOVERY_CHECKPOINT_INVALID: cannot enumerate task refs"
            );
            for reference in String::from_utf8(refs.stdout)?.lines() {
                if self
                    .obsolete_checkpoint_owners
                    .iter()
                    .any(|owner| reference.starts_with(&format!("refs/forge/state/{owner}/")))
                {
                    continue;
                }
                if !git_command(
                    &home,
                    &["cat-file", "-e", &format!("{reference}:checkpoint.json")],
                )?
                .status
                .success()
                {
                    continue;
                }
                let tip = crate::git::git_stdout(
                    &home,
                    &["rev-parse", "--verify", reference],
                    "TASK_RECOVERY_CHECKPOINT_INVALID",
                )?;
                let intent = read_checkpoint_revision(&home, reference, &tip)?;
                if intent.intent.is_none() {
                    continue;
                }
                let previous = crate::git::git_stdout(
                    &home,
                    &["rev-parse", "--verify", &format!("{reference}@{{1}}")],
                    "TASK_RECOVERY_CHECKPOINT_INVALID",
                )?;
                let acknowledged = read_checkpoint_revision(&home, reference, &previous)?;
                anyhow::ensure!(
                    intent.version == 1
                        && acknowledged.version == 1
                        && acknowledged.intent.is_none()
                        && acknowledged.task_id == intent.task_id,
                    "TASK_RECOVERY_AMBIGUOUS: orphaned intent has no prior acknowledged checkpoint"
                );
                let token = intent.intent.as_deref().context("task intent")?;
                let (pid, timestamp) = token
                    .split_once('-')
                    .context("TASK_RECOVERY_AMBIGUOUS: invalid operation token")?;
                anyhow::ensure!(
                    pid.parse::<u32>().is_ok_and(|pid| pid != 0)
                        && timestamp.parse::<u128>().is_ok_and(|time| time != 0),
                    "TASK_RECOVERY_AMBIGUOUS: invalid operation token"
                );
                let mut expected = acknowledged.clone();
                expected.intent = intent.intent.clone();
                expected.max_revision = intent.max_revision;
                expected.cohort.clear();
                expected.cohort_id.clear();
                anyhow::ensure!(
                    serde_json::to_vec(&expected)? == serde_json::to_vec(&intent)?,
                    "TASK_RECOVERY_AMBIGUOUS: intent differs from acknowledged preimage for {}",
                    intent.task_id
                );
                orphaned.push((
                    home.clone(),
                    reference.to_owned(),
                    tip,
                    previous,
                    intent,
                    acknowledged,
                ));
            }
        }
        if let Some(owners) = owners {
            for entry in &orphaned {
                if let Some(expected) = owners.get(&entry.4.task_id) {
                    anyhow::ensure!(serde_json::to_value(&entry.4.task)? == *expected,
                        "TASK_RECOVERY_AMBIGUOUS: handoff intent differs from journal terminal owner");
                }
            }
            let tokens = orphaned
                .iter()
                .filter(|entry| owners.contains_key(&entry.4.task_id))
                .map(|entry| entry.4.intent.clone())
                .collect::<BTreeSet<_>>();
            orphaned.retain(|entry| tokens.contains(&entry.4.intent));
        }
        if orphaned.is_empty() {
            return Ok(0);
        }
        for (_, _, _, _, intent, acknowledged) in &orphaned {
            anyhow::ensure!(
                intent.intent == orphaned[0].4.intent,
                "TASK_RECOVERY_AMBIGUOUS: conflicting operation tokens"
            );
            for (root, reference) in &acknowledged.cohort {
                let peer = orphaned
                    .iter()
                    .find(|entry| entry.5.workspace == *root && entry.1 == *reference)
                    .map(|entry| Ok(entry.5.clone()))
                    .unwrap_or_else(|| read_checkpoint(root, reference))?;
                anyhow::ensure!(
                    peer.intent.is_none()
                        && peer.cohort == acknowledged.cohort
                        && peer.cohort_id == acknowledged.cohort_id,
                    "TASK_RECOVERY_AMBIGUOUS: incomplete acknowledged cohort"
                );
            }
        }
        let context = CheckpointContext {
            root: orphaned[0].0.clone(),
            database: self.database_path.clone(),
            _fence,
            preparations: self.preparation_owners.clone(),
        };
        // Keep the original intents fencing reads until SQL durably releases the claims.
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_observable(&tx)?;
        let ids = orphaned
            .iter()
            .map(|entry| entry.4.task_id.clone())
            .collect();
        let current = states(&tx, &context, &ids, false)?;
        for (_, _, _, _, intent, acknowledged) in &orphaned {
            let mut task = super::super::load(&tx, &intent.task_id)?
                .context("TASK_RECOVERY_AMBIGUOUS: intent has no SQL owner")?;
            anyhow::ensure!(
                task.authority_snapshot
                    .as_ref()
                    .is_some_and(|owner| owner.repository == intent.repository
                        && owner.project == intent.project
                        && owner.milestone_id == intent.milestone_id),
                "TASK_RECOVERY_NAMESPACE_MISMATCH: orphaned intent has a foreign SQL owner"
            );
            let revision: u64 = tx.query_row(
                "SELECT revision FROM task_revisions WHERE task_id=?1",
                [&intent.task_id],
                |row| row.get(0),
            )?;
            let mut expected = acknowledged.clone();
            expected.cohort.clear();
            expected.cohort_id.clear();
            let mut sql = current
                .iter()
                .find(|state| state.task_id == intent.task_id)
                .context("TASK_RECOVERY_AMBIGUOUS: intent has no active SQL preimage")?
                .clone();
            sql.max_revision = acknowledged.max_revision;
            sql.prepared_proofs = acknowledged.prepared_proofs.clone();
            let released = intent.max_revision.checked_add(1) == Some(revision)
                && task.claim_revision == revision
                && task.worker_id.is_none()
                && task.worktree.is_none()
                && task.status
                    == if task.parked_worker_id.is_some() {
                        "parked"
                    } else {
                        "ready"
                    };
            if released {
                let prior = expected
                    .task
                    .as_mut()
                    .context("TASK_RECOVERY_AMBIGUOUS: missing prior claim")?;
                prior.claim_revision = revision;
                prior.worker_id = None;
                prior.worktree = None;
                prior.status = task.status.clone();
                for claim in &mut expected.claims {
                    claim.4 = true;
                }
            } else {
                let floor = revision.max(acknowledged.max_revision);
                anyhow::ensure!(
                    intent.max_revision == floor
                        || floor.checked_add(1) == Some(intent.max_revision),
                    "TASK_RECOVERY_AMBIGUOUS: invalid reservation bound"
                );
            }
            anyhow::ensure!(
                serde_json::to_vec(&expected)? == serde_json::to_vec(&sql)?,
                "TASK_RECOVERY_AMBIGUOUS: acknowledged preimage differs from SQL"
            );
            // A handoff may checkpoint dependent tasks, but does not own their claims.
            let interrupted = owners.is_none()
                && !released
                && task.status == "in_progress"
                && intent.max_revision > revision.max(acknowledged.max_revision);
            if interrupted || released {
                let prior = acknowledged
                    .task
                    .as_ref()
                    .context("TASK_RECOVERY_AMBIGUOUS: missing prior claim")?;
                anyhow::ensure!(
                    prior.status == "in_progress"
                        && acknowledged.claims.iter().filter(|claim| !claim.4).count() == 1
                        && acknowledged.claims.iter().any(|claim| !claim.4
                            && claim.0 == prior.claim_revision
                            && Some(&claim.1) == prior.worker_id.as_ref()
                            && Some(&claim.2) == prior.worktree.as_ref()),
                    "TASK_RECOVERY_AMBIGUOUS: live claim identity mismatch"
                );
            }
            tx.execute("INSERT INTO task_revisions VALUES(?1,?2) ON CONFLICT(task_id) DO UPDATE SET revision=MAX(revision,excluded.revision)", params![intent.task_id, intent.max_revision])?;
            if interrupted {
                super::super::end_claim(&tx, &task.task_id)?;
                task.claim_revision = super::super::next_revision(&tx, &task.task_id)?;
                task.worker_id = None;
                task.worktree = None;
                task.status = if task.parked_worker_id.is_some() {
                    "parked"
                } else {
                    "ready"
                }
                .into();
                super::super::save(&tx, &task)?;
            }
        }
        tx.commit()?;
        for (home, reference, tip, previous, _, _) in &orphaned {
            let output = git_command(home, &["update-ref", reference, previous, tip])?;
            anyhow::ensure!(
                output.status.success(),
                "TASK_RECOVERY_CHECKPOINT_INVALID: cannot rewind {reference}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(orphaned.len())
    }
}

pub(super) fn task_home(
    connection: &Connection,
    task: &Task,
    context: &CheckpointContext,
) -> Result<PathBuf> {
    let home: Option<String> = connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key=?1",
            [format!("checkpoint_home:{}", task.task_id)],
            |row| row.get(0),
        )
        .optional()?;
    Ok(home
        .map(PathBuf::from)
        .or_else(|| task.worktree.as_ref().map(PathBuf::from))
        .unwrap_or_else(|| context.root.clone())
        .canonicalize()?)
}

pub(super) fn validate_owner(
    task: &Task,
    workspace: &Path,
    readmission: bool,
) -> Result<crate::tasks::Milestone> {
    let authority = task.authority_snapshot.as_ref().with_context(|| {
        format!(
            "TASK_RECOVERY_LEGACY_UNENROLLED: {} has no admitted task authority",
            task.task_id
        )
    })?;
    let path = crate::tasks::confined_path(workspace, &task.milestone_ref)
        .with_context(|| format!("TASK_RECOVERY_AUTHORITY_MISSING: {}", task.task_id))?;
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("TASK_RECOVERY_AUTHORITY_MISSING: {}", task.task_id))?;
    let settings = super::super::ArchiveSettings::load(workspace)?;
    let milestone = settings.parse(&text, Path::new(&task.milestone_ref), &authority.repository)?;
    let spec = milestone
        .tasks
        .iter()
        .find(|spec| milestone.task_id(spec) == task.task_id)
        .with_context(|| format!("TASK_RECOVERY_AUTHORITY_MISMATCH: {}", task.task_id))?;
    if milestone.repository != authority.repository
        || milestone.project != authority.project
        || milestone.id != authority.milestone_id
        || (!readmission
            && (crate::tasks::AuthoritySnapshot::capture(&milestone, spec) != *authority
                || milestone.digest(spec)? != task.digest))
    {
        bail!("TASK_RECOVERY_AUTHORITY_MISMATCH: {}", task.task_id);
    }
    Ok(milestone)
}

pub(super) fn states(
    connection: &Connection,
    context: &CheckpointContext,
    ids: &BTreeSet<String>,
    readmission: bool,
) -> Result<Vec<ActiveCheckpoint>> {
    let mut statement = connection.prepare("SELECT tasks.task_id,descriptor,COALESCE(task_revisions.revision,0) FROM tasks LEFT JOIN task_revisions USING(task_id) WHERE tasks.task_id=?1")?;
    let mut tasks = Vec::new();
    for id in ids {
        if let Some(task) = statement
            .query_row([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })
            .optional()?
        {
            tasks.push(task);
        }
    }
    let mut result = Vec::new();
    for (id, descriptor, max_revision) in tasks {
        let task: Task = serde_json::from_str(&descriptor)?;
        if Path::new(&task.milestone_ref)
            .components()
            .any(|part| part.as_os_str() == "archive")
        {
            continue;
        }
        let Some(authority) = &task.authority_snapshot else {
            bail!("TASK_RECOVERY_LEGACY_UNENROLLED: {id} has no admitted task authority");
        };
        let workspace = task_home(connection, &task, context)?;
        validate_owner(&task, &workspace, readmission)?;
        let scopes = connection
            .prepare(
                "SELECT path,base_root,frozen FROM task_scope_paths WHERE task_id=?1 ORDER BY path",
            )?
            .query_map([&id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut state = ActiveCheckpoint {
            version: 1, repository: authority.repository.clone(), project: authority.project.clone(), milestone_id: authority.milestone_id.clone(),
            task_id: id.clone(), workspace, task: Some(task), max_revision,
            claims: connection.prepare("SELECT revision,worker_id,worktree,claimed_at,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")?.query_map([&id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?.collect::<rusqlite::Result<_>>()?,
            claim_session_fingerprints: connection.prepare("SELECT revision,session_fingerprint FROM task_claims WHERE task_id=?1 AND session_fingerprint IS NOT NULL ORDER BY revision")?.query_map([&id], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?,
            gates: connection.prepare("SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY revision,gate")?.query_map([&id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?.collect::<rusqlite::Result<_>>()?,
            findings: connection.prepare("SELECT revision,findings FROM task_findings WHERE task_id=?1 ORDER BY revision")?.query_map([&id], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?,
            dependencies: connection.prepare("SELECT dependency_id,satisfied FROM task_dependencies WHERE task_id=?1 ORDER BY dependency_id")?.query_map([&id], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?,
            submissions: connection.prepare("SELECT revision,result FROM task_submissions WHERE task_id=?1 ORDER BY revision")?.query_map([&id], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?,
            scopes, prepared_proofs: BTreeSet::new(), intent: None, cohort: Vec::new(), cohort_id: String::new(),
        };
        if git_command(
            &state.workspace,
            &["show-ref", "--verify", "--quiet", &state.reference()],
        )?
        .status
        .success()
        {
            let previous = read_checkpoint(&state.workspace, &state.reference())?;
            state.max_revision = state.max_revision.max(previous.max_revision);
            state.prepared_proofs = previous.prepared_proofs;
        }
        for owner in &context.preparations {
            if owner.identity
                == format!(
                    "{}/{}/{}",
                    state.repository, state.project, state.milestone_id
                )
            {
                let home = crate::git::git_stdout(
                    &state.workspace,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    "TASK_CHECKPOINT_OWNER_INVALID",
                )?;
                let owner_home = crate::git::git_stdout(
                    &owner.worktree,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    "TASK_CHECKPOINT_OWNER_INVALID",
                )?;
                if home != owner_home {
                    bail!("TASK_CHECKPOINT_OWNER_INVALID: preparation has a foreign object home");
                }
                state.prepared_proofs.extend(owner.proofs.iter().cloned());
            }
        }
        result.push(state);
    }
    Ok(result)
}

/// One physical-fenced writer operation; SQL COMMIT precedes publication and public success.
impl TasksStore {
    pub(crate) fn import_task_fence(&mut self, state: &ActiveCheckpoint) -> Result<()> {
        if !self.recovery_stage {
            bail!("TASK_RECOVERY_STAGE_INVALID");
        }
        state.validate_cohort()?;
        if state.task.is_some()
            || state.repository != self.repository
            || state.project != self.project
            || !state
                .task_id
                .starts_with(&format!("{}/{}:", self.project, state.milestone_id))
        {
            bail!("TASK_RECOVERY_FENCE_INVALID");
        }
        crate::tasks::valid_identity(
            state
                .task_id
                .rsplit_once(':')
                .context("TASK_RECOVERY_FENCE_INVALID")?
                .1,
        )?;
        self.connection.execute("INSERT INTO task_revisions VALUES(?1,?2) ON CONFLICT(task_id) DO UPDATE SET revision=MAX(revision,excluded.revision)",params![state.task_id,state.max_revision])?;
        Ok(())
    }
    pub(crate) fn adopt_checkpoint_lock(&mut self, fence: DatabaseFence) {
        self.checkpoint_lock = Some(fence);
    }

    /// Restricts a fresh staging store to typed recovery admission without publishing refs.
    pub(crate) fn begin_recovery_stage(&mut self) -> Result<()> {
        let staging: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_store_metadata WHERE key='recovery_staging')",
            [],
            |row| row.get(0),
        )?;
        let populated: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks) OR EXISTS(SELECT 1 FROM task_revisions)",
            [],
            |row| row.get(0),
        )?;
        if populated && !staging {
            bail!("TASK_RECOVERY_STAGE_INVALID: staging store must be empty");
        }
        self.connection.execute(
            "INSERT OR IGNORE INTO task_store_metadata VALUES('recovery_staging','1')",
            [],
        )?;
        self.recovery_stage = true;
        Ok(())
    }

    /// Verifies and closes staging WAL state before filesystem promotion.
    pub(crate) fn verify_recovery_stage(&self) -> Result<()> {
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let invalid: bool = self
            .connection
            .prepare("PRAGMA foreign_key_check")?
            .exists([])?;
        if integrity != "ok" || invalid {
            bail!("TASK_RECOVERY_STAGE_INVALID: integrity or foreign key check failed");
        }
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    pub(crate) fn seal_recovery_stage(&self) -> Result<()> {
        if let Some(root) = &self.workspace_root {
            self.connection.execute("INSERT INTO task_store_metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![ROOT,root.to_string_lossy()])?;
        }
        self.connection.execute(
            "DELETE FROM task_store_metadata WHERE key='recovery_staging'",
            [],
        )?;
        self.verify_recovery_stage()
    }

    /// Imports one validated active checkpoint into a fresh schema6 store.
    /// Callers supply an independently parsed, current manifest task authority.
    pub fn import_checkpoint(&mut self, state: &ActiveCheckpoint, admitted: &Task) -> Result<()> {
        if !self.recovery_stage {
            bail!("TASK_RECOVERY_STAGE_INVALID: import requires a private recovery stage");
        }
        state.validate_cohort()?;
        self.assert_id(&state.task_id)?;
        if state.repository != self.repository
            || state.project != self.project
            || state.task_id != admitted.task_id
        {
            bail!("TASK_RECOVERY_NAMESPACE_MISMATCH");
        }
        if let Some(task) = &state.task {
            if task.contract_revision != admitted.contract_revision
                || task.digest != admitted.digest
                || task.authority_snapshot != admitted.authority_snapshot
                || task.profile_pin != admitted.profile_pin
                || task.initial_scope != admitted.initial_scope
            {
                bail!("TASK_RECOVERY_AUTHORITY_MISMATCH: {}", state.task_id);
            }
            let profile = self.profile_for_task(task)?;
            let scope = task
                .spec
                .scope
                .iter()
                .map(|path| crate::tasks::relative_path(path))
                .collect::<Result<BTreeSet<_>>>()?;
            let stored_scope = state
                .scopes
                .iter()
                .map(|(path, base, _)| {
                    let path = crate::tasks::relative_path(path)?;
                    let base = crate::tasks::relative_path(base)?;
                    if !path.starts_with(base) {
                        bail!("TASK_RECOVERY_SCOPE_INVALID");
                    }
                    crate::tasks::confined_path(&state.workspace, &path.to_string_lossy())?;
                    Ok(path)
                })
                .collect::<Result<BTreeSet<_>>>()?;
            if scope != stored_scope {
                bail!("TASK_RECOVERY_SCOPE_INVALID: scope and persisted boundaries disagree");
            }
            let task_profile =
                crate::engine::tasks::claim::flattened_profile(&profile, Some(task))?;
            let executable = task_profile.executable_gates();
            let position = task_profile
                .index_of(&task.stage)
                .context("TASK_RECOVERY_STAGE_INVALID")?;
            let required = if task.status == "completed" {
                executable.len()
            } else {
                position
            };
            if task.status != "cancelled" {
                for gate in executable.iter().take(required) {
                    if !crate::engine::claim::gate_applicable(task, gate) {
                        continue;
                    }
                    let gate_pos = task_profile.index_of(&gate.id).unwrap_or(0);
                    let bypassed_by_jump = executable[..gate_pos].iter().any(|prior| {
                        if let Some(target) = prior.on_success.as_deref() {
                            if let Some(target_pos) = task_profile.index_of(target) {
                                if target_pos > gate_pos {
                                    return state.gates.iter().any(|(stage, _, status, _)| {
                                        (stage == &prior.id
                                            || stage.rsplit('.').next()
                                                == Some(prior.id.rsplit('.').next().unwrap()))
                                            && status == "passed"
                                    });
                                }
                            }
                        }
                        false
                    });
                    if bypassed_by_jump {
                        continue;
                    }
                    let leaf = gate.id.rsplit('.').next().unwrap_or(&gate.id);
                    if !state.gates.iter().any(|(stage, _, status, evidence)| {
                        (stage == &gate.id || stage.rsplit('.').next() == Some(leaf))
                            && status == "passed"
                            && evidence
                                .as_ref()
                                .and_then(|evidence| {
                                    serde_json::from_str::<crate::tasks::gates::Evidence>(evidence)
                                        .ok()
                                })
                                .is_some_and(|evidence| {
                                    evidence.contract_revision == task.contract_revision
                                })
                    }) {
                        bail!("TASK_RECOVERY_GATE_INVALID: current stage lacks accepted authority");
                    }
                }
            }
            if task.claim_revision > state.max_revision
                || state
                    .claims
                    .iter()
                    .any(|(revision, _, _, _, _)| *revision > state.max_revision)
            {
                bail!("TASK_RECOVERY_REVISION_INVALID");
            }
            let active = state
                .claims
                .iter()
                .filter(|(_, _, _, _, ended)| !ended)
                .collect::<Vec<_>>();
            if task.status == "in_progress" {
                if active.len() != 1
                    || active[0].0 != task.claim_revision
                    || Some(&active[0].1) != task.worker_id.as_ref()
                    || Some(&active[0].2) != task.worktree.as_ref()
                {
                    bail!("TASK_RECOVERY_CLAIM_INVALID");
                }
                crate::tasks::claim_worktree(&active[0].2)?;
            } else if !active.is_empty() {
                bail!("TASK_RECOVERY_CLAIM_INVALID");
            }
            let mut claim_revisions = BTreeSet::new();
            for (revision, _, worktree, _, _) in &state.claims {
                if !claim_revisions.insert(revision) {
                    bail!("TASK_RECOVERY_CLAIM_INVALID: duplicate revision");
                }
                crate::tasks::claim_worktree(worktree)?;
            }
            for (stage, revision, status, evidence) in &state.gates {
                if status == "pending" && evidence.is_none() && *revision <= state.max_revision {
                    profile
                        .by_stage(stage)
                        .context("TASK_RECOVERY_STAGE_INVALID")?;
                    continue;
                }
                let evidence: crate::tasks::gates::Evidence =
                    serde_json::from_str(evidence.as_deref().context("missing gate evidence")?)?;
                if evidence.commit.as_ref().is_some_and(|sha| {
                    !matches!(sha.len(), 40 | 64)
                        || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                }) {
                    bail!("TASK_RECOVERY_GATE_INVALID: unresolved proof object");
                }
                if !matches!(status.as_str(), "passed" | "rejected")
                    || evidence.stage != *stage
                    || evidence.task_id != task.task_id
                    || evidence.claim_revision != *revision
                    || !state
                        .claims
                        .iter()
                        .any(|(rev, worker, worktree, _, ended)| {
                            *rev == *revision
                                && *ended
                                && *worker == evidence.worker_id
                                && *worktree == evidence.worktree
                        })
                {
                    bail!("TASK_RECOVERY_GATE_INVALID");
                }
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct StoredSubmission {
                evidence: crate::tasks::gates::Evidence,
                action: String,
                findings: Option<serde_json::Value>,
                task: Task,
                #[serde(default)]
                boundary_callers: Option<serde_json::Value>,
            }
            for (revision, result) in &state.submissions {
                let submitted: StoredSubmission = serde_json::from_str(result)?;
                let profile = self.profile_for_task(&submitted.task)?;
                let submitted_profile = crate::engine::tasks::claim::flattened_profile(
                    &profile,
                    Some(&submitted.task),
                )?;
                let executable = submitted_profile.executable_gates();
                let position = submitted_profile
                    .index_of(&submitted.evidence.stage)
                    .context("TASK_RECOVERY_STAGE_INVALID")?;
                let gate = executable[position];
                let passed = submitted.action == "pass";
                crate::tasks::gates::validate_gate_proof(
                    &submitted.evidence.proof,
                    gate,
                    &submitted_profile,
                    passed,
                    &submitted.task.spec.proof_policy,
                )
                .context("TASK_RECOVERY_GATE_INVALID: typed proof")?;
                let exhausted = !passed
                    && gate.max_attempts.is_some_and(|limit| {
                        state
                            .gates
                            .iter()
                            .filter(|(stage, rev, status, _)| {
                                stage == &submitted.evidence.stage
                                    && rev <= revision
                                    && status == "rejected"
                            })
                            .count()
                            >= limit as usize
                    });
                let expected_stage =
                    if passed && crate::tasks::profile::gate_is_delivery(gate) || exhausted {
                        submitted.evidence.stage.clone()
                    } else if passed {
                        let mut condition_task = submitted.task.clone();
                        condition_task.worktree = Some(submitted.evidence.worktree.clone());
                        crate::engine::tasks::claim::advance_gate_cursor_for_task(
                            &profile,
                            Some(&submitted.evidence.stage),
                            &condition_task,
                        )
                        .context("TASK_RECOVERY_SUBMISSION_INVALID: successor gate")?
                        .canonical_stage_id
                    } else {
                        let mut condition_task = submitted.task.clone();
                        condition_task.worktree = Some(submitted.evidence.worktree.clone());
                        let cursors = crate::engine::tasks::claim::collect_cursors_for_task(
                            &profile,
                            Some(&condition_task),
                        )?;
                        let attempts = state
                            .gates
                            .iter()
                            .filter(|(stage, rev, status, _)| {
                                stage == &submitted.evidence.stage
                                    && rev <= revision
                                    && status == "rejected"
                            })
                            .count();
                        crate::engine::tasks::claim::resolve_fail_target(
                            &cursors,
                            position,
                            submitted.findings.as_ref(),
                            u32::try_from(attempts).unwrap_or(u32::MAX),
                            |candidate| {
                                crate::engine::tasks::claim::task_matches_gate_condition(
                                    candidate.when.as_deref(),
                                    &condition_task,
                                )
                            },
                        )?
                        .target
                        .context("TASK_RECOVERY_SUBMISSION_INVALID: rejection target gate")?
                        .canonical_stage_id
                    };
                let expected_status = if passed && crate::tasks::profile::gate_is_delivery(gate) {
                    "completed"
                } else if exhausted {
                    "exhausted"
                } else {
                    "ready"
                };
                let authority = submitted
                    .task
                    .authority_snapshot
                    .as_ref()
                    .context("TASK_RECOVERY_SUBMISSION_INVALID: missing authority")?;
                if authority.repository != state.repository
                    || authority.project != state.project
                    || authority.milestone_id != state.milestone_id
                    || format!(
                        "{}/{}:{}",
                        authority.project, authority.milestone_id, authority.task_ref
                    ) != state.task_id
                    || submitted.task.spec.task_ref != authority.task_ref
                    || submitted.task.spec.contract_revision != submitted.evidence.contract_revision
                {
                    bail!("TASK_RECOVERY_SUBMISSION_INVALID: historical identity");
                }
                if expected_status == "completed" {
                    crate::tasks::gates::validate_durable_receipt_with_profile(
                        submitted
                            .task
                            .receipt
                            .as_ref()
                            .context("TASK_RECOVERY_SUBMISSION_INVALID: missing receipt")?,
                        submitted.task.contract_revision,
                        &submitted_profile,
                    )?;
                }
                if submitted.evidence.task_id != task.task_id
                    || submitted.task.task_id != task.task_id
                    || submitted.task.contract_revision != submitted.evidence.contract_revision
                    || submitted.evidence.contract_revision == 0
                    || submitted.evidence.contract_revision > task.contract_revision
                    || (submitted.evidence.contract_revision == task.contract_revision
                        && (submitted.task.digest != task.digest
                            || submitted.task.profile_pin != task.profile_pin
                            || submitted.task.authority_snapshot != task.authority_snapshot))
                    || submitted.task.claim_revision != *revision
                    || submitted.task.stage != expected_stage
                    || submitted.task.status != expected_status
                    || submitted.task.worker_id.is_some()
                    || submitted.task.worktree.is_some()
                    || submitted.evidence.claim_revision != *revision
                    || !matches!(submitted.action.as_str(), "pass" | "reject")
                    || state
                        .gates
                        .iter()
                        .filter(|(_, rev, _, _)| rev == revision)
                        .count()
                        != 1
                    || !state.gates.iter().any(|(stage, rev, status, evidence)| {
                        rev == revision
                            && stage == &submitted.evidence.stage
                            && status == if passed { "passed" } else { "rejected" }
                            && evidence
                                .as_ref()
                                .and_then(|evidence| {
                                    serde_json::from_str::<crate::tasks::gates::Evidence>(evidence)
                                        .ok()
                                })
                                .as_ref()
                                == Some(&submitted.evidence)
                    })
                {
                    bail!("TASK_RECOVERY_SUBMISSION_INVALID");
                }
                if !passed
                    && !state.findings.iter().any(|(rev, findings)| {
                        rev == revision
                            && serde_json::from_str::<serde_json::Value>(findings)
                                .ok()
                                .as_ref()
                                == submitted.findings.as_ref()
                    })
                {
                    bail!("TASK_RECOVERY_SUBMISSION_INVALID: rejected findings");
                }
                let _provenance = (submitted.findings, submitted.boundary_callers);
            }
            for (_, revision, status, _) in &state.gates {
                if matches!(status.as_str(), "passed" | "rejected")
                    && state
                        .submissions
                        .iter()
                        .filter(|(rev, _)| rev == revision)
                        .count()
                        != 1
                {
                    bail!("TASK_RECOVERY_SUBMISSION_INVALID: missing or duplicate submission");
                }
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(task) = &state.task {
            super::super::save(&tx, task)?;
            tx.execute(
                "DELETE FROM task_subtasks WHERE task_id=?1",
                [&task.task_id],
            )?;
            for subtask in &task.spec.subtasks {
                let status = crate::tasks::SubtaskStatus::parse(&subtask.status)?;
                tx.execute(
                    "INSERT INTO task_subtasks VALUES(?1,?2,?3,?4,?5,?6)",
                    params![
                        task.task_id,
                        subtask.subtask_ref,
                        subtask.title,
                        status.as_str(),
                        subtask.evidence,
                        now()
                    ],
                )?;
            }
            for (revision, worker, worktree, at, ended) in &state.claims {
                tx.execute(
                    "INSERT INTO task_claims(task_id,revision,worker_id,worktree,claimed_at,ended,session_fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![state.task_id, revision, worker, worktree, at, ended, state.claim_session_fingerprints.get(revision)],
                )?;
            }
            for (gate, revision, status, evidence) in &state.gates {
                tx.execute(
                    "INSERT INTO task_gates VALUES(?1,?2,?3,?4,?5)",
                    params![state.task_id, gate, revision, status, evidence],
                )?;
            }
            for (revision, findings) in &state.findings {
                tx.execute(
                    "INSERT INTO task_findings VALUES(?1,?2,?3)",
                    params![state.task_id, revision, findings],
                )?;
            }
            tx.execute(
                "DELETE FROM task_scope_paths WHERE task_id=?1",
                [&state.task_id],
            )?;
            for (path, root, frozen) in &state.scopes {
                tx.execute(
                    "INSERT INTO task_scope_paths VALUES(?1,?2,?3,?4)",
                    params![state.task_id, path, root, frozen],
                )?;
            }
            tx.execute(
                "DELETE FROM task_dependencies WHERE task_id=?1",
                [&state.task_id],
            )?;
            for (dependency, satisfied) in &state.dependencies {
                tx.execute(
                    "INSERT INTO task_dependencies VALUES(?1,?2,?3)",
                    params![state.task_id, dependency, satisfied],
                )?;
            }
            for (revision, result) in &state.submissions {
                tx.execute(
                    "INSERT INTO task_submissions VALUES(?1,?2,?3)",
                    params![state.task_id, revision, result],
                )?;
            }
        } else {
            tx.execute("DELETE FROM tasks WHERE task_id=?1", [&state.task_id])?;
        }
        tx.execute("INSERT INTO task_revisions VALUES(?1,?2) ON CONFLICT(task_id) DO UPDATE SET revision=MAX(revision,excluded.revision)", params![state.task_id,state.max_revision])?;
        tx.commit()?;
        Ok(())
    }
    /// Preseeds a canonical archived milestone before any active task admission.
    pub fn seed_archive(
        &mut self,
        milestone_id: &str,
        completed: bool,
        completed_ids: &[String],
    ) -> Result<()> {
        if !self.recovery_stage {
            bail!(
                "TASK_RECOVERY_STAGE_INVALID: archive admission requires a private recovery stage"
            );
        }
        crate::tasks::valid_identity(milestone_id)?;
        let identity = self.milestone_identity(milestone_id);
        self.connection.execute("INSERT INTO milestone_tombstones VALUES(?1,?2,?3,?4) ON CONFLICT(milestone_id) DO UPDATE SET all_tasks_terminal=excluded.all_tasks_terminal,completed_task_ids=excluded.completed_task_ids", params![identity,now(),completed,serde_json::to_string(completed_ids)?])?;
        Ok(())
    }
}
