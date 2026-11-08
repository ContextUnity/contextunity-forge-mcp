use super::*;
use std::{
    fs::{File, OpenOptions},
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
                crate::store::now()
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
            let output = crate::git::git_command(
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
        let output = crate::git::git_command(worktree, &["ls-files", "--stage", "-z"])?;
        if !output.status.success() {
            bail!(
                "TASK_DELIVERY_INDEX_FAILED: could not read the current index: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let object_format = crate::git::git_stdout(
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
    let output =
        crate::git::git_command(worktree, &["ls-tree", "-r", "-z", "--full-tree", candidate])?;
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
    let output = crate::git::git_command(
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
    let details = crate::git::git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_SNAPSHOT_INVALID",
    )?;
    let fields = details.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 {
        bail!("TASK_SNAPSHOT_INVALID: candidate commit must have exactly one baseline parent");
    }
    let treeish = format!("{commit}^{{tree}}");
    let tree = crate::git::git_stdout(worktree, &["rev-parse", &treeish], "TASK_SNAPSHOT_INVALID")?;
    Ok((tree, fields[1].to_owned()))
}

fn candidate_tree(worktree: &Path, commit: &str) -> Result<String> {
    if !(40..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("TASK_SNAPSHOT_INVALID: commit must be a full hexadecimal object id");
    }
    let details = crate::git::git_stdout(
        worktree,
        &["rev-list", "--parents", "-n", "1", commit],
        "TASK_SNAPSHOT_INVALID",
    )?;
    if details.split_whitespace().count() != 1 {
        bail!("TASK_CANDIDATE_INVALID: scoped candidate must be a parentless snapshot commit");
    }
    let treeish = format!("{commit}^{{tree}}");
    crate::git::git_stdout(worktree, &["rev-parse", &treeish], "TASK_SNAPSHOT_INVALID")
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
    let ref_name = crate::git::git_command(worktree, &["symbolic-ref", "-q", "HEAD"])?;
    let ref_name = if ref_name.status.success() {
        String::from_utf8_lossy(&ref_name.stdout).trim().to_owned()
    } else {
        "HEAD".to_owned()
    };
    if ref_name != "HEAD" {
        let valid_ref = crate::git::git_command(worktree, &["check-ref-format", &ref_name])?;
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
    let head = crate::git::git_stdout(
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
        if let Ok(head) = crate::git::git_stdout(
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
    let landed = crate::git::git_stdout(
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
) -> Result<Option<(String, String, crate::git::KeptObjects)>> {
    capture_scoped_snapshot_excluding(worktree, task_id, scope, &[])
}

fn capture_scoped_snapshot_excluding(
    worktree: &Path,
    task_id: &str,
    scope: &[String],
    excluded_paths: &[String],
) -> Result<Option<(String, String, crate::git::KeptObjects)>> {
    let head_out = crate::git::git_command(worktree, &["rev-parse", "HEAD"]);
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

    let gc = crate::git::GitGcProtection::acquire(worktree)?;
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
        add_unscoped_snapshot_entries(worktree, temp_index.path(), excluded_paths)?;
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
    // Materialize exactly the deterministic parentless commit while Git GC is
    // excluded, then transfer it and its tree into a kept pack. The ordinary
    // commit-tree invocation remains the identity check/public process boundary;
    // even GC inside its stdout adapter can now run successfully.
    let commit_bytes=format!("tree {tree_sha}\nauthor forge-agent <forge@contextunity.local> 946684800 +0000\ncommitter forge-agent <forge@contextunity.local> 946684800 +0000\n\n{commit_msg}\n");
    let protected = crate::git::git_input(
        worktree,
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        commit_bytes.as_bytes(),
    )?;
    let objects = crate::git::keep_provisional(worktree, task_id, &protected, &gc)?;
    drop(gc);
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
    if commit_sha != protected {
        bail!("TASK_SNAPSHOT_FAILED: protected commit identity mismatch");
    }

    Ok(Some((commit_sha, head, objects)))
}

fn add_unscoped_snapshot_entries(
    worktree: &Path,
    index: &Path,
    excluded_paths: &[String],
) -> Result<()> {
    let listed = crate::git::git_command(
        worktree,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    if !listed.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: could not enumerate worktree paths: {}",
            String::from_utf8_lossy(&listed.stderr)
        );
    }
    let mut paths = Vec::new();
    for path in listed
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        if excluded_paths.iter().any(|excluded| {
            path == excluded.as_bytes() || (excluded == ".forge" && path.starts_with(b".forge/"))
        }) {
            continue;
        }
        #[cfg(unix)]
        let exists = {
            use std::os::unix::ffi::OsStrExt;
            std::fs::symlink_metadata(worktree.join(std::ffi::OsStr::from_bytes(path)))
        };
        #[cfg(not(unix))]
        let exists =
            { std::fs::symlink_metadata(worktree.join(String::from_utf8_lossy(path).as_ref())) };
        match exists {
            Ok(_) => paths.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).context("TASK_SNAPSHOT_FAILED: could not inspect worktree path")
            }
        }
    }

    for batch in paths.chunks(256) {
        let mut add = std::process::Command::new("git");
        add.args(["add", "-A", "--"]);
        for path in batch {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                let mut pathspec = b":(literal)".to_vec();
                pathspec.extend_from_slice(path);
                add.arg(std::ffi::OsString::from_vec(pathspec));
            }
            #[cfg(not(unix))]
            {
                let path = String::from_utf8(path.to_vec())
                    .context("TASK_SNAPSHOT_FAILED: non-UTF-8 path on this platform")?;
                add.arg(format!(":(literal){path}"));
            }
        }
        let output = add
            .env("GIT_INDEX_FILE", index.to_string_lossy().as_ref())
            .current_dir(worktree)
            .output()?;
        if !output.status.success() {
            bail!(
                "TASK_SNAPSHOT_FAILED: git add -A failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    Ok(())
}

struct NamedSnapshotContext {
    worktree: PathBuf,
    namespace: String,
    reference: String,
}

struct NamedSnapshotLock {
    _file: File,
    #[cfg(not(unix))]
    path: PathBuf,
}

impl Drop for NamedSnapshotLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = self._file.unlock();
        #[cfg(not(unix))]
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_named_snapshot_operation(worktree: &Path) -> Result<NamedSnapshotLock> {
    let git_dir = crate::git::git_stdout(
        worktree,
        &["rev-parse", "--absolute-git-dir"],
        "TASK_SNAPSHOT_LOCK_FAILED",
    )?;
    let lock_path = PathBuf::from(git_dir).join("forge-task-manage-snapshot.lock");

    #[cfg(unix)]
    {
        use std::{os::fd::AsRawFd, os::unix::fs::OpenOptionsExt};

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .with_context(|| {
                format!(
                    "TASK_SNAPSHOT_LOCK_FAILED: could not open per-worktree lock {}",
                    lock_path.display()
                )
            })?;
        if !file.metadata()?.is_file() {
            bail!("TASK_SNAPSHOT_LOCK_FAILED: per-worktree lock is not a regular file");
        }
        // The open file description holds the cross-process lock until this guard drops.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                bail!(
                    "TASK_SNAPSHOT_BUSY: another snapshot or rollback is active in this worktree"
                );
            }
            bail!("TASK_SNAPSHOT_LOCK_FAILED: could not lock worktree snapshot state: {error}");
        }
        Ok(NamedSnapshotLock { _file: file })
    }

    #[cfg(not(unix))]
    {
        let file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "TASK_SNAPSHOT_BUSY: another snapshot or rollback is active in this worktree"
                );
            }
            Err(error) => {
                bail!(
                    "TASK_SNAPSHOT_LOCK_FAILED: could not create worktree snapshot lock: {error}"
                );
            }
        };
        Ok(NamedSnapshotLock {
            _file: file,
            path: lock_path,
        })
    }
}

