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

fn full_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalTask {
    status: String,
    #[serde(deserialize_with = "required_receipt")]
    source_receipt: Option<crate::core::tasks::Receipt>,
    #[serde(deserialize_with = "required_receipt")]
    landed_receipt: Option<crate::core::tasks::Receipt>,
}

fn required_receipt<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<crate::core::tasks::Receipt>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}

fn terminal_tasks(
    milestone: &crate::core::tasks::Milestone,
    landed: &BTreeMap<String, crate::core::tasks::Receipt>,
) -> Result<BTreeMap<String, TerminalTask>> {
    let mut terminal = BTreeMap::new();
    for task in &milestone.tasks {
        let id = milestone.task_id(task);
        let status = task
            .status
            .as_deref()
            .context("HANDOFF_ARCHIVE_INVALID: missing terminal status")?;
        if !matches!(status, "completed" | "cancelled")
            || (status == "completed") != landed.contains_key(&id)
            || (status == "completed" && task.receipt.is_none())
        {
            bail!("HANDOFF_ARCHIVE_INVALID: terminal outcome/receipt mismatch for {id}");
        }
        terminal.insert(
            id.clone(),
            TerminalTask {
                status: status.into(),
                source_receipt: task.receipt.clone(),
                landed_receipt: landed.get(&id).cloned(),
            },
        );
    }
    if landed.len()
        != terminal
            .values()
            .filter(|task| task.status == "completed")
            .count()
    {
        bail!("HANDOFF_ARCHIVE_INVALID: foreign landed receipts");
    }
    Ok(terminal)
}

fn reconcile_terminal_tasks(
    note: &Value,
    milestone: &crate::core::tasks::Milestone,
) -> Result<BTreeMap<String, crate::core::tasks::Receipt>> {
    let accepted: BTreeMap<String, TerminalTask> =
        serde_json::from_value(note["terminal_tasks"].clone())
            .context("HANDOFF_ARCHIVE_INVALID: durable terminal metadata missing or invalid")?;
    let receipts: BTreeMap<String, crate::core::tasks::Receipt> =
        serde_json::from_value(note["landed_receipts"].clone())?;
    let observed = terminal_tasks(milestone, &receipts)?;
    if observed.len() != accepted.len() {
        bail!("HANDOFF_ARCHIVE_INVALID: terminal task identities differ");
    }
    for (id, task) in &observed {
        let previous = accepted
            .get(id)
            .context("HANDOFF_ARCHIVE_INVALID: unknown terminal task")?;
        if (previous.status == "completed"
            && (previous.source_receipt.is_none() || previous.landed_receipt.is_none()))
            || task.status != previous.status
            || task.landed_receipt != previous.landed_receipt
            || (task.source_receipt != previous.source_receipt
                && task.source_receipt != previous.landed_receipt)
        {
            bail!("HANDOFF_ARCHIVE_INVALID: terminal task outcome or receipt changed for {id}");
        }
    }
    let recorded = note["tasks"]
        .as_array()
        .context("HANDOFF_ARCHIVE_INVALID: missing task outcomes")?;
    let mut outcomes = BTreeMap::new();
    for task in recorded {
        let id = task["task_id"]
            .as_str()
            .context("HANDOFF_ARCHIVE_INVALID: missing task identity")?;
        let status = task["status"]
            .as_str()
            .context("HANDOFF_ARCHIVE_INVALID: missing task outcome")?;
        if outcomes.insert(id, status).is_some() {
            bail!("HANDOFF_ARCHIVE_INVALID: duplicate terminal metadata");
        }
    }
    if outcomes.len() != accepted.len()
        || accepted
            .iter()
            .any(|(id, task)| outcomes.get(id.as_str()).copied() != Some(task.status.as_str()))
    {
        bail!("HANDOFF_ARCHIVE_INVALID: passport terminal outcomes disagree");
    }
    Ok(receipts)
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
    validated_authority(root, &item, &store, &relevant)?;
    absorb_polish(root, &item, &relevant)?;
    let commit = integration::resolve_commit(root, "HEAD")?;
    let target = integration::resolve_commit(root, target_base)?;
    let binding = binding(root, &item, &store, &relevant, &commit, &target)?;
    crate::core::git::retain_proof_commit(root, &commit)?;
    crate::core::git::retain_proof_commit(root, &target)?;
    integration::write_handoff_owner(
        root,
        &commit,
        &integration::HandoffProofOwner {
            identity: format!("{}/{}/{}", item.repository, item.project, item.id),
            worktree: root.canonicalize()?,
            proof_shas: BTreeSet::from([commit.clone(), target.clone()]),
        },
    )?;
    Ok(
        json!({"binding":binding, "commit":commit, "target_base":target,
        "requires_project_verification":source_polish(root, &item)?}),
    )
}

