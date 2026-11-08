use super::*;

pub(super) fn accepted_gate_evidence(
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
    let task = store.inspect(task_id)?;
    let mut accepted = BTreeMap::new();
    for (gate_pos, gate) in profile.gates.iter().enumerate() {
        if !crate::engine::tasks::claim::task_matches_gate_condition(gate.when.as_deref(), &task) {
            continue;
        }
        let bypassed_by_jump = profile.gates[..gate_pos].iter().any(|prior| {
            if let Some(target) = prior.on_success.as_deref() {
                if let Some(target_pos) = profile.index_of(target) {
                    if target_pos > gate_pos {
                        return accepted.contains_key(&prior.id);
                    }
                }
            }
            false
        });
        if bypassed_by_jump {
            continue;
        }
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
                "TASK_RECEIPT_INVALID: latest '{}' gate evidence is not accepted for task '{task_id}', state: {}",
                gate.id,
                row["state"]
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
            let Some(current) = accepted.get(&gate.id) else {
                continue;
            };
            let Some(prior_evidence) = accepted.get(prior) else {
                continue;
            };
            if current.worker_id == prior_evidence.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT: accepted '{}' worker matches configured prior gate '{}'; each gate must retain its configured independent worker", gate.id, prior);
            }
        }
    }
    Ok(accepted)
}

pub(super) struct TaskProvenance<'a> {
    task_id: &'a str,
    contract_revision: u64,
    proof_policy: &'a str,
    profile: &'a crate::core::tasks::profile::GateProfile,
    receipt: &'a crate::core::tasks::Receipt,
    milestone_path: &'a str,
    scope: &'a [String],
}

