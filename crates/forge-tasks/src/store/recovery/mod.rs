//! SQLite-owned active task checkpoints. Git records committed state, never a gate decision.

use super::{now, Task, TasksStore};
use crate::git::{git_checkpoint, git_command, git_commit_exists};
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
    if database == Path::new(":memory:") {
        bail!("checkpoint recovery requires a file-backed task store");
    }
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
    if database == Path::new(":memory:") {
        bail!("checkpoint recovery requires a file-backed task store");
    }
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
        match read_checkpoint(root, reference) {
            Ok(cp) => {
                if cp.intent.is_some() {
                    return Err(CheckpointPending.into());
                }
            }
            Err(error) => {
                if let Ok(output) =
                    git_command(root, &["show", &format!("{reference}:checkpoint.json")])
                {
                    if let Ok(cp) = serde_json::from_slice::<ActiveCheckpoint>(&output.stdout) {
                        if !cp.workspace.exists() {
                            let _ = git_command(root, &["update-ref", "-d", reference]);
                            continue;
                        }
                    }
                }
                return Err(error);
            }
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

/// Stored gate tuple (gate, revision, state, evidence).
pub type StoredGateRecord = (String, u64, String, Option<String>);

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
    pub(crate) claims: Vec<(u64, String, String, i64, bool)>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    claim_session_fingerprints: BTreeMap<u64, String>,
    pub(crate) gates: Vec<StoredGateRecord>,
    findings: Vec<(u64, String)>,
    scopes: Vec<(String, String, bool)>,
    dependencies: Vec<(String, bool)>,
    pub(crate) submissions: Vec<(u64, String)>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest_replacement: Option<ManifestReplacement>,
}

/// A manifest replacement committed as part of the SQLite publication intent.
///
/// The payload stores the exact relative path, the expected current-content hash,
/// and the target bytes/hash so recovery can finish the write without re-running
/// the task mutation.

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestReplacement {
    root: PathBuf,
    relative_path: String,
    before_hash: String,
    target_hash: String,
    target: Vec<u8>,
}

impl ManifestReplacement {
    pub(crate) fn new(
        root: &Path,
        relative_path: &str,
        before: &[u8],
        after: &[u8],
    ) -> Result<Self> {
        let root = crate::core::fs::canonical_root(root)?;
        let relative = crate::tasks::relative_path(relative_path)?;
        if relative.to_str() != Some(relative_path)
            || relative
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("md")
        {
            bail!("TASK_MANIFEST_PUBLICATION_INVALID: expected an exact relative Markdown path");
        }
        let path = crate::tasks::confined_path(&root, relative_path)?;
        if path != root.join(&relative) || !path.is_file() {
            bail!("TASK_MANIFEST_PUBLICATION_INVALID: manifest path is not a file");
        }
        let current = std::fs::read(&path)?;
        let before_hash = crate::core::commitments::hash(before);
        if current.as_slice() != before || crate::core::commitments::hash(&current) != before_hash {
            bail!("TASK_MANIFEST_PUBLICATION_CONFLICT: manifest changed before staging");
        }
        let target_hash = crate::core::commitments::hash(after);
        Ok(Self {
            root,
            relative_path: relative_path.to_owned(),
            before_hash,
            target_hash,
            target: after.to_vec(),
        })
    }

    fn validate(&self) -> Result<PathBuf> {
        let root = crate::core::fs::canonical_root(&self.root)?;
        if root != self.root
            || crate::tasks::relative_path(&self.relative_path)?.to_str()
                != Some(self.relative_path.as_str())
            || Path::new(&self.relative_path)
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("md")
            || crate::core::commitments::hash(&self.target) != self.target_hash
        {
            bail!("TASK_MANIFEST_PUBLICATION_INVALID");
        }
        let path = crate::tasks::confined_path(&root, &self.relative_path)?;
        if path != root.join(&self.relative_path) || !path.is_file() {
            bail!("TASK_MANIFEST_PUBLICATION_INVALID: manifest path is not a file");
        }
        Ok(path)
    }

    fn validate_preimage(&self) -> Result<()> {
        let path = self.validate()?;
        let current = std::fs::read(path)?;
        let current_hash = crate::core::commitments::hash(&current);
        if current_hash != self.before_hash && current_hash != self.target_hash {
            bail!("TASK_MANIFEST_PUBLICATION_CONFLICT: manifest no longer matches its preimage or target");
        }
        Ok(())
    }

    fn publish(&self) -> Result<()> {
        let path = self.validate()?;
        let current = std::fs::read(&path)?;
        let current_hash = crate::core::commitments::hash(&current);
        if current_hash == self.target_hash {
            return Ok(());
        }
        if current_hash != self.before_hash {
            bail!("TASK_MANIFEST_PUBLICATION_CONFLICT: manifest no longer matches its preimage or target");
        }
        crate::core::fs::atomic_replace(&path, &self.target)?;
        let published = std::fs::read(&path)?;
        if crate::core::commitments::hash(&published) != self.target_hash {
            bail!("TASK_MANIFEST_PUBLICATION_CONFLICT: published manifest does not match target");
        }
        Ok(())
    }
}

impl Publication {
    fn publish(&self) -> Result<()> {
        self.publish_with(|replacement| replacement.publish())
    }

