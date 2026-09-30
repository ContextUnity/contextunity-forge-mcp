#![cfg(feature = "lang-python")]
use contextunity_forge_mcp::{
    core::models::ReceiverHint,
    engine::{ast, linker},
};
use std::collections::BTreeMap;

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
        matches!(hint("'value'.upper"),Some(ReceiverHint::StringLiteral { member }) if member=="upper")
    );
    assert!(
        matches!(hint("r'value'.strip"),Some(ReceiverHint::StringLiteral { member }) if member=="strip")
    );
    assert!(hint("b'value'.upper").is_none());
    assert!(hint("f'{value}'.upper").is_none());
    assert!(
        matches!(hint("Widget().execute"),Some(ReceiverHint::CallResult { callee,member }) if callee=="Widget" && member=="execute")
    );
    assert!(
        matches!(hint("package.Widget().execute"),Some(ReceiverHint::CallResult { callee,.. }) if callee=="package.Widget")
    );
    assert!(
        matches!(hint("super().execute"),Some(ReceiverHint::Super { member }) if member=="execute")
    );
    assert!(hint("super(Widget, self).execute").is_none());
    assert!(
        matches!(hint("response.request().consume"),Some(ReceiverHint::CallResult { callee,.. }) if callee=="response.request")
    );
    assert!(hint("factory().request().consume").is_none());
}

#[test]
fn unknown_factory_return_and_shadowed_constructor_remain_unresolved() {
    let source = "class Widget:\n    def execute(self): pass\ndef unknown(): return response.request().execute()\ndef shadowed(Widget): return Widget().execute()\n";
    let facts = ast::extract("unknown.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("unknown.py".to_owned(), facts)]));
    for expression in ["response.request().execute", "Widget().execute"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "unresolved", "{expression}");
    }
}

#[test]
fn verified_constructor_super_and_known_string_methods_resolve_at_the_linker() {
    let source="class Base:\n    def execute(self): pass\nclass Child(Base):\n    def execute(self): pass\n    def run(self): return super().execute()\ndef direct(): return Base().execute()\ndef literal(): return 'value'.upper()\ndef invalid(): return 'value'.invented_method()\n";
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
