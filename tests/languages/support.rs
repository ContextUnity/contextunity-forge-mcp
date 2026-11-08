use crate::common::Workspace;
use contextunity_forge_mcp::{core::commitments, db::reader, engine::pipeline};
use rusqlite::Connection;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

pub(crate) fn fresh_cold(workspace: &Workspace) -> Connection {
    let database = fresh_cold_database(workspace);
    let conn = reader::open(&database, workspace.root()).unwrap();
    let _ = conn.execute_batch("COMMIT");
    let _ = std::fs::remove_file(&database);
    let _ = std::fs::remove_file(format!("{}-wal", database.display()));
    let _ = std::fs::remove_file(format!("{}-shm", database.display()));
    let _ = std::fs::remove_file(format!("{}.lock", database.display()));
    let _ = std::fs::remove_file(format!("{}.verified.json", database.display()));
    conn
}

pub(crate) fn fresh_cold_database(workspace: &Workspace) -> PathBuf {
    static NEXT_COLD: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT_COLD.fetch_add(1, Ordering::Relaxed);
    let database = workspace.path(format!(".forge/cold-{sequence}.sqlite"));
    let cache = workspace.path(".forge/cold-cache");
    assert!(!database.exists(), "cold database must be new");
    pipeline::build_with_cache_dir(workspace.root(), &database, None, &cache).unwrap();
    database
}

pub(crate) struct ColdObservations<T> {
    pub incremental: T,
    pub cold: T,
    pub incremental_verification: Result<(), String>,
    pub cold_verification: Result<(), String>,
    pub incremental_connection: Connection,
    pub cold_connection: Connection,
}

impl<T> ColdObservations<T> {
    pub(crate) fn from_connections(
        incremental: Connection,
        cold: Connection,
        observe: impl Fn(&Connection) -> T,
    ) -> Self {
        Self::from_connections_with_cached_cold_verification(incremental, cold, observe, None)
    }

    pub(crate) fn from_connections_with_cold_verification(
        incremental: Connection,
        cold: Connection,
        observe: impl Fn(&Connection) -> T,
        cold_verification: Result<(), String>,
    ) -> Self {
        Self::from_connections_with_cached_cold_verification(
            incremental,
            cold,
            observe,
            Some(cold_verification),
        )
    }

    fn from_connections_with_cached_cold_verification(
        incremental: Connection,
        cold: Connection,
        observe: impl Fn(&Connection) -> T,
        cold_verification: Option<Result<(), String>>,
    ) -> Self {
        let incremental_observation = observe(&incremental);
        let cold_observation = observe(&cold);
        let incremental_verification = commitments::verify(&incremental)
            .map(|_| ())
            .map_err(|error| error.to_string());
        let cold_verification = cold_verification.unwrap_or_else(|| {
            commitments::verify(&cold)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        let _ = incremental.execute_batch("COMMIT");
        let _ = cold.execute_batch("COMMIT");
        Self {
            incremental: incremental_observation,
            cold: cold_observation,
            incremental_verification,
            cold_verification,
            incremental_connection: incremental,
            cold_connection: cold,
        }
    }
}

#[derive(Default)]
pub(crate) struct ColdFilesystemReferenceCache {
    references: Mutex<Vec<CachedColdReference>>,
}

impl Drop for ColdFilesystemReferenceCache {
    fn drop(&mut self) {
        let references = self
            .references
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for reference in references {
            let _ = std::fs::remove_file(&reference.database);
            let _ = std::fs::remove_file(format!("{}-wal", reference.database.display()));
            let _ = std::fs::remove_file(format!("{}-shm", reference.database.display()));
            let _ = std::fs::remove_file(format!(
                "{}.lock",
                reference.database.display()
            ));
            let _ = std::fs::remove_file(format!(
                "{}.verified.json",
                reference.database.display()
            ));
        }
    }
}

struct CachedColdReference {
    inputs: crate::common::workspace::WorkspaceInputSnapshot,
    database: PathBuf,
    verification: Result<(), String>,
}

impl ColdFilesystemReferenceCache {
    pub(crate) fn observe<T>(
        &self,
        workspace: &Workspace,
        incremental: Connection,
        observe: impl Fn(&Connection) -> T,
    ) -> ColdObservations<T> {
        let inputs = workspace.input_snapshot("filesystem-production-v1");
        let cached = self
            .references
            .lock()
            .unwrap()
            .iter()
            .find(|reference| reference.inputs == inputs)
            .map(|reference| (reference.database.clone(), reference.verification.clone()));
        let (cold_verification, cold) = if let Some((database, verification)) = cached {
            let cold = reader::open(&database, workspace.root()).unwrap();
            (verification, cold)
        } else {
            let database = fresh_cold_database(workspace);
            let cold = reader::open(&database, workspace.root()).unwrap();
            let verification = commitments::verify(&cold)
                .map(|_| ())
                .map_err(|error| error.to_string());
            self.references.lock().unwrap().push(CachedColdReference {
                inputs,
                database: database.clone(),
                verification: verification.clone(),
            });
            (verification, cold)
        };
        ColdObservations::from_connections_with_cold_verification(
            incremental,
            cold,
            observe,
            cold_verification,
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RelationRows {
    pub edge_rows: Vec<String>,
    pub coverage_rows: Vec<String>,
}

pub(crate) fn relation_rows(connection: &Connection) -> RelationRows {
    RelationRows {
        edge_rows: rows(
            connection,
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind,(SELECT path FROM path_dictionary WHERE path_id=edges.path_id),line",
        ),
        coverage_rows: rows(
            connection,
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        ),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DeltaRows {
    pub node_rows: Vec<String>,
    pub edge_rows: Vec<String>,
    pub error_rows: Vec<String>,
}

pub(crate) fn delta_rows(connection: &Connection) -> DeltaRows {
    DeltaRows {
        node_rows: rows(
            connection,
            "SELECT id||'|'||kind||'|'||qualname||'|'||line||'|'||end_line||'|'||details FROM nodes ORDER BY id",
        ),
        edge_rows: rows(
            connection,
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
        ),
        error_rows: rows(
            connection,
            "SELECT path||'|'||line||'|'||message FROM errors ORDER BY path,line,message",
        ),
    }
}

pub(crate) fn delta_cold(workspace: &Workspace, path: &str) -> ColdObservations<DeltaRows> {
    workspace.delta(&[path]);
    ColdObservations::from_connections(workspace.open(), fresh_cold(workspace), delta_rows)
}

fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    crate::common::sqlite::query_strings(conn, sql, [])
}
