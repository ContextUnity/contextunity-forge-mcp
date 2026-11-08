use super::*;
use serde_json::json;

pub(super) use crate::support::{ColdFilesystemReferenceCache, ColdObservations};

pub(super) struct CallableRows {
    pub coverage: Vec<String>,
    pub occurrences: Vec<String>,
}

pub(super) fn callable_rows(conn: &Connection) -> CallableRows {
    CallableRows {
        coverage: query_rows(
            conn,
            "SELECT json_array(p.path,c.line,x.expression,c.status) FROM resolution_coverage c JOIN path_dictionary p USING(path_id) JOIN coverage_expressions x USING(expression_id) ORDER BY p.path,c.line,x.expression,c.status",
        ),
        occurrences: query_rows(
            conn,
            "SELECT json_array(p.path,e.line,e.kind,s.id,d.id) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash ORDER BY p.path,e.line,e.kind,s.id,d.id",
        ),
    }
}

pub(super) fn provider_edge_count(conn: &Connection, owner: &str, provider_pattern: &str) -> usize {
    query_count_usize(
        conn,
        "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE p.path=?1 AND dp.path LIKE ?2",
        params![owner, provider_pattern],
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PersistedRows {
    pub relations: crate::support::RelationRows,
    pub node_rows: Vec<String>,
    pub dependency_rows: Vec<String>,
    pub commitment_rows: Vec<String>,
}

pub(super) fn persisted_rows(conn: &Connection) -> PersistedRows {
    PersistedRows {
        relations: crate::support::relation_rows(conn),
        node_rows: query_rows(
            conn,
            "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
        ),
        dependency_rows: query_rows(
            conn,
            "SELECT json_array(p.path,d.ordinal,f.path,d.kind,d.symbol,d.resolution) FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id LEFT JOIN files f ON f.path_hash=d.target_hash ORDER BY p.path,d.ordinal",
        ),
        commitment_rows: query_rows(
            conn,
            "SELECT json_array(d.domain,CASE WHEN d.owner_id=0 THEN NULL ELSE p.path END,hex(d.digest)) FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id ORDER BY d.domain,CASE WHEN d.owner_id=0 THEN NULL ELSE p.path END",
        ),
    }
}

pub(super) fn delta_cold_observations(workspace: &Workspace) -> ColdObservations<PersistedRows> {
    ColdObservations::from_connections(
        workspace.open(),
        crate::support::fresh_cold(workspace),
        persisted_rows,
    )
}

pub(super) fn semantic(id: &str, operation: Value) -> Value {
    json!({"id":id,"namespace":"proof","scope":"project","activation":[],"operation":operation})
}

pub(super) fn syntax(id: &str, language: &str, operation: Value) -> Value {
    json!({"id":id,"language":language,"activation":[],"operation":operation})
}

pub(super) fn pack(semantic_rules: Vec<Value>, syntax_rules: Vec<Value>) -> Value {
    json!({"semantic_rules":semantic_rules,"syntax_rules":syntax_rules})
}

pub(super) fn server(member: &str) -> Value {
    json!({"kind":"constructed_member","module":"webkit","constructors":["Server"],"member":member})
}
