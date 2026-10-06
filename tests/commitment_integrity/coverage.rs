use super::{connection, owner_id};
use contextunity_forge_mcp::core::commitments;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;

pub(super) fn seed_canonical(conn: &Connection) {
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
}

fn coverage_leaf(conn: &Connection, owner: &str) -> Vec<u8> {
    conn.query_row(
        "SELECT digest FROM domain_commitments WHERE domain='resolution_coverage'
         AND owner_id=(SELECT path_id FROM path_dictionary WHERE path=?1)",
        [owner],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn coverage_partial_owner_update_matches_full_seal() {
    let conn = connection();
    let selected = owner_id(&conn, "src/owner_a.rs");
    owner_id(&conn, "src/owner_b.rs");
    seed_canonical(&conn);
    let initial = commitments::seal(&conn).unwrap();
    let previous_selected = coverage_leaf(&conn, "src/owner_a.rs");
    let previous_other = coverage_leaf(&conn, "src/owner_b.rs");
    conn.execute(
        "UPDATE resolution_coverage SET line=line+10 WHERE path_id=?1 AND line=0",
        [selected],
    )
    .unwrap();
    let selected = BTreeSet::from(["src/owner_a.rs".to_owned()]);
    let partial = commitments::seal_owners(&conn, Some(&selected)).unwrap();
    assert_ne!(partial, initial);
    assert_ne!(coverage_leaf(&conn, "src/owner_a.rs"), previous_selected);
    assert_eq!(coverage_leaf(&conn, "src/owner_b.rs"), previous_other);
    assert_eq!(commitments::seal(&conn).unwrap(), partial);
    commitments::verify_owners(&conn, Some(&selected)).unwrap();
    commitments::verify(&conn).unwrap();
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
    let initial = commitments::seal(&conn).unwrap();
    assert_eq!(coverage_leaf(&conn, "src/owner_a.rs").len(), 32);
    commitments::verify(&conn).unwrap();
    conn.execute(
        "UPDATE coverage_evidence SET evidence='line λ: changed' WHERE evidence='line λ: resolved'",
        [],
    )
    .unwrap();
    assert_ne!(commitments::seal(&conn).unwrap(), initial);
}

#[test]
fn resolution_coverage_rejects_rows_with_missing_dictionary_entries() {
    for missing in ["path", "expression", "evidence"] {
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
        commitments::seal(&conn).unwrap();

        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        let orphan = 10_000;
        let (path_id, expression_id, evidence_id) = match missing {
            "path" => (orphan, expression, 1),
            "expression" => (owner, orphan, 1),
            "evidence" => (owner, expression, orphan),
            _ => unreachable!(),
        };
        conn.execute(
            "INSERT INTO resolution_coverage VALUES(?1,2,?2,'resolved',?3)",
            params![path_id, expression_id, evidence_id],
        )
        .unwrap();

        assert!(
            commitments::verify(&conn).is_err(),
            "verify accepted a resolution_coverage row with a missing {missing} dictionary entry"
        );
        assert!(
            commitments::seal(&conn).is_err(),
            "seal accepted a resolution_coverage row with a missing {missing} dictionary entry"
        );
    }
}
