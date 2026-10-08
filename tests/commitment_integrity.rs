#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::Workspace;
use contextunity_forge_mcp::{
    core::{commitments, models::stable_hash64, schema::SCHEMA_DDL},
    db::writer,
};
use rusqlite::{config::DbConfig, params, Connection};
#[path = "commitment_integrity/cold_sealing.rs"]
mod cold_sealing;
#[path = "commitment_integrity/coverage.rs"]
mod coverage;
fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA_DDL).unwrap();
    conn.execute_batch(
        "INSERT INTO coverage_evidence(evidence_id,evidence) VALUES(1,'call'),(2,'exact')",
    )
    .unwrap();
    conn
}

#[test]
fn manifest_input_digest_is_committed_and_tampering_is_detected() {
    let conn = connection();
    conn.execute(
        "INSERT INTO metadata VALUES('manifest_digest','before')",
        [],
    )
    .unwrap();
    let original = commitments::seal(&conn).unwrap();
    commitments::verify(&conn).unwrap();
    conn.execute(
        "UPDATE metadata SET value='after' WHERE key='manifest_digest'",
        [],
    )
    .unwrap();
    assert!(commitments::verify(&conn).is_err());
    assert_ne!(commitments::seal(&conn).unwrap(), original);
}

fn owner_id(conn: &Connection, path: &str) -> i64 {
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

fn node(conn: &Connection, id: &str, path: &str) {
    let path_id = owner_id(conn, path);
    conn.execute(
        "INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) VALUES(?1,'function',?1,?1,?2,1,1,0,'rust',0,'{}',?3,?2)",
        params![id, path_id, stable_hash64(id)],
    )
    .unwrap();
}

fn assert_seal_rejects(conn: &Connection, reason: &str) {
    let error = commitments::seal(conn).expect_err("malformed graph reference must fail sealing");
    assert!(error.to_string().contains(reason), "{error:#}");
}

fn fts_postings(conn: &Connection, vocab: &str) -> Vec<(String, i64, String, i64)> {
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

#[test]
fn sealing_rejects_edges_with_missing_owner_or_node_endpoints() {
    for missing in ["owner", "source", "destination"] {
        let conn = connection();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let owner = if missing == "owner" {
            99
        } else {
            owner_id(&conn, "src/a.rs")
        };
        if missing != "source" {
            node(&conn, "fn:source", "src/a.rs");
        }
        if missing != "destination" {
            node(&conn, "fn:destination", "src/a.rs");
        }
        conn.execute(
            "INSERT INTO edges VALUES(?1,'calls',?2,?3,1,1,2,1)",
            params![
                stable_hash64("fn:source"),
                stable_hash64("fn:destination"),
                owner
            ],
        )
        .unwrap();
        let reason = match missing {
            "owner" => "edge commitment references missing owner path",
            "source" => "edge commitment references missing source node",
            _ => "edge commitment references missing destination node",
        };
        assert_seal_rejects(&conn, reason);
    }
}

#[test]
fn sealing_rejects_occurrences_with_missing_owner_or_node_endpoints() {
    for missing in ["owner", "source", "destination"] {
        let conn = connection();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let owner = if missing == "owner" {
            99
        } else {
            owner_id(&conn, "src/a.rs")
        };
        if missing != "source" {
            node(&conn, "fn:source", "src/a.rs");
        }
        if missing != "destination" {
            node(&conn, "fn:destination", "src/a.rs");
        }
        conn.execute(
            "INSERT INTO edge_occurrences VALUES(?1,0,?2,?3,'calls',1,1,2)",
            params![
                owner,
                stable_hash64("fn:source"),
                stable_hash64("fn:destination")
            ],
        )
        .unwrap();
        let reason = match missing {
            "owner" => "edge occurrence commitment references missing owner path",
            "source" => "edge occurrence commitment references missing source node",
            _ => "edge occurrence commitment references missing destination node",
        };
        assert_seal_rejects(&conn, reason);
    }
}

#[test]
fn sealing_rejects_dependencies_with_missing_owner() {
    let conn = connection();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute(
        "INSERT INTO dependencies VALUES(99,0,?1,'imports','target','resolved')",
        [stable_hash64("src/target.rs")],
    )
    .unwrap();
    assert_seal_rejects(&conn, "dependency commitment references missing owner path");
}

#[test]
fn sealing_rejects_shared_owners_with_missing_owner_or_key() {
    for missing in ["owner", "key"] {
        let conn = connection();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let owner = if missing == "owner" {
            99
        } else {
            owner_id(&conn, "src/a.rs")
        };
        let key_hash = stable_hash64("fn:target");
        if missing != "key" {
            conn.execute(
                "INSERT INTO shared_keys(key_hash,key) VALUES(?1,'fn:target')",
                [key_hash],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO shared_owners VALUES(0,?1,?2,0)",
            params![key_hash, owner],
        )
        .unwrap();
        let reason = if missing == "owner" {
            "shared-owner commitment references missing owner path"
        } else {
            "shared-owner commitment references missing key"
        };
        assert_seal_rejects(&conn, reason);
    }
}

#[test]
fn fts_shadow_commitments_are_stable_across_scan_order_and_vacuum() {
    let conn = connection();
    conn.execute_batch(
        "INSERT INTO node_search(node_search,rank) VALUES('pgsz',256);
         WITH RECURSIVE nums(x) AS (
             VALUES(1)
             UNION ALL SELECT x+1 FROM nums WHERE x<2048
         )
         INSERT INTO node_search(rowid,search_text)
         SELECT x,printf('unique%06d',x) FROM nums;",
    )
    .unwrap();
    for rowid in 1..=8 {
        conn.execute(
            "INSERT INTO doc_sections(doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size) \
             VALUES(?1,?2,?3,'guide',?4,?5,'',0,0)",
            params![
                format!("doc-{rowid}"),
                format!("docs/{rowid}.md"),
                format!("Section {rowid}"),
                format!("content search {rowid}"),
                format!("invariant {rowid}")
            ],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO doc_search(rowid,section_title,content,invariants) \
         SELECT rowid,section_title,content,invariants FROM doc_sections",
        [],
    )
    .unwrap();

    let defensive = conn.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap();
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
        .unwrap();
    conn.execute_batch(
        "CREATE INDEX partial_order_probe ON node_search_idx(segid,pgno DESC,term);
         ANALYZE;",
    )
    .unwrap();

    let expected = commitments::seal(&conn).unwrap();

    let shadow_mutations = [
        (
            "node_search_data",
            "UPDATE node_search_data SET block=CAST(block || X'00' AS BLOB) \
             WHERE id=(SELECT id FROM node_search_data ORDER BY id LIMIT 1)",
        ),
        (
            "node_search_idx",
            "UPDATE node_search_idx SET pgno=pgno+1 \
             WHERE (segid,term)=(SELECT segid,term FROM node_search_idx LIMIT 1)",
        ),
        (
            "node_search_docsize",
            "UPDATE node_search_docsize SET sz=CAST(sz || X'00' AS BLOB) \
             WHERE id=(SELECT id FROM node_search_docsize ORDER BY id LIMIT 1)",
        ),
        (
            "node_search_config",
            "UPDATE node_search_config SET v=CAST(v || 'x' AS BLOB) \
             WHERE k=(SELECT k FROM node_search_config ORDER BY k LIMIT 1)",
        ),
        (
            "doc_search_data",
            "UPDATE doc_search_data SET block=CAST(block || X'00' AS BLOB) \
             WHERE id=(SELECT id FROM doc_search_data ORDER BY id LIMIT 1)",
        ),
        (
            "doc_search_idx",
            "UPDATE doc_search_idx SET pgno=pgno+1 \
             WHERE (segid,term)=(SELECT segid,term FROM doc_search_idx LIMIT 1)",
        ),
        (
            "doc_search_docsize",
            "UPDATE doc_search_docsize SET sz=CAST(sz || X'00' AS BLOB) \
             WHERE id=(SELECT id FROM doc_search_docsize ORDER BY id LIMIT 1)",
        ),
        (
            "doc_search_config",
            "UPDATE doc_search_config SET v=CAST(v || 'x' AS BLOB) \
             WHERE k=(SELECT k FROM doc_search_config ORDER BY k LIMIT 1)",
        ),
    ];
    for (table, mutation) in shadow_mutations {
        conn.execute_batch("SAVEPOINT fts_shadow_tamper").unwrap();
        assert_eq!(conn.execute(mutation, []).unwrap(), 1, "{table}");
        let error = commitments::verify(&conn).expect_err(&format!(
            "tampering with {table} must invalidate the stored seal"
        ));
        assert!(error.to_string().contains(table), "{error:#}");
        conn.execute_batch("ROLLBACK TO fts_shadow_tamper; RELEASE fts_shadow_tamper;")
            .unwrap();
        commitments::verify(&conn).unwrap();
    }

    conn.execute_batch("DROP INDEX partial_order_probe; ANALYZE;")
        .unwrap();
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, defensive)
        .unwrap();
    commitments::verify(&conn).unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), expected);

    conn.pragma_update(None, "reverse_unordered_selects", true)
        .unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), expected);

    conn.execute_batch("ANALYZE; VACUUM;").unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), expected);
    commitments::verify(&conn).unwrap();
}

