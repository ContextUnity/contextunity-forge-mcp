use crate::common::Workspace;
use crate::sqlite_fixture::{IndexFixture, MetadataVersion};
use contextunity_forge_mcp::db::writer::cache;
use std::{
    collections::BTreeSet,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
pub enum RebuildCause {
    Schema,
    Adapter,
    ExplicitManifest,
    ManifestDigest,
}

impl RebuildCause {
    pub const ALL: [Self; 4] = [
        Self::Schema,
        Self::Adapter,
        Self::ExplicitManifest,
        Self::ManifestDigest,
    ];

    pub fn changed_path(self) -> &'static str {
        match self {
            Self::ExplicitManifest => "requirements.txt",
            _ => "src/main.rs",
        }
    }
}

pub struct CacheFixture(Workspace);

impl Deref for CacheFixture {
    type Target = Workspace;

    fn deref(&self) -> &Workspace {
        &self.0
    }
}

impl CacheFixture {
    pub fn new() -> Self {
        let workspace = Workspace::new();
        workspace.write("forge-mcp.yaml", "roots: [src]\n");
        workspace.write("requirements.txt", "requests>=2.0\n");
        workspace.write(
            "src/main.rs",
            &format!(
                "pub fn isolated() {{ let identity = {:?}; }}\n",
                workspace.root()
            ),
        );
        Self(workspace)
    }

    pub fn directory(&self) -> PathBuf {
        self.path(".forge/extraction-cache")
    }

    pub fn clear_cache(&self) {
        fs::remove_dir_all(self.directory()).unwrap();
    }

    pub fn prepare_fallback(&self, cause: RebuildCause) {
        self.build();
        self.clear_cache();
        match cause {
            RebuildCause::Schema => {
                let index = IndexFixture::open(&self.db());
                index.outdated(MetadataVersion::Schema);
                index.seal();
            }
            RebuildCause::Adapter => {
                self.write("forge-mcp.yaml", "roots: [src]\nadapter_version: changed\n");
            }
            RebuildCause::ExplicitManifest | RebuildCause::ManifestDigest => {
                self.write("requirements.txt", "requests>=2.1\n");
            }
        }
    }

    pub fn copy_source_from(&self, other: &Self) {
        self.write(
            "src/main.rs",
            &fs::read_to_string(other.path("src/main.rs")).unwrap(),
        );
    }

    pub fn isolated_symbol_count(&self) -> rusqlite::Result<i64> {
        self.open().query_row(
            "SELECT count(*) FROM nodes WHERE name='isolated'",
            [],
            |row| row.get(0),
        )
    }
}

pub fn entries(root: &Path) -> BTreeSet<PathBuf> {
    fn visit(root: &Path, path: &Path, found: &mut BTreeSet<PathBuf>) {
        let Ok(children) = fs::read_dir(path) else {
            return;
        };
        for child in children.map(Result::unwrap) {
            let path = child.path();
            if path.is_dir() {
                visit(root, &path, found);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "facts")
            {
                found.insert(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    let mut found = BTreeSet::new();
    visit(root, root, &mut found);
    found
}

pub fn global_copies(paths: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    cache::default_cache_root()
        .into_iter()
        .flat_map(|root| paths.iter().map(move |path| root.join(path)))
        .filter(|path| path.exists())
        .collect()
}
