//! Git execution helpers without task store or parser dependencies.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Output};

/// Runs a git command in the specified working directory.
pub fn git_command(worktree: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(worktree)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute git {:?}", args))
}
