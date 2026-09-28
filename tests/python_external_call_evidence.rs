#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer},
};
use rusqlite::Connection;
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
        let root = std::env::temp_dir().join(format!(
            "forge_external_calls_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) {
        writer::build(&self.0, &self.db(), None).unwrap();
    }
    fn delta(&self, path: &str) {
        writer::delta(&self.0, &self.db(), &[PathBuf::from(path)]).unwrap();
    }
    fn open(&self) -> Connection {
        reader::open(&self.db(), &self.0).unwrap()
    }

    fn assert_cold_equivalent(&self) {
        let incremental = self.open();
        commitments::verify(&incremental).unwrap();
        let cold_path = self.0.join(".forge/cold.sqlite");
        writer::build(&self.0, &cold_path, None).unwrap();
        let cold = reader::open(&cold_path, &self.0).unwrap();
        commitments::verify(&cold).unwrap();
        for sql in [
            "SELECT path||'|'||line||'|'||expression||'|'||status||'|'||evidence FROM resolution_coverage ORDER BY path,line,expression,status,evidence",
            "SELECT src_public_id||'|'||dst_public_id||'|'||kind FROM edges ORDER BY src_public_id,dst_public_id,kind",
        ] {
            assert_eq!(rows(&incremental, sql), rows(&cold, sql), "{sql}");
        }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn coverage(conn: &Connection, line: i64, expression: &str) -> (String, String) {
    conn.query_row(
        "SELECT status,evidence FROM resolution_coverage WHERE path='consumer.py' AND line=?1 AND expression=?2",
        (line, expression),
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap()
}

fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn known_external_alias_explains_call_without_claiming_resolution() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "import requests as http\ndef run(): return http.get('https://example.test')\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 1, "requests").0, "external");
    let (status, evidence) = coverage(&conn, 2, "http.get");
    assert_eq!(status, "unresolved");
    assert_eq!(
        evidence,
        "call through external import requests; callable target unverified"
    );
    let edges: i64 = conn
        .query_row("SELECT count(*) FROM edges WHERE kind='calls'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(edges, 0);
}

#[test]
fn unknown_import_and_shadowed_alias_keep_generic_evidence() {
    let w = Workspace::new();
    w.write("consumer.py", "import unknown_package as missing\nimport requests as http\ndef unknown(): return missing.get()\ndef shadowed(http): return http.get()\n");
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 3, "missing.get").0, "unresolved");
    assert!(!coverage(&conn, 3, "missing.get")
        .1
        .starts_with("call through external import "));
    assert_eq!(coverage(&conn, 4, "http.get").0, "unresolved");
    assert!(!coverage(&conn, 4, "http.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn duplicate_aliases_and_local_provider_do_not_gain_external_provenance() {
    let w = Workspace::new();
    w.write("provider.py", "def get(): return 1\n");
    w.write("consumer.py", "import requests as client\nimport httpx as client\nimport provider as local\ndef run(): return client.get(), local.get()\n");
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 4, "client.get").0, "unresolved");
    assert!(!coverage(&conn, 4, "client.get")
        .1
        .starts_with("call through external import "));
    assert_eq!(coverage(&conn, 4, "local.get").0, "resolved");
    assert!(!coverage(&conn, 4, "local.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn local_import_is_scoped_to_its_function() {
    let w = Workspace::new();
    w.write("consumer.py", "def first():\n    import requests as http\n    return http.get('https://example.test')\ndef second(): return http.get('https://example.test')\n");
    w.build();
    let conn = w.open();
    assert_eq!(
        coverage(&conn, 3, "http.get").1,
        "call through external import requests; callable target unverified"
    );
    assert!(!coverage(&conn, 4, "http.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn call_before_external_import_has_no_provenance() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "def run(): return http.get('https://example.test')\nimport requests as http\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 1, "http.get").0, "unresolved");
    assert!(!coverage(&conn, 1, "http.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn assignment_rebind_has_no_external_provenance() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "import json\njson = object()\ndef run(): return json.loads('{}')\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 3, "json.loads").0, "unresolved");
    assert!(!coverage(&conn, 3, "json.loads")
        .1
        .starts_with("call through external import "));
}

#[test]
fn changing_import_provenance_preserves_cold_delta_equivalence() {
    let w = Workspace::new();
    let external =
        "import requests as client\ndef run(): return client.get('https://example.test')\n";
    let unknown =
        "import unknown_package as client\ndef run(): return client.get('https://example.test')\n";
    w.write("consumer.py", external);
    w.build();
    w.write("consumer.py", unknown);
    w.delta("consumer.py");
    w.assert_cold_equivalent();
    assert!(!coverage(&w.open(), 2, "client.get")
        .1
        .starts_with("call through external import "));
    w.write("consumer.py", external);
    w.delta("consumer.py");
    w.assert_cold_equivalent();
    assert_eq!(
        coverage(&w.open(), 2, "client.get").1,
        "call through external import requests; callable target unverified"
    );
}
