use super::super::*;

#[test]
fn rust_async_return_contracts_keep_the_future_boundary() {
    let source = "struct Service; impl Service { fn work(&self) {} }\nfn ready() -> Service { Service {} }\nasync fn pending() -> Service { Service {} }\nfn run() { let direct = ready(); direct.work(); let future = pending(); future.work(); }";
    let facts = ast::extract("src/async.rs", "rust", source).unwrap();
    let pending = facts
        .nodes
        .iter()
        .find(|node| node.name == "pending")
        .unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, pending);
    assert_eq!(details["async"], true);
    assert_eq!(details["value_flow"]["return_type"]["kind"], "unknown");
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
    let details = crate::common::assertions::projected_node_details(client, run);
    let made = details["value_flow"]["bindings"]
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
    let details =
        crate::common::assertions::projected_node_details(&facts["src/operations.rs"], factory);
    assert_eq!(details["value_flow"]["return_type"]["name"], "Service");
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
