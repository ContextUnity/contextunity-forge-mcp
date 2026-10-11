use super::{render_task_receipt, snapshots};
use crate::db::tasks_store::TasksStore;
use crate::engine::tasks;
use anyhow::{bail, Context, Result};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

fn git_ancestor(root: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    crate::core::git::git_ancestor(root, ancestor, descendant)
}

fn git_changed(root: &Path, before: &str, after: &str, scope: &[String]) -> Result<bool> {
    crate::core::git::git_scoped_tree_changed(root, before, after, scope)
}

fn task_landed_commit(
    root: &Path,
    handoff: &str,
    delivery: &str,
    candidate_baseline_head: Option<&str>,
    scope: &[String],
) -> Result<String> {
    let history =
        crate::core::git::git_command(root, &["rev-list", "--first-parent", "--reverse", handoff])?;
    if !history.status.success() {
        bail!("unable to inspect milestone first-parent history");
    }
    let commits = String::from_utf8(history.stdout)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for commit in &commits {
        if git_ancestor(root, delivery, commit)? {
            return Ok(commit.clone());
        }
    }

    // With auto_commit disabled, the accepted candidate snapshot is a root
    // snapshot rather than an ancestor of the user's landing commit. Resolve
    // its first scoped-tree match after the candidate's recorded baseline.
    if let Some(baseline) = candidate_baseline_head {
        for commit in &commits {
            if commit == baseline || !git_ancestor(root, baseline, commit)? {
                continue;
            }
            if !git_changed(root, baseline, commit, scope)?
                || git_changed(root, delivery, commit, scope)?
            {
                continue;
            }
            let parent_line = crate::core::git::git_command(
                root,
                &["rev-list", "--parents", "-n", "1", commit],
            )?;
            let parent_fields = String::from_utf8(parent_line.stdout)?
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if parent_fields.len() >= 2 && git_changed(root, &parent_fields[1], commit, scope)?
            {
                return Ok(commit.clone());
            }
        }
    }
    bail!("TASK_LANDED_COMMIT_NOT_FOUND: no task-specific landing commit contains the accepted delivery")
}