fn source_polish(root: &Path, item: &Snapshot) -> Result<bool> {
    let marker = format!(
        "Forge-Handoff-Source-Polish: {}/{}/{}",
        item.repository, item.project, item.id
    );
    let output = crate::core::git::git_command(
        root,
        &[
            "log",
            "-1",
            "--format=%B",
            "--fixed-strings",
            &format!("--grep={marker}"),
            "HEAD",
        ],
    )?;
    if !output.status.success() {
        bail!("HANDOFF_CONTENT_UNAVAILABLE: cannot inspect source polish provenance");
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .any(|line| line == marker))
}

fn absorb_polish(root: &Path, item: &Snapshot, relevant: &[Task]) -> Result<()> {
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
        if path != &milestone_path
            && !relevant.iter().any(|task| {
                task.status == "completed"
                    && task
                        .spec
                        .scope
                        .iter()
                        .any(|scope| crate::core::tasks::path_matches_scope_entry(path, scope))
            })
        {
            bail!("HANDOFF_SCOPE_UNADMITTED: polish contains unadmitted path '{path}'");
        }
    }
    let source_changed = paths.iter().any(|path| {
        path != &milestone_path && !path.starts_with("docs/") && !path.starts_with("tests/")
    });
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

fn proof_shas(text: &str) -> BTreeSet<String> {
    let normalized = text.replace("\r\n", "\n");
    let metadata = normalized
        .strip_prefix("---\n")
        .and_then(|body| body.split_once("\n---\n"))
        .and_then(|(yaml, _)| serde_yaml::from_str::<serde_yaml::Value>(yaml).ok());
    if let Some(values) = metadata
        .as_ref()
        .and_then(|meta| meta["handoff"]["proof_shas"].as_sequence())
    {
        return values
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|value| full_git_sha(value))
            .map(str::to_owned)
            .collect();
    }
    text.split(|ch: char| !ch.is_ascii_hexdigit())
        .filter(|value| full_git_sha(value))
        .map(str::to_owned)
        .collect()
}

#[derive(Default)]
struct ActiveProofOwners {
    proofs: BTreeSet<String>,
    identities: BTreeSet<String>,
    outgoing_proofs: BTreeSet<String>,
}

