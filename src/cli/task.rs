use anyhow::Result;
use clap::Subcommand;
use forge_tasks::engine as tasks;
use forge_tasks::{
    Action, BlackboardAction, BlackboardRequest, BlackboardScope, Claim, List, Manage,
    ManageAction, MilestoneStatusFilter, Status, Submit, TaskListDetail,
};
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
        /// Optional target task that receives the message, independent of its source scope.
        #[arg(long)]
        recipient_task_id: Option<String>,
        /// Subtask receiving the message.
        #[arg(long)]
        subtask_ref: Option<String>,
        /// Optional active-profile gate receiving the message.
        #[arg(long)]
        gate: Option<String>,
        #[arg(long)]
        /// Canonical message topic: draft, notes, findings, blockers, decisions, or deferred.
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
        /// Optional active-profile gate used to filter messages.
        #[arg(long)]
        gate: Option<String>,
        #[arg(long)]
        /// Restrict results to one canonical topic.
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
        /// Subtask status: ready, in_progress, completed, cancelled, or blocked; pending aliases ready.
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
    /// Execute the active task profile in the selected workspace.
    Run {
        /// Task identifier to execute from its persisted gate cursor.
        task_id: String,
        /// Execute in the selected root without creating a worktree or branch.
        #[arg(long)]
        in_place: bool,
    },
    /// Atomically claim the highest-priority eligible task gate.
    Next {
        /// Worker receiving the selected claim.
        #[arg(long)]
        worker: String,
        /// Requested worktree, overridden by retained parking affinity.
        #[arg(long)]
        worktree: String,
        /// Optional configured task workspace.
        #[arg(long)]
        workspace: Option<String>,
    },
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
        /// Task status: ready (default), in_progress, blocked, parked, completed, cancelled, or all.
        status: Option<Status>,
        #[arg(long)]
        /// Filter by any gate identifier in the active workflow profile.
        stage: Option<String>,
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
        /// Show tasks from all milestone statuses, including cancelled milestones pending sync pruning.
        all: bool,
        #[arg(long, conflicts_with = "detail")]
        /// Include subtask titles and verification evidence.
        full: bool,
        #[arg(long, value_enum)]
        /// Subtask detail level: compact (default) or full.
        detail: Option<TaskListDetail>,
        #[arg(long)]
        /// Maximum tasks returned, from 1 through 100 (default 30).
        limit: Option<usize>,
        #[arg(long, default_value_t = 0)]
        /// Number of matching tasks skipped before this page.
        offset: usize,
        #[arg(long)]
        /// Task-view generation returned by the previous page.
        generation: Option<String>,
    },
    /// Represents the inspect case.
    #[command(visible_alias = "show")]
    Inspect {
        /// The task id value.
        #[arg(required_unless_present = "task", conflicts_with = "task")]
        task_id: Option<String>,
        /// Task identifier as an alternative to the positional argument.
        #[arg(long)]
        task: Option<String>,
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
        #[arg(conflicts_with = "file")]
        milestone_ref: Option<String>,
        /// Milestone file as an alternative to the positional reference.
        #[arg(long)]
        file: Option<String>,
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
    /// Run and cache the claimed gate command without advancing the task.
    Verify {
        /// Task identifier.
        task_id: String,
        /// Explicit test file or qualified test function (e.g. 'tests/acdd/my_test.rs::test_fn').
        #[arg(long)]
        test: Option<String>,
        /// Optional production seam target path or symbol when verifying CodeMap reachability.
        #[arg(long, requires = "test")]
        seam: Option<String>,
        /// Runtime command timeout in seconds, bounded by the active profile.
        #[arg(long = "timeout")]
        timeout_seconds: Option<u64>,
    },
    /// Represents the submit case.
    Submit {
        /// The task id value.
        #[arg(required_unless_present = "task", conflicts_with = "task")]
        task_id: Option<String>,
        /// Task identifier as an alternative to the positional argument.
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        /// The stage value.
        stage: String,
        #[arg(long, value_enum)]
        /// The action value.
        action: Action,
        #[arg(long)]
        /// Optional JSON evidence object; automated gates can infer proof from persisted execution.
        evidence: Option<String>,
        /// Runtime command timeout in seconds for submission without explicit evidence.
        #[arg(long = "timeout", conflicts_with = "evidence")]
        timeout_seconds: Option<u64>,
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
    /// Clean up expired tasks and orphaned checkpoint refs.
    #[command(alias = "clear", alias = "clean")]
    Cleanup,
}

pub(super) fn preflight_error(command: &TaskCommand, error: anyhow::Error) -> anyhow::Error {
    let subject = match command {
        TaskCommand::Run { task_id, .. }
        | TaskCommand::Claim { task_id, .. }
        | TaskCommand::Verify { task_id, .. }
        | TaskCommand::ExtendScope { task_id, .. }
        | TaskCommand::Reset { task_id }
        | TaskCommand::Reopen { task_id }
        | TaskCommand::Context { task_id } => Some(task_id.as_str()),
        TaskCommand::Inspect { task_id, task } | TaskCommand::Submit { task_id, task, .. } => {
            task_id.as_deref().or(task.as_deref())
        }
        TaskCommand::Create { milestone_ref, .. } => Some(milestone_ref.as_str()),
        TaskCommand::Sync {
            milestone_ref,
            file,
            ..
        } => milestone_ref.as_deref().or(file.as_deref()),
        TaskCommand::Delete {
            task_id, milestone, ..
        } => task_id.as_deref().or(milestone.as_deref()),
        TaskCommand::Subtask { command } => Some(match command {
            SubtaskCommand::Add { task_id, .. }
            | SubtaskCommand::List { task_id, .. }
            | SubtaskCommand::Update { task_id, .. } => task_id.as_str(),
        }),
        TaskCommand::Blackboard { command } => match command {
            BlackboardCommand::Post {
                task_id,
                milestone_ref,
                ..
            }
            | BlackboardCommand::Read {
                task_id,
                milestone_ref,
                ..
            } => task_id.as_deref().or(milestone_ref.as_deref()),
            BlackboardCommand::Inspect { .. } => None,
        },
        TaskCommand::Next { .. } | TaskCommand::List { .. } | TaskCommand::Cleanup => None,
    }
    .unwrap_or("unselected");
    anyhow::anyhow!(forge_tasks::tasks::task_lifecycle_diagnostic(
        subject,
        &error.to_string()
    ))
}

/// Performs run.
pub fn run(root: &Path, command: TaskCommand) -> Result<Value> {
    crate::mcp::tasks::ensure_extractors();
    let mut manage = Manage::default();
    match command {
        TaskCommand::Run {
            task_id,
            in_place: _,
        } => {
            let session = forge_tasks::store::host_session_fingerprint().ok_or_else(|| {
                anyhow::anyhow!(
                    "TASK_RUN_SESSION_REQUIRED: supply the genuine host session identity"
                )
            })?;
            return tasks::orchestrator::task_runner::run_task_lifecycle(
                root,
                &task_id,
                &tasks::orchestrator::task_runner::TaskRunOptions {
                    worktree: root.canonicalize()?,
                    worker_id: "task-run-coordinator".into(),
                    session_fingerprint: session,
                    runner_timeout: std::time::Duration::from_secs(3600),
                },
                &mut |_| {},
            );
        }
        TaskCommand::Next {
            worker,
            worktree,
            workspace,
        } => {
            manage.action = ManageAction::Next;
            manage.worker_id = Some(worker);
            manage.worktree = Some(worktree);
            manage.workspace = workspace;
        }
        TaskCommand::Blackboard { command } => {
            return match command {
                BlackboardCommand::Post {
                    scope,
                    milestone_ref,
                    task_id,
                    recipient_task_id,
                    subtask_ref,
                    gate,
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
                        recipient_task_id,
                        subtask_ref,
                        message_id: None,
                        author,
                        topic: Some(topic),
                        gate,
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
                    gate,
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
                        recipient_task_id: None,
                        subtask_ref,
                        message_id: None,
                        author: None,
                        topic,
                        gate,
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
                        recipient_task_id: None,
                        subtask_ref: None,
                        message_id: Some(message_id),
                        author: None,
                        topic: None,
                        gate: None,
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
            detail,
            limit,
            offset,
            generation,
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
                    detail: detail.or(full.then_some(TaskListDetail::Full)),
                    limit,
                    offset,
                    generation,
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
                    session_fingerprint: None,
                    task_id,
                    stage,
                    worker_id: worker,
                    worktree,
                    bundle: if bundle { Some(true) } else { None },
                },
            );
        }
        TaskCommand::Verify {
            task_id,
            test,
            seam,
            timeout_seconds,
        } => {
            return tasks::manage(
                root,
                Manage {
                    action: ManageAction::Verify,
                    task_id: Some(task_id),
                    test,
                    seam,
                    timeout_seconds,
                    ..Default::default()
                },
            );
        }
        TaskCommand::Submit {
            task_id,
            task,
            stage,
            action,
            evidence,
            timeout_seconds,
            findings,
        } => {
            let task_id = task_id
                .or(task)
                .ok_or_else(|| anyhow::anyhow!("task id is required"))?;
            let findings = findings.map(|s| serde_json::from_str(&s)).transpose()?;
            return match evidence {
                Some(evidence) => tasks::submit(
                    root,
                    Submit {
                        session_fingerprint: None,
                        task_id,
                        stage,
                        action,
                        evidence: serde_json::from_str(&evidence)?,
                        findings,
                    },
                ),
                None => tasks::submit_zero_arg_with_timeout(
                    root,
                    &task_id,
                    &stage,
                    action,
                    findings,
                    timeout_seconds,
                    None,
                ),
            };
        }
        TaskCommand::Reset { task_id } | TaskCommand::Reopen { task_id } => {
            return Ok(serde_json::to_value(tasks::reset(root, &task_id)?)?);
        }
        TaskCommand::Context { task_id } => {
            manage.action = ManageAction::Context;
            manage.task_id = Some(task_id);
            return tasks::manage(root, manage);
        }
        TaskCommand::Cleanup => {
            let mut store = tasks::store(root)?;
            let deleted_tasks = store.cleanup(crate::db::tasks_store::now())?;
            let pruned_checkpoints = store.cleanup_orphaned_checkpoints(root)?;
            return Ok(serde_json::json!({
                "deleted_tasks": deleted_tasks,
                "pruned_checkpoints": pruned_checkpoints,
            }));
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
        TaskCommand::Inspect { task_id, task } => {
            manage.task_id = Some(
                task_id
                    .or(task)
                    .ok_or_else(|| anyhow::anyhow!("task id is required"))?,
            );
        }
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
            file,
            workspace,
        } => {
            manage.action = ManageAction::Sync;
            manage.milestone_ref = milestone_ref.or(file);
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
