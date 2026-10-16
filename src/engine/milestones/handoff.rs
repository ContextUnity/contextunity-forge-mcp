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

fn full_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn canonical_git_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .to_owned()
}

fn git_tree_id(root: &Path, commit: &str) -> Result<String> {
    let tree = format!("{commit}^{{tree}}");
    crate::core::git::git_stdout(root, &["rev-parse", &tree], "TASK_COMMIT_INVALID")
}

fn validate_delivery_pointer(
    root: &Path,
    pointer: &str,
    accepted_delivery: &str,
    candidate_baseline: &str,
    scope: &[String],
    milestone_ref: &str,
) -> Result<()> {
    if !full_git_sha(pointer) {
        bail!("TASK_RECEIPT_INVALID: task receipt contains invalid Git commit '{pointer}'");
    }
    if !crate::core::git::git_commit_exists(root, pointer)? {
        bail!("TASK_COMMIT_NOT_FOUND: commit '{pointer}' is not available in workspace Git repository");
    }
    if pointer == accepted_delivery {
        return Ok(());
    }
    if pointer == candidate_baseline {
        bail!("TASK_COMMIT_OBSOLETE: commit '{pointer}' does not advance beyond candidate baseline '{candidate_baseline}'");
    }
    if !git_ancestor(root, candidate_baseline, pointer)? {
        bail!("TASK_COMMIT_UNRELATED: commit '{pointer}' does not descend from candidate baseline '{candidate_baseline}'");
    }

    if !git_changed(root, accepted_delivery, pointer, scope)?
        && git_tree_id(root, accepted_delivery)? == git_tree_id(root, pointer)?
    {
        // A content-identical repack is valid even for receipt-only deliveries
        // that make no change inside the task's declared scope.
        return Ok(());
    }

    crate::core::tasks::validate_commit_for_task(
        root,
        pointer,
        scope,
        Some(milestone_ref),
        Some(accepted_delivery),
        Some(candidate_baseline),
    )
}

