//! SQLite-owned active task checkpoints. Git records committed state, never a gate decision.

use super::{now, Task, TasksStore};
use crate::core::git::{git_checkpoint, git_command, git_commit_exists};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    ops::Deref,
    path::{Path, PathBuf},
};

const PENDING: &str = "active_checkpoint_pending";
const ROOT: &str = "active_checkpoint_root";

#[derive(Debug)]
/// SQL committed, but publication has not completed. Retrying may publish only this checkpoint.
pub struct CheckpointPending;
impl std::fmt::Display for CheckpointPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TASK_CHECKPOINT_PENDING: a managed task operation or committed checkpoint publication is incomplete. Do: retry checkpoint publication with milestone db-recover after the operation releases its fence; do not reset or roll back committed SQLite state.")
    }
}
impl std::error::Error for CheckpointPending {}

/// Recognizes committed publication failures without conflating ordinary transaction failures.
pub fn is_pending(error: &anyhow::Error) -> bool {
    error.downcast_ref::<CheckpointPending>().is_some()
}

struct LockedFile(File);

impl Drop for LockedFile {
    fn drop(&mut self) {
        // close() alone leaves flock held by descriptors inherited during fork.
        // The operation, rather than an unrelated child's eventual exec, owns it.
        let _ = self.0.unlock();
    }
}

#[derive(Clone)]
/// An operation-owned physical fence. Clones share its final explicit unlock.
pub struct DatabaseFence(std::sync::Arc<LockedFile>);

impl Deref for DatabaseFence {
    type Target = File;

    fn deref(&self) -> &File {
        &self.0 .0
    }
}

/// The existing physical task database fence. Acquire before any SQLite writer transaction.
pub fn lock_database(database: &Path) -> Result<DatabaseFence> {
    if let Some(parent) = database.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(database.with_extension("receipt.lock"))?;
    file.lock()?;
    Ok(DatabaseFence(std::sync::Arc::new(LockedFile(file))))
}

/// Holds a nonblocking shared observation fence across file and SQLite composition.
/// Readers never wait behind a writer or take a SQLite writer lock.
pub fn read_database(database: &Path) -> Result<Option<DatabaseFence>> {
    if !database.parent().is_some_and(Path::exists) {
        return Ok(None);
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(database.with_extension("receipt.lock"))?;
    match file.try_lock_shared() {
        Ok(()) => Ok(Some(DatabaseFence(std::sync::Arc::new(LockedFile(file))))),
        Err(std::fs::TryLockError::WouldBlock) => Err(CheckpointPending.into()),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Detect a crashed operation's precommit intent before composing file-backed
/// authority with SQL. The caller holds the physical shared/exclusive fence.
pub fn ensure_no_intents(root: &Path) -> Result<()> {
    ensure_no_intents_except(root, &BTreeSet::new())
}

pub(crate) fn ensure_no_intents_except(root: &Path, obsolete: &BTreeSet<String>) -> Result<()> {
    let refs = git_command(
        root,
        &["for-each-ref", "--format=%(refname)", "refs/forge/state/"],
    )?;
    if !refs.status.success() {
        return Ok(());
    } // Non-Git workspaces have no keepers.
    for reference in String::from_utf8(refs.stdout)?.lines() {
        if obsolete
            .iter()
            .any(|owner| reference.starts_with(&format!("refs/forge/state/{owner}/")))
        {
            continue;
        }
        // Older candidate-only refs contain no checkpoint or task intent.
        // Recovery still requires a strict payload for every active task.
        if !git_command(
            root,
            &["cat-file", "-e", &format!("{reference}:checkpoint.json")],
        )?
        .status
        .success()
        {
            continue;
        }
        if read_checkpoint(root, reference)?.intent.is_some() {
            return Err(CheckpointPending.into());
        }
    }
    Ok(())
}

/// Pins the metadata check and subsequent row reads to one deferred SQLite snapshot.
pub(crate) struct ReadSnapshot<'a>(Option<Transaction<'a>>);
impl Drop for ReadSnapshot<'_> {
    fn drop(&mut self) {
        self.0.take();
    }
}
pub(crate) fn read_snapshot(connection: &Connection) -> Result<ReadSnapshot<'_>> {
    let snapshot = if connection.is_autocommit() {
        Some(connection.unchecked_transaction()?)
    } else {
        None
    };
    ensure_observable(connection)?;
    Ok(ReadSnapshot(snapshot))
}
pub(crate) fn ensure_observable(connection: &Connection) -> Result<()> {
    if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_store_metadata WHERE key=?1)",
        [PENDING],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(CheckpointPending.into());
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// A real task's active SQL provenance, with revision reservations independent of claim state.
pub struct ActiveCheckpoint {
    version: u32,
    /// Canonical repository owner.
    pub repository: String,
    /// Canonical project owner, with aliases already applied once.
    pub project: String,
    /// Admitted milestone identity.
    pub milestone_id: String,
    /// Genuine task owner, never a synthetic coordinator.
    pub task_id: String,
    /// Worktree containing the checkpoint's actual Git object home.
    pub workspace: PathBuf,
    /// Committed active descriptor, absent for a deleted task's allocation fence.
    pub task: Option<Task>,
    /// Maximum allocated revision, including revoked reservations.
    pub max_revision: u64,
    claims: Vec<(u64, String, String, i64, bool)>,
    gates: Vec<(String, u64, String, Option<String>)>,
    findings: Vec<(u64, String)>,
    scopes: Vec<(String, String, bool)>,
    dependencies: Vec<(String, bool)>,
    submissions: Vec<(u64, String)>,
    prepared_proofs: BTreeSet<String>,
    intent: Option<String>,
    cohort: Vec<(PathBuf, String)>,
    cohort_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    active: Vec<ActiveCheckpoint>,
    retired: Vec<ActiveCheckpoint>,
}

