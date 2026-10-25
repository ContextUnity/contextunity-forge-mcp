use super::*;

#[cfg(feature = "lang-python")]
#[test]
fn nested_manifests_classify_borrowed_package_prefixes() {
    let workspace = Workspace::new();
    workspace.write("packages/tool/requirements-dev.txt", "novel_library>=1.0\n");
    workspace.write("node_modules/ignored/requirements.txt", "ignored_package\n");
    let registry = DependencyRegistry::collect(Some(&workspace.0));
    let profile = languages::require("python").unwrap();
    assert_eq!(
        registry.classification(profile, "novel_library.client"),
        Some("external dependency (manifest)")
    );
    assert_eq!(
        registry.classification(profile, "novel_library_other"),
        Some("external dependency")
    );
    assert_eq!(
        registry.classification(profile, "ignored_package"),
        Some("external dependency")
    );
    assert_eq!(
        registry.classification(profile, "pathlib.Path"),
        Some("standard library")
    );
}
#[cfg(feature = "lang-python")]
#[test]
fn custom_adapter_collects_enabled_linked_manifests_and_respects_ignored_names() {
    let workspace = Workspace::new();
    let enabled = Workspace::new();
    let disabled = Workspace::new();
    enabled.write(
        "nested/pyproject.toml",
        "[project]\ndependencies = ['linked-package>=1']\n",
    );
    disabled.write("requirements.txt", "disabled_package\n");
    workspace.write("excluded/requirements.txt", "ignored_package\n");
    workspace.write(
        "custom-adapter.yaml",
        &custom_adapter_config(&enabled.0, &disabled.0),
    );
    let adapter = contextunity_forge_mcp::engine::scanner::load_adapter(
        &workspace.0,
        Some(&workspace.0.join("custom-adapter.yaml")),
    )
    .unwrap();
    let registry = DependencyRegistry::collect_with_scan_config(&workspace.0, Some(&adapter));
    let profile = languages::require("python").unwrap();
    assert_eq!(
        registry.classification(profile, "linked_package.client"),
        Some("external dependency (manifest)")
    );
    assert_eq!(
        registry.classification(profile, "disabled_package"),
        Some("external dependency")
    );
    assert_eq!(
        registry.classification(profile, "ignored_package"),
        Some("external dependency")
    );
}
#[cfg(feature = "lang-typescript")]
#[test]
fn npm_lockfile_discovery_respects_workspace_and_ignored_directories() {
    let workspace = Workspace::new();
    let enabled = Workspace::new();
    let disabled = Workspace::new();
    workspace.write(
        "package-lock.json",
        r#"{"lockfileVersion":3,"packages":{"node_modules/axios":{"version":"1"}}}"#,
    );
    workspace.write("nested/yarn.lock", "axios@^2.0.0:\n  version \"2.0.0\"\n");
    workspace.write(
        "excluded/package-lock.json",
        r#"{"lockfileVersion":3,"packages":{"node_modules/ignored-only":{"version":"1"}}}"#,
    );
    workspace.write(
        "node_modules/vendor/package-lock.json",
        r#"{"lockfileVersion":3,"packages":{"node_modules/vendor-only":{"version":"1"}}}"#,
    );
    enabled.write(
        "pnpm-lock.yaml",
        "lockfileVersion: '9.0'\npackages:\n  '@vue/runtime-core@3.5.0':\n    resolution: {integrity: sha512-example}\n",
    );
    disabled.write("yarn.lock", "disabled-only@^1.0.0:\n  version \"1.0.0\"\n");
    workspace.write(
        "custom-adapter.yaml",
        &custom_adapter_config(&enabled.0, &disabled.0),
    );
    let adapter = contextunity_forge_mcp::engine::scanner::load_adapter(
        &workspace.0,
        Some(&workspace.0.join("custom-adapter.yaml")),
    )
    .unwrap();
    let registry = DependencyRegistry::collect_with_scan_config(&workspace.0, Some(&adapter));
    let profile = languages::require("typescript").unwrap();
    for module in ["axios", "@vue/runtime-core/jsx-runtime"] {
        assert_eq!(
            registry.classification(profile, module),
            Some("external dependency (manifest)"),
            "{module}"
        );
    }
    assert_eq!(
        registry.classification_for_path(profile, "axios", "consumer.ts"),
        Some("external dependency (manifest: package-lock.json)".to_owned())
    );
    assert_eq!(
        registry.classification_for_path(profile, "axios", "nested/consumer.ts"),
        Some("external dependency (manifest: nested/yarn.lock)".to_owned())
    );
    assert_eq!(
        registry.classification_for_path(
            profile,
            "@vue/runtime-core/jsx-runtime",
            "[enabled]/consumer.ts"
        ),
        Some("external dependency (manifest: [enabled]/pnpm-lock.yaml)".to_owned())
    );
    assert_eq!(
        registry.classification_for_path(profile, "axios", "[enabled]/consumer.ts"),
        Some("external dependency".to_owned())
    );
    for module in ["ignored-only", "vendor-only", "disabled-only"] {
        assert_eq!(
            registry.classification(profile, module),
            Some("external dependency"),
            "{module}"
        );
    }
}
#[cfg(feature = "lang-typescript")]
#[test]
fn npm_lockfile_imports_record_manifest_external_origin_in_build() {
    let workspace = Workspace::new();
    workspace.write(
        "package-lock.json",
        r#"{"lockfileVersion":3,"packages":{"node_modules/@vue/runtime-core":{"version":"3.5.0"},"node_modules/axios":{"version":"1.0.0"},"node_modules/local-link":{"resolved":"packages/local","link":true}}}"#,
    );
    workspace.write(
        "consumer.ts",
        "import { createVNode } from '@vue/runtime-core';\nimport axios from 'axios';\nimport local from 'local-link';\nexport const request = () => axios.get('/');\n",
    );
    let db = workspace.0.join(".forge/index.db");
    writer::build(&workspace.0, &db, None).unwrap();
    let conn = reader::open(&db, &workspace.0).unwrap();
    let mut statement = conn
        .prepare("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1")
        .unwrap();
    for symbol in ["createVNode", "default"] {
        let rows: Vec<(String, String)> = statement
            .query_map([symbol], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            rows.iter().any(|(status, evidence)| status == "external"
                && evidence.contains("external dependency (manifest: package-lock.json)")),
            "{symbol}: {rows:?}"
        );
    }
    let mut local_statement = conn
        .prepare("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.ts' AND line=3 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='default'")
        .unwrap();
    let local: Vec<(String, String)> = local_statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        local.iter().any(|(status, evidence)| status == "external"
            && evidence.starts_with("external dependency;")),
        "local: {local:?}"
    );
}
