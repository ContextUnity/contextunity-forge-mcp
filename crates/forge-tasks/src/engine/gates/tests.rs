use super::*;

#[test]
fn command_proof_reconciliation_rejects_forged_count_fields() {
    let supplied = serde_json::json!({
        "command": "check",
        "exit_code": 0,
        "tests_passed": 0,
        "tests_failed": 0
    });
    let observed = serde_json::json!({
        "command": "check",
        "exit_code": 0,
        "tests_passed": 8,
        "tests_failed": 0
    });

    let error =
        super::submit::validate_observed_proof_fields("command_proof", &supplied, &observed)
            .unwrap_err();

    assert!(error.to_string().contains("command_proof.tests_passed"));
}

#[test]
fn public_trait_methods_use_ast_visibility_metadata() {
    let make_node = |kind: &str, name: &str, qualname: &str, details: serde_json::Value| {
        forge_core::models::Node {
            id: format!("{kind}:{qualname}"),
            kind: kind.into(),
            name: name.into(),
            qualname: qualname.into(),
            path: "src/routing.file".into(),
            line: 1,
            end_line: 1,
            is_test: false,
            language: String::new(),
            generated: false,
            details,
            native_semantic: None,
        }
    };
    for details in [
        serde_json::json!({"is_public": true}),
        serde_json::json!({"visibility": "public"}),
    ] {
        let nodes = vec![
            make_node("trait", "Router", "Router", details),
            make_node("method", "route", "Router.route", serde_json::json!({})),
        ];
        let public_trait_methods = super::scope::public_trait_methods(&nodes);
        assert!(public_trait_methods.contains("Router.route"));
        assert!(super::scope::exported_callable(
            &nodes[1],
            &std::collections::BTreeSet::new(),
            &public_trait_methods,
        ));
    }
    let private_nodes = vec![
        make_node("trait", "Router", "Router", serde_json::json!({})),
        make_node("method", "route", "Router.route", serde_json::json!({})),
    ];
    assert!(super::scope::public_trait_methods(&private_nodes).is_empty());
}

fn retired_public_function_facts(
    path: &str,
    _source: &str,
) -> anyhow::Result<Option<forge_core::typed_facts::TypedFacts>> {
    let facts = forge_core::models::Facts {
        nodes: vec![forge_core::models::Node {
            id: "function:retired".into(),
            kind: "function".into(),
            name: "retired".into(),
            qualname: "api::retired".into(),
            path: path.into(),
            line: 1,
            end_line: 1,
            is_test: false,
            language: String::new(),
            generated: false,
            details: serde_json::json!({"is_public": true, "signature": "retired"}),
            native_semantic: None,
        }],
        ..Default::default()
    };
    Ok(Some(forge_core::typed_facts::TypedFacts {
        facts,
        flows: Default::default(),
    }))
}

#[test]
fn deleted_source_retires_public_symbols_without_read_errors() {
    set_ast_extractor(retired_public_function_facts);
    let root = std::env::temp_dir().join(format!(
        "forge-retire-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let git = |args: &[&str]| {
        let output = crate::git::git_command(&root, args).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.name", "ACDD test"]);
    git(&["config", "user.email", "acdd-test@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("forge-mcp.yaml"), "roots: [src]\n").unwrap();
    std::fs::write(root.join("src/api.rs"), "pub fn retired() -> usize { 1 }\n").unwrap();
    git(&["add", "forge-mcp.yaml", "src"]);
    git(&["commit", "-m", "baseline"]);
    std::fs::create_dir_all(root.join(".forge")).unwrap();
    let index = root.join(".forge/code-map.sqlite");
    let connection = rusqlite::Connection::open(&index).unwrap();
    connection.execute_batch(
        "CREATE TABLE path_dictionary (path_id INTEGER PRIMARY KEY, path TEXT UNIQUE);
         CREATE TABLE nodes (node_hash INTEGER PRIMARY KEY, path_id INTEGER, kind TEXT, name TEXT, qualname TEXT, line INTEGER, end_line INTEGER, is_test INTEGER, language TEXT, details TEXT);
         CREATE TABLE edges (src_hash INTEGER, dst_hash INTEGER, kind TEXT);
         CREATE TABLE edge_occurrences (src_hash INTEGER, dst_hash INTEGER, kind TEXT, line INTEGER, evidence_id INTEGER);
         CREATE TABLE coverage_evidence (evidence_id INTEGER PRIMARY KEY, evidence TEXT);
         INSERT INTO path_dictionary VALUES (1, 'src/api.rs');
         INSERT INTO nodes VALUES (1, 1, 'function', 'retired', 'api::retired', 1, 1, 0, 'rust', '{\"signature\":\"pub fn retired() -> usize\"}');"
    ).unwrap();
    std::fs::remove_file(root.join("src/api.rs")).unwrap();
    let result = validate_public_signature_scope(
        &connection,
        "retirement",
        &root,
        &["src/api.rs".into()],
        None,
        None,
    );
    drop(connection);
    std::fs::remove_dir_all(root).unwrap();
    result.expect(
        "deleted tracked source has retired symbols, rather than unreadable current source",
    );
}