impl Publication {
    fn publish(&self) -> Result<()> {
        publish_cohort(&self.active)?;
        for state in &self.retired {
            if state.task.is_some() || state.intent.is_some() {
                bail!("TASK_CHECKPOINT_RETIREMENT_INVALID");
            }
            let reference = state.reference();
            let exists = git_command(
                &state.workspace,
                &["show-ref", "--verify", "--quiet", &reference],
            )?;
            if !exists.status.success() {
                continue;
            }
            let previous = crate::core::git::git_stdout(
                &state.workspace,
                &["rev-parse", "--verify", &reference],
                "TASK_CHECKPOINT_RETIREMENT_FAILED",
            )?;
            let deleted = git_command(
                &state.workspace,
                &[
                    "-c",
                    "core.fsync=committed",
                    "update-ref",
                    "-d",
                    &reference,
                    &previous,
                ],
            )?;
            if !deleted.status.success() {
                bail!(
                    "TASK_CHECKPOINT_RETIREMENT_FAILED: {}",
                    String::from_utf8_lossy(&deleted.stderr)
                );
            }
        }
        Ok(())
    }
}

impl ActiveCheckpoint {
    fn allocation_fence(mut self) -> Self {
        self.task = None;
        self.claims.clear();
        self.gates.clear();
        self.findings.clear();
        self.scopes.clear();
        self.dependencies.clear();
        self.submissions.clear();
        self.prepared_proofs.clear();
        self.intent = None;
        self.cohort.clear();
        self
    }
    /// The canonical per-task ref; no store-wide owner is created.
    pub fn reference(&self) -> String {
        let slug = self
            .task_id
            .rsplit_once(':')
            .map(|(_, slug)| slug)
            .unwrap_or(&self.task_id);
        format!(
            "refs/forge/state/{}/{}/{}/{slug}",
            self.repository, self.project, self.milestone_id
        )
    }
    fn parents(&self) -> Result<Vec<String>> {
        let mut candidates = self.prepared_proofs.clone();
        for (_, _, _, evidence) in &self.gates {
            if let Some(evidence) = evidence {
                let evidence: crate::core::tasks::gates::Evidence = serde_json::from_str(evidence)
                    .context(
                        "TASK_GATE_EVIDENCE_CORRUPTED: cannot checkpoint malformed gate evidence",
                    )?;
                candidates.extend(
                    evidence
                        .commit
                        .into_iter()
                        .chain(evidence.candidate_baseline_head)
                        .filter(|sha| {
                            matches!(sha.len(), 40 | 64)
                                && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                        }),
                );
            }
        }
        for sha in &candidates {
            if !git_commit_exists(&self.workspace, sha)? {
                bail!("TASK_CHECKPOINT_PROOF_UNAVAILABLE: {sha} has no object home");
            }
        }
        fn objects(value: &serde_json::Value, into: &mut BTreeSet<String>) {
            match value {
                serde_json::Value::String(s)
                    if matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit()) =>
                {
                    into.insert(s.clone());
                }
                serde_json::Value::Object(map) => {
                    for value in map.values() {
                        objects(value, into);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        objects(value, into);
                    }
                }
                _ => (),
            }
        }
        if let Some(task) = &self.task {
            objects(&serde_json::to_value(task)?, &mut candidates);
        }
        for (_, _, _, evidence) in &self.gates {
            if let Some(evidence) = evidence {
                objects(
                    &serde_json::from_str(evidence).context(
                        "TASK_GATE_EVIDENCE_CORRUPTED: cannot checkpoint malformed gate evidence",
                    )?,
                    &mut candidates,
                );
            }
        }
        for (_, submission) in &self.submissions {
            objects(&serde_json::from_str(submission)?, &mut candidates);
        }
        candidates
            .into_iter()
            .filter_map(|sha| match git_commit_exists(&self.workspace, &sha) {
                Ok(true) => Some(Ok(sha)),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }
    fn publish(&self) -> Result<()> {
        let keeper = git_checkpoint(
            &self.workspace,
            &self.reference(),
            &serde_json::to_vec(self)?,
            &self.parents()?,
        )?;
        if self.intent.is_none() {
            crate::core::git::release_provisional(&self.workspace, &self.task_id, &keeper)?;
        }
        Ok(())
    }
    /// Refuse ambiguous checkpoints and incomplete multi-task publication cohorts.
    pub fn validate_cohort(&self) -> Result<()> {
        if self.version != 1 || self.intent.is_some() {
            bail!("TASK_RECOVERY_AMBIGUOUS: unresolved task intent or unsupported checkpoint version; acknowledged state cannot be inferred");
        }
        for (root, reference) in &self.cohort {
            let peer = read_checkpoint(root, reference)?;
            if peer.intent.is_some()
                || peer.cohort != self.cohort
                || peer.cohort_id != self.cohort_id
            {
                bail!("TASK_RECOVERY_AMBIGUOUS: incomplete task publication cohort");
            }
        }
        Ok(())
    }
}

/// Reads a strict checkpoint from a real task ref, leaving all Git objects unchanged.
pub fn read_checkpoint(root: &Path, reference: &str) -> Result<ActiveCheckpoint> {
    read_checkpoint_revision(root, reference, reference)
}

fn read_checkpoint_revision(
    root: &Path,
    reference: &str,
    revision: &str,
) -> Result<ActiveCheckpoint> {
    let output = git_command(root, &["show", &format!("{revision}:checkpoint.json")])?;
    if !output.status.success() {
        bail!("TASK_RECOVERY_CHECKPOINT_INVALID: cannot read {reference}");
    }
    let checkpoint: ActiveCheckpoint = serde_json::from_slice(&output.stdout)?;
    let home = crate::core::git::git_stdout(
        &checkpoint.workspace,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        "TASK_RECOVERY_CHECKPOINT_INVALID",
    )?;
    let discovered = crate::core::git::git_stdout(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        "TASK_RECOVERY_CHECKPOINT_INVALID",
    )?;
    if checkpoint.reference() != reference
        || Path::new(&home).canonicalize()? != Path::new(&discovered).canonicalize()?
    {
        bail!(
            "TASK_RECOVERY_CHECKPOINT_INVALID: checkpoint owner does not match its ref or worktree"
        );
    }
    for part in [
        &checkpoint.repository,
        &checkpoint.project,
        &checkpoint.milestone_id,
    ] {
        crate::core::tasks::valid_identity(part)?;
    }
    Ok(checkpoint)
}

pub(crate) enum StoreHealth {
    Healthy,
    Corrupt,
}

/// Probe only a private byte-for-byte copy, so refused stores retain their WAL/SHM bytes.
pub(crate) fn probe_copy(path: &Path) -> Result<StoreHealth> {
    fn inspect(path: &Path) -> Result<StoreHealth> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let index: u64 = connection.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('nodes','edges','files')", [], |r| r.get(0))?;
        if index >= 2 {
            bail!("TASK_DATABASE_COLLISION: recovery refuses a code map database");
        }
        let metadata: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='task_store_metadata' AND type='table')", [], |r| r.get(0))?;
        if !metadata {
            bail!("TASK_RECOVERY_DATABASE_UNKNOWN: no task store metadata; preserve the original database");
        }
        let version: String = connection.query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        if !matches!(version.as_str(), "5" | "6") {
            bail!("TASK_RECOVERY_SCHEMA_UNSUPPORTED: schema {version}; preserve the original database");
        }
        let integrity: String = connection.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(if integrity == "ok" {
            StoreHealth::Healthy
        } else {
            StoreHealth::Corrupt
        })
    }
    match inspect(path) {
        Err(error)
            if error
                .downcast_ref::<rusqlite::Error>()
                .is_some_and(|error| {
                    matches!(
                        error.sqlite_error_code(),
                        Some(
                            rusqlite::ErrorCode::DatabaseCorrupt
                                | rusqlite::ErrorCode::NotADatabase
                        )
                    )
                }) =>
        {
            Ok(StoreHealth::Corrupt)
        }
        result => result,
    }
}