fn accepted_gate_evidence(
    root: &Path,
    store: &TasksStore,
    task_id: &str,
    contract_revision: u64,
    proof_policy: &str,
    profile: &crate::core::tasks::profile::GateProfile,
) -> Result<BTreeMap<String, crate::core::tasks::gates::Evidence>> {
    let details = store.inspect_details(task_id)?;
    let attempts = details["attempts"]
        .as_array()
        .context("TASK_RECEIPT_INVALID: task claim history is malformed")?;
    let rows = details["gates"]
        .as_array()
        .context("TASK_RECEIPT_INVALID: task gate history is malformed")?;
    let mut accepted = BTreeMap::new();
    for gate in &profile.gates {
        let row = rows
            .iter()
            .rev()
            .find(|row| row["stage"] == gate.id)
            .with_context(|| {
                format!(
                    "TASK_RECEIPT_INVALID: task '{task_id}' has no accepted '{}' gate evidence",
                    gate.id
                )
            })?;
        if row["state"] != "passed" {
            bail!("TASK_RECEIPT_INVALID: latest '{}' gate evidence is not accepted", gate.id);
        }
        let revision = row["claim_revision"]
            .as_u64()
            .context("TASK_RECEIPT_INVALID: gate claim revision is malformed")?;
        let serialized = row["evidence"]
            .as_str()
            .context("TASK_RECEIPT_INVALID: accepted gate evidence is missing")?;
        let evidence: crate::core::tasks::gates::Evidence =
            serde_json::from_str(serialized).context("TASK_RECEIPT_INVALID: accepted gate evidence is malformed")?;
        let claims = attempts
            .iter()
            .filter(|attempt| attempt["claim_revision"].as_u64() == Some(revision))
            .collect::<Vec<_>>();
        let [claim] = claims.as_slice() else {
            bail!("TASK_RECEIPT_INVALID: accepted '{}' gate has no unique persisted claim", gate.id);
        };
        let claim_worker = claim["worker_id"]
            .as_str()
            .context("TASK_RECEIPT_INVALID: persisted claim worker is malformed")?;
        let claim_worktree = claim["worktree"]
            .as_str()
            .context("TASK_RECEIPT_INVALID: persisted claim worktree is malformed")?;
        if claim["ended"].as_bool() != Some(true) {
            bail!("TASK_RECEIPT_INVALID: accepted '{}' gate claim is not closed", gate.id);
        }
        if evidence.task_id != task_id
            || evidence.stage != gate.id
            || evidence.claim_revision != revision
            || evidence.contract_revision != contract_revision
            || evidence.worker_id.trim().is_empty()
            || evidence.worktree.trim().is_empty()
            || evidence.worker_id != claim_worker
            || evidence.worktree != claim_worktree
        {
            bail!("TASK_RECEIPT_INVALID: accepted '{}' gate provenance does not match its stored claim", gate.id);
        }
        crate::core::tasks::gates::validate_gate_proof(
            &evidence.proof,
            gate,
            profile,
            true,
            proof_policy,
        )
        .with_context(|| format!("TASK_RECEIPT_INVALID: accepted '{}' gate proof is invalid", gate.id))?;
        if let Some(commit) = evidence.commit.as_deref() {
            if !full_git_sha(commit) || !crate::core::git::git_commit_exists(root, commit)? {
                bail!("TASK_RECEIPT_INVALID: accepted '{}' gate commit is malformed or unavailable", gate.id);
            }
        }
        if gate.sha_snapshot {
            let baseline = evidence
                .candidate_baseline_head
                .as_deref()
                .context("TASK_RECEIPT_INVALID: candidate baseline is missing")?;
            if !full_git_sha(baseline) {
                bail!("TASK_RECEIPT_INVALID: candidate baseline is malformed");
            }
            if !crate::core::git::git_commit_exists(root, baseline)? {
                bail!("TASK_RECEIPT_INVALID: candidate baseline is unavailable");
            }
        }
        accepted.insert(gate.id.clone(), evidence);
    }
    for gate in &profile.gates {
        if let Some(prior) = gate.independent_from.as_deref() {
            let current = accepted
                .get(&gate.id)
                .context("TASK_RECEIPT_INVALID: accepted independent gate evidence is missing")?;
            let prior = accepted
                .get(prior)
                .context("TASK_RECEIPT_INVALID: configured independent gate evidence is missing")?;
            if current.worker_id == prior.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT: accepted '{}' worker matches configured prior gate '{}'; each gate must retain its configured independent worker", gate.id, gate.independent_from.as_deref().unwrap_or_default());
            }
        }
    }
    Ok(accepted)
}

struct TaskProvenance<'a> {
    task_id: &'a str,
    contract_revision: u64,
    proof_policy: &'a str,
    profile: &'a crate::core::tasks::profile::GateProfile,
    receipt: &'a crate::core::tasks::Receipt,
    milestone_path: &'a str,
    scope: &'a [String],
}

