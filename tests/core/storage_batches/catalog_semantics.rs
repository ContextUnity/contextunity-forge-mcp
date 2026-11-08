use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::{
        commitments,
        models::{Facts, Node, NodeSemanticMeta},
    },
    db::{reader, writer},
    engine::{pipeline, scanner},
};
use rusqlite::Connection;
use std::collections::BTreeMap;

#[cfg(feature = "lang-python")]
#[test]
fn lean_symbol_catalog_retains_no_full_facts_in_memory() -> anyhow::Result<()> {
    use contextunity_forge_mcp::{core::semantic::SymbolCatalog, engine::linker};
    let stored_local_facts = |connection: &Connection, path: &str| -> anyhow::Result<Facts> {
        let blob: Vec<u8> = connection.query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get(0),
        )?;
        let mut facts = reader::decode_local_facts(&blob)?;
        facts.nodes.retain(|node| node.kind != "component");
        facts
            .edges
            .retain(|edge| !edge.src.starts_with("component:"));
        Ok(facts)
    };

    let workspace = Workspace::new();
    workspace.write("forge-mcp.yaml", "roots: [src]\ndocs: [README.md]\n");
    workspace.write(
        "src/provider.py",
        "class Service:\n    '''Service documentation.'''\n    def run(self):\n        return 1\n",
    );
    workspace.write(
        "src/consumer.py",
        "from provider import Service\n\ndef caller():\n    service = Service()\n    return service.run()\n",
    );
    workspace.write(
        "README.md",
        "# Service\n\nMUST retain `Service` discovery.\n",
    );

    let adapter = scanner::load_adapter(workspace.root(), None)?;
    let scan = scanner::scan_with_adapter(workspace.root(), &adapter)?;
    let mut full: BTreeMap<_, _> = scan
        .entries
        .iter()
        .map(|entry| {
            Ok((
                entry.path.clone(),
                writer::extract(workspace.root(), entry, &adapter)?,
            ))
        })
        .collect::<anyhow::Result<_>>()?;
    let extracted = full.clone();
    for facts in full.values_mut() {
        for node in &mut facts.nodes {
            node.details["syntax_tree"] = serde_json::json!({"body": "syntax".repeat(1024)});
            node.details["occurrence_token_spans"] = serde_json::json!([[0, 10], [11, 20]]);
        }
    }
    let expected = linker::link(&full);
    let expected_call_edges: Vec<(String, String, String)> = expected
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls")
        .map(|edge| (edge.src.clone(), edge.dst.clone(), edge.kind.clone()))
        .collect();
    let catalog = SymbolCatalog::from_facts(full.clone());
    let actual = linker::link_catalog(&catalog);

    let database = workspace.root().join(".forge/catalog.sqlite");
    let report = pipeline::build_with_cache_dir(
        workspace.root(),
        &database,
        None,
        &workspace.path(".forge/extraction-cache"),
    )?;
    let connection = Connection::open(database)?;
    let catalog_payload_tables: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name='catalog_payloads'",
        [],
        |row| row.get(0),
    )?;
    let persisted_facts = extracted
        .iter()
        .map(|(path, expected)| {
            Ok((
                path.clone(),
                expected.clone(),
                stored_local_facts(&connection, path)?,
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let catalog_commitments_verification = commitments::verify(&connection);

    let routes = Workspace::new();
    routes.write(
        "pyproject.toml",
        "[project]\nname = 'routes'\ndependencies = ['django']\n",
    );
    routes.write(
        "urls.py",
        "from django.urls import path, include\nurlpatterns = [path('shop/', include('shop.urls'))]\n",
    );
    routes.write(
        "shop/urls.py",
        "from django.urls import path, include\nurlpatterns = [path('nested/', include('catalog.urls'))]\n",
    );
    routes.write(
        "catalog/urls.py",
        "from django.urls import path\nfrom catalog.views import endpoint\nurlpatterns = [path('detail/', endpoint, name='detail')]\n",
    );
    routes.write("catalog/views.py", "def endpoint(): return 'handler'\n");
    let route_database = routes.root().join(".forge/catalog.sqlite");
    pipeline::build_with_cache_dir(
        routes.root(),
        &route_database,
        None,
        &routes.path(".forge/extraction-cache"),
    )?;
    let route_connection = Connection::open(route_database)?;
    let route_handles: Vec<(String, String)> = route_connection
        .prepare(
            "SELECT route.name, handler.qualname FROM edge_occurrences edge
             JOIN nodes route ON route.node_hash=edge.src_hash
             JOIN nodes handler ON handler.node_hash=edge.dst_hash
             WHERE edge.kind='handles' ORDER BY route.name",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let route_stored_facts = stored_local_facts(&route_connection, "catalog/urls.py")?;
    let route_catalog = SymbolCatalog::from_facts(BTreeMap::from([(
        "catalog/urls.py".into(),
        route_stored_facts.clone(),
    )]));
    let route_commitments_verification = commitments::verify(&route_connection);

    let expected_graph = serde_json::to_value((&expected.edges, &expected.coverage))?;
    let actual_graph = serde_json::to_value((&actual.edges, &actual.coverage))?;
    let linking_facts_mode = report["linking_facts_mode"].clone();

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
        assert!(expected_call_edges
            .iter()
            .any(|(src, dst, kind)| { src == &caller.id && dst == target && kind == "calls" }));
    }

    let lean_catalog = &catalog;
    assert!(lean_catalog
        .file("src/consumer.py")
        .unwrap()
        .references
        .iter()
        .all(|reference| {
            !matches!(reference.kind.as_str(), "calls" | "references")
                || reference.receiver_hint.is_some()
        }));
    for (path, original) in &full {
        let lean = lean_catalog.file(path).unwrap();
        assert_eq!(
            serde_json::to_value(lean_catalog.references(path).unwrap()).unwrap(),
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
    assert_eq!(actual_graph, expected_graph);
    assert_eq!(linking_facts_mode, "symbol-catalog");
    assert_eq!(catalog_payload_tables, 0);
    for (path, expected, stored) in &persisted_facts {
        assert_eq!(
            serde_json::to_vec(&stored).unwrap(),
            serde_json::to_vec(expected).unwrap(),
            "persisted local facts for {path}"
        );
    }
    assert_eq!(
        route_handles,
        vec![
            ("ANY detail/".into(), "catalog.views.endpoint".into()),
            ("ANY nested/detail/".into(), "catalog.views.endpoint".into()),
            (
                "ANY shop/nested/detail/".into(),
                "catalog.views.endpoint".into()
            ),
        ]
    );
    let stored = &route_stored_facts;
    assert_eq!(
        serde_json::to_value(route_catalog.references("catalog/urls.py").unwrap()).unwrap(),
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
    catalog_commitments_verification
        .as_ref()
        .expect("catalog database commitments verify");
    route_commitments_verification
        .as_ref()
        .expect("route database commitments verify");
    Ok(())
}

#[cfg(feature = "lang-rust")]
#[test]
fn node_semantic_meta_persists_roundtrip_and_matches_delta_equivalence() -> anyhow::Result<()> {
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
    let node_details = |connection: &Connection, name: &str| -> anyhow::Result<serde_json::Value> {
        let details: String =
            connection.query_row("SELECT details FROM nodes WHERE name=?1", [name], |row| {
                row.get(0)
            })?;
        Ok(serde_json::from_str(&details)?)
    };
    let workspace = Workspace::new();
    workspace.write(
        "src/models.py",
        "class Service:\n    def execute(self, value: int) -> str:\n        result = value\n        result = str(result)\n        return result\n",
    );
    workspace.write("src/util.py", "def helper() -> int:\n    return 42\n");

    let cold_db = workspace.root().join(".forge/cold.sqlite");
    pipeline::build_with_cache_dir(
        workspace.root(),
        &cold_db,
        None,
        &workspace.path(".forge/extraction-cache"),
    )?;
    let cold_connection = Connection::open(&cold_db)?;
    let cold_details = node_details(&cold_connection, "execute")?;

    workspace.write(
        "src/models.py",
        "class Service:\n    def execute(self, value: int) -> str:\n        result = value\n        result = str(result)\n        return f'processed:{result}'\n",
    );
    pipeline::delta(
        workspace.root(),
        &cold_db,
        &[std::path::PathBuf::from("src/models.py")],
    )?;
    let delta_details = node_details(&cold_connection, "execute")?;

    let fresh_db = workspace.root().join(".forge/fresh_cold.sqlite");
    pipeline::build_with_cache_dir(
        workspace.root(),
        &fresh_db,
        None,
        &workspace.path(".forge/extraction-cache"),
    )?;
    let fresh_connection = Connection::open(&fresh_db)?;
    let fresh_details = node_details(&fresh_connection, "execute")?;

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
    Ok(())
}

#[test]
fn bundled_framework_manifests_are_fingerprinted_in_dependency_digest() -> anyhow::Result<()> {
    let workspace = Workspace::new();
    let adapter = scanner::load_adapter(workspace.root(), None)?;
    let registry = contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect_with_scan_config(
        workspace.root(),
        Some(&adapter),
    );
    let registry_digest = registry.digest().to_owned();
    let manifests_digest = registry.standard_manifests_digest().to_owned();

    let empty_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_ne!(
        registry_digest, empty_sha256,
        "dependency registry digest must include bundled framework rule manifests"
    );
    assert!(
        !manifests_digest.is_empty(),
        "registry must expose a deterministic standard manifests digest"
    );
    Ok(())
}
