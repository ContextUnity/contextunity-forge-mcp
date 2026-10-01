#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::models::ReceiverHint,
    engine::{ast, linker},
};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn lazy_exports_recognize_manifest_lists_and_attribute_access() {
    let source = r#"
__all__ = ['ItemA', 'ItemB']
def __getattr__(name):
    if name in __all__:
        import importlib
        return importlib.import_module(f".{name}", __name__)
    raise AttributeError(name)
"#;
    let facts = ast::extract("package/__init__.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let init = facts.nodes.iter().find(|n| n.kind == "module").unwrap();
    if let Some(exports) = init.details.get("lazy_exports").and_then(|v| v.as_array()) {
        let names: Vec<_> = exports.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"ItemA"));
        assert!(names.contains(&"ItemB"));
    }
}

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
fn class_assignments_are_bindings_without_leaking_nested_callable_assignments() {
    let facts = ast::extract("class_bindings.py", "python", "class Model:\n    objects = factory()\n    execute: object = custom\n    def method(self):\n        local = 1\n    class Nested:\n        nested = 1\n").unwrap();
    let model = facts
        .nodes
        .iter()
        .find(|node| node.name == "Model")
        .unwrap();
    assert_eq!(model.details["bindings"], json!(["execute", "objects"]));
}

#[test]
fn computed_receiver_hints_are_structural_and_preserve_original_expression() {
    let source = "def run():\n    'value'.upper()\n    r'value'.strip()\n    b'value'.upper()\n    f'{value}'.upper()\n    Widget().execute()\n    package.Widget().execute()\n    super().execute()\n    super(Widget, self).execute()\n    response.request().consume()\n    factory().request().consume()\n";
    let facts = ast::extract("computed.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let hint = |expression| {
        &facts
            .references
            .iter()
            .find(|reference| reference.expression == expression)
            .unwrap()
            .receiver_hint
    };
    assert!(
        matches!(hint("'value'.upper"), Some(ReceiverHint::StringLiteral { member }) if member == "upper")
    );
    assert!(
        matches!(hint("r'value'.strip"), Some(ReceiverHint::StringLiteral { member }) if member == "strip")
    );
    assert!(hint("b'value'.upper").is_none());
    assert!(hint("f'{value}'.upper").is_none());
    assert!(
        matches!(hint("Widget().execute"), Some(ReceiverHint::CallResult { callee, member }) if callee == "Widget" && member == "execute")
    );
    assert!(
        matches!(hint("package.Widget().execute"), Some(ReceiverHint::CallResult { callee, .. }) if callee == "package.Widget")
    );
    assert!(
        matches!(hint("super().execute"), Some(ReceiverHint::Super { member }) if member == "execute")
    );
    assert!(hint("super(Widget, self).execute").is_none());
    assert!(
        matches!(hint("response.request().consume"), Some(ReceiverHint::CallResult { callee, .. }) if callee == "response.request")
    );
    assert!(hint("factory().request().consume").is_none());
}

#[test]
fn verified_constructor_super_and_known_string_methods_resolve_at_the_linker() {
    let source = "class Base:\n    def execute(self): pass\nclass Child(Base):\n    def execute(self): pass\n    def run(self): return super().execute()\ndef direct(): return Base().execute()\ndef literal(): return 'value'.upper()\ndef invalid(): return 'value'.invented_method()\n";
    let facts = ast::extract("resolved.py", "python", source).unwrap();
    let base_method = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "resolved.Base.execute")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("resolved.py".to_owned(), facts)]));
    for expression in ["super().execute", "Base().execute", "'value'.upper"] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            "resolved",
            "{expression}"
        );
    }
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "'value'.invented_method")
            .unwrap()
            .status,
        "unresolved"
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == base_method)
            .count(),
        2
    );
}
