use super::{Receipt, ReviewSummary};
use crate::tasks::profile::{CommandDef, ProofDef, ProofKind};
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
    #[serde(default, rename = "tests_passed")]
    tests_passed: Option<u64>,
    #[serde(default, rename = "tests_failed")]
    tests_failed: Option<u64>,
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
struct SeamTestProof {
    exit_code: i32,
    seam_test_refs: Vec<String>,
    tests_passed: Option<u64>,
    tests_failed: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewProof {
    decision: String,
    contours: std::collections::BTreeMap<String, ContourProof>,
    #[serde(default)]
    extended_scope_review: Option<ExtendedScopeReview>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtendedScopeReview {
    approved_paths: Vec<String>,
    reason_verified: bool,
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
    gate: &crate::tasks::profile::GateDef,
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
        let allows_zero = zero_exit.iter().any(|policy| policy == proof_policy);
        if proof.red_exit_code <= 0 {
            if allows_zero {
                if proof.red_exit_code < 0 {
                    bail!("TASK_EVIDENCE_INVALID: exit code must be non-negative");
                }
            } else {
                bail!("TASK_EVIDENCE_INVALID: red seam test and nonzero exit code required");
            }
        } else if let Some(expected) = &gate.expected_failure {
            if !expected.exit_codes.is_empty()
                && !expected.exit_codes.contains(&proof.red_exit_code)
            {
                bail!(
                    "TASK_EVIDENCE_INVALID: contract exit code {} does not match expected_failure {:?}",
                    proof.red_exit_code,
                    expected.exit_codes
                );
            }
        }
    }
    Ok(())
}

fn command_matches(proof_cmd: &str, expected_cmd: &str) -> bool {
    let p = proof_cmd.trim();
    let e = expected_cmd.trim();
    if p == e {
        return true;
    }
    if let Some(rest) = p.strip_prefix(e) {
        return rest.starts_with(' ') || rest.starts_with('\t');
    }
    false
}

fn validate_command(
    value: &serde_json::Value,
    passed: bool,
    commands: &std::collections::BTreeMap<String, CommandDef>,
) -> Result<()> {
    let proof: CommandProof =
        serde_json::from_value(proof_payload(value, "command_proof")?.clone())
            .context("TASK_EVIDENCE_INVALID: typed command proof required")?;
    let registered = commands.iter().find(|(name, definition)| {
        proof.command == **name
            || command_matches(&proof.command, definition.command())
            || matches!(
                definition,
                CommandDef::Pattern(patterns)
                    if patterns
                        .by_pattern
                        .values()
                        .any(|cmd| command_matches(&proof.command, cmd))
            )
    });
    let Some((_, definition)) = registered else {
        bail!(
            "TASK_EVIDENCE_INVALID: command '{}' is not registered in the active profile",
            proof.command
        );
    };
    let require_counts = definition
        .output()
        .is_none_or(|output| output.require_counts);
    let has_counts = proof.tests_passed.is_some() || proof.tests_failed.is_some();
    if passed {
        if proof.command.trim().is_empty() || proof.exit_code != 0 {
            bail!("TASK_EVIDENCE_INVALID: passing command required");
        }
        if (require_counts || has_counts)
            && (proof.tests_passed.is_none() || proof.tests_failed.is_none())
        {
            bail!("TASK_EVIDENCE_INVALID: passing command requires complete test counts");
        }
        if (require_counts || has_counts)
            && (proof.tests_passed.unwrap_or_default() == 0
                || proof.tests_failed.unwrap_or_default() != 0)
        {
            bail!(
                "TASK_EVIDENCE_INVALID: passing command requires passing tests and zero failures"
            );
        }
    }
    if proof.log.as_ref().is_some_and(|log| log.len() > 64 * 1024) {
        bail!("TASK_EVIDENCE_INVALID: test log exceeds 64 KiB");
    }
    Ok(())
}

fn validate_seam_test(value: &serde_json::Value, passed: bool) -> Result<()> {
    let proof: SeamTestProof =
        serde_json::from_value(proof_payload(value, "seam_test_proof")?.clone())
            .context("TASK_EVIDENCE_INVALID: typed seam test proof required")?;
    if passed {
        if proof.exit_code != 0 {
            bail!("TASK_EVIDENCE_INVALID: seam test command must pass");
        }
        if proof.seam_test_refs.is_empty()
            || proof
                .seam_test_refs
                .iter()
                .any(|reference| reference.trim().is_empty())
        {
            bail!("TASK_EVIDENCE_INVALID: at least one seam_test_ref is required");
        }
        if proof.tests_failed.unwrap_or_default() != 0 {
            bail!("TASK_EVIDENCE_INVALID: seam test proof reports failures");
        }
        if proof.tests_passed.unwrap_or_default() == 0 {
            bail!("TASK_EVIDENCE_INVALID: seam test proof requires a passing test");
        }
    }
    Ok(())
}

pub(crate) fn validate_observed_seam_test(
    supplied: &serde_json::Value,
    actual: &serde_json::Value,
) -> Result<()> {
    let supplied: SeamTestProof = serde_json::from_value(supplied.clone())?;
    let actual: SeamTestProof = serde_json::from_value(actual.clone())?;
    if supplied
        .seam_test_refs
        .iter()
        .any(|reference| !actual.seam_test_refs.contains(reference))
    {
        bail!("TASK_EVIDENCE_INVALID: every seam reference must identify an individually passed test in the claim-bound execution");
    }
    Ok(())
}

#[cfg(test)]
mod command_proof_tests {
    use super::validate_command;
    use crate::tasks::profile::{CommandCountsSpec, CommandDef, CommandOutputSpec};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn command(require_counts: bool) -> BTreeMap<String, CommandDef> {
        BTreeMap::from([(
            "check".into(),
            CommandDef::Detailed {
                command: "cargo test".into(),
                timeout_seconds: None,
                description: None,
                scope_args_by_pattern: BTreeMap::new(),
                output: Some(CommandOutputSpec {
                    counts: Some(CommandCountsSpec {
                        passed: r"(?m)^test result: ok\. (\d+) passed".into(),
                        failed: r"(?m)^test result: .*; (\d+) failed".into(),
                    }),
                    require_counts,
                    noise: Vec::new(),
                    errors: Vec::new(),
                    seam_ref_pattern: None,
                    tests: None,
                }),
            },
        )])
    }

