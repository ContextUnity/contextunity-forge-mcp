use contextunity_forge_mcp::db::reader;
use contextunity_forge_mcp::engine::pipeline;
use rusqlite::Connection;
use serde_json::Value;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Eq, PartialEq)]
// Used by selected language and framework-manifest test binaries only.
#[allow(dead_code)]
pub(crate) struct WorkspaceInputSnapshot {
    root: PathBuf,
    oracle_kind: String,
    entries: Vec<(PathBuf, WorkspaceInput)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
// Used by selected language and framework-manifest test binaries only.
#[allow(dead_code)]
enum WorkspaceInput {
    File(Vec<u8>),
    Symlink(PathBuf),
    Other,
}

/// Owns one isolated source workspace and its default Forge index.
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Creates a unique temporary directory that is removed when this value drops.
    pub fn new() -> Self {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            contextunity_forge_mcp::mcp::tasks::ensure_extractors();
        });
        static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_nanos();

        let root = loop {
            let sequence = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
            let candidate = env::temp_dir().join(format!(
                "contextunity-test-{}-{timestamp}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!(
                    "could not create test workspace {}: {error}",
                    candidate.display()
                ),
            }
        };

        Self { root }
    }

    /// Returns the root of this isolated source workspace.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a workspace-relative path and rejects paths that escape its root.
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        let relative = relative.as_ref();
        assert!(
            !relative.is_absolute()
                && !relative.components().any(|part| matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )),
            "test workspace paths must stay relative to the workspace root: {}",
            relative.display()
        );
        self.root.join(relative)
    }

    /// Writes UTF-8 source or fixture data, creating parent directories as needed.
    pub fn write(&self, relative: impl AsRef<Path>, content: &str) {
        let target = self.path(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!(
                    "could not create test fixture directory {}: {error}",
                    parent.display()
                )
            });
        }
        fs::write(&target, content).unwrap_or_else(|error| {
            panic!("could not write test fixture {}: {error}", target.display())
        });
    }

    /// Writes binary fixture data, creating parent directories as needed.
    pub fn write_bytes(&self, relative: impl AsRef<Path>, content: &[u8]) {
        let target = self.path(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!(
                    "could not create test fixture directory {}: {error}",
                    parent.display()
                )
            });
        }
        fs::write(&target, content).unwrap_or_else(|error| {
            panic!("could not write test fixture {}: {error}", target.display())
        });
    }

    /// Returns the default code map database path.
    pub fn db(&self) -> PathBuf {
        self.root.join(".forge/code-map.sqlite")
    }

    /// Returns the default code map database path.
    pub fn db_path(&self) -> PathBuf {
        self.db()
    }

    /// Builds the default index using the production writer.
    pub fn build(&self) -> Value {
        self.build_to(self.db())
    }

    /// Builds an index at a caller-selected path, for cold-build comparisons.
    pub fn build_to(&self, database: impl AsRef<Path>) -> Value {
        self.build_with_adapter(database, None)
    }

    /// Builds an index with the supplied adapter path.
    pub fn build_with_adapter(&self, database: impl AsRef<Path>, adapter: Option<&Path>) -> Value {
        pipeline::build_with_cache_dir(
            self.root(),
            database.as_ref(),
            adapter,
            &self.path(".forge/extraction-cache"),
        )
        .unwrap_or_else(|error| panic!("could not build test workspace index: {error:#}"))
    }

    /// Applies a production delta to the default index for the changed paths.
    pub fn delta<P: AsRef<Path>>(&self, modified: &[P]) -> Value {
        let modified = modified
            .iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect::<Vec<_>>();
        pipeline::delta_with_cache_dir(
            self.root(),
            &self.db(),
            &modified,
            &self.path(".forge/extraction-cache"),
        )
        .unwrap_or_else(|error| panic!("could not update test workspace index: {error:#}"))
    }

    /// Writes each relative path, creating parent directories as needed.
    pub fn write_all(&self, files: &[(&str, &str)]) {
        for &(path, content) in files {
            self.write(path, content);
        }
    }

    /// Creates a workspace and writes initial files into it.
    pub fn with_files(files: &[(&str, &str)]) -> Self {
        let workspace = Self::new();
        workspace.write_all(files);
        workspace
    }

    /// Creates a workspace with initial files and an empty .git directory.
    pub fn with_gitdir(files: &[(&str, &str)]) -> anyhow::Result<Self> {
        let workspace = Self::with_files(files);
        fs::create_dir_all(workspace.path(".git"))?;
        Ok(workspace)
    }

    /// Seeds a baseline Git repository with an initial commit.
    pub fn seed_git(&self) -> anyhow::Result<()> {
        for args in [
            vec!["init", "-q"],
            vec!["add", "-A"],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "baseline",
            ],
        ] {
            let output = contextunity_forge_mcp::core::git::git_command(self.root(), &args)?;
            anyhow::ensure!(
                output.status.success(),
                "Git fixture command {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Creates Git tags in the workspace.
    pub fn create_git_tags(&self, tags: &[&str]) -> anyhow::Result<()> {
        for tag in tags {
            let output =
                contextunity_forge_mcp::core::git::git_command(self.root(), &["tag", tag])?;
            anyhow::ensure!(
                output.status.success(),
                "Git tag {tag} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Stages relative paths in the workspace Git repository.
    pub fn stage_git_files(&self, paths: &[&str]) -> anyhow::Result<()> {
        let mut args = vec!["add", "--"];
        args.extend_from_slice(paths);
        self.run_git(&args)
    }

    /// Creates a Git commit in the workspace.
    pub fn commit_git(
        &self,
        message: &str,
        only_paths: Option<&[&str]>,
    ) -> anyhow::Result<()> {
        let mut args = vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            message,
        ];
        if let Some(paths) = only_paths {
            args.push("--only");
            args.push("--");
            args.extend_from_slice(paths);
        }
        self.run_git(&args)
    }

    /// Runs a Git command inside the workspace root.
    pub fn run_git(&self, args: &[&str]) -> anyhow::Result<()> {
        let output = contextunity_forge_mcp::core::git::git_command(self.root(), args)?;
        anyhow::ensure!(
            output.status.success(),
            "Git fixture command {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    /// Removes one file inside this workspace. The path must stay relative.
    pub fn try_remove(&self, relative: impl AsRef<Path>) -> std::io::Result<()> {
        fs::remove_file(self.path(relative))
    }

    /// Removes one file inside this workspace. The path must stay relative.
    pub fn remove(&self, relative: impl AsRef<Path>) {
        let relative = relative.as_ref();
        self.try_remove(relative).unwrap_or_else(|error| {
            panic!(
                "could not remove test fixture {}: {error}",
                self.path(relative).display()
            )
        });
    }

    /// Opens the default index through the production reader.
    pub fn open(&self) -> Connection {
        let conn = reader::open(&self.db(), self.root())
            .unwrap_or_else(|error| panic!("could not open test workspace index: {error:#}"));
        let _ = conn.execute_batch("COMMIT");
        conn
    }

    /// Captures every non-generated workspace input, scoped to this root and oracle.
    // Only some integration-test binaries use the cold-reference cache.
    #[allow(dead_code)]
    pub(crate) fn input_snapshot(&self, oracle_kind: &str) -> WorkspaceInputSnapshot {
        fn generated_input(relative: &Path, file_type: fs::FileType) -> bool {
            let mut components = relative.components();
            let Some(Component::Normal(first)) = components.next() else {
                return false;
            };
            if first == ".git" {
                if relative == Path::new(".git") || relative == Path::new(".git/info") {
                    return !file_type.is_dir();
                }
                let scanner_git_input = relative == Path::new(".git/config")
                    || relative == Path::new(".git/info/exclude");
                return !(scanner_git_input && (file_type.is_file() || file_type.is_symlink()));
            }
            if first != ".forge" {
                return false;
            }
            let Some(Component::Normal(second)) = components.next() else {
                return false;
            };
            if second == "frameworks" || second == "acdd" {
                return false;
            }
            if second == "extraction-cache" || second == "cold-cache" {
                return true;
            }
            (file_type.is_file() || file_type.is_symlink())
                && relative.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    [
                        ".sqlite",
                        ".sqlite-wal",
                        ".sqlite-shm",
                        ".sqlite.lock",
                        ".sqlite.verified.json",
                    ]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                })
        }

        fn visit(root: &Path, directory: &Path, entries: &mut Vec<(PathBuf, WorkspaceInput)>) {
            let mut children = fs::read_dir(directory)
                .unwrap_or_else(|error| {
                    panic!(
                        "could not snapshot test workspace {}: {error}",
                        directory.display()
                    )
                })
                .map(|entry| {
                    entry.unwrap_or_else(|error| panic!("could not read workspace entry: {error}"))
                })
                .collect::<Vec<_>>();
            children.sort_by_key(fs::DirEntry::path);
            for child in children {
                let path = child.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("workspace entry stays below its root")
                    .to_path_buf();
                let metadata = fs::symlink_metadata(&path).unwrap_or_else(|error| {
                    panic!(
                        "could not inspect workspace input {}: {error}",
                        path.display()
                    )
                });
                let file_type = metadata.file_type();
                if generated_input(&relative, file_type) {
                    continue;
                }
                if file_type.is_symlink() {
                    entries.push((
                        relative.clone(),
                        WorkspaceInput::Symlink(fs::read_link(&path).unwrap_or_else(|error| {
                            panic!(
                                "could not read workspace symlink {}: {error}",
                                path.display()
                            )
                        })),
                    ));
                    if fs::metadata(&path).is_ok_and(|target| target.is_file()) {
                        if let Ok(bytes) = fs::read(&path) {
                            entries.push((relative, WorkspaceInput::File(bytes)));
                        }
                    }
                } else if file_type.is_dir() {
                    visit(root, &path, entries);
                } else if file_type.is_file() {
                    entries.push((
                        relative,
                        WorkspaceInput::File(fs::read(&path).unwrap_or_else(|error| {
                            panic!("could not read workspace input {}: {error}", path.display())
                        })),
                    ));
                } else {
                    entries.push((relative, WorkspaceInput::Other));
                }
            }
        }

        let mut entries = Vec::new();
        visit(self.root(), self.root(), &mut entries);
        if let Ok(manifest) = fs::read_to_string(self.path("forge-mcp.yaml")) {
            if let Ok(config) = serde_yaml::from_str::<serde_yaml::Value>(&manifest) {
                if let Some(linked_workspaces) = config
                    .get("linked_workspaces")
                    .or_else(|| config.get("workspaces"))
                    .and_then(serde_yaml::Value::as_sequence)
                {
                    for linked_workspace in linked_workspaces {
                        let Some(name) = linked_workspace
                            .get("name")
                            .and_then(serde_yaml::Value::as_str)
                        else {
                            continue;
                        };
                        let Some(path) = linked_workspace
                            .get("path")
                            .and_then(serde_yaml::Value::as_str)
                        else {
                            continue;
                        };
                        let path = PathBuf::from(path);
                        let linked_root = if path.is_absolute() {
                            path
                        } else {
                            self.root().join(path)
                        };
                        if linked_root.is_dir() {
                            let mut linked_entries = Vec::new();
                            visit(&linked_root, &linked_root, &mut linked_entries);
                            let prefix = PathBuf::from(format!("[{name}]"));
                            entries.extend(linked_entries.into_iter().map(|(relative, input)| {
                                (prefix.join(relative), input)
                            }));
                        }
                    }
                }
            }
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        WorkspaceInputSnapshot {
            root: self.root.clone(),
            oracle_kind: oracle_kind.to_owned(),
            entries,
        }
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
