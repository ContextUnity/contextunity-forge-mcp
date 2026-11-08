use super::*;
pub(super) use rusqlite::{params, Connection};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct HtmlEdgeObservation {
    pub(super) owner_path: String,
    pub(super) source_path: String,
    pub(super) source_id: String,
    pub(super) source_name: String,
    pub(super) target_path: String,
    pub(super) target_id: String,
    pub(super) target_name: String,
    pub(super) target_kind: String,
    pub(super) kind: String,
    pub(super) line: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct HtmlNodeObservation {
    pub(super) path: String,
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) name: String,
    pub(super) details: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct HtmlCoverageObservation {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) expression: String,
    pub(super) status: String,
    pub(super) evidence: String,
}

pub(super) struct HtmlWorkspaceObservations {
    connection: Connection,
    validity: OnceLock<ValidityObservations>,
    edges: OnceLock<Vec<HtmlEdgeObservation>>,
    nodes: OnceLock<Vec<HtmlNodeObservation>>,
    coverage: OnceLock<Vec<HtmlCoverageObservation>>,
    persisted_facts: OnceLock<Vec<contextunity_forge_mcp::core::models::Facts>>,
}

impl HtmlWorkspaceObservations {
    pub(super) fn validity(&self) -> &ValidityObservations {
        self.validity
            .get_or_init(|| validity_observations(&self.connection))
    }

    pub(super) fn edges(&self) -> &[HtmlEdgeObservation] {
        self.edges
            .get_or_init(|| html_edge_observations(&self.connection))
    }

    pub(super) fn nodes(&self) -> &[HtmlNodeObservation] {
        self.nodes
            .get_or_init(|| html_node_observations(&self.connection))
    }

    pub(super) fn coverage(&self) -> &[HtmlCoverageObservation] {
        self.coverage
            .get_or_init(|| html_coverage_observations(&self.connection))
    }

    pub(super) fn persisted_facts(&self) -> &[contextunity_forge_mcp::core::models::Facts] {
        self.persisted_facts
            .get_or_init(|| persisted_facts(&self.connection))
    }
}

/// Owns disk-backed HTML relation fixtures and exposes persisted query results.
pub(super) struct HtmlWorkspaceHarness {
    workspace: PersistedWorkspaceHarness,
}

impl HtmlWorkspaceHarness {
    pub(super) fn new() -> Self {
        Self {
            workspace: PersistedWorkspaceHarness::new(),
        }
    }

    pub(super) fn write(&self, path: impl AsRef<Path>, source: &str) {
        self.workspace.write(path, source);
    }

    pub(super) fn build(&self) {
        self.workspace.build();
    }

    pub(super) fn delta<P: AsRef<Path>>(&self, paths: &[P]) -> serde_json::Value {
        self.workspace.delta(paths)
    }

    pub(super) fn root(&self) -> &Path {
        self.workspace.root()
    }

    pub(super) fn db(&self) -> PathBuf {
        self.workspace.db()
    }

    pub(super) fn path(&self, path: impl AsRef<Path>) -> PathBuf {
        self.workspace.path(path)
    }

    pub(super) fn read(&self, path: impl AsRef<Path>) -> std::io::Result<String> {
        std::fs::read_to_string(self.workspace.path(path))
    }

    pub(super) fn remove_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        self.workspace.try_remove(path)
    }

    pub(super) fn rename_file(
        &self,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
    ) -> std::io::Result<()> {
        std::fs::rename(self.workspace.path(from), self.workspace.path(to))
    }

    pub(super) fn observe(&self) -> HtmlWorkspaceObservations {
        HtmlWorkspaceObservations {
            connection: self.workspace.open(),
            validity: OnceLock::new(),
            edges: OnceLock::new(),
            nodes: OnceLock::new(),
            coverage: OnceLock::new(),
            persisted_facts: OnceLock::new(),
        }
    }

    pub(super) fn file_edge_observations(&self, path: &str) -> HtmlFileEdgeObservations {
        file_edge_observations(&self.workspace, path)
    }

    pub(super) fn assert_cold_parity(&self) {
        assert_complete_cold(&self.workspace);
    }
}

