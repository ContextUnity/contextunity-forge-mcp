use super::{
    contract::Snapshot,
    integration::{self, HandoffBinding, IntegrationEvidence},
    render_task_receipt, snapshots,
};
use crate::db::tasks_store::{Task, TasksStore};
use crate::engine::tasks;
use anyhow::{bail, Context, Result};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod provenance;
mod telemetry;
#[cfg(test)]
mod tests;
mod verification;

pub use telemetry::set_symbol_facts_extractor;
pub use verification::{execute_milestone_verification, execute_wave_verification};

use provenance::*;
use telemetry::*;
pub(super) use telemetry::{cleanup_recovered_refs, proof_shas, retire_unowned_evidence};
use verification::*;

fn full_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffJournal {
    binding: HandoffBinding,
    commit: String,
    target_base: String,
    active_ref: String,
    archive_ref: String,
    active_text: String,
    archive_text: String,
    tasks: Vec<Task>,
    receipts: BTreeMap<String, crate::core::tasks::Receipt>,
}

fn journal_path(root: &Path, binding: &HandoffBinding) -> Result<std::path::PathBuf> {
    let owner = (&binding.repository, &binding.project, &binding.milestone);
    Ok(crate::core::fs::checked_child(
        root,
        Path::new(&format!(
            ".forge/handoff-{}.json",
            integration::digest(&owner)?
        )),
    )?)
}

