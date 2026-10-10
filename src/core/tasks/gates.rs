use super::{Milestone, Receipt, ReceiptRollup, ReviewSummary, SubtaskSpec};
use crate::db::tasks_store::{
    check_stage, clear_blackboard, end_claim, load, now, save, Task, TasksStore,
};
use crate::core::tasks::profile::{CommandDef, ProofDef, ProofKind};
use anyhow::{bail, Context, Result};
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
/// Represents evidence data.
pub struct Evidence {
    /// The task id value.
    pub task_id: String,
    /// The stage value.
    pub stage: String,
    /// The claim revision value.
    pub claim_revision: u64,
    /// The contract revision value.
    pub contract_revision: u64,
    /// The worker id value.
    pub worker_id: String,
    /// The worktree value.
    pub worktree: String,
    /// The optional commit value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Baseline HEAD captured with a scoped candidate snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_baseline_head: Option<String>,
    /// The proof value.
    pub proof: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandProof {
    command: String,
    exit_code: i32,
    tests_passed: u64,
    tests_failed: u64,
    log: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractProof {
    seam_test_ref: String,
    red_exit_code: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewProof {
    decision: String,
    contours: std::collections::BTreeMap<String, ContourProof>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContourProof {
    applicable: bool,
    evidence: String,
}

/// The review contours value.
pub const REVIEW_CONTOURS: [&str; 5] = [
    "paths",
    "claims",
    "concurrency",
    "project_isolation",
    "administration",
];

fn proof_payload<'a>(
    value: &'a serde_json::Value,
    expected: &str,
) -> Result<&'a serde_json::Value> {
    let object = value
        .as_object()
        .context("TASK_EVIDENCE_INVALID: typed proof object required")?;
    if object.len() != 1 {
        bail!("TASK_EVIDENCE_INVALID: expected {expected}");
    }
    object
        .get(expected)
        .with_context(|| format!("TASK_EVIDENCE_INVALID: expected {expected}"))
}

fn validate_contract(
    value: &serde_json::Value,
    proof_policy: &str,
    passed: bool,
    zero_exit: &[String],
) -> Result<()> {
    let proof: ContractProof =
        serde_json::from_value(proof_payload(value, "contract_proof")?.clone())
            .context("TASK_EVIDENCE_INVALID: typed contract proof required")?;
    if passed {
        if proof.seam_test_ref.trim().is_empty() {
            bail!("TASK_EVIDENCE_INVALID: seam test reference required");
        }
        if zero_exit.iter().any(|policy| policy == proof_policy) {
            if proof.red_exit_code < 0 {
                bail!("TASK_EVIDENCE_INVALID: exit code must be non-negative");
            }
        } else if proof.red_exit_code <= 0 {
            bail!("TASK_EVIDENCE_INVALID: red seam test and nonzero exit code required");
        }
    }
    Ok(())
}

fn validate_command(
    value: &serde_json::Value,
    passed: bool,
    commands: &std::collections::BTreeMap<String, CommandDef>,
) -> Result<()> {
    let proof: CommandProof = serde_json::from_value(proof_payload(value, "command_proof")?.clone())
        .context("TASK_EVIDENCE_INVALID: typed command proof required")?;
    let registered = commands.iter().any(|(name, definition)| {
        let command = definition.command().trim_end();
        proof.command == *name
            || proof.command == command
            || proof.command.strip_prefix(command)
                .and_then(|suffix| suffix.chars().next())
                .is_some_and(char::is_whitespace)
    });
    if !registered {
        bail!("TASK_EVIDENCE_INVALID: command '{}' is not registered in the active profile", proof.command);
    }
    if passed
        && (proof.command.trim().is_empty()
            || proof.exit_code != 0
            || proof.tests_passed == 0
            || proof.tests_failed != 0)
    {
        bail!("TASK_EVIDENCE_INVALID: passing test command and counts required");
    }
    if proof.log.as_ref().is_some_and(|log| log.len() > 64 * 1024) {
        bail!("TASK_EVIDENCE_INVALID: test log exceeds 64 KiB");
    }
    Ok(())
}

fn validate_review(
    value: &serde_json::Value,
    passed: bool,
    contours_expected: &[String],
) -> Result<()> {
    let proof: ReviewProof = serde_json::from_value(proof_payload(value, "review_proof")?.clone())
        .context("TASK_EVIDENCE_INVALID: typed review proof required")?;
    let expected_decision = if passed { "pass" } else { "reject" };
    if proof.decision != expected_decision {
        bail!("TASK_EVIDENCE_INVALID: review decision must match action");
    }
    validate_review_contours(&proof.contours, contours_expected)
}

fn validate_none(value: &serde_json::Value) -> Result<()> {
    let proof = proof_payload(value, "none_proof")?;
    if !proof.as_object().is_some_and(serde_json::Map::is_empty) {
        bail!("TASK_EVIDENCE_INVALID: none_proof must be an empty object");
    }
    Ok(())
}

fn validate_review_contours(
    contours: &std::collections::BTreeMap<String, ContourProof>,
    expected: &[String],
) -> Result<()> {
    if contours.len() != expected.len() || expected.iter().any(|name| !contours.contains_key(name))
    {
        bail!(
            "TASK_EVIDENCE_INVALID: review_proof requires contours: {}",
            expected.join(", ")
        );
    }
    for contour in contours.values() {
        if contour.evidence.trim().is_empty() {
            bail!(
                "TASK_EVIDENCE_INVALID: missing {}",
                if contour.applicable {
                    "review evidence"
                } else {
                    "N/A rationale"
                }
            );
        }
    }
    Ok(())
}

fn commit_tree_and_parent(worktree: &Path, commit: &str) -> Result<(String, String)> {
    let output = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", commit])
        .current_dir(worktree)
        .output()?;
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
    let tree = std::process::Command::new("git")
        .args(["rev-parse", &treeish])
        .current_dir(worktree)
        .output()?;
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
    let mut command = std::process::Command::new("git");
    command
        .args(["ls-tree", "-r", "-z", "--full-tree", treeish])
        .current_dir(worktree);
    if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
        command.arg("--").args(scope);
    }
    let output = command.output()?;
    if !output.status.success() {
        bail!("TASK_DELIVERY_COMMIT_INVALID: could not read Git tree entries");
    }
    let mut entries = std::collections::BTreeMap::new();
    for record in output.stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
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
        bail!("TASK_CANDIDATE_INVALID: candidate snapshot contains a path outside its admitted scope");
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
    let committed_receipt = std::process::Command::new("git")
        .args(["show", &spec])
        .current_dir(worktree)
        .output()?;
    if !committed_receipt.status.success() || committed_receipt.stdout != expected_receipt.as_bytes() {
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
            crate::engine::milestones::atomic_replace(&self.path, self.original.as_bytes())?;
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
            let _ = crate::engine::milestones::atomic_replace(&self.path, self.original.as_bytes());
        }
    }
}

