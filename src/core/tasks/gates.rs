use super::{Receipt, ReviewSummary};
use crate::core::tasks::profile::{CommandDef, ProofDef, ProofKind};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

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
    let proof: CommandProof =
        serde_json::from_value(proof_payload(value, "command_proof")?.clone())
            .context("TASK_EVIDENCE_INVALID: typed command proof required")?;
    let registered = commands.iter().any(|(name, definition)| {
        let command = definition.command().trim_end();
        proof.command == *name
            || proof.command == command
            || proof
                .command
                .strip_prefix(command)
                .and_then(|suffix| suffix.chars().next())
                .is_some_and(char::is_whitespace)
    });
    if !registered {
        bail!(
            "TASK_EVIDENCE_INVALID: command '{}' is not registered in the active profile",
            proof.command
        );
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

pub(crate) fn validate_gate_proof(
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
        ProofDef::Kind(ProofKind::Command) => validate_command(evidence, passed, &profile.commands),
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

fn review_source_proofs(
    value: &serde_json::Value,
    expected: &[String],
) -> Result<Vec<(String, serde_json::Value)>> {
    if let Some(sources) = value
        .get("review_sources")
        .and_then(serde_json::Value::as_object)
    {
        if sources.len() != expected.len()
            || expected.iter().any(|name| !sources.contains_key(name))
        {
            bail!("TASK_RECEIPT_INVALID: review source rollup does not match the delivery profile");
        }
        return expected
            .iter()
            .map(|name| {
                sources
                    .get(name)
                    .cloned()
                    .map(|proof| (name.clone(), proof))
                    .context("TASK_RECEIPT_INVALID: missing review source proof")
            })
            .collect();
    }
    if expected.len() != 1 {
        bail!("TASK_RECEIPT_INVALID: multiple review sources require a source-keyed rollup");
    }
    Ok(vec![(expected[0].clone(), value.clone())])
}

pub(crate) fn review_rollup_summary(
    sources: &[(String, serde_json::Value)],
) -> Result<ReviewSummary> {
    let mut contours = std::collections::BTreeMap::new();
    for (source, evidence) in sources {
        let proof: ReviewProof =
            serde_json::from_value(proof_payload(evidence, "review_proof")?.clone())
                .context("TASK_RECEIPT_INVALID: typed review proof required")?;
        let summary = review_summary(&proof);
        for (name, disposition) in summary.contours {
            let key = if sources.len() == 1 {
                name
            } else {
                format!("{source}.{name}")
            };
            contours.insert(key, disposition);
        }
    }
    Ok(ReviewSummary {
        decision: "pass".into(),
        contours,
    })
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
    validate_gate_proof(&receipt.evidence, snapshot_gate, profile, true, "receipt")
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