pub(super) fn html_edge_observations(connection: &Connection) -> Vec<HtmlEdgeObservation> {
    connection
        .prepare(
            "SELECT owner.path,source_path.path,source.id,source.name,target_path.path,target.id,target.name,target.kind,occurrence.kind,occurrence.line \
             FROM edge_occurrences occurrence \
             JOIN path_dictionary owner ON owner.path_id=occurrence.owner_id \
             JOIN nodes source ON source.node_hash=occurrence.src_hash \
             JOIN path_dictionary source_path ON source_path.path_id=source.path_id \
             JOIN nodes target ON target.node_hash=occurrence.dst_hash \
             JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
             ORDER BY owner.path,occurrence.line,source.name,target.name,occurrence.kind",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(HtmlEdgeObservation {
                owner_path: row.get(0)?,
                source_path: row.get(1)?,
                source_id: row.get(2)?,
                source_name: row.get(3)?,
                target_path: row.get(4)?,
                target_id: row.get(5)?,
                target_name: row.get(6)?,
                target_kind: row.get(7)?,
                kind: row.get(8)?,
                line: row.get(9)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn html_node_observations(connection: &Connection) -> Vec<HtmlNodeObservation> {
    connection
        .prepare(
            "SELECT path.path,node.id,node.kind,node.name,node.details \
             FROM nodes node JOIN path_dictionary path ON path.path_id=node.path_id \
             ORDER BY path.path,node.kind,node.name,node.id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(HtmlNodeObservation {
                path: row.get(0)?,
                id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                details: row.get(4)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn html_coverage_observations(connection: &Connection) -> Vec<HtmlCoverageObservation> {
    connection
        .prepare(
            "SELECT path.path,coverage.line,expression.expression,coverage.status,evidence.evidence \
             FROM resolution_coverage coverage \
             JOIN path_dictionary path ON path.path_id=coverage.path_id \
             JOIN coverage_expressions expression ON expression.expression_id=coverage.expression_id \
             JOIN coverage_evidence evidence ON evidence.evidence_id=coverage.evidence_id \
             ORDER BY path.path,coverage.line,expression.expression,coverage.status,evidence.evidence",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(HtmlCoverageObservation {
                path: row.get(0)?,
                line: row.get(1)?,
                expression: row.get(2)?,
                status: row.get(3)?,
                evidence: row.get(4)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CompleteRows {
    relations: crate::languages::support::RelationRows,
    nodes: Vec<String>,
    errors: Vec<String>,
    occurrences: Vec<String>,
    aggregated_edges: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ValidityObservations {
    pub(super) errors: Vec<(String, usize, String)>,
    pub(super) commitment_verification: Result<(), String>,
}

impl ValidityObservations {
    pub(super) fn assert_valid(&self) {
        assert!(
            self.errors.is_empty() && self.commitment_verification.is_ok(),
            "{self:?}"
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct HtmlFileEdgeObservations {
    pub(super) validity: ValidityObservations,
    pub(super) page_ids: Vec<String>,
    pub(super) edges: Vec<(String, String, usize)>,
}

pub(super) fn complete_cold(
    workspace: &PersistedWorkspaceHarness,
) -> crate::languages::support::ColdObservations<CompleteRows> {
    workspace.cold_observations(|connection| {
        let rows = |sql| crate::languages::harness::query_strings(connection, sql, []);
        CompleteRows {
            relations: crate::languages::support::relation_rows(connection),
            nodes: rows("SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id"),
            errors: rows("SELECT json_array(path,line,message) FROM errors ORDER BY path,line,message"),
            occurrences: rows(
                "SELECT json_array(s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence) FROM edge_occurrences o JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id JOIN coverage_evidence e ON e.evidence_id=o.evidence_id JOIN coverage_evidence c ON c.evidence_id=o.confidence_id ORDER BY s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence",
            ),
            aggregated_edges: rows(
                "SELECT json_array(s.id,d.id,e.kind,e.occurrence_count) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash ORDER BY s.id,d.id,e.kind",
            ),
        }
    })
}

pub(super) fn assert_complete_cold(workspace: &PersistedWorkspaceHarness) {
    assert_cold_observations(complete_cold(workspace));
}

pub(super) fn assert_cold_observations<T: Debug + PartialEq>(
    parity: crate::languages::support::ColdObservations<T>,
) {
    assert_eq!(parity.incremental_verification, Ok(()));
    assert_eq!(parity.cold_verification, Ok(()));
    assert_eq!(parity.incremental, parity.cold);
}

pub(super) fn file_edge_observations(
    workspace: &PersistedWorkspaceHarness,
    path: &str,
) -> HtmlFileEdgeObservations {
    let conn = workspace.open();
    HtmlFileEdgeObservations {
        validity: validity_observations(&conn),
        page_ids: node_ids_observation(&conn, path, "page"),
        edges: edge_occurrence_rows(&conn, path, "renders_template"),
    }
}

pub(super) fn node_ids_observation(conn: &Connection, path: &str, name: &str) -> Vec<String> {
    let rows = conn
        .prepare("SELECT id FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND name=?2 ORDER BY id")
        .unwrap()
        .query_map(params![path, name], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows
}

pub(super) fn node_id_observation(
    conn: &Connection,
    path: &str,
    name: &str,
) -> Result<String, Vec<String>> {
    let rows = node_ids_observation(conn, path, name);
    match rows.as_slice() {
        [id] => Ok(id.clone()),
        _ => Err(rows.clone()),
    }
}

pub(super) fn edge_occurrence_rows(
    conn: &Connection,
    source_path: &str,
    kind: &str,
) -> Vec<(String, String, usize)> {
    conn.prepare("SELECT s.id,d.id,o.line FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash WHERE p.path=?1 AND o.kind=?2 ORDER BY o.line")
        .unwrap()
        .query_map(params![source_path, kind], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn persisted_facts(
    conn: &Connection,
) -> Vec<contextunity_forge_mcp::core::models::Facts> {
    let blobs: Vec<Vec<u8>> = conn
        .prepare("SELECT facts_blob FROM local_facts ORDER BY path")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    blobs
        .iter()
        .map(|blob| contextunity_forge_mcp::db::reader::decode_local_facts(blob).unwrap())
        .collect()
}

pub(super) fn named_route_ids(conn: &Connection, name: &str) -> Vec<String> {
    conn.prepare("SELECT id FROM nodes WHERE kind='route' AND json_extract(details,'$.route_name')=?1 ORDER BY id")
        .unwrap()
        .query_map([name], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn validity_observations(conn: &Connection) -> ValidityObservations {
    let errors: Vec<(String, usize, String)> = conn
        .prepare("SELECT path,line,message FROM errors ORDER BY path,line,message")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    ValidityObservations {
        errors,
        commitment_verification: commitments::verify(conn)
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}
