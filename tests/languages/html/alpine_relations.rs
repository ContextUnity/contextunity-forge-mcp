use super::*;

#[test]
fn alpine_magic_members_require_declared_alpine_dependency() {
    let workspace = Workspace::new();
    let script = "<script>const refs = this.$refs; this.$dispatch('shown');</script>\n";
    workspace.write(
        "declared/package.json",
        r#"{"dependencies":{"alpinejs":"^3.14.0"}}"#,
    );
    workspace.write("declared/index.html", script);
    workspace.write("plain/package.json", r#"{"dependencies":{"vue":"^3.5.0"}}"#);
    workspace.write("plain/index.html", script);

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let observations = [
        ("declared/index.html", "this.$refs", "external"),
        ("declared/index.html", "this.$dispatch", "external"),
        ("plain/index.html", "this.$refs", "unresolved"),
        ("plain/index.html", "this.$dispatch", "unresolved"),
    ]
    .map(|(path, expression, expected)| {
        let statuses: Vec<(String, String)> = conn
            .prepare("SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path=?1 AND ce.expression=?2 ORDER BY rc.status,ev.evidence")
            .unwrap()
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        ((path, expression), (expected, statuses))
    });
    assert!(
        observations
            .iter()
            .all(|(_, (expected, statuses))| {
                statuses.len() == 1 && statuses[0].0.as_str() == *expected
            }),
        "Alpine magic must be external only in the package that declares alpinejs: {observations:#?}"
    );
    commitments::verify(&conn).unwrap();
}
