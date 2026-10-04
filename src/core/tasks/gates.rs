use super::{Milestone, Receipt, ReceiptRollup, ReviewSummary, SubtaskSpec, GATES};
use crate::db::tasks_store::{check_stage, clear_blackboard, end_claim, load, now, save, Task, TasksStore};
use anyhow::{bail, Context, Result};
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    /// The commit value.
    pub commit: String,
    /// The proof value.
    pub proof: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestProof {
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

fn validate_contract(value: &serde_json::Value, proof_policy: &str, passed: bool) -> Result<()> {
    let proof: ContractProof = serde_json::from_value(proof_payload(value, "contract_proof")?.clone())
        .context("TASK_EVIDENCE_INVALID: typed contract proof required")?;
    if passed {
        if proof.seam_test_ref.trim().is_empty() {
            bail!("TASK_EVIDENCE_INVALID: seam test reference required");
        }
        if proof_policy == "direct-proof" || proof_policy == "deferred-final-test" {
            if proof.red_exit_code < 0 {
                bail!("TASK_EVIDENCE_INVALID: exit code must be non-negative");
            }
        } else if proof.red_exit_code <= 0 {
            bail!("TASK_EVIDENCE_INVALID: red seam test and nonzero exit code required");
        }
    }
    Ok(())
}

fn validate_tests(value: &serde_json::Value, passed: bool) -> Result<()> {
    let proof: TestProof = serde_json::from_value(proof_payload(value, "test_proof")?.clone())
        .context("TASK_EVIDENCE_INVALID: typed test proof required")?;
    if passed && (proof.command.trim().is_empty()
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

fn validate_review(value: &serde_json::Value, passed: bool) -> Result<()> {
    let proof: ReviewProof = serde_json::from_value(proof_payload(value, "review_proof")?.clone())
        .context("TASK_EVIDENCE_INVALID: typed review proof required")?;
    let expected_decision = if passed { "pass" } else { "reject" };
    if proof.decision != expected_decision {
        bail!("TASK_EVIDENCE_INVALID: review decision must match action");
    }
    validate_review_contours(&proof.contours)
}

fn validate_review_contours(
    contours: &std::collections::BTreeMap<String, ContourProof>,
) -> Result<()> {
    if contours.len() != REVIEW_CONTOURS.len()
        || REVIEW_CONTOURS
            .iter()
            .any(|name| !contours.contains_key(*name))
    {
        bail!("TASK_EVIDENCE_INVALID: five admitted review contours required");
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

fn review_summary(proof: &ReviewProof) -> ReviewSummary {
    ReviewSummary {
        decision: proof.decision.clone(),
        contours: proof.contours.iter().map(|(name, contour)| {
            (name.clone(), if contour.applicable { "accepted" } else { "not_applicable" }.into())
        }).collect(),
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
        self.assert_id(id)?;
        if evidence.stage != stage && evidence.stage.split('/').next() != Some(stage) {
            bail!("TASK_STAGE_INVALID");
        }
        if !matches!(action, "pass" | "reject") {
            bail!("invalid submission action");
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = load(&tx, id)?.context("TASK_NOT_FOUND")?;
        let mut document_update = None;
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
                return Ok(accepted.task);
            }
            bail!("TASK_STALE_SUBMISSION");
        }
        check_stage(&task, stage)?;
        if task.status != "in_progress"
            || task.claim_revision != evidence.claim_revision
            || task.contract_revision != evidence.contract_revision
            || task.task_id != evidence.task_id
            || task.worker_id.as_deref() != Some(&evidence.worker_id)
            || task.worktree.as_deref() != Some(&evidence.worktree)
            || evidence.stage != GATES[task.gate]
        {
            bail!("TASK_STALE_SUBMISSION");
        }
        if evidence.commit.trim().is_empty() || evidence.proof.is_null() {
            bail!("TASK_EVIDENCE_INVALID");
        }
        match task.gate {
            0 => validate_contract(&evidence.proof, &task.spec.proof_policy, action == "pass")?,
            1 => validate_tests(&evidence.proof, action == "pass")?,
            2 => validate_review(&evidence.proof, action == "pass")?,
            _ => {}
        }
        if task.gate >= 2 {
            let builder: String = tx.query_row("SELECT json_extract(evidence,'$.worker_id') FROM task_gates WHERE task_id=?1 AND gate='build/v1' AND state='passed' ORDER BY revision DESC LIMIT 1",[id],|r|r.get(0))?;
            if builder == evidence.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT");
            }
            let build = accepted(&tx, id, "build/v1")?;
            if build.commit != evidence.commit {
                bail!("TASK_CANDIDATE_MISMATCH");
            }
        }
        if task.gate == 3 && action == "pass" {
            let build = accepted(&tx, id, "build/v1")?;
            let review = accepted(&tx, id, "review/v1")?;
            let claim_worktree: String = tx.query_row(
                "SELECT worktree FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![id, task.claim_revision], |row| row.get(0))?;
            let path = super::confined_path(Path::new(&claim_worktree), &task.milestone_ref)?;
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
            let review_proof: ReviewProof = serde_json::from_value(
                proof_payload(&review.proof, "review_proof")?.clone(),
            )?;
            let notes = tx.prepare(
                "SELECT payload FROM task_blackboard WHERE task_id=?1 AND topic='architectural_notes' ORDER BY created_at,id",
            )?.query_map([id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let rollup = ReceiptRollup {
                verified_invariants: task.applicable_invariants.clone(),
                architectural_notes: notes,
                review_summary: review_summary(&review_proof),
            };
            let generated = Receipt {
                commit: build.commit.clone(),
                contract_revision: task.contract_revision,
                passed_at: chrono::Utc::now().to_rfc3339(),
                evidence: build.proof.clone(),
                review: review.proof.clone(),
                decision: "pass".into(),
                rollup: Some(rollup),
            };
            let receipt = match (spec.status.as_deref(), spec.receipt.as_ref()) {
                (Some("completed"), Some(existing)) => {
                    // The document may have been replaced immediately before a process crash.
                    // Reuse only the exact accepted outcome, retaining its original timestamp.
                    validate_receipt(existing, &task, &build, &review)?;
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
            validate_receipt(&receipt, &task, &build, &review)?;
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
            let written = crate::engine::milestones::render_task_receipt(&text, task_ref, &receipt, &final_subtasks)?;
            task.spec.subtasks = final_subtasks;
            document_update = Some((path, text, written));
            task.receipt = Some(receipt);
            task.completed_at = Some(now());
            task.status = "completed".into();
        } else if action == "pass" {
            task.gate += 1;
            task.status = "ready".into();
        } else {
            let findings = findings.context("reject requires findings")?;
            tx.execute(
                "INSERT INTO task_findings VALUES(?1,?2,?3)",
                params![id, task.claim_revision, serde_json::to_string(findings)?],
            )?;
            task.gate = task.gate.min(1);
            task.status = "ready".into();
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
            if let Some((path, original, _)) = &document_update {
                crate::engine::milestones::atomic_replace(path, original.as_bytes())
                    .context("failed to restore milestone document after SQLite commit error")?;
            }
            return Err(error.into());
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
    review: &Evidence,
) -> Result<()> {
    validate_durable_receipt(receipt, task.contract_revision)?;
    if receipt.commit != build.commit
        || review.commit != build.commit
        || receipt.contract_revision != task.contract_revision
        || receipt.evidence != build.proof
        || receipt.review != review.proof
        || receipt.decision != "pass"
        || chrono::DateTime::parse_from_rfc3339(&receipt.passed_at).is_err()
        || receipt.rollup.as_ref().is_some_and(|rollup| rollup.verified_invariants != task.applicable_invariants)
    {
        bail!("TASK_RECEIPT_INVALID: accepted proof mismatch");
    }
    Ok(())
}

/// Performs validate durable receipt.
pub fn validate_durable_receipt(receipt: &Receipt, revision: u64) -> Result<()> {
    validate_tests(&receipt.evidence, true)?;
    validate_review(&receipt.review, true)?;
    if let Some(rollup) = &receipt.rollup {
        let proof: ReviewProof = serde_json::from_value(
            proof_payload(&receipt.review, "review_proof")?.clone(),
        )?;
        if rollup.review_summary != review_summary(&proof)
            || rollup.verified_invariants.iter().any(|invariant| invariant.trim().is_empty())
        {
            bail!("TASK_RECEIPT_INVALID: malformed durable rollup");
        }
    }
    if !matches!(receipt.commit.len(), 40 | 64)
        || !receipt.commit.bytes().all(|c| c.is_ascii_hexdigit())
        || receipt.contract_revision != revision
        || receipt.evidence.is_null()
        || receipt.review.is_null()
        || receipt.decision != "pass"
        || chrono::DateTime::parse_from_rfc3339(&receipt.passed_at).is_err()
    {
        bail!("TASK_RECEIPT_INVALID: malformed durable proof");
    }
    Ok(())
}
