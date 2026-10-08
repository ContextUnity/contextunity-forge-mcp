#![cfg(feature = "lang-python")]

use crate::common::Workspace;
use contextunity_forge_mcp::core::commitments;
use rusqlite::Connection;
fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}
fn edges(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT json_array((SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind,(SELECT path FROM path_dictionary WHERE path_id=edges.path_id),line,(SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id),(SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.confidence_id),occurrence_count) FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind")
}

#[test]
fn cold_duplicate_edges_preserve_first_evidence_and_every_occurrence() {
    let w = Workspace::new();
    w.write("provider.py", "def target(): pass\ndef other(): pass\n");
    w.write("consumer.py", "from provider import target as z, target as a, other\ndef run():\n    z()\n    a(); z(); a()\n    other()\n");
    w.build();
    let conn = w.open();
    commitments::verify(&conn).unwrap();
    let target: (i64, i64, String, String) = conn.query_row(
        "SELECT e.occurrence_count,e.line,(SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id),(SELECT evidence FROM coverage_evidence WHERE evidence_id=e.confidence_id) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.qualname='consumer.run' AND d.qualname='provider.target'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(target, (4, 3, "z".into(), "exact".into()));
    let calls: i64 = conn
        .query_row(
            "SELECT count(*) FROM edge_occurrences WHERE kind='calls'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(calls, 5);
    let count_mismatches: i64 = conn.query_row("SELECT count(*) FROM edges e WHERE occurrence_count != (SELECT count(*) FROM edge_occurrences o WHERE o.src_hash=e.src_hash AND o.dst_hash=e.dst_hash AND o.kind=e.kind)", [], |row| row.get(0)).unwrap();
    assert_eq!(count_mismatches, 0);
    let before = edges(&conn);
    drop(conn);
    w.build();
    assert_eq!(edges(&w.open()), before);
}

#[test]
fn duplicate_counts_after_delta_match_cold_and_pass_full_commitment_verification() {
    let w = Workspace::new();
    w.write("provider.py", "def target(): pass\n");
    w.write(
        "consumer.py",
        "from provider import target\ndef run():\n    target()\n    target()\n",
    );
    w.build();
    w.write(
        "consumer.py",
        "from provider import target\ndef run():\n    target()\n    target(); target()\n",
    );
    w.delta(&["consumer.py"]);
    let incremental = w.open();
    commitments::verify(&incremental).unwrap();
    let count: i64 = incremental
        .query_row(
            "SELECT occurrence_count FROM edges WHERE kind='calls'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
    let before = edges(&incremental);
    drop(incremental);
    w.build();
    let cold = w.open();
    commitments::verify(&cold).unwrap();
    assert_eq!(edges(&cold), before);
}

#[test]
fn delta_keeps_first_real_alias_evidence_instead_of_mixing_occurrences() {
    let w = Workspace::new();
    w.write("provider.py", "def target(): pass\n");
    let source = "from provider import target as z, target as a\ndef run():\n    z()\n    a()\n";
    w.write("consumer.py", source);
    w.build();
    let cold = edges(&w.open());
    w.write("consumer.py", &format!("{source}\n"));
    w.delta(&["consumer.py"]);
    let delta = w.open();
    commitments::verify(&delta).unwrap();
    assert_eq!(edges(&delta), cold);
}