fn resume_journal(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    evidence: &IntegrationEvidence,
    fence: &crate::db::tasks_store::recovery::DatabaseFence,
) -> Result<Option<Value>> {
    let path = journal_path(root, &evidence.binding)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let journal: HandoffJournal = serde_json::from_slice(&bytes)
        .context("HANDOFF_JOURNAL_INVALID: malformed handoff journal")?;
    evidence.validate(&journal.binding)?;
    let active = crate::core::fs::checked_child(root, Path::new(&journal.active_ref))?;
    let archive = crate::core::fs::checked_child(root, Path::new(&journal.archive_ref))?;
    let expected_archive = active
        .parent()
        .context("milestone parent missing")?
        .join("archive")
        .join(active.file_name().context("milestone filename missing")?);
    if archive != expected_archive
        || (selector != journal.binding.milestone
            && !active
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&format!("{selector}-"))))
        || integration::resolve_commit(root, commit.unwrap_or("HEAD"))? != journal.commit
        || integration::resolve_commit(root, &evidence.target_base)? != journal.target_base
    {
        bail!("HANDOFF_JOURNAL_INVALID: handoff identity, paths or commits differ");
    }
    let identity = format!(
        "{}/{}/{}",
        journal.binding.repository, journal.binding.project, journal.binding.milestone
    );
    let Some(note) = integration::read_passport(root, &journal.commit, &identity)? else {
        // A crash before note publication has not changed the archive or SQL.
        // Let the ordinary validated handoff prepare the transaction again.
        return Ok(None);
    };
    if note["binding"] != serde_json::to_value(&journal.binding)?
        || note["journal_digest"].as_str()
            != Some(integration::digest(&serde_json::to_value(&journal)?)?.as_str())
    {
        bail!("HANDOFF_JOURNAL_INVALID: journal differs from its prepared digest");
    }
    if active.exists() && fs::read_to_string(&active)? != journal.active_text {
        bail!("HANDOFF_ARCHIVE_INVALID: active authority changed during handoff");
    }
    let settings = tasks::TaskSettings::load(root)?;
    let parsed = crate::db::tasks_store::configured_milestone_archive(
        root,
        &journal.archive_text,
        &archive,
        &journal.binding.repository,
        &journal.binding.project,
    )?
    .context("HANDOFF_JOURNAL_INVALID: journal archive is not durable authority")?;
    if parsed.id != journal.binding.milestone {
        bail!("HANDOFF_JOURNAL_INVALID: archive milestone differs");
    }
    if archive.exists() {
        let text = fs::read_to_string(&archive)?;
        if let Some((header, _)) = text
            .strip_prefix("---\n")
            .and_then(|text| text.split_once("\n---\n"))
        {
            if let Ok(header) = serde_yaml::from_str::<serde_yaml::Value>(header) {
                let project = settings.resolve_project(
                    archive.strip_prefix(root)?,
                    header["project"].as_str(),
                    settings
                        .task_project
                        .as_deref()
                        .unwrap_or(&settings.task_repository),
                )?;
                if header["id"]
                    .as_str()
                    .is_some_and(|id| id != journal.binding.milestone)
                    || header["repository"]
                        .as_str()
                        .unwrap_or(&settings.task_repository)
                        != journal.binding.repository
                    || project != journal.binding.project
                {
                    bail!("HANDOFF_ARCHIVE_ID_MISMATCH: existing archive belongs to another milestone");
                }
            }
        }
    }
    let mut store = TasksStore::open_project(
        &tasks::database_path(root)?,
        &journal.binding.repository,
        &journal.binding.project,
    )?;
    store.adopt_checkpoint_lock(fence.clone());
    store.set_workspace_root(root);
    store.set_handoff_pruning(settings.handoff_pruning);
    store.reconcile_checkpoints()?;
    let tombstoned = store.milestone_tombstoned(&journal.binding.milestone)?;
    if !tombstoned && !active.exists() {
        crate::core::fs::atomic_replace(&active, journal.active_text.as_bytes())?;
    }
    store.recover_handoff_intents(&[root.to_path_buf()], &journal.tasks)?;
    integration::configure_checkpoint_owners(&mut store, root)?;
    let owners = active_proof_owners(root, &store, &identity)?;
    let prefix = format!("{}/{}:", journal.binding.project, journal.binding.milestone);
    let stored = store.list(None, "all", None)?;
    let relevant = stored
        .iter()
        .filter(|task| task.task_id.starts_with(&prefix))
        .collect::<Vec<_>>();
    let relocated = !settings.handoff_pruning
        && relevant.len() == journal.tasks.len()
        && relevant.iter().all(|task| {
            task.milestone_ref == journal.archive_ref
                && journal.tasks.iter().any(|expected| {
                    expected.task_id == task.task_id && expected.status == task.status
                })
                && task.receipt.as_ref() == journal.receipts.get(&task.task_id)
        });
    fs::create_dir_all(archive.parent().context("archive parent missing")?)?;
    crate::core::fs::atomic_replace(&archive, journal.archive_text.as_bytes())?;
    if !tombstoned && !relocated {
        // Checkpoint enrollment validates the original authority before the SQL
        // transition. A post-unlink crash restores only that journal-bound input.
        if !active.exists() {
            crate::core::fs::atomic_replace(&active, journal.active_text.as_bytes())?;
        }
        store.handoff_milestone_with_unlink(
            &prefix,
            &journal.active_ref,
            &journal.archive_ref,
            &journal.tasks,
            &journal.receipts,
            || {
                fs::remove_file(&active)?;
                Ok(())
            },
        )?;
    } else if active.exists() {
        fs::remove_file(&active)?;
    }
    cleanup_handoff_refs(
        root,
        &journal.binding.repository,
        &journal.binding.project,
        &journal.binding.milestone,
        &journal.archive_text,
        &owners,
    )?;
    fs::remove_file(path)?;
    Ok(Some(
        json!({"id":journal.binding.milestone,"status":"completed","path":journal.archive_ref}),
    ))
}

fn canonical_git_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

fn archive_project(root: &Path, item: &Snapshot, settings: &tasks::TaskSettings) -> Result<String> {
    let default = settings.project_for_path(
        item.path.strip_prefix(root)?,
        settings
            .task_project
            .as_deref()
            .unwrap_or(&settings.task_repository),
    );
    // Preserve the parser's declared input, so a single-hop alias is not applied
    // again to an already canonical project when the archive is read or repaired.
    Ok(item.frontmatter["project"]
        .as_str()
        .unwrap_or(&default)
        .to_owned())
}

