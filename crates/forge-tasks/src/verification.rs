//! Proof policy definitions, verification gates, and evidence templates.

use serde::{Deserialize, Serialize};

/// Presentation chosen for a command verification diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyDiagnosticMode {
    /// The command passed and only a compact summary is needed.
    Pass,
    /// The full diagnostic fits in the response.
    Inline,
    /// The diagnostic was truncated and the raw log is retained.
    Truncated,
    /// The command exceeded its execution timeout.
    Timeout,
}

/// A path found outside the active task's admitted write scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeViolationAlert {
    /// Project-relative path reported by Git.
    pub path: String,
}

/// Result of a command check without a task lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyOutcome {
    /// Command exit code, absent when the process did not exit normally.
    pub exit_code: Option<i32>,
    /// Diagnostic presentation mode.
    pub diagnostic_mode: VerifyDiagnosticMode,
    /// Compact summary or diagnostic excerpt.
    pub output: String,
    /// Optional raw log location for truncated output.
    pub log_path: Option<String>,
    /// Paths outside the admitted write scope observed before or after execution.
    pub scope_violations: Vec<ScopeViolationAlert>,
}

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
