use anyhow::{bail, Result};
use std::{
    io::Write,
    path::Path,
    process::{Command, ExitStatus, Output},
};

/// Short, fail-closed participation in Git's own repository GC exclusion protocol.
/// Held only while constructing a kept pack, never across a SQLite transaction.
pub(crate) struct GitGcProtection {
    path: std::path::PathBuf,
    token: String,
}

impl GitGcProtection {
    pub(crate) fn acquire(root: &Path) -> Result<Self> {
        let common = git_stdout(
            root,
            &["rev-parse", "--git-common-dir"],
            "TASK_OBJECT_PROTECTION_FAILED",
        )?;
        let common = root.join(common).canonicalize()?;
        let path = common.join("gc.pid");
        let lock = common.join("gc.pid.lock");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)?;
        let acquired = (|| -> Result<String> {
            if path.exists() {
                bail!("TASK_OBJECT_PROTECTION_FAILED: Git GC already owns the repository");
            }
            let hostname = Command::new("hostname").output()?;
            if !hostname.status.success() {
                bail!("TASK_OBJECT_PROTECTION_FAILED: hostname unavailable");
            }
            let hostname = String::from_utf8(hostname.stdout)?;
            let token = format!("{} {}", std::process::id(), hostname.trim());
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&lock, &path)?;
            std::fs::File::open(&common)?.sync_all()?;
            Ok(token)
        })();
        if acquired.is_err() {
            let _ = std::fs::remove_file(lock);
        }
        Ok(Self {
            path,
            token: acquired?,
        })
    }
}

impl Drop for GitGcProtection {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).ok().as_ref() == Some(&self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionalPackOwner {
    kind: String,
    task_id: String,
    object: String,
}

/// A genuine task's provisional object home, independent of logical gate authority.
pub(crate) struct KeptObjects {
    path: std::path::PathBuf,
    retain: bool,
}
impl KeptObjects {
    /// A committed SQL pending publication must retain its object home across process exit.
    pub(crate) fn retain_pending(&mut self) {
        self.retain = true;
    }
}
impl Drop for KeptObjects {
    fn drop(&mut self) {
        if !self.retain {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// GC exclusion must already cover object construction. Transfer protection to a
/// unique kept pack before releasing exclusion or returning commit-tree stdout.
pub(crate) fn keep_provisional(
    root: &Path,
    task_id: &str,
    object: &str,
    _gc: &GitGcProtection,
) -> Result<KeptObjects> {
    let objects = git_stdout(
        root,
        &["rev-parse", "--git-path", "objects"],
        "TASK_OBJECT_PROTECTION_FAILED",
    )?;
    let directory = root.join(objects).canonicalize()?.join("pack");
    std::fs::create_dir_all(&directory)?;
    // A distinct, genuine ownership blob prevents two concurrent guards for an
    // identical candidate from sharing (and prematurely releasing) a keep file.
    let owner = ProvisionalPackOwner {
        kind: "forge-provisional-v1".into(),
        task_id: task_id.into(),
        object: object.into(),
    };
    let bytes = serde_json::to_vec(&owner)?;
    let nonce = format!(
        "{}:{}:{}",
        String::from_utf8(bytes.clone())?,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let blob = git_input(root, &["hash-object", "-w", "--stdin"], nonce.as_bytes())?;
    let prefix = directory.join("pack");
    let packed = git_input(
        root,
        &[
            "pack-objects",
            "--revs",
            prefix
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("non-UTF8 object home"))?,
        ],
        format!("{object}\n{blob}\n").as_bytes(),
    )?;
    if !matches!(packed.len(), 40 | 64) || !packed.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("TASK_OBJECT_PROTECTION_FAILED: invalid pack identity");
    }
    let path = directory.join(format!("pack-{packed}.keep"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::File::open(&directory)?.sync_all()?;
    Ok(KeptObjects {
        path,
        retain: false,
    })
}

/// Release only this task's provisional packs once their objects are actual
/// ancestors of its durable keeper. Unrelated packs and owners are untouched.
pub(crate) fn release_provisional(root: &Path, task_id: &str, keeper: &str) -> Result<()> {
    let directory = root.join(git_stdout(
        root,
        &["rev-parse", "--git-path", "objects/pack"],
        "TASK_OBJECT_PROTECTION_FAILED",
    )?);
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "keep") {
            continue;
        }
        let Ok(owner) = serde_json::from_slice::<ProvisionalPackOwner>(&std::fs::read(&path)?)
        else {
            continue;
        };
        if owner.kind == "forge-provisional-v1"
            && owner.task_id == task_id
            && git_ancestor(root, &owner.object, keeper)?
        {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Write a Git object using explicit bytes on stdin.
pub fn git_input(worktree: &Path, args: &[&str], bytes: &[u8]) -> Result<String> {
    use std::process::Stdio;
    let mut child = Command::new("git")
        .args(["-c", "core.fsync=committed"])
        .args(args)
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().expect("piped stdin").write_all(bytes)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "TASK_CHECKPOINT_GIT_FAILED: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Publish a checkpoint blob as a commit whose parents retain the supplied objects.
/// The caller owns the ref and its publication fence; this function never creates global pins.
pub fn git_checkpoint(
    root: &Path,
    reference: &str,
    bytes: &[u8],
    parents: &[String],
) -> Result<String> {
    if !root.is_absolute() {
        bail!("TASK_CHECKPOINT_HOME_INVALID: object home must be an absolute worktree");
    }
    let blob = git_input(root, &["hash-object", "-w", "--stdin"], bytes)?;
    let tree = git_input(
        root,
        &["mktree"],
        format!("100644 blob {blob}\tcheckpoint.json\n").as_bytes(),
    )?;
    let mut args = vec![
        "-c",
        "user.name=Forge checkpoint",
        "-c",
        "user.email=checkpoint@forge.invalid",
        "commit-tree",
        &tree,
    ];
    for parent in parents {
        args.extend(["-p", parent]);
    }
    let commit = git_input(root, &args, b"Task checkpoint\n")?;
    let previous = git_command(root, &["rev-parse", "--verify", reference])?;
    let previous = if previous.status.success() {
        String::from_utf8(previous.stdout)?.trim().to_owned()
    } else {
        "0".repeat(commit.len())
    };
    let output = git_command(
        root,
        &[
            "-c",
            "core.fsync=committed",
            "update-ref",
            "--create-reflog",
            reference,
            &commit,
            &previous,
        ],
    )?;
    if !output.status.success() {
        bail!(
            "TASK_CHECKPOINT_GIT_FAILED: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(commit)
}

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

/// Validate a resolved proof object. Retention belongs to its task keeper.
pub fn retain_proof_commit(worktree: &Path, commit: &str) -> Result<()> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("TASK_PROOF_INVALID: expected a resolved proof object ID");
    }
    if !git_commit_exists(worktree, commit)? {
        bail!("TASK_PROOF_RETENTION_FAILED: proof object {commit} is unavailable");
    }
    Ok(())
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