// Capture durable task/prepared-handoff owners before deciding whether SQLite was pruned.
fn active_proof_owners(
    root: &Path,
    store: &TasksStore,
    outgoing: &str,
) -> Result<ActiveProofOwners> {
    let output = crate::core::git::git_command(root, &["worktree", "list", "--porcelain", "-z"])?;
    if !output.status.success() {
        bail!("HANDOFF_REF_CLEANUP_FAILED: cannot enumerate linked worktrees");
    }
    let mut active = BTreeMap::new();
    let mut terminal = BTreeSet::new();
    for field in output.stdout.split(|byte| *byte == 0) {
        let Some(path) = field.strip_prefix(b"worktree ") else {
            continue;
        };
        let tree = std::path::PathBuf::from(std::str::from_utf8(path)?).canonicalize()?;
        let settings = tasks::TaskSettings::load(&tree)?;
        let mut identities = BTreeSet::new();
        for path in super::milestone_paths(&tree)? {
            if super::is_archived_path(&path) {
                continue;
            }
            let item = super::contract::snapshot(&tree, path, &settings)?;
            let identity = format!("{}/{}/{}", item.repository, item.project, item.id);
            if matches!(item.status.as_str(), "active" | "planned") && identity != outgoing {
                if !item.tasks.is_empty()
                    && item.tasks.iter().all(|task| {
                        matches!(task["status"].as_str(), Some("completed" | "cancelled"))
                    })
                {
                    terminal.insert((tree.clone(), identity.clone()));
                }
                identities.insert(identity);
            }
        }
        active.insert(tree, identities);
    }
    let mut owners = ActiveProofOwners {
        identities: active.values().flatten().cloned().collect(),
        ..Default::default()
    };
    for owner in integration::handoff_owners(root)? {
        if owner.identity == outgoing {
            owners.outgoing_proofs.extend(owner.proof_shas);
            continue;
        }
        if active
            .get(&owner.worktree)
            .is_some_and(|identities| identities.contains(&owner.identity))
        {
            terminal.remove(&(owner.worktree.clone(), owner.identity.clone()));
            owners.proofs.extend(owner.proof_shas);
        }
    }
    if !terminal.is_empty() {
        bail!("HANDOFF_PROOF_OWNERSHIP_UNAVAILABLE: terminal foreign milestones may have unowned legacy handoff preparations: {terminal:?}. Run handoff-context in those worktrees with their actual target bases before retrying cleanup.");
    }
    let descriptors: Vec<String> = store
        .connection
        .prepare("SELECT descriptor FROM tasks")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for descriptor in descriptors {
        let task: Task = serde_json::from_str(&descriptor)?;
        if super::is_archived_path(Path::new(&task.milestone_ref)) {
            continue;
        }
        let Some(authority) = &task.authority_snapshot else {
            continue;
        };
        let identity = format!(
            "{}/{}/{}",
            authority.repository, authority.project, authority.milestone_id
        );
        if identity == outgoing {
            continue;
        }
        owners.identities.insert(identity);
        owners.proofs.extend(proof_shas(&descriptor));
        let evidence: Vec<String> = store
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND evidence IS NOT NULL")?
            .query_map([&task.task_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for proof in evidence {
            owners.proofs.extend(proof_shas(&proof));
        }
    }
    Ok(owners)
}

fn cleanup_handoff_refs(
    root: &Path,
    repository: &str,
    project: &str,
    id: &str,
    text: &str,
    owners: &ActiveProofOwners,
) -> Result<()> {
    let mut proofs = proof_shas(text);
    proofs.extend(owners.outgoing_proofs.iter().cloned());
    for sha in proofs {
        if owners.proofs.contains(&sha) {
            continue;
        }
        integration::git_write(
            root,
            &["update-ref", "-d", &format!("refs/forge/evidence/{sha}")],
            "HANDOFF_REF_CLEANUP_FAILED",
        )?;
    }
    // Keepers are namespace-qualified; the producer belongs to the next task.
    for prefix in [
        format!("refs/forge/state/{repository}/{project}/{id}/"),
        format!("refs/forge/state/{project}/{id}/"),
        format!("refs/forge/state/{id}/"),
    ] {
        let shared = if prefix == format!("refs/forge/state/{id}/") {
            owners
                .identities
                .iter()
                .any(|identity| identity.rsplit('/').next() == Some(id))
        } else if prefix == format!("refs/forge/state/{project}/{id}/") {
            owners
                .identities
                .iter()
                .any(|identity| identity.ends_with(&format!("/{project}/{id}")))
        } else {
            false
        };
        if shared {
            continue;
        }
        let output =
            crate::core::git::git_command(root, &["for-each-ref", "--format=%(refname)", &prefix])?;
        if !output.status.success() {
            bail!("HANDOFF_REF_CLEANUP_FAILED: cannot enumerate keeper refs");
        }
        let refs = String::from_utf8(output.stdout)?;
        for reference in refs
            .lines()
            .filter(|reference| reference.starts_with(&prefix))
        {
            integration::git_write(
                root,
                &["update-ref", "-d", reference],
                "HANDOFF_REF_CLEANUP_FAILED",
            )?;
        }
    }
    prune_snapshot_refs(root, &format!("{project}/{id}:"));
    integration::clear_handoff_owners(root, &format!("{repository}/{project}/{id}"))?;
    Ok(())
}

fn already_handed_off(id: &str) -> anyhow::Error {
    anyhow::anyhow!("MILESTONE_ALREADY_HANDED_OFF: Milestone '{id}' has already been handed off and pruned in SQLite by another worktree. Merge or pull the handoff commit into this branch.")
}

fn symbol_facts(root: &Path, revision: &str, path: &str) -> Result<BTreeMap<String, String>> {
    let Some(language) = crate::engine::languages::for_path(Path::new(path)) else {
        return Ok(BTreeMap::new());
    };
    let blob = crate::core::git::git_command(root, &["show", &format!("{revision}:{path}")])?;
    if !blob.status.success() {
        let exists = crate::core::git::git_command(
            root,
            &["cat-file", "-e", &format!("{revision}:{path}")],
        )?;
        if !exists.status.success() {
            return Ok(BTreeMap::new());
        }
        bail!("HANDOFF_TELEMETRY_FAILED: cannot read AST input '{path}'");
    }
    let source = String::from_utf8(blob.stdout)?;
    let facts = crate::engine::ast::extract(path, language.id(), &source)?;
    let lines: Vec<_> = source.lines().collect();
    facts
        .nodes
        .iter()
        .filter(|node| node.kind != "module")
        .map(|node| {
            let first = node.line.saturating_sub(1).min(lines.len());
            let end = node.end_line.min(lines.len()).max(first);
            Ok((
                format!("{path}:{}:{}", node.kind, node.qualname),
                integration::digest(&lines[first..end])?,
            ))
        })
        .collect()
}

fn telemetry_passport(
    root: &Path,
    item: &Snapshot,
    store: &TasksStore,
    relevant: &[Task],
    commits: (&str, &str),
    evidence: &IntegrationEvidence,
    times: (chrono::DateTime<Utc>, chrono::DateTime<Utc>),
) -> Result<(Value, BTreeSet<String>)> {
    let (sha, target) = commits;
    let (started, completed) = times;
    let mut proofs = BTreeSet::from([sha.to_owned(), target.to_owned()]);
    let mut per_task = Vec::new();
    for task in relevant {
        let claimed_at: Option<i64> = store.connection.query_row(
            "SELECT min(claimed_at) FROM task_claims WHERE task_id=?1",
            [&task.task_id],
            |row| row.get(0),
        )?;
        let profile = store.profile_for_task(task)?;
        let review_gates: BTreeSet<_> = profile
            .gates
            .iter()
            .filter(|gate| {
                gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Review)
            })
            .map(|gate| gate.id.as_str())
            .collect();
        let claims: Vec<(String, String)> = store
            .connection
            .prepare("SELECT gate,state FROM task_gates WHERE task_id=?1 ORDER BY revision")?
            .query_map([&task.task_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let gates: Vec<String> = store
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND evidence IS NOT NULL")?
            .query_map([&task.task_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for gate in gates {
            proofs.extend(proof_shas(&gate));
        }
        if let Some(receipt) = &task.receipt {
            proofs.extend(proof_shas(&serde_json::to_string(receipt)?));
        }
        per_task.push(json!({"task_id":task.task_id,"status":task.status,"first_claimed_at":claimed_at,"completed_at":task.completed_at,
            "execution_wall_seconds":claimed_at.zip(task.completed_at).and_then(|(start,end)| end.checked_sub(start).filter(|duration| *duration >= 0)),
            "review_cycles":claims.iter().filter(|(gate,_)| review_gates.contains(gate.as_str())).count(),
            "rejected_reviews":claims.iter().filter(|(gate,state)| review_gates.contains(gate.as_str()) && state == "rejected").count()}));
    }
    let diff = crate::core::git::git_command(
        root,
        &["diff", "--numstat", "--no-renames", "-z", target, sha],
    )?;
    if !diff.status.success() {
        bail!("HANDOFF_TELEMETRY_FAILED: cannot inspect integration diff");
    }
    let mut breakdown: BTreeMap<&str, Vec<Value>> = BTreeMap::from([
        ("src", vec![]),
        ("tests", vec![]),
        ("docs", vec![]),
        ("other", vec![]),
    ]);
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for row in diff
        .stdout
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
    {
        let row = std::str::from_utf8(row)?;
        let mut fields = row.splitn(3, '\t');
        let added = fields.next().context("invalid numstat")?;
        let removed = fields.next().context("invalid numstat")?;
        let path = fields.next().context("invalid numstat path")?;
        let category = if path.starts_with("src/") {
            "src"
        } else if path.starts_with("tests/") {
            "tests"
        } else if path.starts_with("docs/") {
            "docs"
        } else {
            "other"
        };
        breakdown.get_mut(category).context("diff category missing")?.push(json!({"path":path,"added":added.parse::<u64>().ok(),"removed":removed.parse::<u64>().ok()}));
        before.extend(symbol_facts(root, target, path)?);
        after.extend(symbol_facts(root, sha, path)?);
    }
    let added: Vec<_> = after
        .keys()
        .filter(|key| !before.contains_key(*key))
        .cloned()
        .collect();
    let removed: Vec<_> = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .cloned()
        .collect();
    let modified: Vec<_> = after
        .iter()
        .filter(|(key, value)| before.get(*key).is_some_and(|old| old != *value))
        .map(|(key, _)| key.clone())
        .collect();
    let settings = tasks::TaskSettings::load(root)?;
    let canonical = |dep: &str, project: &str| -> Result<String> {
        // Only an explicitly qualified owner is raw alias input; the default
        // project has already been canonicalized by the manifest parser.
        let (owner, id) = match dep.split_once('/') {
            Some((owner, id)) => (
                crate::core::tasks::canonical_project(owner, &settings.aliases)?,
                id,
            ),
            None => (project.to_owned(), dep),
        };
        Ok(format!("{}/{}/{}", item.repository, owner, id))
    };
    let identity = format!("{}/{}/{}", item.repository, item.project, item.id);
    let upstream: BTreeSet<_> = item
        .depends_on
        .iter()
        .map(|dep| canonical(dep, &item.project))
        .collect::<Result<_>>()?;
    let mut downstream = BTreeSet::new();
    let mut known = BTreeSet::new();
    for path in super::milestone_paths(root)? {
        let text = fs::read_to_string(&path)?;
        let Ok(parsed) = crate::core::tasks::parse_milestone_manifest(
            &text,
            path.strip_prefix(root)?,
            &settings.task_repository,
            settings
                .task_project
                .as_deref()
                .unwrap_or(&settings.task_repository),
            &settings.owners,
            &settings.aliases,
        ) else {
            continue;
        };
        let owner = format!("{}/{}/{}", parsed.repository, parsed.project, parsed.id);
        known.insert(owner.clone());
        if parsed
            .depends_on
            .iter()
            .map(|dep| canonical(dep, &parsed.project))
            .collect::<Result<Vec<_>>>()?
            .contains(&identity)
        {
            downstream.insert(owner);
        }
    }
    let lint = if let Some(selection) = &evidence.verification.lint_command {
        json!({"status":"clean","observation":integration::observe_command(root, selection, &evidence.binding)?})
    } else {
        json!({"status":"unobserved"})
    };
    let verification = if source_polish(root, item)? {
        let selection = evidence.verification.project_command.as_deref().context("HANDOFF_COMMAND_REQUIRED: source polish requires an explicitly selected registered project_command")?;
        Some(integration::observe_command(
            root,
            selection,
            &evidence.binding,
        )?)
    } else {
        None
    };
    let mut passport = json!({"version":1,"identity":identity,"binding":evidence.binding,"commit":sha,"target_base":target,
        "active_authority_digest":integration::digest(&integration::manifest_projection(item.text.as_bytes())?)?,
        "started_at":started.to_rfc3339(),"completed_at":completed.to_rfc3339(),"duration_seconds":(completed-started).num_seconds(),
        "tasks_count":relevant.len(),"tasks":per_task,"diff":breakdown,"ast_symbol_deltas":{"added":added,"removed":removed,"modified":modified},
        "test_suite_results":evidence.verification,"automatic_project_verification":verification,"linter":lint,
        "topology":{"upstream":upstream,"downstream":downstream,"unresolved_upstream":upstream.difference(&known).collect::<Vec<_>>()}});
    let token = integration::digest(&passport)?;
    passport["verified_handoff_proof"] = token.into();
    Ok((passport, proofs))
}

// Resolve recovery before readiness: accepted gates may already have been pruned.
fn retry_handoff(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    evidence: &IntegrationEvidence,
) -> Result<Option<Value>> {
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
    let store = TasksStore::open_project(&db, repository, project)?;
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
                cleanup_handoff_refs(root, repository, project, id, &text, &owners)?;
                return Ok(Some(json!({"id":id,"status":"completed","path":path})));
            }
        }
        cleanup_handoff_refs(root, repository, project, id, "", &owners)?;
        return Err(already_handed_off(id));
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
    // No SQLite access is allowed below this decision.
    if !tombstoned && !relocated {
        return Ok(None);
    }
    if !archived.exists() {
        cleanup_handoff_refs(root, &item.repository, &item.project, &item.id, "", &owners)?;
        return Err(already_handed_off(&item.id));
    }
    let mut archive_text = fs::read_to_string(&archived)?;
    if !valid {
        if !active_present {
            bail!("HANDOFF_ARCHIVE_INVALID: archive is not durable authority");
        }
        let sha = integration::resolve_commit(root, commit.unwrap_or("HEAD"))?;
        let target = integration::resolve_commit(root, &evidence.target_base)?;
        let mut proofs = proof_shas(&archive_text);
        proofs.extend(proof_shas(&item.text));
        proofs.extend([sha.clone(), target.clone()]);
        let note = integration::read_passport(root, &sha, &format!("{}/{}/{}", item.repository, item.project, item.id))?
            .context("HANDOFF_PASSPORT_UNAVAILABLE: durable pre-prune passport is required for torn archive repair")?;
        if note["binding"] != serde_json::to_value(&evidence.binding)? {
            bail!("HANDOFF_EVIDENCE_STALE: recovery evidence differs from durable passport");
        }
        evidence.validate(&serde_json::from_value(note["binding"].clone())?)?;
        let mut meta = item.frontmatter.clone();
        meta["status"] = "completed".into();
        meta["repository"] = item.repository.clone().into();
        meta["project"] = archive_project(root, &item, &settings)?.into();
        meta["handoff"] = serde_yaml::to_value(
            json!({"completed_at":note["completed_at"],"commit":sha,"target_base":target,"integration_evidence":evidence,"proof_shas":proofs,"passport":note}),
        )?;
        let mut restored = item.text.clone();
        let parsed = crate::core::tasks::parse_milestone_manifest(
            &restored,
            item.path.strip_prefix(root)?,
            &item.repository,
            settings
                .task_project
                .as_deref()
                .unwrap_or(&settings.task_repository),
            &settings.owners,
            &settings.aliases,
        )?;
        if note["active_authority_digest"].as_str()
            != Some(
                integration::digest(&integration::manifest_projection(item.text.as_bytes())?)?
                    .as_str(),
            )
            || parsed
                .tasks
                .iter()
                .any(|task| !matches!(task.status.as_deref(), Some("completed" | "cancelled")))
        {
            bail!("HANDOFF_ARCHIVE_INVALID: active authority changed or tasks are not terminal");
        }
        let receipts = reconcile_terminal_tasks(&note, &parsed)?;
        for task in &parsed.tasks {
            if let Some(receipt) = receipts.get(&parsed.task_id(task)) {
                restored = render_task_receipt(&restored, &task.task_ref, receipt, &task.subtasks)?;
            }
        }
        let (_, body) = restored
            .strip_prefix("---\n")
            .context("milestone frontmatter missing")?
            .split_once("\n---\n")
            .context("unterminated milestone frontmatter")?;
        archive_text = format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?);
        if crate::db::tasks_store::configured_milestone_archive(
            root,
            &archive_text,
            &archived,
            &item.repository,
            &item.project,
        )?
        .is_none_or(|archive| archive.id != item.id)
        {
            bail!("HANDOFF_ARCHIVE_INVALID: active manifest cannot restore terminal archive");
        }
        crate::core::fs::atomic_replace(&archived, archive_text.as_bytes())?;
    }
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
    if let Some(result) = retry_handoff(root, selector, commit, evidence)? {
        return Ok(result);
    }
    let (item, mut store, relevant) = ready_milestone(root, selector)?;
    let owners = active_proof_owners(
        root,
        &store,
        &format!("{}/{}/{}", item.repository, item.project, item.id),
    )?;
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
        landed_receipts.insert(task.task_id.clone(), landed);
    }
    // A mutable ref or worktree edit during evidence inspection cannot publish a different tree.
    integration::require_clean_tree(root, &sha, &archive_outputs(root, &item)?)?;
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
    let (mut passport, mut proofs) = telemetry_passport(
        root,
        &item,
        &store,
        &relevant,
        (&sha, &target),
        evidence,
        (started, completed),
    )?;
    passport
        .as_object_mut()
        .context("passport mapping missing")?
        .remove("verified_handoff_proof");
    passport["landed_receipts"] = serde_json::to_value(&landed_receipts)?;
    passport["terminal_tasks"] = serde_json::to_value(terminal_tasks(
        &crate::core::tasks::parse_milestone_manifest(
            &item.text,
            item.path.strip_prefix(root)?,
            &item.repository,
            settings
                .task_project
                .as_deref()
                .unwrap_or(&settings.task_repository),
            &settings.owners,
            &settings.aliases,
        )?,
        &landed_receipts,
    )?)?;
    passport["verified_handoff_proof"] = integration::digest(&passport)?.into();
    proofs.extend(proof_shas(&item.text));
    evidence.validate(&binding(root, &item, &store, &relevant, &sha, &target)?)?;
    integration::write_passport(
        root,
        &sha,
        &format!("{}/{}/{}", item.repository, item.project, item.id),
        &passport,
    )?;
    let declared_project = archive_project(root, &item, &settings)?;
    let mut meta = item.frontmatter;
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
        proof_list.extend(proof_shas(&fs::read_to_string(&archived)?));
        meta["handoff"]["proof_shas"] = serde_yaml::to_value(proof_list)?;
    }
    let archive_text = format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?);
    let backup = archived.with_extension("md.bak");
    let had_archive = archived.exists();
    if had_archive {
        fs::copy(&archived, &backup)?;
    }
    crate::core::fs::atomic_replace(&archived, archive_text.as_bytes())?;
    if let Err(error) = store.handoff_milestone(
        &prefix,
        &relative,
        &archive_ref,
        &relevant,
        &landed_receipts,
    ) {
        if had_archive {
            fs::rename(&backup, &archived)?;
        } else {
            fs::remove_file(&archived)?;
        }
        return Err(error);
    }
    if let Err(error) = fs::remove_file(&item.path) {
        // The durable archive and committed pruning/relocation remain authoritative.
        return Err(error.into());
    }
    if had_archive {
        fs::remove_file(&backup)?;
    }
    cleanup_handoff_refs(
        root,
        &item.repository,
        &item.project,
        &item.id,
        &archive_text,
        &owners,
    )?;
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