#[test]
fn fts_sealing_survives_valid_rebuild_reindex_and_vacuum() {
    let conn = connection();
    conn.execute_batch(
        "INSERT INTO node_search(node_search,rank) VALUES('pgsz',256);
         WITH RECURSIVE nums(x) AS (
             VALUES(1)
             UNION ALL SELECT x+1 FROM nums WHERE x<512
         )
         INSERT INTO node_search(rowid,search_text)
         SELECT x,printf('nodeword%06d shared',x) FROM nums;
         INSERT INTO doc_sections(doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size)
         VALUES('doc-1','docs/one.md','One','guide','shared document content','stable invariant','',0,0),
               ('doc-2','docs/two.md','Two','guide','other document content','stable invariant','',0,0);
         INSERT INTO doc_search(rowid,section_title,content,invariants)
         SELECT rowid,section_title,content,invariants FROM doc_sections;
         CREATE VIRTUAL TABLE node_search_vocab USING fts5vocab(node_search,'instance');
         CREATE VIRTUAL TABLE doc_search_vocab USING fts5vocab(doc_search,'instance');
         CREATE INDEX fts_maintenance_probe ON doc_sections(path);",
    )
    .unwrap();

    let node_postings = fts_postings(&conn, "node_search_vocab");
    let doc_postings = fts_postings(&conn, "doc_search_vocab");
    let initial_root = commitments::seal(&conn).unwrap();
    commitments::verify(&conn).unwrap();

    conn.execute_batch(
        "INSERT INTO node_search(node_search) VALUES('delete-all');
          WITH RECURSIVE nums(x) AS (
              VALUES(1)
              UNION ALL SELECT x+1 FROM nums WHERE x<512
          )
          INSERT INTO node_search(rowid,search_text)
          SELECT x,printf('nodeword%06d shared',x) FROM nums;
         INSERT INTO doc_search(doc_search) VALUES('rebuild');",
    )
    .unwrap();
    assert_eq!(fts_postings(&conn, "node_search_vocab"), node_postings);
    assert_eq!(fts_postings(&conn, "doc_search_vocab"), doc_postings);
    let rebuilt_root = commitments::seal(&conn).unwrap();
    assert_eq!(
        rebuilt_root, initial_root,
        "rebuilding identical FTS postings must preserve this fixture's commitment root"
    );
    commitments::verify(&conn).unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), rebuilt_root);

    conn.execute_batch("REINDEX; ANALYZE; VACUUM;").unwrap();
    assert_eq!(
        fts_postings(&conn, "node_search_vocab"),
        node_postings,
        "maintenance must preserve semantic postings even if shadow storage is repacked"
    );
    assert_eq!(fts_postings(&conn, "doc_search_vocab"), doc_postings);
    let maintained_root = commitments::seal(&conn).unwrap();
    assert_eq!(
        maintained_root, rebuilt_root,
        "REINDEX, ANALYZE, and VACUUM must preserve the physical FTS commitment root"
    );
    commitments::verify(&conn).unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), maintained_root);
}

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
    commitments::verify(&first_conn).unwrap();
    first_conn
        .execute_batch("REINDEX; ANALYZE; VACUUM;")
        .unwrap();
    assert_eq!(
        first_conn
            .query_row(
                "SELECT value FROM metadata WHERE key='output_root'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        first_root,
        "valid index maintenance must preserve the stored Merkle root"
    );
    commitments::verify(&first_conn).unwrap();
    drop(first_conn);

    let second_db = workspace.path("second.sqlite");
    let second_report = writer::build(&source, &second_db, None).unwrap();
    assert_eq!(
        second_report["output_root"].as_str().unwrap(),
        first_root,
        "identical unchanged source must produce the same cold-build Merkle root"
    );
    commitments::verify(&Connection::open(&second_db).unwrap()).unwrap();
}

