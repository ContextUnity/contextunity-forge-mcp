use super::*;

#[test]
fn rust_cross_file_inherent_impl_resolves_only_real_receiver_members() {
    let facts: BTreeMap<_, _> = [
        ("src/model.rs", "pub struct Service; pub type ServiceAlias = Service;"),
        ("src/operations.rs", "use crate::model::Service;\nimpl Service { pub fn work(&self) {} pub fn new() -> Self { Self {} } }"),
        ("src/client.rs", "use crate::model::{Service, ServiceAlias};\nfn run(client: &Service) { client.work(); client.missing(); let create = Service::new; let made = create(); made.work(); }\nfn alias(aliased: &ServiceAlias) { aliased.work(); }"),
        ("src/api/sibling.rs", "pub fn func() {} pub struct MyStruct; impl MyStruct { pub fn new() {} }"),
        ("src/api/child.rs", "pub fn func() {} fn relative_calls() { super::sibling::func(); super::sibling::MyStruct::new(); }"),
        ("src/api/ancestor.rs", "pub fn func() {}"),
        ("src/api/nested/mod.rs", "pub mod child;"),
        ("src/api/nested/child.rs", "fn ancestor_call() { super::super::ancestor::func(); }"),
        ("src/api/mod.rs", "pub mod child; pub mod nested; fn child_call() { self::child::func(); }"),
        ("src/root_client.rs", "fn root_call() { crate::api::sibling::func(); }"),
    ].into_iter().map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap())).collect();
    let graph = linker::link(&facts);
    let target = facts["src/operations.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "work")
        .unwrap();
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == "client.work"
            && edge.dst == target.id),
        "{facts:#?}\n{graph:#?}"
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| coverage.expression == "client.missing"
                && coverage.status == "unresolved")
    );
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == "made.work"
            && edge.dst == target.id),
        "{graph:#?}"
    );
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == "aliased.work"
            && edge.dst == target.id),
        "{graph:#?}"
    );

    for (source_path, expression, target_path, target_name, target_kind) in [
        (
            "src/api/child.rs",
            "super::sibling::func",
            "src/api/sibling.rs",
            "func",
            "function",
        ),
        (
            "src/api/nested/child.rs",
            "super::super::ancestor::func",
            "src/api/ancestor.rs",
            "func",
            "function",
        ),
        (
            "src/root_client.rs",
            "crate::api::sibling::func",
            "src/api/sibling.rs",
            "func",
            "function",
        ),
        (
            "src/api/mod.rs",
            "self::child::func",
            "src/api/child.rs",
            "func",
            "function",
        ),
        (
            "src/api/child.rs",
            "super::sibling::MyStruct::new",
            "src/api/sibling.rs",
            "new",
            "method",
        ),
    ] {
        let target = facts[target_path]
            .nodes
            .iter()
            .find(|node| node.name == target_name && node.kind == target_kind)
            .unwrap();
        assert!(
            graph
                .coverage
                .iter()
                .any(|coverage| coverage.path == source_path
                    && coverage.expression == expression
                    && coverage.status == "resolved"),
            "{expression}: {graph:#?}"
        );
        assert!(
            graph.edges.iter().any(|edge| edge.path == source_path
                && edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id),
            "{expression}: {graph:#?}"
        );
        assert!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == target.id)
                .count()
                >= 1,
            "target {} has no call sites: {graph:#?}",
            target.id
        );
    }
}

#[test]
fn rust_nested_module_glob_uses_visible_same_crate_import_providers() {
    let sources = [
        ("crates/alpha/src/services.rs", "pub fn serve() {}"),
        (
            "crates/alpha/src/api/mod.rs",
            "pub fn direct() {}\nuse crate::services::serve;",
        ),
        (
            "crates/alpha/src/api/child.rs",
            "use super::*;\nfn run() { direct(); serve(); missing(); }",
        ),
        ("crates/beta/src/services.rs", "pub fn serve() {}"),
        (
            "crates/beta/src/api/mod.rs",
            "pub fn direct() {}\nuse crate::services::serve;",
        ),
        (
            "crates/beta/src/api/child.rs",
            "use super::*;\nfn run() { direct(); serve(); }",
        ),
    ];
    let facts: BTreeMap<_, _> = sources
        .into_iter()
        .map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap()))
        .collect();
    let graph = linker::link(&facts);
    for crate_name in ["alpha", "beta"] {
        let child_path = format!("crates/{crate_name}/src/api/child.rs");
        for (expression, provider_path) in [
            ("direct", format!("crates/{crate_name}/src/api/mod.rs")),
            ("serve", format!("crates/{crate_name}/src/services.rs")),
        ] {
            let provider = facts[&provider_path]
                .nodes
                .iter()
                .find(|node| node.name == expression && node.kind == "function")
                .unwrap();
            assert!(
                graph.edges.iter().any(|edge| edge.path == child_path
                    && edge.kind == "calls"
                    && edge.evidence == expression
                    && edge.dst == provider.id),
                "{graph:#?}"
            );
        }
    }
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.path == "crates/alpha/src/api/child.rs"
            && coverage.expression == "missing"
            && coverage.status == "unresolved"));
}

#[test]
fn rust_parent_glob_preserves_renamed_local_and_standard_import_providers() {
    let facts: BTreeMap<_, _> = [
        ("src/shared.rs", "pub struct Token; impl Token { pub fn check(&self) {} } pub fn text() {}"),
        (
            "src/api/mod.rs",
            "pub struct ParentType; impl ParentType { pub fn verify(&self) {} } use crate::shared::{text as render, Token as Alias}; use std::path::Path as FilePath;",
        ),
        (
            "src/api/child.rs",
            "use super::*; fn run(_value: Alias, parent: &ParentType) { render(); parent.verify(); FilePath::new(\"x\"); missing(); }",
        ),
    ]
    .into_iter()
    .map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap()))
    .collect();
    let graph = linker::link(&facts);
    let function = facts["src/shared.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "text")
        .unwrap();
    let token = facts["src/shared.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "Token")
        .unwrap();
    let verify = facts["src/api/mod.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "verify" && node.kind == "method")
        .unwrap();
    for (expression, target) in [
        ("render", function),
        ("Alias", token),
        ("parent.verify", verify),
    ] {
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.path == "src/api/child.rs"
                    && edge.evidence == expression
                    && edge.dst == target.id),
            "{expression}: {graph:#?}"
        );
    }
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.path == "src/api/child.rs"
                && row.expression == "FilePath::new"
                && row.status == "external"
                && row.evidence.contains("std.path.Path")),
        "{graph:#?}"
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.path == "src/api/child.rs"
                && row.expression == "missing"
                && row.status == "unresolved"),
        "{graph:#?}"
    );
}