#[derive(Clone)]
pub(crate) struct CheckpointContext {
    root: PathBuf,
    database: PathBuf,
    _fence: DatabaseFence,
    preparations: Vec<PreparationOwner>,
}

#[derive(Clone)]
pub(crate) struct PreparationOwner {
    pub identity: String,
    pub worktree: PathBuf,
    pub proofs: BTreeSet<String>,
}

impl TasksStore {
    pub(crate) fn checkpoint_homes(&self) -> Result<Vec<PathBuf>> {
        Ok(self.connection.prepare("SELECT DISTINCT value FROM task_store_metadata WHERE key=?1 OR key LIKE 'checkpoint_home:%'")?
            .query_map([ROOT], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(PathBuf::from).collect())
    }

    pub(crate) fn set_obsolete_checkpoint_owners(&mut self, owners: BTreeSet<String>) {
        self.obsolete_checkpoint_owners = owners;
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
        if self.recovery_stage {
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
                let tip = crate::core::git::git_stdout(
                    &home,
                    &["rev-parse", "--verify", reference],
                    "TASK_RECOVERY_CHECKPOINT_INVALID",
                )?;
                let intent = read_checkpoint_revision(&home, reference, &tip)?;
                if intent.intent.is_none() {
                    continue;
                }
                let previous = crate::core::git::git_stdout(
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
            let mut task = super::load(&tx, &intent.task_id)?
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
                super::end_claim(&tx, &task.task_id)?;
                task.claim_revision = super::next_revision(&tx, &task.task_id)?;
                task.worker_id = None;
                task.worktree = None;
                task.status = if task.parked_worker_id.is_some() {
                    "parked"
                } else {
                    "ready"
                }
                .into();
                super::save(&tx, &task)?;
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

fn task_home(connection: &Connection, task: &Task, context: &CheckpointContext) -> Result<PathBuf> {
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

fn validate_owner(
    task: &Task,
    workspace: &Path,
    readmission: bool,
) -> Result<crate::core::tasks::Milestone> {
    let authority = task.authority_snapshot.as_ref().with_context(|| {
        format!(
            "TASK_RECOVERY_LEGACY_UNENROLLED: {} has no admitted task authority",
            task.task_id
        )
    })?;
    let path = crate::core::tasks::confined_path(workspace, &task.milestone_ref)
        .with_context(|| format!("TASK_RECOVERY_AUTHORITY_MISSING: {}", task.task_id))?;
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("TASK_RECOVERY_AUTHORITY_MISSING: {}", task.task_id))?;
    let settings = super::ArchiveSettings::load(workspace)?;
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
            && (crate::core::tasks::AuthoritySnapshot::capture(&milestone, spec) != *authority
                || milestone.digest(spec)? != task.digest))
    {
        bail!("TASK_RECOVERY_AUTHORITY_MISMATCH: {}", task.task_id);
    }
    Ok(milestone)
}

fn states(
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
                let home = crate::core::git::git_stdout(
                    &state.workspace,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    "TASK_CHECKPOINT_OWNER_INVALID",
                )?;
                let owner_home = crate::core::git::git_stdout(
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
pub(crate) struct ManagedTransaction<'a> {
    tx: Option<Transaction<'a>>,
    context: Option<CheckpointContext>,
    before: Vec<ActiveCheckpoint>,
    frozen: Option<Vec<ActiveCheckpoint>>,
    retired: Option<String>,
    new_admissions: BTreeSet<String>,
    covered: BTreeSet<String>,
    original_rows: BTreeMap<String, Vec<String>>,
    intent_token: String,
}
impl<'a> Deref for ManagedTransaction<'a> {
    type Target = Transaction<'a>;
    fn deref(&self) -> &Self::Target {
        self.tx.as_ref().expect("active transaction")
    }
}

pub(crate) fn transaction(
    connection: &mut Connection,
    context: Option<CheckpointContext>,
) -> Result<ManagedTransaction<'_>> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_observable(&tx)?;
    let original_rows = if context.is_some() {
        task_rows(&tx)?
    } else {
        BTreeMap::new()
    };
    let managed = ManagedTransaction {
        tx: Some(tx),
        context,
        before: Vec::new(),
        frozen: None,
        retired: None,
        new_admissions: BTreeSet::new(),
        covered: BTreeSet::new(),
        original_rows,
        intent_token: format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ),
    };
    Ok(managed)
}

// Comparison-only observation: unrelated history is never checkpoint payload or
// enrollment input. Any undeclared task delta aborts before SQLite COMMIT.
fn task_rows(connection: &Connection) -> Result<BTreeMap<String, Vec<String>>> {
    let mut rows = BTreeMap::<String, Vec<String>>::new();
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
        let mut statement = connection.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
        let count = statement.column_count();
        let values = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (0..count)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            ))
        })?;
        for value in values {
            let (id, value) = value?;
            rows.entry(id)
                .or_default()
                .push(format!("{table}:{value:?}"));
        }
    }
    Ok(rows)
}

