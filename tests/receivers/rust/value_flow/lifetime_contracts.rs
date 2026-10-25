use super::super::*;

#[test]
fn rust_lifetime_only_nominals_preserve_imported_receiver_contracts() {
    let source = "use tree_sitter::Node;\nfn run<'a>(node: &Node<'a>, owned: Node<'_>, mixed: Vec<Node<'a>>, generic: Node<u8>, counted: Node<'a, 3>) {\n    node.kind(); owned.child_by_field_name(\"name\"); generic.kind(); counted.kind();\n}\nfn copy<'a>(node: Node<'a>) -> Node<'a> { node }";
    let facts = ast::extract("src/nominals.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, run);
    let bindings = details["value_flow"]["bindings"].as_array().unwrap();
    for (name, expected) in [
        (
            "node",
            serde_json::json!({
                "kind":"applied",
                "base":"&",
                "args":[{"kind":"named","name":"Node"}]
            }),
        ),
        ("owned", serde_json::json!({"kind":"named","name":"Node"})),
    ] {
        let binding = bindings
            .iter()
            .find(|binding| binding["name"] == name)
            .unwrap();
        assert_eq!(binding["value"]["type_expr"], expected);
    }
    let generic = bindings
        .iter()
        .find(|binding| binding["name"] == "generic")
        .unwrap();
    assert_eq!(generic["value"]["type_expr"]["base"], "Node");
    assert_eq!(generic["value"]["type_expr"]["args"][0]["name"], "u8");
    let mixed = bindings
        .iter()
        .find(|binding| binding["name"] == "mixed")
        .unwrap();
    assert_eq!(mixed["value"]["type_expr"]["base"], "Vec");
    assert_eq!(mixed["value"]["type_expr"]["args"][0]["name"], "Node");
    let counted = bindings
        .iter()
        .find(|binding| binding["name"] == "counted")
        .unwrap();
    assert_eq!(counted["value"]["type_expr"]["base"], "Node");
    assert_eq!(counted["value"]["type_expr"]["args"][0]["kind"], "unknown");
    let copy = facts.nodes.iter().find(|node| node.name == "copy").unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, copy);
    assert_eq!(details["value_flow"]["return_type"]["name"], "Node");
    let restored = serde_json::from_slice(&serde_json::to_vec(&facts).unwrap()).unwrap();
    for facts in [facts, restored] {
        let graph = linker::link(&BTreeMap::from([("src/nominals.rs".to_owned(), facts)]));
        for expression in ["node.kind", "owned.child_by_field_name"] {
            assert!(
                graph.coverage.iter().any(|row| row.expression == expression
                    && row.status == "external"
                    && row.evidence.contains("tree_sitter::Node at line 1")),
                "{graph:#?}"
            );
        }
        for expression in ["generic.kind", "counted.kind"] {
            assert!(
                graph
                    .coverage
                    .iter()
                    .any(|row| row.expression == expression && row.status == "unresolved"),
                "{graph:#?}"
            );
        }
    }
}

#[test]
fn rust_lifetime_nominal_factory_contracts_survive_cache_and_delta() {
    use crate::common::Workspace;
    use contextunity_forge_mcp::{core::commitments, db::reader};

    let workspace = Workspace::new();
    workspace.write(
        "src/provider.rs",
        "use tree_sitter::Node;\npub fn identity<'a>(node: Node<'a>) -> Node<'a> { node }",
    );
    workspace.write(
        "src/client.rs",
        "use crate::provider::identity;\nuse tree_sitter::Node;\nfn run(node: Node<'_>) { let returned = identity(node); returned.kind(); }",
    );
    workspace.build();
    let coverage = |conn: &rusqlite::Connection| -> Vec<(String, String)> {
        let mut statement = conn.prepare("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='src/client.rs' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='returned.kind' ORDER BY status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)").unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let initial = workspace.open();
    let expected = coverage(&initial);
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0].0, "external");
    assert!(
        expected[0].1.contains("tree_sitter::Node at line 1"),
        "{expected:?}"
    );
    commitments::verify(&initial).unwrap();
    drop(initial);
    workspace.write(
        "src/provider.rs",
        "use tree_sitter::Node as Syntax;\npub fn identity<'a>(node: Syntax<'a>) -> Syntax<'a> { node }",
    );
    workspace.delta(&["src/provider.rs"]);
    let delta = workspace.open();
    commitments::verify(&delta).unwrap();
    assert_eq!(coverage(&delta), expected);
    let cold_db = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold_db);
    let cold = reader::open(&cold_db, workspace.root()).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(coverage(&delta), coverage(&cold));
}
