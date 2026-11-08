use super::harness::*;
use rusqlite::params;

#[test]
fn manifest_input_digest_is_committed_and_tampering_is_detected() {
    let conn = connection();
    conn.execute(
        "INSERT INTO metadata VALUES('manifest_digest','before')",
        [],
    )
    .unwrap();
    let original_root = commitments::seal(&conn).unwrap();
    commitments::verify(&conn).expect("the original manifest digest verifies");

    conn.execute(
        "UPDATE metadata SET value='after' WHERE key='manifest_digest'",
        [],
    )
    .unwrap();
    assert!(commitments::verify(&conn).is_err());
    let tampered_root =
        commitments::seal(&conn).expect("tampered manifest digest can be resealed");
    assert_ne!(tampered_root, original_root);
}

#[test]
fn coverage_partial_owner_update_matches_full_seal() {
    let conn = connection();
    let selected_owner = owner_id(&conn, "src/owner_a.rs");
    owner_id(&conn, "src/owner_b.rs");
    conn.execute_batch(
        "WITH RECURSIVE n(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<63)
         INSERT INTO coverage_expressions(expression)
         SELECT 'item_'||printf('%02d',x) FROM n ORDER BY x DESC;
         INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES('λz'),('a'),(''),(char(0)||'suffix');
         INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id)
         SELECT p.path_id,x.expression_id%2,x.expression_id,
             CASE e.evidence_id%4 WHEN 0 THEN 'resolved' WHEN 1 THEN 'external'
                 WHEN 2 THEN 'unresolved' ELSE 'ambiguous' END,e.evidence_id
         FROM path_dictionary p CROSS JOIN coverage_expressions x CROSS JOIN coverage_evidence e
         WHERE p.path LIKE 'src/owner_%';",
    )
    .unwrap();
    let initial_root = commitments::seal(&conn).unwrap();
    let selected_leaf_before = coverage_leaf(&conn, "src/owner_a.rs");
    let other_leaf_before = coverage_leaf(&conn, "src/owner_b.rs");
    conn.execute(
        "UPDATE resolution_coverage SET line=line+10 WHERE path_id=?1 AND line=0",
        [selected_owner],
    )
    .unwrap();
    let selected = std::collections::BTreeSet::from(["src/owner_a.rs".to_owned()]);
    let partial_root = commitments::seal_owners(&conn, Some(&selected))
        .expect("partial owner sealing succeeds");
    let selected_leaf_after = coverage_leaf(&conn, "src/owner_a.rs");
    let other_leaf_after = coverage_leaf(&conn, "src/owner_b.rs");
    assert_ne!(partial_root, initial_root);
    assert_ne!(selected_leaf_after, selected_leaf_before);
    assert_eq!(other_leaf_after, other_leaf_before);

    let full_root_after = commitments::seal(&conn)
        .expect("full sealing succeeds after the owner update");
    assert_eq!(full_root_after, partial_root);

    commitments::verify_owners(&conn, Some(&selected))
        .expect("selected owner verification succeeds");
    commitments::verify(&conn).expect("full verification succeeds");
}

#[test]
fn coverage_uses_dictionary_values_for_unicode_and_custom_status() {
    let conn = connection();
    let owner = owner_id(&conn, "src/owner_a.rs");
    conn.execute(
        "INSERT INTO coverage_expressions(expression) VALUES('λ.receiver')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO coverage_evidence(evidence) VALUES('line λ: resolved')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO resolution_coverage VALUES(?1,7,
         (SELECT expression_id FROM coverage_expressions WHERE expression='λ.receiver'),
         'custom-status',
         (SELECT evidence_id FROM coverage_evidence WHERE evidence='line λ: resolved'))",
        params![owner],
    )
    .unwrap();
    let original_root = commitments::seal(&conn).unwrap();
    let leaf_before = coverage_leaf(&conn, "src/owner_a.rs");
    assert_eq!(leaf_before.len(), 32);

    commitments::verify(&conn).expect("dictionary-backed Unicode evidence verifies");

    conn.execute(
        "UPDATE coverage_evidence SET evidence='line λ: changed' WHERE evidence='line λ: resolved'",
        [],
    )
    .unwrap();
    let root_after = commitments::seal(&conn)
        .expect("coverage evidence mutation seals");
    assert_ne!(root_after, original_root);
}

#[test]
fn resolution_coverage_rejects_rows_with_missing_dictionary_entries() {
    #[derive(Clone, Copy)]
    enum MissingEntry {
        Path,
        Expression,
        Evidence,
    }

    let cases = [
        (MissingEntry::Path, "path"),
        (MissingEntry::Expression, "expression"),
        (MissingEntry::Evidence, "evidence"),
    ];
    let conn = connection();
    let owner = owner_id(&conn, "src/owner_a.rs");
    conn.execute(
        "INSERT INTO coverage_expressions(expression) VALUES('known.expression')",
        [],
    )
    .unwrap();
    let expression: i64 = conn
        .query_row(
            "SELECT expression_id FROM coverage_expressions WHERE expression='known.expression'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT INTO resolution_coverage VALUES(?1,1,?2,'resolved',1)",
        params![owner, expression],
    )
    .unwrap();
    commitments::seal(&conn)
        .expect("canonical coverage row seals before missing dictionary references");
    conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    let orphan = 10_000;
    for (entry, label) in cases {
        let (path_id, expression_id, evidence_id) = match entry {
            MissingEntry::Path => (orphan, expression, 1),
            MissingEntry::Expression => (owner, orphan, 1),
            MissingEntry::Evidence => (owner, expression, orphan),
        };
        conn.execute_batch("SAVEPOINT missing_coverage_dictionary;")
            .unwrap();
        conn.execute(
            "INSERT INTO resolution_coverage VALUES(?1,2,?2,'resolved',?3)",
            params![path_id, expression_id, evidence_id],
        )
        .unwrap();
        let verification = commitments::verify(&conn);
        let seal_after_orphan = commitments::seal(&conn);
        conn.execute_batch(
            "ROLLBACK TO missing_coverage_dictionary; RELEASE missing_coverage_dictionary;",
        )
        .unwrap();
        assert!(
            verification.is_err(),
            "verify accepted a resolution_coverage row with a missing {label} dictionary entry"
        );
        assert!(
            seal_after_orphan.is_err(),
            "seal accepted a resolution_coverage row with a missing {label} dictionary entry"
        );
    }
}
