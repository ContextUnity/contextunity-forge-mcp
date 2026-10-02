use crate::engine::tasks::{
    self, Action, Claim, List, Manage, ManageAction, Stage, Status, Submit,
};
use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Subcommand)]
/// Enumerates the supported task command values.
pub enum TaskCommand {
    /// Represents the list case.
    List {
        #[arg(long)]
        /// Optional repository value.
        repository: Option<String>,
        #[arg(long)]
        /// Optional milestone value.
        milestone: Option<String>,
        #[arg(long, value_enum)]
        /// Optional status value.
        status: Option<Status>,
        #[arg(long, value_enum)]
        /// Optional stage value.
        stage: Option<Stage>,
    },
    /// Represents the inspect case.
    Inspect {
        /// The task id value.
        task_id: String,
    },
    /// Represents the create case.
    Create {
        /// The milestone ref value.
        milestone_ref: String,
        /// The task ref value.
        task_ref: String,
        #[arg(long)]
        /// Optional workspace value.
        workspace: Option<String>,
    },
    /// Represents the sync case.
    Sync {
        /// Optional milestone ref value.
        milestone_ref: Option<String>,
        #[arg(long)]
        /// Optional workspace value.
        workspace: Option<String>,
    },
    /// Represents the claim case.
    Claim {
        /// The task id value.
        task_id: String,
        #[arg(long)]
        /// The stage value.
        stage: String,
        #[arg(long)]
        /// The worker value.
        worker: String,
        #[arg(long)]
        /// The worktree value.
        worktree: String,
    },
    /// Represents the submit case.
    Submit {
        /// The task id value.
        task_id: String,
        #[arg(long)]
        /// The stage value.
        stage: String,
        #[arg(long, value_enum)]
        /// The action value.
        action: Action,
        #[arg(long)]
        /// The evidence value.
        evidence: String,
        #[arg(long)]
        /// Optional findings value.
        findings: Option<String>,
    },
    /// Represents the extend scope case.
    ExtendScope {
        /// The task id value.
        task_id: String,
        #[arg(required = true)]
        /// The paths value.
        paths: Vec<String>,
    },
    /// Represents the delete case.
    Delete {
        /// Optional task id value.
        task_id: Option<String>,
        #[arg(long, conflicts_with = "task_id")]
        /// Optional milestone value.
        milestone: Option<String>,
        #[arg(long)]
        /// Whether force applies.
        force: bool,
        #[arg(long, requires = "milestone")]
        /// Optional workspace value.
        workspace: Option<String>,
    },
    /// Represents the reset case.
    Reset {
        /// The task id value.
        task_id: String,
    },
    /// Represents the cleanup case.
    Cleanup,
}

/// Performs run.
pub fn run(root: &Path, command: TaskCommand) -> Result<Value> {
    let mut manage = Manage {
        workspace: None,
        action: ManageAction::Inspect,
        task_id: None,
        milestone_ref: None,
        task_ref: None,
        paths: None,
        force: false,
    };
    match command {
        TaskCommand::List {
            repository,
            milestone,
            status,
            stage,
        } => {
            return tasks::list(
                root,
                List {
                    repository,
                    milestone_ref: milestone,
                    status,
                    stage,
                },
            )
        }
        TaskCommand::Claim {
            task_id,
            stage,
            worker,
            worktree,
        } => {
            return tasks::claim(
                root,
                Claim {
                    task_id,
                    stage,
                    worker_id: worker,
                    worktree,
                },
            )
        }
        TaskCommand::Submit {
            task_id,
            stage,
            action,
            evidence,
            findings,
        } => {
            return tasks::submit(
                root,
                Submit {
                    task_id,
                    stage,
                    action,
                    evidence_ref: evidence,
                    findings: findings.map(|s| serde_json::from_str(&s)).transpose()?,
                },
            )
        }
        TaskCommand::Reset { task_id } => {
            return Ok(serde_json::to_value(
                tasks::store_for_task(root, &task_id)?.reset(&task_id)?,
            )?)
        }
        TaskCommand::Cleanup => {
            return Ok(
                serde_json::json!({"deleted":tasks::store(root)?.cleanup(crate::db::tasks_store::now())?}),
            )
        }
        TaskCommand::Inspect { task_id } => manage.task_id = Some(task_id),
        TaskCommand::Create {
            milestone_ref,
            task_ref,
            workspace,
        } => {
            manage.action = ManageAction::Create;
            manage.milestone_ref = Some(milestone_ref);
            manage.task_ref = Some(task_ref);
            manage.workspace = workspace;
        }
        TaskCommand::Sync {
            milestone_ref,
            workspace,
        } => {
            manage.action = ManageAction::Sync;
            manage.milestone_ref = milestone_ref;
            manage.workspace = workspace;
        }
        TaskCommand::ExtendScope { task_id, paths } => {
            manage.action = ManageAction::ExtendScope;
            manage.task_id = Some(task_id);
            manage.paths = Some(paths);
        }
        TaskCommand::Delete {
            task_id,
            milestone,
            force,
            workspace,
        } => {
            manage.action = ManageAction::Delete;
            manage.task_id = task_id;
            manage.milestone_ref = milestone;
            manage.force = force;
            manage.workspace = workspace;
        }
    }
    tasks::manage(root, manage)
}
