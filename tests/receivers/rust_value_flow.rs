#![cfg(feature = "lang-rust")]

use contextunity_forge_mcp::engine::{ast, linker};
use std::collections::BTreeMap;

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
    assert_eq!(run.details["param_types"]["client"], "&'a Service");
    let flow = &run.details["value_flow"];
    assert_eq!(flow["return_type"]["name"], "Service");
    let bindings = flow["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 5);
    assert_eq!(bindings[0]["name"], "client");
    assert_eq!(bindings[0]["value"]["type_expr"]["name"], "Service");
    assert_eq!(bindings[1]["value"]["type_expr"]["name"], "Service");
    assert_eq!(bindings[2]["value"]["callee"], "Service");
    assert_eq!(bindings[3]["value"]["callee"], "make");
    assert_eq!(bindings[4]["value"]["name"], "made");
}

#[test]
fn conditional_and_reassigned_rust_bindings_do_not_keep_an_unconditional_proof() {
    let facts = ast::extract("src/service.rs", "rust", "struct Service;\nfn run(flag: bool) {\n    let mut client = Service {};\n    if flag { let nested = Service {}; }\n    client = other();\n}\n").unwrap();
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let bindings = run.details["value_flow"]["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 3);
    assert_eq!(bindings[1]["conditional"], true);
    assert_eq!(bindings[2]["name"], "client");
    assert_eq!(bindings[2]["value"]["kind"], "unknown");
}

#[test]
fn rust_cross_file_inherent_impl_resolves_only_real_receiver_members() {
    let facts: BTreeMap<_, _> = [
        ("src/model.rs", "pub struct Service; pub type ServiceAlias = Service;"),
        ("src/operations.rs", "use crate::model::Service;\nimpl Service { pub fn work(&self) {} pub fn new() -> Self { Self {} } }"),
        ("src/client.rs", "use crate::model::{Service, ServiceAlias};\nfn run(client: &Service) { client.work(); client.missing(); let create = Service::new; let made = create(); made.work(); }\nfn alias(aliased: &ServiceAlias) { aliased.work(); }"),
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
        ("src/shared.rs", "pub struct Token; pub fn text() {}"),
        (
            "src/api/mod.rs",
            "use crate::shared::{text as render, Token as Alias}; use std::path::Path as FilePath;",
        ),
        (
            "src/api/child.rs",
            "use super::*; fn run(_value: Alias) { render(); FilePath::new(\"x\"); missing(); }",
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
    for (expression, target) in [("render", function), ("Alias", token)] {
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

#[test]
fn rust_local_constructor_binding_resolves_receiver_methods() {
    let path = "src/workspace.rs";
    let source = "pub struct Workspace {}\nimpl Workspace {\n    pub fn new() -> Self { Self {} }\n    pub fn open(&self) {}\n    pub fn create(&self) {}\n}\nfn run() {\n    let workspace = Workspace::new();\n    workspace.open();\n    workspace.create();\n}\n";
    let facts = ast::extract(path, "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let files = BTreeMap::from([(path.to_owned(), facts)]);
    let graph = linker::link(&files);
    let workspace = files[path]
        .nodes
        .iter()
        .find(|node| node.kind == "struct" && node.name == "Workspace")
        .unwrap();
    let constructor = files[path]
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "new")
        .unwrap();
    let self_references: Vec<_> = files[path]
        .references
        .iter()
        .filter(|reference| reference.kind == "references" && reference.expression == "Self")
        .collect();
    assert!(!self_references.is_empty(), "{:#?}", files[path]);
    for reference in self_references {
        assert!(
            graph.coverage.iter().any(|row| row.path == path
                && row.line == reference.line
                && row.expression == "Self"
                && row.status == "resolved"),
            "{graph:#?}"
        );
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "references"
                && edge.path == path
                && edge.line == reference.line
                && edge.evidence == "Self"
                && edge.src == constructor.id
                && edge.dst == workspace.id),
            "{graph:#?}"
        );
    }
    for (expression, method) in [
        ("Workspace::new", "new"),
        ("workspace.open", "open"),
        ("workspace.create", "create"),
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|coverage| coverage.expression == expression && coverage.status == "resolved"),
            "{expression}: {graph:#?}"
        );
        let target = files[path]
            .nodes
            .iter()
            .find(|node| node.kind == "method" && node.name == method)
            .unwrap();
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id
                && (expression == "Workspace::new" || edge.confidence == "inferred")),
            "{expression}: {graph:#?}"
        );
    }
}

