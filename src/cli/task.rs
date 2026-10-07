use crate::engine::tasks::{
    self, Action, BlackboardAction, BlackboardRequest, BlackboardScope, Claim, List, Manage,
    ManageAction, MilestoneStatusFilter, Stage, Status, Submit, TaskListDetail,
};
use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Subcommand)]
/// Operations on milestone-, task-, and subtask-scoped blackboard messages.
pub enum BlackboardCommand {
    /// Post a message to a resolved blackboard scope.
    Post {
        /// Optional hierarchy scope; omitted keys resolve to one unique active context.
        #[arg(long, value_enum)]
        scope: Option<BlackboardScope>,
        /// Milestone reference used by milestone scope or as a context filter.
        #[arg(long)]
        milestone_ref: Option<String>,
        /// Task receiving the message.
        task_id: Option<String>,
        /// Subtask receiving the message.
        #[arg(long)]
        subtask_ref: Option<String>,
        #[arg(long)]
        /// Message category.
        topic: String,
        #[arg(long)]
        /// Message text or serialized JSON.
        payload: String,
        #[arg(long)]
        /// Posting agent identifier.
        author: Option<String>,
    },
    /// Read one bounded page of message summaries from a resolved scope.
    Read {
        /// Optional hierarchy scope; omitted keys resolve to one unique active context.
        #[arg(long, value_enum)]
        scope: Option<BlackboardScope>,
        /// Milestone reference used by milestone scope or as a context filter.
        #[arg(long)]
        milestone_ref: Option<String>,
        /// Task whose messages are read.
        task_id: Option<String>,
        /// Subtask whose messages are read.
        #[arg(long)]
        subtask_ref: Option<String>,
        #[arg(long)]
        /// Restrict results to this category.
        topic: Option<String>,
        #[arg(long)]
        /// Maximum number of messages to return (default 10, maximum 50).
        limit: Option<usize>,
        #[arg(long)]
        /// Number of messages to skip before this page.
        offset: Option<usize>,
    },
    /// Inspect one message including its payload.
    Inspect {
        /// Message identifier returned by post or read.
        message_id: u64,
    },
}