fn snapshot_tree(worktree: &Path, commit: &str) -> Result<String> {
    let treeish = format!("{commit}^{{tree}}");
    crate::git::git_stdout(
        worktree,
        &["rev-parse", "--verify", treeish.as_str()],
        "TASK_SNAPSHOT_STATE_CHANGED",
    )
}

fn ensure_named_snapshot_state(
    worktree: &Path,
    expected_head: &str,
    expected_index_tree: &str,
    expected_worktree_tree: &str,
    excluded_paths: &[String],
    name: &str,
) -> Result<()> {
    let head = crate::git::git_stdout(
        worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_STATE_CHANGED",
    )?;
    if head != expected_head {
        bail!("TASK_SNAPSHOT_STATE_CHANGED: Git HEAD changed before rollback was applied");
    }
    let index_tree =
        crate::git::git_stdout(worktree, &["write-tree"], "TASK_SNAPSHOT_STATE_CHANGED")?;
    if index_tree != expected_index_tree {
        bail!("TASK_SNAPSHOT_STATE_CHANGED: Git index changed before rollback was applied");
    }
    let guard_id = format!(
        "task-manage-fence:{}:{}:{}:{}:{}",
        name,
        expected_head,
        std::process::id(),
        crate::store::now(),
        SNAPSHOT_INDEX_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let (guard_commit, captured_head, kept) =
        capture_named_snapshot(worktree, &guard_id, excluded_paths)?;
    let current_head = crate::git::git_stdout(
        worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_STATE_CHANGED",
    )?;
    if captured_head != expected_head || current_head != expected_head {
        bail!("TASK_SNAPSHOT_STATE_CHANGED: Git HEAD changed while fencing rollback state");
    }
    let current_index_tree =
        crate::git::git_stdout(worktree, &["write-tree"], "TASK_SNAPSHOT_STATE_CHANGED")?;
    if current_index_tree != expected_index_tree {
        bail!("TASK_SNAPSHOT_STATE_CHANGED: Git index changed while fencing rollback state");
    }
    if snapshot_tree(worktree, &guard_commit)? != expected_worktree_tree {
        bail!("TASK_SNAPSHOT_STATE_CHANGED: working tree changed after recovery snapshot capture");
    }
    drop(kept);
    Ok(())
}

fn named_snapshot_context(worktree: &Path, name: &str) -> Result<NamedSnapshotContext> {
    if crate::tasks::valid_identity(name).is_err() {
        bail!("TASK_SNAPSHOT_INVALID_NAME: snapshot name must be a Git-safe identity");
    }
    let selected_root = worktree.canonicalize()?;
    let top_level = crate::git::git_stdout(
        &selected_root,
        &["rev-parse", "--show-toplevel"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    let worktree = PathBuf::from(top_level).canonicalize()?;
    let namespace = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(worktree.to_string_lossy().as_bytes())
    );
    let reference = format!("refs/forge/snapshots/{namespace}/{name}");
    let check = crate::git::git_command(&worktree, &["check-ref-format", reference.as_str()])?;
    if !check.status.success() {
        bail!("TASK_SNAPSHOT_INVALID_NAME: snapshot name is not valid in a Git reference");
    }
    Ok(NamedSnapshotContext {
        worktree,
        namespace,
        reference,
    })
}

fn named_snapshot_exclusions(worktree: &Path, database: &Path) -> Vec<String> {
    let mut paths = vec![".forge".to_owned()];
    if let Ok(relative) = database.strip_prefix(worktree) {
        let database = relative.to_string_lossy().replace('\\', "/");
        if !database.is_empty() && database != ".forge" && !database.starts_with(".forge/") {
            paths.extend([
                database.clone(),
                format!("{database}-journal"),
                format!("{database}-wal"),
                format!("{database}-shm"),
                format!("{database}.lock"),
            ]);
        }
    }
    paths
}

fn ensure_snapshot_exclusions_untracked(worktree: &Path, excluded_paths: &[String]) -> Result<()> {
    let output = crate::git::git_command(worktree, &["ls-files", "-z"])?;
    if !output.status.success() {
        bail!(
            "TASK_SNAPSHOT_FAILED: could not inspect tracked runtime paths: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let tracked_runtime_path = output.stdout.split(|byte| *byte == 0).find(|tracked| {
        excluded_paths.iter().any(|excluded| {
            *tracked == excluded.as_bytes()
                || (excluded == ".forge" && (*tracked).starts_with(b".forge/"))
        })
    });
    if let Some(path) = tracked_runtime_path {
        bail!(
            "TASK_SNAPSHOT_RUNTIME_PATH_TRACKED: refusing to snapshot or rollback while runtime path '{}' is tracked",
            String::from_utf8_lossy(path)
        );
    }
    Ok(())
}

fn named_snapshot_ref_exists(worktree: &Path, reference: &str) -> Result<bool> {
    let output =
        crate::git::git_command(worktree, &["show-ref", "--verify", "--quiet", reference])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "TASK_SNAPSHOT_FAILED: could not inspect snapshot ref '{}': {}",
            reference,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn capture_named_snapshot(
    worktree: &Path,
    snapshot_id: &str,
    excluded_paths: &[String],
) -> Result<(String, String, crate::git::KeptObjects)> {
    capture_scoped_snapshot_excluding(worktree, snapshot_id, &[], excluded_paths)?
        .context("TASK_SNAPSHOT_FAILED: no Git HEAD is available for a named snapshot")
}

fn clean_exclude_pattern(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len() + 1);
    for character in path.chars() {
        if "\\*?[]".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    if path == ".forge" {
        format!("/{escaped}/")
    } else {
        format!("/{escaped}")
    }
}

fn restore_named_snapshot_tree(
    worktree: &Path,
    commit: &str,
    excluded_paths: &[String],
) -> Result<()> {
    let mut clean_args = vec!["clean".to_owned(), "-d".to_owned(), "-f".to_owned()];
    for path in excluded_paths {
        clean_args.push("-e".to_owned());
        clean_args.push(clean_exclude_pattern(path));
    }
    clean_args.extend(["--".to_owned(), ".".to_owned()]);
    let clean_args = clean_args.iter().map(String::as_str).collect::<Vec<_>>();
    let clean = crate::git::git_command(worktree, &clean_args)?;
    if !clean.status.success() {
        bail!(
            "TASK_SNAPSHOT_ROLLBACK_FAILED: could not remove later untracked files: {}",
            String::from_utf8_lossy(&clean.stderr)
        );
    }
    let read_tree = crate::git::git_command(worktree, &["read-tree", "--reset", "-u", commit])?;
    if !read_tree.status.success() {
        bail!(
            "TASK_SNAPSHOT_ROLLBACK_FAILED: could not restore the snapshot tree: {}",
            String::from_utf8_lossy(&read_tree.stderr)
        );
    }
    Ok(())
}

pub(super) fn create_named_snapshot(worktree: &Path, database: &Path, name: &str) -> Result<Value> {
    let context = named_snapshot_context(worktree, name)?;
    let _snapshot_lock = lock_named_snapshot_operation(&context.worktree)?;
    let excluded_paths = named_snapshot_exclusions(&context.worktree, database);
    ensure_snapshot_exclusions_untracked(&context.worktree, &excluded_paths)?;
    if named_snapshot_ref_exists(&context.worktree, &context.reference)? {
        bail!("TASK_SNAPSHOT_ALREADY_EXISTS: snapshot '{name}' already exists in this worktree");
    }
    let baseline = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    let snapshot_id = format!("task-manage:{}:{}:{}", context.namespace, name, baseline);
    let (commit, captured_head, kept) =
        capture_named_snapshot(&context.worktree, &snapshot_id, &excluded_paths)?;
    let current_head = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    if captured_head != baseline || current_head != baseline {
        bail!("TASK_SNAPSHOT_HEAD_CHANGED: Git HEAD changed while creating the named snapshot");
    }
    let object_format = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--show-object-format"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    let empty_object = match object_format.as_str() {
        "sha1" => "0000000000000000000000000000000000000000",
        "sha256" => "0000000000000000000000000000000000000000000000000000000000000000",
        _ => bail!("TASK_SNAPSHOT_FAILED: unsupported Git object format '{object_format}'"),
    };
    let update = crate::git::git_command(
        &context.worktree,
        &[
            "update-ref",
            context.reference.as_str(),
            commit.as_str(),
            empty_object,
        ],
    )?;
    if !update.status.success() {
        if named_snapshot_ref_exists(&context.worktree, &context.reference)? {
            bail!(
                "TASK_SNAPSHOT_ALREADY_EXISTS: snapshot '{name}' already exists in this worktree"
            );
        }
        bail!(
            "TASK_SNAPSHOT_CREATE_FAILED: could not publish named snapshot: {}",
            String::from_utf8_lossy(&update.stderr)
        );
    }
    drop(kept);
    Ok(json!({
        "action":"snapshot",
        "snapshot_name":name,
        "snapshot_ref":context.reference,
        "snapshot_commit":commit,
        "head":baseline,
    }))
}

pub(super) fn rollback_named_snapshot(
    worktree: &Path,
    database: &Path,
    name: &str,
) -> Result<Value> {
    let context = named_snapshot_context(worktree, name)?;
    let _snapshot_lock = lock_named_snapshot_operation(&context.worktree)?;
    let excluded_paths = named_snapshot_exclusions(&context.worktree, database);
    ensure_snapshot_exclusions_untracked(&context.worktree, &excluded_paths)?;
    if !named_snapshot_ref_exists(&context.worktree, &context.reference)? {
        bail!("TASK_SNAPSHOT_NOT_FOUND: named snapshot '{name}' does not exist in this worktree");
    }
    let commitish = format!("{}^{{commit}}", context.reference);
    let commit = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--verify", commitish.as_str()],
        "TASK_SNAPSHOT_INVALID",
    )?;
    let object_type = crate::git::git_stdout(
        &context.worktree,
        &["cat-file", "-t", commit.as_str()],
        "TASK_SNAPSHOT_INVALID",
    )?;
    if object_type != "commit" {
        bail!("TASK_SNAPSHOT_INVALID: named snapshot ref does not point to a commit");
    }
    let parents = crate::git::git_stdout(
        &context.worktree,
        &["rev-list", "--parents", "-n", "1", commit.as_str()],
        "TASK_SNAPSHOT_INVALID",
    )?;
    if parents.split_whitespace().count() != 1 {
        bail!("TASK_SNAPSHOT_INVALID: named snapshot commit must be parentless");
    }
    let message = crate::git::git_stdout(
        &context.worktree,
        &["show", "-s", "--format=%B", commit.as_str()],
        "TASK_SNAPSHOT_INVALID",
    )?;
    let marker_prefix = format!(
        "forge-snapshot: task-manage:{}:{}:",
        context.namespace, name
    );
    let Some(snapshot_head) = message.strip_prefix(&marker_prefix) else {
        bail!("TASK_SNAPSHOT_INVALID: snapshot commit does not match this name and worktree");
    };
    let head_len = match crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--show-object-format"],
        "TASK_SNAPSHOT_INVALID",
    )?
    .as_str()
    {
        "sha1" => 40,
        "sha256" => 64,
        format => bail!("TASK_SNAPSHOT_INVALID: unsupported Git object format '{format}'"),
    };
    if snapshot_head.len() != head_len
        || !snapshot_head.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("TASK_SNAPSHOT_INVALID: snapshot commit has an invalid baseline head");
    }
    let baseline = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    if baseline != snapshot_head {
        bail!(
            "TASK_SNAPSHOT_HEAD_CHANGED: snapshot '{name}' belongs to HEAD '{}', current HEAD is '{}'; rollback was not applied",
            snapshot_head,
            baseline
        );
    }
    let index_tree = crate::git::git_stdout(
        &context.worktree,
        &["write-tree"],
        "TASK_SNAPSHOT_STATE_CHANGED",
    )?;
    let recovery_id = format!(
        "task-manage-recovery:{}:{}:{}:{}:{}",
        context.namespace,
        name,
        baseline,
        std::process::id(),
        crate::store::now()
    );
    let (recovery_commit, captured_head, recovery_kept) =
        capture_named_snapshot(&context.worktree, &recovery_id, &excluded_paths)?;
    let current_head = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    if captured_head != baseline || current_head != baseline {
        bail!(
            "TASK_SNAPSHOT_HEAD_CHANGED: Git HEAD changed before rollback; no snapshot was applied"
        );
    }
    let recovery_tree = snapshot_tree(&context.worktree, &recovery_commit)?;
    ensure_named_snapshot_state(
        &context.worktree,
        &baseline,
        &index_tree,
        &recovery_tree,
        &excluded_paths,
        name,
    )?;
    if let Err(restore_error) =
        restore_named_snapshot_tree(&context.worktree, &commit, &excluded_paths)
    {
        return match restore_named_snapshot_tree(
            &context.worktree,
            &recovery_commit,
            &excluded_paths,
        ) {
            Ok(()) => Err(anyhow::anyhow!(
                "TASK_SNAPSHOT_ROLLBACK_FAILED: named snapshot could not be applied; the original worktree was restored: {restore_error:#}"
            )),
            Err(recovery_error) => Err(anyhow::anyhow!(
                "TASK_SNAPSHOT_ROLLBACK_FAILED: named snapshot failed: {restore_error:#}; recovery failed: {recovery_error:#}"
            )),
        };
    }
    let final_head = crate::git::git_stdout(
        &context.worktree,
        &["rev-parse", "--verify", "HEAD"],
        "TASK_SNAPSHOT_FAILED",
    )?;
    if final_head != baseline {
        let recovery =
            restore_named_snapshot_tree(&context.worktree, &recovery_commit, &excluded_paths);
        return match recovery {
            Ok(()) => Err(anyhow::anyhow!(
                "TASK_SNAPSHOT_HEAD_CHANGED: Git HEAD changed during rollback; the original worktree was restored"
            )),
            Err(recovery_error) => Err(anyhow::anyhow!(
                "TASK_SNAPSHOT_ROLLBACK_FAILED: Git HEAD changed during rollback and worktree recovery failed: {recovery_error:#}"
            )),
        };
    }
    drop(recovery_kept);
    Ok(json!({
        "action":"rollback",
        "snapshot_name":name,
        "snapshot_ref":context.reference,
        "snapshot_commit":commit,
        "head":baseline,
        "restored":true,
    }))
}