#[test]
fn rust_generic_impl_methods_follow_the_declared_nominal_receiver() {
    let path = "src/store.rs";
    let source = "struct Store<T> { value: T }\nimpl<T> Store<T> { fn new(value: T) -> Self { Self { value } } fn read(&self) -> &T { &self.value } fn requires_clone(&self) where T: Clone {} fn constrained<U: Clone>(&self, _value: U) {} }\nimpl<T> Store<Vec<T>> { fn specialized(&self) {} }\ntrait Reading { fn default_constrained<U: Clone>(&self, _value: U) {} }\nimpl<T> Reading for Store<T> {}\nstruct Limited<T>(T);\nimpl<T: Clone> Limited<T> { fn checked(&self) {} }\nfn run(store: &Store<u8>, limited: &Limited<u8>) { store.read(); Store::new(1u8).read(); store.specialized(); store.requires_clone(); store.constrained(1u8); store.default_constrained(1u8); limited.checked(); }\nstruct Other;\nfn unknown(other: &Other) { other.read(); }";
    let facts = ast::extract(path, "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let method = facts
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "read")
        .unwrap();
    let method_id = method.id.clone();
    let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
    for expression in ["store.read", "Store::new(1u8).read"] {
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == method_id),
            "{expression}: {graph:#?}"
        );
    }
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.expression == "other.read" && row.status == "unresolved"),
        "{graph:#?}"
    );
    for expression in [
        "store.specialized",
        "store.requires_clone",
        "store.constrained",
        "store.default_constrained",
        "limited.checked",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "unresolved"),
            "{expression}: {graph:#?}"
        );
    }
}

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
fn rust_generic_annotations_preserve_arguments_without_implicit_unwrapping() {
    let source = "struct Service; struct Error;\nfn containers(items: Vec<Service>, optional: Option<Service>, result: Result<Service, Error>) { items.len(); optional.is_some(); result.is_ok(); items.work(); optional.work(); }\nmod custom { struct Vec<T> { item: T } fn run(items: Vec<String>) { items.len(); } }";
    let facts = ast::extract("src/containers.rs", "rust", source).unwrap();
    let function = facts
        .nodes
        .iter()
        .find(|node| node.name == "containers")
        .unwrap();
    let bindings = function.details["value_flow"]["bindings"]
        .as_array()
        .unwrap();
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
fn rust_async_return_contracts_keep_the_future_boundary() {
    let source = "struct Service; impl Service { fn work(&self) {} }\nfn ready() -> Service { Service {} }\nasync fn pending() -> Service { Service {} }\nfn run() { let direct = ready(); direct.work(); let future = pending(); future.work(); }";
    let facts = ast::extract("src/async.rs", "rust", source).unwrap();
    let pending = facts
        .nodes
        .iter()
        .find(|node| node.name == "pending")
        .unwrap();
    assert_eq!(pending.details["async"], true);
    assert_eq!(
        pending.details["value_flow"]["return_type"]["kind"],
        "unknown"
    );
    let graph = linker::link(&BTreeMap::from([("src/async.rs".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| coverage.expression == "direct.work" && coverage.status == "resolved"),
        "{graph:#?}"
    );
    assert!(graph.coverage.iter().any(|coverage| coverage.expression == "future.work" && coverage.status == "unresolved"), "{graph:#?}");
}

#[test]
fn rust_structural_callees_preserve_generic_factory_and_receiver_identity() {
    let facts: BTreeMap<_, _> = [
        ("src/model.rs", "pub struct Service;"),
        ("src/operations.rs", "use crate::model::Service;\nimpl Service { pub fn work<T>(&self) {} }\npub fn make<T>() -> Service { Service {} }"),
        ("src/client.rs", "use crate::model::Service; use crate::operations::make;\nfn run(client: &Service) {\n    let made = make::<u8>();\n    made.work::<u16>();\n    client\n        .work::<u32>();\n}"),
    ].into_iter().map(|(path, source)| (path.to_owned(), ast::extract(path, "rust", source).unwrap())).collect();
    let client = &facts["src/client.rs"];
    assert!(client.errors.is_empty(), "{client:#?}");
    for (expression, line, column) in [("make", 3, 15), ("made.work", 4, 4), ("client.work", 5, 4)]
    {
        assert!(
            client
                .references
                .iter()
                .any(|reference| reference.kind == "calls"
                    && reference.expression == expression
                    && reference.line == line
                    && reference.column == column
                    && !reference.dynamic),
            "{client:#?}"
        );
    }
    let run = client.nodes.iter().find(|node| node.name == "run").unwrap();
    let made = run.details["value_flow"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["name"] == "made")
        .unwrap();
    assert_eq!(made["value"]["kind"], "call");
    assert_eq!(made["value"]["callee"], "make");
    let graph = linker::link(&facts);
    let service = facts["src/model.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "Service" && node.kind == "struct")
        .unwrap();
    assert_eq!(service.name, "Service");
    let model_module = facts["src/model.rs"]
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.path == "src/operations.rs"
                && edge.kind == "imports"
                && edge.dst == model_module.id),
        "{graph:#?}"
    );
    let factory = facts["src/operations.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "make" && node.kind == "function")
        .unwrap();
    assert_eq!(
        factory.details["value_flow"]["return_type"]["name"],
        "Service"
    );
    for (expression, target) in [
        ("make", "make"),
        ("made.work", "work"),
        ("client.work", "work"),
    ] {
        let target = facts["src/operations.rs"]
            .nodes
            .iter()
            .find(|node| node.name == target)
            .unwrap();
        assert!(
            graph.edges.iter().any(|edge| edge.path == "src/client.rs"
                && edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id),
            "{facts:#?}\n{graph:#?}"
        );
    }
}

#[test]
fn rust_structural_external_callees_keep_import_origins() {
    let facts = ast::extract(
        "src/client.rs",
        "rust",
        "use rusqlite::Row;\nfn run(row: &Row) {\n    row\n        .get::<_, String>(0);\n}",
    )
    .unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "calls"
                && reference.expression == "row.get"
                && reference.line == 3
                && reference.column == 4
                && !reference.dynamic),
        "{facts:#?}"
    );
    let graph = linker::link(&BTreeMap::from([("src/client.rs".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| coverage.expression == "row.get"
                && coverage.status == "external"
                && coverage.evidence.contains("rusqlite::Row")),
        "{graph:#?}"
    );
}

#[test]
fn rust_structural_method_initializers_use_nominal_return_contracts() {
    let source = "struct Service;\nimpl Service { fn rebuild<T>(&self) -> Self { Self {} } fn work(&self) {} }\nfn run(client: &Service) { let made = client.rebuild::<u8>(); made.work(); }";
    let facts = ast::extract("src/service.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let made = run.details["value_flow"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["name"] == "made")
        .unwrap();
    assert_eq!(made["value"]["kind"], "call");
    assert_eq!(made["value"]["callee"], "client.rebuild");
    let facts = BTreeMap::from([("src/service.rs".to_owned(), facts)]);
    let graph = linker::link(&facts);
    for (expression, target) in [("client.rebuild", "rebuild"), ("made.work", "work")] {
        let target = facts["src/service.rs"]
            .nodes
            .iter()
            .find(|node| node.name == target)
            .unwrap();
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id),
            "{facts:#?}\n{graph:#?}"
        );
    }
}

#[test]
fn rust_associated_generic_factory_uses_declared_return_for_member_chains() {
    let source = "struct Factory; struct Service;\nimpl Factory { fn build<T>() -> Service { Service } }\nimpl Service { fn new() -> Self { Self } fn work(&self) {} }\nfn run() { Factory::build::<u8>().work(); let built = Factory::build::<u16>(); built.work(); Service::new().work(); }\nfn missing() { Factory::missing().work(); }";
    let facts = ast::extract("src/service.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let facts = BTreeMap::from([("src/service.rs".to_owned(), facts)]);
    let graph = linker::link(&facts);
    let work = facts["src/service.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "work" && node.kind == "method")
        .unwrap();
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == work.id && edge.line == 4)
            .count(),
        3,
        "{graph:#?}"
    );
    assert!(!graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.dst == work.id && edge.line == 5));
    assert!(graph.coverage.iter().any(|coverage| coverage.line == 5
        && coverage.expression.contains("Factory::missing")
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

#[test]
fn rust_structural_callees_observe_segment_and_byte_limits() {
    let maximum = "a.b.c.d.e.f.g\n        .h::<u8>";
    let beyond = "a.b.c.d.e.f.g.h\n        .i::<u8>";
    let maximum_name = "f".repeat(512);
    let long_name = "f".repeat(513);
    let source = format!(
        "fn run() {{ {maximum}(); {beyond}(); {maximum_name}::<u8>(); {long_name}::<u8>(); }}"
    );
    let facts = ast::extract("src/bounds.rs", "rust", &source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "calls"
                && reference.expression == "a.b.c.d.e.f.g.h"
                && !reference.dynamic),
        "{facts:#?}"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "calls"
                && reference.expression == beyond
                && reference.dynamic),
        "{facts:#?}"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "calls"
                && reference.expression == maximum_name
                && !reference.dynamic),
        "{facts:#?}"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "calls"
                && reference.expression.contains("[sha256:")
                && reference.dynamic),
        "{facts:#?}"
    );
}

