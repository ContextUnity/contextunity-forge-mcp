use super::*;

#[derive(Default)]
pub(in super::super) struct VirtualWorkspace {
    pub(in super::super) files: BTreeMap<String, String>,
}

pub(in super::super) struct FrameworkIndex {
    pub(in super::super) registry: DependencyRegistry,
    pub(in super::super) conn: Connection,
}

impl VirtualWorkspace {
    pub(in super::super) fn from_files(files: &[(&str, &str)]) -> Self {
        Self::from_map(
            files
                .iter()
                .map(|&(path, source)| (path.to_owned(), source.to_owned()))
                .collect(),
        )
    }

    pub(in super::super) fn from_map(files: BTreeMap<String, String>) -> Self {
        Self { files }
    }

    pub(in super::super) fn write(&mut self, path: impl Into<String>, source: &str) {
        self.files.insert(path.into(), source.to_owned());
    }

    pub(in super::super) fn index(&self) -> anyhow::Result<FrameworkIndex> {
        let mut manifests: Vec<_> = self
            .files
            .iter()
            .filter(|(path, _)| {
                path.starts_with(".forge/frameworks/")
                    || languages::dependency_registry::is_dependency_manifest_filename(
                        path.rsplit('/').next().unwrap(),
                    )
            })
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        manifests.sort_by_key(|(path, _)| path.starts_with(".forge/frameworks/"));
        let harness = FrameworkHarness::from_manifests(&manifests)?;
        let conn = self.index_with_registry(&harness.registry)?;
        Ok(FrameworkIndex {
            registry: harness.registry,
            conn,
        })
    }

    pub(in super::super) fn index_with_registry(
        &self,
        registry: &DependencyRegistry,
    ) -> anyhow::Result<Connection> {
        use contextunity_forge_mcp::forge_core::{config::Adapter, models::FileEntry};
        let mut entries = Vec::new();
        let mut facts = BTreeMap::new();
        for (path, source) in &self.files {
            if path.starts_with(".forge/") {
                continue;
            }
            let Some(profile) = languages::for_path(std::path::Path::new(path)) else {
                continue;
            };
            entries.push(FileEntry {
                path: path.clone(),
                digest: commitments::hash(source.as_bytes()),
                bytes: source.len() as u64,
                mtime_ns: 0,
                identity: [0; 3],
                language: profile.id().to_owned(),
                is_doc: false,
            });
            facts.insert(
                path.clone(),
                ast::extract_typed_with_registry(path, profile.id(), source, registry)?,
            );
        }
        ast::route_includes::compose_route_includes(&mut facts);
        contextunity_forge_mcp::engine::pipeline::components(&mut facts);
        let graph = linker::link_compact_with_typed_registry(&facts, None, None, registry);
        let mut conn = Connection::open_in_memory()?;
        let root = std::path::Path::new("/__forge_virtual_workspace__");
        let adapter = Adapter {
            roots: vec![root.into()],
            ..Default::default()
        };
        writer::populate_owned(
            &mut conn,
            root,
            &adapter,
            &entries,
            facts,
            &graph,
            writer::BuildInputs {
                manifest_digest: registry.digest(),
                memory_budget: 64 * 1024 * 1024,
                schema_ready: false,
            },
            &mut BTreeMap::new(),
            None,
        )?;
        Ok(conn)
    }
}
