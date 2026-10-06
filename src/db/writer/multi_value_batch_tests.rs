use super::{
    insert_multi_value_batch, multi_value_batch_rows, persist_files, persist_graph,
    prefill_coverage_dictionary, CoverageEvidenceCache, CoverageExpressionCache,
    PathDictionaryCache, SharedKeyCache,
};
use crate::{
    core::{
        models::compact_graph::{Coverage, Edge, Graph},
        models::{Facts, Node, Reference},
        schema::SCHEMA_DDL,
        typed_facts::TypedFacts,
    },
    engine::scanner::FileEntry,
};
use rusqlite::{limits::Limit, types::Value as SqlValue, Connection};
use serde_json::json;
use std::collections::BTreeMap;

fn entries_and_facts(count: usize) -> (Vec<FileEntry>, BTreeMap<String, TypedFacts>) {
    let mut entries = Vec::with_capacity(count);
    let mut facts = BTreeMap::new();
    for index in 0..count {
        let path = format!("src/file-{index:03}.rs");
        entries.push(FileEntry {
            path: path.clone(),
            digest: format!("digest-{index}"),
            bytes: 1,
            mtime_ns: 0,
            identity: [0; 3],
            language: "rust".into(),
            is_doc: false,
        });
        facts.insert(
            path.clone(),
            TypedFacts::from_public(Facts {
                nodes: vec![Node {
                    id: format!("function:{path}"),
                    kind: "function".into(),
                    name: format!("function_{index}"),
                    qualname: format!("function_{index}"),
                    path,
                    line: 1,
                    end_line: 1,
                    is_test: false,
                    language: "rust".into(),
                    generated: false,
                    details: json!({}),
                }],
                ..Facts::default()
            }),
        );
    }
    (entries, facts)
}

fn constrain_variable_limit(conn: &Connection) -> i32 {
    let limit = conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER);
    assert!(limit >= 15, "test needs enough variables for one node row");
    let constrained = limit.min(52);
    conn.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, constrained);
    assert_eq!(conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER), constrained);
    constrained
}

fn row_count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn endpoint_hash_cache_preserves_targets_and_missing_endpoint_errors() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE nodes(id TEXT PRIMARY KEY,path TEXT); INSERT INTO nodes VALUES('node:λ','src/λ.rs');").unwrap();
    let mut statement = conn.prepare("SELECT path FROM nodes WHERE id=?1").unwrap();
    let mut cache = hashbrown::HashMap::new();
    assert_eq!(
        super::graph_endpoint_hashes(&mut cache, &mut statement, "node:λ", "other.rs", "calls")
            .unwrap(),
        (
            crate::core::models::stable_hash64("node:λ"),
            Some(crate::core::models::stable_hash64("src/λ.rs"))
        )
    );
    let missing =
        super::graph_endpoint_hashes(&mut cache, &mut statement, "missing", "other.rs", "calls")
            .unwrap_err()
            .to_string();
    assert_eq!(missing, "edge calls has missing endpoint missing");
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Read {
            table_name: "nodes",
            ..
        } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
    assert_eq!(
        super::graph_endpoint_hashes(&mut cache, &mut statement, "node:λ", "src/λ.rs", "calls")
            .unwrap(),
        (crate::core::models::stable_hash64("node:λ"), None)
    );
    assert_eq!(
        super::graph_endpoint_hashes(&mut cache, &mut statement, "missing", "other.rs", "calls")
            .unwrap_err()
            .to_string(),
        missing
    );
}