fn validate_task_provenance(
    root: &Path,
    store: &TasksStore,
    task: TaskProvenance<'_>,
) -> Result<(String, String)> {
    let evidence = accepted_gate_evidence(
        root,
        store,
        task.task_id,
        task.contract_revision,
        task.proof_policy,
        task.profile,
    )?;
    let delivery_index = task.profile
        .gates
        .iter()
        .position(|gate| gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Delivery))
        .context("TASK_RECEIPT_INVALID: profile has no delivery gate")?;
    let snapshot_gate = task.profile.gates[..delivery_index]
        .iter()
        .rev()
        .find(|gate| gate.sha_snapshot)
        .context("TASK_RECEIPT_INVALID: profile has no candidate snapshot gate")?;
    let snapshot = evidence
        .get(&snapshot_gate.id)
        .context("TASK_RECEIPT_INVALID: accepted candidate snapshot is missing")?;
    let snapshot_commit = snapshot
        .commit
        .as_deref()
        .context("TASK_RECEIPT_INVALID: accepted candidate commit is missing")?;
    let baseline = snapshot
        .candidate_baseline_head
        .as_deref()
        .context("TASK_RECEIPT_INVALID: accepted candidate baseline is missing")?;
    if task.receipt.evidence != snapshot.proof {
        bail!("TASK_RECEIPT_INVALID: durable candidate proof does not match accepted build evidence");
    }

    let delivery_gate = &task.profile.gates[delivery_index];
    if delivery_gate.review_sources.is_empty() {
        bail!("TASK_RECEIPT_INVALID: delivery profile has no review sources");
    }
    for source in &delivery_gate.review_sources {
        let review = evidence
            .get(source)
            .context("TASK_RECEIPT_INVALID: accepted review gate evidence is missing")?;
        if review.commit.as_deref() != Some(snapshot_commit) {
            bail!("TASK_RECEIPT_INVALID: accepted review '{source}' does not match the candidate snapshot");
        }
        if review.candidate_baseline_head.as_deref() != Some(baseline) {
            bail!("TASK_RECEIPT_INVALID: accepted review '{source}' does not match the candidate baseline");
        }
    }
    if let Some(sources) = task.receipt.review.get("review_sources").and_then(Value::as_object) {
        if sources.len() != delivery_gate.review_sources.len() {
            bail!("TASK_RECEIPT_INVALID: durable review rollup does not match accepted review evidence");
        }
        for source in &delivery_gate.review_sources {
            let accepted_review = evidence
                .get(source)
                .context("TASK_RECEIPT_INVALID: accepted review gate evidence is missing")?;
            if sources.get(source) != Some(&accepted_review.proof) {
                bail!("TASK_RECEIPT_INVALID: durable review source '{source}' does not match accepted gate evidence");
            }
        }
    } else if delivery_gate.review_sources.len() == 1 {
        let source = &delivery_gate.review_sources[0];
        if evidence.get(source).map(|value| &value.proof) != Some(&task.receipt.review) {
            bail!("TASK_RECEIPT_INVALID: durable review proof does not match accepted gate evidence");
        }
    } else {
        bail!("TASK_RECEIPT_INVALID: multiple accepted review sources require a source-keyed receipt");
    }

    let delivered = evidence
        .get(&delivery_gate.id)
        .context("TASK_RECEIPT_INVALID: accepted delivery evidence is missing")?;
    let delivery_commit = delivered
        .commit
        .as_deref()
        .context("TASK_RECEIPT_INVALID: accepted delivery commit is missing")?;
    if delivered.candidate_baseline_head.as_deref() != Some(baseline) {
        bail!("TASK_RECEIPT_INVALID: accepted delivery does not match the candidate baseline");
    }
    let receipt_path = canonical_git_path(task.milestone_path);
    let differing = crate::core::git::git_scoped_differing_files(
        root,
        snapshot_commit,
        delivery_commit,
        task.scope,
    )?;
    let unreviewed = differing
        .into_iter()
        .filter(|path| canonical_git_path(path) != receipt_path)
        .collect::<Vec<_>>();
    if !unreviewed.is_empty() {
        bail!(
            "TASK_RECEIPT_INVALID: accepted delivery differs from reviewed candidate in task scope: {}",
            unreviewed.join(", ")
        );
    }
    Ok((delivery_commit.to_owned(), baseline.to_owned()))
}

struct LandedTask {
    task_ref: String,
    delivery: String,
    landing_index: usize,
    scope: Vec<String>,
}