fn publish_cohort(states: &[ActiveCheckpoint]) -> Result<()> {
    for state in states {
        state.publish()?;
    }
    let mut validated = BTreeSet::new();
    for state in states {
        if state.version != 1 || state.intent.is_some() {
            bail!("TASK_RECOVERY_AMBIGUOUS");
        }
        if validated.insert((&state.cohort_id, &state.cohort)) {
            state.validate_cohort()?;
        }
    }
    Ok(())
}

impl ManagedTransaction<'_> {
    /// Declare potential writers before any owned file or SQL side effect.
    /// Initial enrollment includes genuine peers of the same admitted milestone;
    /// subsequent publication preserves their cohort without allocating peer revisions.
    pub(crate) fn enroll(&mut self, ids: &[String], readmission: bool) -> Result<()> {
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        let requested = ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut covered = requested.clone();
        for id in &requested {
            if let Some(task) = super::load(self, id)? {
                let milestone =
                    validate_owner(&task, &task_home(self, &task, &context)?, readmission)?;
                covered.extend(milestone.tasks.iter().map(|spec| milestone.task_id(spec)));
            }
        }
        // Old cohorts must be replaced together: no peer may retain an ambiguous
        // acknowledgement merely because another peer receives a newer checkpoint.
        let mut visited_cohorts = BTreeSet::new();
        loop {
            let before = states(self, &context, &covered, readmission)?;
            let mut extended = false;
            for state in &before {
                if git_command(
                    &state.workspace,
                    &["show-ref", "--verify", "--quiet", &state.reference()],
                )?
                .status
                .success()
                {
                    let previous = read_checkpoint(&state.workspace, &state.reference())?;
                    if visited_cohorts.insert(previous.cohort.clone()) {
                        for (root, reference) in previous.cohort {
                            let peer = read_checkpoint(&root, &reference)?;
                            extended |= covered.insert(peer.task_id);
                        }
                    }
                }
            }
            if !extended {
                break;
            }
        }
        let additions = covered
            .difference(&self.covered)
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut before = states(self, &context, &additions, readmission)?;
        // Validate the whole requested cohort before publishing even its first intent.
        for state in &mut before {
            if requested.contains(&state.task_id) {
                state.max_revision = state
                    .max_revision
                    .checked_add(1)
                    .context("task revision exhausted")?;
            }
        }
        for state in &before {
            let mut intent = state.clone();
            intent.intent = Some(self.intent_token.clone());
            intent.publish()?;
            self.before.push(state.clone());
            if requested.contains(&state.task_id) {
                self.execute("INSERT INTO task_revisions VALUES(?1,?2) ON CONFLICT(task_id) DO UPDATE SET revision=MAX(revision,excluded.revision)", params![state.task_id, state.max_revision - 1])?;
            }
        }
        self.covered.extend(covered);
        Ok(())
    }

    pub(crate) fn enroll_dependencies(&mut self, changes: &[(String, bool)]) -> Result<()> {
        let mut ids = BTreeSet::new();
        for (dependency, satisfied) in changes {
            ids.extend(self.prepare("SELECT task_id FROM task_dependencies WHERE dependency_id=?1 AND satisfied!=?2")?
                .query_map(params![dependency, satisfied], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        for id in &ids {
            if super::load(self, id)?.is_none() {
                bail!("TASK_RECOVERY_LEGACY_UNENROLLED: {id} has no admitted dependency owner");
            }
        }
        self.enroll(&ids.into_iter().collect::<Vec<_>>(), false)
    }

    fn ensure_coverage(&self) -> Result<()> {
        if self.context.is_none() {
            return Ok(());
        }
        let after = task_rows(self)?;
        for id in self.original_rows.keys().chain(after.keys()) {
            if self.original_rows.get(id) != after.get(id)
                && (!self.covered.contains(id)
                    || !self.before.iter().any(|state| &state.task_id == id))
            {
                bail!("TASK_CHECKPOINT_COHORT_UNDECLARED: {id}; SQL mutation has no validated owner intent");
            }
        }
        Ok(())
    }
    pub(crate) fn is_new_admission(&self, id: &str) -> bool {
        self.new_admissions.contains(id)
    }
    pub(crate) fn reserve_admissions(
        &mut self,
        milestone: &crate::core::tasks::Milestone,
        selected: Option<&str>,
    ) -> Result<()> {
        let Some(context) = &self.context else {
            return Ok(());
        };
        for spec in &milestone.tasks {
            if selected.is_some_and(|selected| selected != spec.task_ref) {
                continue;
            }
            let id = milestone.task_id(spec);
            self.covered.insert(id.clone());
            if self.before.iter().any(|state| state.task_id == id) {
                continue;
            }
            milestone.digest(spec)?;
            let revision = self
                .query_row(
                    "SELECT revision FROM task_revisions WHERE task_id=?1",
                    [&id],
                    |row| row.get::<_, u64>(0),
                )
                .optional()?;
            if revision.is_none() {
                self.new_admissions.insert(id.clone());
            }
            let mut fence = ActiveCheckpoint {
                version: 1,
                repository: milestone.repository.clone(),
                project: milestone.project.clone(),
                milestone_id: milestone.id.clone(),
                task_id: id,
                workspace: context.root.canonicalize()?,
                task: None,
                max_revision: revision.unwrap_or(0),
                claims: Vec::new(),
                gates: Vec::new(),
                findings: Vec::new(),
                scopes: Vec::new(),
                dependencies: Vec::new(),
                submissions: Vec::new(),
                prepared_proofs: BTreeSet::new(),
                intent: None,
                cohort: Vec::new(),
                cohort_id: String::new(),
            };
            if git_command(
                &fence.workspace,
                &["show-ref", "--verify", "--quiet", &fence.reference()],
            )?
            .status
            .success()
            {
                let previous = read_checkpoint(&fence.workspace, &fence.reference())?;
                fence.max_revision = fence.max_revision.max(previous.max_revision);
            }
            fence.max_revision = fence
                .max_revision
                .checked_add(1)
                .context("task revision exhausted")?;
            let mut intent = fence.clone();
            intent.intent = Some(format!("admission-{}-{}", std::process::id(), now()));
            intent.publish()?;
            self.execute("INSERT INTO task_revisions VALUES(?1,?2) ON CONFLICT(task_id) DO UPDATE SET revision=MAX(revision,excluded.revision)",params![fence.task_id,fence.max_revision-1])?;
            self.before.push(fence);
        }
        Ok(())
    }
    /// Freeze a handoff's outgoing and foreign dependency deltas before the pruning decision.
    pub(crate) fn freeze_handoff(
        &mut self,
        outgoing: &str,
        completed: &BTreeSet<String>,
    ) -> Result<()> {
        if let Some(context) = &self.context {
            self.ensure_coverage()?;
            let mut after = states(self, context, &self.covered, true)?;
            for state in &mut after {
                for (dependency, satisfied) in &mut state.dependencies {
                    if completed.contains(dependency) {
                        *satisfied = true;
                    }
                }
            }
            after.retain(|state| !state.task_id.starts_with(outgoing));
            self.retired = Some(outgoing.to_owned());
            self.frozen = Some(after);
        }
        Ok(())
    }
    pub(crate) fn commit(mut self) -> Result<()> {
        let Some(context) = self.context.clone() else {
            self.tx.take().expect("active transaction").commit()?;
            return Ok(());
        };
        let mut after = match self.frozen.take() {
            Some(after) => after,
            None => {
                self.ensure_coverage()?;
                states(&self, &context, &self.covered, true)?
            }
        };
        // Deleted tasks retain their genuine task-owned fence until milestone handoff.
        for prior in &self.before {
            if !after.iter().any(|state| state.task_id == prior.task_id)
                && !self
                    .retired
                    .as_ref()
                    .is_some_and(|prefix| prior.task_id.starts_with(prefix))
            {
                after.push(prior.clone().allocation_fence());
            }
        }
        let cohort = after
            .iter()
            .filter(|state| state.task.is_some())
            .map(|state| (state.workspace.clone(), state.reference()))
            .collect::<Vec<_>>();
        let cohort_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        for state in &mut after {
            state.cohort = if state.task.is_some() {
                cohort.clone()
            } else {
                Vec::new()
            };
            state.cohort_id = cohort_id.clone();
        }
        let retired = self
            .before
            .iter()
            .filter(|state| {
                self.retired
                    .as_ref()
                    .is_some_and(|prefix| state.task_id.starts_with(prefix))
            })
            .cloned()
            .map(ActiveCheckpoint::allocation_fence)
            .collect();
        let publication = Publication {
            active: after,
            retired,
        };
        self.execute("INSERT INTO task_store_metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![PENDING, serde_json::to_string(&publication)?])?;
        self.execute("INSERT INTO task_store_metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![ROOT, context.root.to_string_lossy()])?;
        if let Err(error) = self.tx.take().expect("active transaction").commit() {
            for state in &self.before {
                state.publish()?;
            }
            return Err(error.into());
        }
        // No Drop compensation is legal after SQL COMMIT, even when Git publication fails.
        self.before.clear();
        publication
            .publish()
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        let connection = Connection::open(&context.database)
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        connection
            .execute("DELETE FROM task_store_metadata WHERE key=?1", [PENDING])
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        Ok(())
    }
}
impl Drop for ManagedTransaction<'_> {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            // Rollback only the uncommitted SQL transaction. Fence maxima remain allocated.
            if tx.rollback().is_ok() {
                for state in &self.before {
                    let _ = state.publish();
                }
            }
        }
    }
}

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
        crate::core::tasks::valid_identity(
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
                .map(|path| crate::core::tasks::relative_path(path))
                .collect::<Result<BTreeSet<_>>>()?;
            let stored_scope = state
                .scopes
                .iter()
                .map(|(path, base, _)| {
                    let path = crate::core::tasks::relative_path(path)?;
                    let base = crate::core::tasks::relative_path(base)?;
                    if !path.starts_with(base) {
                        bail!("TASK_RECOVERY_SCOPE_INVALID");
                    }
                    crate::core::tasks::confined_path(&state.workspace, &path.to_string_lossy())?;
                    Ok(path)
                })
                .collect::<Result<BTreeSet<_>>>()?;
            if scope != stored_scope {
                bail!("TASK_RECOVERY_SCOPE_INVALID: scope and persisted boundaries disagree");
            }
            let position = profile
                .index_of(&task.stage)
                .context("TASK_RECOVERY_STAGE_INVALID")?;
            let required = if task.status == "completed" {
                profile.gates.len()
            } else {
                position
            };
            if task.status != "cancelled" {
                for gate in profile.gates.iter().take(required) {
                    if !state.gates.iter().any(|(stage, _, status, evidence)| {
                        stage == &gate.id
                            && status == "passed"
                            && evidence
                                .as_ref()
                                .and_then(|evidence| {
                                    serde_json::from_str::<crate::core::tasks::gates::Evidence>(
                                        evidence,
                                    )
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
                crate::core::tasks::claim_worktree(&active[0].2)?;
            } else if !active.is_empty() {
                bail!("TASK_RECOVERY_CLAIM_INVALID");
            }
            let mut claim_revisions = BTreeSet::new();
            for (revision, _, worktree, _, _) in &state.claims {
                if !claim_revisions.insert(revision) {
                    bail!("TASK_RECOVERY_CLAIM_INVALID: duplicate revision");
                }
                crate::core::tasks::claim_worktree(worktree)?;
            }
            for (stage, revision, status, evidence) in &state.gates {
                if status == "pending" && evidence.is_none() && *revision <= state.max_revision {
                    profile
                        .by_stage(stage)
                        .context("TASK_RECOVERY_STAGE_INVALID")?;
                    continue;
                }
                let evidence: crate::core::tasks::gates::Evidence =
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
                evidence: crate::core::tasks::gates::Evidence,
                action: String,
                findings: Option<serde_json::Value>,
                task: Task,
                #[serde(default)]
                boundary_callers: Option<serde_json::Value>,
            }
            for (revision, result) in &state.submissions {
                let submitted: StoredSubmission = serde_json::from_str(result)?;
                let submitted_profile = self.profile_for_task(&submitted.task)?;
                let position = submitted_profile
                    .index_of(&submitted.evidence.stage)
                    .context("TASK_RECOVERY_STAGE_INVALID")?;
                let gate = &submitted_profile.gates[position];
                let passed = submitted.action == "pass";
                crate::core::tasks::gates::validate_gate_proof(
                    &submitted.evidence.proof,
                    gate,
                    &submitted_profile,
                    passed,
                    &submitted.task.spec.proof_policy,
                )
                .context("TASK_RECOVERY_GATE_INVALID: typed proof")?;
                let expected_stage = if passed {
                    submitted_profile.gate_id((position + 1).min(submitted_profile.gates.len() - 1))
                } else {
                    gate.reject_to.as_deref().unwrap_or(&gate.id)
                };
                let expected_status = if passed && position + 1 == submitted_profile.gates.len() {
                    "completed"
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
                    crate::core::tasks::gates::validate_durable_receipt_with_profile(
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
                                    serde_json::from_str::<crate::core::tasks::gates::Evidence>(
                                        evidence,
                                    )
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
            super::save(&tx, task)?;
            tx.execute(
                "DELETE FROM task_subtasks WHERE task_id=?1",
                [&task.task_id],
            )?;
            for subtask in &task.spec.subtasks {
                let status = crate::core::tasks::SubtaskStatus::parse(&subtask.status)?;
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
                    "INSERT INTO task_claims VALUES(?1,?2,?3,?4,?5,?6)",
                    params![state.task_id, revision, worker, worktree, at, ended],
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
        crate::core::tasks::valid_identity(milestone_id)?;
        let identity = self.milestone_identity(milestone_id);
        self.connection.execute("INSERT INTO milestone_tombstones VALUES(?1,?2,?3,?4) ON CONFLICT(milestone_id) DO UPDATE SET all_tasks_terminal=excluded.all_tasks_terminal,completed_task_ids=excluded.completed_task_ids", params![identity,now(),completed,serde_json::to_string(completed_ids)?])?;
        Ok(())
    }
}
