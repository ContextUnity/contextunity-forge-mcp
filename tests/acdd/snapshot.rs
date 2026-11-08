pub(crate) use crate::acdd::support::{
    snapshot_git_bytes as git_bytes, snapshot_git_command, NamedSnapshotHarness,
};
use crate::common::Workspace;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
};

pub(crate) fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = snapshot_git_command(root);
    command.args(args);
    command
}

const RUNNER_COMPILE_ARGS: [&str; 4] = [
    "--test",
    "tests/runner_fixture.rs",
    "-o",
    ".forge/runner_fixture",
];

#[derive(PartialEq, Eq)]
struct RunnerCompileKey {
    source: Vec<u8>,
    compiler: Vec<u8>,
    environment: Vec<(OsString, OsString)>,
    args: [&'static str; 4],
}

#[derive(Clone)]
struct RunnerArtifact {
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

static PRECOMPILED_RUNNER: OnceLock<Mutex<Option<(RunnerCompileKey, RunnerArtifact)>>> =
    OnceLock::new();

struct FixtureFile {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

/// A physical snapshot that is restored only in the workspace that created it.
pub(crate) struct FixtureSnapshot {
    root: PathBuf,
    #[cfg(unix)]
    root_identity: (u64, u64),
    baseline_commit: String,
    original_branch: String,
    branches: BTreeSet<String>,
    forge_refs: BTreeMap<String, String>,
    sqlite_bytes: Vec<u8>,
    fixture_files: Vec<FixtureFile>,
    preserve_runner: bool,
}

impl FixtureSnapshot {
    /// The caller must close every SQLite connection before capture or restore.
    /// A successful WAL checkpoint does not prove that idle readers are closed.
    pub(crate) fn capture(root: &Workspace) -> Self {
        let path = assert_fixture_root(root);
        let baseline_commit = git(&path, &["rev-parse", "HEAD"]);
        let original_branch = git(&path, &["branch", "--show-current"]);
        assert!(
            !original_branch.is_empty(),
            "fixture must start on a branch"
        );
        let (branches, forge_refs) = repository_refs(&path);
        validate_database_files(&path, true);
        let connection = rusqlite::Connection::open(path.join(".forge/tasks.sqlite")).unwrap();
        let (busy, _log, _checkpointed): (i64, i64, i64) = connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(
            busy, 0,
            "SQLite WAL checkpoint is busy; close fixture connections before snapshot/restore"
        );
        let sqlite_bytes = fs::read(path.join(".forge/tasks.sqlite")).unwrap();

        // clean -x removes all ignored artifacts, including profile.yaml, command
        // traces and the runner. Preserve the baseline, not files from later rows.
        let mut paths = BTreeSet::new();
        for args in [
            &["ls-files", "--others", "--exclude-standard", "-z"][..],
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ][..],
        ] {
            for entry in git_bytes(&path, args).split(|byte| *byte == 0) {
                if entry.is_empty() {
                    continue;
                }
                let relative = PathBuf::from(std::str::from_utf8(entry).unwrap());
                assert!(relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))));
                if !matches!(
                    relative.to_str(),
                    Some(
                        ".forge/tasks.sqlite"
                            | ".forge/tasks.sqlite-wal"
                            | ".forge/tasks.sqlite-shm"
                            | ".forge/tasks.sqlite-journal"
                    )
                ) {
                    paths.insert(relative);
                }
            }
        }
        let fixture_files: Vec<FixtureFile> = paths
            .into_iter()
            .map(|relative| {
                assert_safe_path(&path, &relative, true);
                let full = path.join(&relative);
                let metadata = fs::symlink_metadata(&full).unwrap();
                assert!(
                    metadata.is_file(),
                    "unexpected non-file fixture artifact: {}",
                    full.display()
                );
                FixtureFile {
                    path: relative,
                    bytes: fs::read(full).unwrap(),
                    permissions: metadata.permissions(),
                }
            })
            .collect();
        let preserve_runner = fixture_files
            .iter()
            .any(|file| file.path == Path::new(".forge/runner_fixture"));
        Self {
            #[cfg(unix)]
            root_identity: root_identity(&path),
            root: path,
            baseline_commit,
            original_branch,
            branches,
            forge_refs,
            sqlite_bytes,
            fixture_files,
            preserve_runner,
        }
    }

    /// Commit captured from the fixture before any later scenario mutations.
    pub(crate) fn baseline_commit(&self) -> &str {
        &self.baseline_commit
    }

    pub(crate) fn restore(&self, root: &Workspace) {
        let path = assert_fixture_root(root);
        assert_eq!(
            path, self.root,
            "cannot restore a snapshot in another workspace"
        );
        #[cfg(unix)]
        assert_eq!(
            root_identity(&path),
            self.root_identity,
            "fixture directory was replaced"
        );
        // The current state is discarded rather than captured. All callers
        // close their connections before this point, so no WAL checkpoint is
        // needed, but the paths must still be safe before clean or replacement.
        // A scenario may legitimately remove the task store (for example,
        // terminal handoff pruning). Validate existing components without
        // requiring the captured database to still exist before restoration.
        validate_database_files(&path, false);
        // Handoff may leave the current branch dirty; discard its changes only
        // after verifying the fixture root, then reset the original branch.
        if self.preserve_runner {
            assert_safe_path(&path, Path::new(".forge/runner_fixture"), false);
            git(
                &path,
                &[
                    "clean",
                    "-fdx",
                    "-e",
                    ".forge/tasks.sqlite",
                    "-e",
                    ".forge/runner_fixture",
                ],
            );
        } else {
            git(&path, &["clean", "-fdx", "-e", ".forge/tasks.sqlite"]);
        }
        git(
            &path,
            &["switch", "--discard-changes", &self.original_branch],
        );
        git(&path, &["reset", "--hard", &self.baseline_commit]);
        let (current_branches, current) = repository_refs(&path);
        let temporary_branches: Vec<_> = current_branches
            .difference(&self.branches)
            .map(String::as_str)
            .collect();
        if !temporary_branches.is_empty() {
            let mut args = vec!["branch", "-D", "--"];
            args.extend(temporary_branches);
            git(&path, &args);
        }

        let mut updates = String::new();
        for name in current
            .keys()
            .filter(|name| !self.forge_refs.contains_key(*name))
        {
            updates.push_str(&format!("delete {name}\n"));
        }
        for (name, object) in &self.forge_refs {
            if current.get(name) != Some(object) {
                updates.push_str(&format!("update {name} {object}\n"));
            }
        }
        if !updates.is_empty() {
            let mut command = git_command(&path, &[])
                .args(["update-ref", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            command
                .stdin
                .take()
                .unwrap()
                .write_all(updates.as_bytes())
                .unwrap();
            let result = command.wait_with_output().unwrap();
            assert!(
                result.status.success(),
                "git update-ref --stdin: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }

        let db = path.join(".forge/tasks.sqlite");
        assert_safe_path(&path, Path::new(".forge/tasks.sqlite"), false);
        for suffix in ["-wal", "-shm", "-journal"] {
            let relative = PathBuf::from(format!(".forge/tasks.sqlite{suffix}"));
            assert_safe_path(&path, &relative, false);
            let auxiliary = path.join(relative);
            if auxiliary.exists() {
                fs::remove_file(auxiliary).unwrap();
            }
        }
        if db.exists() {
            fs::remove_file(&db).unwrap();
        }
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        assert_safe_path(&path, Path::new(".forge/tasks.sqlite"), false);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(db)
            .unwrap()
            .write_all(&self.sqlite_bytes)
            .unwrap();
        for file in &self.fixture_files {
            assert_safe_path(&path, &file.path, false);
            let dest = path.join(&file.path);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            assert_safe_path(&path, &file.path, false);
            if dest.exists() {
                if file.path == Path::new(".forge/runner_fixture")
                    && fs::read(&dest).unwrap() == file.bytes
                    && fs::metadata(&dest).unwrap().permissions() == file.permissions
                {
                    continue;
                }
                fs::remove_file(&dest).unwrap();
            }
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)
                .unwrap()
                .write_all(&file.bytes)
                .unwrap();
            fs::set_permissions(dest, file.permissions.clone()).unwrap();
        }
    }
}

fn assert_fixture_root(root: &Workspace) -> PathBuf {
    assert!(
        fs::symlink_metadata(root.root()).unwrap().is_dir(),
        "fixture root is not an owned directory"
    );
    let path = root.root().canonicalize().unwrap();
    assert_eq!(
        path.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(&format!("contextunity-test-{}-", std::process::id())));
    assert!(fs::symlink_metadata(path.join(".git")).unwrap().is_dir());
    let identities = git(
        &path,
        &[
            "rev-parse",
            "--show-toplevel",
            "--absolute-git-dir",
            "--git-common-dir",
        ],
    );
    let mut identities = identities.lines();
    assert_eq!(
        PathBuf::from(identities.next().unwrap())
            .canonicalize()
            .unwrap(),
        path
    );
    let expected_git_dir = path.join(".git").canonicalize().unwrap();
    for _ in 0..2 {
        let dir = PathBuf::from(identities.next().unwrap());
        assert_eq!(
            path.join(dir).canonicalize().unwrap(),
            expected_git_dir,
            "Git directory escaped fixture root"
        );
    }
    assert!(
        identities.next().is_none(),
        "unexpected Git identity output"
    );
    path
}

#[cfg(unix)]
fn root_identity(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

fn validate_database_files(root: &Path, database_must_exist: bool) {
    assert_safe_path(root, Path::new(".forge/tasks.sqlite"), database_must_exist);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert_safe_path(
            root,
            Path::new(&format!(".forge/tasks.sqlite{suffix}")),
            false,
        );
    }
}

// Read branches and proof refs together using full names. In particular, a
// tag with the same short name as a branch must not change branch cleanup.
fn repository_refs(root: &Path) -> (BTreeSet<String>, BTreeMap<String, String>) {
    let output = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads/",
            "refs/forge/",
            "refs/notes/forge",
        ],
    );
    let mut branches = BTreeSet::new();
    let mut proof_refs = BTreeMap::new();
    for line in output.lines() {
        let (name, object) = line.split_once(' ').expect("malformed Git ref output");
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            branches.insert(branch.to_owned());
        } else if name.starts_with("refs/forge/") || name == "refs/notes/forge" {
            proof_refs.insert(name.to_owned(), object.to_owned());
        }
    }
    (branches, proof_refs)
}