fn binding(
    root: &Path,
    item: &Snapshot,
    profile: &crate::core::tasks::profile::GateProfile,
    store: &TasksStore,
    relevant: &[Task],
    commit: &str,
    target: &str,
) -> Result<HandoffBinding> {
    integration::require_clean_tree(root, commit, &archive_outputs(root, item)?)?;
    let (authority_digest, builders) = validated_authority(root, item, store, relevant)?;
    let milestone_path = item.path.strip_prefix(root)?.to_string_lossy().into_owned();
    let changes = crate::core::git::git_command(
        root,
        &["diff", "--name-only", "--no-renames", "-z", target, commit],
    )?;
    if !changes.status.success() {
        bail!("HANDOFF_CONTENT_UNAVAILABLE: cannot compare target base and final content");
    }
    let mut polish_scope = profile.polish_scope.clone();
    polish_scope.extend(item.polish_scope.clone());
    for path in changes
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?;
        if !is_admitted_handoff_path(path, &milestone_path, &polish_scope, relevant) {
            bail!("HANDOFF_SCOPE_UNADMITTED: final delta contains unadmitted path '{path}'");
        }
    }
    Ok(HandoffBinding {
        version: 1,
        repository: item.repository.clone(),
        project: item.project.clone(),
        milestone: item.id.clone(),
        authority_digest,
        implementation_digest: integration::content_digest(root, commit, &milestone_path)?,
        target_base_digest: integration::content_digest(root, target, &milestone_path)?,
        builders,
    })
}

struct PreparedHandoff {
    item: Snapshot,
    store: TasksStore,
    tasks: Vec<Task>,
    profile: crate::core::tasks::profile::GateProfile,
    binding: HandoffBinding,
    commit: String,
    target: String,
}

fn prepare_handoff_context(
    root: &Path,
    selector: &str,
    target_base: &str,
) -> Result<PreparedHandoff> {
    let fence = crate::db::tasks_store::recovery::lock_database(&tasks::database_path(root)?)?;
    let (item, mut store, relevant) = ready_milestone(root, selector)?;
    store.adopt_checkpoint_lock(fence.clone());
    validated_authority(root, &item, &store, &relevant)?;
    let handoff_profile = (*store.profile_for_milestone(
        root,
        item.frontmatter
            .get("acdd_profile")
            .and_then(|value| value.as_str()),
        &store.profile,
        &relevant,
    )?)
    .clone();
    let root_cursors = crate::engine::gate_graph::collect_root_cursors(&handoff_profile)?;
    let delivery_gate = root_cursors
        .iter()
        .find(|cursor| crate::core::tasks::profile::gate_is_delivery(&cursor.gate));
    let mut polish_scope = handoff_profile.polish_scope.clone();
    polish_scope.extend(item.polish_scope.clone());
    let should_absorb = delivery_gate
        .map(|cursor| cursor.gate.absorb_working_tree)
        .unwrap_or(true);
    if should_absorb {
        absorb_polish(root, &item, &polish_scope, &relevant)?;
    }
    let commit = integration::resolve_commit(root, "HEAD")?;
    let target = integration::resolve_commit(root, target_base)?;
    let binding = binding(
        root,
        &item,
        &handoff_profile,
        &store,
        &relevant,
        &commit,
        &target,
    )?;
    crate::core::git::retain_proof_commit(root, &commit)?;
    crate::core::git::retain_proof_commit(root, &target)?;
    let owner = integration::HandoffProofOwner {
        identity: format!("{}/{}/{}", item.repository, item.project, item.id),
        worktree: root.canonicalize()?,
        proof_shas: BTreeSet::from([commit.clone(), target.clone()]),
    };
    integration::enroll_preparation(&mut store, root, &owner)?;
    integration::write_handoff_owner(root, &commit, &owner)?;
    Ok(PreparedHandoff {
        item,
        store,
        tasks: relevant,
        profile: handoff_profile,
        binding,
        commit,
        target,
    })
}

/// Prepare content identities for external integration review and verification.
/// Accepted proof objects are retained independently of mutable branch history.
pub fn handoff_context(root: &Path, selector: &str, target_base: &str) -> Result<Value> {
    let prepared = prepare_handoff_context(root, selector, target_base)?;
    Ok(json!({
        "binding":prepared.binding,
        "commit":prepared.commit,
        "target_base":prepared.target,
        "requires_project_verification": true
    }))
}

fn archive_outputs(root: &Path, item: &Snapshot) -> Result<Vec<String>> {
    let archived = item
        .path
        .parent()
        .context("milestone parent missing")?
        .join("archive")
        .join(
            item.path
                .file_name()
                .context("milestone filename missing")?,
        );
    let path = archived.strip_prefix(root)?.to_string_lossy().into_owned();
    Ok(vec![path.clone(), format!("{path}.bak")])
}

