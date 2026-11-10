use anyhow::Result;
use clap::{Args, Subcommand};
use forge_tasks::milestones::{self, Init};
use serde_json::Value;
use std::{
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Subcommand)]
/// Commands for inspecting and completing repository milestones.
pub enum MilestoneCommand {
    /// Execute profile gates, task waves, verification, and content-bound handoff.
    Run {
        /// Milestone ID or numeric prefix.
        id: String,
        /// Execute task writers serially in the current checkout.
        #[arg(long)]
        in_place: bool,
        /// Explicit integration base used by review and verification.
        #[arg(long)]
        target_base: String,
    },
    /// Restores acknowledged active tasks from durable per-task checkpoints.
    DbRecover {
        /// Optional milestone ID or reference to recover.
        #[arg(value_name = "MILESTONE")]
        milestone: Option<String>,
    },
    /// Lists milestone metadata and SQLite task progress.
    List {
        /// Includes documents in the milestone archive.
        #[arg(long)]
        archive: bool,
        /// Selects planned, active, completed, or all milestones.
        #[arg(long)]
        status: Option<String>,
        /// Maximum archived documents to include, clamped to 1..=100.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Shows one milestone by ID or numeric file prefix.
    Show {
        /// The milestone ID or numeric prefix.
        id: String,
        /// Includes the complete Markdown document.
        #[arg(long)]
        full: bool,
    },
    /// Completes a verified milestone and archives its document.
    Handoff {
        /// The milestone ID or numeric prefix.
        id: String,
        /// The full Git commit SHA; defaults to HEAD.
        #[arg(long)]
        commit: Option<String>,
        /// JSON file containing content-bound independent review and verification.
        #[arg(long)]
        evidence_file: PathBuf,
    },
    /// Commits admitted polish and prepares exact content identities for independent review.
    HandoffContext {
        /// The milestone ID or numeric prefix.
        id: String,
        /// Explicit integration target base (for example main).
        #[arg(long)]
        target_base: String,
    },
    /// Runs the first applicable command gate from the milestone profile.
    Verify {
        /// The milestone ID or numeric prefix.
        id: String,
        /// Explicit integration target base used to bind this verification run.
        #[arg(long)]
        target_base: String,
    },
    /// Validates a milestone manifest, checks required skills, and prepares repository worktrees.
    Init {
        /// Manifest file to initialize; omitted retains the legacy template scaffolding behavior.
        manifest_path: Option<PathBuf>,
        /// Keep work in the current checkout while still running all manifest preflight checks.
        #[arg(long)]
        in_place: bool,
        /// Legacy template scaffolding options; use `milestone draft` for new workflows.
        #[command(flatten)]
        draft: DraftArgs,
    },
    /// Creates a numbered milestone contract draft.
    Draft(DraftArgs),
    /// Purge milestone tasks, execution traces, blackboard messages, and Git checkpoints.
    Clear {
        /// Milestone ID, slug, or numeric prefix.
        id: String,
    },
}

#[derive(Debug, Args)]
/// Options for creating a numbered milestone draft.
pub struct DraftArgs {
    /// Explicit milestone number; defaults to the next number.
    #[arg(long)]
    num: Option<String>,
    /// Stable lowercase milestone slug.
    #[arg(long)]
    slug: Option<String>,
    /// Human-readable milestone title.
    #[arg(long)]
    title: Option<String>,
    /// Source plan path within the workspace.
    #[arg(long)]
    plan: Option<PathBuf>,
    /// Target directory for the milestone document.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Description added to the milestone body.
    #[arg(long)]
    desc: Option<String>,
    /// Milestone IDs that this contract depends on.
    #[arg(long)]
    depends_on: Vec<String>,
    /// Starts the active development clock at creation.
    #[arg(long)]
    active: bool,
}

impl DraftArgs {
    fn has_options(&self) -> bool {
        self.num.is_some()
            || self.slug.is_some()
            || self.title.is_some()
            || self.plan.is_some()
            || self.dir.is_some()
            || self.desc.is_some()
            || !self.depends_on.is_empty()
            || self.active
    }
}

/// Runs the selected milestone command within the repository root.
pub fn run(root: &Path, command: MilestoneCommand) -> Result<Value> {
    crate::mcp::tasks::ensure_extractors();
    match command {
        MilestoneCommand::Run {
            id,
            in_place,
            target_base,
        } => {
            let session = forge_tasks::store::host_session_fingerprint().ok_or_else(|| {
                anyhow::anyhow!(
                    "TASK_RUN_SESSION_REQUIRED: supply the genuine host session identity"
                )
            })?;
            forge_tasks::engine::orchestrator::waves::run_milestone_waves(
                root,
                &id,
                &forge_tasks::engine::orchestrator::waves::MilestoneRunOptions {
                    task: forge_tasks::engine::orchestrator::task_runner::TaskRunOptions {
                        worktree: root.canonicalize()?,
                        worker_id: "milestone-run-coordinator".into(),
                        session_fingerprint: session,
                        runner_timeout: std::time::Duration::from_secs(3600),
                    },
                    in_place,
                    target_base,
                },
                &mut |_| {},
            )
        }
        MilestoneCommand::DbRecover { milestone } => {
            milestones::db_recover_milestone(root, milestone.as_deref())
        }
        MilestoneCommand::List {
            archive,
            status,
            limit,
        } => milestones::list(root, archive, status.as_deref(), limit),
        MilestoneCommand::Show { id, full } => milestones::show(root, &id, full),
        MilestoneCommand::Handoff {
            id,
            commit,
            evidence_file,
        } => {
            let evidence: milestones::IntegrationEvidence =
                serde_json::from_slice(&std::fs::read(evidence_file)?)?;
            milestones::handoff(root, &id, commit.as_deref(), &evidence)
        }
        MilestoneCommand::HandoffContext { id, target_base } => {
            milestones::handoff_context(root, &id, &target_base)
        }
        MilestoneCommand::Verify { id, target_base } => {
            milestones::execute_milestone_verification(root, &id, &target_base)
        }
        MilestoneCommand::Init {
            manifest_path: Some(manifest),
            in_place,
            draft,
        } => {
            if draft.has_options() {
                anyhow::bail!("manifest initialization does not accept draft options");
            }
            milestones::init_from_manifest(root, &manifest, in_place)
        }
        MilestoneCommand::Init {
            manifest_path: None,
            in_place,
            draft,
        } => {
            if in_place {
                anyhow::bail!("--in-place requires a milestone manifest path");
            }
            run_draft(root, draft, false)
        }
        MilestoneCommand::Draft(draft) => run_draft(root, draft, true),
        MilestoneCommand::Clear { id } => milestones::clear_milestone(root, &id),
    }
}

fn run_draft(root: &Path, draft: DraftArgs, canonical_draft: bool) -> Result<Value> {
    let mut stdin = String::new();
    if !io::stdin().is_terminal() {
        io::stdin().read_to_string(&mut stdin)?;
    }
    let options = Init {
        num: draft.num,
        slug: draft.slug,
        title: draft.title,
        plan: draft.plan,
        dir: draft.dir,
        desc: draft.desc,
        depends_on: draft.depends_on,
        active: draft.active,
        stdin,
    };
    if canonical_draft {
        milestones::draft(root, options)
    } else {
        milestones::init(root, options)
    }
}
