use super::*;
use std::path::Path;

pub(crate) struct SeedGitTemplate {
    permissions: std::fs::Permissions,
    entries: Vec<GitTemplateEntry>,
}

struct GitTemplateEntry {
    path: std::path::PathBuf,
    permissions: std::fs::Permissions,
    contents: Option<SeedFile>,
}

pub(crate) struct SeedFile {
    pub(crate) bytes: Vec<u8>,
    #[cfg(target_os = "linux")]
    pub(crate) source: Option<std::fs::File>,
}

impl SeedFile {
    pub(crate) fn materialize(&self, destination: &Path) -> bool {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .unwrap();
        #[cfg(target_os = "linux")]
        if let Some(source) = &self.source {
            use std::os::fd::AsRawFd;
            // SAFETY: both descriptors remain open for the ioctl. FICLONE
            // takes a source descriptor, not a pointer, and creates shared
            // extents in this new destination inode without changing source.
            if unsafe { libc::ioctl(file.as_raw_fd(), libc::FICLONE, source.as_raw_fd()) } == 0 {
                return true;
            }
        }
        // Unsupported filesystems, cross-device copies, and other platforms
        // still get independent mutable files with exactly the captured bytes.
        file.set_len(0).unwrap();
        file.write_all(&self.bytes).unwrap();
        false
    }
}

impl SeedGitTemplate {
    fn capture(source: &Path) -> Self {
        fn visit(root: &Path, current: &Path, entries: &mut Vec<GitTemplateEntry>) {
            for entry in std::fs::read_dir(current).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let file_type = entry.file_type().unwrap_or_else(|error| {
                    panic!("could not inspect seed entry {}: {error}", path.display())
                });
                let permissions = entry
                    .metadata()
                    .unwrap_or_else(|error| {
                        panic!("could not read seed metadata {}: {error}", path.display())
                    })
                    .permissions();
                let relative = path.strip_prefix(root).unwrap().to_owned();
                if file_type.is_dir() {
                    entries.push(GitTemplateEntry {
                        path: relative,
                        permissions,
                        contents: None,
                    });
                    visit(root, &path, entries);
                } else {
                    assert!(file_type.is_file(), "unexpected non-file in Git seed");
                    entries.push(GitTemplateEntry {
                        path: relative,
                        permissions,
                        contents: Some(SeedFile {
                            bytes: std::fs::read(&path).unwrap(),
                            #[cfg(target_os = "linux")]
                            source: Some(std::fs::File::open(path).unwrap()),
                        }),
                    });
                }
            }
        }

        let mut entries = Vec::new();
        visit(source, source, &mut entries);
        Self {
            permissions: std::fs::metadata(source).unwrap().permissions(),
            entries,
        }
    }

    pub(crate) fn materialize(&self, destination: &Path) -> usize {
        let mut cloned = 0;
        std::fs::create_dir_all(destination).unwrap();
        for entry in &self.entries {
            let path = destination.join(&entry.path);
            if let Some(contents) = &entry.contents {
                // Existing scenario files are intentional modifications to the
                // committed seed; preserve them instead of hiding their delta.
                if !path.exists() {
                    cloned += usize::from(contents.materialize(&path));
                    std::fs::set_permissions(path, entry.permissions.clone()).unwrap();
                }
            } else {
                std::fs::create_dir_all(path).unwrap();
            }
        }
        // Apply directory permissions after writing descendants so even
        // restrictive template modes cannot block materialization.
        for entry in self
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.contents.is_none())
        {
            std::fs::set_permissions(destination.join(&entry.path), entry.permissions.clone())
                .unwrap();
        }
        std::fs::set_permissions(destination, self.permissions.clone()).unwrap();
        cloned
    }

    pub(crate) fn get() -> &'static Self {
        static SEED: std::sync::OnceLock<SeedGitTemplate> = std::sync::OnceLock::new();
        SEED.get_or_init(|| {
            // All mutable Git files are copied, never hardlinked.
            let source = Workspace::new();
            seed_git_workspace(&source, &[]);
            Self::capture(source.root())
        })
    }
}

