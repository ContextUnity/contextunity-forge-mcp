use super::*;
pub(super) use rusqlite::{params, Connection};

pub(super) fn complete_parity(workspace: &Workspace) {
    workspace.assert_cold_equivalent();
    let incremental = workspace.open();
    let cold = reader::open(&workspace.0.join(".forge/cold.sqlite"), &workspace.0).unwrap();
    for sql in [
        "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
        "SELECT json_array(path,line,message) FROM errors ORDER BY path,line,message",
        "SELECT json_array(s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence) FROM edge_occurrences o JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN path_dictionary p ON p.path_id=o.owner_id JOIN coverage_evidence e ON e.evidence_id=o.evidence_id JOIN coverage_evidence c ON c.evidence_id=o.confidence_id ORDER BY s.id,d.id,o.kind,p.path,o.line,e.evidence,c.evidence",
        "SELECT json_array(s.id,d.id,e.kind,e.occurrence_count) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash ORDER BY s.id,d.id,e.kind",
    ] {
        let rows = |conn: &Connection| conn.prepare(sql).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(rows(&incremental), rows(&cold), "{sql}");
    }
}

pub(super) fn file_edges(workspace: &Workspace, path: &str, target: Option<(&str, usize)>) {
    let conn = workspace.open();
    assert_valid(&conn);
    let expected = target
        .map(|(target, line)| {
            (
                node_id(&conn, path, "page"),
                format!("module:{target}"),
                line,
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        edge_occurrence_rows(&conn, path, "renders_template"),
        expected,
        "{path}"
    );
}

pub(super) fn node_id(conn: &Connection, path: &str, name: &str) -> String {
    let rows = conn
        .prepare("SELECT id FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND name=?2 ORDER BY id")
        .unwrap()
        .query_map(params![path, name], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows.len(), 1, "unique definition {path}:{name}: {rows:?}");
    rows[0].clone()
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
    conn.prepare(
        "SELECT id FROM nodes WHERE kind='route' AND json_extract(details,'$.route_name')=?1 ORDER BY id",
    )
    .unwrap()
    .query_map([name], |row| row.get(0))
    .unwrap()
    .collect::<rusqlite::Result<Vec<_>>>()
    .unwrap()
}

pub(super) fn named_route_coverage(
    conn: &Connection,
    path: &str,
    line: usize,
    name: &str,
) -> String {
    let expression = format!("django.route_name.{name}");
    conn.query_row(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        params![path, line, expression],
        |row| row.get(0),
    )
    .unwrap_or_else(|error| {
        panic!("missing named-route coverage {path}:{line} {expression}: {error}")
    })
}

pub(super) fn assert_valid(conn: &Connection) {
    let errors: Vec<(String, usize, String)> = conn
        .prepare("SELECT path,line,message FROM errors ORDER BY path,line,message")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(errors.is_empty(), "invalid fixture: {errors:?}");
    commitments::verify(conn).unwrap();
}

pub(super) fn assert_source_valid(path: &str, language: &str, source: &str, names: &[&str]) {
    let facts = ast::extract(path, language, source).unwrap();
    assert!(
        facts.errors.is_empty(),
        "{path} must parse: {:?}",
        facts.errors
    );
    for name in names {
        assert_eq!(
            facts.nodes.iter().filter(|node| node.name == *name).count(),
            1,
            "{path} expected ordinary producer/consumer definition {name}: {:?}",
            facts.nodes
        );
    }
}
