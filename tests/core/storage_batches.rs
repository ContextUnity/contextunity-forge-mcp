use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::models::{Facts, Node, NodeSemanticMeta},
    db::{reader, writer},
    engine::{pipeline, scanner},
};
use rusqlite::Connection;
use std::collections::BTreeMap;

#[cfg(feature = "lang-python")]
#[test]
fn lean_symbol_catalog_retains_no_full_facts_in_memory() {
    use contextunity_forge_mcp::{core::semantic::SymbolCatalog, engine::linker};

    let ws = Workspace::new();
    ws.write("forge-mcp.yaml", "roots: [src]\ndocs: [README.md]\n");
    ws.write(
        "src/provider.py",
        "class Service:\n    '''Service documentation.'''\n    def run(self):\n        return 1\n",
    );
    ws.write(
        "src/consumer.py",
        "from provider import Service\n\ndef caller():\n    service = Service()\n    return service.run()\n",
    );
    ws.write(
        "README.md",
        "# Service\n\nMUST retain `Service` discovery.\n",
    );
    let adapter = scanner::load_adapter(ws.root(), None).unwrap();
    let scan = scanner::scan_with_adapter(ws.root(), &adapter).unwrap();
    let mut full: BTreeMap<_, _> = scan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                writer::extract(ws.root(), entry, &adapter).unwrap(),
            )
        })
        .collect();
    let extracted = full.clone();
    for facts in full.values_mut() {
        for node in &mut facts.nodes {
            node.details["syntax_tree"] = serde_json::json!({"body": "syntax".repeat(1024)});
            node.details["occurrence_token_spans"] = serde_json::json!([[0, 10], [11, 20]]);
        }
    }
    let expected = linker::link(&full);
    let service = full["src/provider.py"]
        .nodes
        .iter()
        .find(|node| node.name == "Service")
        .unwrap();
    let run = full["src/provider.py"]
        .nodes
        .iter()
        .find(|node| node.name == "run")
        .unwrap();
    let caller = full["src/consumer.py"]
        .nodes
        .iter()
        .find(|node| node.name == "caller")
        .unwrap();
    for target in [&service.id, &run.id] {
        assert!(expected
            .edges
            .iter()
            .any(|edge| { edge.src == caller.id && edge.dst == *target && edge.kind == "calls" }));
    }

    let catalog = SymbolCatalog::from_facts(full.clone());
    assert!(catalog
        .file("src/consumer.py")
        .unwrap()
        .references
        .iter()
        .all(|reference| {
            !matches!(reference.kind.as_str(), "calls" | "references")
                || reference.receiver_hint.is_some()
        }));
    for (path, original) in &full {
        let lean = catalog.file(path).unwrap();
        assert_eq!(
            serde_json::to_value(catalog.references(path).unwrap()).unwrap(),
            serde_json::to_value(&original.references).unwrap()
        );
        assert_eq!(lean.nodes.len(), original.nodes.len());
        for (node, original) in lean.nodes.iter().zip(&original.nodes) {
            assert_eq!(
                (&node.id, &node.kind, &node.qualname),
                (&original.id, &original.kind, &original.qualname)
            );
            for key in [
                "namespace",
                "bases",
                "param_types",
                "imports",
                "exports",
                "receiver_type",
            ] {
                assert_eq!(node.details.get(key), original.details.get(key));
            }
            for key in ["doc", "docstring", "syntax_tree", "occurrence_token_spans"] {
                assert!(node.details.get(key).is_none(), "retained {key}");
            }
        }
        for doc in &lean.docs {
            assert!(doc.content.is_empty());
            assert!(doc.invariants.is_empty());
        }
    }
    // This includes constructor calls, the method call inferred from the local
    // binding's typed value flow, import resolution, and document discovery.
    let actual = linker::link_catalog(&catalog);
    assert_eq!(
        serde_json::to_value((&actual.edges, &actual.coverage)).unwrap(),
        serde_json::to_value((&expected.edges, &expected.coverage)).unwrap()
    );

    let database = ws.root().join(".forge/catalog.sqlite");
    let report = pipeline::build(ws.root(), &database, None).unwrap();
    assert_eq!(report["linking_facts_mode"], "symbol-catalog");
    let conn = Connection::open(database).unwrap();
    let payload_tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='catalog_payloads'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        payload_tables, 0,
        "staging payloads must not survive publication"
    );
    for (path, expected) in extracted {
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT facts_blob FROM local_facts WHERE path=?1",
                [&path],
                |row| row.get(0),
            )
            .unwrap();
        let mut stored = reader::decode_local_facts(&blob).unwrap();
        stored.nodes.retain(|node| node.kind != "component");
        stored
            .edges
            .retain(|edge| !edge.src.starts_with("component:"));
        assert_eq!(
            serde_json::to_vec(&stored).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
    }
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();

    let routes = Workspace::new();
    routes.write(
        "pyproject.toml",
        "[project]\nname = 'routes'\ndependencies = ['django']\n",
    );
    routes.write("urls.py", "from django.urls import path, include\nurlpatterns = [path('shop/', include('shop.urls'))]\n");
    routes.write("shop/urls.py", "from django.urls import path, include\nurlpatterns = [path('nested/', include('catalog.urls'))]\n");
    routes.write("catalog/urls.py", "from django.urls import path\nfrom catalog.views import endpoint\nurlpatterns = [path('detail/', endpoint, name='detail')]\n");
    routes.write("catalog/views.py", "def endpoint(): return 'handler'\n");
    let database = routes.root().join(".forge/catalog.sqlite");
    pipeline::build(routes.root(), &database, None).unwrap();
    let conn = Connection::open(database).unwrap();
    let handles: Vec<(String, String)> = conn
        .prepare(
            "SELECT route.name, handler.qualname FROM edge_occurrences edge
         JOIN nodes route ON route.node_hash=edge.src_hash
         JOIN nodes handler ON handler.node_hash=edge.dst_hash
         WHERE edge.kind='handles' ORDER BY route.name",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        handles,
        vec![
            ("ANY detail/".into(), "catalog.views.endpoint".into()),
            ("ANY nested/detail/".into(), "catalog.views.endpoint".into()),
            (
                "ANY shop/nested/detail/".into(),
                "catalog.views.endpoint".into()
            ),
        ]
    );
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path='catalog/urls.py'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stored = reader::decode_local_facts(&blob).unwrap();
    let catalog =
        SymbolCatalog::from_facts(BTreeMap::from([("catalog/urls.py".into(), stored.clone())]));
    assert_eq!(
        serde_json::to_value(catalog.references("catalog/urls.py").unwrap()).unwrap(),
        serde_json::to_value(&stored.references).unwrap(),
        "already composed facts must not duplicate references"
    );
    assert_eq!(
        stored
            .references
            .iter()
            .filter(
                |reference| reference.source.contains(":included:") && reference.kind == "handles"
            )
            .count(),
        2,
        "both composed handlers must survive durable fact restoration"
    );
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_build_preserves_extracted_storage_across_full_and_remainder_batches() {
    use contextunity_forge_mcp::core::{commitments, models::stable_hash64};

    let ws = Workspace::new();
    for index in 0..251 {
        ws.write(
            format!("src/unit_{index:03}.rs"),
            &format!(
                "/// Unicode λ, quotes \"double\", slash \\, and control \0.\npub fn unit_{index:03}() {{}}\n"
            ),
        );
    }
    let markdown: String = (0..251)
        .map(|index| {
            format!(
                "# Section {index:03} λ\n\nMUST preserve \"quotes\", slash \\, and control \0. See `unit_{index:03}`.\n\n"
            )
        })
        .collect();
    ws.write("README.md", &markdown);

    let adapter = scanner::load_adapter(ws.root(), None).unwrap();
    let scan = scanner::scan_with_adapter(ws.root(), &adapter).unwrap();
    let facts: Vec<_> = scan
        .entries
        .iter()
        .map(|entry| writer::extract(ws.root(), entry, &adapter).unwrap())
        .collect();
    assert_eq!(scan.entries.len(), 252);
    assert_eq!(facts.iter().map(|fact| fact.docs.len()).sum::<usize>(), 251);

    let db_path = ws.root().join(".forge/storage.sqlite");
    writer::build(ws.root(), &db_path, None).unwrap();
    let conn = Connection::open(&db_path).unwrap();
    let inventory: Vec<(String, String, String, u64)> = conn
        .prepare("SELECT path,status,digest,size FROM source_inventory ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let expected_inventory: Vec<_> = scan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                "indexed".to_owned(),
                entry.digest.clone(),
                entry.bytes,
            )
        })
        .collect();
    assert_eq!(inventory, expected_inventory);

    type LocalFactsRow = (String, String, String, bool, bool, Vec<u8>, String);
    let local_facts: Vec<LocalFactsRow> = conn
        .prepare("SELECT path,source_digest,language,is_test,generated,facts_blob,typeof(facts_blob) FROM local_facts ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(local_facts.len(), scan.entries.len());
    let mut durable_facts = Vec::<Facts>::new();
    for (entry, (path, digest, language, is_test, generated, blob, storage_type)) in
        scan.entries.iter().zip(local_facts)
    {
        assert_eq!(path, entry.path);
        assert_eq!(digest, entry.digest);
        assert_eq!(language, entry.language);
        assert_eq!(
            is_test,
            contextunity_forge_mcp::core::models::is_test(&entry.path)
        );
        assert!(!generated);
        assert_eq!(storage_type, "blob");
        assert!(blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"));
        assert!(blob["FORGE-LOCAL-FACTS\0\x03".len()..].starts_with(&[0x28, 0xb5, 0x2f, 0xfd]));
        let fact = reader::decode_local_facts(&blob).unwrap();
        durable_facts.push(fact);
    }

    for (index, node) in durable_facts
        .iter()
        .flat_map(|fact| &fact.nodes)
        .enumerate()
    {
        let (node_id, details): (i64, String) = conn
            .query_row(
                "SELECT node_id,details FROM nodes WHERE id=?1",
                [&node.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(node_id, index as i64 + 1);
        let navigation: serde_json::Value = serde_json::from_str(&details).unwrap();
        assert!(
            !navigation
                .as_object()
                .is_some_and(serde_json::Map::is_empty),
            "empty node details must be stored as null for {}",
            node.id
        );
        for key in ["signature", "doc", "receiver_name"] {
            if node.details[key]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            {
                assert_eq!(navigation[key], node.details[key]);
            }
        }
    }
    let durable_nodes: BTreeMap<_, _> = durable_facts
        .iter()
        .flat_map(|fact| &fact.nodes)
        .map(|node| (node.id.as_str(), node))
        .collect();
    for node in facts.iter().flat_map(|fact| &fact.nodes) {
        let actual = durable_nodes.get(node.id.as_str()).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(node).unwrap()
        );
    }

    let doc_rows: Vec<(i64, contextunity_forge_mcp::core::models::DocSection, String)> = conn
        .prepare("SELECT rowid,doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size,is_invariant,typeof(mtime) FROM doc_sections ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            let invariants: String = row.get(6)?;
            let referenced_symbols: String = row.get(7)?;
            Ok((row.get(0)?, contextunity_forge_mcp::core::models::DocSection {
                doc_id: row.get(1)?,
                path: row.get(2)?,
                section_title: row.get(3)?,
                doc_type: row.get(4)?,
                content: row.get(5)?,
                invariants: serde_json::from_str(&invariants).unwrap(),
                referenced_symbols: serde_json::from_str(&referenced_symbols).unwrap(),
                mtime: row.get(8)?,
                size: row.get(9)?,
                is_invariant: row.get(10)?,
            }, row.get(11)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let expected_docs: Vec<_> = facts.iter().flat_map(|fact| &fact.docs).collect();
    assert_eq!(doc_rows.len(), expected_docs.len());
    for (index, ((rowid, actual, storage_type), expected)) in
        doc_rows.iter().zip(expected_docs).enumerate()
    {
        assert_eq!(*rowid, index as i64 + 1);
        assert_eq!(storage_type, "real");
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    let shared_keys: Vec<(i64, String)> = conn
        .prepare("SELECT k.key_hash,CASE WHEN typeof(k.key)='integer' THEN n.qualname ELSE k.key END FROM shared_keys k LEFT JOIN nodes n ON n.node_id=k.key AND typeof(k.key)='integer' ORDER BY k.key_hash")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(shared_keys.len() > 250);
    let keys_by_text: BTreeMap<_, _> = shared_keys
        .iter()
        .map(|(hash, key)| (key.as_str(), *hash))
        .collect();
    for node in facts.iter().flat_map(|fact| &fact.nodes) {
        assert_eq!(
            keys_by_text.get(node.qualname.as_str()),
            Some(&stable_hash64(&node.qualname))
        );
    }
    for (hash, key) in shared_keys {
        assert_eq!(hash, stable_hash64(&key));
    }
    commitments::verify(&conn).unwrap();
}

#[test]
fn cli_memory_budget_precedence_fallback_and_rejection_are_observable() {
    use std::{path::Path, process::Command};

    fn configure(ws: &Workspace, memory_budget_mb: Option<&str>) {
        let limits = memory_budget_mb.map_or_else(String::new, |value| {
            format!("limits:\n  memory_budget_mb: {value}\n")
        });
        ws.write(
            "forge-mcp.yaml",
            &format!("roots: [src]\ndocs: []\n{limits}"),
        );
    }

    fn run_build(
        ws: &Workspace,
        name: &str,
        env_budget_mb: Option<&str>,
    ) -> (std::process::Output, std::path::PathBuf) {
        let database = ws.root().join(".forge").join(format!("{name}.sqlite"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
        command
            .arg("--format")
            .arg("json")
            .arg("build")
            .arg(ws.root())
            .arg("--output")
            .arg(&database)
            .env_remove("FORGE_MEMORY_BUDGET_MB");
        if let Some(value) = env_budget_mb {
            command.env("FORGE_MEMORY_BUDGET_MB", value);
        }
        (command.output().unwrap(), database)
    }

    fn success_report(output: &std::process::Output) -> serde_json::Value {
        assert!(
            output.status.success(),
            "CLI build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn persisted_commitments(database: &Path) -> Vec<(String, i64, String)> {
        let connection = Connection::open(database).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT domain,owner_id,lower(hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
            )
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn assert_cli_rejects(
        ws: &Workspace,
        name: &str,
        yaml_budget_mb: Option<&str>,
        env_budget_mb: Option<&str>,
    ) {
        configure(ws, yaml_budget_mb);
        let (output, database) = run_build(ws, name, env_budget_mb);
        assert!(
            !output.status.success(),
            "invalid budget unexpectedly built"
        );
        assert!(
            !database.exists(),
            "invalid budget published a database: {}",
            database.display()
        );
    }

    let ws = Workspace::new();
    ws.write("src/lib.rs", "pub fn stable() -> u8 { 1 }\n");

    configure(&ws, Some("50"));
    let (yaml_output, yaml_database) = run_build(&ws, "yaml", Some("1000"));
    let yaml_report = success_report(&yaml_output);
    assert_eq!(
        yaml_report["memory_budget_bytes"].as_u64(),
        Some(50 * 1024 * 1024)
    );

    configure(&ws, None);
    let (env_output, env_database) = run_build(&ws, "environment", Some("200"));
    let env_report = success_report(&env_output);
    assert_eq!(
        env_report["memory_budget_bytes"].as_u64(),
        Some(200 * 1024 * 1024)
    );
    let (env_high_output, env_high_database) = run_build(&ws, "environment-high", Some("1000"));
    let env_high_report = success_report(&env_high_output);
    assert_eq!(
        env_high_report["memory_budget_bytes"].as_u64(),
        Some(1000 * 1024 * 1024)
    );
    assert_eq!(
        env_report["output_root"], env_high_report["output_root"],
        "the transient environment budget must not alter the adapter policy fingerprint"
    );

    let (adaptive_output, adaptive_database) = run_build(&ws, "adaptive", None);
    let adaptive_report = success_report(&adaptive_output);
    let adaptive = adaptive_report["memory_budget_bytes"].as_u64().unwrap();
    assert!((64 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&adaptive));

    let yaml_commitments = persisted_commitments(&yaml_database);
    assert_eq!(yaml_commitments, persisted_commitments(&env_database));
    assert_eq!(yaml_commitments, persisted_commitments(&env_high_database));
    assert_eq!(yaml_commitments, persisted_commitments(&adaptive_database));

    assert_cli_rejects(&ws, "invalid-environment-text", None, Some("many"));
    assert_cli_rejects(&ws, "invalid-environment-zero", None, Some("0"));
    assert_cli_rejects(
        &ws,
        "invalid-environment-overflow",
        None,
        Some("18446744073709551615"),
    );
    assert_cli_rejects(&ws, "invalid-yaml-text", Some("many"), Some("200"));
    assert_cli_rejects(&ws, "invalid-yaml-zero", Some("0"), Some("200"));
    assert_cli_rejects(
        &ws,
        "invalid-yaml-overflow",
        Some("18446744073709551615"),
        Some("200"),
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn public_extraction_rejects_a_file_entry_digest_mismatch() {
    let ws = Workspace::new();
    ws.write("src/lib.rs", "pub fn current() -> u8 { 1 }\n");
    let adapter = scanner::load_adapter(ws.root(), None).unwrap();
    let mut entry = scanner::scan_with_adapter(ws.root(), &adapter)
        .unwrap()
        .entries
        .into_iter()
        .find(|entry| entry.path == "src/lib.rs")
        .unwrap();
    entry.digest = "deliberately-stale-digest".to_owned();

    let error = writer::extract(ws.root(), &entry, &adapter).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("source changed during extraction"),
        "stale public extraction must fail closed: {error:#}"
    );
}

#[test]
fn node_semantic_meta_persists_roundtrip_and_matches_delta_equivalence() {
    use serde_json::json;

    let meta = NodeSemanticMeta {
        bindings: vec!["foo".into(), "bar".into()],
        rebindings: vec!["baz".into()],
        receiver: Some("(s *Service)".into()),
        receiver_type: Some("Service".into()),
        receiver_hint: Some("Service".into()),
        param_types: std::collections::BTreeMap::from([("arg".into(), "str".into())]),
    };

    let serialized = serde_json::to_vec(&meta).expect("serialization succeeds");
    let deserialized: NodeSemanticMeta =
        serde_json::from_slice(&serialized).expect("deserialization succeeds");
    assert_eq!(meta, deserialized);

    let mut node = Node {
        native_semantic: None,
        id: "func:foo".into(),
        path: "src/foo.go".into(),
        language: "go".into(),
        kind: "method".into(),
        name: "foo".into(),
        qualname: "Service.foo".into(),
        line: 1,
        end_line: 5,
        is_test: false,
        generated: false,
        details: json!({
            "receiver": "(s *Service)",
            "receiver_type": "Service"
        }),
    };
    node.apply_semantic_meta(meta.clone());
    let extracted_meta = node.semantic_meta();
    assert_eq!(meta, extracted_meta);
    assert_eq!(node.details["receiver"], "(s *Service)");
    assert_eq!(node.details["receiver_type"], "Service");

    // Verify clearing metadata with default
    node.apply_semantic_meta(NodeSemanticMeta::default());
    let cleared_meta = node.semantic_meta();
    assert_eq!(cleared_meta, NodeSemanticMeta::default());
    assert!(node.details.get("bindings").is_none());
    assert!(node.details.get("rebindings").is_none());
    assert!(node.details.get("receiver").is_none());
    assert!(node.details.get("receiver_type").is_none());
    assert!(node.details.get("receiver_hint").is_none());
    assert!(node.details.get("param_types").is_none());

    // Verify hint-only metadata roundtrip without inventing receiver or receiver_type
    let mut node2 = Node {
        native_semantic: None,
        id: "func:bar".into(),
        path: "src/bar.py".into(),
        language: "python".into(),
        kind: "function".into(),
        name: "bar".into(),
        qualname: "bar".into(),
        line: 1,
        end_line: 5,
        is_test: false,
        generated: false,
        details: serde_json::Value::Null,
    };
    let empty_details_node = Node {
        details: json!({}),
        ..node2.clone()
    };
    assert_eq!(
        serde_json::to_value(empty_details_node).unwrap()["details"],
        serde_json::Value::Null
    );
    let hint_meta = NodeSemanticMeta {
        receiver_hint: Some("Service".into()),
        ..Default::default()
    };
    node2.apply_semantic_meta(hint_meta.clone());
    assert_eq!(node2.semantic_meta(), hint_meta);
    assert_eq!(node2.details["receiver_hint"], "Service");
    assert!(node2.details.get("receiver").is_none());
    assert!(node2.details.get("receiver_type").is_none());

    // Exercise real cold build and delta persistence to prove SQLite round-trip
    // and cold-versus-delta equivalence on extracted semantic metadata.
    let ws = Workspace::new();
    ws.write(
        "src/models.py",
        "class Service:\n    def execute(self, value: int) -> str:\n        result = value\n        result = str(result)\n        return result\n",
    );
    ws.write("src/util.py", "def helper() -> int:\n    return 42\n");

    let cold_db = ws.root().join(".forge/cold.sqlite");
    pipeline::build(ws.root(), &cold_db, None).expect("cold build succeeds");

    let conn = Connection::open(&cold_db).expect("open cold_db succeeds");
    let cold_details_str: String = conn
        .query_row(
            "SELECT details FROM nodes WHERE name = 'execute'",
            [],
            |row| row.get(0),
        )
        .expect("query cold details succeeds");
    let cold_details: serde_json::Value =
        serde_json::from_str(&cold_details_str).expect("parse cold details succeeds");
    assert_eq!(cold_details["param_types"]["value"], "int");
    assert!(cold_details["bindings"].as_array().is_some());
    assert!(cold_details["rebindings"].as_array().is_some());

    let mut cold_node = Node {
        native_semantic: None,
        id: "func:Service.execute".into(),
        path: "src/models.py".into(),
        language: "python".into(),
        kind: "method".into(),
        name: "execute".into(),
        qualname: "Service.execute".into(),
        line: 2,
        end_line: 3,
        is_test: false,
        generated: false,
        details: cold_details.clone(),
    };
    let cold_meta = cold_node.semantic_meta();
    assert_eq!(cold_meta.receiver_type.as_deref(), Some("Service"));

    // Verify persisted metadata roundtrip through apply_semantic_meta
    cold_node.apply_semantic_meta(cold_meta.clone());
    assert_eq!(cold_node.semantic_meta(), cold_meta);

    // Modify file and run delta indexing
    ws.write(
        "src/models.py",
        "class Service:\n    def execute(self, value: int) -> str:\n        result = value\n        result = str(result)\n        return f'processed:{result}'\n",
    );
    pipeline::delta(
        ws.root(),
        &cold_db,
        &[std::path::PathBuf::from("src/models.py")],
    )
    .expect("delta indexing succeeds");

    let delta_details_str: String = conn
        .query_row(
            "SELECT details FROM nodes WHERE name = 'execute'",
            [],
            |row| row.get(0),
        )
        .expect("query delta details succeeds");
    let delta_details: serde_json::Value =
        serde_json::from_str(&delta_details_str).expect("parse delta details succeeds");
    for key in ["bindings", "rebindings", "param_types"] {
        assert_eq!(
            delta_details[key], cold_details[key],
            "{key} changed in delta"
        );
    }
    let delta_node = Node {
        native_semantic: None,
        id: "func:Service.execute".into(),
        path: "src/models.py".into(),
        language: "python".into(),
        kind: "method".into(),
        name: "execute".into(),
        qualname: "Service.execute".into(),
        line: 2,
        end_line: 3,
        is_test: false,
        generated: false,
        details: delta_details,
    };
    let delta_meta = delta_node.semantic_meta();

    // Fresh cold build for comparison
    let fresh_cold_db = ws.root().join(".forge/fresh_cold.sqlite");
    pipeline::build(ws.root(), &fresh_cold_db, None).expect("fresh cold build succeeds");
    let fresh_conn = Connection::open(&fresh_cold_db).expect("open fresh_cold_db succeeds");
    let fresh_details_str: String = fresh_conn
        .query_row(
            "SELECT details FROM nodes WHERE name = 'execute'",
            [],
            |row| row.get(0),
        )
        .expect("query fresh details succeeds");
    let fresh_details: serde_json::Value =
        serde_json::from_str(&fresh_details_str).expect("parse fresh details succeeds");
    for key in ["bindings", "rebindings", "param_types"] {
        assert_eq!(
            fresh_details[key], cold_details[key],
            "{key} changed in cold build"
        );
    }
    let fresh_node = Node {
        native_semantic: None,
        id: "func:Service.execute".into(),
        path: "src/models.py".into(),
        language: "python".into(),
        kind: "method".into(),
        name: "execute".into(),
        qualname: "Service.execute".into(),
        line: 2,
        end_line: 3,
        is_test: false,
        generated: false,
        details: fresh_details,
    };
    let fresh_meta = fresh_node.semantic_meta();

    // Exact cold-versus-delta equivalence
    assert_eq!(delta_meta, fresh_meta);
}

#[test]
fn bundled_framework_manifests_are_fingerprinted_in_dependency_digest() {
    let ws = Workspace::new();
    let adapter = scanner::load_adapter(ws.root(), None).unwrap();
    let registry = contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect_with_scan_config(
        ws.root(),
        Some(&adapter),
    );

    let empty_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_ne!(
        registry.digest(),
        empty_sha256,
        "dependency registry digest must include bundled framework rule manifests"
    );
    assert!(
        !registry.standard_manifests_digest().is_empty(),
        "registry must expose a deterministic standard manifests digest"
    );
}
