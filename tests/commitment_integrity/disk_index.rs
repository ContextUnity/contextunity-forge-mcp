use crate::common::Workspace;
use super::harness::*;
use contextunity_forge_mcp::{
    core::schema::SCHEMA_DDL,
    db::{reader, writer},
};
use rusqlite::Connection;

#[test]
fn unchanged_input_cold_builds_have_the_same_root_after_reindex_and_vacuum() {
    let workspace = Workspace::new();
    let source = workspace.path("source");
    workspace.write(
        "source/src/lib.rs",
        "pub fn stable_build_fixture() -> &'static str { \"unchanged\" }\n",
    );
    let first_db = workspace.path("first.sqlite");
    let first_report = writer::build(&source, &first_db, None).unwrap();
    let first_root = first_report["output_root"].as_str().unwrap().to_owned();
    let first_conn = Connection::open(&first_db).unwrap();
    first_conn
        .execute_batch("REINDEX; ANALYZE; VACUUM;")
        .unwrap();
    let stored_root_after_maintenance: String = first_conn
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stored_root_after_maintenance, first_root,
        "valid index maintenance must preserve the stored Merkle root"
    );
    commitments::verify(&first_conn).expect("the maintained database still verifies");
    drop(first_conn);

    let second_db = workspace.path("second.sqlite");
    let second_report = writer::build(&source, &second_db, None).unwrap();
    let second_root = second_report["output_root"].as_str().unwrap();
    assert_eq!(
        second_root, first_root,
        "identical unchanged source must produce the same cold-build Merkle root"
    );
    commitments::verify(&Connection::open(&second_db).unwrap())
        .expect("the second cold build verifies");
}

#[cfg(feature = "lang-rust")]
#[test]
fn built_nodes_keep_owner_paths_and_repeatable_merkle_leaves() {
    #[derive(Clone, Copy, Debug)]
    enum EdgeMutation {
        DeleteFirstEdge,
        ChangeFirstEdgeLine,
    }

    #[derive(Clone, Copy, Debug)]
    enum SchemaMutation {
        UnsupportedSchemaVersion,
        LegacyIndexSemantics,
    }

    let edge_mutations = [
        EdgeMutation::DeleteFirstEdge,
        EdgeMutation::ChangeFirstEdgeLine,
    ];
    let schema_mutations = [
        SchemaMutation::UnsupportedSchemaVersion,
        SchemaMutation::LegacyIndexSemantics,
    ];
    let edge_sql = |mutation| match mutation {
        EdgeMutation::DeleteFirstEdge => {
            "DELETE FROM edges WHERE (src_hash,kind,dst_hash)=(SELECT src_hash,kind,dst_hash FROM edges LIMIT 1)"
        }
        EdgeMutation::ChangeFirstEdgeLine => {
            "UPDATE edges SET line=line+1 WHERE (src_hash,kind,dst_hash)=(SELECT src_hash,kind,dst_hash FROM edges LIMIT 1)"
        }
    };
    let schema_sql = |mutation| match mutation {
        SchemaMutation::UnsupportedSchemaVersion => {
            "UPDATE metadata SET value='16' WHERE key='schema_version'"
        }
        SchemaMutation::LegacyIndexSemantics => {
            "UPDATE metadata SET value='15' WHERE key='schema_version'; UPDATE metadata SET value='old-v15-semantics' WHERE key='index_semantics_version'"
        }
    };
    let edge_label = |mutation| match mutation {
        EdgeMutation::DeleteFirstEdge => "delete first edge",
        EdgeMutation::ChangeFirstEdgeLine => "change first edge line",
    };
    let schema_label = |mutation| match mutation {
        SchemaMutation::UnsupportedSchemaVersion => "unsupported schema version",
        SchemaMutation::LegacyIndexSemantics => "legacy index semantics",
    };
    let read_owner_leaf = |path: &std::path::Path| -> (i64, Vec<u8>) {
        let conn = Connection::open(path).unwrap();
        commitments::verify(&conn).expect("built database verifies");
        let owner: i64 = conn
            .query_row(
                "SELECT path_id FROM path_dictionary WHERE path='a.rs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let node_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM nodes WHERE owner_path_id=?1",
                [owner],
                |row| row.get(0),
            )
            .unwrap();
        let digest: Vec<u8> = conn
            .query_row(
                "SELECT digest FROM domain_commitments WHERE domain='nodes' AND owner_id=?1",
                [owner],
                |row| row.get(0),
            )
            .unwrap();
        (node_count, digest)
    };

    let workspace = Workspace::new();
    let source = workspace.path("source");
    workspace.write("source/a.rs", "pub fn exported() {}\n");
    let first = workspace.path("first.sqlite");
    let second = workspace.path("second.sqlite");
    writer::build(&source, &first, None).unwrap();
    writer::build(&source, &second, None).unwrap();

    let (first_node_count, first_digest) = read_owner_leaf(&first);
    let (_second_node_count, second_digest) = read_owner_leaf(&second);
    assert!(first_node_count > 0);
    assert_eq!(first_digest.len(), 32);
    assert_eq!(first_digest, second_digest);

    let restore_built_seed = |database: &std::path::Path| {
        let source_conn = Connection::open(&second).unwrap();
        let mut destination_conn = Connection::open(database).unwrap();
        rusqlite::backup::Backup::new(&source_conn, &mut destination_conn)
            .unwrap()
            .run_to_completion(256, std::time::Duration::ZERO, None)
            .unwrap();
    };

    for mutation in edge_mutations {
        let database = workspace.path("corruption.sqlite");
        restore_built_seed(&database);
        drop(reader::open(&database, &source).unwrap());
        let conn = Connection::open(&database).unwrap();
        let rows_changed = conn.execute(edge_sql(mutation), []).unwrap();
        assert_eq!(rows_changed, 1, "{}", edge_label(mutation));
        assert!(commitments::verify(&conn).is_err(), "{}", edge_label(mutation));
        drop(conn);
        assert!(
            reader::open(&database, &source).is_err(),
            "{}",
            edge_label(mutation)
        );
    }

    let conn = Connection::open(&first).unwrap();
    conn.execute_batch("CREATE TEMP TABLE victim AS SELECT src_hash AS hash FROM edges LIMIT 1; DELETE FROM nodes WHERE node_hash IN (SELECT hash FROM victim)").unwrap();
    assert!(commitments::verify(&conn).is_err());
    drop(conn);

    for mutation in schema_mutations {
        let legacy = workspace.path("legacy.sqlite");
        restore_built_seed(&legacy);
        let conn = Connection::open(&legacy).unwrap();
        conn.execute_batch(schema_sql(mutation)).unwrap();
        drop(conn);
        assert!(
            reader::open(&legacy, &source).is_err(),
            "{}",
            schema_label(mutation)
        );
        writer::build(&source, &legacy, None).unwrap();
        let conn = reader::open(&legacy, &source).unwrap();
        let schema_after_rebuild: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            schema_after_rebuild,
            contextunity_forge_mcp::core::config::ENGINE_SCHEMA_VERSION,
            "{}",
            schema_label(mutation)
        );
        commitments::verify(&conn).expect("rebuilt schema verifies");
    }
}

