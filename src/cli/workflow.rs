use crate::forge_tasks::engine::orchestrator::workflow;
use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Subcommand)]
/// Standalone workflow operations.
pub enum WorkflowCommand {
    /// List standalone workflow documents without executing them.
    List,
    /// Show one validated root-gate workflow.
    Show {
        /// Workflow name or discovered root-relative path.
        selector: String,
    },
    /// Execute a standalone workflow in the current workspace.
    Run {
        /// Workflow name or discovered root-relative path.
        selector: String,
    },
}

/// Dispatch a workflow operation without task-store initialization.
pub fn run(root: &Path, command: WorkflowCommand) -> Result<Value> {
    match command {
        WorkflowCommand::List => Ok(json!({"workflows": workflow::list_workflows(root)?})),
        WorkflowCommand::Show { selector } => {
            let path = workflow::resolve_workflow(root, &selector)?;
            Ok(json!({"path":path, "profile":workflow::read_workflow(root, &path)?}))
        }
        WorkflowCommand::Run { selector } => {
            let path = workflow::resolve_workflow(root, &selector)?;
            workflow::run_standalone_workflow(root, &path)
        }
    }
}
