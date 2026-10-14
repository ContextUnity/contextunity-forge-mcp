use crate::core::tasks::{
    canonical_commit_from_prefix,
    gates::{
        review_rollup_summary, validate_durable_receipt_with_profile, validate_gate_proof, Evidence,
    },
    profile::ProofKind,
    Milestone, Receipt, ReceiptRollup, SubtaskSpec,
};
use crate::db::tasks_store::{
    check_stage, clear_blackboard, end_claim, load, now, save, Task, TasksStore,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn commit_tree_and_parent(worktree: &Path, commit: &str) -> Result<(String, String)> {
    let output =
        crate::core::git::git_command(worktree, &["rev-list", "--parents", "-n", "1", commit])?;
    if !output.status.success() {
        bail!("TASK_RECEIPT_INVALID: Git commit is unavailable");
    }
    let fields = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if fields.len() != 2 {
        bail!("TASK_RECEIPT_INVALID: candidate and delivery commits must have one baseline parent");
    }
    let treeish = format!("{commit}^{{tree}}");
    let tree = crate::core::git::git_command(worktree, &["rev-parse", &treeish])?;
    if !tree.status.success() {
        bail!("TASK_RECEIPT_INVALID: Git commit tree is unavailable");
    }
    Ok((
        String::from_utf8_lossy(&tree.stdout).trim().to_owned(),
        fields[1].clone(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryTreeEntry {
    mode: String,
    object: String,
}

fn delivery_tree_entries(
    worktree: &Path,
    treeish: &str,
    scope: Option<&[String]>,
) -> Result<std::collections::BTreeMap<Vec<u8>, DeliveryTreeEntry>> {
    let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", treeish];
    if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
        args.push("--");
        args.extend(scope.iter().map(String::as_str));
    }
    let output = crate::core::git::git_command(worktree, &args)?;
    if !output.status.success() {
        bail!("TASK_DELIVERY_COMMIT_INVALID: could not read Git tree entries");
    }
    let mut entries = std::collections::BTreeMap::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("TASK_DELIVERY_COMMIT_INVALID: malformed Git tree entry")?;
        let (header, path_with_separator) = record.split_at(separator);
        let path = &path_with_separator[1..];
        let fields = header.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("TASK_DELIVERY_COMMIT_INVALID: malformed Git tree entry header");
        }
        let entry = DeliveryTreeEntry {
            mode: String::from_utf8(fields[0].to_vec())?,
            object: String::from_utf8(fields[2].to_vec())?,
        };
        if entries.insert(path.to_vec(), entry).is_some() {
            bail!("TASK_DELIVERY_COMMIT_INVALID: duplicate Git tree path");
        }
    }
    Ok(entries)
}

fn validate_delivery_commit(
    worktree: &Path,
    candidate: &str,
    candidate_baseline: &str,
    scope: &[String],
    landed: &str,
    receipt_path: &Path,
    expected_receipt: &str,
) -> Result<()> {
    let landed_sha = landed;
    let candidate_entries = delivery_tree_entries(worktree, candidate, None)
        .context("TASK_CANDIDATE_INVALID: candidate snapshot tree is unavailable")?;
    let candidate_scoped = delivery_tree_entries(worktree, candidate, Some(scope))
        .context("TASK_CANDIDATE_INVALID: candidate scope entries are unavailable")?;
    if candidate_entries != candidate_scoped {
        bail!(
            "TASK_CANDIDATE_INVALID: candidate snapshot contains a path outside its admitted scope"
        );
    }
    let baseline_entries = delivery_tree_entries(worktree, candidate_baseline, None)
        .context("TASK_CANDIDATE_BASELINE_INVALID: baseline tree is unavailable")?;
    let baseline_scoped = delivery_tree_entries(worktree, candidate_baseline, Some(scope))
        .context("TASK_CANDIDATE_BASELINE_INVALID: baseline scope entries are unavailable")?;
    let landed = commit_tree_and_parent(worktree, landed_sha)
        .context("TASK_DELIVERY_COMMIT_INVALID: landed commit must have a baseline parent")?;
    if candidate_baseline != landed.1 {
        bail!("TASK_DELIVERY_COMMIT_INVALID: delivery parent must match the reviewed candidate baseline");
    }
    let receipt_path = receipt_path.to_string_lossy().replace('\\', "/");
    let mut expected_entries = baseline_entries;
    for path in baseline_scoped.keys() {
        expected_entries.remove(path);
    }
    expected_entries.extend(candidate_entries);
    let delivered_entries = delivery_tree_entries(worktree, landed_sha, None)
        .context("TASK_DELIVERY_COMMIT_INVALID: delivery tree is unavailable")?;
    let receipt_entry = delivered_entries
        .get(receipt_path.as_bytes())
        .cloned()
        .context("TASK_DELIVERY_COMMIT_INVALID: delivery tree is missing its milestone receipt")?;
    expected_entries.insert(receipt_path.as_bytes().to_vec(), receipt_entry);
    if expected_entries != delivered_entries {
        bail!("TASK_DELIVERY_COMMIT_INVALID: delivery tree must preserve the baseline, apply only candidate scope entries, and add the completed task receipt");
    }
    let spec = format!("{landed_sha}:{}", receipt_path);
    let committed_receipt = crate::core::git::git_command(worktree, &["show", &spec])?;
    if !committed_receipt.status.success()
        || committed_receipt.stdout != expected_receipt.as_bytes()
    {
        bail!("TASK_DELIVERY_COMMIT_INVALID: committed task receipt differs from its commitless milestone document");
    }
    Ok(())
}

struct ProvisionalReceiptRollback {
    path: PathBuf,
    original: String,
    armed: bool,
}

impl ProvisionalReceiptRollback {
    fn restore(&mut self) -> Result<()> {
        if self.armed {
            crate::core::fs::atomic_replace(&self.path, self.original.as_bytes())?;
            self.armed = false;
        }
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProvisionalReceiptRollback {
    fn drop(&mut self) {
        if self.armed {
            let _ = crate::core::fs::atomic_replace(&self.path, self.original.as_bytes());
        }
    }
}

impl TasksStore {
    /// Performs submit.
    pub fn submit(
        &mut self,
        id: &str,
        stage: &str,
        evidence: &Evidence,
        action: &str,
        findings: Option<&serde_json::Value>,
    ) -> Result<Task> {
        let mut evidence = evidence.clone();
        self.submit_with_pre_persist(id, stage, &mut evidence, action, findings, |_| Ok(None))
    }

    /// Performs submit after running a caller-owned operation once gate evidence is valid.
    /// The hook runs while the gate transaction is held and before its state is persisted.
    pub fn submit_with_pre_persist(
        &mut self,
        id: &str,
        stage: &str,
        evidence: &mut Evidence,
        action: &str,
        findings: Option<&serde_json::Value>,
        before_persist: impl FnOnce(&Task) -> Result<Option<String>>,
    ) -> Result<Task> {
        self.assert_id(id)?;
        if evidence.stage != stage {
            bail!("TASK_STAGE_INVALID: task '{id}'. evidence.stage '{}' != submitted stage '{stage}'. Do: set evidence.stage to '{stage}'.", evidence.stage);
        }
        if !matches!(action, "pass" | "reject") {
            bail!("invalid submission action");
        }
        let observed_task = self.inspect(id)?;
        let profile = self.profile_for_task(&observed_task)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task =
            load(&tx, id)?.context(crate::db::tasks_store::task_not_found_message(id))?;
        if task.profile_pin != observed_task.profile_pin || task.stage != observed_task.stage {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. stage or profile_pin changed concurrently. Do: retry submission.");
        }
        let mut document_update = None;
        let mut auto_delivery: Option<(String, String, std::path::PathBuf)> = None;
        let retry: Option<String> = tx
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                params![id, evidence.claim_revision],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(retry) = retry {
            let accepted: Submission = serde_json::from_str(&retry)?;
            if accepted.evidence == *evidence
                && accepted.action == action
                && accepted.findings.as_ref() == findings
                && accepted.task.contract_revision == task.contract_revision
            {
                before_persist(&accepted.task)?;
                return Ok(accepted.task);
            }
            bail!("TASK_STALE_SUBMISSION: task '{id}'. claim_revision {} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {}.", evidence.claim_revision, evidence.claim_revision);
        }
        let gate_index =
            profile.index_of(&task.stage).with_context(|| {
                format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        )
            })?;
        task.gate = gate_index;
        let gate_def = profile
            .gates
            .get(gate_index)
            .cloned()
            .context("TASK_STAGE_UNKNOWN")?;
        check_stage(id, &gate_def.id, stage)?;
        if task.status != "in_progress" {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. status is '{}'. Do: claim the task before submitting.", task.status);
        }
        let stale_field = if task.claim_revision != evidence.claim_revision {
            Some((
                "claim_revision",
                evidence.claim_revision.to_string(),
                task.claim_revision.to_string(),
            ))
        } else if task.contract_revision != evidence.contract_revision {
            Some((
                "contract_revision",
                evidence.contract_revision.to_string(),
                task.contract_revision.to_string(),
            ))
        } else if task.task_id != evidence.task_id {
            Some(("task_id", evidence.task_id.clone(), task.task_id.clone()))
        } else if task.worker_id.as_deref() != Some(&evidence.worker_id) {
            Some((
                "worker_id",
                evidence.worker_id.clone(),
                task.worker_id.clone().unwrap_or_default(),
            ))
        } else if task.worktree.as_deref() != Some(&evidence.worktree) {
            Some((
                "worktree",
                evidence.worktree.clone(),
                task.worktree.clone().unwrap_or_default(),
            ))
        } else {
            None
        };
        if let Some((field, evidence_value, active_value)) = stale_field {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {evidence_value} != active {active_value}. Do: resubmit with active {field} {active_value}.");
        }
        if evidence.proof.is_null() {
            bail!("TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. proof is null. Do: provide non-empty proof for stage '{stage}'.");
        }
        if evidence
            .commit
            .as_ref()
            .is_some_and(|commit| commit.trim().is_empty())
        {
            let snapshot = candidate_snapshot(&tx, id, &profile, gate_index)?
                .unwrap_or_else(|| "unavailable".into());
            bail!("TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. commit field is empty. Do: set commit to candidate snapshot {snapshot}.");
        }
        validate_gate_proof(
            &evidence.proof,
            &gate_def,
            &profile,
            action == "pass",
            &task.spec.proof_policy,
        )?;
        if let Some(prior) = gate_def.independent_from.as_deref() {
            let builder: String = tx.query_row(
                "SELECT json_extract(evidence,'$.worker_id') FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",
                params![id, prior],
                |r| r.get(0),
            )?;
            if builder == evidence.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT: task '{id}'. reviewer worker '{}' matches builder '{builder}'. Do: submit review with a worker_id other than '{builder}'.", evidence.worker_id);
            }
            let build = accepted(&tx, id, prior)?;
            if gate_def.proof.kind() != Some(ProofKind::Delivery) {
                let canonical = evidence.commit.as_deref().and_then(|supplied| {
                    build
                        .commit
                        .as_deref()
                        .and_then(|snapshot| canonical_commit_from_prefix(supplied, snapshot))
                });
                if let Some(canonical) = canonical {
                    evidence.commit = Some(canonical);
                }
                if build.commit != evidence.commit {
                    let supplied = evidence.commit.as_deref().unwrap_or("missing");
                    let snapshot = build.commit.as_deref().unwrap_or("missing");
                    bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != candidate snapshot {snapshot}. Do: resubmit with commit {snapshot}.");
                }
            }
        }
        if gate_def.proof.kind() == Some(ProofKind::Delivery) && action == "pass" {
            let snapshot_gate = profile.gates[..gate_index]
                .iter()
                .rev()
                .find(|gate| gate.sha_snapshot)
                .context("TASK_GATES_INVALID: delivery requires a prior sha_snapshot gate")?;
            let build = accepted(&tx, id, &snapshot_gate.id)?;
            let candidate_snapshot = build.commit.as_deref().unwrap_or("missing");
            if !gate_def.auto_commit {
                let canonical = evidence.commit.as_deref().and_then(|supplied| {
                    canonical_commit_from_prefix(supplied, candidate_snapshot)
                });
                if let Some(canonical) = canonical {
                    evidence.commit = Some(canonical);
                }
            }
            let receipt_commit = evidence
                .commit
                .as_deref()
                .context(format!("TASK_EVIDENCE_INVALID: task '{id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {candidate_snapshot}."))?;
            let candidate_baseline = build.candidate_baseline_head.as_deref().context(
                "TASK_CANDIDATE_BASELINE_INVALID: accepted candidate has no captured baseline",
            )?;
            if evidence.candidate_baseline_head.as_deref() != Some(candidate_baseline) {
                bail!("TASK_CANDIDATE_BASELINE_MISMATCH: delivery baseline differs from the accepted candidate");
            }
            if !gate_def.auto_commit && build.commit.as_deref() != Some(receipt_commit) {
                bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {receipt_commit} != candidate snapshot {candidate_snapshot}. Do: resubmit with commit {candidate_snapshot}.");
            }
            let mut review_proofs = Vec::with_capacity(gate_def.review_sources.len());
            for review_name in &gate_def.review_sources {
                let review = accepted(&tx, id, review_name)?;
                if review.commit != build.commit {
                    bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. review source '{review_name}' commit != candidate snapshot {candidate_snapshot}. Do: resubmit review evidence with commit {candidate_snapshot}.");
                }
                let review_gate = profile
                    .by_stage(review_name)
                    .context("TASK_GATES_INVALID")?;
                validate_gate_proof(
                    &review.proof,
                    review_gate,
                    &profile,
                    true,
                    &task.spec.proof_policy,
                )?;
                review_proofs.push((review_name.clone(), review.proof.clone()));
            }
            let claim_worktree: String = tx.query_row(
                "SELECT worktree FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![id, task.claim_revision],
                |row| row.get(0),
            )?;
            let path = super::confined_path(Path::new(&claim_worktree), &task.milestone_ref)?;
            if gate_def.auto_commit {
                auto_delivery = Some((
                    build
                        .commit
                        .as_deref()
                        .context("TASK_SNAPSHOT_NOT_FOUND")?
                        .to_owned(),
                    candidate_baseline.to_owned(),
                    path.clone(),
                ));
            }
            let text = std::fs::read_to_string(&path)?;
            let milestone = Milestone::parse_with_identity(&text, &self.repository, &self.project)?;
            let spec = milestone
                .tasks
                .iter()
                .find(|s| milestone.task_id(s) == id)
                .context("TASK_RECEIPT_INVALID: task not found")?;
            if milestone.digest(spec)? != task.digest {
                bail!("TASK_RECEIPT_INVALID: specification mismatch");
            }
            let review_value = if review_proofs.len() == 1 {
                review_proofs[0].1.clone()
            } else {
                serde_json::json!({"review_sources": review_proofs.iter().cloned().collect::<std::collections::BTreeMap<_, _>>()})
            };
            let outcomes = tx
                .prepare(
                    "SELECT topic,payload FROM task_blackboard \
                 WHERE task_id=?1 AND topic IN ('decisions','deferred') \
                 ORDER BY created_at,id",
                )?
                .query_map([id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|(topic, payload)| format!("{topic}: {payload}"))
                .collect::<Vec<_>>();
            let rollup = ReceiptRollup {
                verified_invariants: task.applicable_invariants.clone(),
                architectural_notes: outcomes,
                review_summary: review_rollup_summary(&review_proofs)?,
            };
            let document_commit = if gate_def.auto_commit {
                None
            } else {
                Some(receipt_commit.to_owned())
            };
            let generated = Receipt {
                commit: document_commit.clone(),
                contract_revision: task.contract_revision,
                passed_at: chrono::Utc::now().to_rfc3339(),
                evidence: build.proof.clone(),
                review: review_value.clone(),
                decision: "pass".into(),
                rollup: Some(rollup),
            };
            let receipt = match (spec.status.as_deref(), spec.receipt.as_ref()) {
                (Some("completed"), Some(existing)) => {
                    // The document may have been replaced immediately before a process crash.
                    // Reuse only the exact accepted outcome, retaining its original timestamp.
                    validate_receipt(
                        existing,
                        &task,
                        &build,
                        &review_value,
                        document_commit.as_deref(),
                        &profile,
                    )?;
                    if existing.rollup != generated.rollup {
                        bail!("TASK_RECEIPT_INVALID: rollup differs from accepted task context");
                    }
                    existing.clone()
                }
                (Some("completed"), None) | (_, Some(_)) => {
                    bail!("TASK_RECEIPT_INVALID: incomplete terminal receipt")
                }
                _ => generated,
            };
            validate_receipt(
                &receipt,
                &task,
                &build,
                &review_value,
                document_commit.as_deref(),
                &profile,
            )?;
            let (_, task_ref) = id.rsplit_once(':').context("invalid task identity")?;
            let mut stmt = tx.prepare(
                "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
            )?;
            let current_subtasks = stmt
                .query_map([id], |r| {
                    Ok(SubtaskSpec {
                        subtask_ref: r.get(0)?,
                        title: r.get(1)?,
                        status: r.get(2)?,
                        evidence: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let final_subtasks = if !current_subtasks.is_empty() {
                current_subtasks
            } else {
                task.spec.subtasks.clone()
            };
            let written = crate::engine::milestones::render_task_receipt(
                &text,
                task_ref,
                &receipt,
                &final_subtasks,
            )?;
            task.spec.subtasks = final_subtasks;
            document_update = Some((path, text, written));
            task.receipt = Some(receipt);
            task.completed_at = Some(now());
            task.status = "completed".into();
        } else if action == "pass" {
            task.gate = gate_index + 1;
            task.stage = profile.gate_id(task.gate).to_owned();
            task.status = "ready".into();
        } else {
            let findings = findings.context("reject requires findings")?;
            tx.execute(
                "INSERT INTO task_findings VALUES(?1,?2,?3)",
                params![id, task.claim_revision, serde_json::to_string(findings)?],
            )?;
            if let Some(target) = gate_def.reject_to.as_deref() {
                task.gate = profile.index_of(target).with_context(|| {
                    format!("TASK_STAGE_INVALID: task '{id}'. reject target stage '{target}' not defined in profile. Do: set reject_to to a stage that exists in the profile.")
                })?;
                task.stage = target.to_owned();
            }
            task.status = "ready".into();
        }
        let mut provisional_rollback = None;
        let pre_persist = (|| -> Result<()> {
            if let Some((path, original, written)) = &document_update {
                provisional_rollback = Some(ProvisionalReceiptRollback {
                    path: path.clone(),
                    original: original.clone(),
                    armed: true,
                });
                crate::core::fs::atomic_replace(path, written.as_bytes())?;
            }
            let created_delivery = before_persist(&task)?;
            match (auto_delivery.as_ref(), created_delivery) {
                (Some((candidate, candidate_baseline, path)), Some(landed)) => {
                    let worktree = Path::new(&evidence.worktree);
                    let relative = path.strip_prefix(worktree).context(
                        "TASK_DELIVERY_COMMIT_INVALID: receipt path is outside the worktree",
                    )?;
                    let written = document_update
                        .as_ref()
                        .map(|(_, _, written)| written.as_str())
                        .context(
                            "TASK_DELIVERY_COMMIT_INVALID: provisional receipt document is missing",
                        )?;
                    validate_delivery_commit(
                        worktree,
                        candidate,
                        candidate_baseline,
                        &task.spec.scope,
                        &landed,
                        relative,
                        written,
                    )?;
                    evidence.commit = Some(landed.clone());
                    let receipt = task
                        .receipt
                        .as_mut()
                        .context("TASK_RECEIPT_INVALID: delivery receipt is missing")?;
                    receipt.commit = Some(landed);
                    validate_durable_receipt_with_profile(
                        receipt,
                        task.contract_revision,
                        &profile,
                    )?;
                }
                (Some(_), None) => {
                    bail!("TASK_DELIVERY_COMMIT_REQUIRED: auto_commit delivery must create a hooked commit");
                }
                (None, Some(_)) => {
                    bail!("TASK_DELIVERY_COMMIT_INVALID: unexpected commit result for this gate");
                }
                (None, None) => {}
            }
            Ok(())
        })();
        if let Err(error) = pre_persist {
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback
                    .restore()
                    .context("failed to restore milestone document after delivery failure")?;
            }
            return Err(error);
        }
        tx.execute(
            "INSERT INTO task_gates VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                evidence.stage,
                task.claim_revision,
                if action == "pass" {
                    "passed"
                } else {
                    "rejected"
                },
                serde_json::to_string(evidence)?
            ],
        )?;
        end_claim(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        save(&tx, &task)?;
        let submission = Submission {
            evidence: evidence.clone(),
            action: action.into(),
            findings: findings.cloned(),
            task: task.clone(),
        };
        tx.execute(
            "INSERT INTO task_submissions VALUES(?1,?2,?3)",
            params![
                id,
                evidence.claim_revision,
                serde_json::to_string(&submission)?
            ],
        )?;
        if task.status == "completed" {
            clear_blackboard(&tx, id)?;
        }
        if let Some((path, _, written)) = &document_update {
            crate::core::fs::atomic_replace(path, written.as_bytes())?;
        }
        if let Err(error) = tx.commit() {
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback
                    .restore()
                    .context("failed to restore milestone document after SQLite commit error")?;
            } else if let Some((path, original, _)) = &document_update {
                crate::core::fs::atomic_replace(path, original.as_bytes())
                    .context("failed to restore milestone document after SQLite commit error")?;
            }
            return Err(error.into());
        }
        if let Some(rollback) = provisional_rollback.as_mut() {
            rollback.disarm();
        }
        Ok(task)
    }
}

#[derive(Serialize, Deserialize)]
struct Submission {
    evidence: Evidence,
    action: String,
    findings: Option<serde_json::Value>,
    task: Task,
}

fn accepted(conn: &rusqlite::Connection, id: &str, gate: &str) -> Result<Evidence> {
    let json: String = conn.query_row("SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",params![id,gate],|r|r.get(0))?;
    Ok(serde_json::from_str(&json)?)
}

fn candidate_snapshot(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::core::tasks::profile::GateProfile,
    gate_index: usize,
) -> Result<Option<String>> {
    let Some(snapshot_gate) = profile.gates[..gate_index]
        .iter()
        .rev()
        .find(|gate| gate.sha_snapshot)
    else {
        return Ok(None);
    };
    let serialized: Option<String> = conn
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",
            params![id, snapshot_gate.id],
            |row| row.get(0),
        )
        .optional()?;
    serialized
        .map(|json| {
            let evidence: Evidence = serde_json::from_str(&json)?;
            Ok(evidence.commit)
        })
        .transpose()
        .map(Option::flatten)
}

fn validate_receipt(
    receipt: &Receipt,
    task: &Task,
    build: &Evidence,
    review: &serde_json::Value,
    expected_commit: Option<&str>,
    profile: &crate::core::tasks::profile::GateProfile,
) -> Result<()> {
    validate_durable_receipt_with_profile(receipt, task.contract_revision, profile)?;
    if receipt.commit.as_deref() != expected_commit
        || receipt.contract_revision != task.contract_revision
        || receipt.evidence != build.proof
        || &receipt.review != review
        || receipt.decision != "pass"
        || chrono::DateTime::parse_from_rfc3339(&receipt.passed_at).is_err()
        || receipt
            .rollup
            .as_ref()
            .is_some_and(|rollup| rollup.verified_invariants != task.applicable_invariants)
    {
        bail!("TASK_RECEIPT_INVALID: accepted proof mismatch");
    }
    Ok(())
}
