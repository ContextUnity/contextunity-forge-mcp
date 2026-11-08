use super::*;

impl FrameworkHarness {
    pub(in super::super) fn discover_with_custom_adapter(
        workspace_files: &[(&str, &str)],
        enabled_files: &[(&str, &str)],
        disabled_files: &[(&str, &str)],
    ) -> DependencyRegistry {
        let workspace = Self::persisted(workspace_files);
        let enabled = Self::persisted(enabled_files);
        let disabled = Self::persisted(disabled_files);
        workspace.write(
            "custom-adapter.yaml",
            &format!(
                "roots: [.]\nexcluded_directory_names: [excluded]\nlinked_workspaces:\n  - name: enabled\n    path: '{}'\n    roots: [.]\n  - name: disabled\n    path: '{}'\n    enabled: false\n    roots: [.]\n",
                enabled.root().display(),
                disabled.root().display()
            ),
        );
        let adapter = contextunity_forge_mcp::engine::scanner::load_adapter(
            workspace.root(),
            Some(&workspace.path("custom-adapter.yaml")),
        )
        .unwrap();
        DependencyRegistry::collect_with_scan_config(workspace.root(), Some(&adapter))
    }
}