fn git(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git_bytes(root, args))
        .unwrap()
        .trim()
        .to_owned()
}

/// Production fixture setup also spawns Git; reject redirecting environment
/// before it can reach those subprocesses. The snapshot's own Git calls scrub it.
pub(crate) fn assert_safe_git_environment() {
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if name.starts_with("GIT_") {
            assert!(
                matches!(
                    name.as_ref(),
                    "GIT_PAGER"
                        | "GIT_TERMINAL_PROMPT"
                        | "GIT_CONFIG_NOSYSTEM"
                        | "GIT_AUTHOR_NAME"
                        | "GIT_AUTHOR_EMAIL"
                        | "GIT_AUTHOR_DATE"
                        | "GIT_COMMITTER_NAME"
                        | "GIT_COMMITTER_EMAIL"
                        | "GIT_COMMITTER_DATE"
                        | "GIT_LFS_SKIP_SMUDGE"
                        | "GIT_EDITOR"
                ),
                "unsafe Git environment variable {name} in fixture test"
            );
        }
    }
}

/// Compile the genuine seam runner once for matrix fixtures. Every gate still
/// executes that same runner and collects its real red/green result.
pub(crate) fn precompile_runner(root: &Workspace) {
    assert_safe_git_environment();
    let path = assert_fixture_root(root);
    assert_safe_path(&path, Path::new("tests/runner_fixture.rs"), true);
    assert_safe_path(&path, Path::new(".forge/runner_fixture"), false);
    assert_safe_path(&path, Path::new(".forge/acdd/profile.yaml"), true);
    let source = fs::read(path.join("tests/runner_fixture.rs")).unwrap();
    let mut environment: Vec<_> = std::env::vars_os().collect();
    environment.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let compiler = runner_rustc_command(&path, &environment)
        .args(["--version", "--verbose"])
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "identify fixture runner compiler: {}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    let key = RunnerCompileKey {
        source: source.clone(),
        compiler: compiler.stdout,
        environment: environment.clone(),
        args: RUNNER_COMPILE_ARGS,
    };

    // The runner fixture is standalone: it has no workspace-local includes or
    // modules, and its relative source path is fixed by the compile arguments.
    // Keep a process-wide binary only while those compile inputs remain equal
    // and rustc did not embed this workspace's absolute path.
    let mut cache = PRECOMPILED_RUNNER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cached = cache
        .as_ref()
        .filter(|(cached_key, artifact)| {
            cached_key == &key
                && runner_source_is_standalone(&source)
                && !contains_bytes(&artifact.bytes, path.to_string_lossy().as_bytes())
        })
        .map(|(_, artifact)| artifact.clone());
    let artifact = match cached {
        Some(artifact) => artifact,
        None => {
            let result = runner_rustc_command(&path, &environment)
                .args(RUNNER_COMPILE_ARGS)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "precompile fixture runner: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let runner = path.join(".forge/runner_fixture");
            let artifact = RunnerArtifact {
                bytes: fs::read(&runner).unwrap(),
                permissions: fs::metadata(runner).unwrap().permissions(),
            };
            if runner_source_is_standalone(&source)
                && !contains_bytes(&artifact.bytes, path.to_string_lossy().as_bytes())
                && cache.is_none()
            {
                *cache = Some((key, artifact.clone()));
            }
            artifact
        }
    };
    drop(cache);

    let runner_path = path.join(".forge/runner_fixture");
    assert_safe_path(&path, Path::new(".forge/runner_fixture"), false);
    fs::write(&runner_path, &artifact.bytes).unwrap();
    fs::set_permissions(runner_path, artifact.permissions).unwrap();

    let profile_path = path.join(".forge/acdd/profile.yaml");
    let mut profile: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
    const COMPILE: &str = "rustc --test tests/runner_fixture.rs -o .forge/runner_fixture && ";
    const READY: &str = "test -x .forge/runner_fixture && ";
    for name in ["test", "rust-check-and-test", "check_full_repo"] {
        let Some(command) = profile["commands"][name]["command"].as_str() else {
            continue;
        };
        let updated = match command.matches(COMPILE).count() {
            0 if command.contains(".forge/runner_fixture") => command.to_owned(),
            1 => command.replacen(COMPILE, READY, 1),
            _ => panic!("unexpected fixture command: {name}: {command}"),
        };
        profile["commands"][name]["command"] = serde_yaml::Value::String(updated);
    }
    fs::write(profile_path, serde_yaml::to_string(&profile).unwrap()).unwrap();
}

