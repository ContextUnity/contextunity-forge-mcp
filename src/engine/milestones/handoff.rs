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
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

fn full_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn canonical_git_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
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
            bail!(
                "TASK_RECEIPT_INVALID: latest '{}' gate evidence is not accepted",
                gate.id
            );
        }
        let revision = row["claim_revision"]
            .as_u64()
            .context("TASK_RECEIPT_INVALID: gate claim revision is malformed")?;
        let serialized = row["evidence"]
            .as_str()
            .context("TASK_RECEIPT_INVALID: accepted gate evidence is missing")?;
        let evidence: crate::core::tasks::gates::Evidence = serde_json::from_str(serialized)
            .context("TASK_RECEIPT_INVALID: accepted gate evidence is malformed")?;
        let claims = attempts
            .iter()
            .filter(|attempt| attempt["claim_revision"].as_u64() == Some(revision))
            .collect::<Vec<_>>();
        let [claim] = claims.as_slice() else {
            bail!(
                "TASK_RECEIPT_INVALID: accepted '{}' gate has no unique persisted claim",
                gate.id
            );
        };
        let claim_worker = claim["worker_id"]
            .as_str()
            .context("TASK_RECEIPT_INVALID: persisted claim worker is malformed")?;
        let claim_worktree = claim["worktree"]
            .as_str()
            .context("TASK_RECEIPT_INVALID: persisted claim worktree is malformed")?;
        if claim["ended"].as_bool() != Some(true) {
            bail!(
                "TASK_RECEIPT_INVALID: accepted '{}' gate claim is not closed",
                gate.id
            );
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
        .with_context(|| {
            format!(
                "TASK_RECEIPT_INVALID: accepted '{}' gate proof is invalid",
                gate.id
            )
        })?;
        if let Some(commit) = evidence.commit.as_deref() {
            if !full_git_sha(commit) || !crate::core::git::git_commit_exists(root, commit)? {
                bail!(
                    "TASK_RECEIPT_INVALID: accepted '{}' gate commit is malformed or unavailable",
                    gate.id
                );
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
) -> Result<BTreeMap<String, crate::core::tasks::gates::Evidence>> {
    let evidence = accepted_gate_evidence(
        root,
        store,
        task.task_id,
        task.contract_revision,
        task.proof_policy,
        task.profile,
    )?;
    let delivery_index = task
        .profile
        .gates
        .iter()
        .position(|gate| {
            gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Delivery)
        })
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
        bail!(
            "TASK_RECEIPT_INVALID: durable candidate proof does not match accepted build evidence"
        );
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
    if let Some(sources) = task
        .receipt
        .review
        .get("review_sources")
        .and_then(Value::as_object)
    {
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
            bail!(
                "TASK_RECEIPT_INVALID: durable review proof does not match accepted gate evidence"
            );
        }
    } else {
        bail!(
            "TASK_RECEIPT_INVALID: multiple accepted review sources require a source-keyed receipt"
        );
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
    Ok(evidence)
}

fn ready_milestone(root: &Path, selector: &str) -> Result<(Snapshot, TasksStore, Vec<Task>)> {
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
        .into_iter()
        .filter(|task| task.task_id.starts_with(&prefix))
        .collect();
    let mut incomplete = Vec::new();
    for spec in &item.tasks {
        let task_ref = spec["task_ref"].as_str().context("task_ref missing")?;
        let id = format!("{prefix}{task_ref}");
        if !relevant.iter().any(|task| {
            task.task_id == id
                && matches!(task.status.as_str(), "completed" | "cancelled")
                && spec["status"].as_str() == Some(task.status.as_str())
        }) {
            incomplete.push(task_ref.to_string());
        }
    }
    for task in &relevant {
        if !matches!(task.status.as_str(), "completed" | "cancelled")
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
    Ok((item, store, relevant))
}

fn validate_effective_scope(
    store: &TasksStore,
    task: &Task,
    original: &crate::core::tasks::TaskSpec,
) -> Result<()> {
    let invalid = || {
        anyhow::anyhow!(
            "TASK_AUTHORITY_CHANGED: task '{}' scope differs from its admitted extension ledger",
            task.task_id
        )
    };
    let normalize = |value: &str| -> Result<String> {
        crate::core::tasks::relative_path(value)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|_| invalid())
    };

    let original_scope = original
        .scope
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<BTreeSet<_>>>()?;
    if !task.initial_scope.is_empty()
        && task
            .initial_scope
            .iter()
            .map(|path| normalize(path))
            .collect::<Result<BTreeSet<_>>>()?
            != original_scope
    {
        return Err(invalid());
    }

    let rows = {
        let mut statement = store.connection.prepare(
            "SELECT path,base_root,frozen FROM task_scope_paths WHERE task_id=?1 ORDER BY rowid",
        )?;
        let rows = statement
            .query_map([&task.task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut frozen_paths = BTreeSet::new();
    let mut frozen_bases = BTreeSet::new();
    let mut extensions = Vec::new();
    for (stored_path, stored_base, frozen) in rows {
        let path = normalize(&stored_path)?;
        let base = normalize(&stored_base)?;
        if path != stored_path
            || base != stored_base
            || !Path::new(&path).starts_with(Path::new(&base))
        {
            return Err(invalid());
        }
        match frozen {
            1 => {
                if !frozen_paths.insert(path) {
                    return Err(invalid());
                }
                frozen_bases.insert(base);
            }
            0 => extensions.push((path, base)),
            _ => return Err(invalid()),
        }
    }
    if frozen_paths != original_scope {
        return Err(invalid());
    }

    let allowed_bases = if original.scope_roots.is_empty() {
        frozen_bases
    } else {
        original
            .scope_roots
            .iter()
            .map(|path| normalize(path))
            .collect::<Result<BTreeSet<_>>>()?
    };
    let mut admitted_scope = frozen_paths;
    for (path, base) in extensions {
        if !allowed_bases.contains(&base)
            || !Path::new(&path).starts_with(Path::new(&base))
            || !admitted_scope.insert(path)
        {
            return Err(invalid());
        }
    }
    let effective_scope = task
        .spec
        .scope
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<BTreeSet<_>>>()?;
    if effective_scope != admitted_scope {
        return Err(invalid());
    }
    Ok(())
}

fn validated_authority(
    root: &Path,
    item: &Snapshot,
    store: &TasksStore,
    relevant: &[Task],
) -> Result<(String, Vec<String>)> {
    let mut milestone = crate::core::tasks::Milestone::parse_with_identity(
        &item.text,
        &item.repository,
        &item.project,
    )?;
    milestone.project = item.project.clone();
    if milestone.tasks.len() != relevant.len() {
        bail!("TASK_AUTHORITY_MISSING: current milestone task set differs from accepted tasks");
    }
    let milestone_rel = item.path.strip_prefix(root)?.to_string_lossy().into_owned();
    let mut accepted = BTreeMap::new();
    let mut builders = BTreeSet::new();
    for task in relevant {
        let spec = milestone
            .tasks
            .iter()
            .find(|spec| milestone.task_id(spec) == task.task_id)
            .context("TASK_AUTHORITY_MISSING: accepted task is missing from current milestone")?;
        let mut task_authority = task.spec.clone();
        task_authority.scope = spec.scope.clone();
        if milestone.digest(spec)? != task.digest
            || milestone.digest(&task_authority)? != task.digest
            || spec.contract_revision != task.contract_revision
            || task.spec.contract_revision != task.contract_revision
            || spec.status.as_deref() != Some(task.status.as_str())
            || task.milestone_ref != milestone_rel
        {
            bail!(
                "TASK_AUTHORITY_CHANGED: task '{}' contract differs from accepted authority",
                task.task_id
            );
        }
        validate_effective_scope(store, task, spec)?;
        if task.status == "cancelled" {
            if task.spec.status.as_deref() != Some("cancelled")
                || task.receipt.is_some()
                || task.spec.receipt.is_some()
                || spec.receipt.is_some()
            {
                bail!("TASK_RECEIPT_INVALID: cancelled task cannot carry completion authority");
            }
            accepted.insert(task.task_id.clone(), json!({"digest":task.digest, "contract_revision":task.contract_revision, "status":"cancelled", "milestone_ref":task.milestone_ref}));
            continue;
        }
        let original = task
            .receipt
            .as_ref()
            .context("TASK_RECEIPT_INVALID: completed task has no durable receipt")?;
        let markdown = spec
            .receipt
            .as_ref()
            .context("TASK_RECEIPT_INVALID: completed task has no Markdown receipt")?;
        if !original.matches_proofs(markdown) {
            bail!("TASK_RECEIPT_INVALID: Markdown receipt differs from accepted task proofs");
        }
        let profile = store.profile_for_task(task)?;
        let mut proof_receipt = original.clone();
        proof_receipt.commit = None;
        crate::core::tasks::gates::validate_durable_receipt_with_profile(
            &proof_receipt,
            task.contract_revision,
            &profile,
        )?;
        let evidence = validate_task_provenance(
            root,
            store,
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
        for gate in &profile.gates {
            let proof = &evidence[&gate.id];
            if gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Command) {
                builders.insert(proof.worker_id.clone());
            }
            for commit in [
                proof.commit.as_deref(),
                proof.candidate_baseline_head.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                crate::core::git::retain_proof_commit(root, commit)?;
            }
        }
        accepted.insert(task.task_id.clone(), json!({"digest":task.digest, "contract_revision":task.contract_revision, "scope":task.spec.scope, "profile":profile.as_ref(), "gates":evidence, "receipt":proof_receipt}));
    }
    Ok((
        integration::digest(&accepted)?,
        builders.into_iter().collect(),
    ))
}

fn binding(
    root: &Path,
    item: &Snapshot,
    store: &TasksStore,
    relevant: &[Task],
    commit: &str,
    target: &str,
) -> Result<HandoffBinding> {
    integration::require_clean_tree(root, commit)?;
    let (authority_digest, builders) = validated_authority(root, item, store, relevant)?;
    let milestone_path = item.path.strip_prefix(root)?.to_string_lossy().into_owned();
    let changes = crate::core::git::git_command(
        root,
        &["diff", "--name-only", "--no-renames", "-z", target, commit],
    )?;
    if !changes.status.success() {
        bail!("HANDOFF_CONTENT_UNAVAILABLE: cannot compare target base and final content");
    }
    for path in changes
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?;
        if path != milestone_path
            && !relevant.iter().any(|task| {
                task.status == "completed"
                    && task
                        .spec
                        .scope
                        .iter()
                        .any(|entry| crate::core::tasks::path_matches_scope_entry(path, entry))
            })
        {
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

/// Prepare content identities for external integration review and verification.
/// Accepted proof objects are retained independently of mutable branch history.
pub fn handoff_context(root: &Path, selector: &str, target_base: &str) -> Result<Value> {
    let (item, store, relevant) = ready_milestone(root, selector)?;
    let commit = integration::resolve_commit(root, "HEAD")?;
    let target = integration::resolve_commit(root, target_base)?;
    let binding = binding(root, &item, &store, &relevant, &commit, &target)?;
    crate::core::git::retain_proof_commit(root, &commit)?;
    crate::core::git::retain_proof_commit(root, &target)?;
    Ok(json!({"binding":binding, "commit":commit, "target_base":target}))
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

/// Archive a completed milestone using immutable task proofs and final content evidence.
pub fn handoff(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    evidence: &IntegrationEvidence,
) -> Result<Value> {
    let (item, mut store, relevant) = ready_milestone(root, selector)?;
    let sha = integration::resolve_commit(root, commit.unwrap_or("HEAD"))?;
    let target = integration::resolve_commit(root, &evidence.target_base)?;
    let current = binding(root, &item, &store, &relevant, &sha, &target)?;
    evidence.validate(&current)?;
    crate::core::git::retain_proof_commit(root, &sha)?;
    crate::core::git::retain_proof_commit(root, &target)?;
    let prefix = format!("{}/{}:", item.project, item.id);
    let settings = tasks::TaskSettings::load(root)?;
    store.set_handoff_pruning(settings.handoff_pruning);
    let record_task_commits = settings.records_task_commits();
    let mut original_receipts = BTreeMap::new();
    let mut landed_receipts = BTreeMap::new();
    let mut handoff_text = item.text.replace("\r\n", "\n");
    for task in relevant.iter().filter(|task| task.status == "completed") {
        let original = task
            .receipt
            .as_ref()
            .context("TASK_RECEIPT_INVALID: completed task has no receipt")?;
        let mut landed = original.clone();
        let profile = store.profile_for_task(task)?;
        let accepted = accepted_gate_evidence(
            root,
            &store,
            &task.task_id,
            task.contract_revision,
            &task.spec.proof_policy,
            &profile,
        )?;
        let delivery = profile
            .gates
            .iter()
            .find(|gate| {
                gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Delivery)
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
        original_receipts.insert(task.task_id.clone(), original.clone());
        landed_receipts.insert(task.task_id.clone(), landed);
    }
    // A mutable ref or worktree edit during evidence inspection cannot publish a different tree.
    integration::require_clean_tree(root, &sha)?;
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
        "commit": sha, "target_base": target, "integration_evidence": evidence, "verification": {"command": evidence.verification.command, "status": "passed", "tests_passed": evidence.verification.tests_passed, "tests_failed": evidence.verification.tests_failed}
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
        &relevant,
        &landed_receipts,
    ) {
        fs::remove_file(&archived)?;
        return Err(error);
    }
    if let Err(error) = fs::remove_file(&item.path) {
        // Pruning is irreversible here; preserve the archive when unlink fails.
        if settings.handoff_pruning {
            return Err(error.into());
        }
        let mut archived_tasks = relevant.clone();
        for task in &mut archived_tasks {
            task.milestone_ref = archive_ref.clone();
            if let Some(receipt) = landed_receipts.get(&task.task_id) {
                task.receipt = Some(receipt.clone());
                task.spec.receipt = Some(receipt.clone());
            }
        }
        store.handoff_milestone(
            &prefix,
            &archive_ref,
            &relative,
            &archived_tasks,
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
    fs::write(root.join(".git/info/exclude"), ".forge/\n").unwrap();
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
    let base = git(&["rev-parse", "HEAD"]);
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
    let context = handoff_context(root, "m-native-shared", &base).unwrap();
    let evidence: IntegrationEvidence = serde_json::from_value(json!({
        "binding": context["binding"], "target_base": base,
        "review": {"worker_id":"integration-reviewer", "decision":"pass", "summary":"Reviewed combined delta and task authority", "scope_findings":[], "contract_findings":[]},
        "verification": {"command":"cargo test --test acdd", "exit_code":0, "tests_passed":1, "tests_failed":0}
    })).unwrap();
    let result = handoff(root, "m-native-shared", Some(&head), &evidence);
    if result.is_err() {
        assert_eq!(fs::read(root.join(active)).unwrap(), before);
        assert!(!root.join(archived).exists());
    }
    result.expect("a later completed reviewed task may change an earlier task's shared path");
    let reopened = TasksStore::open(&root.join(".forge/tasks.sqlite")).unwrap();
    let archived_doc = fs::read_to_string(root.join(archived)).unwrap();
    let parsed = crate::core::tasks::Milestone::parse(&archived_doc, "forge-mcp").unwrap();
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
    for (index, (_, landed)) in landings.iter().enumerate() {
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
