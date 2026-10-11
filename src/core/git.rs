use anyhow::{bail, Result};
use std::{
    path::Path,
    process::{Command, ExitStatus, Output},
};

/// Run a Git command in the given worktree and capture its output.
pub fn git_command(worktree: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new("git")
        .args(args)
        .current_dir(worktree)
        .output()?)
}

/// Read non-empty UTF-8-lossy standard output from a successful Git command.
pub fn git_stdout(worktree: &Path, args: &[&str], error_code: &str) -> Result<String> {
    let output = git_command(worktree, args)?;
    if !output.status.success() {
        bail!("{error_code}: {}", String::from_utf8_lossy(&output.stderr));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        bail!("{error_code}: empty Git response");
    }
    Ok(value)
}

/// Return whether a Git command completed successfully.
pub fn git_status(worktree: &Path, args: &[&str]) -> Result<ExitStatus> {
    Ok(git_command(worktree, args)?.status)
}

/// Check whether a commit object exists in the worktree's repository.
pub fn git_commit_exists(worktree: &Path, commit: &str) -> Result<bool> {
    let object = format!("{commit}^{{commit}}");
    Ok(git_status(worktree, &["cat-file", "-e", &object])?.success())
}

/// Return the list of files modified, added, or deleted by a commit.
pub fn git_commit_changed_files(worktree: &Path, commit: &str) -> Result<Vec<String>> {
    let parents_raw = git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_COMMIT_INVALID",
    )?;
    let parents: Vec<&str> = parents_raw.split_whitespace().collect();
    let args = if parents.len() <= 1 {
        vec!["diff-tree", "--root", "--no-commit-id", "--name-only", "-r", commit]
    } else if parents.len() == 2 {
        vec!["diff-tree", "--no-commit-id", "--name-only", "-r", commit]
    } else {
        vec!["diff-tree", "--no-commit-id", "--name-only", "-r", "--first-parent", "-m", commit]
    };
    let output = git_command(worktree, &args)?;
    if !output.status.success() {
        bail!("TASK_COMMIT_INVALID: unable to inspect changed files for commit {commit}");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Check whether `ancestor` is an ancestor of `descendant` in the Git commit graph.
pub fn git_ancestor(worktree: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let output = git_command(
        worktree,
        &["merge-base", "--is-ancestor", ancestor, descendant],
    )?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "TASK_ANCESTRY_FAILED: unable to determine commit ancestry between {ancestor} and {descendant}: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// Check whether two commits differ in the tree for the given task scope paths.
/// If `scope` is empty, compares the full repository tree between the two commits.
pub fn git_scoped_tree_changed(
    worktree: &Path,
    commit_a: &str,
    commit_b: &str,
    scope: &[String],
) -> Result<bool> {
    let mut args = vec!["diff", "--quiet", commit_a, commit_b];
    if !scope.is_empty() {
        args.push("--");
        args.extend(scope.iter().map(String::as_str));
    }
    let output = git_command(worktree, &args)?;
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => bail!(
            "TASK_DIFF_FAILED: unable to compare task-scoped trees between {commit_a} and {commit_b}: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}
