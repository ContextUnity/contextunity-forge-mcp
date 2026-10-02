use contextunity_forge_mcp::{
    core::{commitments, models::stable_hash64, schema::SCHEMA_DDL},
    db::writer,
};
use rusqlite::{config::DbConfig, params, Connection};
#[path = "commitment_integrity/cold_sealing.rs"]
mod cold_sealing;
#[path = "commitment_integrity/coverage.rs"]
mod coverage;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct TemporaryWorkspace(PathBuf);

impl TemporaryWorkspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_commitment_integrity_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA_DDL).unwrap();
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
    conn.execute("INSERT INTO path_dictionary(path) VALUES(?1)", [path])
        .unwrap();
    conn.query_row(
        "SELECT path_id FROM path_dictionary WHERE path=?1",
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

fn node(conn: &Connection, id: &str, path: &str) {
    conn.execute(
        "INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details,node_hash) VALUES(?1,'function',?1,?1,?2,1,1,0,'rust',0,'{}',?3)",
        params![id, path, stable_hash64(id)],
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
            "INSERT INTO edges_raw VALUES(?1,'calls',?2,?3,1,'call','exact',1)",
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
            "INSERT INTO edge_occurrences_raw VALUES(?1,0,?2,?3,'calls',1,'call','exact')",
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
    conn.execute(
        "INSERT INTO dependencies_raw VALUES(99,0,?1,'imports','target','resolved')",
        [stable_hash64("src/target.rs")],
    )
    .unwrap();
    assert_seal_rejects(&conn, "dependency commitment references missing owner path");
}

#[test]
fn sealing_rejects_shared_owners_with_missing_owner_or_key() {
    for missing in ["owner", "key"] {
        let conn = connection();
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
            "INSERT INTO shared_owners_raw VALUES(0,?1,?2,0)",
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
         INSERT INTO owned_search_raw(node_id,search_text)
         SELECT x,printf('nodeword%06d shared',x) FROM nums;
         INSERT INTO node_search(rowid,search_text)
         SELECT node_id,search_text FROM owned_search_raw;
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
         INSERT INTO node_search(rowid,search_text)
         SELECT node_id,search_text FROM owned_search_raw;
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
    let workspace = TemporaryWorkspace::new();
    let source = workspace.0.join("source");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(
        source.join("src/lib.rs"),
        "pub fn stable_build_fixture() -> &'static str { \"unchanged\" }\n",
    )
    .unwrap();

    let first_db = workspace.0.join("first.sqlite");
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

    let second_db = workspace.0.join("second.sqlite");
    let second_report = writer::build(&source, &second_db, None).unwrap();
    assert_eq!(
        second_report["output_root"].as_str().unwrap(),
        first_root,
        "identical unchanged source must produce the same cold-build Merkle root"
    );
    commitments::verify(&Connection::open(&second_db).unwrap()).unwrap();
}

#[cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-html",
    feature = "lang-vue"
))]
#[test]
fn built_file_commitments_match_json_encoding_of_durable_facts_and_linked_graph() {
    use contextunity_forge_mcp::{
        core::models::{Facts, Node},
        db::writer,
        engine::{languages, linker, scanner},
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    let workspace = TemporaryWorkspace::new();
    let source = workspace.0.join("source");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(
        source.join("src/lib.rs"),
        r#"/// Quotes "double", slash \\, and Unicode λ.
pub fn produce(input: &str) -> &str { input }
pub fn consume() { produce("escaped\\\"\n\tλ"); }
"#,
    )
    .unwrap();
    #[cfg(feature = "lang-python")]
    fs::write(
        source.join("src/flow.py"),
        "class Worker:\n    def work(self):\n        return 1\n\ndef make_worker() -> Worker:\n    return Worker()\n\ndef use_worker():\n    return make_worker().work()\n\nWorkerAlias = Worker\n\nclass Service:\n    def __init__(self):\n        self.worker = Worker()\n    def run(self):\n        return self.worker.work()\n",
    )
    .unwrap();
    #[cfg(feature = "lang-typescript")]
    fs::write(
        source.join("src/flow.ts"),
        "class Worker { work(): number { return 1; } }\ntype WorkerAlias = Worker;\nfunction makeWorker(): WorkerAlias { return new Worker(); }\nexport function useWorker() { const worker = makeWorker(); return worker.work(); }\n",
    )
    .unwrap();
    #[cfg(all(feature = "lang-html", feature = "lang-typescript"))]
    fs::write(
        source.join("src/classic.html"),
        "<script>class HtmlWorker { work() { return 1; } }</script><script>function makeHtmlWorker() { return new HtmlWorker(); } makeHtmlWorker().work();</script>",
    )
    .unwrap();
    #[cfg(feature = "lang-vue")]
    fs::write(
        source.join("src/flow.vue"),
        "<script setup lang=\"ts\">class VueWorker { work(): number { return 1; } }\ntype VueAlias = VueWorker;\nfunction makeVueWorker(): VueAlias { return new VueWorker(); }\nconst worker = makeVueWorker();\nworker.work();\n</script>\n<template>{{ worker.work() }}</template>\n",
    )
    .unwrap();
    fs::write(source.join("src/empty.rs"), "").unwrap();
    fs::write(
        source.join("README.md"),
        "# Unicode λ and quotes \"\\\"\n\nA documented source contract.\n",
    )
    .unwrap();
    let adapter = scanner::load_adapter(&source, None).unwrap();
    let scan = scanner::scan_with_adapter(&source, &adapter).unwrap();
    let public_facts: BTreeMap<String, Facts> = scan
        .entries
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                writer::extract(&source, file, &adapter).unwrap(),
            )
        })
        .collect();
    for file in &scan.entries {
        let Some(profile) = languages::by_id(&file.language) else {
            continue;
        };
        let file_source = fs::read_to_string(source.join(&file.path)).unwrap();
        let module = profile.module_name_for_source(&file.path, &file_source);
        let mut legacy_facts = Facts {
            nodes: vec![Node {
                id: format!("module:{}", file.path),
                kind: "module".into(),
                name: file.path.rsplit('/').next().unwrap_or(&file.path).into(),
                qualname: module.clone(),
                path: file.path.clone(),
                line: 1,
                end_line: file_source.lines().count().max(1),
                is_test: false,
                language: file.language.clone(),
                generated: false,
                details: json!({}),
            }],
            ..Facts::default()
        };
        profile
            .extract_file(&file.path, &file_source, &module, &mut legacy_facts)
            .unwrap();
        profile.finish(&mut legacy_facts);
        assert_eq!(
            serde_json::to_vec(&legacy_facts).unwrap(),
            serde_json::to_vec(&public_facts[&file.path]).unwrap(),
            "typed AST extraction must match direct legacy profile bytes for {}",
            file.path
        );
    }
    let database = workspace.0.join("encoding.sqlite");
    writer::build(&source, &database, None).unwrap();
    let conn = Connection::open(&database).unwrap();
    let mut statement = conn
        .prepare("SELECT path,facts_blob FROM local_facts ORDER BY path")
        .unwrap();
    let durable_facts: BTreeMap<String, (Vec<u8>, Facts)> = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (path, blob) = row.unwrap();
            let bytes = zstd::stream::decode_all(blob.as_slice()).unwrap();
            let facts = serde_json::from_slice(&bytes).unwrap();
            (path, (bytes, facts))
        })
        .collect();
    assert_eq!(durable_facts.len(), public_facts.len());
    for (path, (bytes, facts)) in &durable_facts {
        assert_eq!(
            serde_json::to_vec(facts).unwrap(),
            *bytes,
            "durable JSON Fact bytes must retain their decode order for {path}"
        );
    }
    let facts: BTreeMap<String, Facts> = durable_facts
        .into_iter()
        .map(|(path, (_, facts))| (path, facts))
        .collect();
    #[cfg(any(
        feature = "lang-python",
        feature = "lang-typescript",
        feature = "lang-vue",
        all(feature = "lang-html", feature = "lang-typescript")
    ))]
    let has_generated_flow = |path: &str| {
        public_facts.get(path).is_some_and(|fact| {
            fact.nodes.iter().any(|node| {
                node.details
                    .get("value_flow")
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|flow| !flow.is_empty())
            })
        })
    };
    #[cfg(feature = "lang-python")]
    assert!(has_generated_flow("src/flow.py"));
    #[cfg(feature = "lang-typescript")]
    assert!(has_generated_flow("src/flow.ts"));
    #[cfg(all(feature = "lang-html", feature = "lang-typescript"))]
    assert!(has_generated_flow("src/classic.html"));
    #[cfg(feature = "lang-vue")]
    assert!(has_generated_flow("src/flow.vue"));
    let graph = linker::link_with_root(&facts, None, Some(&source));
    assert!(!graph.edges.is_empty());
    assert!(!graph.coverage.is_empty());
    let json_hash = |value: serde_json::Value| commitments::hash(value.to_string().as_bytes());
    for (path, fact) in &facts {
        let (digest, bytes): (String, u64) = conn
            .query_row(
                "SELECT digest,size FROM files WHERE path=?1",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| &edge.path == path)
            .collect();
        let coverage: Vec<_> = graph
            .coverage
            .iter()
            .filter(|item| &item.path == path)
            .collect();
        let search: Vec<_> = fact
            .nodes
            .iter()
            .map(|node| {
                format!(
                    "{} {} {} {}",
                    node.name, node.qualname, node.path, node.details
                )
            })
            .collect();
        let expected = [
            json_hash(json!([path, digest, bytes])),
            commitments::hash(&serde_json::to_vec(fact).unwrap()),
            json_hash(json!(fact.nodes)),
            json_hash(json!(edges)),
            json_hash(json!(search)),
            json_hash(json!(coverage)),
        ];
        let actual: [String; 6] = conn.query_row(
            "SELECT inventory_hash,facts_hash,nodes_hash,edges_hash,search_hash,deps_hash FROM file_commitments WHERE path=?1",
            [path],
            |row| Ok([row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?]),
        ).unwrap();
        assert_eq!(actual, expected, "file commitment encoding for {path}");
    }
    commitments::verify(&conn).unwrap();
}