/// Verifies completion, writes a handoff receipt, and archives the milestone.
pub fn handoff(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    command: &str,
    passed: u64,
    failed: u64,
) -> Result<Value> {
    if command.trim().is_empty() || failed != 0 {
        bail!("handoff requires a passed verification command and zero failed tests");
    }
    let mut found = snapshots(root, false, Some(selector))?;
    if found.len() != 1 {
        bail!("milestone selector must resolve to exactly one active document: {selector}");
    }
    let item = found.pop().context("milestone missing")?;
    if !matches!(item.status.as_str(), "planned" | "active") {
        bail!("milestone is not ready for handoff");
    }
    let db = tasks::database_path(root)?;
    let mut store = TasksStore::open_project(&db, &item.repository, &item.project)?;
    store.set_profile(crate::core::tasks::profile::load_for_workspace(root)?);
    store.set_workspace_root(root);
    let stored = store.list(None, "all", None)?;
    let prefix = format!("{}/{}/{}:", item.repository, item.project, item.id);
    let relevant: Vec<_> = stored
        .iter()
        .filter(|task| task.task_id.starts_with(&prefix))
        .collect();
    let mut incomplete = Vec::new();
    for spec in &item.tasks {
        let task_ref = spec["task_ref"].as_str().context("task_ref missing")?;
        let id = format!("{prefix}{task_ref}");
        if !relevant
            .iter()
            .any(|task| task.task_id == id && task.status == "completed")
        {
            incomplete.push(task_ref.to_string());
        }
    }
    for task in &relevant {
        if task.status != "completed"
            && !incomplete
                .iter()
                .any(|name| task.task_id.ends_with(&format!(":{name}")))
        {
            incomplete.push(task.task_id.clone());
        }
    }
    if !incomplete.is_empty() {
        bail!("milestone has incomplete tasks: {}", incomplete.join(", "));
    }
    if relevant.is_empty() {
        bail!("milestone tasks are not synced to SQLite");
    }
    let expected_ids: Vec<String> = relevant.iter().map(|task| task.task_id.clone()).collect();
    let sha = match commit {
        Some(value) => value.to_string(),
        None => crate::core::git::git_stdout(
            root,
            &["rev-parse", "HEAD"],
            "unable to resolve milestone commit",
        )?,
    };
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("handoff commit must be a full Git SHA");
    }
    let commit_object = format!("{sha}^{{commit}}");
    let verified_commit = crate::core::git::git_status(root, &["cat-file", "-e", &commit_object])?;
    if !verified_commit.success() {
        bail!("handoff commit is not available in the workspace Git object database");
    }
    let mut original_receipts = BTreeMap::new();
    let mut landed_receipts = BTreeMap::new();
    let mut handoff_text = item.text.replace("\r\n", "\n");
    for task in &relevant {
        let original = task
            .receipt
            .as_ref()
            .context("TASK_RECEIPT_INVALID: completed task has no durable receipt")?;
        let profile = store.profile_for_task(task)?;
        crate::core::tasks::gates::validate_durable_receipt_with_profile(
            original,
            task.contract_revision,
            &profile,
        )?;
        let mut landed = original.clone();
        let (_, task_ref) = task
            .task_id
            .rsplit_once(':')
            .context("invalid task identity")?;
        let markdown_commit = item
            .tasks
            .iter()
            .find(|spec| spec["task_ref"].as_str() == Some(task_ref))
            .and_then(|spec| spec.get("receipt"))
            .and_then(|receipt| receipt.get("commit"))
            .and_then(|commit| commit.as_str());
        let candidate_baseline = store
            .build_snapshot_candidate(&task.task_id)?
            .map(|(_, baseline)| baseline);
        let effective_delivery = match markdown_commit {
            Some(md_sha) => {
                if !matches!(md_sha.len(), 40 | 64) || !md_sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    bail!("TASK_RECEIPT_INVALID: milestone task receipt contains invalid Git commit '{md_sha}'");
                }
                if !crate::core::git::git_commit_exists(root, md_sha)? {
                    bail!("TASK_RECEIPT_INVALID: task receipt commit '{md_sha}' is not available in workspace Git repository");
                }
                if original.commit.as_deref() != Some(md_sha) {
                    if !git_ancestor(root, md_sha, &sha)? {
                        bail!("TASK_DELIVERY_COMMIT_INVALID: task receipt commit '{md_sha}' is not an ancestor of milestone handoff commit '{sha}'");
                    }
                    let milestone_rel = item.path.strip_prefix(root).unwrap_or(&item.path).to_string_lossy();
                    crate::core::tasks::validate_commit_for_task(
                        root,
                        md_sha,
                        &task.spec.scope,
                        Some(&milestone_rel),
                        original.commit.as_deref(),
                        candidate_baseline.as_deref(),
                    )
                    .map_err(|e| anyhow::anyhow!("TASK_DELIVERY_COMMIT_INVALID: {e}"))?;
                }
                md_sha
            }
            None => {
                original
                    .commit
                    .as_deref()
                    .context("TASK_RECEIPT_INVALID: completed task receipt has no delivery commit")?
            }
        };
        let landed_sha = task_landed_commit(
            root,
            &sha,
            effective_delivery,
            candidate_baseline.as_deref(),
            &task.spec.scope,
        )?;
        landed.commit = Some(landed_sha);
        handoff_text = render_task_receipt(&handoff_text, task_ref, &landed, &task.spec.subtasks)?;
        original_receipts.insert(task.task_id.clone(), original.clone());
        landed_receipts.insert(task.task_id.clone(), landed);
    }
    let started = match item.frontmatter["started_at"].as_str() {
        Some(value) => chrono::DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc),
        None => Utc
            .timestamp_opt(
                store
                    .earliest_claim(&prefix)?
                    .context("milestone activation time missing")?,
                0,
            )
            .single()
            .context("invalid claim timestamp")?,
    };
    let completed = Utc::now();
    if completed < started {
        bail!("milestone started_at is in the future");
    }
    let minutes = (completed - started).num_minutes();
    let mut meta = item.frontmatter;
    let map = meta
        .as_mapping_mut()
        .context("milestone frontmatter must be a mapping")?;
    map.insert("status".into(), "completed".into());
    map.insert("started_at".into(), started.to_rfc3339().into());
    map.insert("handoff".into(), serde_yaml::to_value(json!({
        "completed_at": completed.to_rfc3339(), "duration": format!("{}h {}m", minutes / 60, minutes % 60),
        "commit": sha, "verification": {"command": command, "status": "passed", "tests_passed": passed, "tests_failed": failed}
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
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archived)?;
    let archive_text = format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?);
    if let Err(error) = file.write_all(archive_text.as_bytes()) {
        drop(file);
        fs::remove_file(&archived)?;
        return Err(error.into());
    }
    drop(file);
    if let Err(error) = store.handoff_milestone(
        &prefix,
        &relative,
        &archive_ref,
        &expected_ids,
        &landed_receipts,
    ) {
        fs::remove_file(&archived)?;
        return Err(error);
    }
    if let Err(error) = fs::remove_file(&item.path) {
        store.handoff_milestone(
            &prefix,
            &archive_ref,
            &relative,
            &expected_ids,
            &original_receipts,
        )?;
        fs::remove_file(&archived)?;
        return Err(error.into());
    }
    prune_snapshot_refs(root, &prefix);
    Ok(
        json!({"id": item.id, "status": "completed", "path": archive_ref, "handoff": meta["handoff"]}),
    )
}

fn prune_snapshot_refs(root: &Path, prefix: &str) {
    let clean = prefix
        .strip_suffix(':')
        .unwrap_or(prefix)
        .replace(['\\', ' '], "_");
    let target_prefix = format!("refs/forge/snapshots/{clean}/");
    if let Ok(output) = crate::core::git::git_command(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ],
    ) {
        let refs = String::from_utf8_lossy(&output.stdout);
        for ref_line in refs.lines() {
            let r = ref_line.trim();
            if r.starts_with(&target_prefix) {
                let _ = crate::core::git::git_status(root, &["update-ref", "-d", r]);
            }
        }
    }
}
