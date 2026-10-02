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

#[test]
fn local_receiver_dictionary_and_logger_methods_resolve_in_function_scope() {
    let source = r#"
from typing import Mapping
import logging

def decode(raw: Mapping[str, object], logger: logging.LoggerAdapter):
    val = raw.get("field")
    items = raw.items()
    logger.info("decoded")
    logger.warning("retry")

def helper():
    row = {}
    return row.get("id")
"#;
    let facts = ast::extract("decode.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("decode.py".to_owned(), facts)]));
    for expression in ["raw.get", "raw.items", "row.get"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|c| c.expression == expression)
            .unwrap_or_else(|| panic!("missing coverage for {expression}"));
        assert_eq!(
            coverage.status, "resolved",
            "expression {expression} should be resolved, evidence: {}",
            coverage.evidence
        );
    }
    for expression in ["logger.info", "logger.warning"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|c| c.expression == expression)
            .unwrap_or_else(|| panic!("missing coverage for {expression}"));
        assert_eq!(
            coverage.status, "external",
            "expression {expression} should be external, evidence: {}",
            coverage.evidence
        );
        assert!(coverage.evidence.contains("logging.LoggerAdapter"));
    }
}

#[test]
fn package_reexports_and_monorepo_hubs_resolve_overloads_type_aliases_and_runtime_implementations()
{
    let types_src = r#"
from typing import TypeAlias
JsonPrimitive: TypeAlias = str | int
JsonDict: TypeAlias = dict[str, JsonPrimitive]
StructData = JsonDict
"#;
    let decorators_src = r#"
from typing import overload

@overload
def grpc_error_handler(method: None = None): ...

@overload
def grpc_error_handler(method: object): ...

def grpc_error_handler(method: object = None):
    return method
"#;
    let grpc_errors_src = r#"
from .grpc_error_decorators import grpc_error_handler
"#;
    let core_init_src = r#"
from .types import StructData
from .grpc_errors import grpc_error_handler
"#;
    let commerce_typing_src = r#"
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    class TypedTabularInline:
        """Type stub only."""
        pass
else:
    class TypedTabularInline:
        """Runtime implementation."""
        def helper(self):
            return 42
"#;
    let consumer_src = r#"
from contextunity.core import StructData, grpc_error_handler
from contextunity.commerce.typing import TypedTabularInline

def run(data: StructData):
    return grpc_error_handler(data)

def admin():
    return TypedTabularInline().helper()
"#;

    let mut facts_map = BTreeMap::new();
    facts_map.insert(
        "packages/core/src/contextunity/core/types.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/types.py",
            "python",
            types_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "packages/core/src/contextunity/core/grpc_error_decorators.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/grpc_error_decorators.py",
            "python",
            decorators_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "packages/core/src/contextunity/core/grpc_errors.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/grpc_errors.py",
            "python",
            grpc_errors_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "packages/core/src/contextunity/core/__init__.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/__init__.py",
            "python",
            core_init_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "extensions/commerce/src/contextunity/commerce/typing.py".to_owned(),
        ast::extract(
            "extensions/commerce/src/contextunity/commerce/typing.py",
            "python",
            commerce_typing_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "consumer.py".to_owned(),
        ast::extract("consumer.py", "python", consumer_src).unwrap(),
    );

    let graph = linker::link(&facts_map);

    let coverages: Vec<_> = graph
        .coverage
        .iter()
        .filter(|c| c.path == "consumer.py")
        .collect();

    for c in &coverages {
        println!(
            "consumer coverage: line={} expr='{}' status='{}' evidence='{}'",
            c.line, c.expression, c.status, c.evidence
        );
    }

    let coverage_for = |expr: &str, line: usize| {
        graph
            .coverage
            .iter()
            .find(|c| c.path == "consumer.py" && c.expression == expr && c.line == line)
            .unwrap_or_else(|| panic!("missing coverage for {expr} at line {line} in consumer.py"))
    };

    // Import coverage
    let struct_data_import = coverage_for("StructData", 2);
    assert_eq!(
        struct_data_import.status, "resolved",
        "StructData import should resolve: {}",
        struct_data_import.evidence
    );

    let handler_import = coverage_for("grpc_error_handler", 2);
    assert_eq!(
        handler_import.status, "resolved",
        "grpc_error_handler import should resolve: {}",
        handler_import.evidence
    );

    let tabular_import = coverage_for("TypedTabularInline", 3);
    assert_eq!(
        tabular_import.status, "resolved",
        "TypedTabularInline import should resolve (not ambiguous): {}",
        tabular_import.evidence
    );

    // Call coverage
    let handler_call = coverage_for("grpc_error_handler", 6);
    assert_eq!(
        handler_call.status, "resolved",
        "grpc_error_handler call should resolve: {}",
        handler_call.evidence
    );

    let helper_call = coverage_for("TypedTabularInline().helper", 9);
    assert_eq!(
        helper_call.status, "resolved",
        "TypedTabularInline().helper call should resolve: {}",
        helper_call.evidence
    );
}