fn serial_leaf(conn: &Connection, table: &str, owner: &str, order: &str) -> String {
    use rusqlite::types::ValueRef;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(commitments::ALGORITHM);
    hasher.update(table);
    hasher.update((owner.len() as u64).to_le_bytes());
    hasher.update(owner.as_bytes());
    let ownership = if matches!(table, "nodes" | "edges" | "resolution_coverage") {
        "path"
    } else {
        "owner"
    };
    let mut statement = conn
        .prepare(&format!(
            "SELECT * FROM {table} WHERE {ownership}=?1 ORDER BY {order}"
        ))
        .unwrap();
    let columns = statement.column_count();
    let mut rows = statement.query([owner]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        hasher.update([0xff]);
        for column in 0..columns {
            match row.get_ref(column).unwrap() {
                ValueRef::Null => hasher.update(b"n"),
                ValueRef::Integer(value) => {
                    hasher.update(b"i");
                    hasher.update(value.to_le_bytes());
                }
                ValueRef::Real(value) => {
                    hasher.update(b"r");
                    hasher.update(value.to_bits().to_le_bytes());
                }
                ValueRef::Text(value) | ValueRef::Blob(value) => {
                    hasher.update(
                        if matches!(row.get_ref(column).unwrap(), ValueRef::Text(_)) {
                            b"t"
                        } else {
                            b"b"
                        },
                    );
                    hasher.update((value.len() as u64).to_le_bytes());
                    hasher.update(value);
                }
            }
        }
    }
    hex::encode(hasher.finalize())
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
                "INSERT INTO edges_raw VALUES(?1,'calls',?2,?3,?4,'call','exact',2)",
                params![src, dst, owner, ordinal + 1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO edge_occurrences_raw VALUES(?1,?2,?3,?4,'calls',?5,'call','exact')",
                params![owner, ordinal, src, dst, ordinal + 1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO dependencies_raw VALUES(?1,?2,NULL,'imports',?3,'external')",
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
    for (table, order) in [
        ("nodes", "id"),
        ("edges", "src_public_id,dst_public_id,kind"),
        ("edge_occurrences", "ordinal"),
        ("dependencies", "target,kind,symbol,resolution"),
        ("resolution_coverage", "line,expression,status,evidence"),
    ] {
        for owner in &owners {
            let key = format!("leaf:{}", serde_json::json!([table, owner]));
            let actual: String = conn
                .query_row(
                    "SELECT digest FROM domain_commitments WHERE domain=?1",
                    [key],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                actual,
                serial_leaf(&conn, table, owner, order),
                "{table}: {owner}"
            );
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