#[test]
fn cold_dictionary_batches_match_first_seen_ids_and_delta_keeps_existing_ids() {
    let values = (0..251)
        .flat_map(|index| [format!("callee_{index}_λ'"), "duplicate".into()])
        .collect::<Vec<_>>();
    let old = Connection::open_in_memory().unwrap();
    old.execute_batch(SCHEMA_DDL).unwrap();
    let cold = Connection::open_in_memory().unwrap();
    cold.execute_batch(SCHEMA_DDL).unwrap();
    constrain_variable_limit(&cold);
    let mut expressions = CoverageExpressionCache::new(&old).unwrap();
    let mut evidence = CoverageEvidenceCache::new(&old).unwrap();
    for value in &values {
        expressions.get_or_insert(value).unwrap();
        evidence.get_or_insert(value).unwrap();
    }
    let cold_expressions = prefill_coverage_dictionary(
        &cold,
        "coverage_expressions",
        "expression",
        "INSERT INTO coverage_expressions(expression_id,expression) VALUES",
        values.iter().map(String::as_str),
    )
    .unwrap();
    let cold_evidence = prefill_coverage_dictionary(
        &cold,
        "coverage_evidence",
        "evidence",
        "INSERT INTO coverage_evidence(evidence_id,evidence) VALUES",
        values.iter().map(String::as_str),
    )
    .unwrap();
    assert_eq!(
        cold_expressions,
        expressions
            .cache
            .iter()
            .map(|(value, id)| (value.as_str(), *id))
            .collect()
    );
    assert_eq!(
        cold_evidence,
        evidence
            .cache
            .iter()
            .map(|(value, id)| (value.as_str(), *id))
            .collect()
    );
    for table in ["coverage_expressions", "coverage_evidence"] {
        let sql = format!("SELECT * FROM {table} ORDER BY 1");
        assert_eq!(
            crate::db::reader::rows(&old, &sql, &[], 1_000).unwrap(),
            crate::db::reader::rows(&cold, &sql, &[], 1_000).unwrap()
        );
    }
    cold.execute(
        "INSERT INTO coverage_expressions VALUES(999,'existing expression')",
        [],
    )
    .unwrap();
    cold.execute(
        "INSERT INTO coverage_evidence VALUES(999,'existing evidence')",
        [],
    )
    .unwrap();
    let mut expressions = CoverageExpressionCache::new(&cold).unwrap();
    let mut evidence = CoverageEvidenceCache::new(&cold).unwrap();
    assert_eq!(
        expressions.get_or_insert("existing expression").unwrap(),
        999
    );
    assert_eq!(evidence.get_or_insert("existing evidence").unwrap(), 999);
    assert_eq!(expressions.get_or_insert("new expression").unwrap(), 1_000);
    assert_eq!(evidence.get_or_insert("new evidence").unwrap(), 1_000);
}

#[test]
fn cold_shared_key_batches_preserve_keys_collisions_and_delta_lookup() {
    let old = Connection::open_in_memory().unwrap();
    old.execute_batch(SCHEMA_DDL).unwrap();
    let cold = Connection::open_in_memory().unwrap();
    cold.execute_batch(SCHEMA_DDL).unwrap();
    constrain_variable_limit(&cold);
    let mut original = SharedKeyCache::new(&old, false).unwrap();
    let mut batched = SharedKeyCache::new_batched(&cold).unwrap();
    for index in 0..251 {
        let key = format!("symbol_{index}_λ'");
        assert_eq!(
            original.get_or_insert(&key).unwrap(),
            batched.get_or_insert(&key).unwrap()
        );
        assert_eq!(
            batched.get_or_insert(&key).unwrap(),
            crate::core::models::stable_hash64(&key)
        );
    }
    batched.flush().unwrap();
    let sql = "SELECT * FROM shared_keys ORDER BY key_hash";
    assert_eq!(
        crate::db::reader::rows(&old, sql, &[], 1_000).unwrap(),
        crate::db::reader::rows(&cold, sql, &[], 1_000).unwrap()
    );
    let key = "collision probe";
    batched.hash_to_key.insert(
        crate::core::models::stable_hash64(key),
        "different key".into(),
    );
    assert!(batched
        .get_or_insert(key)
        .unwrap_err()
        .to_string()
        .contains("collision"));
    drop(batched);
    let mut delta = SharedKeyCache::new(&cold, true).unwrap();
    assert_eq!(
        delta.get_or_insert("symbol_0_λ'").unwrap(),
        crate::core::models::stable_hash64("symbol_0_λ'")
    );
    assert_eq!(row_count(&cold, "shared_keys"), 251);
}