fn git_child_environment(root: &Path) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let temp_dir = std::env::temp_dir();
    let mut environment = vec![
        (
            "PATH".into(),
            std::env::var_os("PATH").expect("test Git children require PATH"),
        ),
        ("HOME".into(), root.join(".git-test-home").into_os_string()),
        (
            "XDG_CONFIG_HOME".into(),
            root.join(".git-test-xdg-config").into_os_string(),
        ),
        (
            "GIT_CONFIG_GLOBAL".into(),
            root.join(".git-test-global-config").into_os_string(),
        ),
        (
            "GIT_CONFIG_SYSTEM".into(),
            root.join(".git-test-system-config").into_os_string(),
        ),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ];
    #[cfg(unix)]
    environment.push(("TMPDIR".into(), temp_dir.into_os_string()));
    #[cfg(windows)]
    {
        environment.push(("TEMP".into(), temp_dir.clone().into_os_string()));
        environment.push(("TMP".into(), temp_dir.into_os_string()));
        for key in ["SystemRoot", "ComSpec", "PATHEXT"] {
            if let Some(value) = std::env::var_os(key) {
                environment.push((key.into(), value));
            }
        }
    }
    environment
}

fn isolated_git_command(
    mut command: std::process::Command,
    root: &Path,
    args: &[&str],
) -> std::process::Command {
    command
        .env_clear()
        .envs(git_child_environment(root))
        .args(args)
        .current_dir(root);
    command
}

pub(crate) fn git_command(root: &Path, args: &[&str]) -> std::process::Command {
    isolated_git_command(std::process::Command::new("git"), root, args)
}

fn seed_git_command_with(
    command: std::process::Command,
    root: &Path,
    args: &[&str],
) -> std::process::Command {
    let mut command = isolated_git_command(command, root, &[]);
    // A detached maintenance process can mutate .git while it is captured.
    // Override both modern maintenance and legacy auto-gc only for this child.
    command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
        ])
        .args(args);
    command
}

fn seed_git_command_with_environment(
    root: &Path,
    args: &[&str],
    environment: &[(std::ffi::OsString, std::ffi::OsString)],
) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    for (key, value) in environment {
        command.env(key, value);
    }
    seed_git_command_with(command, root, args)
}

fn seed_git_with_environment(
    root: &Path,
    args: &[&str],
    environment: &[(std::ffi::OsString, std::ffi::OsString)],
) {
    let output = seed_git_command_with_environment(root, args, environment)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seed_git_workspace(
    workspace: &Workspace,
    environment: &[(std::ffi::OsString, std::ffi::OsString)],
) {
    seed_git_with_environment(
        workspace.root(),
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=main",
        ],
        environment,
    );
    seed_git_with_environment(
        workspace.root(),
        &["config", "--local", "commit.gpgsign", "false"],
        environment,
    );
    seed_git_with_environment(
        workspace.root(),
        &["config", "--local", "user.name", "ACDD test"],
        environment,
    );
    seed_git_with_environment(
        workspace.root(),
        &[
            "config",
            "--local",
            "user.email",
            "acdd-test@example.invalid",
        ],
        environment,
    );
    workspace.write("tests/runner_fixture.rs", RUNNER_FIXTURE);
    workspace.write("tests/acdd/tasks.rs", TASK_SEAM_FIXTURE);
    workspace.write("src/lib.rs", "pub fn example() {}\n");
    workspace.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    workspace.write("docs/010-test.md", SPEC);
    workspace.write(".git/info/exclude", FIXTURE_EXCLUDE);
    seed_git_with_environment(workspace.root(), &["add", "--all"], environment);
    seed_git_with_environment(
        workspace.root(),
        &["commit", "-m", "initial fixture baseline"],
        environment,
    );
}