#[test]
fn read_only_query_only_verification_uses_temporary_logical_fts_views() {
    let workspace = Workspace::new();
    let path = workspace.path("fts-commitments.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute_batch(SCHEMA_DDL).unwrap();
    search_nodes(&conn, 1);
    conn.execute(
        "INSERT INTO node_search(rowid,search_text) VALUES(1,'stable node posting')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO doc_sections(doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size) \
         VALUES('doc-17','docs/17.md','Stable title','guide','stable content','stable invariant','',0,0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO doc_search(rowid,section_title,content,invariants) \
         SELECT rowid,section_title,content,invariants FROM doc_sections",
        [],
    )
    .unwrap();
    commitments::seal(&conn).unwrap();
    drop(conn);

    let reader = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read-only fixture open succeeds");
    reader.pragma_update(None, "query_only", true).unwrap();
    commitments::verify(&reader).expect("query-only verification succeeds");
    let query_only_mode: i64 = reader
        .query_row("PRAGMA query_only", [], |row| row.get(0))
        .unwrap();
    assert_eq!(query_only_mode, 1, "verification restores query_only mode");

    reader.execute_batch("BEGIN").unwrap();
    let _: String = reader
        .query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("BEGIN IMMEDIATE; DELETE FROM node_search WHERE rowid=1; INSERT INTO node_search(rowid,search_text) VALUES(1,'changed posting'); COMMIT;")
        .unwrap();
    commitments::verify(&reader).expect("the active snapshot sees its original sealed state");
    reader.execute_batch("ROLLBACK").unwrap();
    assert!(
        commitments::verify(&reader).is_err(),
        "a new snapshot sees the unsealed mutation"
    );
}
