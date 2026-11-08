use super::*;

#[cfg(feature = "lang-python")]
#[test]
fn nested_manifests_classify_borrowed_package_prefixes() {
    let workspace = FrameworkHarness::persisted(&[
        (
            "packages/tool/requirements-dev.txt",
            "novel_library>=1.0\\n",
        ),
        (
            "node_modules/ignored/requirements.txt",
            "ignored_package\\n",
        ),
    ]);
    let registry = DependencyRegistry::collect(Some(workspace.root()));
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
    let registry = FrameworkHarness::discover_with_custom_adapter(
        &[("excluded/requirements.txt", "ignored_package\n")],
        &[(
            "nested/pyproject.toml",
            "[project]\ndependencies = ['linked-package>=1']\n",
        )],
        &[("requirements.txt", "disabled_package\n")],
    );
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
    let registry = FrameworkHarness::discover_with_custom_adapter(
        &[
            (
                "package-lock.json",
                r#"{"lockfileVersion":3,"packages":{"node_modules/axios":{"version":"1"}}}"#,
            ),
            ("nested/yarn.lock", "axios@^2.0.0:\n  version \"2.0.0\"\n"),
            (
                "excluded/package-lock.json",
                r#"{"lockfileVersion":3,"packages":{"node_modules/ignored-only":{"version":"1"}}}"#,
            ),
            (
                "node_modules/vendor/package-lock.json",
                r#"{"lockfileVersion":3,"packages":{"node_modules/vendor-only":{"version":"1"}}}"#,
            ),
        ],
        &[(
            "pnpm-lock.yaml",
            "lockfileVersion: '9.0'\npackages:\n  '@vue/runtime-core@3.5.0':\n    resolution: {integrity: sha512-example}\n",
        )],
        &[("yarn.lock", "disabled-only@^1.0.0:\n  version \"1.0.0\"\n")],
    );
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
    let workspace = FrameworkHarness::persisted(&[
        (
            "package-lock.json",
            r#"{"lockfileVersion":3,"packages":{"node_modules/@vue/runtime-core":{"version":"3.5.0"},"node_modules/axios":{"version":"1.0.0"},"node_modules/local-link":{"resolved":"packages/local","link":true}}}"#,
        ),
        (
            "consumer.ts",
            "import { createVNode } from '@vue/runtime-core';\nimport axios from 'axios';\nimport local from 'local-link';\nexport const request = () => axios.get('/');\n",
        ),
    ]);
    workspace.build();
    let conn = workspace.open();
    let runtime = coverage_for_expression(&conn, "consumer.ts", "createVNode", None);
    let axios = coverage_for_expression(&conn, "consumer.ts", "default", None);
    let local = coverage_for_expression(&conn, "consumer.ts", "default", Some(3));
    for (symbol, rows) in [("createVNode", &runtime), ("default", &axios)] {
        assert!(
            rows.iter().any(|(status, evidence)| status == "external"
                && evidence.contains("external dependency (manifest: package-lock.json)")),
            "{symbol}: {rows:?}"
        );
    }
    assert!(
        local.iter().any(|(status, evidence)| status == "external"
            && evidence.starts_with("external dependency;")),
        "local: {local:?}"
    );
}