#[test]
fn semantic_dependencies_include_dynamic_receivers_without_scanning_dynamic_text() {
    let (_, mut files) = entries_and_facts(1);
    let facts = files.values_mut().next().unwrap();
    facts.nodes[0].details = json!({
        "value_flow":{"return_type":{"kind":"named","name":"pkg.Worker"}},
        "exports":[{"local":"factory", "module":"./provider"}],
        "param_types":{"client":"ParamType"}
    });
    facts.flows.ingest(&mut facts.facts.nodes[0]);
    let source = facts.nodes[0].id.clone();
    facts.references.push(Reference {
        source,
        expression: "untrusted.dynamic.text".into(),
        kind: "calls".into(),
        line: 2,
        column: 0,
        alias: None,
        module: None,
        dynamic: true,
        receiver_hint: Some(crate::core::models::ReceiverHint::ConstructorResult {
            callee: "pkg.Factory".into(),
            member: "run".into(),
        }),
    });
    let keys = super::reference_keys(facts);
    for key in [
        "pkg.Worker",
        "Worker",
        "pkg.Factory",
        "Factory",
        "factory",
        "./provider",
        "provider",
        "ParamType",
    ] {
        assert!(keys.contains(&key), "{key}");
    }
    assert!(!keys.contains(&"untrusted.dynamic.text"));
}

#[test]
fn graph_owner_and_commitment_batches_preserve_rows_across_variable_limits() {
    let (entries, mut facts) = entries_and_facts(251);
    let mut graph = Graph::default();
    for (index, file) in entries.iter().enumerate() {
        let f = facts.get_mut(&file.path).unwrap();
        f.nodes[0].details = json!({"default_export":true});
        let source = f.nodes[0].id.clone();
        f.references.push(Reference {
            source,
            expression: "remote.call".into(),
            kind: "calls".into(),
            line: 2,
            column: 0,
            alias: None,
            module: None,
            dynamic: false,
            receiver_hint: None,
        });
        graph.edges.push(Edge {
            src: f.nodes[0].id.clone(),
            dst: format!("function:{}", entries[(index + 1) % entries.len()].path),
            kind: "calls".into(),
            path: file.path.clone(),
            line: 2,
            evidence: "cross-file call λ'\0".into(),
            confidence: "exact".into(),
        });
        for status in [
            "resolved",
            "unresolved",
            "ambiguous",
            "external",
            "external",
        ] {
            graph.coverage.push(Coverage {
                path: file.path.clone(),
                line: 3,
                expression: format!("call_{status}_λ'\0"),
                status: status.into(),
                evidence: format!("evidence {status}"),
            });
        }
    }
    let snapshot = |constrained| {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_DDL).unwrap();
        if constrained {
            constrain_variable_limit(&conn);
        }
        let tx = conn.transaction().unwrap();
        let mut path_cache = PathDictionaryCache::new();
        persist_files(&tx, &mut path_cache, &entries, &facts, None, true).unwrap();
        let mut node_paths: hashbrown::HashMap<_, _> = facts
            .values()
            .flat_map(|f| f.nodes.iter())
            .map(|n| {
                (
                    n.id.clone(),
                    Some(super::CachedNodePath::new(&n.id, n.path.clone())),
                )
            })
            .collect();
        persist_graph(&tx, &mut path_cache, &graph, &facts, &mut node_paths, true).unwrap();
        tx.commit().unwrap();
        assert_eq!(row_count(&conn, "resolution_coverage"), 251 * 4);
        assert_eq!(row_count(&conn, "dependencies"), 251 * 4);
        assert_eq!(row_count(&conn, "shared_owners"), 251 * 5);
        assert_eq!(row_count(&conn, "edge_occurrences"), 251);
        assert_eq!(row_count(&conn, "edges"), 251);
        let dependencies = crate::db::reader::rows(&conn,
                "SELECT ordinal,target_hash,kind,symbol,resolution FROM dependencies WHERE owner_id=(SELECT path_id FROM path_dictionary WHERE path='src/file-000.rs') ORDER BY ordinal", &[], 10).unwrap();
        assert_eq!(dependencies.len(), 4);
        for (ordinal, dependency) in dependencies.iter().enumerate() {
            assert_eq!(dependency["ordinal"], ordinal);
        }
        let resolved = dependencies
            .iter()
            .find(|row| row["resolution"] == "resolved")
            .unwrap();
        assert!(resolved["target_hash"].as_i64().is_some());
        assert_eq!(resolved["kind"], "calls");
        assert_eq!(resolved["symbol"], "cross-file call λ'\0");
        for status in ["unresolved", "ambiguous", "external"] {
            let dependency = dependencies
                .iter()
                .find(|row| row["resolution"] == status)
                .unwrap();
            assert!(dependency["target_hash"].is_null());
            assert_eq!(dependency["kind"], "unresolved");
            assert_eq!(dependency["symbol"], format!("call_{status}_λ'\0"));
        }
        [
            "edge_occurrences",
            "edges",
            "shared_owners",
            "resolution_coverage",
            "dependencies",
        ]
        .map(|table| {
            crate::db::reader::rows(
                &conn,
                &format!("SELECT * FROM {table} ORDER BY 1,2,3,4"),
                &[],
                10_000,
            )
            .unwrap()
        })
    };
    assert_eq!(snapshot(false), snapshot(true));
}

