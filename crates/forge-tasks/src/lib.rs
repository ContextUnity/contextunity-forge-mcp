//! Pure Rust ContextUnity Forge ACDD task lifecycle, blackboard, and verification.

pub mod blackboard;
pub mod config;
pub mod engine;
pub mod fs;
pub mod git;
pub mod lifecycle;
pub mod milestones;
pub mod response;
pub mod store;
pub mod sys;
pub mod tasks;
pub mod verification;

// Explicit public exports:
pub use blackboard::{BlackboardEntry, BlackboardScope};
pub use config::{
    load_adapter, Adapter, AdapterFile, LinkedWorkspace, LinkedWorkspaceConfig,
    ENGINE_SCHEMA_VERSION, INDEX_SEMANTICS_VERSION,
};
pub use engine::gates::set_ast_extractor;
pub use engine::{
    candidate_fingerprint, claim, compile_evidence_envelope, database_path, default_db, list,
    list_workspaces, manage, primary_worktree_root, project_identity, read_checkpoint_fence,
    rebase_speculative_dependency, reopen, reset, store, store_for_task, submit, submit_zero_arg,
    telemetry_passport_note, verify_red_to_green, Action, BlackboardAction, BlackboardRequest,
    Claim, CommitTrackingSetting, List, Manage, ManageAction, MilestoneStatusFilter, Status,
    Submit, TaskListDetail, TaskSettings,
};
pub use fs::*;
pub use git::*;
pub use lifecycle::{Subtask, Task, TaskContract, TaskStage, TaskStatus};
pub use milestones::handoff::set_symbol_facts_extractor;
pub use milestones::{
    draft, execute_command_gate, execute_milestone_verification, extract_shared_helpers, handoff,
    handoff_context, init_from_manifest, synthesize_milestone_contracts, validate_wave_dag,
    verify_contract_matrix_parity, HandoffBinding, IntegrationEvidence, IntegrationReview,
    IntegrationVerification,
};
pub use response::*;
pub use store::{TasksStore, TASKS_SCHEMA_VERSION};
pub use tasks::*;
pub use verification::{Evidence as GateEvidence, ProofPolicy, VerificationResult};

// Internal path compatibility aliases
pub(crate) mod core {
    pub use crate::config;
    pub use crate::fs;
    pub use crate::git;
    pub use crate::tasks;
    pub use forge_core::commitments;
}

pub(crate) mod db {
    pub use crate::store as tasks_store;
}

/// Returns the forge-tasks crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Regular expression dependency shared with application-level readers.
pub use regex;
