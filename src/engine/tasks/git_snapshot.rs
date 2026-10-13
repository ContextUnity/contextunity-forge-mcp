use super::*;
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};

static SNAPSHOT_INDEX_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct TemporaryGitIndex(pub(super) PathBuf);

impl TemporaryGitIndex {
    pub(super) fn create() -> Result<Self> {
        loop {
            let sequence = SNAPSHOT_INDEX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "forge-snap-{}-{}-{sequence}.index",
                std::process::id(),
                crate::db::tasks_store::now()
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(index) => {
                    drop(index);
                    return Ok(Self(candidate));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryGitIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(super) struct GitHeadRollback {
    worktree: PathBuf,
    ref_name: String,
    baseline: String,
    landed: String,
    index: Option<GitIndexRollback>,
    armed: bool,
}

impl GitHeadRollback {
    pub(super) fn rollback(&mut self) -> Result<()> {
        if !self.armed && self.index.as_ref().is_none_or(|index| !index.armed) {
            return Ok(());
        }
        let mut failures = Vec::new();
        if self.armed {
            let output = crate::core::git::git_command(
                &self.worktree,
                &[
                    "update-ref",
                    self.ref_name.as_str(),
                    self.baseline.as_str(),
                    self.landed.as_str(),
                ],
            );
            match output {
                Ok(output) if output.status.success() => self.armed = false,
                Ok(output) => failures.push(format!(
                    "branch ref: {}",
                    String::from_utf8_lossy(&output.stderr)
                )),
                Err(error) => failures.push(format!("branch ref: {error}")),
            }
        }
        if let Some(index) = self.index.as_mut() {
            if let Err(error) = index.restore() {
                failures.push(format!("index: {error:#}"));
            }
        }
        if !failures.is_empty() {
            bail!("TASK_DELIVERY_ROLLBACK_FAILED: {}", failures.join("; "));
        }
        Ok(())
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
        if let Some(index) = self.index.as_mut() {
            index.disarm();
        }
    }
}

impl Drop for GitHeadRollback {
    fn drop(&mut self) {
        if self.armed || self.index.as_ref().is_some_and(|index| index.armed) {
            let _ = self.rollback();
        }
    }
}

#[derive(Debug, Clone)]
struct GitIndexEntry {
    mode: String,
    object: String,
    stage: String,
    path: Vec<u8>,
}

struct GitIndexRollback {
    worktree: PathBuf,
    paths: Vec<Vec<u8>>,
    previous: Vec<GitIndexEntry>,
    object_id_len: usize,
    armed: bool,
}

impl GitIndexRollback {
    fn capture(worktree: &Path, paths: Vec<Vec<u8>>) -> Result<Self> {
        let output = crate::core::git::git_command(worktree, &["ls-files", "--stage", "-z"])?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_INDEX_FAILED: could not read the current index: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let object_format = crate::core::git::git_stdout(
            worktree,
            &["rev-parse", "--show-object-format"],
            "TASK_DELIVERY_INDEX_FAILED",
        )?;
        let object_id_len = match object_format.as_str() {
            "sha1" => 40,
            "sha256" => 64,
            _ => {
                bail!("TASK_DELIVERY_INDEX_FAILED: unsupported Git object format '{object_format}'")
            }
        };
        let changed = paths
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let mut previous = Vec::new();
        for record in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            let separator = record
                .iter()
                .position(|byte| *byte == b'\t')
                .context("TASK_DELIVERY_INDEX_FAILED: malformed index entry")?;
            let (header, path_with_separator) = record.split_at(separator);
            let path = &path_with_separator[1..];
            if !changed.contains(path) {
                continue;
            }
            let mut fields = header.split(|byte| *byte == b' ');
            let mode = fields.next().context("malformed index mode")?;
            let object = fields.next().context("malformed index object")?;
            let stage = fields.next().context("malformed index stage")?;
            if fields.next().is_some() {
                bail!("TASK_DELIVERY_INDEX_FAILED: malformed index entry header");
            }
            previous.push(GitIndexEntry {
                mode: String::from_utf8(mode.to_vec()).context("invalid index mode")?,
                object: String::from_utf8(object.to_vec()).context("invalid index object id")?,
                stage: String::from_utf8(stage.to_vec()).context("invalid index stage")?,
                path: path.to_vec(),
            });
        }
        Ok(Self {
            worktree: worktree.to_path_buf(),
            paths,
            previous,
            object_id_len,
            armed: true,
        })
    }

    fn apply(&mut self, commit: &str) -> Result<()> {
        if self.paths.is_empty() {
            self.armed = false;
            return Ok(());
        }
        let mut pathspecs = Vec::new();
        for path in &self.paths {
            pathspecs.extend_from_slice(b":(literal)");
            pathspecs.extend_from_slice(path);
            pathspecs.push(0);
        }
        let mut child = std::process::Command::new("git")
            .args([
                "reset",
                "--quiet",
                commit,
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ])
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .as_mut()
            .context("TASK_DELIVERY_INDEX_FAILED: Git reset stdin is unavailable")?
            .write_all(&pathspecs)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_INDEX_FAILED: could not reconcile delivery paths: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let zero = "0".repeat(self.object_id_len);
        let mut input = Vec::new();
        for path in &self.paths {
            for stage in 0..=3 {
                input.extend_from_slice(format!("0 {zero} {stage}\t").as_bytes());
                input.extend_from_slice(path);
                input.push(0);
            }
        }
        for entry in &self.previous {
            input.extend_from_slice(
                format!("{} {} {}\t", entry.mode, entry.object, entry.stage).as_bytes(),
            );
            input.extend_from_slice(&entry.path);
            input.push(0);
        }
        let mut child = std::process::Command::new("git")
            .args(["update-index", "-z", "--index-info"])
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .as_mut()
            .context("TASK_DELIVERY_ROLLBACK_FAILED: Git index stdin is unavailable")?
            .write_all(&input)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_ROLLBACK_FAILED: could not restore index entries: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        self.armed = false;
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for GitIndexRollback {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.restore();
        }
    }
}

fn run_git_input(
    worktree: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: &[u8],
    error_code: &str,
) -> Result<()> {
    let mut command = std::process::Command::new("git");
    command
        .args(args)
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn()?;
    child
        .stdin
        .as_mut()
        .context("Git command stdin is unavailable")?
        .write_all(input)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!("{error_code}: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn add_candidate_entries_to_index(
    worktree: &Path,
    temp_index: &Path,
    candidate: &str,
) -> Result<()> {
    let output = crate::core::git::git_command(
        worktree,
        &["ls-tree", "-r", "-z", "--full-tree", candidate],
    )?;
    if !output.status.success() {
        bail!(
            "TASK_CANDIDATE_INVALID: could not read candidate entries: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut entries = Vec::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("TASK_CANDIDATE_INVALID: malformed candidate tree entry")?;
        let (header, path_with_separator) = record.split_at(separator);
        let path = &path_with_separator[1..];
        let fields = header.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("TASK_CANDIDATE_INVALID: malformed candidate tree entry header");
        }
        entries.extend_from_slice(fields[0]);
        entries.push(b' ');
        entries.extend_from_slice(fields[2]);
        entries.extend_from_slice(b" 0\t");
        entries.extend_from_slice(path);
        entries.push(0);
    }
    if !entries.is_empty() {
        run_git_input(
            worktree,
            &["update-index", "--add", "-z", "--index-info"],
            Some(temp_index),
            &entries,
            "TASK_DELIVERY_COMMIT_FAILED: could not overlay candidate tree",
        )?;
    }
    Ok(())
}

fn delivery_changed_paths(worktree: &Path, baseline: &str, landed: &str) -> Result<Vec<Vec<u8>>> {
    let output = crate::core::git::git_command(
        worktree,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            "-z",
            baseline,
            landed,
        ],
    )?;
    if !output.status.success() {
        bail!(
            "TASK_DELIVERY_INDEX_FAILED: could not list delivery paths: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

fn commit_tree_and_parent(worktree: &Path, commit: &str) -> Result<(String, String)> {
    if !(40..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("TASK_SNAPSHOT_INVALID: commit must be a full hexadecimal object id");
    }
    let details = crate::core::git::git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_SNAPSHOT_INVALID",
    )?;
    let fields = details.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 {
        bail!("TASK_SNAPSHOT_INVALID: candidate commit must have exactly one baseline parent");
    }
    let treeish = format!("{commit}^{{tree}}");
    let tree =
        crate::core::git::git_stdout(worktree, &["rev-parse", &treeish], "TASK_SNAPSHOT_INVALID")?;
    Ok((tree, fields[1].to_owned()))
}

fn candidate_tree(worktree: &Path, commit: &str) -> Result<String> {
    if !(40..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("TASK_SNAPSHOT_INVALID: commit must be a full hexadecimal object id");
    }
    let details = crate::core::git::git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_SNAPSHOT_INVALID",
    )?;
    if details.split_whitespace().count() != 1 {
        bail!("TASK_CANDIDATE_INVALID: scoped candidate must be a parentless snapshot commit");
    }
    let treeish = format!("{commit}^{{tree}}");
    crate::core::git::git_stdout(worktree, &["rev-parse", &treeish], "TASK_SNAPSHOT_INVALID")
}

pub(super) fn commit_candidate_snapshot(
    worktree: &Path,
    task: &Task,
    candidate: &str,
    candidate_baseline: &str,
    scope: &[String],
    receipt_path: &Path,
) -> Result<(String, Option<GitHeadRollback>)> {
    candidate_tree(worktree, candidate)?;
    let ref_name = crate::core::git::git_command(worktree, &["symbolic-ref", "-q", "HEAD"])?;
    let ref_name = if ref_name.status.success() {
        String::from_utf8_lossy(&ref_name.stdout).trim().to_owned()
    } else {
        "HEAD".to_owned()
    };
    if ref_name != "HEAD" {
        let valid_ref = crate::core::git::git_command(worktree, &["check-ref-format", &ref_name])?;
        if !valid_ref.status.success() {
            bail!("TASK_DELIVERY_COMMIT_FAILED: current branch ref is invalid");
        }
    }
    let temp_index = TemporaryGitIndex::create()?;
    let temp_index_str = temp_index.path().to_string_lossy().to_string();
    let baseline_index = std::process::Command::new("git")
        .args(["read-tree", candidate_baseline])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !baseline_index.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not initialize baseline delivery index: {}",
            String::from_utf8_lossy(&baseline_index.stderr)
        );
    }
    let mut scope_paths = scope.iter().map(String::as_str).collect::<Vec<_>>();
    if scope_paths.is_empty() {
        scope_paths.push(".");
    }
    let mut remove_scoped = std::process::Command::new("git");
    remove_scoped
        .args(["rm", "--cached", "--quiet", "-r", "--ignore-unmatch", "--"])
        .args(&scope_paths)
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree);
    let removed = remove_scoped.output()?;
    if !removed.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not remove baseline scope entries: {}",
            String::from_utf8_lossy(&removed.stderr)
        );
    }
    add_candidate_entries_to_index(worktree, temp_index.path(), candidate)?;
    let receipt_path = receipt_path
        .strip_prefix(worktree)
        .context("TASK_DELIVERY_COMMIT_INVALID: receipt path is outside the worktree")?
        .to_string_lossy()
        .replace('\\', "/");
    let staged_receipt = std::process::Command::new("git")
        .args(["add", "--force", "--", &receipt_path])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !staged_receipt.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not stage the milestone receipt: {}",
            String::from_utf8_lossy(&staged_receipt.stderr)
        );
    }
    let write_tree = std::process::Command::new("git")
        .arg("write-tree")
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !write_tree.status.success() {
        bail!(
            "TASK_DELIVERY_COMMIT_FAILED: could not write delivery tree: {}",
            String::from_utf8_lossy(&write_tree.stderr)
        );
    }
    let expected_tree = String::from_utf8_lossy(&write_tree.stdout)
        .trim()
        .to_owned();
    let head = crate::core::git::git_stdout(
        worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_CANDIDATE_BASELINE_MISMATCH",
    )?;
    if head != candidate_baseline {
        let (head_tree, head_parent) = commit_tree_and_parent(worktree, &head)
            .unwrap_or_else(|_| (String::new(), String::new()));
        if head_tree == expected_tree && head_parent == candidate_baseline {
            return Ok((head, None));
        }
        bail!("TASK_CANDIDATE_BASELINE_MISMATCH: HEAD '{head}' does not match candidate baseline '{candidate_baseline}'");
    }
    let mut message = format!(
        "forge-task: {}\n\nTarget: {}\n\nCompleted subtasks:",
        single_line_commit_text(&task.task_id),
        single_line_commit_text(&task.spec.target),
    );
    let mut completed_subtasks = 0;
    for subtask in task
        .spec
        .subtasks
        .iter()
        .filter(|subtask| subtask.status == "completed")
    {
        message.push_str("\n- ");
        message.push_str(&single_line_commit_text(&subtask.subtask_ref));
        message.push_str(": ");
        message.push_str(&single_line_commit_text(&subtask.title));
        completed_subtasks += 1;
    }
    if completed_subtasks == 0 {
        message.push_str(" none");
    }
    let committed = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", &message])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !committed.status.success() {
        if let Ok(head) = crate::core::git::git_stdout(
            worktree,
            &["rev-parse", "--verify", "HEAD"],
            "TASK_DELIVERY_COMMIT_FAILED",
        ) {
            if let Ok((tree, parent)) = commit_tree_and_parent(worktree, &head) {
                if tree == expected_tree && parent == candidate_baseline {
                    let mut rollback = GitHeadRollback {
                        worktree: worktree.to_path_buf(),
                        ref_name,
                        baseline: candidate_baseline.to_owned(),
                        landed: head,
                        index: None,
                        armed: true,
                    };
                    rollback.rollback()?;
                }
            }
        }
        bail!(
            "TASK_DELIVERY_HOOK_REJECTED: {}\n\nRecovery: The worktree must match the accepted candidate before rejecting delivery. Restore any modified files to the candidate snapshot (e.g. via git checkout/show from candidate SHA), then submit delivery rejection ('task submit --stage deliver --action reject') to return the task to build stage. Do not edit or format files during deliver stage.",
            String::from_utf8_lossy(&committed.stderr)
        );
    }
    let landed = crate::core::git::git_stdout(
        worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_DELIVERY_COMMIT_FAILED",
    )?;
    let mut rollback = GitHeadRollback {
        worktree: worktree.to_path_buf(),
        ref_name,
        baseline: candidate_baseline.to_owned(),
        landed: landed.clone(),
        index: None,
        armed: true,
    };
    let (landed_tree, landed_parent) = match commit_tree_and_parent(worktree, &landed) {
        Ok(details) => details,
        Err(error) => {
            rollback.rollback()?;
            return Err(error);
        }
    };
    if landed_tree != expected_tree || landed_parent != candidate_baseline {
        rollback.rollback()?;
        bail!("TASK_DELIVERY_COMMIT_INVALID: hooks or concurrent changes altered the candidate tree or parent");
    }
    let changed_paths = match delivery_changed_paths(worktree, candidate_baseline, &landed) {
        Ok(paths) => paths,
        Err(error) => {
            rollback.rollback()?;
            return Err(error);
        }
    };
    let mut index_rollback = match GitIndexRollback::capture(worktree, changed_paths) {
        Ok(index) => index,
        Err(error) => {
            rollback.rollback()?;
            return Err(error);
        }
    };
    if let Err(error) = index_rollback.apply(&landed) {
        index_rollback
            .restore()
            .context("TASK_DELIVERY_ROLLBACK_FAILED")?;
        rollback.rollback()?;
        return Err(error);
    }
    rollback.index = Some(index_rollback);
    Ok((landed, Some(rollback)))
}