fn runner_rustc_command(root: &Path, environment: &[(OsString, OsString)]) -> Command {
    let mut command = Command::new("rustc");
    command
        .current_dir(root)
        .env_clear()
        .envs(environment.iter().cloned());
    command
}

fn runner_source_is_standalone(source: &[u8]) -> bool {
    [
        &b"include!"[..],
        b"include_str!",
        b"include_bytes!",
        b"#[path",
        b"mod ",
        b"mod\t",
    ]
    .iter()
    .all(|marker| !contains_bytes(source, marker))
}

fn contains_bytes(source: &[u8], pattern: &[u8]) -> bool {
    !pattern.is_empty()
        && source
            .windows(pattern.len())
            .any(|window| window == pattern)
}

// Check every existing component without following symlinks, including the
// final file. A missing component is allowed only for restoration destinations.
fn assert_safe_path(root: &Path, relative: &Path, must_exist: bool) {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    assert!(!components.is_empty());
    let mut missing = false;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(part) = component else {
            panic!("invalid fixture path {}", relative.display())
        };
        current.push(part);
        if missing {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                assert!(
                    !metadata.file_type().is_symlink(),
                    "fixture path contains symlink: {}",
                    current.display()
                );
                if index + 1 == components.len() {
                    assert!(
                        metadata.is_file(),
                        "fixture destination is not a file: {}",
                        current.display()
                    );
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        assert_eq!(
                            metadata.nlink(),
                            1,
                            "fixture destination is hardlinked: {}",
                            current.display()
                        );
                    }
                } else {
                    assert!(
                        metadata.is_dir(),
                        "fixture ancestor is not a directory: {}",
                        current.display()
                    );
                }
                assert!(
                    current.canonicalize().unwrap().starts_with(root),
                    "fixture path escaped workspace"
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
            Err(error) => panic!("cannot inspect fixture path {}: {error}", current.display()),
        }
    }
    assert!(
        !must_exist || !missing,
        "fixture file missing: {}",
        current.display()
    );
}
