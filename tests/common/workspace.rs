use contextunity_forge_mcp::db::{reader, writer};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Owns one isolated source workspace and its default Forge index.
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Creates a unique temporary directory that is removed when this value drops.
    pub fn new() -> Self {
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
        fs::write(&target, content)
            .unwrap_or_else(|error| panic!("could not write test fixture {}: {error}", target.display()));
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
        fs::write(&target, content)
            .unwrap_or_else(|error| panic!("could not write test fixture {}: {error}", target.display()));
    }

    /// Returns the default code map database path.
    pub fn db(&self) -> PathBuf {
        self.root.join(".forge/code-map.sqlite")
    }

    /// Builds the default index using the production writer.
    pub fn build(&self) -> Value {
        self.build_to(self.db())
    }

    /// Builds an index at a caller-selected path, for cold-build comparisons.
    pub fn build_to(&self, database: impl AsRef<Path>) -> Value {
        writer::build(self.root(), database.as_ref(), None)
            .unwrap_or_else(|error| panic!("could not build test workspace index: {error:#}"))
    }

    /// Builds an index with the supplied adapter path.
    pub fn build_with_adapter(
        &self,
        database: impl AsRef<Path>,
        adapter: Option<&Path>,
    ) -> Value {
        writer::build(self.root(), database.as_ref(), adapter)
            .unwrap_or_else(|error| panic!("could not build test workspace index: {error:#}"))
    }

    /// Applies a production delta to the default index for the changed paths.
    pub fn delta<P: AsRef<Path>>(&self, modified: &[P]) -> Value {
        let modified = modified
            .iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect::<Vec<_>>();
        writer::delta(self.root(), &self.db(), &modified)
            .unwrap_or_else(|error| panic!("could not update test workspace index: {error:#}"))
    }

    /// Opens the default index through the production reader.
    pub fn open(&self) -> Connection {
        reader::open(&self.db(), self.root())
            .unwrap_or_else(|error| panic!("could not open test workspace index: {error:#}"))
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
