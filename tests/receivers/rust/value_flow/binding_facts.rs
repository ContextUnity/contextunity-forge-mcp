use super::super::*;

#[test]
fn rust_aliases_and_explicit_flow_preserve_types_without_lifetime_noise() {
    let facts = ast::extract("src/service.rs", "rust", "type ServiceId = u64;\nstruct Service;\nfn run<'a, T>(client: &'a Service, ignored: T, hole: _) -> Service {\n    let direct: Service = Service {};\n    let made = Service {};\n    let next = make();\n    let copied = made;\n    direct.work();\n    Service {}\n}\n").unwrap();
    assert!(facts.nodes.iter().any(|node| node.name == "ServiceId"
        && node.kind == "type"
        && node.id.starts_with("type:")));
    assert!(
        !facts
            .references
            .iter()
            .any(|reference| matches!(reference.expression.as_str(), "a" | "'a" | "T" | "_")),
        "{facts:#?}"
    );
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, run);
    assert_eq!(details["param_types"]["client"], "&'a Service");
    let flow = &details["value_flow"];
    assert_eq!(flow["return_type"]["name"], "Service");
    let bindings = flow["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 5);
    assert_eq!(bindings[0]["name"], "client");
    assert_eq!(bindings[0]["value"]["type_expr"]["kind"], "applied");
    assert_eq!(bindings[0]["value"]["type_expr"]["base"], "&");
    assert_eq!(
        bindings[0]["value"]["type_expr"]["args"][0]["name"],
        "Service"
    );
    assert_eq!(bindings[1]["value"]["type_expr"]["name"], "Service");
    assert_eq!(bindings[2]["value"]["callee"], "Service");
    assert_eq!(bindings[3]["value"]["callee"], "make");
    assert_eq!(bindings[4]["value"]["name"], "made");
}

#[test]
fn conditional_and_reassigned_rust_bindings_do_not_keep_an_unconditional_proof() {
    let facts = ast::extract("src/service.rs", "rust", "struct Service;\nfn run(flag: bool) {\n    let mut client = Service {};\n    if flag { let nested = Service {}; }\n    client = other();\n}\n").unwrap();
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, run);
    let bindings = details["value_flow"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 3);
    assert_eq!(bindings[1]["conditional"], true);
    assert_eq!(bindings[2]["name"], "client");
    assert_eq!(bindings[2]["value"]["kind"], "unknown");
}

#[test]
fn rust_declaration_proofs_start_after_the_initializer() {
    let facts: BTreeMap<_, _> = [("src/service.rs", "struct Other;\nimpl Other { fn len(&self) -> String { String::new() } }\nfn run(value: Other) { let value: String = value.len(); value.len(); }\n")]
        .into_iter().map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap())).collect();
    let graph = linker::link(&facts);
    let method = facts["src/service.rs"]
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "len")
        .unwrap();
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls"
                && edge.evidence == "value.len"
                && edge.dst == method.id)
            .count(),
        1,
        "{graph:#?}"
    );
    assert_eq!(
        graph
            .coverage
            .iter()
            .filter(|coverage| coverage.expression == "value.len" && coverage.status == "resolved")
            .count(),
        1,
        "{graph:#?}"
    );
    assert_eq!(
        graph
            .coverage
            .iter()
            .filter(|coverage| coverage.expression == "value.len" && coverage.status == "external")
            .count(),
        1,
        "{graph:#?}"
    );
}

#[test]
fn rust_destructuring_preserves_payload_bindings_and_constructor_calls() {
    let source = "struct Wrap<T>(T); struct Named { callback: fn(), shorthand: fn() }\nfn run(input: Option<fn()>, result: Result<fn(), ()>, wrapped: Wrap<fn()>, named: Named) {\n    let Some(ref mut payload) = input else { return; };\n    let Ok(success) = result else { return; };\n    let capture @ Wrap(inner) = wrapped;\n    let Named { callback: renamed, ref mut shorthand } = named;\n    Some(payload); Ok(success); Err(());\n}\nfn local(input: Option<fn()>) { let Some(Some) = input else { return; }; Some(); }";
    let facts = ast::extract("src/patterns.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    assert_eq!(
        crate::common::assertions::projected_node_details(&facts, run)["rebindings"],
        serde_json::json!([
            "capture",
            "inner",
            "payload",
            "renamed",
            "shorthand",
            "success"
        ])
    );
    let details = crate::common::assertions::projected_node_details(&facts, run);
    let payloads: Vec<_> = details["value_flow"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|binding| binding["position"]["line"].as_u64().unwrap() >= 3)
        .map(|binding| binding["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        payloads,
        [
            "payload",
            "success",
            "capture",
            "inner",
            "renamed",
            "shorthand"
        ]
    );
    let graph = linker::link(&BTreeMap::from([("src/patterns.rs".to_owned(), facts)]));
    for expression in ["Some", "Ok", "Err"] {
        assert!(
            graph.coverage.iter().any(|row| row.line == 7
                && row.expression == expression
                && row.status == "external"),
            "{graph:#?}"
        );
    }
    assert!(
        graph.coverage.iter().any(|row| row.line == 9
            && row.expression == "Some"
            && row.status == "unresolved"
            && row.evidence.contains("shadowed")),
        "{graph:#?}"
    );
}