fn navigation_pointer(
    root: &Path,
    head: &str,
    accepted: &str,
    scope: &[String],
) -> Result<Option<String>> {
    if crate::core::git::git_ancestor(root, accepted, head)? {
        return Ok(Some(accepted.to_owned()));
    }
    let mut args = vec!["log", "--format=%H", "--full-history", head, "--"];
    args.extend(scope.iter().map(String::as_str));
    let history = crate::core::git::git_command(root, &args)?;
    if !history.status.success() {
        return Ok(None);
    }
    let mut found = None;
    for candidate in String::from_utf8(history.stdout)?.lines() {
        if !crate::core::git::git_scoped_tree_changed(root, accepted, candidate, scope)? {
            if found.is_some() {
                return Ok(None);
            }
            found = Some(candidate.to_owned());
        }
    }
    Ok(found)
}

fn validate_archived_retry(
    root: &Path,
    archive_text: &str,
    active_text: Option<&str>,
    commit: Option<&str>,
    evidence: &IntegrationEvidence,
) -> Result<()> {
    proof_shas(archive_text)?;
    let header = archive_text
        .strip_prefix("---\n")
        .and_then(|text| text.split_once("\n---\n"))
        .context("HANDOFF_ARCHIVE_INVALID: archive frontmatter is missing")?
        .0;
    let metadata: serde_yaml::Value = serde_yaml::from_str(header)?;
    let accepted_binding: HandoffBinding =
        serde_yaml::from_value(metadata["handoff"]["integration_evidence"]["binding"].clone())?;
    let accepted_commit = metadata["handoff"]["commit"]
        .as_str()
        .context("HANDOFF_ARCHIVE_INVALID: candidate is missing")?;
    let accepted_target = metadata["handoff"]["target_base"]
        .as_str()
        .context("HANDOFF_ARCHIVE_INVALID: target base is missing")?;
    evidence.validate(&accepted_binding)?;
    if integration::resolve_commit(root, &evidence.target_base)? != accepted_target
        || commit
            .map(|commit| integration::resolve_commit(root, commit))
            .transpose()?
            .as_deref()
            .is_some_and(|commit| commit != accepted_commit)
        || active_text
            .map(|text| -> Result<bool> {
                Ok(
                    integration::digest(&integration::manifest_projection(text.as_bytes())?)?
                        != metadata["handoff"]["passport"]["active_authority_digest"]
                            .as_str()
                            .unwrap_or_default(),
                )
            })
            .transpose()?
            .unwrap_or(false)
    {
        bail!("HANDOFF_EVIDENCE_STALE: handoff retry differs from the archived candidate or authority");
    }
    Ok(())
}

