use super::{connection, owner_id, serial_leaf};
use contextunity_forge_mcp::core::commitments;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;

pub(super) fn seed_canonical(conn: &Connection) {
    conn.execute_batch(
        "WITH RECURSIVE n(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<63)
         INSERT INTO coverage_expressions(expression)
         SELECT 'item_'||printf('%02d',x) FROM n ORDER BY x DESC;
         INSERT INTO coverage_evidence(evidence) VALUES('λz'),('a'),(''),(char(0)||'suffix');
         INSERT INTO resolution_coverage_data(path_id,line,expression_id,status,evidence_id)
         SELECT p.path_id,x.expression_id%2,x.expression_id,
             CASE e.evidence_id%4 WHEN 0 THEN '' WHEN 1 THEN 'custom/λ'
                 WHEN 2 THEN 'resolved' ELSE char(0)||'pending' END,e.evidence_id
         FROM path_dictionary p CROSS JOIN coverage_expressions x CROSS JOIN coverage_evidence e
         WHERE p.path LIKE 'src/owner_%';",
    )
    .unwrap();
}

fn assert_coverage_leaf(conn: &Connection, owner: &str) {
    let key = format!("leaf:{}", serde_json::json!(["resolution_coverage", owner]));
    let actual: String = conn
        .query_row(
            "SELECT digest FROM domain_commitments WHERE domain=?1",
            [key],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        actual,
        serial_leaf(
            conn,
            "resolution_coverage",
            owner,
            "line,expression,status,evidence"
        )
    );
}

#[test]
fn coverage_inner_joins_and_partial_owner_updates_preserve_serial_commitments() {
    let conn = connection();
    let selected = owner_id(&conn, "src/owner_a.rs");
    owner_id(&conn, "src/owner_b.rs");
    seed_canonical(&conn);
    let initial = commitments::seal(&conn).unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO resolution_coverage_data VALUES(-999,1,1,'orphan-path',1),
          ({selected},2,-999,'orphan-expression',1),
          ({selected},3,1,'orphan-evidence',-999);"
    ))
    .unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), initial);
    conn.execute(
        "UPDATE resolution_coverage_data SET line=line+10 WHERE path_id=?1 AND line=0",
        [selected],
    )
    .unwrap();
    let selected = BTreeSet::from(["src/owner_a.rs".to_owned()]);
    let partial = commitments::seal_owners(&conn, Some(&selected)).unwrap();
    assert_ne!(partial, initial);
    for owner in ["src/owner_a.rs", "src/owner_b.rs"] {
        assert_coverage_leaf(&conn, owner);
    }
    assert_eq!(commitments::seal(&conn).unwrap(), partial);
    commitments::verify_owners(&conn, Some(&selected)).unwrap();
    commitments::verify(&conn).unwrap();
}

#[test]
fn coverage_non_strict_values_keep_sql_storage_tags_and_ordering() {
    for update in [
        "UPDATE resolution_coverage_data SET line=1.5",
        "UPDATE resolution_coverage_data SET line='noninteger'",
        "UPDATE resolution_coverage_data SET status=x'00ff'",
        "UPDATE resolution_coverage_data SET status=CAST(x'ff' AS TEXT)",
        "UPDATE coverage_expressions SET expression=x'ff00'",
        "UPDATE coverage_evidence SET evidence=CAST(x'ff' AS TEXT)",
        "INSERT INTO coverage_expressions(expression) VALUES(CAST(x'ff' AS TEXT))",
        "INSERT INTO coverage_evidence(evidence) VALUES(x'ff')",
    ] {
        let conn = connection();
        owner_id(&conn, "src/owner_a.rs");
        conn.execute(
            "INSERT INTO resolution_coverage VALUES(?1,1,?2,?3,?4)",
            params![
                "src/owner_a.rs",
                "expression",
                "arbitrary-status",
                "evidence"
            ],
        )
        .unwrap();
        conn.execute_batch(update).unwrap();
        let sealed = commitments::seal(&conn).unwrap();
        assert_coverage_leaf(&conn, "src/owner_a.rs");
        assert_eq!(
            commitments::seal_owners(&conn, Some(&BTreeSet::from(["src/owner_a.rs".to_owned()])))
                .unwrap(),
            sealed
        );
        commitments::verify(&conn).unwrap();
    }
}