#[derive(Debug, Subcommand)]
/// Subtask management commands.
pub enum SubtaskCommand {
    /// Add an iterative subtask to an existing task.
    Add {
        /// Task identifier.
        task_id: String,
        /// Unique subtask reference slug within the task.
        subtask_ref: String,
        /// Description of the subtask goal.
        title: String,
        #[arg(long)]
        /// Optional workspace value.
        workspace: Option<String>,
    },
    /// List subtasks for a task.
    List {
        /// Task identifier.
        task_id: String,
        #[arg(long)]
        /// Optional workspace value.
        workspace: Option<String>,
    },
    /// Update a subtask's status and evidence.
    Update {
        /// Task identifier.
        task_id: String,
        /// Subtask reference slug.
        subtask_ref: String,
        #[arg(long)]
        /// Subtask status: pending, in_progress, or completed.
        status: String,
        #[arg(long)]
        /// Optional verification evidence or test command.
        evidence: Option<String>,
        #[arg(long)]
        /// Optional workspace value.
        workspace: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
/// Enumerates the supported task command values.
pub enum TaskCommand {
    /// Manage iterative subtasks for a task.
    Subtask {
        #[command(subcommand)]
        /// Selected subtask operation.
        command: SubtaskCommand,
    },
    /// Exchange task-scoped collaboration messages.
    Blackboard {
        #[command(subcommand)]
        /// Selected blackboard operation.
        command: BlackboardCommand,
    },
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
        #[arg(long, value_enum, conflicts_with_all = ["planned", "completed", "all"])]
        /// Filter by milestone lifecycle status: active, planned, completed, or all.
        milestone_status: Option<MilestoneStatusFilter>,
        #[arg(long, conflicts_with_all = ["milestone_status", "completed", "all"])]
        /// Show tasks from planned milestones.
        planned: bool,
        #[arg(long, conflicts_with_all = ["milestone_status", "planned", "all"])]
        /// Show tasks from completed milestones.
        completed: bool,
        #[arg(long, conflicts_with_all = ["milestone_status", "planned", "completed"])]
        /// Show tasks from all statuses, including cancelled tasks pending sync pruning.
        all: bool,
        #[arg(long)]
        /// Include subtask titles and verification evidence.
        full: bool,
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
        #[arg(long)]
        /// Return unified zero-shot task context bundle.
        bundle: bool,
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
        /// JSON evidence object.
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
    /// Reset or reopen a task (including a completed task), resetting it back to ready at contract stage.
    Reset {
        /// The task id value.
        task_id: String,
    },
    /// Reopen a task (including a completed task), resetting it back to ready at contract stage.
    Reopen {
        /// The task id value.
        task_id: String,
    },
    /// Return the unified zero-shot task context bundle.
    Context {
        /// The task id value.
        task_id: String,
    },
    /// Represents the cleanup case.
    Cleanup,
}

/// Performs run.
pub fn run(root: &Path, command: TaskCommand) -> Result<Value> {
    let mut manage = Manage::default();
    match command {
        TaskCommand::Blackboard { command } => {
            return match command {
                BlackboardCommand::Post {
                    scope,
                    milestone_ref,
                    task_id,
                    subtask_ref,
                    topic,
                    payload,
                    author,
                } => tasks::blackboard(
                    root,
                    BlackboardRequest {
                        action: BlackboardAction::Post,
                        scope,
                        milestone_ref,
                        task_id,
                        subtask_ref,
                        message_id: None,
                        author,
                        topic: Some(topic),
                        payload: Some(payload),
                        limit: None,
                        offset: None,
                    },
                    "cli",
                ),
                BlackboardCommand::Read {
                    scope,
                    milestone_ref,
                    task_id,
                    subtask_ref,
                    topic,
                    limit,
                    offset,
                } => tasks::blackboard(
                    root,
                    BlackboardRequest {
                        action: BlackboardAction::Read,
                        scope,
                        milestone_ref,
                        task_id,
                        subtask_ref,
                        message_id: None,
                        author: None,
                        topic,
                        payload: None,
                        limit,
                        offset,
                    },
                    "cli",
                ),
                BlackboardCommand::Inspect { message_id } => tasks::blackboard(
                    root,
                    BlackboardRequest {
                        action: BlackboardAction::Inspect,
                        scope: None,
                        milestone_ref: None,
                        task_id: None,
                        subtask_ref: None,
                        message_id: Some(message_id),
                        author: None,
                        topic: None,
                        payload: None,
                        limit: None,
                        offset: None,
                    },
                    "cli",
                ),
            };
        }
        TaskCommand::List {
            repository,
            milestone,
            status,
            stage,
            milestone_status,
            planned,
            completed,
            all,
            full,
        } => {
            let milestone_status = milestone_status.or({
                if planned {
                    Some(MilestoneStatusFilter::Planned)
                } else if completed {
                    Some(MilestoneStatusFilter::Completed)
                } else if all {
                    Some(MilestoneStatusFilter::All)
                } else {
                    None
                }
            });
            return tasks::list(
                root,
                List {
                    repository,
                    milestone_ref: milestone,
                    milestone_status,
                    status,
                    stage,
                    detail: full.then_some(TaskListDetail::Full),
                },
            );
        }
        TaskCommand::Claim {
            task_id,
            stage,
            worker,
            worktree,
            bundle,
        } => {
            return tasks::claim(
                root,
                Claim {
                    task_id,
                    stage,
                    worker_id: worker,
                    worktree,
                    bundle: if bundle { Some(true) } else { None },
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
                    evidence: serde_json::from_str(&evidence)?,
                    findings: findings.map(|s| serde_json::from_str(&s)).transpose()?,
                },
            )
        }
        TaskCommand::Reset { task_id } | TaskCommand::Reopen { task_id } => {
            return Ok(serde_json::to_value(tasks::reset(root, &task_id)?)?)
        }
        TaskCommand::Context { task_id } => {
            manage.action = ManageAction::Context;
            manage.task_id = Some(task_id);
            return tasks::manage(root, manage);
        }
        TaskCommand::Cleanup => {
            return Ok(
                serde_json::json!({"deleted":tasks::store(root)?.cleanup(crate::db::tasks_store::now())?}),
            )
        }
        TaskCommand::Subtask { command } => match command {
            SubtaskCommand::Add {
                task_id,
                subtask_ref,
                title,
                workspace,
            } => {
                manage.action = ManageAction::SubtaskAdd;
                manage.task_id = Some(task_id);
                manage.subtask_ref = Some(subtask_ref);
                manage.title = Some(title);
                manage.workspace = workspace;
            }
            SubtaskCommand::List { task_id, workspace } => {
                manage.action = ManageAction::SubtaskList;
                manage.task_id = Some(task_id);
                manage.workspace = workspace;
            }
            SubtaskCommand::Update {
                task_id,
                subtask_ref,
                status,
                evidence,
                workspace,
            } => {
                manage.action = ManageAction::SubtaskUpdate;
                manage.task_id = Some(task_id);
                manage.subtask_ref = Some(subtask_ref);
                manage.subtask_status = Some(status);
                manage.evidence = evidence;
                manage.workspace = workspace;
            }
        },
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
