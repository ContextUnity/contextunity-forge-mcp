use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::{
        ast,
        languages::{self, dependency_registry::DependencyRegistry, manifests::FrameworkManifestValue, LanguageFamily},
        linker,
    },
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
};

use super::support::Workspace;


// ---------------------------------------------------------------------------
// Language Manifest Profile Tests
// ---------------------------------------------------------------------------

#[cfg(feature = "lang-typescript")]
#[test]
fn package_json_dependencies_include_runtime_development_and_peer_packages() {
    let profile = languages::require("typescript").unwrap();
    assert!(profile.manifest_filenames().contains(&"package.json"));
    let dependencies = profile.extract_manifest_dependencies(
        "package.json",
        r#"{"dependencies":{"react":"^18","@scope/pkg":"1","local":"workspace:*"},"devDependencies":{"vitest":"1"},"peerDependencies":{"typescript":"5"},"optionalDependencies":{"ignored":"1"}}"#,
    );
    assert_eq!(
        dependencies,
        ["react", "@scope/pkg", "vitest", "typescript"]
    );
    assert!(profile.is_stdlib("fs"));
    assert!(profile.is_stdlib("node:fs/promises"));
    assert!(profile.is_stdlib("node:custom-built-in"));
    assert!(!profile.is_stdlib("@scope/fs"));
    assert!(profile.builtin("process"));
    assert!(profile.builtin("Buffer"));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn npm_lockfiles_register_direct_transitive_and_scoped_dependencies() {
    use std::collections::BTreeSet;

    let profile = languages::require("typescript").unwrap();
    for filename in ["package-lock.json", "pnpm-lock.yaml", "yarn.lock"] {
        assert!(
            profile.manifest_filenames().contains(&filename),
            "{filename}"
        );
    }
    let cases: [(&str, &str, &[&str]); 3] = [
        (
            "package-lock.json",
            r#"{
  "lockfileVersion": 3,
  "packages": {
    "": {"name": "app", "dependencies": {"axios": "^1"}},
    "node_modules/axios": {"version": "1.0.0"},
    "node_modules/@vue/runtime-core": {"version": "3.5.0"},
    "node_modules/axios/node_modules/follow-redirects": {"version": "1.0.0"},
    "node_modules/local-link": {"resolved": "packages/local", "link": true}
  },
  "dependencies": {"legacy-only": {"version": "1.0.0"}}
}"#,
            &["axios", "@vue/runtime-core", "follow-redirects", "legacy-only"],
        ),
        (
            "pnpm-lock.yaml",
            "lockfileVersion: '9.0'\ndependencies:\n  legacy-only: 1.0.0\nimporters:\n  .:\n    dependencies:\n      '@types/node':\n        specifier: ^22.0.0\n        version: 22.0.0\n      workspace-local:\n        specifier: workspace:*\n        version: link:../local\n      linked-local:\n        specifier: ^1.0.0\n        version: link:../linked\npackages:\n  '@vue/runtime-core@3.5.0':\n    resolution: {integrity: sha512-example}\n  axios@1.0.0:\n    resolution: {integrity: sha512-example}\n  'workspace-local@link:../local':\n    resolution: {directory: ../local, type: directory}\n  'empty-selector@':\n",
            &["@types/node", "@vue/runtime-core", "axios", "legacy-only"],
        ),
        (
            "yarn.lock",
            "# yarn lockfile v1\n\"@vue/runtime-core@^3.5.0\":\n  version \"3.5.0\"\naxios@^1.0.0:\n  version \"1.0.0\"\n  dependencies:\n    follow-redirects \"^1.0.0\"\nfollow-redirects@^1.0.0:\n  version \"1.0.0\"\nlocal@workspace:*:\n  version \"0.0.0-use.local\"\nempty-selector@:\n",
            &["@vue/runtime-core", "axios", "follow-redirects"],
        ),
    ];
    let mut mismatches = Vec::new();
    for (filename, content, expected) in cases {
        let actual: BTreeSet<_> = profile
            .extract_manifest_dependencies(filename, content)
            .into_iter()
            .collect();
        let expected: BTreeSet<_> = expected.iter().map(|name| (*name).to_owned()).collect();
        if actual != expected {
            mismatches.push(format!(
                "{filename}: actual={actual:?}, expected={expected:?}"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn package_lock_v1_nested_dependencies_register_transitive_packages() {
    use std::collections::BTreeSet;

    let profile = languages::require("javascript").unwrap();
    let content = r#"{
  "lockfileVersion": 1,
  "dependencies": {
    "legacy-parent": {
      "version": "1.0.0",
      "dependencies": {
        "legacy-child": {
          "version": "2.0.0",
          "dependencies": {"@types/nested": {"version": "3.0.0"}}
        }
      }
    }
  }
}"#;
    let actual: BTreeSet<_> = profile
        .extract_manifest_dependencies("package-lock.json", content)
        .into_iter()
        .collect();
    assert_eq!(
        actual,
        BTreeSet::from([
            "legacy-parent".to_owned(),
            "legacy-child".to_owned(),
            "@types/nested".to_owned(),
        ])
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_require_imports_bind_identifiers_and_destructured_exports() {
    let facts = ast::extract(
        "sample.cjs",
        "javascript",
        "const fs = require('fs');\nconst { readFile, writeFile: write } = require('node:fs/promises');\nfunction run(require) { const local = require('not-a-module'); }\n",
    )
    .unwrap();
    let imports: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == "imports")
        .collect();
    assert_eq!(imports.len(), 3, "{imports:?}");
    assert!(imports.iter().any(|r| r.expression == "*"
        && r.alias.as_deref() == Some("fs")
        && r.module.as_deref() == Some("fs")));
    assert!(imports.iter().any(|r| r.expression == "readFile"
        && r.alias.as_deref() == Some("readFile")
        && r.module.as_deref() == Some("node:fs/promises")));
    assert!(imports.iter().any(|r| r.expression == "writeFile"
        && r.alias.as_deref() == Some("write")
        && r.module.as_deref() == Some("node:fs/promises")));
    assert!(!imports
        .iter()
        .any(|r| r.module.as_deref() == Some("not-a-module")));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_destructuring_default_reads_invalidate_import_provenance() {
    let source = "const { spare = require } = require('node:assert/strict');\nspare('x');\n";
    let facts = ast::extract("consumer.cjs", "javascript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("consumer.cjs".to_owned(), facts)]));
    let coverage = graph
        .coverage
        .iter()
        .find(|row| row.line == 2 && row.expression == "spare")
        .unwrap_or_else(|| panic!("missing call coverage: {graph:#?}"));
    assert_eq!(coverage.status, "unresolved", "{coverage:#?}");
}

#[cfg(feature = "lang-rust")]
#[test]
fn cargo_dependencies_include_workspace_and_target_scoped_declarations() {
    let profile = languages::require("rust").unwrap();
    let manifest = "[dependencies]\nserde = \"1\"\n\"my-crate\" = { version = \"2\", package = \"actual-crate\" }\n\n[dev-dependencies]\npretty_assertions = \"1\"\n\n[target.'cfg(unix)'.dependencies]\nunix-only = \"1\"\n\n[workspace.dependencies]\nshared-crate = \"1\"\n\n[package.metadata.dependencies]\nnot-a-dependency = \"1\"\n\n[package]\nname = \"not-a-dependency\"\n";
    let dependencies = profile.extract_manifest_dependencies("Cargo.toml", manifest);
    assert_eq!(
        dependencies,
        [
            "my_crate",
            "pretty_assertions",
            "serde",
            "shared_crate",
            "unix_only"
        ]
    );
    assert!(profile.is_stdlib("std::collections"));
    assert!(profile.is_stdlib("core::fmt"));
    assert!(profile.is_stdlib("proc_macro::TokenStream"));
    assert!(!profile.is_stdlib("std_extra::thing"));
}

#[cfg(feature = "lang-go")]
#[test]
fn go_mod_dependencies_include_modules_and_versioned_paths() {
    let profile = languages::require("go").unwrap();
    assert_eq!(profile.manifest_filenames(), ["go.mod"]);
    let manifest = "module example.com/app\n\ngo 1.22\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgolang.org/x/sync v0.7.0 // indirect\n)\n\nrequire rsc.io/quote/v3 v3.1.0\n";
    let dependencies = profile.extract_manifest_dependencies("go.mod", manifest);
    assert_eq!(
        dependencies,
        [
            "github.com/gin-gonic/gin",
            "golang.org/x/sync",
            "quote",
            "rsc.io/quote/v3",
            "sync"
        ]
    );
}

#[cfg(feature = "lang-java")]
#[test]
fn gradle_and_maven_dependencies_include_configurations_and_group_ids() {
    let profile = languages::require("java").unwrap();
    assert_eq!(
        profile.manifest_filenames(),
        ["pom.xml", "build.gradle", "build.gradle.kts"]
    );
    let pom = r#"<project>
  <dependencies>
    <dependency>
      <groupId>com.google.guava</groupId>
      <artifactId>guava</artifactId>
      <version>33.0.0-jre</version>
    </dependency>
  </dependencies>
</project>"#;
    assert_eq!(
        profile.extract_manifest_dependencies("pom.xml", pom),
        ["com.google.guava"]
    );

    let gradle = r#"dependencies {
    implementation("org.springframework.boot:spring-boot-starter-web")
    testImplementation 'org.junit.jupiter:junit-jupiter:5.10.0'
}"#;
    assert_eq!(
        profile.extract_manifest_dependencies("build.gradle.kts", gradle),
        ["org.junit.jupiter", "org.springframework.boot"]
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn pyproject_and_pipfile_dependencies_include_optional_and_poetry_groups() {
    let profile = languages::require("python").unwrap();
    let pyproject = "[project]\nname='ignored'\ndependencies = [\n 'New-Package[extra]>=1; python_version > \\\"3\\\"', # comment\n \"another_package @ https://example.test/archive.whl\",\n]\n[project.optional-dependencies]\ntest=['pytest>=8']\n[tool.poetry.dependencies]\npython='^3.12'\nrequests={version='*',extras=['security']}\n[tool.poetry.group.dev.dependencies]\nhttpx='*'\n[tool.unrelated]\nrequests='ignored'\n";
    assert_eq!(
        profile.extract_manifest_dependencies("pyproject.toml", pyproject),
        vec![
            "another_package",
            "httpx",
            "new_package",
            "pytest",
            "requests"
        ]
    );
    assert_eq!(
        profile.extract_manifest_dependencies(
            "Pipfile",
            "[packages]\n\"new-package\"='*'\n[dev-packages]\npytest={version='*'}\n[requires]\npython_version='3.12'\n"
        ),
        vec!["new_package", "pytest"]
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn requirements_and_setup_cfg_parse_package_names_without_options_or_versions() {
    let profile = languages::require("python").unwrap();
    assert_eq!(
        profile.extract_manifest_dependencies(
            "requirements-dev.txt",
            "# comment\nnew-package[feature]>=1\nrequests @ https://example.test/requests.whl\n-r requirements.txt\n--index-url https://example.test\n-e git+https://example.test/repo#egg=editable-package\n./local-package\n"
        ),
        vec!["editable_package", "new_package", "requests"]
    );
    assert_eq!(
        profile.extract_manifest_dependencies(
            "setup.cfg",
            "[metadata]\nname=ignore\n[options]\ninstall_requires =\n    new-package>=1\n    requests\npython_requires = >=3.10\n[options.extras_require]\ntest =\n    pytest>=8\n"
        ),
        vec!["new_package", "pytest", "requests"]
    );
    assert!(profile.is_stdlib("pathlib.Path"));
    assert!(profile.external_import("unregistered_package").is_none());
}

#[cfg(feature = "lang-python")]
#[test]
fn pyproject_dependencies_accept_toml_multiline_strings_and_unicode_escapes() {
    let profile = languages::require("python").unwrap();
    let source = r#"[project]
dependencies = [
    '''
literal-package>=1''',
    """
basic-package>=1""",
    "\U0000006Eovel-package>=1",
]
"#;
    assert_eq!(
        profile.extract_manifest_dependencies("pyproject.toml", source),
        vec!["basic_package", "literal_package", "novel_package"]
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn requirements_reject_bare_urls_and_local_artifacts() {
    let profile = languages::require("python").unwrap();
    assert_eq!(
        profile.extract_manifest_dependencies(
            "requirements.txt",
            "git+https://example.test/repository\nhttps://example.test/archive.whl\nfile:///tmp/archive.whl\nrelative/archive.whl\narchive.whl\narchive.tar.gz\nvalid-package @ git+https://example.test/repository\n"
        ),
        vec!["valid_package"]
    );
}

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
        &format!(
            "roots: [.]\nexcluded_directory_names: [excluded]\nlinked_workspaces:\n  - name: enabled\n    path: '{}'\n    roots: [.]\n  - name: disabled\n    path: '{}'\n    enabled: false\n    roots: [.]\n",
            enabled.0.display(),
            disabled.0.display()
        ),
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
        &format!(
            "roots: [.]\nexcluded_directory_names: [excluded]\nlinked_workspaces:\n  - name: enabled\n    path: '{}'\n    roots: [.]\n  - name: disabled\n    path: '{}'\n    enabled: false\n    roots: [.]\n",
            enabled.0.display(),
            disabled.0.display()
        ),
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

#[cfg(feature = "lang-python")]
#[test]
fn manifest_only_delta_reclassifies_existing_import_and_matches_cold_graph() {
    let workspace = Workspace::new();
    workspace.write(
        "consumer.py",
        "import novel_library as client\ndef run():\n    return client.fetch()\n",
    );
    let db = workspace.0.join(".forge/index.db");
    writer::build(&workspace.0, &db, None).unwrap();
    workspace.write("requirements-extra.txt", "novel_library>=2\n");
    writer::delta(
        &workspace.0,
        &db,
        &[PathBuf::from("requirements-extra.txt")],
    )
    .unwrap();
    let cold = workspace.0.join(".forge/cold.db");
    writer::build(&workspace.0, &cold, None).unwrap();
    let query = "SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)";
    let snapshot = |path: &std::path::Path| {
        let conn = reader::open(path, &workspace.0).unwrap();
        let mut statement = conn.prepare(query).unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    assert_eq!(snapshot(&db), snapshot(&cold));
    assert!(snapshot(&db)
        .iter()
        .any(
            |(expression, status, evidence)| expression == "novel_library"
                && status == "external"
                && evidence.contains("manifest")
        ));
    fs::remove_file(workspace.0.join("requirements-extra.txt")).unwrap();
    writer::delta(
        &workspace.0,
        &db,
        &[PathBuf::from("requirements-extra.txt")],
    )
    .unwrap();
    assert!(snapshot(&db)
        .iter()
        .any(
            |(expression, status, evidence)| expression == "novel_library"
                && status == "external"
                && !evidence.contains("manifest")
        ));
}

#[cfg(feature = "lang-python")]
#[test]
fn unlisted_manifest_change_rebuilds_unmodified_import_owners() {
    let workspace = Workspace::new();
    workspace.write("consumer.py", "def run(): return 1\n");
    workspace.write(
        "unchanged.py",
        "import novel_library\ndef run(): return novel_library.fetch()\n",
    );
    let db = workspace.0.join(".forge/index.db");
    writer::build(&workspace.0, &db, None).unwrap();
    workspace.write("requirements.txt", "novel_library>=1\n");
    workspace.write("consumer.py", "def run(): return 2\n");
    writer::delta(&workspace.0, &db, &[PathBuf::from("consumer.py")]).unwrap();
    let conn = reader::open(&db, &workspace.0).unwrap();
    let evidence: String = conn.query_row("SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='unchanged.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='novel_library'", [], |row| row.get(0)).unwrap();
    assert!(evidence.contains("manifest"), "{evidence}");
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_namespace_origins_require_completed_stable_module_bindings() {
    use contextunity_forge_mcp::engine::linker;
    use std::collections::BTreeMap;

    let declaration = "const assert = require('node:assert/strict');\n";
    for (prefix, body, expected_status, invalidated) in [
        ("", "assert.equal(1, 1);", "external", false),
        ("", "function run() { assert.equal(1, 1); } run();", "external", false),
        ("", "function local(assert) { assert = custom; } function run() { assert.equal(1, 1); }", "external", false),
        ("", "function local(require) { require = custom; } function run() { assert.equal(1, 1); }", "external", false),
        ("", "for (let assert of values) { assert.equal = custom; } function run() { assert.equal(1, 1); }", "external", false),
        ("run();\n", "function run() { assert.equal(1, 1); }", "unresolved", false),
        ("require = custom;\n", "assert.equal(1, 1);", "unresolved", true),
        ("assert.equal(1, 1);\n", "", "unresolved", false),
        ("", "function run(assert) { assert.equal(1, 1); }", "unresolved", false),
        ("", "function run() { const assert = custom; assert.equal(1, 1); }", "unresolved", false),
        ("", "assert = custom; assert.equal(1, 1);", "unresolved", true),
        ("", "assert.equal = custom; assert.equal(1, 1);", "unresolved", true),
        ("", "[assert.equal] = [custom]; assert.equal(1, 1);", "unresolved", true),
        ("", "for (assert.equal of [custom]) {} assert.equal(1, 1);", "unresolved", true),
        ("", "assert++; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = assert; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = () => assert; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = { assert }; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = [assert]; assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = (assert); assert.equal(1, 1);", "unresolved", true),
        ("", "const escaped = assert.strict; escaped.equal = custom; assert.equal(1, 1);", "unresolved", true),
    ] {
        let source = format!("{prefix}{declaration}{body}");
        let facts = ast::extract("origin.cjs", "javascript", &source).unwrap();
        assert!(facts.errors.is_empty(), "{source}: {:?}", facts.errors);
        let module = facts.nodes.iter().find(|node| node.kind == "module").unwrap();
        let provenance = module.details["commonjs_bindings"].as_array().unwrap();
        assert_eq!(provenance.len(), 1, "{source}");
        assert_eq!(provenance[0]["alias"], "assert");
        assert_eq!(provenance[0]["module"], "node:assert/strict");
        assert_eq!(provenance[0]["invalidated"], invalidated, "{source}");
        assert_eq!(provenance[0]["captured_safe"], prefix.is_empty(), "{source}");
        let graph = linker::link(&BTreeMap::from([("origin.cjs".into(), facts)]));
        let coverage = graph.coverage.iter().find(|item| item.expression == "assert.equal" && item.line == source.lines().position(|line| line.contains("assert.equal(")).unwrap() + 1).unwrap();
        assert_eq!(coverage.status, expected_status, "{source}: {}", coverage.evidence);
        if expected_status == "external" {
            assert!(coverage.evidence.contains("node:assert.strict"), "{}", coverage.evidence);
        }
    }
    let facts = ast::extract("chain.cjs", "javascript", "const assert = require('node:assert');\nfunction run() { assert.strict.equal(1, 1); }\nrun();").unwrap();
    let module = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    assert_eq!(module.details["commonjs_bindings"][0]["invalidated"], false);
    let graph = linker::link(&BTreeMap::from([("chain.cjs".into(), facts)]));
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|item| item.expression == "assert.strict.equal")
            .unwrap()
            .status,
        "external"
    );
    for source in [
        "function run() { const assert = require('node:assert/strict'); assert.equal(1, 1); }",
        "if (enabled) { const assert = require('node:assert/strict'); assert.equal(1, 1); }",
    ] {
        let facts = ast::extract("scope.cjs", "javascript", source).unwrap();
        let module = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module")
            .unwrap();
        assert_eq!(module.details["commonjs_bindings"], serde_json::json!([]));
        let graph = linker::link(&BTreeMap::from([("scope.cjs".into(), facts)]));
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|item| item.expression == "assert.equal")
                .unwrap()
                .status,
            "unresolved"
        );
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_binding_provenance_persists_and_delta_matches_cold() {
    use contextunity_forge_mcp::core::commitments;
    let workspace = Workspace::new();
    let declaration = "const assert = require('node:assert/strict');\n";
    workspace.write(
        "origin.cjs",
        &format!("{declaration}function run() {{ assert.equal(1, 1); }}\nrun();\n"),
    );
    let db = workspace.0.join(".forge/commonjs.db");
    writer::build(&workspace.0, &db, None).unwrap();
    let check = |db: &std::path::Path, expected_status: &str, invalidated: bool| {
        let conn = reader::open(db, &workspace.0).unwrap();
        let details: String = conn
            .query_row(
                "SELECT details FROM nodes WHERE id='module:origin.cjs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let details: serde_json::Value = serde_json::from_str(&details).unwrap();
        assert_eq!(details["commonjs_bindings"][0]["invalidated"], invalidated);
        let status: String = conn.query_row("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='origin.cjs' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='assert.equal'", [], |row| row.get(0)).unwrap();
        assert_eq!(status, expected_status);
        commitments::verify(&conn).unwrap();
    };
    check(&db, "external", false);
    workspace.write("origin.cjs", &format!("{declaration}assert.equal = custom;\nfunction run() {{ assert.equal(1, 1); }}\nrun();\n"));
    writer::delta(&workspace.0, &db, &[PathBuf::from("origin.cjs")]).unwrap();
    check(&db, "unresolved", true);
    let cold = workspace.0.join(".forge/commonjs-cold.db");
    writer::build(&workspace.0, &cold, None).unwrap();
    check(&cold, "unresolved", true);
    let incremental = reader::open(&db, &workspace.0).unwrap();
    let cold = reader::open(&cold, &workspace.0).unwrap();
    for sql in [
        "SELECT path||'|'||hex(facts_blob) FROM local_facts ORDER BY path",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        "SELECT p.path||'|'||c.domain||'|'||hex(c.digest) FROM domain_commitments c JOIN path_dictionary p ON p.path_id=c.owner_id ORDER BY p.path,c.domain",
    ] {
        let rows = |conn: &rusqlite::Connection| conn.prepare(sql).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>();
        assert_eq!(rows(&incremental), rows(&cold), "{sql}");
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn commonjs_provider_body_edits_preserve_unchanged_consumer_contracts() {
    use contextunity_forge_mcp::core::commitments;
    let workspace = Workspace::new();
    let provider = |checkpoint| {
        format!("const assert = require('node:assert/strict');\nfunction work(value) {{\n  const checkpoint = {checkpoint};\n  assert.equal(value, value);\n  return value;\n}}\nmodule.exports = {{ work }};\n")
    };
    let before_source = provider(1);
    let after_source = provider(2000);
    let metadata = |source: &str| {
        ast::extract("provider.cjs", "javascript", source)
            .unwrap()
            .nodes
            .into_iter()
            .find(|node| node.kind == "module")
            .unwrap()
            .details["commonjs_bindings"]
            .clone()
    };
    assert_eq!(metadata(&before_source), metadata(&after_source));
    workspace.write("provider.cjs", &before_source);
    workspace.write(
        "consumer.cjs",
        "const provider = require('./provider');\nfunction run() { provider.work(1); }\nrun();\n",
    );
    let db = workspace.0.join(".forge/commonjs-body.db");
    writer::build(&workspace.0, &db, None).unwrap();
    let snapshot = |db: &std::path::Path| {
        let conn = reader::open(db, &workspace.0).unwrap();
        commitments::verify(&conn).unwrap();
        let mut statement = conn.prepare("SELECT domain||'|'||hex(digest) FROM domain_commitments WHERE owner_id=(SELECT path_id FROM path_dictionary WHERE path='consumer.cjs') ORDER BY domain").unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    let consumer_before = snapshot(&db);
    workspace.write("provider.cjs", &after_source);
    let report = writer::delta(&workspace.0, &db, &[PathBuf::from("provider.cjs")]).unwrap();
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(snapshot(&db), consumer_before);
    let cold = workspace.0.join(".forge/commonjs-body-cold.db");
    writer::build(&workspace.0, &cold, None).unwrap();
    assert_eq!(snapshot(&db), snapshot(&cold));
}

#[cfg(feature = "lang-html")]
#[test]
fn framework_manifest_requires_all_rule_tables_and_accepts_complete_manifest() {
    let tables = ["receivers", "builtins", "filters", "routes"];
    let mut failures = Vec::new();
    let complete_toml = "receivers = [{ name = 'render', receiver = 'request' }]\n\
        builtins = ['$refs', '$dispatch']\n\
        filters = ['escape']\n\
        [[routes]]\n\
        pattern = '/users/{id}'\n\
        method = 'GET'\n\
        [[routes]]\n\
        pattern = '/teams/{id}'\n\
        method = 'POST'\n";
    let complete_yaml = "receivers:\n  - name: render\n    receiver: request\nbuiltins: [\"$refs\", \"$dispatch\"]\nfilters: [escape]\nroutes:\n  - pattern: \"/users/{id}\"\n    method: GET\n  - pattern: \"/teams/{id}\"\n    method: POST\n";

    for (extension, complete_manifest) in [
        ("toml", complete_toml),
        ("yaml", complete_yaml),
        ("yml", complete_yaml),
    ] {
        let workspace = Workspace::new();
        workspace.write(
            "pyproject.toml",
            "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
        );
        workspace.write("index.html", "<p>Manifest-backed project</p>\n");
        workspace.write(
            format!(".forge/frameworks/django.{extension}"),
            complete_manifest,
        );
        if let Err(error) = writer::build(&workspace.0, &workspace.db(), None) {
            failures.push(format!(
                "valid .{extension} manifest failed to build: {error:#}"
            ));
            continue;
        }

        let dependencies = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
        let Some(loaded) = dependencies.framework_manifest_for_path(
            LanguageFamily("python"),
            "index.html",
            "django",
            "django",
        ) else {
            failures.push(format!(
                "complete .{extension} manifest did not reach its linker"
            ));
            continue;
        };
        if loaded.name != "django" {
            failures.push(format!(
                ".{extension} manifest name was {:?}, expected django",
                loaded.name
            ));
        }
        if !matches!(
            loaded.receivers.as_slice(),
            [FrameworkManifestValue::Table(fields)]
                if fields.get("name") == Some(&FrameworkManifestValue::String("render".into()))
                    && fields.get("receiver") == Some(&FrameworkManifestValue::String("request".into()))
        ) {
            failures.push(format!(
                ".{extension} receiver array did not retain its typed table value"
            ));
        }
        if loaded.builtins
            != vec![
                FrameworkManifestValue::String("$refs".into()),
                FrameworkManifestValue::String("$dispatch".into()),
            ]
        {
            failures.push(format!(
                ".{extension} builtins did not deserialize as a typed string array"
            ));
        }
        if loaded.filters != vec![FrameworkManifestValue::String("escape".into())] {
            failures.push(format!(
                ".{extension} filters did not deserialize as a typed string array"
            ));
        }
        if !matches!(
            loaded.routes.as_slice(),
            [FrameworkManifestValue::Table(first), FrameworkManifestValue::Table(second)]
                if first.get("pattern") == Some(&FrameworkManifestValue::String("/users/{id}".into()))
                    && first.get("method") == Some(&FrameworkManifestValue::String("GET".into()))
                    && second.get("pattern") == Some(&FrameworkManifestValue::String("/teams/{id}".into()))
                    && second.get("method") == Some(&FrameworkManifestValue::String("POST".into()))
        ) {
            failures.push(format!(
                ".{extension} route array did not retain both typed table values"
            ));
        }
        let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
        let indexed_file: i64 = conn
            .query_row(
                "SELECT count(*) FROM path_dictionary WHERE path='index.html'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        if indexed_file != 1 {
            failures.push(format!(
                "complete .{extension} manifest published {indexed_file} index rows"
            ));
        }
    }

    for extension in ["toml", "yaml", "yml"] {
        for missing in tables {
            let manifest = tables
                .iter()
                .filter(|table| **table != missing)
                .map(|table| {
                    if extension == "toml" {
                        format!("{table} = []\n")
                    } else {
                        format!("{table}: []\n")
                    }
                })
                .collect::<String>();
            let workspace = Workspace::new();
            workspace.write(
                "pyproject.toml",
                "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
            );
            workspace.write("index.html", "<p>Missing required manifest table</p>\n");
            workspace.write(format!(".forge/frameworks/django.{extension}"), &manifest);

            let result = writer::build(&workspace.0, &workspace.db(), None);
            if result.is_ok() {
                failures.push(format!(
                    ".{extension} manifest missing {missing} was accepted"
                ));
            }
            if workspace.db().exists() {
                failures.push(format!(
                    "rejected .{extension} manifest missing {missing} published an index"
                ));
            }
        }
    }

    for extension in ["toml", "yaml", "yml"] {
        for non_array in tables {
            let manifest = tables
                .iter()
                .map(|table| {
                    if *table == non_array {
                        if extension == "toml" {
                            format!("{table} = 'not-an-array'\n")
                        } else {
                            format!("{table}: not-an-array\n")
                        }
                    } else if extension == "toml" {
                        format!("{table} = []\n")
                    } else {
                        format!("{table}: []\n")
                    }
                })
                .collect::<String>();
            let workspace = Workspace::new();
            workspace.write(
                "pyproject.toml",
                "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
            );
            workspace.write("index.html", "<p>Non-array manifest table</p>\n");
            workspace.write(format!(".forge/frameworks/django.{extension}"), &manifest);

            let result = writer::build(&workspace.0, &workspace.db(), None);
            if result.is_ok() {
                failures.push(format!(
                    ".{extension} manifest with non-array {non_array} was accepted"
                ));
            }
            if workspace.db().exists() {
                failures.push(format!(
                    "rejected .{extension} manifest with non-array {non_array} published an index"
                ));
            }
        }
    }

    let undeclared = Workspace::new();
    undeclared.write("index.html", "<p>No framework declaration</p>\n");
    undeclared.write(".forge/frameworks/django.toml", complete_toml);
    let undeclared_dependencies = DependencyRegistry::collect_with_scan_config(&undeclared.0, None);
    if undeclared_dependencies
        .framework_manifest_for_path(LanguageFamily("python"), "index.html", "django", "django")
        .is_some()
    {
        failures.push("an undeclared project received the Django manifest".to_owned());
    }
    assert!(
        failures.is_empty(),
        "manifest contract failures:\n{}",
        failures.join("\n")
    );
}

#[cfg(feature = "lang-html")]
#[test]
fn invalid_user_framework_manifest_fails_before_index_publication() {
    let complete = "receivers = []\nbuiltins = []\nfilters = []\nroutes = []\n";
    let mut failures = Vec::new();
    let invalid_manifests = [
        ("malformed TOML syntax", "toml", "receivers = [\n".to_owned()),
        (
            "duplicate table headers",
            "toml",
            format!("{complete}[unused]\nfirst = 1\n[unused]\nsecond = 2\n"),
        ),
        (
            "duplicate keys",
            "toml",
            format!("{complete}[unused]\nfirst = 1\nfirst = 2\n"),
        ),
        (
            "required array redeclared as a table",
            "toml",
            format!("{complete}[receivers]\nname = 'duplicate-shape'\n"),
        ),
        (
            "nested table before dotted key",
            "toml",
            format!(
                "{complete}[unused.receivers]\nname = 'declared-as-table'\n[unused]\nreceivers = []\n"
            ),
        ),
        (
            "dotted key before nested table",
            "toml",
            format!(
                "{complete}[unused]\nreceivers = []\n[unused.receivers]\nname = 'declared-as-table'\n"
            ),
        ),
        (
            "malformed YAML syntax",
            "yaml",
            "receivers: [\nbuiltins: []\nfilters: []\nroutes: []\n".to_owned(),
        ),
        (
            "malformed YAML syntax",
            "yml",
            "receivers: [\nbuiltins: []\nfilters: []\nroutes: []\n".to_owned(),
        ),
    ];

    for (invalid_kind, extension, manifest) in invalid_manifests {
        let workspace = Workspace::new();
        workspace.write(
            "pyproject.toml",
            "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
        );
        workspace.write(format!(".forge/frameworks/django.{extension}"), &manifest);
        workspace.write(
            "shop/views.py",
            "from django.shortcuts import render\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n",
        );
        workspace.write("shop/templates/page.html", "<h1>{{ title }}</h1>\n");

        let result = writer::build(&workspace.0, &workspace.db(), None);
        if result.is_ok() {
            failures.push(format!(
                "a declared Django project with {invalid_kind} in its .{extension} manifest was accepted"
            ));
        }
        if workspace.db().exists() {
            failures.push(format!(
                "rejected .{extension} manifest with {invalid_kind} published an index containing framework edges"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "invalid manifest contract failures:\n{}",
        failures.join("\n")
    );
}
