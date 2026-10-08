use contextunity_forge_mcp::db::writer;
use serde_json::Value;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Owns an isolated temporary source workspace used by manual benchmark runners.
pub struct BenchmarkWorkspace {
    parent: PathBuf,
    root: PathBuf,
}

impl BenchmarkWorkspace {
    /// Creates a collision-safe temporary workspace and its main project root.
    pub fn new() -> Self {
        static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_nanos();
        let parent = loop {
            let sequence = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "contextunity-benchmark-{}-{timestamp}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create benchmark workspace: {error}"),
            }
        };
        let root = parent.join("main");
        fs::create_dir_all(&root).expect("could not create main benchmark project");
        Self { parent, root }
    }

    /// Returns the main source workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the temporary parent used for linked workspaces.
    pub fn parent(&self) -> &Path {
        &self.parent
    }

    /// Resolves a main-project or `[linked]/relative/path` benchmark input.
    pub fn file(&self, path: &str) -> PathBuf {
        if let Some((name, rest)) = path.strip_prefix('[').and_then(|p| p.split_once("]/")) {
            self.parent.join("linked").join(name).join(rest)
        } else {
            self.root.join(path)
        }
    }

    /// Resolves a safe relative path beneath the main project root.
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        let relative = relative.as_ref();
        assert!(
            !relative.is_absolute()
                && !relative.components().any(|part| matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )),
            "benchmark workspace paths must remain relative: {}",
            relative.display()
        );
        self.root.join(relative)
    }

    /// Writes benchmark fixture data and creates its parent directories.
    pub fn write(&self, relative: impl AsRef<Path>, content: &str) {
        let target = self.path(relative);
        fs::create_dir_all(target.parent().expect("fixture has a parent"))
            .expect("could not create benchmark fixture directory");
        fs::write(target, content).expect("could not write benchmark fixture");
    }

    /// Returns the default Forge database path.
    pub fn db(&self) -> PathBuf {
        self.root.join(".forge/code-map.sqlite")
    }

    /// Runs a cold build through the production writer.
    pub fn build(&self) -> Value {
        self.build_to(self.db())
    }

    /// Runs a cold build to a caller-selected database path.
    pub fn build_to(&self, database: impl AsRef<Path>) -> Value {
        writer::build(&self.root, database.as_ref(), None)
            .expect("production writer failed to build benchmark workspace")
    }

    /// Runs a delta through the production writer for the changed relative paths.
    pub fn delta(&self, modified: &[impl AsRef<Path>]) -> Value {
        let modified = modified
            .iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect::<Vec<_>>();
        writer::delta(&self.root, &self.db(), &modified)
            .expect("production writer failed to update benchmark workspace")
    }
}

impl Drop for BenchmarkWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

impl Default for BenchmarkWorkspace {
    fn default() -> Self {
        Self::new()
    }
}