fn validate_gate_proof(
    evidence: &serde_json::Value,
    gate: &crate::core::tasks::profile::GateDef,
    profile: &crate::core::tasks::profile::GateProfile,
    passed: bool,
    proof_policy: &str,
) -> Result<()> {
    match &gate.proof {
        ProofDef::Kind(ProofKind::Contract) => validate_contract(
            evidence,
            proof_policy,
            passed,
            &profile.contract_zero_exit_policies,
        ),
        ProofDef::Kind(ProofKind::Command) => {
            validate_command(evidence, passed, &profile.commands)
        }
        ProofDef::Kind(ProofKind::Review) => {
            let contour_set = profile
                .contour_set(gate.contours.as_deref())
                .context("TASK_GATES_INVALID: review contour set missing")?;
            let expected = contour_set
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            validate_review(evidence, passed, &expected)
        }
        ProofDef::Kind(ProofKind::Delivery) => {
            proof_payload(evidence, "delivery_proof").map(|_| ())
        }
        ProofDef::Kind(ProofKind::None) => validate_none(evidence),
        ProofDef::Scheme { scheme } => {
            let value = proof_payload(evidence, "scheme_proof")?;
            scheme.validate_value(value, "$.scheme_proof")
        }
    }
}

fn review_summary(proof: &ReviewProof) -> ReviewSummary {
    ReviewSummary {
        decision: proof.decision.clone(),
        contours: proof
            .contours
            .iter()
            .map(|(name, contour)| {
                (
                    name.clone(),
                    if contour.applicable {
                        "accepted"
                    } else {
                        "not_applicable"
                    }
                    .into(),
                )
            })
            .collect(),
    }
}

fn review_source_proofs(value: &serde_json::Value, expected: &[String]) -> Result<Vec<(String, serde_json::Value)>> {
    if let Some(sources) = value.get("review_sources").and_then(serde_json::Value::as_object) {
        if sources.len() != expected.len() || expected.iter().any(|name| !sources.contains_key(name)) {
            bail!("TASK_RECEIPT_INVALID: review source rollup does not match the delivery profile");
        }
        return expected.iter().map(|name| {
            sources.get(name).cloned().map(|proof| (name.clone(), proof))
                .context("TASK_RECEIPT_INVALID: missing review source proof")
        }).collect();
    }
    if expected.len() != 1 {
        bail!("TASK_RECEIPT_INVALID: multiple review sources require a source-keyed rollup");
    }
    Ok(vec![(expected[0].clone(), value.clone())])
}