    fn publish_with(
        &self,
        mut publish_manifest: impl FnMut(&ManifestReplacement) -> Result<()>,
    ) -> Result<()> {
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
            let previous = crate::git::git_stdout(
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
        if let Some(replacement) = &self.manifest_replacement {
            publish_manifest(replacement)?;
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
        let mut proof_candidates: Vec<(Option<PathBuf>, String)> = self
            .prepared_proofs
            .iter()
            .map(|s| (None, s.clone()))
            .collect();
        for (_, _, _, evidence) in &self.gates {
            if let Some(evidence) = evidence {
                let evidence: crate::tasks::gates::Evidence = serde_json::from_str(evidence)
                    .context(
                        "TASK_GATE_EVIDENCE_CORRUPTED: cannot checkpoint malformed gate evidence",
                    )?;
                let wt = PathBuf::from(&evidence.worktree);
                for sha in evidence
                    .commit
                    .into_iter()
                    .chain(evidence.candidate_baseline_head)
                {
                    if matches!(sha.len(), 40 | 64)
                        && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                    {
                        candidates.insert(sha.clone());
                        proof_candidates.push((Some(wt.clone()), sha));
                    }
                }
            }
        }
        for (worktree, sha) in &proof_candidates {
            let in_workspace = git_commit_exists(&self.workspace, sha)?;
            let in_worktree = worktree
                .as_ref()
                .is_some_and(|w| git_commit_exists(w, sha).unwrap_or(false));
            if !in_workspace && !in_worktree {
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
            crate::git::release_provisional(&self.workspace, &self.task_id, &keeper)?;
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
    if !checkpoint.workspace.exists() {
        bail!(
            "TASK_RECOVERY_CHECKPOINT_INVALID: checkpoint '{reference}' references missing worktree '{}'. The worktree was removed from disk.",
            checkpoint.workspace.display()
        );
    }
    let home = crate::git::git_stdout(
        &checkpoint.workspace,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        "TASK_RECOVERY_CHECKPOINT_INVALID",
    )?;
    let discovered = crate::git::git_stdout(
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
        crate::tasks::valid_identity(part)?;
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
        let index: u64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('nodes','edges','files')",
            [],
            |r| r.get(0),
        )?;
        if index >= 2 {
            bail!("TASK_DATABASE_COLLISION: recovery refuses a code map database");
        }
        let metadata: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='task_store_metadata' AND type='table')",
            [],
            |r| r.get(0),
        )?;
        if !metadata {
            bail!("TASK_RECOVERY_DATABASE_UNKNOWN: no task store metadata; preserve the original database");
        }
        let version: String = connection.query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        if !matches!(version.as_str(), "5" | "6" | "7") {
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

pub(crate) struct ManagedTransaction<'a> {
    tx: Option<Transaction<'a>>,
    context: Option<CheckpointContext>,
    database_path: Option<PathBuf>,
    publication_fence: Option<DatabaseFence>,
    manifest_replacement: Option<ManifestReplacement>,
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
    let database_path = context
        .as_ref()
        .map(|context| context.database.clone())
        .or_else(|| connection.path().map(PathBuf::from));
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
        database_path,
        publication_fence: None,
        manifest_replacement: None,
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
    fn persistent_database_path(&mut self) -> Result<PathBuf> {
        let path = self
            .database_path
            .as_deref()
            .context("manifest publication requires a file-backed task store")?;
        if path == Path::new(":memory:") || path.to_string_lossy().starts_with("file:") {
            bail!("manifest publication requires a file-backed task store");
        }
        let path = path.canonicalize()?;
        if !path.is_file() {
            bail!("manifest publication requires a file-backed task store");
        }
        self.database_path = Some(path.clone());
        Ok(path)
    }

    /// Stage a manifest replacement in the same durable publication as this SQL transaction.
    pub(crate) fn stage_manifest_replacement(
        &mut self,
        replacement: ManifestReplacement,
    ) -> Result<()> {
        if self.manifest_replacement.is_some() {
            bail!("TASK_MANIFEST_PUBLICATION_INVALID: only one manifest replacement may be staged");
        }
        if self.context.is_none() {
            let database = self.persistent_database_path()?;
            self.publication_fence = Some(lock_database(&database)?);
        }
        replacement.validate_preimage()?;
        self.manifest_replacement = Some(replacement);
        Ok(())
    }

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
        milestone: &crate::tasks::Milestone,
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
                claim_session_fingerprints: BTreeMap::new(),
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
    pub(crate) fn commit(self) -> Result<()> {
        self.commit_with_publisher(|replacement| replacement.publish())
    }

    fn commit_with_publisher(
        mut self,
        mut publish_manifest: impl FnMut(&ManifestReplacement) -> Result<()>,
    ) -> Result<()> {
        let Some(context) = self.context.clone() else {
            let Some(replacement) = self.manifest_replacement.take() else {
                self.tx.take().expect("active transaction").commit()?;
                return Ok(());
            };
            let database = self.persistent_database_path()?;
            if self.publication_fence.is_none() {
                bail!("manifest publication requires the task database fence");
            }
            let publication = Publication {
                active: Vec::new(),
                retired: Vec::new(),
                manifest_replacement: Some(replacement),
            };
            self.execute(
                "INSERT INTO task_store_metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![PENDING, serde_json::to_string(&publication)?],
            )?;
            self.tx.take().expect("active transaction").commit()?;
            publication
                .publish_with(&mut publish_manifest)
                .map_err(|error| {
                    anyhow::Error::new(CheckpointPending).context(error.to_string())
                })?;
            clear_pending(&database).map_err(|error| {
                anyhow::Error::new(CheckpointPending).context(error.to_string())
            })?;
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
            manifest_replacement: self.manifest_replacement.take(),
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
            .publish_with(&mut publish_manifest)
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        clear_pending(&context.database)
            .map_err(|error| anyhow::Error::new(CheckpointPending).context(error.to_string()))?;
        Ok(())
    }
}

fn clear_pending(database: &Path) -> Result<()> {
    let connection = Connection::open(database)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute("DELETE FROM task_store_metadata WHERE key=?1", [PENDING])?;
    Ok(())
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

mod replay;
use replay::{states, task_home, validate_owner};
#[cfg(test)]
mod tests;
