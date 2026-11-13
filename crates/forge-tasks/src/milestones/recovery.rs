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
use rusqlite::OptionalExtension;
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
            let pb = PathBuf::from(path);
            if pb.exists() {
                if let Ok(canon) = pb.canonicalize() {
                    roots.insert(canon);
                }
            }
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
    let roots = [canonical.clone()];
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
    _gates: &std::sync::Arc<crate::core::tasks::profile::GateProfile>,
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
                completed.push(milestone.task_id(task));
            }
            Some("cancelled") if task.receipt.is_none() => (),
            _ => bail!("TASK_RECOVERY_ARCHIVE_INVALID: task status and durable receipt disagree"),
        }
    }
    Ok((status == "completed", completed))
}

fn cleanup_archives(invoking_root: &Path, roots: &[PathBuf], store: &TasksStore) -> Result<()> {
    let registry = Registry::load(invoking_root)?;
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
            let _record = archive_record(
                &workspace.root,
                &reference,
                &text,
                &milestone,
                &workspace.gates,
            )?;
            archives.push((milestone.repository, milestone.project, milestone.id, text));
        }
    }
    for root in roots {
        super::handoff::cleanup_recovered_refs(root, store, &archives)?;
    }
    Ok(())
}

fn copy_non_target_rows_from_backup(
    staging_path: &Path,
    backup_path: &Path,
    target_milestone_ref: Option<&str>,
) -> Result<usize> {
    let conn = rusqlite::Connection::open(staging_path)?;
    conn.execute(
        "ATTACH DATABASE ?1 AS original",
        [backup_path.to_str().context("backup path invalid UTF-8")?],
    )?;
    let count: usize = if let Some(target_ref) = target_milestone_ref {
        conn.execute(
            "INSERT OR REPLACE INTO tasks SELECT * FROM original.tasks WHERE milestone_ref != ?1",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_subtasks SELECT * FROM original.task_subtasks WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_claims SELECT * FROM original.task_claims WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_gates SELECT * FROM original.task_gates WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_findings SELECT * FROM original.task_findings WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_scope_paths SELECT * FROM original.task_scope_paths WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_dependencies SELECT * FROM original.task_dependencies WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_submissions SELECT * FROM original.task_submissions WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_revisions SELECT * FROM original.task_revisions WHERE task_id IN (SELECT task_id FROM original.tasks WHERE milestone_ref != ?1)",
            [target_ref],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO task_blackboard SELECT * FROM original.task_blackboard WHERE milestone_ref != ?1",
            [target_ref],
        )?;
        conn.query_row(
            "SELECT COUNT(*) FROM original.tasks WHERE milestone_ref != ?1",
            [target_ref],
            |r| r.get(0),
        )?
    } else {
        0
    };
    conn.execute("DETACH DATABASE original", [])?;
    Ok(count)
}

fn checkpoint_is_ahead(
    cp: &ActiveCheckpoint,
    cp_task: &crate::store::Task,
    stored: &crate::store::Task,
    store: &TasksStore,
) -> bool {
    if cp_task.claim_revision > stored.claim_revision {
        return true;
    }
    if cp_task.status == "completed" && stored.status != "completed" {
        return true;
    }
    if let Ok(profile) = store.profile_for_task(stored) {
        if let Ok(tp) = crate::engine::tasks::claim::flattened_profile(&profile, Some(stored)) {
            if let (Some(cp_pos), Some(stored_pos)) =
                (tp.index_of(&cp_task.stage), tp.index_of(&stored.stage))
            {
                if cp_pos > stored_pos {
                    return true;
                }
            }
        }
    }
    let stored_gates = store.task_gates(&stored.task_id).unwrap_or_default();
    for (cp_gate, cp_rev, cp_status, _) in &cp.gates {
        if cp_status == "passed" {
            let has_passed = stored_gates.iter().any(|(sg, sr, ss, _)| {
                (sg == cp_gate || sg.rsplit('.').next() == cp_gate.rsplit('.').next())
                    && sr >= cp_rev
                    && ss == "passed"
            });
            if !has_passed {
                return true;
            }
        }
    }
    let stored_submissions = store.task_submissions(&stored.task_id).unwrap_or_default();
    for cp_sub in &cp.submissions {
        if !stored_submissions.iter().any(|s| s.0 >= cp_sub.0) {
            return true;
        }
    }
    if cp_task.spec.subtasks.len() > stored.spec.subtasks.len() {
        return true;
    }
    for cp_st in &cp_task.spec.subtasks {
        match stored
            .spec
            .subtasks
            .iter()
            .find(|s| s.subtask_ref == cp_st.subtask_ref)
        {
            None => return true,
            Some(stored_st) => {
                if cp_st.status != stored_st.status {
                    return true;
                }
                if cp_st.evidence != stored_st.evidence {
                    return true;
                }
            }
        }
    }
    false
}

