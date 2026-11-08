use super::*;

const CONSUMER: &str = "import novel_library as client\ndef run():\n    return client.fetch()\n";
const EXTRA_REQUIREMENTS: &str = "novel_library>=2\n";

#[cfg(feature = "lang-python")]
#[test]
fn manifest_addition_reclassifies_existing_import_and_matches_cold_graph() {
    let workspace = FrameworkHarness::published(&[("consumer.py", CONSUMER)]);
    workspace.write("requirements-extra.txt", EXTRA_REQUIREMENTS);
    workspace.delta(&["requirements-extra.txt"]);
    let incremental = coverage_details(&workspace.open());
    let cold_workspace = FrameworkHarness::published(&[
        ("consumer.py", CONSUMER),
        ("requirements-extra.txt", EXTRA_REQUIREMENTS),
    ]);
    let cold = coverage_details(&cold_workspace.open());
    assert_eq!(incremental, cold);
    assert!(incremental.iter().any(
        |(expression, status, evidence)| expression == "novel_library"
            && status == "external"
            && evidence.contains("manifest")
    ));
}
#[cfg(feature = "lang-python")]
#[test]
fn manifest_removal_restores_unlisted_import_origin() {
    let workspace = FrameworkHarness::published(&[
        ("consumer.py", CONSUMER),
        ("requirements-extra.txt", EXTRA_REQUIREMENTS),
    ]);
    workspace.remove("requirements-extra.txt");
    workspace.delta(&["requirements-extra.txt"]);
    let coverage = coverage_details(&workspace.open());
    assert!(coverage.iter().any(
        |(expression, status, evidence)| expression == "novel_library"
            && status == "external"
            && !evidence.contains("manifest")
    ));
}
#[cfg(feature = "lang-python")]
#[test]
fn unlisted_manifest_change_rebuilds_unmodified_import_owners() {
    let workspace = FrameworkHarness::published(&[
        ("consumer.py", "def run(): return 1\n"),
        (
            "unchanged.py",
            "import novel_library\ndef run(): return novel_library.fetch()\n",
        ),
    ]);
    workspace.write("requirements.txt", "novel_library>=1\n");
    workspace.write("consumer.py", "def run(): return 2\n");
    workspace.delta(&["consumer.py"]);
    let conn = workspace.open();
    let evidence = coverage_status_and_evidence(&conn, "unchanged.py", "novel_library").1;
    assert!(evidence.contains("manifest"), "{evidence}");
}