// Resolve recovery before readiness: accepted gates may already have been pruned.
fn retry_handoff(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    evidence: &IntegrationEvidence,
    fence: &crate::db::tasks_store::recovery::DatabaseFence,
) -> Result<Option<Value>> {
    if let Some(result) = resume_journal(root, selector, commit, evidence, fence)? {
        return Ok(Some(result));
    }
    let settings = tasks::TaskSettings::load(root)?;
    let mut active = snapshots(root, false, Some(selector))?;
    if active.len() > 1 {
        bail!("milestone selector must resolve to exactly one active document: {selector}");
    }
    let item = if let Some(item) = active.pop() {
        Some(item)
    } else {
        let mut archived = snapshots(root, true, Some(selector))?;
        if archived.len() > 1 {
            bail!("milestone selector is ambiguous: {selector}");
        }
        archived.pop()
    };
    let (repository, project, id) = item
        .as_ref()
        .map(|item| {
            (
                item.repository.as_str(),
                item.project.as_str(),
                item.id.as_str(),
            )
        })
        .unwrap_or((
            &evidence.binding.repository,
            &evidence.binding.project,
            &evidence.binding.milestone,
        ));
    if item.is_none() && selector != id {
        return Err(already_handed_off(id));
    }
    for identity in [repository, project, id] {
        crate::core::tasks::valid_identity(identity)?;
    }
    let db = tasks::database_path(root)?;
    let mut store = TasksStore::open_project(&db, repository, project)?;
    store.adopt_checkpoint_lock(fence.clone());
    store.set_workspace_root(root);
    integration::configure_checkpoint_owners(&mut store, root)?;
    let owners = active_proof_owners(root, &store, &format!("{repository}/{project}/{id}"))?;
    let tombstoned = store.milestone_tombstoned(id)?;
    let Some(item) = item else {
        drop(store);
        if !tombstoned {
            return Ok(None);
        }
        let files = crate::core::git::git_stdout(
            root,
            &["ls-tree", "-r", "--name-only", "HEAD"],
            "HANDOFF_CONTENT_UNAVAILABLE",
        )?;
        for path in files
            .lines()
            .filter(|path| super::is_archived_path(Path::new(path)))
        {
            let text = crate::core::git::git_stdout(
                root,
                &["show", &format!("HEAD:{path}")],
                "HANDOFF_CONTENT_UNAVAILABLE",
            )?;
            let Ok(parsed) = crate::core::tasks::parse_milestone_manifest(
                &text,
                Path::new(path),
                repository,
                settings
                    .task_project
                    .as_deref()
                    .unwrap_or(&settings.task_repository),
                &settings.owners,
                &settings.aliases,
            ) else {
                continue;
            };
            if parsed.id == id
                && parsed.repository == repository
                && parsed.project == project
                && crate::db::tasks_store::configured_milestone_archive(
                    root,
                    &text,
                    Path::new(path),
                    repository,
                    &parsed.project,
                )?
                .is_some()
            {
                validate_archived_retry(root, &text, None, commit, evidence)?;
                cleanup_handoff_refs(root, repository, project, id, &text, &owners)?;
                return Ok(Some(json!({"id":id,"status":"completed","path":path})));
            }
        }
        bail!("HANDOFF_ARCHIVE_INVALID: archived authority is unavailable; recover the archived milestone before retrying handoff");
    };
    let active_present = !super::is_archived_path(&item.path);
    let archived = if active_present {
        item.path
            .parent()
            .context("milestone parent missing")?
            .join("archive")
            .join(
                item.path
                    .file_name()
                    .context("milestone filename missing")?,
            )
    } else {
        item.path.clone()
    };
    let archive_ref = archived.strip_prefix(root)?.to_string_lossy().into_owned();
    if archived.exists() {
        let text = fs::read_to_string(&archived)?;
        if let Some(header) = text
            .strip_prefix("---\n")
            .and_then(|body| body.split_once("\n---\n"))
            .and_then(|(header, _)| serde_yaml::from_str::<serde_yaml::Value>(header).ok())
        {
            let owner = settings.resolve_project(
                archived.strip_prefix(root)?,
                header["project"].as_str(),
                settings
                    .task_project
                    .as_deref()
                    .unwrap_or(&settings.task_repository),
            )?;
            if header["id"].as_str().is_some_and(|id| id != item.id)
                || header["repository"]
                    .as_str()
                    .unwrap_or(&settings.task_repository)
                    != item.repository
                || owner != item.project
            {
                bail!("HANDOFF_ARCHIVE_ID_MISMATCH: existing archive belongs to another milestone namespace");
            }
        }
    }
    let valid = if archived.exists() {
        crate::db::tasks_store::read_milestone_archive(
            root,
            &archived,
            &item.repository,
            &item.project,
        )?
        .is_some_and(|archive| archive.id == item.id)
    } else {
        false
    };
    let relocated = !settings.handoff_pruning
        && valid
        && store
            .list(None, "all", None)?
            .iter()
            .filter(|task| {
                task.task_id
                    .starts_with(&format!("{}/{}:", item.project, item.id))
            })
            .all(|task| {
                task.milestone_ref == archive_ref
                    && matches!(task.status.as_str(), "completed" | "cancelled")
            });
    drop(store);
    // Determine retry eligibility before validating durable closure.
    if !tombstoned && !relocated {
        return Ok(None);
    }
    if !archived.exists() {
        bail!("HANDOFF_ARCHIVE_INVALID: archived authority is unavailable; recover the archived milestone before retrying handoff");
    }
    let archive_text = fs::read_to_string(&archived)?;
    proof_shas(&archive_text)?;
    proof_shas(&item.text)?;
    if !valid {
        bail!("HANDOFF_ARCHIVE_INVALID: torn archive requires its prepared .forge handoff journal");
    }
    validate_archived_retry(
        root,
        &archive_text,
        active_present.then_some(item.text.as_str()),
        commit,
        evidence,
    )?;
    if active_present {
        fs::remove_file(&item.path)?;
    }
    let backup = archived.with_extension("md.bak");
    if backup.exists() {
        fs::remove_file(backup)?;
    }
    cleanup_handoff_refs(
        root,
        &item.repository,
        &item.project,
        &item.id,
        &archive_text,
        &owners,
    )?;
    Ok(Some(
        json!({"id":item.id,"status":"completed","path":archive_ref}),
    ))
}

