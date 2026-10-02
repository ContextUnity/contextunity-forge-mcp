use anyhow::Result;
use contextunity_forge_mcp::{
    core::{commitments, models::stable_hash64, schema::SCHEMA_DDL},
    db::{reader, writer},
};
use rusqlite::{params, Connection};

#[test]
fn canonical_graph_tables_store_numeric_keys_and_first_class_owners() -> Result<()> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_DDL)?;
    for table in [
        "nodes",
        "edges",
        "edge_occurrences",
        "dependencies",
        "shared_keys",
        "shared_owners",
        "resolution_coverage",
        "domain_commitments",
    ] {
        let kind: String = conn.query_row(
            "SELECT type FROM sqlite_master WHERE name=?1",
            [table],
            |row| row.get(0),
        )?;
        assert_eq!(kind, "table", "{table} is a canonical table");
    }
    let virtual_shims: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type IN('view','trigger')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(virtual_shims, 0);

    for path in ["src/a.rs", "src/b.rs", "src"] {
        conn.execute("INSERT INTO path_dictionary(path) VALUES(?1)", [path])?;
    }
    conn.execute_batch(
        "INSERT INTO coverage_evidence(evidence_id,evidence) VALUES(1,'call'),(2,'exact');
         INSERT INTO coverage_expressions(expression_id,expression) VALUES(1,'fn:b');",
    )?;
    for (id, path) in [("fn:a", "src/a.rs"), ("fn:b", "src/b.rs")] {
        conn.execute(
            "INSERT INTO nodes(id,kind,name,qualname,path_id,path,line,end_line,is_test,language,generated,details,node_hash,owner_path_id)
             VALUES(?1,'function',?1,?1,(SELECT path_id FROM path_dictionary WHERE path=?2),?2,1,1,0,'rust',0,'{}',?3,
             (SELECT path_id FROM path_dictionary WHERE path=?2))",
            params![id, path, stable_hash64(id)],
        )?;
    }
    conn.execute(
        "INSERT INTO nodes(id,kind,name,qualname,path_id,path,line,end_line,is_test,language,generated,details,node_hash,owner_path_id)
         VALUES('component:src','component','src','src',(SELECT path_id FROM path_dictionary WHERE path='src'),'src',1,1,0,'workspace',0,'{}',?1,
         (SELECT path_id FROM path_dictionary WHERE path='src/a.rs'))",
        [stable_hash64("component:src")],
    )?;
    conn.execute(
        "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count)
         VALUES(?1,'calls',?2,(SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),1,1,2,1)
         ON CONFLICT(src_hash,kind,dst_hash) DO UPDATE SET occurrence_count=edges.occurrence_count+excluded.occurrence_count",
        params![stable_hash64("fn:a"), stable_hash64("fn:b")],
    )?;
    conn.execute(
        "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count)
         VALUES(?1,'calls',?2,(SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),1,1,2,1)
         ON CONFLICT(src_hash,kind,dst_hash) DO UPDATE SET occurrence_count=edges.occurrence_count+excluded.occurrence_count",
        params![stable_hash64("fn:a"), stable_hash64("fn:b")],
    )?;
    let edge: (String, String, i64) = conn.query_row(
        "SELECT typeof(src_hash),typeof(evidence_id),occurrence_count FROM edges",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(edge, ("integer".into(), "integer".into(), 2));

    conn.execute(
        "INSERT INTO edge_occurrences VALUES((SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),0,?1,?2,'calls',1,1,2)",
        params![stable_hash64("fn:a"), stable_hash64("fn:b")],
    )?;
    conn.execute(
        "INSERT INTO dependencies VALUES((SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),0,?1,'calls','fn:b','resolved')",
        [stable_hash64("src/b.rs")],
    )?;
    conn.execute(
        "INSERT INTO shared_keys VALUES(?1,'fn:b')",
        [stable_hash64("fn:b")],
    )?;
    conn.execute(
        "INSERT INTO shared_owners VALUES(1,?1,(SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),0)",
        [stable_hash64("fn:b")],
    )?;
    conn.execute(
        "INSERT INTO resolution_coverage VALUES((SELECT path_id FROM path_dictionary WHERE path='src/a.rs'),1,1,'resolved',1)",
        [],
    )?;
    let owner: String = conn.query_row(
        "SELECT p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id WHERE n.id='component:src'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(owner, "src/a.rs");
    let nodes = reader::rows(&conn, "SELECT * FROM nodes ORDER BY id", &[], 3)?;
    assert_eq!(nodes.len(), 3);
    assert!(nodes.iter().all(|node| {
        let object = node.as_object().unwrap();
        object.contains_key("path")
            && !object.contains_key("node_hash")
            && !object.contains_key("owner_path_id")
    }));
    commitments::seal(&conn)?;
    commitments::verify(&conn)?;
    Ok(())
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_build_populates_node_owner_paths() -> Result<()> {
    struct Workspace(std::path::PathBuf);
    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let workspace = Workspace(
        std::env::temp_dir().join(format!("forge_schema_{}_{nonce}", std::process::id())),
    );
    let source = workspace.0.join("source");
    std::fs::create_dir_all(&source)?;
    std::fs::write(source.join("provider.rs"), "pub fn exported() {}\n")?;
    let db = workspace.0.join("index.sqlite");
    writer::build(&source, &db, None)?;
    let conn = Connection::open(&db)?;
    let owned: i64 = conn.query_row(
        "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id
         WHERE p.path='provider.rs' AND n.kind='function'",
        [],
        |row| row.get(0),
    )?;
    assert!(owned > 0);
    commitments::verify(&conn)?;
    Ok(())
}
