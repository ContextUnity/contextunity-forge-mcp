#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::languages::{self, manifests::DependencyRegistry},
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge_manifests_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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
    workspace.write("custom-adapter.yaml", &format!("roots: [.]\nexcluded_directory_names: [excluded]\nlinked_workspaces:\n  - name: enabled\n    path: '{}'\n    roots: [.]\n  - name: disabled\n    path: '{}'\n    enabled: false\n    roots: [.]\n", enabled.0.display(), disabled.0.display()));
    let adapter = contextunity_forge_mcp::engine::scanner::load_adapter(
        &workspace.0,
        Some(&workspace.0.join("custom-adapter.yaml")),
    )
    .unwrap();
    let registry = DependencyRegistry::collect_with_adapter(&workspace.0, Some(&adapter));
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
    let query = "SELECT expression,status,evidence FROM resolution_coverage ORDER BY path,line,expression,status,evidence";
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
    let evidence: String = conn.query_row("SELECT evidence FROM resolution_coverage WHERE path='unchanged.py' AND expression='novel_library'", [], |row| row.get(0)).unwrap();
    assert!(evidence.contains("manifest"), "{evidence}");
}
