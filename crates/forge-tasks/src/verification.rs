//! Proof policy definitions, verification gates, and evidence templates.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// Admitted proof policy for task contracts.
pub enum ProofPolicy {
    /// Direct single production seam test proof.
    DirectProof,
    /// Multi-dimensional matrix parameter proof.
    MatrixProof,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
/// Evidence collected during gate verification.
pub struct Evidence {
    /// Exit code from verification command.
    pub exit_code: i32,
    /// Number of passed tests.
    pub tests_passed: usize,
    /// Number of failed tests.
    pub tests_failed: usize,
    /// Seam test reference paths.
    pub seam_test_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Outcome of an automated verification gate check.
pub struct VerificationResult {
    /// Whether the gate passed.
    pub passed: bool,
    /// Associated evidence.
    pub evidence: Evidence,
    /// Optional failure message or explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}