#[test]
fn rust_destructuring_preserves_payload_bindings_and_constructor_calls() {
    let source = "struct Wrap<T>(T); struct Named { callback: fn(), shorthand: fn() }\nfn run(input: Option<fn()>, result: Result<fn(), ()>, wrapped: Wrap<fn()>, named: Named) {\n    let Some(ref mut payload) = input else { return; };\n    let Ok(success) = result else { return; };\n    let capture @ Wrap(inner) = wrapped;\n    let Named { callback: renamed, ref mut shorthand } = named;\n    Some(payload); Ok(success); Err(());\n}\nfn local(input: Option<fn()>) { let Some(Some) = input else { return; }; Some(); }";
    let facts = ast::extract("src/patterns.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    assert_eq!(
        run.details["rebindings"],
        serde_json::json!([
            "capture",
            "inner",
            "payload",
            "renamed",
            "shorthand",
            "success"
        ])
    );
    let payloads: Vec<_> = run.details["value_flow"]["bindings"]
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

#[test]
fn rust_lifetime_only_nominals_preserve_imported_receiver_contracts() {
    let source = "use tree_sitter::Node;\nfn run<'a>(node: &Node<'a>, owned: Node<'_>, mixed: Vec<Node<'a>>, generic: Node<u8>, counted: Node<'a, 3>) {\n    node.kind(); owned.child_by_field_name(\"name\"); generic.kind(); counted.kind();\n}\nfn copy<'a>(node: Node<'a>) -> Node<'a> { node }";
    let facts = ast::extract("src/nominals.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let bindings = run.details["value_flow"]["bindings"].as_array().unwrap();
    for name in ["node", "owned"] {
        let binding = bindings
            .iter()
            .find(|binding| binding["name"] == name)
            .unwrap();
        assert_eq!(
            binding["value"]["type_expr"],
            serde_json::json!({"kind":"named","name":"Node"})
        );
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
    assert_eq!(copy.details["value_flow"]["return_type"]["name"], "Node");
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
    use contextunity_forge_mcp::{
        core::commitments,
        db::reader,
    };
    use crate::common::Workspace;

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

#[test]
fn rust_declared_fields_resolve_nominal_parameters_and_tuple_receivers() {
    let source = "struct Service;\nimpl Service { fn work(&self) {} }\nstruct Named { id: String, nodes: Vec<Service>, service: Service }\nstruct Tuple(#[doc = \"service\"] pub Service, pub(crate) String, Vec<Service>);\nfn named(node: &Named) { node.id.as_str(); node.nodes.iter(); node.service.work(); }\nfn tuple(holder: &Tuple) { holder.0.work(); holder.1.as_str(); holder.2.iter(); }\nimpl Tuple { fn run(&self) { self.0.work(); } }";
    let facts = ast::extract("src/fields.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let work = facts.nodes.iter().find(|node| node.name == "work").unwrap();
    let work_id = work.id.clone();
    let tuple = facts
        .nodes
        .iter()
        .find(|node| node.name == "Tuple" && node.kind == "struct")
        .unwrap();
    let fields = tuple.details["value_flow"]["fields"].as_array().unwrap();
    assert_eq!(
        fields
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["0", "1", "2"]
    );
    assert_eq!(fields[0]["value"]["type_expr"]["name"], "Service");
    assert_eq!(fields[1]["value"]["type_expr"]["name"], "String");
    assert_eq!(fields[2]["value"]["type_expr"]["base"], "Vec");
    assert_eq!(
        fields[2]["value"]["type_expr"]["args"][0]["name"],
        "Service"
    );
    let graph = linker::link(&BTreeMap::from([("src/fields.rs".to_owned(), facts)]));
    for expression in ["node.service.work", "holder.0.work", "self.0.work"] {
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == work_id),
            "{graph:#?}"
        );
    }
    for expression in [
        "node.id.as_str",
        "node.nodes.iter",
        "holder.1.as_str",
        "holder.2.iter",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "external"),
            "{graph:#?}"
        );
    }
}

#[test]
fn rust_generic_field_roots_preserve_declared_unknown_boundaries() {
    let source = "struct Service;\ntrait Bound { type Item; }\nimpl Bound for Service { type Item = Service; }\ntype T = Service;\nstruct Named<T: Bound = Service> { direct: T, associated: T::Item, fixed: Service }\nstruct Tuple<T: Bound = Service>(T, T::Item, Service);";
    let facts = ast::extract("src/generic_fields.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    for name in ["Named", "Tuple"] {
        let owner = facts
            .nodes
            .iter()
            .find(|node| node.name == name && node.kind == "struct")
            .unwrap();
        let fields = owner.details["value_flow"]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3, "{facts:#?}");
        for field in &fields[..2] {
            assert_eq!(field["value"]["type_expr"]["kind"], "unknown", "{facts:#?}");
        }
        assert_eq!(fields[2]["value"]["type_expr"]["name"], "Service");
    }
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
    assert_eq!(
        inspect.details["value_flow"]["bindings"][0]["value"]["type_expr"]["name"],
        "str"
    );
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

#[test]
fn rust_smart_pointer_deref_dispatch_persists_exact_providers_and_boundaries() {
    use crate::common::Workspace;

    let workspace = Workspace::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = \"smart-pointer-provenance\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let sources = [
        (
            "lib.rs",
            "mod service; mod box_case; mod arc_case; mod rc_case; mod option_case; mod unimported_case; mod shadow_case; mod one; mod two; mod ambiguous_case;",
        ),
        (
            "service.rs",
            "pub struct Service; impl Service { pub fn work(&self) {} pub fn as_ref(&self) {} }",
        ),
        (
            "box_case.rs",
            "use crate::service::Service; pub fn run(value: Box<Service>) { value.work(); value.as_ref(); }",
        ),
        (
            "arc_case.rs",
            "use crate::service::Service; use std::sync::Arc; pub fn run(value: Arc<Service>) { value.work(); value.as_ref(); }",
        ),
        (
            "rc_case.rs",
            "use crate::service::Service; use std::rc::Rc; pub fn run(value: Rc<Service>) { value.work(); value.as_ref(); }",
        ),
        (
            "option_case.rs",
            "use crate::service::Service; pub fn run(value: Option<Service>) { value.work(); }",
        ),
        (
            "unimported_case.rs",
            "use crate::service::Service; pub fn run(value: Arc<Service>) { value.work(); }",
        ),
        (
            "shadow_case.rs",
            "use crate::service::Service; pub struct Arc<T>(T); impl<T> Arc<T> { pub fn as_ref(&self) -> &T { &self.0 } } pub fn run(value: Arc<Service>) { value.work(); value.as_ref(); }",
        ),
        ("one.rs", "pub struct Service; impl Service { pub fn work(&self) {} }"),
        ("two.rs", "pub struct Service; impl Service { pub fn work(&self) {} }"),
        (
            "ambiguous_case.rs",
            "use crate::one::Service; use crate::two::Service; pub fn run(value: Box<Service>) { value.work(); }",
        ),
    ];
    for (name, source) in sources {
        workspace.write(format!("src/{name}"), source);
    }

    workspace.build();
    let conn = workspace.open();
    let mut coverage = conn
        .prepare("SELECT c.status,v.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE p.path=?1 AND x.expression=?2 ORDER BY c.status,v.evidence")
        .unwrap();
    let mut edges = conn
        .prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.kind='calls' AND v.evidence=?2 ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap();

    for (path, expression, statuses, target, origin) in [
        (
            "src/box_case.rs",
            "value.work",
            &["resolved"][..],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/arc_case.rs",
            "value.work",
            &["resolved"],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/rc_case.rs",
            "value.work",
            &["resolved"],
            Some("src/service.rs"),
            None,
        ),
        (
            "src/box_case.rs",
            "value.as_ref",
            &["external"],
            None,
            Some("Rust standard library built-in"),
        ),
        (
            "src/arc_case.rs",
            "value.as_ref",
            &["external"],
            None,
            Some("Rust standard library built-in"),
        ),
        (
            "src/rc_case.rs",
            "value.as_ref",
            &["external"],
            None,
            Some("Rust standard library built-in"),
        ),
        (
            "src/option_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/unimported_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/shadow_case.rs",
            "value.work",
            &["unresolved"],
            None,
            None,
        ),
        (
            "src/shadow_case.rs",
            "value.as_ref",
            &["resolved"],
            Some("src/shadow_case.rs"),
            None,
        ),
        (
            "src/ambiguous_case.rs",
            "value.work",
            &["unresolved", "ambiguous"],
            None,
            None,
        ),
    ] {
        let rows: Vec<(String, String)> = coverage
            .query_map([path, expression], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows.len(), 1, "{path} {expression}: {rows:?}");
        assert!(
            statuses.contains(&rows[0].0.as_str()),
            "{path} {expression}: {rows:?}"
        );
        if let Some(origin) = origin {
            assert!(rows[0].1.contains(origin), "{path} {expression}: {rows:?}");
        }
        let targets: Vec<String> = edges
            .query_map([path, expression], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            targets,
            target.map_or_else(Vec::new, |target| vec![target.to_owned()]),
            "{path} {expression}: {rows:?}"
        );
    }
}
