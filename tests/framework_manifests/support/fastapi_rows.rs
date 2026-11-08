use super::*;

pub(in super::super) fn edges(conn: &Connection) -> Vec<(String, String, usize)> {
    conn
        .prepare("SELECT s.id,d.id,o.line FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash WHERE p.path=?1 AND o.kind=?2 ORDER BY o.line")
        .unwrap().query_map(["app/views.py", "injects_dependency"], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
}

pub(in super::super) fn coverage(conn: &Connection) -> Vec<(String, String)> {
    conn
        .prepare("SELECT x.expression,c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path=?1 AND e.evidence=?2 ORDER BY x.expression")
        .unwrap().query_map(["app/views.py", "declarative imported symbol; exact import and unique callable target required"], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

pub(in super::super) fn rebound_coverage(conn: &Connection) -> Vec<(String, String)> {
    conn
        .prepare("SELECT p.path,c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path LIKE 'app/rebound_%.py' AND x.expression='get_db' AND e.evidence=?1 ORDER BY p.path")
        .unwrap().query_map(["declarative imported symbol; exact import and unique callable target required"], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

pub(in super::super) fn unactivated_dependency_count(conn: &Connection) -> i64 {
    query_count(conn, "SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path=?1 AND o.kind=?2", ["unactivated/views.py", "injects_dependency"])
}

pub(in super::super) fn rebound_dependency_count(conn: &Connection) -> i64 {
    query_count(conn, "SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path LIKE 'app/rebound_%.py' AND o.kind=?1", ["injects_dependency"])
}
