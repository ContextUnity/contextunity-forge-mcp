use super::*;
use languages::dependency_registry::{ManifestSource, WorkspaceManifestSources};

impl FrameworkHarness {
    pub(in super::super) fn registry_pair_in(
        workspace: &Workspace,
        linked: &Workspace,
        name: &str,
        manifests: &[(&str, &str)],
    ) -> (DependencyRegistry, DependencyRegistry) {
        workspace.write_all(manifests);
        linked.write("requirements.txt", "fastapi\n");
        workspace.write(
            "forge-mcp.yaml",
            &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: '{name}'\n    path: '{}'\n    roots: [.]\n",
            linked.root().display()
        ),
        );
        let main: Vec<_> = manifests
            .iter()
            .map(|&(path, source)| ManifestSource { path, source })
            .collect();
        let linked_sources = [ManifestSource {
            path: "requirements.txt",
            source: "fastapi\n",
        }];
        let source = DependencyRegistry::try_collect_from_sources([
            WorkspaceManifestSources {
                name: "",
                manifests: &main,
            },
            WorkspaceManifestSources {
                name,
                manifests: &linked_sources,
            },
        ])
        .unwrap();
        let adapter =
            contextunity_forge_mcp::engine::scanner::load_adapter(workspace.root(), None).unwrap();
        let disk =
            DependencyRegistry::try_collect_with_scan_config(workspace.root(), Some(&adapter))
                .unwrap();
        (source, disk)
    }

    pub(in super::super) fn effective_manifest(
        registry: &DependencyRegistry,
        path: &str,
        pack: &str,
    ) -> Value {
        serde_json::to_value(registry.framework_manifest_for_path(
            LanguageFamily("python"),
            path,
            pack,
            pack,
        ))
        .unwrap()
    }
}
