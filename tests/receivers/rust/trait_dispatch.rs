use super::*;

#[test]
fn rust_trait_methods_require_visible_proven_traits() {
    let facts: BTreeMap<_, _> = [
        ("src/model.rs", "pub struct Service;"),
        ("src/traits.rs", "pub trait Work { fn work(&self); }"),
        ("src/operations.rs", "use crate::model::Service; use crate::traits::Work;\nimpl Work for Service { fn work(&self) {} }"),
        ("src/client.rs", "use crate::model::Service; use crate::traits::Work;\nfn run(client: &Service) { client.work(); }"),
        ("src/no_trait.rs", "use crate::model::Service;\nfn run(client: &Service) { client.work(); }"),
        ("src/hidden_trait.rs", "use crate::model::Service;\nmod hidden { pub trait Hidden { fn hidden(&self); } }\nimpl hidden::Hidden for Service { fn hidden(&self) {} }\nfn run(client: &Service) { client.hidden(); }"),
    ].into_iter().map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap())).collect();
    let graph = linker::link(&facts);
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| coverage.path == "src/client.rs"
                && coverage.expression == "client.work"
                && coverage.status == "resolved"),
        "{facts:#?}\n{graph:#?}"
    );
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.path == "src/no_trait.rs"
            && coverage.expression == "client.work"
            && coverage.status == "unresolved"));
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.path == "src/hidden_trait.rs"
            && coverage.expression == "client.hidden"
            && coverage.status == "unresolved"));
}

#[test]
fn rust_trait_default_methods_require_a_visible_concrete_impl() {
    let facts: BTreeMap<_, _> = [
        ("src/model.rs", "pub struct Service;"),
        (
            "src/traits.rs",
            "pub trait Work { fn work(&self) {} fn overridden(&self) {} fn required(&self); }",
        ),
        (
            "src/operations.rs",
            "use crate::model::Service; use crate::traits::Work;\nimpl Work for Service { fn overridden(&self) {} fn required(&self) {} }",
        ),
        (
            "src/client.rs",
            "use crate::model::Service; use crate::traits::Work;\nfn run(client: &Service) { client.work(); client.overridden(); client.required(); }",
        ),
        (
            "src/no_trait.rs",
            "use crate::model::Service;\nfn run(client: &Service) { client.work(); }",
        ),
        (
            "src/no_impl.rs",
            "use crate::traits::Work;\nstruct Other;\nfn run(other: &Other) { other.work(); }",
        ),
    ]
    .into_iter()
    .map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap()))
    .collect();
    let graph = linker::link(&facts);
    let default_method = facts["src/traits.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "work" && node.kind == "method")
        .unwrap();
    let implementations = &facts["src/operations.rs"].nodes;
    for (expression, target) in [
        ("client.work", default_method),
        (
            "client.overridden",
            implementations
                .iter()
                .find(|node| node.name == "overridden" && node.kind == "method")
                .unwrap(),
        ),
        (
            "client.required",
            implementations
                .iter()
                .find(|node| node.name == "required" && node.kind == "method")
                .unwrap(),
        ),
    ] {
        assert!(
            graph.edges.iter().any(|edge| edge.path == "src/client.rs"
                && edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id),
            "{graph:#?}"
        );
    }
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.path == "src/no_trait.rs"
            && coverage.expression == "client.work"
            && coverage.status == "unresolved"));
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.path == "src/no_impl.rs"
            && coverage.expression == "other.work"
            && coverage.status == "unresolved"));
}

#[test]
fn rust_computed_and_qualified_callees_keep_opaque_dispatch() {
    let source = "struct Service; trait Work { fn work(&self); }\nimpl Work for Service { fn work(&self) {} }\nfn choose() -> Service { Service {} }\nfn run(client: &Service) { choose().work(); <Service as Work>::work(client); }\nmacro_rules! invoke { () => {} }\nfn macro_call() { invoke!(); }";
    let facts = ast::extract("src/dispatch.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    for expression in ["choose().work", "<Service as Work>::work"] {
        assert!(
            facts
                .references
                .iter()
                .any(|reference| reference.kind == "calls"
                    && reference.expression == expression
                    && reference.dynamic),
            "{facts:#?}"
        );
    }
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "calls"
                && reference.expression == "invoke"
                && !reference.dynamic),
        "{facts:#?}"
    );
    let graph = linker::link(&BTreeMap::from([("src/dispatch.rs".to_owned(), facts)]));
    for expression in ["choose().work", "<Service as Work>::work"] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|coverage| coverage.expression == expression
                    && coverage.status == "unresolved"
                    && coverage.evidence.starts_with("computed receiver")),
            "{graph:#?}"
        );
    }
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| coverage.expression == "choose" && coverage.status == "resolved"),
        "{graph:#?}"
    );
}