/// Archive a completed milestone using immutable task proofs and final content evidence.
pub fn handoff(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    evidence: &IntegrationEvidence,
) -> Result<Value> {
    let fence = crate::db::tasks_store::recovery::lock_database(&tasks::database_path(root)?)?;
    if let Some(result) = retry_handoff(root, selector, commit, evidence, &fence)? {
        return Ok(result);
    }
    let (item, mut store, relevant) = ready_milestone(root, selector)?;
    proof_shas(&item.text)?;
    store.adopt_checkpoint_lock(fence.clone());
    let owners = active_proof_owners(
        root,
        &store,
        &format!("{}/{}/{}", item.repository, item.project, item.id),
    )?;
    let sha = integration::resolve_commit(root, commit.unwrap_or("HEAD"))?;
    let target = integration::resolve_commit(root, &evidence.target_base)?;
    let handoff_profile = store.profile_for_milestone(
        root,
        item.frontmatter
            .get("acdd_profile")
            .and_then(|value| value.as_str()),
        &store.profile,
        &relevant,
    )?;
    let _ = binding(
        root,
        &item,
        &handoff_profile,
        &store,
        &relevant,
        &sha,
        &target,
    )?;
    let identity = format!("{}/{}/{}", item.repository, item.project, item.id);
    crate::core::git::retain_proof_commit(root, &sha)?;
    crate::core::git::retain_proof_commit(root, &target)?;
    let prefix = format!("{}/{}:", item.project, item.id);
    let settings = tasks::TaskSettings::load(root)?;
    store.set_handoff_pruning(settings.handoff_pruning);
    let record_task_commits = settings.records_task_commits();
    let mut landed_receipts = BTreeMap::new();
    let mut handoff_text = item.text.replace("\r\n", "\n");
    for task in relevant.iter().filter(|task| task.status == "completed") {
        let original = task
            .receipt
            .as_ref()
            .context("TASK_RECEIPT_INVALID: completed task has no receipt")?;
        let mut landed = original.clone();
        let profile = store.profile_for_task(task)?;
        let flattened_profile =
            crate::engine::tasks::claim::flattened_profile(&profile, Some(task))?;
        let accepted = accepted_gate_evidence(
            root,
            &store,
            &task.task_id,
            task.contract_revision,
            &task.spec.proof_policy,
            &flattened_profile,
        )?;
        let delivery = flattened_profile
            .gates
            .iter()
            .find(|gate| {
                gate.id == task.stage
                    && gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Delivery)
            })
            .context("delivery gate missing")?;
        landed.commit = if record_task_commits {
            navigation_pointer(
                root,
                &sha,
                accepted[&delivery.id]
                    .commit
                    .as_deref()
                    .context("accepted delivery missing")?,
                &task.spec.scope,
            )?
        } else {
            None
        };
        let (_, task_ref) = task
            .task_id
            .rsplit_once(':')
            .context("invalid task identity")?;
        handoff_text = render_task_receipt(&handoff_text, task_ref, &landed, &task.spec.subtasks)?;
        landed_receipts.insert(task.task_id.clone(), landed);
    }
    // A mutable ref or worktree edit during evidence inspection cannot publish a different tree.
    integration::require_clean_tree(root, &sha, &archive_outputs(root, &item)?)?;
    let completed = Utc::now();
    let started = match item.frontmatter["started_at"].as_str() {
        Some(value) => chrono::DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc),
        None => match store.earliest_claim(&prefix)? {
            Some(timestamp) => Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .context("invalid claim timestamp")?,
            None => completed,
        },
    };
    if completed < started {
        bail!("milestone started_at is in the future");
    }
    let minutes = (completed - started).num_minutes();
    let (passport, mut proofs) = telemetry_passport(
        root,
        &item,
        &store,
        &relevant,
        (&sha, &target),
        evidence,
        (started, completed),
    )?;
    proofs.extend(proof_shas(&item.text)?);
    require_closed_verification(
        &store,
        handoff_profile.as_ref(),
        &identity,
        evidence,
        &binding(
            root,
            &item,
            &handoff_profile,
            &store,
            &relevant,
            &sha,
            &target,
        )?,
        &sha,
    )?;
    let declared_project = archive_project(root, &item, &settings)?;
    let mut meta = item.frontmatter.clone();
    let map = meta
        .as_mapping_mut()
        .context("milestone frontmatter must be a mapping")?;
    map.insert("status".into(), "completed".into());
    map.insert("started_at".into(), started.to_rfc3339().into());
    map.insert("repository".into(), item.repository.clone().into());
    map.insert("project".into(), declared_project.into());
    map.insert("handoff".into(), serde_yaml::to_value(json!({
        "completed_at": completed.to_rfc3339(), "duration": format!("{}h {}m", minutes / 60, minutes % 60),
        "commit": sha, "target_base": target, "integration_evidence": evidence, "proof_shas": proofs, "passport": passport,
        "verification": {"command": evidence.verification.command, "status": "passed", "tests_passed": evidence.verification.tests_passed, "tests_failed": evidence.verification.tests_failed}
    }))?);
    let relative = item.path.strip_prefix(root)?.to_string_lossy().into_owned();
    let parent = item.path.parent().context("milestone parent missing")?;
    let archive_dir = parent.join("archive");
    let archived = archive_dir.join(
        item.path
            .file_name()
            .context("milestone filename missing")?,
    );
    let archive_ref = archived.strip_prefix(root)?.to_string_lossy().into_owned();
    fs::create_dir_all(&archive_dir)?;
    let (_, body) = handoff_text
        .strip_prefix("---\n")
        .context("milestone frontmatter missing")?
        .split_once("\n---\n")
        .context("unterminated milestone frontmatter")?;
    if archived.exists() {
        let mut proof_list: BTreeSet<String> =
            serde_yaml::from_value(meta["handoff"]["proof_shas"].clone())?;
        proof_list.extend(proof_shas(&fs::read_to_string(&archived)?)?);
        meta["handoff"]["proof_shas"] = serde_yaml::to_value(proof_list)?;
    }
    let archive_text = format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?);
    let journal = HandoffJournal {
        binding: evidence.binding.clone(),
        commit: sha.clone(),
        target_base: target.clone(),
        active_ref: relative.clone(),
        archive_ref: archive_ref.clone(),
        active_text: item.text.clone(),
        archive_text: archive_text.clone(),
        tasks: relevant.to_vec(),
        receipts: landed_receipts.clone(),
    };
    let journal_path = journal_path(root, &evidence.binding)?;
    crate::core::fs::atomic_write(&journal_path, &serde_json::to_vec(&journal)?, false)?;
    let mut note = json!({
        "version": 1, "identity": passport["identity"], "binding": evidence.binding,
        "commit": sha, "target_base": target, "started_at": passport["started_at"],
        "completed_at": passport["completed_at"], "duration_seconds": passport["duration_seconds"],
        "tasks_count": relevant.len(),
        "task_statuses": {"completed": relevant.iter().filter(|task| task.status == "completed").count(),
            "cancelled": relevant.iter().filter(|task| task.status == "cancelled").count()},
        "verification": {"exit_code": evidence.verification.exit_code,
            "tests_passed": evidence.verification.tests_passed, "tests_failed": evidence.verification.tests_failed},
        "journal_digest": integration::digest(&serde_json::to_value(&journal)?)?
    });
    note["verified_handoff_proof"] = integration::digest(&note)?.into();
    integration::write_passport(
        root,
        &sha,
        &format!("{}/{}/{}", item.repository, item.project, item.id),
        &note,
    )?;
    crate::core::fs::atomic_replace(&archived, archive_text.as_bytes())?;
    store.handoff_milestone_with_unlink(
        &prefix,
        &relative,
        &archive_ref,
        &relevant,
        &landed_receipts,
        || {
            fs::remove_file(&item.path)?;
            Ok(())
        },
    )?;
    cleanup_handoff_refs(
        root,
        &item.repository,
        &item.project,
        &item.id,
        &archive_text,
        &owners,
    )?;
    fs::remove_file(journal_path)?;
    Ok(
        json!({"id": item.id, "status": "completed", "path": archive_ref, "handoff": meta["handoff"]}),
    )
}