/// Copies the committed fixture tree without forking Git per workspace.
pub(crate) fn init_git(root: &Path) {
    assert!(
        !root.join(".git").exists(),
        "fixture repository already exists"
    );
    SeedGitTemplate::get().materialize(root);
}

pub(crate) fn git_head(root: &Path) -> String {
    let output = git_command(root, &["rev-parse", "--verify", "HEAD"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not read ACDD fixture HEAD: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn git(root: &Path, args: &[&str]) {
    let output = git_command(root, args).output().unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn commit_handoff_fixture_inputs(root: &Workspace, paths: &[&str]) {
    let added = git_command(root.root(), &["add", "--force", "--"])
        .args(paths)
        .output()
        .unwrap();
    assert!(added.status.success(), "{added:?}");
    let committed = git_command(
        root.root(),
        &["commit", "-m", "admit handoff fixture inputs"],
    )
    .output()
    .unwrap();
    assert!(committed.status.success(), "{committed:?}");
}

pub(crate) fn commit_fixture_baseline(root: &Workspace) {
    let added = git_command(root.root(), &["add", "--all"])
        .output()
        .unwrap();
    assert!(added.status.success());
    let committed = git_command(
        root.root(),
        &["commit", "--quiet", "-m", "fixture source baseline"],
    )
    .output()
    .unwrap();
    assert!(
        committed.status.success(),
        "could not commit fixture baseline: {}",
        String::from_utf8_lossy(&committed.stderr)
    );
}

pub(crate) fn pinned_candidate(root: &std::path::Path, task_id: &str) -> String {
    let reference = format!("refs/forge/snapshots/{}", task_id.replace(':', "/"));
    let output = git_command(root, &["rev-parse", &reference])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[cfg(test)]
mod workspace_fixture_tests {
    use super::*;

    fn checked_git(root: &Workspace, args: &[&str]) -> String {
        let mut command = git_command(root.root(), &["-c", "core.hooksPath=/dev/null"]);
        command.args(args);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }

    #[test]
    fn seed_git_template_isolates_materialized_workspaces_and_preserves_dirty_entries() {
        let first = Workspace::new();
        first.write("src/lib.rs", "pre-existing dirty content\n");
        first.write("untracked.txt", "untracked pre-seed\n");
        init_git(first.root());

        let second = Workspace::new();
        init_git(second.root());

        let baseline = checked_git(&second, &["rev-parse", "HEAD"]);
        assert_eq!(checked_git(&first, &["rev-parse", "HEAD"]), baseline);

        // Pre-existing dirty file preserved in first; clean in second
        assert_eq!(
            std::fs::read_to_string(first.path("src/lib.rs")).unwrap(),
            "pre-existing dirty content\n"
        );
        assert_eq!(
            std::fs::read_to_string(second.path("src/lib.rs")).unwrap(),
            "pub fn example() {}\n"
        );
        assert!(first.path("untracked.txt").exists());
        assert!(!second.path("untracked.txt").exists());

        // Mutating first does not affect second
        first.write("src/lib.rs", "mutated in first only\n");
        checked_git(&first, &["add", "src/lib.rs"]);
        checked_git(&first, &["commit", "-m", "first only mutation"]);
        assert_ne!(checked_git(&first, &["rev-parse", "HEAD"]), baseline);
        assert_eq!(checked_git(&second, &["rev-parse", "HEAD"]), baseline);
        assert!(checked_git(&second, &["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn materialize_strategy_falls_back_cleanly_when_cow_unavailable() {
        let fallback = SeedFile {
            bytes: b"explicit byte-copy fallback\n".to_vec(),
            #[cfg(target_os = "linux")]
            source: None,
        };
        let fallback_root = Workspace::new();
        let fallback_path = fallback_root.path("fallback");
        assert!(!fallback.materialize(&fallback_path));
        assert_eq!(std::fs::read(fallback_path).unwrap(), fallback.bytes);
    }
}
