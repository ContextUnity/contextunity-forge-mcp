//! ACDD task lifecycle, stages, statuses, and contract definitions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Current stage of an ACDD task lifecycle.
pub enum TaskStage {
    /// Initial contract synthesis stage by an architect.
    ContractSynthesis,
    /// Formal contract authoring and red-seam verification.
    Contract,
    /// Implementation and green-seam verification by a builder.
    Build,
    /// Independent five-contour review by a reviewer.
    Review,
    /// Final verification and commit delivery by an integrator.
    Deliver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Operational status of an ACDD task or subtask.
pub enum TaskStatus {
    /// Ready to be claimed.
    Ready,
    /// Currently claimed and in progress.
    InProgress,
    /// Successfully passed and completed.
    Completed,
    /// Temporarily blocked by dependencies or review findings.
    Blocked,
    /// Explicitly cancelled.
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a subtask within an ACDD implementation task.
pub struct Subtask {
    /// Unique subtask reference slug.
    pub subtask_ref: String,
    /// Human-readable title and objective.
    pub title: String,
    /// Current completion status.
    pub status: TaskStatus,
    /// Evidence recorded upon completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// ACDD contract boundaries, admitted scope, and invariant bindings.
pub struct TaskContract {
    /// Admitted file and directory write scope paths.
    pub write_scope: Vec<String>,
    /// Task dependency IDs that must be delivered first.
    pub dependencies: Vec<String>,
    /// Invariant identifiers governing this task.
    pub invariants: Vec<String>,
    /// Selected proof policy identifier.
    pub proof_policy: String,
    /// Target description.
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a managed ACDD task.
pub struct Task {
    /// Qualified task identifier.
    pub id: String,
    /// Active milestone reference.
    pub milestone_ref: String,
    /// Current lifecycle stage.
    pub stage: TaskStage,
    /// Current operational status.
    pub status: TaskStatus,
    /// Task contract.
    pub contract: TaskContract,
    /// Subtasks composing the implementation.
    pub subtasks: Vec<Subtask>,
    /// Current worker claiming the task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
}
