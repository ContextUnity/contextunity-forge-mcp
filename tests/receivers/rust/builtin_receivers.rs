use super::*;

#[test]
fn rust_standard_constructors_have_finite_external_origins() {
    let path = "src/constructors.rs";
    let source = "use std::path::Path;\nuse std::sync::Arc;\nfn run() { let mut values = Vec::new(); values.push(1); let text = String::from(\"x\"); text.len(); let path = Path::new(\"x\"); path.join(\"y\"); let boxed = Box::new(1); boxed.as_ref(); let shared = Arc::new(1); shared.clone(); }";
    let facts = ast::extract(path, "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
    for expression in ["Vec::new", "String::from", "Box::new"] {
        assert!(
            graph.coverage.iter().any(|row| row.path == path
                && row.expression == expression
                && row.status == "external"
                && row.evidence.contains("Rust standard library")),
            "{expression}: {graph:#?}"
        );
    }
    for (expression, origin) in [("Path::new", "std.path.Path"), ("Arc::new", "std.sync.Arc")] {
        assert!(
            graph.coverage.iter().any(|row| row.path == path
                && row.expression == expression
                && row.status == "external"
                && row.evidence.contains(origin)),
            "{expression}: {graph:#?}"
        );
    }
    for expression in [
        "values.push",
        "text.len",
        "path.join",
        "boxed.as_ref",
        "shared.clone",
    ] {
        assert!(
            graph.coverage.iter().any(|row| row.path == path
                && row.expression == expression
                && row.status == "external"),
            "{expression}: {graph:#?}"
        );
    }
}

#[test]
fn rust_builtin_members_are_finite_and_do_not_replace_local_types() {
    let facts: BTreeMap<_, _> = [("src/service.rs", "fn builtin(value: String) { value.len(); value.imaginary(); }\nmod local { struct String; fn custom(value: String) { value.len(); } }\n")]
        .into_iter().map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap())).collect();
    let graph = linker::link(&facts);
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.expression == "value.len" && coverage.status == "external"));
    assert!(graph.coverage.iter().any(
        |coverage| coverage.expression == "value.imaginary" && coverage.status == "unresolved"
    ));
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.expression == "value.len" && coverage.status == "unresolved"));
}

#[test]
fn rust_generic_annotations_preserve_arguments_without_implicit_unwrapping() {
    let source = "struct Service; struct Error;\nfn containers(items: Vec<Service>, optional: Option<Service>, result: Result<Service, Error>) { items.len(); optional.is_some(); result.is_ok(); items.work(); optional.work(); }\nmod custom { struct Vec<T> { item: T } fn run(items: Vec<String>) { items.len(); } }";
    let facts = ast::extract("src/containers.rs", "rust", source).unwrap();
    let function = facts
        .nodes
        .iter()
        .find(|node| node.name == "containers")
        .unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, function);
    let bindings = details["value_flow"]["bindings"].as_array().unwrap();
    let items = bindings
        .iter()
        .find(|binding| binding["name"] == "items")
        .unwrap();
    assert_eq!(items["value"]["type_expr"]["base"], "Vec");
    assert_eq!(items["value"]["type_expr"]["args"][0]["name"], "Service");
    let graph = linker::link(&BTreeMap::from([("src/containers.rs".to_owned(), facts)]));
    for expression in ["items.len", "optional.is_some", "result.is_ok"] {
        assert!(
            graph.coverage.iter().any(|coverage| coverage.line == 2
                && coverage.expression == expression
                && coverage.status == "external"),
            "{graph:#?}"
        );
    }
    for expression in ["items.work", "optional.work"] {
        assert!(
            graph.coverage.iter().any(
                |coverage| coverage.expression == expression && coverage.status == "unresolved"
            ),
            "{graph:#?}"
        );
    }
    assert!(
        graph.coverage.iter().any(|coverage| coverage.line == 3
            && coverage.expression == "items.len"
            && coverage.status == "unresolved"),
        "{graph:#?}"
    );
}

#[test]
fn rust_str_receivers_resolve_core_inherent_members_without_prelude_imports() {
    let source = "#![no_std]\n#![no_implicit_prelude]\nfn inspect<'a>(value: &'a str) { value.len(); value.is_empty(); value.as_bytes(); value.trim(); value.split_once(':'); value.is_ascii(); }\nfn generic<str>(unknown: &str) { unknown.as_bytes(); }";
    let facts = ast::extract("src/strings.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let inspect = facts
        .nodes
        .iter()
        .find(|node| node.name == "inspect")
        .unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, inspect);
    let receiver_type = &details["value_flow"]["bindings"][0]["value"]["type_expr"];
    assert_eq!(receiver_type["kind"], "applied");
    assert_eq!(receiver_type["base"], "&");
    assert_eq!(receiver_type["args"][0]["name"], "str");
    let primitive_line = inspect.line;
    let local = ast::extract(
        "src/local.rs",
        "rust",
        "struct str;\nimpl str { fn as_bytes(&self) {} }\nfn local(value: &str) { value.as_bytes(); }",
    )
    .unwrap();
    assert!(local.errors.is_empty(), "{local:#?}");
    let id = |name| {
        local
            .nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap()
            .id
            .clone()
    };
    let local_owner = id("local");
    let local_target = id("as_bytes");
    let graph = linker::link(&BTreeMap::from([
        ("src/strings.rs".to_owned(), facts),
        ("src/local.rs".to_owned(), local),
    ]));
    for member in [
        "len",
        "is_empty",
        "as_bytes",
        "trim",
        "split_once",
        "is_ascii",
    ] {
        assert!(
            graph.coverage.iter().any(|row| row.path == "src/strings.rs"
                && row.line == primitive_line
                && row.expression == format!("value.{member}")
                && row.status == "external"),
            "{graph:#?}"
        );
    }
    assert!(
        graph.edges.iter().any(|edge| edge.src == local_owner
            && edge.dst == local_target
            && edge.kind == "calls"
            && edge.evidence == "value.as_bytes"),
        "{graph:#?}"
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.expression == "unknown.as_bytes" && row.status == "unresolved"),
        "{graph:#?}"
    );
}
