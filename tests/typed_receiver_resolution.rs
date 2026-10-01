#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript"
))]

use contextunity_forge_mcp::{
    core::models::{Facts, Graph},
    engine::{ast, linker},
};
use std::collections::BTreeMap;

#[cfg(any(feature = "lang-python", feature = "lang-typescript"))]
#[path = "receivers/persistence.rs"]
mod persistence;

fn linked(files: &[(&str, &str, &str)]) -> (BTreeMap<String, Facts>, Graph) {
    let facts: BTreeMap<_, _> = files
        .iter()
        .map(|(path, language, source)| {
            (
                (*path).to_owned(),
                ast::extract(path, language, source).unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
    (facts, graph)
}

fn call<'a>(
    graph: &'a Graph,
    expression: &str,
) -> &'a contextunity_forge_mcp::core::models::Coverage {
    graph
        .coverage
        .iter()
        .find(|coverage| coverage.expression == expression)
        .unwrap()
}

fn assert_inferred(facts: &BTreeMap<String, Facts>, graph: &Graph, expression: &str, method: &str) {
    assert_eq!(
        call(graph, expression).status,
        "resolved",
        "{:?}\n{facts:#?}",
        call(graph, expression)
    );
    let target = facts
        .values()
        .flat_map(|facts| &facts.nodes)
        .find(|node| node.kind == "method" && node.name == method)
        .unwrap();
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.dst == target.id
        && edge.evidence == expression
        && edge.confidence == "inferred"));
}

#[cfg(feature = "lang-python")]
#[test]
fn typed_local_and_imported_receivers_resolve_actual_methods() {
    let (facts, graph) = linked(&[
        ("service.py", "python", "class Worker:\n    def get(self): return 1\n"),
        ("consumer.py", "python", "from service import Worker as Service\ndef run(client: Service): return client.get()\n"),
    ]);
    assert_inferred(&facts, &graph, "client.get", "get");
    let (facts, graph) = linked(&[("service.py", "python", "class Worker:\n    def get(self): return 1\ndef run(client: 'Worker'):\n    def nested(): return client.get()\n    return nested()\n")]);
    assert_inferred(&facts, &graph, "client.get", "get");
}

#[cfg(feature = "lang-python")]
#[test]
fn typed_external_receivers_retain_actual_import_provenance() {
    let (_, graph) = linked(&[("consumer.py", "python", "import pytest\nfrom unknown_vendor import Client as Service\ndef run(monkeypatch: pytest.MonkeyPatch, client: Service):\n    monkeypatch.setattr('name', 1)\n    return client.get()\n")]);
    assert_eq!(call(&graph, "monkeypatch.setattr").status, "external");
    assert!(call(&graph, "monkeypatch.setattr")
        .evidence
        .contains("external import pytest"));
    assert_eq!(call(&graph, "client.get").status, "external");
    assert!(call(&graph, "client.get")
        .evidence
        .contains("external import unknown_vendor"));
    assert!(!graph
        .edges
        .iter()
        .any(|edge| edge.evidence == "client.get" || edge.evidence == "monkeypatch.setattr"));
}

#[cfg(feature = "lang-python")]
#[test]
fn receiver_rebinding_shadowing_and_unspecified_types_stay_unresolved() {
    for body in [
        "def run(client: Worker, other):\n    client = other\n    return client.get()\n",
        "def run(client): return client.get()\n",
        "def run(client: Worker):\n    def nested(client): return client.get()\n    return nested(client)\n",
        "def run(client: Worker | None): return client.get()\n",
        "def run(*client: Worker): return client.get()\n",
        "def run(**client: Worker): return client.get()\n",
        "def outer(Worker):\n    def inner(client: Worker): return client.get()\n    return inner\n",
    ] {
        let source = format!("class Worker:\n    def get(self): return 1\n{body}");
        let (_, graph) = linked(&[("service.py", "python", &source)]);
        assert_eq!(call(&graph, "client.get").status, "unresolved", "{source}");
        assert!(!graph.edges.iter().any(|edge| edge.evidence == "client.get"), "{source}");
    }
}

#[cfg(feature = "lang-python")]
#[test]
fn ambiguous_receiver_type_never_selects_one_imported_class() {
    let (_, graph) = linked(&[
        ("first.py", "python", "class Worker:\n    def get(self): return 1\n"),
        ("second.py", "python", "class Worker:\n    def get(self): return 2\n"),
        ("consumer.py", "python", "from first import Worker as Service\nfrom second import Worker as Service\ndef run(client: Service): return client.get()\n"),
    ]);
    assert_eq!(call(&graph, "client.get").status, "ambiguous");
    assert!(!graph.edges.iter().any(|edge| edge.evidence == "client.get"));
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_reference_parameter_resolves_method_with_inferred_confidence() {
    let (facts, graph) = linked(&[(
        "service.rs",
        "rust",
        "struct Worker; impl Worker { fn get(&self) {} } fn run(client: &Worker) { client.get(); }",
    )]);
    assert_inferred(&facts, &graph, "client.get", "get");
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_parameter_resolves_method_with_inferred_confidence() {
    let (facts, graph) = linked(&[(
        "service.ts",
        "typescript",
        "class Worker { get() {} } function run(client: Worker) { client.get(); }",
    )]);
    assert_inferred(&facts, &graph, "client.get", "get");
}

#[cfg(feature = "lang-python")]
#[test]
fn local_annotation_bindings_override_external_imports() {
    let (facts, graph) = linked(&[("consumer.py", "python", "from unknown_vendor import Worker\nclass Worker:\n    def get(self): return 1\ndef run(client: Worker): return client.get()\n")]);
    assert_inferred(&facts, &graph, "client.get", "get");
    let (_, graph) = linked(&[("consumer.py", "python", "from unknown_vendor import Worker\ndef outer(Worker):\n    def run(client: Worker): return client.get()\n    return run\n")]);
    assert_eq!(call(&graph, "client.get").status, "unresolved");
    assert!(!call(&graph, "client.get")
        .evidence
        .contains("external import"));
}

#[cfg(feature = "lang-python")]
#[test]
fn non_typing_vendor_any_name_retains_external_provenance() {
    let (_, graph) = linked(&[(
        "consumer.py",
        "python",
        "from unknown_vendor import Any\ndef run(client: Any): return client.get()\n",
    )]);
    assert_eq!(call(&graph, "client.get").status, "external");
}

#[cfg(feature = "lang-python")]
#[path = "receivers/python_value_flow.rs"]
mod python_value_flow;

#[cfg(feature = "lang-rust")]
#[path = "receivers/rust_value_flow.rs"]
mod rust_value_flow;
