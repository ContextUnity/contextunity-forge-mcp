use crate::git::{git_command, git_stdout};
use anyhow::{bail, Context, Result};
use std::path::Path;

/// Rebase a speculative task branch after its upstream snapshot advances.
pub(super) fn rebase_speculative_dependency(
    root: &Path,
    task_branch: &str,
    upstream_snapshot: &str,
) -> Result<()> {
    for branch in [task_branch, upstream_snapshot] {
        if branch.trim().is_empty() {
            bail!(
                "TASK_SPECULATIVE_REBASE_INVALID: branch and upstream snapshot must not be empty"
            );
        }
    }
    let task_branch_check = git_command(root, &["check-ref-format", "--branch", task_branch])?;
    if !task_branch_check.status.success() {
        bail!("TASK_SPECULATIVE_REBASE_INVALID: '{task_branch}' is not a valid branch name");
    }
    let status = git_command(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    if !status.status.success() {
        bail!("TASK_SPECULATIVE_REBASE_FAILED: could not inspect worktree state");
    }
    if !status.stdout.is_empty() {
        let dirty_paths = String::from_utf8_lossy(&status.stdout)
            .lines()
            .take(8)
            .collect::<Vec<_>>()
            .join(", ");
        bail!("TASK_SPECULATIVE_REBASE_DIRTY: speculative rebase requires a clean worktree; dirty paths: {dirty_paths}");
    }
    let original_branch = git_command(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let original_branch = if original_branch.status.success() {
        Some(
            String::from_utf8_lossy(&original_branch.stdout)
                .trim()
                .to_owned(),
        )
    } else {
        None
    };
    let original_head = git_stdout(
        root,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SPECULATIVE_REBASE_FAILED",
    )?;
    let task_ref = format!("refs/heads/{task_branch}^{{commit}}");
    let task_head = git_stdout(
        root,
        &["rev-parse", "--verify", &task_ref],
        "TASK_SPECULATIVE_REBASE_INVALID",
    )?;
    let upstream_head = git_stdout(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{upstream_snapshot}^{{commit}}"),
        ],
        "TASK_SPECULATIVE_REBASE_INVALID",
    )?;
    if task_head == upstream_head {
        bail!("TASK_SPECULATIVE_REBASE_INVALID: task branch already equals the upstream snapshot");
    }
    let switch = git_command(root, &["switch", task_branch])?;
    if !switch.status.success() {
        bail!(
            "TASK_SPECULATIVE_REBASE_FAILED: could not switch to '{task_branch}': {}",
            String::from_utf8_lossy(&switch.stderr).trim()
        );
    }
    let rebase = git_command(root, &["rebase", &upstream_head])?;
    if !rebase.status.success() {
        let abort = git_command(root, &["rebase", "--abort"]);
        let after_abort = git_stdout(
            root,
            &["rev-parse", "--verify", &task_ref],
            "TASK_SPECULATIVE_REBASE_ROLLBACK_FAILED",
        );
        if let Ok(current) = after_abort {
            if current != task_head {
                let update = git_command(
                    root,
                    &[
                        "update-ref",
                        &format!("refs/heads/{task_branch}"),
                        &task_head,
                        &current,
                    ],
                )?;
                if !update.status.success() {
                    bail!("TASK_SPECULATIVE_REBASE_ROLLBACK_FAILED: could not restore speculative branch ref");
                }
            }
        }
        let restore = restore_checkout(root, original_branch.as_deref(), &original_head);
        if let Err(error) = restore {
            bail!("TASK_SPECULATIVE_REBASE_ROLLBACK_FAILED: rebase failed and original checkout could not be restored: {error:#}");
        }
        let abort_note = abort
            .ok()
            .filter(|output| !output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
            .filter(|note| !note.is_empty())
            .map(|note| format!("; abort: {note}"))
            .unwrap_or_default();
        bail!(
            "TASK_SPECULATIVE_REBASE_CONFLICT: rebase of '{task_branch}' onto {upstream_head} failed: {}{abort_note}",
            String::from_utf8_lossy(&rebase.stderr).trim()
        );
    }
    let is_ancestor = git_command(
        root,
        &["merge-base", "--is-ancestor", &upstream_head, task_branch],
    )?;
    if !is_ancestor.status.success() {
        let _ = git_command(root, &["rebase", "--abort"]);
        restore_checkout(root, original_branch.as_deref(), &original_head)?;
        bail!("TASK_SPECULATIVE_REBASE_FAILED: upstream snapshot is not an ancestor of the rebased task branch");
    }
    restore_checkout(root, original_branch.as_deref(), &original_head)
        .context("TASK_SPECULATIVE_REBASE_RESTORE_FAILED: rebased branch is ready but the prior checkout could not be restored")?;
    Ok(())
}

fn restore_checkout(root: &Path, branch: Option<&str>, detached_head: &str) -> Result<()> {
    let output = match branch {
        Some(branch) => git_command(root, &["switch", branch])?,
        None => git_command(root, &["checkout", "--detach", detached_head])?,
    };
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}