fn single_line_commit_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn capture_scoped_snapshot(
    worktree: &Path,
    task_id: &str,
    scope: &[String],
) -> Result<Option<(String, String)>> {
    let head_out = crate::core::git::git_command(worktree, &["rev-parse", "HEAD"]);
    let head = match head_out {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                return Ok(None);
            }
            s
        }
        _ => return Ok(None),
    };

    let temp_index = TemporaryGitIndex::create()?;
    let temp_index_str = temp_index.path().to_string_lossy().to_string();

    // Keep unrelated branch content out of this task's candidate tree.
    let read_tree = std::process::Command::new("git")
        .args(["read-tree", "--empty"])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !read_tree.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: git read-tree failed: {}",
            String::from_utf8_lossy(&read_tree.stderr)
        );
    }

    let mut valid_paths = Vec::with_capacity(scope.len());
    for path in scope {
        match std::fs::symlink_metadata(worktree.join(path)) {
            Ok(_) => valid_paths.push(path.as_str()),
            // The temporary index is empty, so a deleted scoped path is
            // already absent from the candidate tree and must not be passed
            // to `git add` as a pathspec.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("TASK_SNAPSHOT_FAILED: could not inspect scoped path {path}")
                });
            }
        }
    }

    if !valid_paths.is_empty() {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["add", "-A", "--"]);
        cmd.args(&valid_paths);
        cmd.env("GIT_INDEX_FILE", &temp_index_str);
        cmd.current_dir(worktree);
        let out = cmd.output()?;
        if !out.status.success() {
            bail!(
                "TASK_SNAPSHOT_FAILED: git add failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    } else if scope.is_empty() {
        let out = std::process::Command::new("git")
            .args(["add", "-A"])
            .env("GIT_INDEX_FILE", &temp_index_str)
            .current_dir(worktree)
            .output()?;
        if !out.status.success() {
            bail!(
                "TASK_SNAPSHOT_FAILED: git add -A failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    let write_tree_out = std::process::Command::new("git")
        .args(["write-tree"])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .current_dir(worktree)
        .output()?;
    if !write_tree_out.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: git write-tree failed: {}",
            String::from_utf8_lossy(&write_tree_out.stderr)
        );
    }
    let tree_sha = String::from_utf8_lossy(&write_tree_out.stdout)
        .trim()
        .to_string();
    if tree_sha.is_empty() {
        bail!("TASK_SNAPSHOT_FAILED: empty tree SHA from write-tree");
    }

    let commit_msg = format!("forge-snapshot: {task_id}");
    let commit_out = std::process::Command::new("git")
        .args(["commit-tree", &tree_sha, "-m", &commit_msg])
        .env("GIT_INDEX_FILE", &temp_index_str)
        .env("GIT_AUTHOR_NAME", "forge-agent")
        .env("GIT_AUTHOR_EMAIL", "forge@contextunity.local")
        .env("GIT_COMMITTER_NAME", "forge-agent")
        .env("GIT_COMMITTER_EMAIL", "forge@contextunity.local")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .current_dir(worktree)
        .output()?;
    if !commit_out.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: git commit-tree failed: {}",
            String::from_utf8_lossy(&commit_out.stderr)
        );
    }
    let commit_sha = String::from_utf8_lossy(&commit_out.stdout)
        .trim()
        .to_string();
    if !(7..=64).contains(&commit_sha.len()) {
        bail!("TASK_SNAPSHOT_FAILED: invalid commit SHA length: {commit_sha}");
    }

    Ok(Some((commit_sha, head)))
}