fn review_rollup_summary(sources: &[(String, serde_json::Value)]) -> Result<ReviewSummary> {
    let mut contours = std::collections::BTreeMap::new();
    for (source, evidence) in sources {
        let proof: ReviewProof = serde_json::from_value(proof_payload(evidence, "review_proof")?.clone())
            .context("TASK_RECEIPT_INVALID: typed review proof required")?;
        let summary = review_summary(&proof);
        for (name, disposition) in summary.contours {
            let key = if sources.len() == 1 { name } else { format!("{source}.{name}") };
            contours.insert(key, disposition);
        }
    }
    Ok(ReviewSummary { decision: "pass".into(), contours })
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
            bail!("TASK_STAGE_INVALID");
        }
        if !matches!(action, "pass" | "reject") {
            bail!("invalid submission action");
        }
        let observed_task = self.inspect(id)?;
        let profile = self.profile_for_task(&observed_task)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        if task.profile_pin != observed_task.profile_pin || task.stage != observed_task.stage {
            bail!("TASK_STALE_SUBMISSION");
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
            bail!("TASK_STALE_SUBMISSION");
        }
        let gate_index = profile.index_of(&task.stage).with_context(|| format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        ))?;
        task.gate = gate_index;
        let gate_def = profile.gates.get(gate_index).cloned().context("TASK_STAGE_UNKNOWN")?;
        check_stage(&gate_def.id, stage)?;
        if task.status != "in_progress"
            || task.claim_revision != evidence.claim_revision
            || task.contract_revision != evidence.contract_revision
            || task.task_id != evidence.task_id
            || task.worker_id.as_deref() != Some(&evidence.worker_id)
            || task.worktree.as_deref() != Some(&evidence.worktree)
            || evidence.stage != gate_def.id
        {
            bail!("TASK_STALE_SUBMISSION");
        }
        if evidence.proof.is_null()
            || evidence
                .commit
                .as_ref()
                .is_some_and(|c| c.trim().is_empty())
        {
            bail!("TASK_EVIDENCE_INVALID");
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
                bail!("TASK_REVIEW_NOT_INDEPENDENT");
            }
            let build = accepted(&tx, id, prior)?;
            if build.commit != evidence.commit {
                bail!("TASK_CANDIDATE_MISMATCH");
            }
        }
        if gate_def.proof.kind() == Some(ProofKind::Delivery) && action == "pass" {
            let snapshot_gate = profile.gates[..gate_index]
                .iter()
                .rev()
                .find(|gate| gate.sha_snapshot)
                .context("TASK_GATES_INVALID: delivery requires a prior sha_snapshot gate")?;
            let build = accepted(&tx, id, &snapshot_gate.id)?;
            let receipt_commit = evidence
                .commit
                .as_deref()
                .context("TASK_EVIDENCE_INVALID: delivery requires a commit")?;
            let candidate_baseline = build
                .candidate_baseline_head
                .as_deref()
                .context("TASK_CANDIDATE_BASELINE_INVALID: accepted candidate has no captured baseline")?;
            if evidence.candidate_baseline_head.as_deref() != Some(candidate_baseline) {
                bail!("TASK_CANDIDATE_BASELINE_MISMATCH: delivery baseline differs from the accepted candidate");
            }
            if !gate_def.auto_commit && build.commit.as_deref() != Some(receipt_commit) {
                bail!("TASK_CANDIDATE_MISMATCH: delivery commit must reference the accepted candidate snapshot");
            }
            let mut review_proofs = Vec::with_capacity(gate_def.review_sources.len());
            for review_name in &gate_def.review_sources {
                let review = accepted(&tx, id, review_name)?;
                if review.commit != build.commit {
                    bail!("TASK_CANDIDATE_MISMATCH: review source '{review_name}' does not match the candidate snapshot");
                }
                let review_gate = profile.by_stage(review_name).context("TASK_GATES_INVALID")?;
                validate_gate_proof(&review.proof, review_gate, &profile, true, &task.spec.proof_policy)?;
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
                    build.commit.as_deref().context("TASK_SNAPSHOT_NOT_FOUND")?.to_owned(),
                    candidate_baseline.to_owned(),
                    path.clone(),
                ));
            }
            let text = std::fs::read_to_string(&path)?;
            let mut identity = id.split('/');
            let repository = identity.next().context("invalid task identity")?;
            let project = identity.next().context("invalid task identity")?;
            let milestone = Milestone::parse_with_identity(&text, repository, project)?;
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
            let notes = tx.prepare(
                "SELECT payload FROM task_blackboard WHERE task_id=?1 AND topic='architectural_notes' ORDER BY created_at,id",
            )?.query_map([id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let rollup = ReceiptRollup {
                verified_invariants: task.applicable_invariants.clone(),
                architectural_notes: notes,
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
                    validate_receipt(existing, &task, &build, &review_value, document_commit.as_deref(), &profile)?;
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
            validate_receipt(&receipt, &task, &build, &review_value, document_commit.as_deref(), &profile)?;
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
                task.gate = profile
                    .index_of(target)
                    .context("TASK_STAGE_INVALID")?;
                task.stage = target.to_owned();
            }
            task.status = "ready".into();
        }
        let mut provisional_rollback = None;
        let pre_persist = (|| -> Result<()> {
            if auto_delivery.is_some() {
                if let Some((path, original, written)) = &document_update {
                    provisional_rollback = Some(ProvisionalReceiptRollback {
                        path: path.clone(),
                        original: original.clone(),
                        armed: true,
                    });
                    crate::engine::milestones::atomic_replace(path, written.as_bytes())?;
                }
            }
            let created_delivery = before_persist(&task)?;
            match (auto_delivery.as_ref(), created_delivery) {
                (Some((candidate, candidate_baseline, path)), Some(landed)) => {
                    let worktree = Path::new(&evidence.worktree);
                    let relative = path.strip_prefix(worktree)
                        .context("TASK_DELIVERY_COMMIT_INVALID: receipt path is outside the worktree")?;
                    let written = document_update.as_ref()
                        .map(|(_, _, written)| written.as_str())
                        .context("TASK_DELIVERY_COMMIT_INVALID: provisional receipt document is missing")?;
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
                    let receipt = task.receipt.as_mut()
                        .context("TASK_RECEIPT_INVALID: delivery receipt is missing")?;
                    receipt.commit = Some(landed);
                    validate_durable_receipt_with_profile(receipt, task.contract_revision, &profile)?;
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
                rollback.restore()
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
            crate::engine::milestones::atomic_replace(path, written.as_bytes())?;
        }
        if let Err(error) = tx.commit() {
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback.restore()
                    .context("failed to restore milestone document after SQLite commit error")?;
            } else if let Some((path, original, _)) = &document_update {
                crate::engine::milestones::atomic_replace(path, original.as_bytes())
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

use rusqlite::OptionalExtension;
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

/// Performs validate durable receipt.
pub fn validate_durable_receipt(receipt: &Receipt, revision: u64) -> Result<()> {
    let profile = crate::core::tasks::profile::compiled();
    validate_durable_receipt_with_profile(receipt, revision, &profile)
}

/// Validate a durable receipt using the profile that governed its delivery gate.
pub fn validate_durable_receipt_with_profile(
    receipt: &Receipt,
    revision: u64,
    profile: &crate::core::tasks::profile::GateProfile,
) -> Result<()> {
    let delivery_index = profile
        .gates
        .iter()
        .position(|gate| gate.proof.kind() == Some(ProofKind::Delivery))
        .context("TASK_RECEIPT_INVALID: profile has no delivery gate")?;
    let delivery = &profile.gates[delivery_index];
    let snapshot_gate = profile.gates[..delivery_index]
        .iter()
        .rev()
        .find(|gate| gate.sha_snapshot)
        .context("TASK_RECEIPT_INVALID: profile has no candidate snapshot gate")?;
    validate_gate_proof(
        &receipt.evidence,
        snapshot_gate,
        profile,
        true,
        "receipt",
    )
    .context("TASK_RECEIPT_INVALID: candidate evidence is invalid")?;
    let review_sources = review_source_proofs(&receipt.review, &delivery.review_sources)?;
    for (review_name, proof) in &review_sources {
        let review_gate = profile
            .by_stage(review_name)
            .context("TASK_RECEIPT_INVALID: referenced review gate is missing")?;
        validate_gate_proof(proof, review_gate, profile, true, "receipt")
            .context("TASK_RECEIPT_INVALID: review evidence is invalid")?;
    }
    if let Some(rollup) = &receipt.rollup {
        if rollup.review_summary != review_rollup_summary(&review_sources)?
            || rollup
                .verified_invariants
                .iter()
                .any(|invariant| invariant.trim().is_empty())
        {
            bail!("TASK_RECEIPT_INVALID: malformed durable rollup");
        }
    }
    if let Some(commit) = &receipt.commit {
        if !(7..=64).contains(&commit.len()) || !commit.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("TASK_RECEIPT_INVALID: malformed durable proof");
        }
    }
    if receipt.contract_revision != revision
        || receipt.evidence.is_null()
        || receipt.review.is_null()
        || receipt.decision != "pass"
        || chrono::DateTime::parse_from_rfc3339(&receipt.passed_at).is_err()
    {
        bail!("TASK_RECEIPT_INVALID: malformed durable proof");
    }
    Ok(())
}