    fn proof(counts: Option<(u64, u64)>) -> serde_json::Value {
        let mut proof = json!({"command":"check", "exit_code":0});
        if let Some((passed, failed)) = counts {
            proof["tests_passed"] = json!(passed);
            proof["tests_failed"] = json!(failed);
        }
        json!({"command_proof":proof})
    }

    #[test]
    fn passing_command_rejects_reported_test_failures() {
        let error = validate_command(&proof(Some((10, 1))), true, &command(false)).unwrap_err();

        assert!(error.to_string().contains("zero failures"));
    }

    #[test]
    fn passing_command_rejects_missing_required_counts() {
        let error = validate_command(&proof(None), true, &command(true)).unwrap_err();

        assert!(error.to_string().contains("complete test counts"));
    }

    #[test]
    fn passing_command_allows_count_free_linter_proofs() {
        validate_command(&proof(None), true, &command(false)).unwrap();
    }
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
    if let Some(scope_review) = &proof.extended_scope_review {
        if !scope_review.reason_verified {
            bail!("TASK_EVIDENCE_INVALID: extended_scope_review must verify the scope-addition reason");
        }
        let mut paths = std::collections::BTreeSet::new();
        if scope_review
            .approved_paths
            .iter()
            .any(|path| path.trim().is_empty() || !paths.insert(path))
        {
            bail!(
                "TASK_EVIDENCE_INVALID: extended_scope_review paths must be non-empty and unique"
            );
        }
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
    gate: &crate::tasks::profile::GateDef,
    profile: &crate::tasks::profile::GateProfile,
    passed: bool,
    proof_policy: &str,
) -> Result<()> {
    let mut catalog_schema_validated = false;
    if let Some(kind) = gate.proof.kind() {
        if let Some(catalog_entry) = profile.proofs.get(kind.as_str()) {
            if let Some(schema) = &catalog_entry.schema {
                let key = format!("{}_proof", kind.as_str());
                if passed || matches!(kind, ProofKind::Review) {
                    if schema.properties.contains_key(&key) || schema.required.contains(&key) {
                        schema.validate_value(evidence, "$")?;
                    } else {
                        let payload = proof_payload(evidence, &key)?;
                        schema.validate_value(payload, &format!("$.{key}"))?;
                    }
                    catalog_schema_validated = true;
                }
            }
        }
    }
    match &gate.proof {
        ProofDef::Kind(ProofKind::Contract) => validate_contract(
            evidence,
            gate,
            proof_policy,
            passed,
            &profile.contract_zero_exit_policies(),
        ),
        ProofDef::Kind(ProofKind::Command) => validate_command(evidence, passed, &profile.commands),
        ProofDef::Kind(ProofKind::SeamTest) => validate_seam_test(evidence, passed),
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
        ProofDef::Kind(ProofKind::None) | ProofDef::Unspecified => {
            if !catalog_schema_validated {
                validate_none(evidence)?;
            }
            Ok(())
        }
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
    let profile = crate::tasks::profile::compiled();
    validate_durable_receipt_with_profile(receipt, revision, &profile)
}

/// Validate a durable receipt using the profile that governed its delivery gate.
pub fn validate_durable_receipt_with_profile(
    receipt: &Receipt,
    revision: u64,
    profile: &crate::tasks::profile::GateProfile,
) -> Result<()> {
    let deliveries = profile.executable_gate_paths();
    if receipt.candidate_gate.is_none()
        && deliveries
            .iter()
            .filter(|(_, gate)| gate.proof.kind() == Some(ProofKind::Delivery))
            .count()
            > 1
    {
        bail!("TASK_RECEIPT_INVALID: multi-track receipt must identify its candidate gate");
    }
    let mut matching = deliveries.iter().enumerate().filter(|(_, (path, gate))| {
        gate.proof.kind() == Some(ProofKind::Delivery)
            && receipt
                .delivery_gate
                .as_ref()
                .is_none_or(|selected| selected == path)
    });
    let (delivery_index, _) = matching
        .next()
        .context("TASK_RECEIPT_INVALID: selected delivery gate is missing")?;
    if matching.next().is_some() {
        bail!("TASK_RECEIPT_INVALID: receipt must identify its delivery gate in a multi-track profile");
    }
    let (delivery_path, delivery) = &deliveries[delivery_index];
    let snapshot_gate = deliveries[..delivery_index]
        .iter()
        .rev()
        .filter(|(path, gate)| {
            gate.sha_snapshot
                && receipt
                    .candidate_gate
                    .as_ref()
                    .is_none_or(|selected| selected == path)
        })
        .find(|(_, gate)| {
            validate_gate_proof(&receipt.evidence, gate, profile, true, "receipt").is_ok()
        })
        .or_else(|| {
            deliveries[..delivery_index]
                .iter()
                .rev()
                .find(|(path, gate)| {
                    gate.sha_snapshot
                        && receipt
                            .candidate_gate
                            .as_ref()
                            .is_none_or(|selected| selected == path)
                })
        })
        .map(|(_, gate)| *gate)
        .context("TASK_RECEIPT_INVALID: profile has no candidate snapshot gate")?;
    validate_gate_proof(&receipt.evidence, snapshot_gate, profile, true, "receipt")
        .context("TASK_RECEIPT_INVALID: candidate evidence is invalid")?;
    let selected_reviews = delivery
        .review_sources
        .iter()
        .map(|reference| {
            let mut scope = delivery_path.rsplit_once('.').map(|(scope, _)| scope);
            if reference.contains('.') && deliveries.iter().any(|(path, _)| path == reference) {
                return Ok(reference.clone());
            }
            while let Some(owner) = scope {
                let local = format!("{owner}.{reference}");
                if deliveries.iter().any(|(path, _)| path == &local) {
                    return Ok(local);
                }
                scope = owner.rsplit_once('.').map(|(parent, _)| parent);
            }
            if deliveries.iter().any(|(path, _)| path == reference) {
                return Ok(reference.clone());
            }
            let mut matches = deliveries.iter().filter(|(path, _)| {
                path == reference || path.rsplit('.').next() == Some(reference.as_str())
            });
            let selected = matches
                .next()
                .context("TASK_RECEIPT_INVALID: referenced review gate is missing")?;
            anyhow::ensure!(
                matches.next().is_none(),
                "TASK_RECEIPT_INVALID: referenced review gate is ambiguous"
            );
            Ok(selected.0.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    let review_sources = review_source_proofs(&receipt.review, &selected_reviews)?;
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
