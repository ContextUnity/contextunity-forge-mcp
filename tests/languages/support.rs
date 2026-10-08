use crate::common::Workspace as SharedWorkspace;
use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer},
};
use rusqlite::Connection;
use std::{
    ops::Deref,
    path::{Path, PathBuf},
};

/// Language suites use one shared fixture lifecycle while retaining the old
/// path-oriented call shape used by their assertions.
pub(crate) struct Workspace(pub(crate) PathBuf, SharedWorkspace);

impl Workspace {
    pub(crate) fn new() -> Self {
        let shared = SharedWorkspace::new();
        Self(shared.root().to_path_buf(), shared)
    }

    pub(crate) fn write(&self, relative: impl AsRef<Path>, content: &str) {
        self.1.write(relative, content);
    }

    pub(crate) fn db(&self) -> PathBuf {
        self.1.db()
    }

    pub(crate) fn build(&self) -> Connection {
        self.1.build();
        self.1.open()
    }

    pub(crate) fn delta(&self, path: &str) {
        self.1.delta(&[path]);
    }

    pub(crate) fn open(&self) -> Connection {
        self.1.open()
    }

    pub(crate) fn assert_cold_equivalent(&self) {
        let incremental = self.open();
        commitments::verify(&incremental).unwrap();
        let cold_path = self.0.join(".forge/cold.sqlite");
        writer::build(self.1.root(), &cold_path, None).unwrap();
        let cold = reader::open(&cold_path, self.1.root()).unwrap();
        commitments::verify(&cold).unwrap();
        for sql in [
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind,(SELECT path FROM path_dictionary WHERE path_id=edges.path_id),line",
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        ] {
            assert_eq!(rows(&incremental, sql), rows(&cold, sql), "{sql}");
        }
    }

    pub(crate) fn cold_parity(&self, path: &str) {
        self.delta(path);
        let incremental = self.open();
        commitments::verify(&incremental).unwrap();
        let cold_path = self.0.join(".forge/cold.sqlite");
        writer::build(self.1.root(), &cold_path, None).unwrap();
        let cold = reader::open(&cold_path, self.1.root()).unwrap();
        commitments::verify(&cold).unwrap();
        for sql in [
            "SELECT id||'|'||kind||'|'||qualname||'|'||line||'|'||end_line||'|'||details FROM nodes ORDER BY id",
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
            "SELECT path||'|'||line||'|'||message FROM errors ORDER BY path,line,message",
        ] {
            assert_eq!(rows(&incremental, sql), rows(&cold, sql), "{sql}");
        }
    }
}

impl Deref for Workspace {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.0.as_path()
    }
}

impl AsRef<Path> for Workspace {
    fn as_ref(&self) -> &Path {
        self.0.as_path()
    }
}

fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}