pub(super) fn validate_task_provenance(
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
                && task
                    .receipt
                    .delivery_gate
                    .as_ref()
                    .is_none_or(|selected| selected == &gate.id)
                && evidence.contains_key(&gate.id)
        })
        .context("TASK_RECEIPT_INVALID: profile has no delivery gate")?;
    let (snapshot_gate, snapshot) = task.profile.gates[..delivery_index]
        .iter()
        .rev()
        .filter(|gate| gate.sha_snapshot)
        .find_map(|gate| evidence.get(&gate.id).map(|ev| (gate, ev)))
        .context("TASK_RECEIPT_INVALID: accepted candidate snapshot is missing")?;
    let snapshot_commit = snapshot
        .commit
        .as_deref()
        .context("TASK_RECEIPT_INVALID: accepted candidate commit is missing")?;
    let baseline = snapshot
        .candidate_baseline_head
        .as_deref()
        .context("TASK_RECEIPT_INVALID: accepted candidate baseline is missing")?;
    if task.receipt.evidence != snapshot.proof
        || task
            .receipt
            .candidate_gate
            .as_deref()
            .is_some_and(|gate| gate != snapshot_gate.id)
    {
        bail!(
            "TASK_RECEIPT_INVALID: durable candidate proof does not match accepted build evidence"
        );
    }

    let delivery_gate = &task.profile.gates[delivery_index];
    if delivery_gate.review_sources.is_empty() {
        bail!("TASK_RECEIPT_INVALID: delivery profile has no review sources");
    }
    let resolve_review_source = |source: &str| -> Option<&crate::core::tasks::gates::Evidence> {
        if let Some(r) = evidence.get(source) {
            if r.commit.as_deref() == Some(snapshot_commit) {
                return Some(r);
            }
        }
        if let Some((prefix, _)) = snapshot_gate.id.rsplit_once('.') {
            let qualified = format!("{prefix}.{source}");
            if let Some(r) = evidence.get(&qualified) {
                if r.commit.as_deref() == Some(snapshot_commit) {
                    return Some(r);
                }
            }
        }
        evidence.get(source)
    };
    for source in &delivery_gate.review_sources {
        let review = resolve_review_source(source)
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
            let accepted_review = resolve_review_source(source)
                .context("TASK_RECEIPT_INVALID: accepted review gate evidence is missing")?;
            if sources.get(source) != Some(&accepted_review.proof) {
                bail!("TASK_RECEIPT_INVALID: durable review source '{source}' does not match accepted gate evidence");
            }
        }
    } else if delivery_gate.review_sources.len() == 1 {
        let source = &delivery_gate.review_sources[0];
        let matched_evidence = resolve_review_source(source);
        if matched_evidence.map(|value| &value.proof) != Some(&task.receipt.review) {
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

pub(super) fn ready_milestone(
    root: &Path,
    selector: &str,
) -> Result<(Snapshot, TasksStore, Vec<Task>)> {
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

pub(super) fn validate_effective_scope(
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

    let extended_scope = original
        .extended_scope
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<BTreeSet<_>>>()?;
    let norm_init = task
        .initial_scope
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<BTreeSet<_>>>()?;
    let genuine_extensions: BTreeSet<_> = if !norm_init.is_empty() {
        extended_scope.difference(&norm_init).cloned().collect()
    } else {
        extended_scope.clone()
    };
    let original_scope = original
        .scope
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<BTreeSet<_>>>()?
        .difference(&genuine_extensions)
        .cloned()
        .collect::<BTreeSet<_>>();
    if !task.initial_scope.is_empty() && norm_init != original_scope {
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

pub(super) fn validated_authority(
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
        let flattened_profile =
            crate::engine::tasks::claim::flattened_profile(&profile, Some(task))?;
        let mut proof_receipt = original.clone();
        proof_receipt.commit = None;
        crate::core::tasks::gates::validate_durable_receipt_with_profile(
            &proof_receipt,
            task.contract_revision,
            &flattened_profile,
        )?;
        let evidence = validate_task_provenance(
            root,
            store,
            TaskProvenance {
                task_id: &task.task_id,
                contract_revision: task.contract_revision,
                proof_policy: &task.spec.proof_policy,
                profile: &flattened_profile,
                receipt: original,
                milestone_path: &milestone_rel,
                scope: &task.spec.scope,
            },
        )?;
        for gate in &flattened_profile.gates {
            let Some(proof) = evidence.get(&gate.id) else {
                continue;
            };
            if matches!(
                gate.proof.kind(),
                Some(
                    crate::core::tasks::profile::ProofKind::Command
                        | crate::core::tasks::profile::ProofKind::SeamTest
                )
            ) {
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

pub(super) fn is_admitted_handoff_path(
    path: &str,
    milestone_path: &str,
    polish_scope: &[String],
    relevant: &[Task],
) -> bool {
    path == milestone_path
        || polish_scope
            .iter()
            .any(|entry| crate::core::tasks::path_matches_scope_entry(path, entry))
        || relevant.iter().any(|task| {
            task.status == "completed"
                && task
                    .spec
                    .scope
                    .iter()
                    .any(|entry| crate::core::tasks::path_matches_scope_entry(path, entry))
        })
}

pub(super) fn absorb_polish(
    root: &Path,
    item: &Snapshot,
    polish_scope: &[String],
    relevant: &[Task],
) -> Result<()> {
    let changed = crate::core::git::git_command(
        root,
        &["diff", "--name-only", "--no-renames", "-z", "HEAD"],
    )?;
    let untracked =
        crate::core::git::git_command(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    if !changed.status.success() || !untracked.status.success() {
        bail!("HANDOFF_CONTENT_UNAVAILABLE: cannot inspect polish");
    }
    let generated = archive_outputs(root, item)?;
    let paths: BTreeSet<String> = changed
        .stdout
        .split(|byte| *byte == 0)
        .chain(untracked.stdout.split(|byte| *byte == 0))
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?
        .into_iter()
        .filter(|path| !generated.contains(path))
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    let milestone_path = item.path.strip_prefix(root)?.to_string_lossy().into_owned();
    for path in &paths {
        if !is_admitted_handoff_path(path, &milestone_path, polish_scope, relevant) {
            bail!("HANDOFF_SCOPE_UNADMITTED: polish contains unadmitted path '{path}'");
        }
    }
    let source_changed = paths
        .iter()
        .any(|path| path != &milestone_path && !forge_core::is_doc_path(path));
    let mut add = vec!["add", "--all", "--"];
    add.extend(paths.iter().map(String::as_str));
    integration::git_write(root, &add, "HANDOFF_POLISH_FAILED")?;
    let message = if source_changed {
        format!(
            "Integrate milestone polish\n\nForge-Handoff-Source-Polish: {}/{}/{}",
            item.repository, item.project, item.id
        )
    } else {
        "Integrate milestone documentation and test polish".to_owned()
    };
    let mut commit = vec!["commit", "-m", &message, "--"];
    commit.extend(paths.iter().map(String::as_str));
    crate::core::git::git_stdout(root, &commit, "HANDOFF_POLISH_FAILED")?;
    Ok(())
}
