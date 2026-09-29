use anyhow::Result;
use contextunity_forge_mcp::{
    core::{models::stable_hash64, schema::SCHEMA_DDL},
    db::reader,
};
use rusqlite::{params, Connection};

#[test]
fn compact_graph_tables_keep_text_views_and_hide_storage_hashes() -> Result<()> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_DDL)?;

    for path in ["src/a.rs", "src/b.rs"] {
        conn.execute(
            "INSERT INTO files VALUES(?1,'indexed','digest',1,'rust',0,0,?2)",
            params![path, stable_hash64(path)],
        )?;
    }
    for (id, path) in [("fn:a", "src/a.rs"), ("fn:b", "src/b.rs")] {
        conn.execute(
            "INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details,node_hash) VALUES(?1,'function',?1,?1,?2,1,1,0,'rust',0,'{}',?3)",
            params![id, path, stable_hash64(id)],
        )?;
    }
    conn.execute(
        "INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details,node_hash) VALUES('component:src','component','src','src','src',1,1,0,'workspace',0,'{}',?1)",
        [stable_hash64("component:src")],
    )?;
    conn.execute(
        "INSERT INTO node_owner_overrides VALUES('component:src','src/a.rs')",
        [],
    )?;

    for _ in 0..2 {
        conn.execute(
            "INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) VALUES('fn:a','fn:b','calls','src/a.rs',1,'call','exact',1)",
            [],
        )?;
    }
    let edge: (String, String, String, i64) = conn.query_row(
        "SELECT src_public_id,dst_public_id,kind,occurrence_count FROM edges",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(edge, ("fn:a".into(), "fn:b".into(), "calls".into(), 2));
    let raw_types: (String, String) = conn.query_row(
        "SELECT typeof(src_hash),typeof(dst_hash) FROM edges_raw",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(raw_types, ("integer".into(), "integer".into()));

    conn.execute(
        "INSERT INTO edge_occurrences_raw VALUES((SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),0,?1,?2,'calls',1,'call','exact')",
        params![stable_hash64("fn:a"), stable_hash64("fn:b")],
    )?;
    let occurrence: (String, String, String, String) = conn.query_row(
        "SELECT owner,path,src,dst FROM edge_occurrences",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(
        occurrence,
        (
            "src/a.rs".into(),
            "src/a.rs".into(),
            "fn:a".into(),
            "fn:b".into()
        )
    );

    conn.execute(
        "INSERT INTO dependencies_raw(owner_id,ordinal,target_hash,kind,symbol,resolution) VALUES((SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),0,?1,'calls','fn:b','resolved')",
        [stable_hash64("src/b.rs")],
    )?;
    let target: String = conn.query_row(
        "SELECT target FROM dependencies WHERE owner='src/a.rs'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(target, "src/b.rs");

    conn.execute(
        "INSERT INTO owned_search_raw VALUES((SELECT node_id FROM nodes WHERE id='fn:a'),'function fn:a src/a.rs')",
        [],
    )?;
    let search_owner: String = conn.query_row(
        "SELECT owner FROM owned_search WHERE public_id='fn:a'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(search_owner, "src/a.rs");
    conn.execute(
        "INSERT INTO shared_keys(key_hash,key) VALUES(?1,'fn:b')",
        [stable_hash64("fn:b")],
    )?;
    conn.execute(
        "INSERT INTO shared_owners_raw(kind_id,key_hash,owner_id,ordinal) VALUES(1,?1,(SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),0)",
        [stable_hash64("fn:b")],
    )?;
    let shared_owner: String = conn.query_row(
        "SELECT owner FROM shared_owners WHERE kind='reference' AND key='fn:b'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(shared_owner, "src/a.rs");
    conn.execute(
        "INSERT INTO resolution_coverage(path,line,expression,status,evidence) VALUES('src/a.rs',1,'fn:b','resolved','fixture')",
        [],
    )?;
    let coverage_path: String = conn.query_row(
        "SELECT path FROM resolution_coverage WHERE expression='fn:b'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(coverage_path, "src/a.rs");

    for view in [
        "edges",
        "edge_occurrences",
        "dependencies",
        "owned_nodes",
        "owned_search",
        "shared_owners",
        "resolution_coverage",
    ] {
        let kind: String = conn.query_row(
            "SELECT type FROM sqlite_master WHERE name=?1",
            [view],
            |row| row.get(0),
        )?;
        assert_eq!(kind, "view", "{view} remains a text-facing view");
    }
    let owned: String = conn.query_row(
        "SELECT details_json FROM owned_nodes WHERE owner='src/a.rs'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(owned, "{}");
    let component_owner: String = conn.query_row(
        "SELECT owner FROM owned_nodes WHERE public_id='component:src'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(component_owner, "src/a.rs");
    let nodes = reader::rows(&conn, "SELECT * FROM nodes ORDER BY id", &[], 2)?;
    assert_eq!(nodes.len(), 2);
    assert!(nodes
        .iter()
        .all(|node| !node.as_object().unwrap().contains_key("node_hash")));

    let removed_indexes: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN('idx_edges_path','idx_coverage_evidence','idx_owned_nodes_public','idx_dependencies_owner')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(removed_indexes, 0);
    Ok(())
}