fn task_landed_commit(
    root: &Path,
    commits: &[String],
    delivery: &str,
    candidate_baseline_head: &str,
    scope: &[String],
    task_ref: &str,
) -> Result<(String, usize)> {
    let baseline_index = commits
        .iter()
        .position(|commit| commit == candidate_baseline_head)
        .with_context(|| {
            format!("TASK_LANDED_COMMIT_NOT_FOUND: task '{task_ref}' candidate baseline is not on the milestone first-parent history")
        })?;
    let delivery_tree = git_tree_id(root, delivery)?;
    for index in baseline_index + 1..commits.len() {
        let commit = &commits[index];
        if git_changed(root, delivery, commit, scope)? {
            continue;
        }
        let previous = &commits[index - 1];
        if git_changed(root, previous, commit, scope)? {
            return Ok((commit.clone(), index));
        }

        // Receipt-only deliveries can have an unchanged scoped tree. Accept the
        // first first-parent merge that introduces the accepted delivery object,
        // or its exact-tree repack, after the recorded baseline.
        let contains_delivery = git_ancestor(root, delivery, commit)?;
        let previous_contains_delivery = git_ancestor(root, delivery, previous)?;
        if (contains_delivery && !previous_contains_delivery)
            || git_tree_id(root, commit)? == delivery_tree
        {
            return Ok((commit.clone(), index));
        }
    }

    let differing = crate::core::git::git_scoped_differing_files(
        root,
        delivery,
        commits.last().context("milestone first-parent history is empty")?,
        scope,
    )?;
    let detail = if differing.is_empty() {
        String::new()
    } else {
        format!("\nDiffering paths in task scope:\n  - {}", differing.join("\n  - "))
    };
    let restore_cmd = if differing.is_empty() {
        format!("git checkout {delivery} -- <paths>")
    } else {
        format!("git checkout {delivery} -- {}", differing.join(" "))
    };
    bail!(
        "TASK_LANDED_COMMIT_NOT_FOUND: task '{task_ref}'. No first-parent commit after baseline '{candidate_baseline_head}' lands the accepted delivery tree '{delivery}'.{detail}\n\
         Do: merge a scope-equivalent task delivery after the accepted baseline.\n\
         To restore scoped files from delivery, run: {restore_cmd}"
    )
}

