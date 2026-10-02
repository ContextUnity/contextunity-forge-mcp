use super::{Milestone, Receipt, GATES};
use crate::db::tasks_store::{check_stage, end_claim, load, now, save, Task, TasksStore};
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
    result: String,
    artifacts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewProof {
    decision: String,
    evidence_ref: String,
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

fn validate_tests(value: &serde_json::Value) -> Result<()> {
    let proof: TestProof = serde_json::from_value(value.clone())
        .context("TASK_EVIDENCE_INVALID: typed test proof required")?;
    if proof.command.trim().is_empty()
        || proof.result != "passed"
        || proof.artifacts.iter().any(|a| a.trim().is_empty())
    {
        bail!("TASK_EVIDENCE_INVALID: passing test command and artifacts required");
    }
    Ok(())
}

fn validate_review(value: &serde_json::Value) -> Result<()> {
    let proof: ReviewProof = serde_json::from_value(value.clone())
        .context("TASK_EVIDENCE_INVALID: typed review proof required")?;
    if proof.decision != "pass"
        || proof.evidence_ref.trim().is_empty()
        || proof.contours.len() != REVIEW_CONTOURS.len()
        || REVIEW_CONTOURS
            .iter()
            .any(|name| !proof.contours.contains_key(*name))
    {
        bail!("TASK_EVIDENCE_INVALID: five admitted review contours required");
    }
    for contour in proof.contours.values() {
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
        if action == "pass" && task.gate == 2 {
            validate_tests(&evidence.proof)?;
        }
        if action == "pass" && task.gate == 3 {
            validate_review(&evidence.proof)?;
        }
        if task.gate >= 3 {
            let builder: String = tx.query_row("SELECT json_extract(evidence,'$.worker_id') FROM task_gates WHERE task_id=?1 AND gate='build/v1' AND state='passed' ORDER BY revision DESC LIMIT 1",[id],|r|r.get(0))?;
            if task.gate == 3 && builder == evidence.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT");
            }
            let build = accepted(&tx, id, "build/v1")?;
            if build.commit != evidence.commit {
                bail!("TASK_CANDIDATE_MISMATCH");
            }
        }
        if task.gate == 4 && action == "pass" {
            let build = accepted(&tx, id, "build/v1")?;
            let review = accepted(&tx, id, "review/v1")?;
            let claim_worktree: String = tx.query_row(
                "SELECT worktree FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![id, task.claim_revision], |row| row.get(0))?;
            let path = super::confined_path(Path::new(&claim_worktree), &task.milestone_ref)?;
            let text = std::fs::read_to_string(path)?;
            let mut identity = id.split('/');
            let repository = identity.next().context("invalid task identity")?;
            let project = identity.next().context("invalid task identity")?;
            let milestone = Milestone::parse_with_identity(&text, repository, project)?;
            let spec = milestone
                .tasks
                .iter()
                .find(|s| milestone.task_id(s) == id)
                .context("TASK_RECEIPT_INVALID: task not found")?;
            if milestone.digest(spec)? != task.digest || spec.status.as_deref() != Some("completed")
            {
                bail!("TASK_RECEIPT_INVALID: specification mismatch");
            }
            let receipt = spec
                .receipt
                .as_ref()
                .context("TASK_RECEIPT_INVALID: missing receipt")?;
            validate_receipt(receipt, &task, &build, &review)?;
            task.receipt = Some(receipt.clone());
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
            task.gate = task.gate.min(2);
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
        tx.commit()?;
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
    {
        bail!("TASK_RECEIPT_INVALID: accepted proof mismatch");
    }
    Ok(())
}

/// Performs validate durable receipt.
pub fn validate_durable_receipt(receipt: &Receipt, revision: u64) -> Result<()> {
    validate_tests(&receipt.evidence)?;
    validate_review(&receipt.review)?;
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
