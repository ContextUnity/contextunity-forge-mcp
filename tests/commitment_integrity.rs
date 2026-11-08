#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

#[path = "commitment_integrity/cold_sealing.rs"]
mod cold_sealing;
#[path = "commitment_integrity/coverage.rs"]
mod coverage;
#[path = "commitment_integrity/disk_index.rs"]
mod disk_index;
#[path = "commitment_integrity/harness.rs"]
mod harness;
#[path = "commitment_integrity/parallel_sealing.rs"]
mod parallel_sealing;

use contextunity_forge_mcp::core::models::stable_hash64;
use harness::*;
use rusqlite::{config::DbConfig, params};

#[test]
fn logical_fts_commitments_ignore_physical_insertion_order() {
    let cases = [("forward", false), ("reverse", true)];
    let mut roots = Vec::with_capacity(2);

    for (label, reverse) in cases {
        let conn = connection();
        search_nodes(&conn, 64);
        let path_id = owner_id(&conn, "src/fts.rs");
        conn.execute(
            "INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) VALUES('fts:negative','function','negative','negative',?1,1,1,0,'rust',0,'{}',-1,?1)",
            [path_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO node_search(rowid,search_text) VALUES(-1,'α unicode unicode')",
            [],
        )
        .unwrap();
        for rowid in 1..=16 {
            conn.execute(
                "INSERT INTO doc_sections(doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size) \
                 VALUES(?1,?2,?3,'guide',?4,?5,'',0,0)",
                params![
                    format!("doc-{rowid}"),
                    format!("docs/{rowid}.md"),
                    format!("Section {rowid}"),
                    format!("content token {rowid}"),
                    format!("invariant token {rowid}")
                ],
            )
            .unwrap();
        }

        let mut rowids: Vec<_> = (1..=64).collect();
        if reverse {
            rowids.reverse();
        }
        for rowid in rowids {
            conn.execute(
                "INSERT INTO node_search(rowid,search_text) VALUES(?1,?2)",
                params![rowid, format!("node token {rowid}")],
            )
            .unwrap();
        }

        let mut doc_rowids: Vec<_> = (1..=16).collect();
        if reverse {
            doc_rowids.reverse();
        }
        for rowid in doc_rowids {
            let (title, content, invariants): (String, String, String) = conn
                .query_row(
                    "SELECT section_title,content,invariants FROM doc_sections WHERE rowid=?1",
                    [rowid],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            conn.execute(
                "INSERT INTO doc_search(rowid,section_title,content,invariants) VALUES(?1,?2,?3,?4)",
                params![rowid, title, content, invariants],
            )
            .unwrap();
        }

        conn.execute_batch(
            "CREATE VIRTUAL TABLE temp.node_search_commit_vocab USING fts5vocab(main,node_search,'instance');",
        )
        .unwrap();
        let ordered = fts_postings(&conn, "node_search_commit_vocab");
        let streaming_postings_sorted =
            ordered.len() > 1 && ordered.windows(2).all(|pair| pair[0] <= pair[1]);
        assert!(
            streaming_postings_sorted,
            "{label}: streaming branch is exercised"
        );

        let root = commitments::seal(&conn).expect("initial sealing succeeds");
        commitments::verify(&conn).expect("the streaming result verifies");

        conn.execute_batch(
            "CREATE VIRTUAL TABLE temp.raw_node_vocab USING fts5vocab(main,node_search,'instance');
             DROP TABLE temp.node_search_commit_vocab;
             CREATE TEMP TABLE node_search_commit_vocab AS
               SELECT term,col,offset,doc FROM raw_node_vocab
               ORDER BY term DESC,doc DESC,col DESC,offset DESC;",
        )
        .unwrap();
        let reversed = conn
            .prepare("SELECT term,doc,col,offset FROM node_search_commit_vocab ORDER BY term DESC,doc DESC,col DESC,offset DESC")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let fallback_postings_reversed = reversed.windows(2).any(|pair| pair[0] > pair[1]);
        assert!(
            fallback_postings_reversed,
            "{label}: sorted fallback is exercised"
        );

        commitments::verify(&conn).expect("the fallback result verifies");
        let fallback_root = commitments::seal(&conn).expect("fallback sealing succeeds");
        assert_eq!(
            fallback_root, root,
            "{label}: fallback root must match initial root"
        );

        conn.execute(
            "UPDATE node_search_commit_vocab SET doc=9000001 WHERE rowid=(SELECT min(rowid) FROM node_search_commit_vocab)",
            [],
        )
        .unwrap();
        assert!(
            commitments::verify(&conn).is_err(),
            "{label}: missing posting owner must fail closed"
        );

        roots.push(root);
    }

    assert_eq!(
        roots[0], roots[1],
        "node_search and doc_search logical postings must seal identically regardless of insertion order"
    );
}

#[test]
fn sealing_rejects_missing_commitment_references() {
    #[derive(Clone, Copy)]
    enum MissingReference {
        EdgeOwner,
        EdgeSource,
        EdgeDestination,
        OccurrenceOwner,
        OccurrenceSource,
        OccurrenceDestination,
        DependencyOwner,
        SharedOwner,
        SharedKey,
    }

    let cases = [
        (
            MissingReference::EdgeOwner,
            "edge commitment references missing owner path",
        ),
        (
            MissingReference::EdgeSource,
            "edge commitment references missing source node",
        ),
        (
            MissingReference::EdgeDestination,
            "edge commitment references missing destination node",
        ),
        (
            MissingReference::OccurrenceOwner,
            "edge occurrence commitment references missing owner path",
        ),
        (
            MissingReference::OccurrenceSource,
            "edge occurrence commitment references missing source node",
        ),
        (
            MissingReference::OccurrenceDestination,
            "edge occurrence commitment references missing destination node",
        ),
        (
            MissingReference::DependencyOwner,
            "dependency commitment references missing owner path",
        ),
        (
            MissingReference::SharedOwner,
            "shared-owner commitment references missing owner path",
        ),
        (
            MissingReference::SharedKey,
            "shared-owner commitment references missing key",
        ),
    ];
    let conn = connection();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    let owner = owner_id(&conn, "src/a.rs");
    for id in ["fn:source", "fn:destination"] {
        conn.execute(
            "INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id)
             VALUES(?1,'function',?1,?1,?2,1,1,0,'rust',0,'{}',?3,?2)",
            params![id, owner, stable_hash64(id)],
        )
        .unwrap();
    }
    let source_hash = stable_hash64("fn:source");
    let destination_hash = stable_hash64("fn:destination");
    let target_key_hash = stable_hash64("fn:target");
    conn.execute(
        "INSERT INTO shared_keys(key_hash,key) VALUES(?1,'fn:target')",
        [target_key_hash],
    )
    .unwrap();

    for (mutation, reason) in cases {
        conn.execute_batch("SAVEPOINT missing_commitment_reference;")
            .unwrap();
        let _ = match mutation {
            MissingReference::EdgeOwner => conn
                .execute(
                    "INSERT INTO edges VALUES(?1,'calls',?2,99,1,1,2,1)",
                    params![source_hash, destination_hash],
                )
                .unwrap(),
            MissingReference::EdgeSource => conn
                .execute(
                    "INSERT INTO edges VALUES(?1,'calls',?2,?3,1,1,2,1)",
                    params![stable_hash64("fn:missing-source"), destination_hash, owner],
                )
                .unwrap(),
            MissingReference::EdgeDestination => conn
                .execute(
                    "INSERT INTO edges VALUES(?1,'calls',?2,?3,1,1,2,1)",
                    params![source_hash, stable_hash64("fn:missing-destination"), owner],
                )
                .unwrap(),
            MissingReference::OccurrenceOwner => conn
                .execute(
                    "INSERT INTO edge_occurrences VALUES(99,0,?1,?2,'calls',1,1,2)",
                    params![source_hash, destination_hash],
                )
                .unwrap(),
            MissingReference::OccurrenceSource => conn
                .execute(
                    "INSERT INTO edge_occurrences VALUES(?1,0,?2,?3,'calls',1,1,2)",
                    params![owner, stable_hash64("fn:missing-source"), destination_hash],
                )
                .unwrap(),
            MissingReference::OccurrenceDestination => conn
                .execute(
                    "INSERT INTO edge_occurrences VALUES(?1,0,?2,?3,'calls',1,1,2)",
                    params![owner, source_hash, stable_hash64("fn:missing-destination")],
                )
                .unwrap(),
            MissingReference::DependencyOwner => conn
                .execute(
                    "INSERT INTO dependencies VALUES(99,0,?1,'imports','target','resolved')",
                    [stable_hash64("src/target.rs")],
                )
                .unwrap(),
            MissingReference::SharedOwner => conn
                .execute(
                    "INSERT INTO shared_owners VALUES(0,?1,99,0)",
                    [target_key_hash],
                )
                .unwrap(),
            MissingReference::SharedKey => conn
                .execute(
                    "INSERT INTO shared_owners VALUES(0,?1,?2,0)",
                    params![stable_hash64("fn:missing-target"), owner],
                )
                .unwrap(),
        };
        let sealing = commitments::seal(&conn);
        conn.execute_batch(
            "ROLLBACK TO missing_commitment_reference; RELEASE missing_commitment_reference;",
        )
        .unwrap();
        let error = sealing.expect_err("malformed commitment reference must fail sealing");
        assert!(error.to_string().contains(reason), "{reason}: {error:#}");
    }
}

#[test]
fn logical_fts_commitments_are_stable_and_reject_semantic_tampering() {
    let conn = connection();
    search_nodes(&conn, 2048);
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
    let expected_root = commitments::seal(&conn).unwrap();

    let cases = [
        ("node_search_postings", "INSERT INTO node_search(rowid,search_text) VALUES(9000001,'logicalpostingtamper')", "node_search_postings"),
        ("node_search_docsize", "UPDATE node_search_docsize SET sz=CAST(sz || X'00' AS BLOB) WHERE id=(SELECT id FROM node_search_docsize ORDER BY id LIMIT 1)", "node_search_docsize"),
        ("node_search_config", "UPDATE node_search_config SET v=CAST(v || 'x' AS BLOB) WHERE k=(SELECT k FROM node_search_config ORDER BY k LIMIT 1)", "node_search_config"),
        ("doc_search_postings", "INSERT INTO doc_search(rowid,section_title,content,invariants) VALUES(9000001,'logical posting tamper','','')", "doc_search_postings"),
        ("doc_search_docsize", "UPDATE doc_search_docsize SET sz=CAST(sz || X'00' AS BLOB) WHERE id=(SELECT id FROM doc_search_docsize ORDER BY id LIMIT 1)", "doc_search_docsize"),
        ("doc_search_config", "UPDATE doc_search_config SET v=CAST(v || 'x' AS BLOB) WHERE k=(SELECT k FROM doc_search_config ORDER BY k LIMIT 1)", "doc_search_config"),
    ];
    for (table, sql, commitment) in cases {
        conn.execute_batch("SAVEPOINT fts_shadow_tamper").unwrap();
        let rows_changed = conn.execute(sql, []).unwrap();
        assert_eq!(rows_changed, 1, "{table}");
        let error = commitments::verify(&conn)
            .expect_err("tampering must invalidate the stored logical seal");
        assert!(error.to_string().contains(commitment), "{error:#}");
        conn.execute_batch("ROLLBACK TO fts_shadow_tamper; RELEASE fts_shadow_tamper;")
            .unwrap();
        commitments::verify(&conn).expect("rolling back tampering restores verification");
    }

    conn.execute_batch("DROP INDEX partial_order_probe; ANALYZE;")
        .unwrap();
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, defensive)
        .unwrap();
    commitments::verify(&conn).expect("cleanup restores verification");
    let cleanup_root = commitments::seal(&conn).expect("cleanup reseals");
    assert_eq!(cleanup_root, expected_root);

    conn.pragma_update(None, "reverse_unordered_selects", true)
        .unwrap();
    let reverse_unordered_root =
        commitments::seal(&conn).expect("reverse unordered selects reseal");
    assert_eq!(reverse_unordered_root, expected_root);

    conn.execute_batch("ANALYZE; VACUUM;").unwrap();
    let vacuum_root = commitments::seal(&conn).expect("ANALYZE and VACUUM reseal");
    assert_eq!(vacuum_root, expected_root);
    commitments::verify(&conn).expect("the maintained FTS state verifies");
}

#[test]
fn fts_sealing_survives_valid_rebuild_reindex_and_vacuum() {
    let conn = connection();
    search_nodes(&conn, 512);
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

    let node_postings_before = fts_postings(&conn, "node_search_vocab");
    let doc_postings_before = fts_postings(&conn, "doc_search_vocab");
    let initial_root = commitments::seal(&conn).unwrap();
    commitments::verify(&conn).expect("the original FTS state verifies");

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
    let node_postings_after_rebuild = fts_postings(&conn, "node_search_vocab");
    let doc_postings_after_rebuild = fts_postings(&conn, "doc_search_vocab");
    assert_eq!(node_postings_after_rebuild, node_postings_before);
    assert_eq!(doc_postings_after_rebuild, doc_postings_before);

    let rebuilt_root = commitments::seal(&conn).expect("the rebuilt FTS state seals");
    assert_eq!(
        rebuilt_root, initial_root,
        "rebuilding identical FTS postings must preserve this fixture's commitment root"
    );
    commitments::verify(&conn).expect("the rebuilt FTS state verifies");
    let resealed_rebuild_root = commitments::seal(&conn).expect("the rebuilt FTS state reseals");
    assert_eq!(resealed_rebuild_root, rebuilt_root);

    conn.execute_batch("REINDEX; ANALYZE; VACUUM;").unwrap();
    let node_postings_after_maintenance = fts_postings(&conn, "node_search_vocab");
    let doc_postings_after_maintenance = fts_postings(&conn, "doc_search_vocab");
    assert_eq!(
        node_postings_after_maintenance, node_postings_before,
        "maintenance must preserve semantic postings even if shadow storage is repacked"
    );
    assert_eq!(doc_postings_after_maintenance, doc_postings_before);

    let maintained_root = commitments::seal(&conn).expect("the maintained FTS state seals");
    assert_eq!(
        maintained_root, rebuilt_root,
        "REINDEX, ANALYZE, and VACUUM must preserve the logical FTS commitment root"
    );
    commitments::verify(&conn).expect("the maintained FTS state verifies");
    let resealed_maintenance_root =
        commitments::seal(&conn).expect("the maintained FTS state reseals");
    assert_eq!(resealed_maintenance_root, maintained_root);
}

#[test]
fn owner_language_expression_batches_preserve_repeatable_full_and_partial_roots() {
    let cases = [
        harness::ExpressionOrder::Ascending,
        harness::ExpressionOrder::Descending,
    ];
    let mut roots = Vec::with_capacity(2);
    for order in cases {
        let sql_order = match order {
            harness::ExpressionOrder::Ascending => "ASC",
            harness::ExpressionOrder::Descending => "DESC",
        };
        let conn = connection();
        let owner = owner_id(&conn, "src/owner_language.rs");
        conn.execute_batch(&format!(
            "WITH RECURSIVE n(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<900)
             INSERT INTO coverage_expressions(expression)
             SELECT 'owner_expr_'||printf('%04d',x) FROM n ORDER BY x {};",
            sql_order
        ))
        .unwrap();
        let inserted_expressions = conn
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
        assert_eq!(inserted_expressions, 901);
        let owner_rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM coverage_owner_language WHERE path_id=?1",
                [owner],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner_rows, 901);

        let root = commitments::seal(&conn).unwrap();
        let repeat_root = commitments::seal(&conn).unwrap();
        assert_eq!(repeat_root, root);

        let selected = std::collections::BTreeSet::from(["src/owner_language.rs".to_owned()]);
        let partial_root = commitments::seal_owners(&conn, Some(&selected)).unwrap();
        assert_eq!(partial_root, root);

        commitments::verify_owners(&conn, Some(&selected)).expect("selected owner verifies");
        commitments::verify(&conn).expect("full database verifies");

        roots.push(root);
    }
    assert_eq!(roots[0], roots[1]);
}