fn validate_later_scope_changes(root: &Path, handoff: &str, tasks: &[LandedTask]) -> Result<()> {
    for earlier in tasks {
        let differing = crate::core::git::git_scoped_differing_files(
            root,
            &earlier.delivery,
            handoff,
            &earlier.scope,
        )?;
        for path in differing {
            let mut explained = false;
            for later in tasks.iter().filter(|later| {
                later.landing_index > earlier.landing_index
                    && later.scope.iter().any(|scope| {
                        crate::core::tasks::path_matches_scope_entry(&path, scope)
                    })
            }) {
                if !git_changed(root, &later.delivery, handoff, std::slice::from_ref(&path))? {
                    explained = true;
                    break;
                }
            }
            if !explained {
                bail!(
                    "TASK_LANDED_COMMIT_NOT_FOUND: task '{}' path '{}' differs from its accepted delivery, and no later reviewed delivery explains the final tree.\n\
                     Differing paths in task scope:\n  - {}\n\
                     Do: merge a scope-equivalent reviewed delivery for task '{}'.\n\
                     To restore the scoped file from the accepted delivery, run: git checkout {} -- {}",
                    earlier.task_ref,
                    path,
                    path,
                    earlier.task_ref,
                    earlier.delivery,
                    path,
                );
            }
        }
    }
    Ok(())
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
    let prefix = format!("{}/{}:", item.project, item.id);
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
    let settings = crate::engine::tasks::TaskSettings::load(root).ok();
    let record_task_commits = settings
        .as_ref()
        .map(|s| s.records_task_commits())
        .unwrap_or(true);
    let mut original_receipts = BTreeMap::new();
    let mut landed_receipts = BTreeMap::new();
    let history_output = crate::core::git::git_command(
        root,
        &["rev-list", "--first-parent", "--reverse", &sha],
    )?;
    if !history_output.status.success() {
        bail!("unable to inspect milestone first-parent history");
    }
    let first_parent = String::from_utf8(history_output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let milestone_rel = item
        .path
        .strip_prefix(root)
        .unwrap_or(&item.path)
        .to_string_lossy()
        .into_owned();
    let mut landed_tasks = Vec::new();
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
        if let Some(md_sha) = markdown_commit {
            if !full_git_sha(md_sha) {
                bail!("TASK_RECEIPT_INVALID: milestone task receipt contains invalid Git commit '{md_sha}'");
            }
            if !crate::core::git::git_commit_exists(root, md_sha)? {
                bail!("TASK_COMMIT_NOT_FOUND: commit '{md_sha}' is not available in workspace Git repository");
            }
        }
        let (accepted_delivery, candidate_baseline) = validate_task_provenance(
            root,
            &store,
            TaskProvenance {
                task_id: &task.task_id,
                contract_revision: task.contract_revision,
                proof_policy: &task.spec.proof_policy,
                profile: &profile,
                receipt: original,
                milestone_path: &milestone_rel,
                scope: &task.spec.scope,
            },
        )?;
        let durable_delivery = original
            .commit
            .as_deref()
            .context("TASK_RECEIPT_INVALID: completed task receipt has no delivery commit")?;
        validate_delivery_pointer(
            root,
            durable_delivery,
            &accepted_delivery,
            &candidate_baseline,
            &task.spec.scope,
            &milestone_rel,
        )?;
        let mut landed = original.clone();
        let effective_delivery = match markdown_commit {
            Some(md_sha) => {
                if original.commit.as_deref() != Some(md_sha)
                    && !git_ancestor(root, md_sha, &sha)?
                {
                    bail!(
                        "TASK_DELIVERY_COMMIT_INVALID: task '{task_ref}' receipt commit '{md_sha}' is not an ancestor of milestone handoff commit '{sha}'.\n\
                         Do: merge '{md_sha}' into '{sha}', or update the milestone markdown receipt to the repacked commit SHA."
                    );
                }
                validate_delivery_pointer(
                    root,
                    md_sha,
                    &accepted_delivery,
                    &candidate_baseline,
                    &task.spec.scope,
                    &milestone_rel,
                )
                .map_err(|e| anyhow::anyhow!("TASK_DELIVERY_COMMIT_INVALID: {e}"))?;
                md_sha
            }
            None => durable_delivery,
        };
        let (landed_sha, landing_index) = task_landed_commit(
            root,
            &first_parent,
            effective_delivery,
            &candidate_baseline,
            &task.spec.scope,
            task_ref,
        )?;
        landed_tasks.push(LandedTask {
            task_ref: task_ref.to_owned(),
            delivery: accepted_delivery,
            landing_index,
            scope: task.spec.scope.clone(),
        });
        landed.commit = Some(landed_sha.clone());
        let mut doc_receipt = landed.clone();
        if !record_task_commits {
            doc_receipt.commit = None;
        }
        handoff_text =
            render_task_receipt(&handoff_text, task_ref, &doc_receipt, &task.spec.subtasks)?;
        original_receipts.insert(task.task_id.clone(), original.clone());
        landed_receipts.insert(task.task_id.clone(), landed);
    }
    validate_later_scope_changes(root, &sha, &landed_tasks)?;
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

#[cfg(test)]
#[test]
fn handoff_accepts_completed_reviewed_shared_scope() {
    // The integration suite owns the full permutation table. This native adapter
    // also gives the static seam index a direct edge to the public handoff owner.
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture = Cleanup(std::env::temp_dir().join(format!(
        "forge-native-handoff-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    )));
    let root = &fixture.0;
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/milestones")).unwrap();
    let git = |args: &[&str]| {
        let output = crate::core::git::git_command(root, args).unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.name", "ACDD test"]);
    git(&["config", "user.email", "acdd-test@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    fs::write(
        root.join("forge-mcp.yaml"),
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    )
    .unwrap();
    let source = "---\nid: m-native-shared\ntitle: Reviewed shared handoff\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Reviewed shared handoff\n```yaml\ntask_ref: first\ntarget: First version\nproof_policy: seam-test-first\nscope: [src/shared.rs]\n```\n```yaml\ntask_ref: second\ntarget: Reviewed next version\nproof_policy: seam-test-first\nscope: [src/shared.rs]\ndepends_on: [first]\n```\n";
    let active = "docs/milestones/010-native-shared.md";
    let archived = "docs/milestones/archive/010-native-shared.md";
    fs::write(root.join(active), source).unwrap();
    fs::write(root.join("src/shared.rs"), "pub fn shared() -> u8 { 0 }\n").unwrap();
    git(&["add", "forge-mcp.yaml", "docs", "src"]);
    git(&["commit", "-m", "admit native shared handoff"]);
    let milestone = crate::core::tasks::Milestone::parse(source, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, active, root).unwrap();
    let mut landings = Vec::new();
    for (index, spec) in milestone.tasks.iter().enumerate() {
        let id = milestone.task_id(spec);
        fs::write(
            root.join("src/shared.rs"),
            format!("pub fn shared() -> u8 {{ {} }}\n", index + 1),
        )
        .unwrap();
        for (stage, worker) in [
            ("contract", "contract-author"),
            ("build", "builder"),
            ("review", "independent-reviewer"),
            ("deliver", "delivery-reviewer"),
        ] {
            let claim = tasks::claim(
                root,
                tasks::Claim {
                    task_id: id.clone(),
                    stage: stage.into(),
                    worker_id: worker.into(),
                    worktree: root.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut evidence = claim["workflow_guidance"]["expected_evidence_template"].clone();
            evidence["proof"] = match stage {
                "contract" => {
                    json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
                }
                "build" => {
                    json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}})
                }
                "review" => {
                    let contours: serde_json::Map<String, Value> = crate::core::tasks::gates::REVIEW_CONTOURS.iter().map(|name| (
                            (*name).into(), json!({"applicable":true,"evidence":"real Git and SQLite handoff seam"})
                        )).collect();
                    json!({"review_proof":{"decision":"pass","contours":contours}})
                }
                _ => json!({"delivery_proof":{}}),
            };
            tasks::submit(
                root,
                serde_json::from_value(json!({
                    "task_id":id, "stage":stage, "action":"pass", "evidence":evidence
                }))
                .unwrap(),
            )
            .unwrap();
        }
        landings.push((id, git(&["rev-parse", "HEAD"])));
    }
    let before = fs::read(root.join(active)).unwrap();
    let head = git(&["rev-parse", "HEAD"]);
    drop(store);
    let result = handoff(
        root,
        "m-native-shared",
        Some(&head),
        "cargo test --test acdd",
        1,
        0,
    );
    if result.is_err() {
        assert_eq!(fs::read(root.join(active)).unwrap(), before);
        assert!(!root.join(archived).exists());
    }
    result.expect("a later completed reviewed task may change an earlier task's shared path");
    let reopened = TasksStore::open(&root.join(".forge/tasks.sqlite")).unwrap();
    let archived_doc = fs::read_to_string(root.join(archived)).unwrap();
    let parsed = crate::core::tasks::Milestone::parse(&archived_doc, "forge-mcp").unwrap();
    for (index, (id, landed)) in landings.iter().enumerate() {
        let task = reopened.inspect(id).unwrap();
        assert_eq!(task.milestone_ref, archived);
        assert_eq!(
            task.receipt.unwrap().commit.as_deref(),
            Some(landed.as_str())
        );
        assert_eq!(
            parsed.tasks[index]
                .receipt
                .as_ref()
                .unwrap()
                .commit
                .as_deref(),
            Some(landed.as_str())
        );
    }
    assert!(!root.join(active).exists());
}
