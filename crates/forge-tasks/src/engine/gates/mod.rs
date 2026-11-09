use crate::store::Task;
use crate::tasks::gates::Evidence;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod proof;
mod scope;
mod seam;
mod submit;
#[cfg(test)]
mod tests;

use proof::{validate_delivery_commit, validate_extended_scope_review, ProvisionalReceiptRollback};
use scope::{
    accepted_boundary_callers, capture_baseline_boundary_callers, validate_public_signature_scope,
};
pub(crate) use seam::{
    validate_seam_file_and_symbol, validate_seam_reachability,
    validate_seam_reachability_with_target,
};

pub(crate) use scope::{
    accepted_evidence_for_cursor, code_map_connection, extract_typed, public_callable_nodes,
};
pub use scope::{set_ast_extractor, AstExtractor};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BaselineBoundaryCallers {
    baseline_commit: String,
    entries: Vec<BaselineCallerEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BaselineCallerEntry {
    path: String,
    kind: String,
    name: String,
    qualname: String,
    caller_paths: BTreeSet<String>,
    #[serde(default)]
    target_present: bool,
}

#[derive(Serialize, Deserialize)]
struct Submission {
    evidence: Evidence,
    action: String,
    findings: Option<serde_json::Value>,
    task: Task,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_callers: Option<BaselineBoundaryCallers>,
}

pub(crate) fn resolve_rejection_target<'a>(
    gate_def: &'a crate::tasks::profile::GateDef,
    findings: Option<&serde_json::Value>,
    current_stage: &'a str,
) -> &'a str {
    crate::engine::gate_graph::resolve_failure_reference(gate_def, findings, current_stage)
}
