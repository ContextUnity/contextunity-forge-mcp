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
        vec![
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            commit,
        ]
    } else if parents.len() == 2 {
        vec!["diff-tree", "--no-commit-id", "--name-only", "-r", commit]
    } else {
        vec![
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--first-parent",
            "-m",
            commit,
        ]
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

/// Return the list of files differing between two commits within the given scope.
pub fn git_scoped_differing_files(
    worktree: &Path,
    commit_a: &str,
    commit_b: &str,
    scope: &[String],
) -> Result<Vec<String>> {
    let mut args = vec!["diff", "--name-only", commit_a, commit_b];
    if !scope.is_empty() {
        args.push("--");
        args.extend(scope.iter().map(String::as_str));
    }
    let output = git_command(worktree, &args)?;
    if !output.status.success() {
        bail!(
            "TASK_DIFF_FAILED: unable to list differing files between {commit_a} and {commit_b}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

fn file_entry(worktree: &Path, commit: &str, path: &str) -> Result<Option<(String, String)>> {
    let output = git_command(
        worktree,
        &["--literal-pathspecs", "ls-tree", "-z", commit, "--", path],
    )?;
    if !output.status.success() {
        bail!("TASK_MERGE_INVALID: unable to inspect file entry");
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    let header = output
        .stdout
        .split(|byte| *byte == b'\t')
        .next()
        .unwrap_or_default();
    let fields = std::str::from_utf8(header)?
        .split_whitespace()
        .collect::<Vec<_>>();
    if fields.len() != 3 || fields[1] != "blob" {
        bail!("TASK_MERGE_INVALID: expected a file entry");
    }
    Ok(Some((fields[0].into(), fields[2].into())))
}

/// Verify a file's clean three-way merge from committed inputs without changing the index.
pub fn git_clean_file_merge_matches(
    worktree: &Path,
    ours: &str,
    theirs: &str,
    result: &str,
    path: &str,
) -> Result<bool> {
    let bases = git_stdout(
        worktree,
        &["merge-base", "--all", ours, theirs],
        "TASK_MERGE_INVALID",
    )?;
    let bases = bases.lines().collect::<Vec<_>>();
    if bases.len() != 1 {
        return Ok(false);
    }
    let base = file_entry(worktree, bases[0], path)?;
    let ours = file_entry(worktree, ours, path)?;
    let theirs = file_entry(worktree, theirs, path)?;
    let result = file_entry(worktree, result, path)?;
    if ours == base {
        return Ok(result == theirs);
    }
    if theirs == base || ours == theirs {
        return Ok(result == ours);
    }
    let (Some(base), Some(ours), Some(theirs), Some(result)) = (base, ours, theirs, result) else {
        return Ok(false);
    };
    if !matches!(base.0.as_str(), "100644" | "100755")
        || base.0 != ours.0
        || base.0 != theirs.0
        || base.0 != result.0
    {
        return Ok(false);
    }
    struct MergeFiles(Vec<std::path::PathBuf>);
    impl Drop for MergeFiles {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let mut files = MergeFiles(Vec::new());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    for (index, entry) in [&ours, &base, &theirs].iter().enumerate() {
        let blob = git_command(worktree, &["cat-file", "blob", &entry.1])?;
        if !blob.status.success() {
            bail!("TASK_MERGE_INVALID: file blob is unavailable");
        }
        let path = std::env::temp_dir().join(format!(
            "forge-merge-{}-{nonce}-{index}",
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        files.0.push(path);
        std::io::Write::write_all(&mut file, &blob.stdout)?;
    }
    let paths = files
        .0
        .iter()
        .map(|path| {
            path.to_str()
                .ok_or_else(|| anyhow::anyhow!("TASK_MERGE_INVALID: temporary path is not UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;
    let merged = git_command(
        worktree,
        &["merge-file", "-p", "--", paths[0], paths[1], paths[2]],
    )?;
    if !merged.status.success() {
        return Ok(false);
    }
    let expected = git_command(worktree, &["cat-file", "blob", &result.1])?;
    if !expected.status.success() {
        bail!("TASK_MERGE_INVALID: result blob is unavailable");
    }
    Ok(merged.stdout == expected.stdout)
}
