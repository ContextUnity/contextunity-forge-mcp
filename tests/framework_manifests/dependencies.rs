use super::*;

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
            "rsc.io/quote/v3"
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
    let mut dependencies = profile.extract_manifest_dependencies("build.gradle.kts", gradle);
    dependencies.sort();
    assert_eq!(
        dependencies,
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
