#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{core::commitments, db::{reader, writer}};
use rusqlite::Connection;
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("forge_edge_aggregation_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn db(&self) -> PathBuf { self.0.join(".forge/code-map.sqlite") }
    fn build(&self) { writer::build(&self.0, &self.db(), None).unwrap(); }
    fn open(&self) -> Connection { reader::open(&self.db(), &self.0).unwrap() }
}
impl Drop for Workspace {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}
fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql).unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
}
fn edges(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT json_array(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) FROM edges ORDER BY src_public_id,dst_public_id,kind")
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
        "SELECT e.occurrence_count,e.line,e.evidence,e.confidence FROM edges e JOIN nodes s ON s.id=e.src_public_id JOIN nodes d ON d.id=e.dst_public_id WHERE e.kind='calls' AND s.qualname='consumer.run' AND d.qualname='provider.target'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(target, (4, 3, "z".into(), "exact".into()));
    let calls: i64 = conn.query_row("SELECT count(*) FROM edge_occurrences WHERE kind='calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(calls, 5);
    let count_mismatches: i64 = conn.query_row("SELECT count(*) FROM edges e WHERE occurrence_count != (SELECT count(*) FROM edge_occurrences o WHERE o.src=e.src_public_id AND o.dst=e.dst_public_id AND o.kind=e.kind)", [], |row| row.get(0)).unwrap();
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
    w.write("consumer.py", "from provider import target\ndef run():\n    target()\n    target()\n");
    w.build();
    w.write("consumer.py", "from provider import target\ndef run():\n    target()\n    target(); target()\n");
    writer::delta(&w.0, &w.db(), &[PathBuf::from("consumer.py")]).unwrap();
    let incremental = w.open();
    commitments::verify(&incremental).unwrap();
    let count: i64 = incremental.query_row("SELECT occurrence_count FROM edges WHERE kind='calls'", [], |row| row.get(0)).unwrap();
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
    writer::delta(&w.0, &w.db(), &[PathBuf::from("consumer.py")]).unwrap();
    let delta = w.open();
    commitments::verify(&delta).unwrap();
    assert_eq!(edges(&delta), cold);
}
