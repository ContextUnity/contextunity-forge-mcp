#![cfg(feature = "lang-python")]
use contextunity_forge_mcp::engine::languages;

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
    assert_eq!(profile.extract_manifest_dependencies("Pipfile", "[packages]\n\"new-package\"='*'\n[dev-packages]\npytest={version='*'}\n[requires]\npython_version='3.12'\n"), vec!["new_package", "pytest"]);
}

#[test]
fn requirements_and_setup_cfg_parse_package_names_without_options_or_versions() {
    let profile = languages::require("python").unwrap();
    assert_eq!(profile.extract_manifest_dependencies("requirements-dev.txt", "# comment\nnew-package[feature]>=1\nrequests @ https://example.test/requests.whl\n-r requirements.txt\n--index-url https://example.test\n-e git+https://example.test/repo#egg=editable-package\n./local-package\n"), vec!["editable_package", "new_package", "requests"]);
    assert_eq!(profile.extract_manifest_dependencies("setup.cfg", "[metadata]\nname=ignore\n[options]\ninstall_requires =\n    new-package>=1\n    requests\npython_requires = >=3.10\n[options.extras_require]\ntest =\n    pytest>=8\n"), vec!["new_package", "pytest", "requests"]);
    assert!(profile.is_stdlib("pathlib.Path"));
    assert!(profile.external_import("unregistered_package").is_none());
}

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

#[test]
fn requirements_reject_bare_urls_and_local_artifacts() {
    let profile = languages::require("python").unwrap();
    assert_eq!(profile.extract_manifest_dependencies("requirements.txt", "git+https://example.test/repository\nhttps://example.test/archive.whl\nfile:///tmp/archive.whl\nrelative/archive.whl\narchive.whl\narchive.tar.gz\nvalid-package @ git+https://example.test/repository\n"), vec!["valid_package"]);
}
