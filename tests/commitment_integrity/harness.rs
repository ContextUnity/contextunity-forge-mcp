pub(super) use contextunity_forge_mcp::core::commitments;
use contextunity_forge_mcp::core::schema::SCHEMA_DDL;
use rusqlite::params;
use rusqlite::Connection;

pub(super) fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA_DDL).unwrap();
    conn.execute_batch(
        "INSERT INTO coverage_evidence(evidence_id,evidence) VALUES(1,'call'),(2,'exact')",
    )
    .unwrap();
    conn
}

pub(super) fn owner_id(conn: &Connection, path: &str) -> i64 {
    conn.execute(
        "INSERT OR IGNORE INTO path_dictionary(path) VALUES(?1)",
        [path],
    )
    .unwrap();
    conn.query_row(
        "SELECT path_id FROM path_dictionary WHERE path=?1",
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

pub(super) fn search_nodes(conn: &Connection, count: i64) {
    let path_id = owner_id(conn, "src/fts.rs");
    for node_hash in 1..=count {
        let id = format!("fts:{node_hash}");
        conn.execute(
            "INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) \
             VALUES(?1,'function',?1,?1,?2,1,1,0,'rust',0,'{}',?3,?2)",
            params![id, path_id, node_hash],
        )
        .unwrap();
    }
}

pub(super) fn fts_postings(conn: &Connection, vocab: &str) -> Vec<(String, i64, String, i64)> {
    let mut statement = conn
        .prepare(&format!(
            "SELECT term,doc,col,offset FROM {vocab} ORDER BY term,doc,col,offset"
        ))
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

pub(super) fn coverage_leaf(conn: &Connection, owner: &str) -> Vec<u8> {
    conn.query_row(
        "SELECT digest FROM domain_commitments WHERE domain='resolution_coverage'
         AND owner_id=(SELECT path_id FROM path_dictionary WHERE path=?1)",
        [owner],
        |row| row.get(0),
    )
    .unwrap()
}

#[derive(Clone, Copy)]
pub(super) enum ExpressionOrder {
    Ascending,
    Descending,
}
