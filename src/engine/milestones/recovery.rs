//! Explicit recovery of acknowledged active task state.

use crate::{
    core::{
        git::{git_command, git_stdout},
        tasks::Milestone,
    },
    db::tasks_store::{
        recovery::{self as persistence, ActiveCheckpoint, StoreHealth},
        TasksStore,
    },
    engine::tasks::{self, Registry},
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

struct RecoveryFiles {
    directory: PathBuf,
}
impl Drop for RecoveryFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn database_files(path: &Path) -> [PathBuf; 3] {
    [
        path.to_owned(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ]
}

fn durable_copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to)?;
    fs::File::open(to)?.sync_all()?;
    Ok(())
}

fn worktrees(root: &Path) -> Result<Vec<PathBuf>> {
    let output = git_stdout(
        root,
        &["worktree", "list", "--porcelain", "-z"],
        "TASK_RECOVERY_WORKTREES_INVALID",
    )?;
    let mut roots = BTreeSet::from([root.canonicalize()?]);
    for field in output.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            roots.insert(PathBuf::from(path).canonicalize()?);
        }
    }
    Ok(roots.into_iter().collect())
}

fn checkpoints(
    roots: &[PathBuf],
    archives: &BTreeMap<String, (bool, Vec<String>)>,
    obsolete: &BTreeSet<String>,
) -> Result<BTreeMap<(String, String, String), ActiveCheckpoint>> {
    let mut states = BTreeMap::new();
    for root in roots {
        let output = git_command(
            root,
            &["for-each-ref", "--format=%(refname)", "refs/forge/state/"],
        )?;
        if !output.status.success() {
            bail!("TASK_RECOVERY_CHECKPOINT_INVALID: cannot enumerate task keepers");
        }
        for reference in String::from_utf8(output.stdout)?.lines() {
            if archives
                .keys()
                .chain(obsolete.iter())
                .any(|owner| reference.starts_with(&format!("refs/forge/state/{owner}/")))
            {
                continue;
            }
            let state = persistence::read_checkpoint(root, reference)?;
            state.validate_cohort()?;
            let key = (
                state.repository.clone(),
                state.project.clone(),
                state.task_id.clone(),
            );
            if let Some(previous) = states.insert(key, state.clone()) {
                if serde_json::to_vec(&previous)? != serde_json::to_vec(&state)? {
                    bail!("TASK_RECOVERY_CHECKPOINT_CONFLICT: multiple object homes disagree");
                }
            }
        }
    }
    Ok(states)
}

fn identity(milestone: &Milestone) -> String {
    format!(
        "{}/{}/{}",
        milestone.repository, milestone.project, milestone.id
    )
}

fn recovery_metadata(text: &str) -> Result<serde_yaml::Value> {
    let text = text.replace("\r\n", "\n");
    let (header, _) = text
        .strip_prefix("---\n")
        .and_then(|body| body.split_once("\n---\n"))
        .context("TASK_RECOVERY_ARCHIVE_INVALID: missing or unterminated frontmatter")?;
    Ok(serde_yaml::from_str(header)?)
}

fn current_archive(text: &str) -> Result<bool> {
    if !text.replace("\r\n", "\n").starts_with("---\n") {
        return Ok(false);
    }
    Ok(recovery_metadata(text)?
        .get("handoff")
        .and_then(|handoff| handoff.get("proof_shas"))
        .is_some())
}

fn identity_header(text: &str) -> Result<String> {
    let metadata = recovery_metadata(text)?;
    let mut owner = serde_yaml::Mapping::new();
    for key in ["id", "repository", "project"] {
        if let Some(value) = metadata.get(key) {
            owner.insert(serde_yaml::Value::String(key.into()), value.clone());
        }
    }
    Ok(format!("---\n{}---\n", serde_yaml::to_string(&owner)?))
}

