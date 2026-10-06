use crate::engine::milestones::{self, Init};
use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use std::{
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Subcommand)]
/// Commands for inspecting and completing repository milestones.
pub enum MilestoneCommand {
    /// Lists milestone metadata and SQLite task progress.
    List {
        /// Includes documents in the milestone archive.
        #[arg(long)]
        archive: bool,
        /// Selects planned, active, completed, or all milestones.
        #[arg(long)]
        status: Option<String>,
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
        /// The command that verified milestone delivery.
        #[arg(long)]
        verification_command: String,
        /// Number of passing tests reported by verification.
        #[arg(long)]
        tests_passed: u64,
        /// Number of failing tests reported by verification.
        #[arg(long)]
        tests_failed: u64,
    },
    /// Creates a numbered milestone contract.
    Init {
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
    },
}

/// Runs the selected milestone command within the repository root.
pub fn run(root: &Path, command: MilestoneCommand) -> Result<Value> {
    match command {
        MilestoneCommand::List { archive, status } => {
            milestones::list(root, archive, status.as_deref())
        }
        MilestoneCommand::Show { id, full } => milestones::show(root, &id, full),
        MilestoneCommand::Handoff {
            id,
            commit,
            verification_command,
            tests_passed,
            tests_failed,
        } => milestones::handoff(
            root,
            &id,
            commit.as_deref(),
            &verification_command,
            tests_passed,
            tests_failed,
        ),
        MilestoneCommand::Init {
            num,
            slug,
            title,
            plan,
            dir,
            desc,
            depends_on,
            active,
        } => {
            let mut stdin = String::new();
            if !io::stdin().is_terminal() {
                io::stdin().read_to_string(&mut stdin)?;
            }
            milestones::init(
                root,
                Init {
                    num,
                    slug,
                    title,
                    plan,
                    dir,
                    desc,
                    depends_on,
                    active,
                    stdin,
                },
            )
        }
    }
}