#[cfg(feature = "lang-rust")]
#[test]
fn built_nodes_keep_owner_paths_and_repeatable_merkle_leaves() {
    let workspace = Workspace::new();
    let source = workspace.path("source");
    workspace.write("source/a.rs", "pub fn exported() {}\n");
    let first = workspace.path("first.sqlite");
    let second = workspace.path("second.sqlite");
    writer::build(&source, &first, None).unwrap();
    writer::build(&source, &second, None).unwrap();
    let leaf = |path: &std::path::Path| {
        let conn = Connection::open(path).unwrap();
        commitments::verify(&conn).unwrap();
        let owner: i64 = conn
            .query_row(
                "SELECT path_id FROM path_dictionary WHERE path='a.rs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM nodes WHERE owner_path_id=?1",
                [owner],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0);
        let digest: Vec<u8> = conn
            .query_row(
                "SELECT digest FROM domain_commitments WHERE domain='nodes' AND owner_id=?1",
                [owner],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(digest.len(), 32);
        digest
    };
    assert_eq!(leaf(&first), leaf(&second));
}

#[test]
fn parallel_owner_sealing_matches_serial_encoding_and_thread_counts() {
    cold_sealing::assert_disk_build_matches_serial_seal();
    let mut conn = connection();
    let mut owners = std::collections::BTreeSet::new();
    for owner_index in (0..128).rev() {
        let path = format!("src/owner_{owner_index:03}.rs");
        owners.insert(path.clone());
        let owner = owner_id(&conn, &path);
        for ordinal in (0..64).rev() {
            let id = format!("fn:{owner_index}:{ordinal}");
            node(&conn, &id, &path);
            conn.execute(
                "UPDATE nodes SET details=?1 WHERE id=?2",
                params!["x".repeat(1024), id],
            )
            .unwrap();
        }
        for ordinal in (0..64).rev() {
            let src = stable_hash64(&format!("fn:{owner_index}:{ordinal}"));
            let dst = stable_hash64(&format!("fn:{owner_index}:0"));
            conn.execute(
                "INSERT INTO edges VALUES(?1,'calls',?2,?3,?4,1,2,2)",
                params![src, dst, owner, ordinal + 1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO edge_occurrences VALUES(?1,?2,?3,?4,'calls',?5,1,2)",
                params![owner, ordinal, src, dst, ordinal + 1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO dependencies VALUES(?1,?2,NULL,'imports',?3,'external')",
                params![owner, ordinal, format!("symbol_{ordinal}")],
            )
            .unwrap();
        }
    }
    coverage::seed_canonical(&conn);
    let single = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let parallel = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let connection = &mut conn;
    let expected_root = single
        .install(move || commitments::seal(connection))
        .unwrap();
    for table in [
        "nodes",
        "edges",
        "edge_occurrences",
        "dependencies",
        "resolution_coverage",
    ] {
        for owner in &owners {
            let owner_path_id: i64 = conn
                .query_row(
                    "SELECT path_id FROM path_dictionary WHERE path=?1",
                    [owner],
                    |row| row.get(0),
                )
                .unwrap();
            let actual: Vec<u8> = conn
                .query_row(
                    "SELECT digest FROM domain_commitments WHERE domain=?1 AND owner_id=?2",
                    params![table, owner_path_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(actual.len(), 32, "{table}: {owner}");
        }
    }
    assert_eq!(
        parallel
            .install({
                let connection = &mut conn;
                move || commitments::seal(connection)
            })
            .unwrap(),
        expected_root
    );
    assert_eq!(
        parallel
            .install({
                let connection = &mut conn;
                let owners = &owners;
                move || commitments::seal_owners(connection, Some(owners))
            })
            .unwrap(),
        expected_root
    );
    parallel
        .install({
            let connection = &mut conn;
            move || commitments::verify(connection)
        })
        .unwrap();
}

fn owner_language_root_with_expression_order(order: &str) -> String {
    let conn = connection();
    let owner = owner_id(&conn, "src/owner_language.rs");
    conn.execute_batch(&format!(
        "WITH RECURSIVE n(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<900)
         INSERT INTO coverage_expressions(expression)
         SELECT 'owner_expr_'||printf('%04d',x) FROM n ORDER BY x {order};"
    ))
    .unwrap();
    let inserted = conn
        .execute(
            "INSERT INTO coverage_owner_language(path_id,line,expression_id,status,evidence_id,language)
             SELECT ?1,
                    CAST(substr(expression,12) AS INTEGER)%17,
                    expression_id,
                    CASE CAST(substr(expression,12) AS INTEGER)%3
                        WHEN 0 THEN 'resolved'
                        WHEN 1 THEN 'external'
                        ELSE 'unresolved'
                    END,
                    1,
                    'javascript'
             FROM coverage_expressions
             WHERE expression LIKE 'owner_expr_%'",
            [owner],
        )
        .unwrap();
    assert_eq!(inserted, 901);
    let row_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM coverage_owner_language WHERE path_id=?1",
            [owner],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row_count, 901);

    let root = commitments::seal(&conn).unwrap();
    assert_eq!(commitments::seal(&conn).unwrap(), root);
    let selected = std::collections::BTreeSet::from(["src/owner_language.rs".to_owned()]);
    assert_eq!(
        commitments::seal_owners(&conn, Some(&selected)).unwrap(),
        root
    );
    commitments::verify_owners(&conn, Some(&selected)).unwrap();
    commitments::verify(&conn).unwrap();
    root
}

#[test]
fn owner_language_expression_batches_preserve_repeatable_full_and_partial_roots() {
    let ascending = owner_language_root_with_expression_order("ASC");
    let descending = owner_language_root_with_expression_order("DESC");
    assert_eq!(ascending, descending);
}
