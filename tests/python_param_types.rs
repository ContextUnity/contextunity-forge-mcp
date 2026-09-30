#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::engine::ast;
use serde_json::json;

#[test]
fn typed_parameters_preserve_forward_qualified_and_default_types_in_their_scope() {
    let source = "class Service:\n    def execute(self): pass\ndef run(service: Service, forward: 'pkg.Service', default: pkg.Service = None, *items: Service, **options: 'pkg.Service'):\n    def nested(service: Other): pass\n    return service.execute()\nclass Client:\n    def call(self, service: Service): return service.execute()\ndef untyped(service): return service.execute()\n";
    let facts = ast::extract("parameters.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let node = |name| facts.nodes.iter().find(|node| node.name == name).unwrap();
    assert_eq!(
        node("run").details["param_types"],
        json!({"service":"Service", "forward":"pkg.Service", "default":"pkg.Service"})
    );
    assert_eq!(
        node("nested").details["param_types"],
        json!({"service":"Other"})
    );
    assert_eq!(
        node("call").details["param_types"],
        json!({"service":"Service"})
    );
    assert!(node("untyped").details.get("param_types").is_none());
}

#[test]
fn variadic_annotations_do_not_treat_the_container_as_an_element_receiver() {
    let source = "class Connection:\n    def execute(self): pass\ndef positional(*db: Connection): return db.execute()\ndef keyword(**db: Connection): return db.execute()\n";
    let facts = ast::extract("variadic.py", "python", source).unwrap();
    for node in facts
        .nodes
        .iter()
        .filter(|node| matches!(node.name.as_str(), "positional" | "keyword"))
    {
        assert!(node.details.get("param_types").is_none());
    }
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "variadic.py".to_owned(),
            facts,
        )]));
    let calls: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| coverage.expression == "db.execute")
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|coverage| coverage.status == "unresolved"));
}

#[test]
fn class_assignments_are_bindings_without_leaking_nested_callable_assignments() {
    let facts=ast::extract("class_bindings.py","python","class Model:\n    objects = factory()\n    execute: object = custom\n    def method(self):\n        local = 1\n    class Nested:\n        nested = 1\n").unwrap();
    let model = facts
        .nodes
        .iter()
        .find(|node| node.name == "Model")
        .unwrap();
    assert_eq!(model.details["bindings"], json!(["execute", "objects"]));
}
