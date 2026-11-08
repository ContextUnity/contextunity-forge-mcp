use super::*;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn git_bytes(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = git_command(root).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

pub(crate) fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

pub(crate) fn git_checked(root: &Path, args: &[&str]) {
    git_bytes(root, args);
}

/// Owns the real Git workspace and filesystem setup for named task snapshots.
pub(crate) struct NamedSnapshotHarness {
    root: Workspace,
}

impl NamedSnapshotHarness {
    pub(crate) fn new() -> Self {
        let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: runtime/tasks.sqlite\n",
        );
        root.write("src/lib.rs", "snapshot baseline\n");
        root.write(".forge/managed-state", "preserve Forge state\n");
        root.write(".git/info/exclude", "\n.forge/\n");
        git_checked(root.root(), &["add", "forge-mcp.yaml", "src/lib.rs"]);
        git_checked(root.root(), &["commit", "-m", "snapshot fixture baseline"]);
        Self { root }
    }

    pub(crate) fn manage(&self, action: &str, name: &str) -> anyhow::Result<serde_json::Value> {
        contextunity_forge_mcp::engine::tasks::manage(
            self.root.root(),
            serde_json::from_value(serde_json::json!({ "action": action, "name": name }))?,
        )
    }

    pub(crate) fn head(&self) -> String {
        crate::acdd::support::git_head(self.root.root())
    }

    pub(crate) fn write(&self, path: &str, contents: &str) {
        self.root.write(path, contents);
    }

    pub(crate) fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.path(path)).unwrap()
    }

    pub(crate) fn exists(&self, path: &str) -> bool {
        self.root.path(path).exists()
    }

    pub(crate) fn status_porcelain(&self) -> String {
        String::from_utf8(git_bytes(self.root.root(), &["status", "--porcelain"]))
            .expect("Git status must be UTF-8")
    }

    pub(crate) fn commit_file(&self, path: &str, message: &str) {
        git_checked(self.root.root(), &["add", path]);
        git_checked(self.root.root(), &["commit", "-m", message]);
    }

    #[cfg(unix)]
    pub(crate) fn snapshot_lock_path(&self) -> PathBuf {
        let git_dir = String::from_utf8(git_bytes(
            self.root.root(),
            &["rev-parse", "--absolute-git-dir"],
        ))
        .expect("Git directory path must be UTF-8");
        PathBuf::from(git_dir.trim()).join("forge-task-manage-snapshot.lock")
    }

    #[cfg(unix)]
    pub(crate) fn lock_snapshot_operation(&self) -> std::io::Result<NamedSnapshotLock> {
        use std::{fs::OpenOptions, os::fd::AsRawFd};

        let path = self.snapshot_lock_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        // SAFETY: this passes a live file descriptor to `flock`; the returned guard
        // retains the descriptor until it releases the lock in `Drop`.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(NamedSnapshotLock { file })
    }
}

#[cfg(unix)]
pub(crate) struct NamedSnapshotLock {
    file: std::fs::File,
}

#[cfg(unix)]
impl Drop for NamedSnapshotLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the file descriptor remains owned by this live guard.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}
