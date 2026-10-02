use crate::engine::tasks::{
    self, Action, Claim, List, Manage, ManageAction, Stage, Status, Submit,
};
use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    List {
        #[arg(long)]
        repository: Option<String>,
        #[arg(long)]
        milestone: Option<String>,
        #[arg(long, value_enum)]
        status: Option<Status>,
        #[arg(long, value_enum)]
        stage: Option<Stage>,
    },
    Inspect {
        task_id: String,
    },
    Create {
        milestone_ref: String,
        task_ref: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    Sync {
        milestone_ref: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
    Claim {
        task_id: String,
        #[arg(long)]
        stage: String,
        #[arg(long)]
        worker: String,
        #[arg(long)]
        worktree: String,
    },
    Submit {
        task_id: String,
        #[arg(long)]
        stage: String,
        #[arg(long, value_enum)]
        action: Action,
        #[arg(long)]
        evidence: String,
        #[arg(long)]
        findings: Option<String>,
    },
    ExtendScope {
        task_id: String,
        #[arg(required = true)]
        paths: Vec<String>,
    },
    Delete {
        task_id: Option<String>,
        #[arg(long, conflicts_with = "task_id")]
        milestone: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long, requires = "milestone")]
        workspace: Option<String>,
    },
    Reset {
        task_id: String,
    },
    Cleanup,
}

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
