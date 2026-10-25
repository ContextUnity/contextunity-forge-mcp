use crate::engine::milestones::{self, Init};
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Subcommand)]
/// Commands for inspecting and completing repository milestones.
pub enum MilestoneCommand {
    /// Restores acknowledged active tasks from durable per-task checkpoints.
    DbRecover,
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
    /// Runs the full repository verification container before milestone documentation.
    Verify {
        /// The milestone ID or numeric prefix.
        id: String,
        /// Explicit integration target base used to bind this verification run.
        #[arg(long)]
        target_base: String,
    },
    /// Claims or submits the architect build step in milestone verification fallback.
    VerifyBuild {
        /// The milestone ID or numeric prefix.
        id: String,
        /// Explicit integration target base used to bind this verification run.
        #[arg(long)]
        target_base: String,
        /// Architect worker claiming or submitting the bounded build step.
        #[arg(long)]
        worker_id: String,
        /// Claims before changes or submits after committing a changed candidate.
        #[arg(long, value_parser = ["claim", "submit"])]
        action: String,
    },
    /// Submits content-bound independent review for a passing milestone verification.
    #[command(name = "verify-review")]
    VerifyReview {
        /// The milestone ID or numeric prefix.
        id: String,
        /// JSON file containing the verification binding, target base, and review.
        #[arg(long)]
        review_file: PathBuf,
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
    match command {
        MilestoneCommand::DbRecover => milestones::db_recover(root),
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
        MilestoneCommand::VerifyBuild {
            id,
            target_base,
            worker_id,
            action,
        } => milestones::milestone_verification_build(root, &id, &target_base, &worker_id, &action),
        MilestoneCommand::VerifyReview { id, review_file } => {
            let review: milestones::MilestoneVerificationReview =
                serde_json::from_slice(&std::fs::read(review_file)?)?;
            milestones::submit_milestone_verification_review(root, &id, &review)
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