pub(crate) fn obsolete_checkpoint_owners(
    root: &Path,
    directories: &[PathBuf],
    repository: &str,
    project: &str,
    project_owners: &BTreeMap<String, String>,
    aliases: &BTreeMap<String, String>,
) -> Result<BTreeSet<String>> {
    let mut owners = BTreeSet::new();
    let canonical = root.canonicalize()?;
    let mut roots = BTreeSet::from([canonical.clone()]);
    let top = git_command(root, &["rev-parse", "--show-toplevel"])?;
    if top.status.success() {
        let top = PathBuf::from(String::from_utf8(top.stdout)?.trim()).canonicalize()?;
        let relative = canonical.strip_prefix(&top)?;
        for worktree in worktrees(root)? {
            if worktree != canonical {
                let candidate = worktree.join(relative);
                if candidate.is_dir() {
                    roots.insert(candidate);
                }
            }
        }
    }
    let directories = directories
        .iter()
        .map(|dir| {
            Ok(dir
                .strip_prefix(&canonical)
                .or_else(|_| dir.strip_prefix(root))?
                .to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    for root in roots {
        let directories = directories
            .iter()
            .map(|dir| root.join(dir))
            .collect::<Vec<_>>();
        for path in tasks::numbered_manifests(&directories)? {
            let reference = path.strip_prefix(&root)?;
            if !super::is_archived_path(reference) {
                continue;
            }
            let reference = reference.to_string_lossy();
            let path = crate::core::tasks::confined_path(&root, &reference)?;
            let text = fs::read_to_string(path)?;
            // Classification is not archive validation or profile loading.
            let Ok(metadata) = recovery_metadata(&text) else {
                continue;
            };
            if metadata
                .get("id")
                .and_then(serde_yaml::Value::as_str)
                .is_none()
                || metadata
                    .get("handoff")
                    .and_then(|value| value.get("proof_shas"))
                    .is_some()
            {
                continue;
            }
            if let Ok(milestone) = crate::core::tasks::parse_milestone_manifest(
                &identity_header(&text)?,
                Path::new(reference.as_ref()),
                repository,
                project,
                project_owners,
                aliases,
            ) {
                owners.insert(identity(&milestone));
            }
        }
    }
    Ok(owners)
}

fn archive_record(
    root: &Path,
    reference: &str,
    text: &str,
    milestone: &Milestone,
    gates: &std::sync::Arc<crate::core::tasks::profile::GateProfile>,
) -> Result<(bool, Vec<String>)> {
    super::handoff::proof_shas(text)?;
    let status = Milestone::status_from_frontmatter(text, true)?;
    if !matches!(status, "completed" | "cancelled") {
        bail!("TASK_RECOVERY_ARCHIVE_INVALID: archive must be terminal");
    }
    if status == "completed"
        && crate::db::tasks_store::read_milestone_archive(
            root,
            &root.join(reference),
            &milestone.repository,
            &milestone.project,
        )?
        .is_none()
    {
        bail!("TASK_RECOVERY_ARCHIVE_INVALID: archive authority is invalid");
    }
    let mut completed = Vec::new();
    for task in &milestone.tasks {
        match task.status.as_deref() {
            Some("completed")
                if task
                    .receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.decision == "pass") =>
            {
                let pin = task
                    .acdd_profile
                    .as_ref()
                    .or(milestone.acdd_profile.as_ref());
                let profile = pin
                    .map(|pin| crate::core::tasks::profile::load_pinned_over(root, pin, gates))
                    .transpose()?
                    .unwrap_or_else(|| gates.clone());
                crate::core::tasks::gates::validate_durable_receipt_with_profile(
                    task.receipt.as_ref().context("completed receipt")?,
                    task.contract_revision,
                    &profile,
                )?;
                completed.push(milestone.task_id(task));
            }
            Some("cancelled") if task.receipt.is_none() => (),
            _ => bail!("TASK_RECOVERY_ARCHIVE_INVALID: task status and durable receipt disagree"),
        }
    }
    Ok((status == "completed", completed))
}

fn cleanup_archives(roots: &[PathBuf], store: &TasksStore) -> Result<()> {
    let mut validated = Vec::new();
    for root in roots {
        let registry = Registry::load(root)?;
        let mut archives = Vec::new();
        for workspace in registry.selected(Some("all"))? {
            for reference in registry.recovery_manifests(workspace)? {
                if !super::is_archived_path(Path::new(&reference)) {
                    continue;
                }
                let path = crate::core::tasks::confined_path(&workspace.root, &reference)?;
                let text = fs::read_to_string(path)?;
                if !current_archive(&text)? {
                    continue;
                }
                let milestone = registry.parse_milestone_manifest(workspace, &reference, &text)?;
                if milestone.repository != workspace.repository
                    || milestone.project != workspace.project
                {
                    continue;
                }
                archive_record(
                    &workspace.root,
                    &reference,
                    &text,
                    &milestone,
                    &workspace.gates,
                )?;
                archives.push((milestone.repository, milestone.project, milestone.id, text));
            }
        }
        validated.push((root, archives));
    }
    for (root, archives) in validated {
        super::handoff::cleanup_recovered_refs(root, store, &archives)?;
    }
    Ok(())
}

/// Enrolls or restores acknowledged active task checkpoints.
pub fn db_recover(root: &Path) -> Result<Value> {
    let registry = Registry::load(root)?;
    let mut roots = worktrees(root)?;
    for workspace in registry.selected(Some("all"))? {
        roots.push(workspace.root.clone());
    }
    roots.sort();
    roots.dedup();
    let obsolete = registry.obsolete_checkpoint_owners()?;
    let database = &registry.database;
    fs::create_dir_all(database.parent().context("task database has no parent")?)?;
    let fence = persistence::lock_database(database)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let directory = database
        .parent()
        .context("task database parent")?
        .join(format!("task-recovery-{}-{nonce}", std::process::id()));
    fs::create_dir(&directory)?;
    let scratch = RecoveryFiles { directory };
    let original = database_files(database);
    let backup = database_files(&scratch.directory.join("original.sqlite"));
    let existed = original.each_ref().map(|path| path.exists());
    for (from, to) in original.iter().zip(&backup) {
        if from.exists() {
            durable_copy(from, to)?;
        }
    }
    if existed[0] && matches!(persistence::probe_copy(&backup[0])?, StoreHealth::Healthy) {
        let mut store = tasks::store(root)?;
        store.set_obsolete_checkpoint_owners(obsolete.clone());
        store.adopt_checkpoint_lock(fence.clone());
        let reconciled = store.reconcile_checkpoints()?;
        let orphaned = store.recover_orphaned_intents(&roots)?;
        if !reconciled || orphaned != 0 {
            for root in &roots {
                persistence::ensure_no_intents_except(root, &obsolete)?;
            }
            store.enroll_checkpoints()?;
        }
        cleanup_archives(&roots, &store)?;
        let unenrolled = store.unenrolled_checkpoints()?;
        if unenrolled.is_empty() {
            for root in &roots {
                super::handoff::retire_unowned_evidence(root, &store)?;
            }
        }
        return Ok(
            json!({"status":"healthy","checkpoint_reconciled":reconciled,"unenrolled_tasks":unenrolled,"legacy_cleanup_complete":unenrolled.is_empty()}),
        );
    }
    // Missing/corrupt SQL cannot authorize a claim. All authority must preexist in real keepers.
    let staging = scratch.directory.join("restore.sqlite");
    let mut seed = TasksStore::open(&staging)?;
    seed.begin_recovery_stage()?;
    drop(seed);
    // Discover archives across every worktree before admitting any active manifest.
    let mut archives = BTreeMap::new();
    let mut active = Vec::new();
    for worktree in &roots {
        let registry = Registry::load(worktree)?;
        for workspace in registry.selected(Some("all"))? {
            for reference in registry.recovery_manifests(workspace)? {
                let path = crate::core::tasks::confined_path(&workspace.root, &reference)?;
                let text = fs::read_to_string(path)?;
                if super::is_archived_path(Path::new(&reference)) && !current_archive(&text)? {
                    continue;
                }
                if super::is_archived_path(Path::new(&reference)) {
                    let milestone =
                        registry.parse_milestone_manifest(workspace, &reference, &text)?;
                    if milestone.repository != workspace.repository
                        || milestone.project != workspace.project
                    {
                        continue;
                    }
                    let record = archive_record(
                        &workspace.root,
                        &reference,
                        &text,
                        &milestone,
                        &workspace.gates,
                    )?;
                    if let Some(previous) = archives.insert(identity(&milestone), record.clone()) {
                        if previous != record {
                            bail!("TASK_RECOVERY_ARCHIVE_CONFLICT: terminal worktrees disagree");
                        }
                    }
                    let mut store = registry.open_at(workspace, &staging)?;
                    store.begin_recovery_stage()?;
                    store.seed_archive(&milestone.id, record.0, &record.1)?;
                } else {
                    active.push((
                        workspace.root.clone(),
                        workspace.name.clone(),
                        reference,
                        text,
                    ));
                }
            }
        }
    }
    let states = checkpoints(&roots, &archives, &obsolete)?;
    let mut imported = BTreeSet::new();
    let mut admitted_milestones = BTreeMap::new();
    for (workspace_root, name, reference, text) in active {
        let registry = Registry::load(&workspace_root)?;
        let workspace = registry.select(Some(&name))?;
        let owner =
            registry.parse_milestone_manifest(workspace, &reference, &identity_header(&text)?)?;
        if obsolete.contains(&identity(&owner)) {
            continue;
        }
        let milestone = registry.parse_milestone_manifest(workspace, &reference, &text)?;
        if milestone.repository != workspace.repository || milestone.project != workspace.project {
            continue;
        }
        if archives.contains_key(&identity(&milestone)) {
            continue;
        }
        let authority = milestone
            .tasks
            .iter()
            .map(|spec| Ok((milestone.task_id(spec), milestone.digest(spec)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        if let Some(previous) = admitted_milestones.insert(identity(&milestone), authority.clone())
        {
            if previous != authority {
                bail!("TASK_RECOVERY_AUTHORITY_MISMATCH: active worktree contracts disagree");
            }
            continue;
        }
        let mut store = registry.open_at(workspace, &staging)?;
        store.begin_recovery_stage()?;
        let admitted = store.sync(&milestone, &reference, &workspace.root)?;
        for task in admitted {
            let key = (
                milestone.repository.clone(),
                milestone.project.clone(),
                task.task_id.clone(),
            );
            let state = states.get(&key).with_context(|| format!("TASK_RECOVERY_LEGACY_UNENROLLED: {} has no checkpoint; manifests cannot reconstruct runtime claims",task.task_id))?;
            if imported.insert(key) {
                store.import_checkpoint(state, &task)?;
            }
        }
        for (key, state) in &states {
            if state.task.is_none()
                && state.repository == milestone.repository
                && state.project == milestone.project
                && state.milestone_id == milestone.id
                && imported.insert(key.clone())
            {
                store.import_task_fence(state)?;
            }
        }
    }
    for (key, state) in &states {
        if archives.contains_key(&format!(
            "{}/{}/{}",
            state.repository, state.project, state.milestone_id
        )) {
            continue;
        }
        if state.task.is_none() && !imported.contains(key) {
            let workspace = registry
                .selected(Some("all"))?
                .into_iter()
                .find(|workspace| {
                    workspace.repository == state.repository && workspace.project == state.project
                })
                .context(
                    "TASK_RECOVERY_NAMESPACE_MISMATCH: allocation fence has no registered owner",
                )?;
            let mut store = registry.open_at(workspace, &staging)?;
            store.begin_recovery_stage()?;
            store.import_task_fence(state)?;
            imported.insert(key.clone());
        }
        if !imported.contains(key) {
            bail!(
                "TASK_RECOVERY_AUTHORITY_MISSING: {} has no current admitted manifest",
                state.task_id
            );
        }
    }
    let mut verify = TasksStore::open(&staging)?;
    verify.set_workspace_root(root);
    verify.seal_recovery_stage()?;
    drop(verify);
    // Preserve original bytes, including WAL/SHM, independently of writable SQLite handles.
    // Promotion is compensated file-by-file on failure; no cross-file atomicity is claimed.
    let retained = PathBuf::from(format!("{}-recovery-backup.{nonce}", database.display()));
    if existed.iter().any(|present| *present) {
        fs::create_dir(&retained)?;
        for (index, from) in backup.iter().enumerate() {
            if from.exists() {
                durable_copy(from, &retained.join(index.to_string()))?;
            }
        }
    }
    let promotion = (|| -> Result<()> {
        for path in &original {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        fs::rename(&staging, database)?;
        fs::File::open(database)?.sync_all()?;
        fs::File::open(database.parent().context("task database parent")?)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = promotion {
        for (index, path) in original.iter().enumerate() {
            if path.exists() {
                fs::remove_file(path)?;
            }
            if existed[index] {
                durable_copy(&backup[index], path)?;
            }
        }
        return Err(error.context("TASK_RECOVERY_PROMOTION_FAILED: original bytes restored"));
    }
    let mut restored = tasks::store(root)?;
    restored.adopt_checkpoint_lock(fence.clone());
    cleanup_archives(&roots, &restored)?;
    restored.enroll_checkpoints()?;
    for root in &roots {
        super::handoff::retire_unowned_evidence(root, &restored)?;
    }
    Ok(
        json!({"status":"recovered","active_tasks":imported.len(),"tombstones":archives.len(),"original_backup":if existed[0] {Some(retained)} else {None}}),
    )
}