#[test]
fn multi_value_helper_splits_at_the_live_sqlite_variable_limit() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE pairs(left_value INTEGER,right_value INTEGER);")
        .unwrap();
    conn.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 6);
    assert_eq!(conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER), 6);
    assert_eq!(multi_value_batch_rows::<2>(&conn).unwrap(), 3);

    let batch = (0..8)
        .map(|value| [SqlValue::Integer(value), SqlValue::Integer(value * 10)])
        .collect::<Vec<_>>();
    insert_multi_value_batch(&conn, "INSERT INTO pairs VALUES", &batch).unwrap();
    assert_eq!(row_count(&conn, "pairs"), 8);
    assert_eq!(
        conn.query_row("SELECT sum(right_value) FROM pairs", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        280
    );
}

#[test]
fn cold_file_node_and_search_inserts_respect_live_batch_boundaries() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA_DDL).unwrap();
    let variable_limit = constrain_variable_limit(&conn) as usize;
    let rows_by_columns =
        [8, 15, 2].map(|columns| (variable_limit / columns).min(super::MAX_MULTI_VALUE_BATCH_ROWS));
    assert!(rows_by_columns
        .iter()
        .all(|rows| (1..=super::MAX_MULTI_VALUE_BATCH_ROWS).contains(rows)));
    assert_eq!(
        multi_value_batch_rows::<8>(&conn).unwrap(),
        rows_by_columns[0]
    );
    assert_eq!(
        multi_value_batch_rows::<15>(&conn).unwrap(),
        rows_by_columns[1]
    );
    assert_eq!(
        multi_value_batch_rows::<2>(&conn).unwrap(),
        rows_by_columns[2]
    );

    let (entries, facts) = entries_and_facts(251);
    let tx = conn.transaction().unwrap();
    let mut path_cache = PathDictionaryCache::new();
    persist_files(&tx, &mut path_cache, &entries, &facts, None, true).unwrap();
    tx.commit().unwrap();

    for table in [
        "source_inventory",
        "files",
        "local_facts",
        "nodes",
        "node_search",
    ] {
        assert_eq!(row_count(&conn, table), 251, "{table}");
    }
}

#[test]
fn cold_batch_failure_rolls_back_previously_flushed_rows() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA_DDL).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_selected_shared_owner
             BEFORE INSERT ON shared_owners
             WHEN (SELECT path FROM path_dictionary WHERE path_id=NEW.owner_id)='src/file-250.rs'
             BEGIN SELECT RAISE(ABORT,'forced cold-batch failure'); END;",
    )
    .unwrap();
    constrain_variable_limit(&conn);
    let (entries, facts) = entries_and_facts(251);

    let error = {
        let tx = conn.transaction().unwrap();
        let mut path_cache = PathDictionaryCache::new();
        let error = persist_files(&tx, &mut path_cache, &entries, &facts, None, true)
            .expect_err("the injected late failure must abort persistence");
        drop(tx);
        error
    };
    assert!(
        error.to_string().contains("forced cold-batch failure"),
        "{error:#}"
    );

    for table in [
        "source_inventory",
        "files",
        "path_dictionary",
        "local_facts",
        "nodes",
        "node_search",
        "shared_keys",
        "shared_owners",
    ] {
        assert_eq!(row_count(&conn, table), 0, "{table} must roll back");
    }
}
