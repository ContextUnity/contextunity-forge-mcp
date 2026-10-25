use super::*;

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
