use super::*;
use rusqlite::Connection;

fn status(conn: &Connection, expression: &str) -> String {
    query_scalar(
        conn,
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1 AND line=1",
        [expression],
    )
}

fn call_targets(conn: &Connection) -> Vec<String> {
    query_strings(
        conn,
        "SELECT dst.qualname FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer.consume' AND e.kind='calls' ORDER BY dst.qualname",
        [],
    )
}

fn call_target_paths(conn: &Connection) -> Vec<String> {
    query_strings(
        conn,
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer.consume' AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)",
        [],
    )
}

fn import_targets(conn: &Connection) -> Vec<String> {
    query_strings(
        conn,
        "SELECT dst.qualname FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY dst.qualname",
        [],
    )
}

fn import_target_paths(conn: &Connection) -> Vec<String> {
    query_strings(
        conn,
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)",
        [],
    )
}

mod module_resolution;
mod package_exports;
mod stubs;
mod type_checking;