/// Enrolls or restores acknowledged active task checkpoints.
pub fn db_recover(root: &Path) -> Result<Value> {
    db_recover_milestone(root, None)
}

/// Enrolls or restores acknowledged active task checkpoints for an optional milestone.
pub fn db_recover_milestone(root: &Path, milestone_filter: Option<&str>) -> Result<Value> {
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
    let immutable = database_files(&scratch.directory.join("immutable.sqlite"));
    let working = database_files(&scratch.directory.join("working.sqlite"));
    let existed = original.each_ref().map(|path| path.exists());
    // Snapshot immutable original bytes immediately before opening any SQLite connection (R6).
    for ((from, imm), work) in original.iter().zip(&immutable).zip(&working) {
        if from.exists() {
            durable_copy(from, imm)?;
            durable_copy(from, work)?;
        }
    }

    // Unconditionally classify the original copy before opening or mutating the live store (R2).
    let health = if existed[0] {
        Some(persistence::probe_copy(&working[0])?)
    } else {
        None
    };

    // Reconcile pending publications and validated orphaned intents from healthy SQL under the fence (R4).
    if matches!(health, Some(StoreHealth::Healthy)) {
        if let Ok(mut store) = tasks::store(root) {
            store.set_obsolete_checkpoint_owners(obsolete.clone());
            store.adopt_checkpoint_lock(fence.clone());
            let _reconciled = store.reconcile_checkpoints()?;
            let _orphaned = store.recover_orphaned_intents(&roots)?;
        }
        // Refresh working copy with reconciled SQL decisions for staging:
        for (from, work) in original.iter().zip(&working) {
            if from.exists() {
                durable_copy(from, work)?;
            }
        }
    }

    // Discover archives in the current repository root to identify terminal history (R5).
    let mut archives = BTreeMap::new();
    for workspace in registry.selected(Some("all"))? {
        for reference in registry.recovery_manifests(workspace)? {
            if super::is_archived_path(Path::new(&reference)) {
                let path = crate::core::tasks::confined_path(&workspace.root, &reference)?;
                if let Ok(text) = fs::read_to_string(&path) {
                    if current_archive(&text)? {
                        super::handoff::proof_shas(&text)?;
                        let milestone =
                            registry.parse_milestone_manifest(workspace, &reference, &text)?;
                        if milestone.repository == workspace.repository
                            && milestone.project == workspace.project
                        {
                            let record = archive_record(
                                &workspace.root,
                                &reference,
                                &text,
                                &milestone,
                                &workspace.gates,
                            )?;
                            archives.insert(identity(&milestone), record);
                        }
                    }
                }
            }
        }
    }

    // Active manifests are loaded STRICTLY from the current workspace root (R5).
    // Never crawl foreign/unmerged worktree directories for active manifests!
    let mut all_active = Vec::new();
    for workspace in registry.selected(Some("all"))? {
        for reference in registry.recovery_manifests(workspace)? {
            if super::is_archived_path(Path::new(&reference)) {
                continue;
            }
            let path = crate::core::tasks::confined_path(&workspace.root, &reference)?;
            let text = fs::read_to_string(path)?;
            all_active.push((
                workspace.root.clone(),
                workspace.name.clone(),
                reference,
                text,
            ));
        }
    }

    // If any active milestone is shadowed by an archive in the same repository root, fail closed (R5).
    for (workspace_root, name, reference, text) in &all_active {
        let reg = Registry::load(workspace_root)?;
        let ws = reg.select(Some(name))?;
        if let Ok(owner) = reg.parse_milestone_manifest(ws, reference, &identity_header(text)?) {
            if archives.contains_key(&identity(&owner)) {
                bail!(
                    "MILESTONE_CONFLICT: milestone '{}' is present as both active and archived",
                    owner.id
                );
            }
        }
    }

    // If milestone filter is specified, validate that it matches exactly one active milestone canonically (R1, R9).
    let (_target_milestone_id, target_milestone_ref) = if let Some(filter) = milestone_filter {
        let filter_trim = filter.trim();
        if filter_trim.is_empty() {
            bail!("milestone reference cannot be empty");
        }
        let (ws, resolved_ref) = registry.resolve_milestone(None, filter_trim)?;
        let full_path = crate::core::tasks::confined_path(&ws.root, &resolved_ref)?;
        let text = fs::read_to_string(&full_path)?;
        let parsed = registry.parse_milestone_manifest(ws, &resolved_ref, &text)?;
        (Some(parsed.id), Some(resolved_ref))
    } else {
        (None, None)
    };

    let mut states = checkpoints(&roots, &archives, &obsolete)?;

    // Propagate committed SQL claim releases and revision maxima into checkpoints (R4):
    if matches!(health, Some(StoreHealth::Healthy)) {
        let working_conn = rusqlite::Connection::open(&working[0])?;
        for ((_, _, tid), state) in states.iter_mut() {
            let stored_task = crate::db::tasks_store::load(&working_conn, tid)?;
            let stored_max_rev: Option<u64> = working_conn
                .query_row(
                    "SELECT revision FROM task_revisions WHERE task_id = ?1",
                    [tid],
                    |r| r.get(0),
                )
                .optional()?;
            if let (Some(stored_task), Some(max_rev)) = (stored_task, stored_max_rev) {
                let cp_worker = state.task.as_ref().and_then(|t| t.worker_id.as_deref());
                let cp_rev = state.task.as_ref().map(|t| t.claim_revision).unwrap_or(0);
                let released_in_sql = stored_task.worker_id.is_none()
                    && stored_task.status == "ready"
                    && cp_worker.is_some()
                    && stored_task.claim_revision >= cp_rev;
                if released_in_sql
                    || stored_task.claim_revision > cp_rev
                    || max_rev > state.max_revision
                {
                    if let Some(t) = state.task.as_mut() {
                        if released_in_sql || stored_task.claim_revision > cp_rev {
                            t.status = stored_task.status;
                            t.worker_id = stored_task.worker_id;
                            t.worktree = stored_task.worktree;
                            t.claim_revision = stored_task.claim_revision;
                        }
                    }
                    state.max_revision = state.max_revision.max(max_rev);
                    if released_in_sql {
                        for claim in &mut state.claims {
                            claim.4 = true;
                        }
                    }
                }
            }
        }
    }

    // Logical staleness check: Git checkpoint is genuinely ahead of SQL (R1, R3). Do not touch disk if store did not exist (R10).
    let is_stale = if existed[0] {
        if let Ok(store) = tasks::store(root) {
            states.iter().any(|(_key, cp)| {
                if let Some(cp_task) = &cp.task {
                    match store.inspect(&cp_task.task_id) {
                        Ok(stored) => checkpoint_is_ahead(cp, cp_task, &stored, &store),
                        Err(_) => true,
                    }
                } else {
                    store.inspect(&cp.task_id).is_ok()
                }
            })
        } else {
            true
        }
    } else {
        true
    };

    if !is_stale && matches!(health, Some(StoreHealth::Healthy)) {
        let mut store = tasks::store(root)?;
        store.set_obsolete_checkpoint_owners(obsolete.clone());
        store.adopt_checkpoint_lock(fence.clone());
        store.enroll_checkpoints()?;
        let _ = cleanup_archives(root, &roots, &store);
        let unenrolled = store.unenrolled_checkpoints()?;
        if unenrolled.is_empty() {
            for r in &roots {
                let _ = super::handoff::retire_unowned_evidence(r, &store);
            }
        }
        return Ok(
            json!({"status":"healthy","checkpoint_reconciled":true,"unenrolled_tasks":unenrolled,"legacy_cleanup_complete":unenrolled.is_empty()}),
        );
    }

    // Missing, corrupt, or stale SQL cannot authorize a claim. All authority must preexist in real keepers.
    // Staging store is always created clean and preserves all active milestones (R1, R11).
    let staging = scratch.directory.join("restore.sqlite");
    let mut seed = registry.open_at(registry.select(None)?, &staging)?;
    seed.begin_recovery_stage()?;
    drop(seed);

    for workspace in registry.selected(Some("all"))? {
        let mut store = registry.open_at(workspace, &staging)?;
        store.begin_recovery_stage()?;
        for (ident, (completed, completed_ids)) in &archives {
            let prefix = format!("{}/{}/", workspace.repository, workspace.project);
            if let Some(milestone_id) = ident.strip_prefix(&prefix) {
                store.seed_archive(milestone_id, *completed, completed_ids)?;
            }
        }
    }

    let mut imported = BTreeSet::new();
    let mut admitted_milestone_ids = BTreeSet::new();

    // Preserve non-target rows from healthy SQL during targeted recovery (R1):
    if let Some(target_ref) = &target_milestone_ref {
        if matches!(health, Some(StoreHealth::Healthy)) {
            copy_non_target_rows_from_backup(&staging, &working[0], Some(target_ref))?;
            let conn = rusqlite::Connection::open(&staging)?;
            let mut stmt = conn.prepare("SELECT task_id FROM tasks WHERE milestone_ref != ?1")?;
            let non_targets: Vec<String> = stmt
                .query_map([target_ref], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            for tid in non_targets {
                if let Some((key, state)) = states.iter().find(|((_, _, id), _)| id == &tid) {
                    if let Some(cp_task) = &state.task {
                        let ws = registry.owner(&tid)?;
                        let mut store = registry.open_at(ws, &staging)?;
                        let stored = store.inspect(&tid)?;
                        if checkpoint_is_ahead(state, cp_task, &stored, &store) {
                            let conn = rusqlite::Connection::open(&staging)?;
                            conn.execute("DELETE FROM task_claims WHERE task_id = ?1", [&tid])?;
                            conn.execute("DELETE FROM task_gates WHERE task_id = ?1", [&tid])?;
                            conn.execute("DELETE FROM task_findings WHERE task_id = ?1", [&tid])?;
                            conn.execute(
                                "DELETE FROM task_scope_paths WHERE task_id = ?1",
                                [&tid],
                            )?;
                            conn.execute(
                                "DELETE FROM task_dependencies WHERE task_id = ?1",
                                [&tid],
                            )?;
                            conn.execute(
                                "DELETE FROM task_submissions WHERE task_id = ?1",
                                [&tid],
                            )?;
                            conn.execute("DELETE FROM task_subtasks WHERE task_id = ?1", [&tid])?;
                            drop(conn);
                            store.begin_recovery_stage()?;
                            store.import_checkpoint(state, &stored)?;
                        }
                    }
                    imported.insert(key.clone());
                }
            }
        }
    }

    for (workspace_root, name, reference, text) in all_active {
        let reg = Registry::load(&workspace_root)?;
        let workspace = reg.select(Some(&name))?;
        let owner =
            reg.parse_milestone_manifest(workspace, &reference, &identity_header(&text)?)?;
        if obsolete.contains(&identity(&owner)) {
            continue;
        }
        let milestone = reg.parse_milestone_manifest(workspace, &reference, &text)?;
        if milestone.repository != workspace.repository || milestone.project != workspace.project {
            continue;
        }
        if archives.contains_key(&identity(&milestone)) {
            continue;
        }
        admitted_milestone_ids.insert(milestone.id.clone());

        if let Some(target_ref) = &target_milestone_ref {
            if reference != *target_ref && matches!(health, Some(StoreHealth::Healthy)) {
                // Non-target milestone already preserved via copy_non_target_rows_from_backup
                continue;
            }
        }

        let mut store = reg.open_at(workspace, &staging)?;
        store.begin_recovery_stage()?;
        let admitted = store.sync(&milestone, &reference, &workspace.root)?;
        for task in admitted {
            let key = (
                milestone.repository.clone(),
                milestone.project.clone(),
                task.task_id.clone(),
            );
            if let Some(state) = states.get(&key) {
                if imported.insert(key) {
                    store.import_checkpoint(state, &task)?;
                }
            } else {
                imported.insert(key);
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
        if state.task.is_none()
            && !imported.contains(key)
            && admitted_milestone_ids.contains(&state.milestone_id)
        {
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
    }
    let mut verify = registry.open_at(registry.select(None)?, &staging)?;
    verify.set_workspace_root(root);
    verify.seal_recovery_stage()?;
    drop(verify);
    // Preserve original bytes, including WAL/SHM, independently of writable SQLite handles (R6).
    // Promotion is compensated file-by-file on failure; no cross-file atomicity is claimed.
    let retained = PathBuf::from(format!("{}-recovery-backup.{nonce}", database.display()));
    if existed.iter().any(|present| *present) {
        fs::create_dir(&retained)?;
        for (index, from) in immutable.iter().enumerate() {
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
                let _ = fs::remove_file(path);
            }
            if immutable[index].exists() {
                durable_copy(&immutable[index], path)?;
            }
        }
        return Err(error.context("TASK_RECOVERY_PROMOTION_FAILED: original bytes restored"));
    }
    let mut restored = tasks::store(root)?;
    restored.adopt_checkpoint_lock(fence.clone());
    let _ = cleanup_archives(root, &roots, &restored);
    restored.enroll_checkpoints()?;
    for root in &roots {
        let _ = super::handoff::retire_unowned_evidence(root, &restored);
    }
    Ok(
        json!({"status":"recovered","active_tasks":imported.len(),"tombstones":archives.len(),"original_backup":if existed[0] {Some(retained)} else {None}}),
    )
}
