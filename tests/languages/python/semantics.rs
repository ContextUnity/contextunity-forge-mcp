use super::*;

fn python_linked(path: &str, source: &str) -> crate::languages::harness::LinkedFacts {
    ExtractorHarness::for_lang("python")
        .at_path(path)
        .linked(source)
}

fn python_extracted(path: &str, source: &str) -> contextunity_forge_mcp::core::models::Facts {
    ExtractorHarness::for_lang("python")
        .at_path(path)
        .extract(source)
}

#[test]
fn typed_parameters_preserve_forward_qualified_and_default_types_in_their_scope() {
    let source = "class Service:\n    def execute(self): pass\ndef run(service: Service, forward: 'pkg.Service', default: pkg.Service = None, *items: Service, **options: 'pkg.Service'):\n    def nested(service: Other): pass\n    return service.execute()\nclass Client:\n    def call(self, service: Service): return service.execute()\ndef untyped(service): return service.execute()\n";
    let facts = python_extracted("parameters.py", source);
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let node = |name| facts.nodes.iter().find(|node| node.name == name).unwrap();
    let details = |name| crate::common::assertions::projected_node_details(&facts, node(name));
    assert_eq!(
        details("run")["param_types"],
        json!({"service":"Service", "forward":"pkg.Service", "default":"pkg.Service"})
    );
    assert_eq!(details("nested")["param_types"], json!({"service":"Other"}));
    assert_eq!(details("call")["param_types"], json!({"service":"Service"}));
    assert!(details("untyped").get("param_types").is_none());
}

#[test]
fn class_assignments_are_bindings_without_leaking_nested_callable_assignments() {
    let facts = ExtractorHarness::for_lang("python")
        .at_path("class_bindings.py")
        .extract("class Model:\n    objects = factory()\n    execute: object = custom\n    def method(self):\n        local = 1\n    class Nested:\n        nested = 1\n");
    let model = facts
        .nodes
        .iter()
        .find(|node| node.name == "Model")
        .unwrap();
    assert_eq!(
        crate::common::assertions::projected_node_details(&facts, model)["bindings"],
        json!(["execute", "objects"])
    );
}

#[test]
fn computed_receiver_hints_are_structural_and_preserve_original_expression() {
    use contextunity_forge_mcp::core::semantic::{SourcePosition, ValueExpr};
    let source = "def run():\n    'value'.upper()\n    r'value'.strip()\n    b'value'.upper()\n    f'{value}'.upper()\n    Widget().execute()\n    package.Widget().execute()\n    super().execute()\n    super(Widget, self).execute()\n    response.request().consume()\n    factory().request().consume()\n";
    let facts = python_extracted("computed.py", source);
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
        matches!(hint("Widget().execute"), Some(ReceiverHint::TypedCallResult { value: ValueExpr::Call { callee, site: None }, member }) if callee == "Widget" && member == "execute")
    );
    assert!(
        matches!(hint("package.Widget().execute"), Some(ReceiverHint::TypedCallResult { value: ValueExpr::Call { callee, site: None }, member }) if callee == "package.Widget" && member == "execute")
    );
    assert!(
        matches!(hint("super().execute"), Some(ReceiverHint::Super { member }) if member == "execute")
    );
    assert!(hint("super(Widget, self).execute").is_none());
    assert!(
        matches!(hint("response.request().consume"), Some(ReceiverHint::TypedCallResult { value: ValueExpr::Call { callee, site: None }, member }) if callee == "response.request" && member == "consume")
    );
    let Some(ReceiverHint::TypedCallResult {
        value: ValueExpr::Call {
            callee,
            site: Some(site),
        },
        member,
    }) = hint("factory().request().consume")
    else {
        panic!("nested receiver must retain its canonical call evidence");
    };
    assert_eq!(callee, "request");
    assert_eq!(member, "consume");
    assert_eq!(
        site.position,
        SourcePosition {
            line: 11,
            column: 4
        }
    );
    assert!(site.arguments.is_empty());
    assert!(
        matches!(site.receiver.as_deref(), Some(ValueExpr::Call { callee, site: None }) if callee == "factory")
    );
}

#[test]
fn verified_constructor_super_and_known_string_methods_resolve_at_the_linker() {
    let source = "class Base:\n    def execute(self): pass\nclass Child(Base):\n    def execute(self): pass\n    def run(self): return super().execute()\ndef direct(): return Base().execute()\ndef literal(): return 'value'.upper()\ndef invalid(): return 'value'.invented_method()\n";
    let graph = python_linked("resolved.py", source);
    let facts = &graph.facts;
    let base_method = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "resolved.Base.execute")
        .unwrap()
        .id
        .clone();
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
    keys = raw.keys()
    values = raw.values()
    logger.info("decoded")
    logger.warning("retry")
    logger.debug("details")
    logger.error("failed")

def helper():
    row = {"id": 1}
    row.get("id")
    row.keys()
    row.values()
    row.update({"next": 2})
    return row.pop("id")

class LocalMapping:
    def get(self, key): return key

def local(mapping: LocalMapping):
    return mapping.get("field")

def conventional(logger, log):
    logger.exception("failed")
    log.debug("details")
"#;
    let graph = python_linked("decode.py", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let local_get = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "decode.LocalMapping.get")
        .unwrap()
        .id
        .clone();
    let local = graph
        .coverage
        .iter()
        .find(|coverage| coverage.expression == "mapping.get")
        .unwrap();
    assert_eq!(local.status, "resolved", "{local:#?}");
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.evidence == "mapping.get"
        && edge.dst == local_get));
    for expression in [
        "raw.get",
        "raw.items",
        "raw.keys",
        "raw.values",
        "row.get",
        "row.keys",
        "row.values",
        "row.update",
        "row.pop",
    ] {
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
        assert!(
            coverage.evidence.contains("Python standard library"),
            "{expression}: {}",
            coverage.evidence
        );
    }
    for expression in [
        "logger.info",
        "logger.warning",
        "logger.debug",
        "logger.error",
    ] {
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
        assert!(
            coverage.evidence.contains("line 3"),
            "{}",
            coverage.evidence
        );
    }
    for expression in ["logger.exception", "log.debug"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "unresolved", "{expression}: {coverage:#?}");
    }
}

#[test]
fn logging_factory_methods_have_verified_builtin_origin_after_persistence() {
    let w = fixture_workspace(&[]);
    let source = "import logging\nlogging.basicConfig(level=logging.INFO)\nlogger = logging.getLogger(__name__)\nlogger.info('i')\nlogger.warning('w')\nlogger.error('e')\nlogger.debug('d')\nlogger.exception('x')\nlogger.process('x')\ndef unknown(logger):\n    logger.info('x')\n";
    let service_source = "import sqlite3\nimport logging\n\ndef run(conn: sqlite3.Connection, adapter: logging.LoggerAdapter):\n    conn.execute('SELECT 1')\n    conn.commit()\n    conn.rollback()\n    conn.cursor()\n    conn.close()\n    conn.unknown_method()\n    adapter.info('i')\n    adapter.warning('w')\n    adapter.error('e')\n    adapter.debug('d')\n    adapter.critical('c')\n    adapter.exception('x')\n    adapter.unknown_call()\n";
    w.write("src/factory.py", source);
    w.write(
        "src/rebound.py",
        "import logging\nlogging = object()\nlogger = logging.getLogger(__name__)\nlogger.info('x')\n",
    );
    w.write("src/service.py", service_source);
    let check = |conn: &rusqlite::Connection| {
        for (line, method) in [
            (4, "info"),
            (5, "warning"),
            (6, "error"),
            (7, "debug"),
            (8, "exception"),
        ] {
            let expression = format!("logger.{method}");
            let (status, evidence) = persisted_coverage(conn, "src/factory.py", line, &expression);
            assert_eq!(status, "external", "{expression}: {evidence}");
            assert_eq!(
                evidence,
                format!("builtin:logging.Logger built-in: {expression}")
            );
        }
        assert_eq!(
            persisted_status(conn, "src/factory.py", 9, "logger.process"),
            "unresolved"
        );
        assert_eq!(
            persisted_status(conn, "src/factory.py", 11, "logger.info"),
            "unresolved"
        );
        assert_eq!(
            persisted_status(conn, "src/rebound.py", 4, "logger.info"),
            "unresolved"
        );

        let (status, evidence) = persisted_coverage(conn, "src/service.py", 1, "sqlite3");
        assert_eq!(status, "external");
        assert!(evidence.contains("standard library"), "{evidence}");

        let (status, evidence) =
            persisted_coverage(conn, "src/service.py", 4, "sqlite3.Connection");
        assert_eq!(status, "external");
        assert!(evidence.contains("sqlite3"), "{evidence}");

        for (line, method) in [
            (5, "execute"),
            (6, "commit"),
            (7, "rollback"),
            (8, "cursor"),
            (9, "close"),
        ] {
            let expression = format!("conn.{method}");
            let (status, evidence) = persisted_coverage(conn, "src/service.py", line, &expression);
            assert_eq!(status, "external", "{expression}: {evidence}");
            assert!(
                evidence.contains("builtin:sqlite3"),
                "{expression}: {evidence}"
            );
        }
        assert_eq!(
            persisted_status(conn, "src/service.py", 10, "conn.unknown_method"),
            "unresolved"
        );

        for (line, method) in [
            (11, "info"),
            (12, "warning"),
            (13, "error"),
            (14, "debug"),
            (15, "critical"),
            (16, "exception"),
        ] {
            let expression = format!("adapter.{method}");
            let (status, evidence) = persisted_coverage(conn, "src/service.py", line, &expression);
            assert_eq!(status, "external", "{expression}: {evidence}");
            assert!(
                evidence.contains("builtin:logging.LoggerAdapter"),
                "{expression}: {evidence}"
            );
        }
        assert_eq!(
            persisted_status(conn, "src/service.py", 17, "adapter.unknown_call"),
            "unresolved"
        );

        let contains_edges: i64 = query_scalar(
            conn,
            "SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path='src/service.py' AND o.kind='contains'",
            [],
        );
        assert_eq!(contains_edges, 2);

        let calls_edges: i64 = query_scalar(
            conn,
            "SELECT count(*) FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id WHERE p.path='src/service.py' AND o.kind='calls'",
            [],
        );
        assert_eq!(
            calls_edges, 0,
            "no fabricated calls edges should be emitted for unknown calls"
        );
    };
    w.build();
    let cold = w.open();
    check(&cold);
    drop(cold);
    w.write("src/factory.py", &format!("{source}# index delta\n"));
    w.write(
        "src/service.py",
        &format!("{service_source}# index delta\n"),
    );
    w.delta(&["src/factory.py", "src/service.py"]);
    let delta = w.open();
    check(&delta);
}

#[test]
fn with_statement_as_target_uses_the_context_value() {
    let source = "class Conn:\n    def execute(self):\n        return 1\n\ndef use():\n    with Conn() as db:\n        db.execute()\n    db.execute()\n";
    let graph = python_linked("ctx.py", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let execute = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "ctx.Conn.execute")
        .unwrap()
        .id
        .clone();
    let rows: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| coverage.expression == "db.execute")
        .collect();
    assert_eq!(rows.len(), 2, "{rows:#?}");
    assert!(
        rows.iter().all(|coverage| coverage.status == "resolved"),
        "{rows:#?}"
    );
    let calls = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == "db.execute" && edge.dst == execute)
        .count();
    assert_eq!(calls, 2);
}

#[test]
fn inherited_self_member_resolves_cross_file_mixin_methods() {
    let catalog = ExtractorHarness::linked_with_root(
        "python",
        &[
            ("pkg/__init__.py", ""),
            (
                "pkg/connection.py",
                "class SqliteConnectionMixin:\n    def _get_connection(self):\n        return self\n",
            ),
            (
                "pkg/projection.py",
                "from .connection import SqliteConnectionMixin\nclass SqliteCellEdgeProjectionLayer(SqliteConnectionMixin):\n    pass\n",
            ),
            (
                "pkg/validation.py",
                "from .projection import SqliteCellEdgeProjectionLayer\nclass SqliteCellEdgeValidationLayer(SqliteCellEdgeProjectionLayer):\n    pass\n",
            ),
            (
                "pkg/mutations.py",
                "from .validation import SqliteCellEdgeValidationLayer\nclass SqliteCellEdgeMutationLayer(SqliteCellEdgeValidationLayer, UnindexedBase):\n    def execute(self):\n        self._get_connection()\n        self._missing_method()\n",
            ),
        ],
        None,
    );

    assert_eq!(
        catalog.coverage_line("pkg/mutations.py", 4, "self._get_connection"),
        "resolved"
    );
    assert_eq!(
        catalog.coverage_line("pkg/mutations.py", 5, "self._missing_method"),
        "unresolved"
    );
}

#[test]
fn call_return_type_and_context_manager_propagation_resolves_receiver_types() {
    let catalog = ExtractorHarness::linked_with_root(
        "python",
        &[
            (
                "db_factory.py",
                "import sqlite3\n\ndef get_db() -> sqlite3.Connection:\n    return sqlite3.connect(':memory:')\n\ndef unannotated_db():\n    return sqlite3.connect(':memory:')\n",
            ),
            (
                "consumer.py",
                "import sqlite3\nfrom db_factory import get_db, unannotated_db\n\ndef run():\n    with get_db() as db:\n        db.execute('SELECT 1')\n    with unannotated_db() as raw:\n        raw.execute('SELECT 2')\n    conn = get_db()\n    conn.execute('SELECT 3')\n",
            ),
            (
                "mixin_consumer.py",
                "import sqlite3\nclass BaseRepo:\n    def get_conn(self) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\nclass ChildRepo(BaseRepo):\n    def perform(self):\n        with self.get_conn() as db:\n            db.execute('SELECT 4')\n",
            ),
        ],
        None,
    );

    assert_eq!(
        catalog.coverage_line("consumer.py", 8, "raw.execute"),
        "unresolved"
    );
    for (path, line, expression) in [
        ("consumer.py", 6, "db.execute"),
        ("consumer.py", 10, "conn.execute"),
        ("mixin_consumer.py", 9, "db.execute"),
    ] {
        let evidence = catalog
            .graph
            .coverage
            .iter()
            .find(|coverage| {
                coverage.path == path && coverage.line == line && coverage.expression == expression
            })
            .unwrap();
        assert_eq!(evidence.status, "external", "{expression}: {evidence:#?}");
        assert!(
            evidence.evidence.contains("builtin:sqlite3"),
            "{expression}: {evidence:#?}"
        );
    }
}

#[test]
fn loop_element_types_require_proven_unshadowed_iterables() {
    let resolved_source = concat!(
        "from typing import Sequence, Mapping, Iterable\n\n",
        "def process_list(items: list[dict]):\n",
        "    for row in items:\n        row.get('key')\n\n",
        "def process_seq(items: Sequence[Mapping]):\n",
        "    for row in items:\n        row.get('key')\n\n",
        "def process_iter(items: Iterable[dict]):\n",
        "    for row in items:\n        row.get('key')\n\n",
        "def process_untyped(items):\n",
        "    for row in items:\n        row.get('key')\n",
    );
    let resolved = python_linked("loop_service.py", resolved_source);
    for line in [5, 9, 13] {
        let coverage = resolved
            .coverage("row.get")
            .find(|coverage| coverage.line == line)
            .unwrap();
        assert_eq!(coverage.status, "external", "{coverage:#?}");
        assert!(
            coverage.evidence.contains("Python standard library"),
            "{coverage:#?}"
        );
    }
    assert_eq!(
        resolved
            .coverage("row.get")
            .find(|coverage| coverage.line == 17)
            .unwrap()
            .status,
        "unresolved"
    );

    let guarded_source = concat!(
        "class Sequence:\n    pass\n\n",
        "def rebound(items: list[dict]):\n",
        "    items = None\n    for row in items:\n        row.get('key')\n\n",
        "def shadowed(items: Sequence[dict]):\n",
        "    for row in items:\n        row.get('key')\n",
    );
    let guarded = python_linked("loop_guard.py", guarded_source);
    for line in [7, 11] {
        assert_eq!(
            guarded
                .coverage("row.get")
                .find(|coverage| coverage.line == line)
                .unwrap()
                .status,
            "unresolved"
        );
    }
}

#[test]
fn nested_self_and_unbound_cls_do_not_take_the_enclosing_class() {
    let graph = python_linked(
        "src/repo.py",
        "import sqlite3\n\nclass Repo:\n    def connect(cls) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\n    def save(self) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\n    def run(self):\n        def helper(self):\n            nested = self.save()\n            nested.execute('SELECT 1')\n        borrowed = cls.connect()\n        borrowed.execute('SELECT 2')\n\n    def make(cls):\n        owned = cls.connect()\n        owned.execute('SELECT 3')\n",
    );
    assert_eq!(
        graph.coverage("nested.execute").next().unwrap().status,
        "unresolved"
    );
    assert_eq!(
        graph.coverage("borrowed.execute").next().unwrap().status,
        "unresolved"
    );
    let owned = graph.coverage("owned.execute").next().unwrap();
    assert_eq!(owned.status, "external");
    assert!(owned.evidence.contains("builtin:sqlite3"), "{owned:#?}");
}

#[test]
fn global_name_declaration_does_not_mask_same_named_instance_field() {
    let catalog = ExtractorHarness::linked_with_root(
        "python",
        &[
            (
                "fields.py",
                "class Client:\n    def run(self):\n        pass\n\nclass Box:\n    def __init__(self):\n        global client\n        self.client = Client()\n\ndef invoke(box: Box):\n    box.client.run()\n",
            ),
            (
                "nonlocal_fields.py",
                "class LocalClient:\n    def run(self):\n        pass\n\ndef outer():\n    client = None\n    class Box:\n        def __init__(self):\n            nonlocal client\n            self.client = LocalClient()\n        def invoke(self):\n            self.client.run()\n",
            ),
        ],
        None,
    );
    assert_eq!(
        catalog.coverage_line("fields.py", 11, "box.client.run"),
        "resolved"
    );
    let targets = catalog.edge_qualnames("fields.invoke", "calls");
    assert!(
        targets.iter().any(|target| target == "fields.Client.run"),
        "{targets:#?}"
    );
    assert_eq!(
        catalog.coverage_line("nonlocal_fields.py", 12, "self.client.run"),
        "resolved"
    );
}

#[test]
fn awaited_generic_returns_provide_only_proven_loop_element_types() {
    let facts = [
        (
            "providers.py",
            "def passthrough(fn):\n    return fn\n\nasync def typed_rows() -> list[dict[str, object]]:\n    return []\n\nasync def untyped_rows():\n    return []\n\n@passthrough\nasync def decorated_rows() -> list[dict[str, object]]:\n    return []\n",
        ),
        (
            "consumer.py",
            "from providers import typed_rows, untyped_rows, decorated_rows\n\nasync def run():\n    typed_items = await typed_rows()\n    for item in typed_items:\n        item.get('typed')\n    untyped_items = await untyped_rows()\n    for unknown in untyped_items:\n        unknown.get('unknown')\n    decorated_items = await decorated_rows()\n    for decorated in decorated_items:\n        decorated.get('decorated')\n",
        ),
    ]
    .into_iter()
    .map(|(path, source)| {
        (
            path.to_owned(),
            python_extracted(path, source),
        )
    })
    .collect::<std::collections::BTreeMap<_, _>>();
    let graph = ExtractorHarness::link_graph(&facts);
    let typed = graph
        .coverage
        .iter()
        .find(|coverage| {
            coverage.path == "consumer.py"
                && coverage.line == 6
                && coverage.expression == "item.get"
        })
        .unwrap();
    assert_eq!(typed.status, "external");
    assert!(
        typed.evidence.contains("Python standard library"),
        "{typed:#?}"
    );
    for (line, expression) in [(9, "unknown.get"), (12, "decorated.get")] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| {
                    coverage.path == "consumer.py"
                        && coverage.line == line
                        && coverage.expression == expression
                })
                .unwrap()
                .status,
            "unresolved"
        );
    }
}

#[test]
fn awaited_typed_receiver_methods_preserve_async_return_types() {
    let catalog = ExtractorHarness::linked_with_root(
        "python",
        &[
            (
                "models.py",
                "class Worker:\n    def work(self) -> None:\n        pass\n\nclass Client:\n    async def fetch(self) -> Worker:\n        return Worker()\n\n    async def fetch_all(self) -> list[Worker]:\n        return []\n",
            ),
            (
                "consumer.py",
                "from models import Client\n\nasync def typed(client: Client):\n    result = await client.fetch()\n    result.work()\n\nasync def untyped(client):\n    result = await client.fetch()\n    result.work()\n\nasync def shadowed(client: Client):\n    client = object()\n    result = await client.fetch()\n    result.work()\n\nasync def spaced(client: Client):\n    result = await client . fetch()\n    result.work()\n\nasync def typed_collection(client: Client):\n    results = await client.fetch_all()\n    for worker in results:\n        worker.work()\n",
            ),
        ],
        None,
    );
    assert_eq!(
        catalog.coverage_line("consumer.py", 5, "result.work"),
        "resolved"
    );
    for (function, expression) in [
        ("consumer.typed", "result.work"),
        ("consumer.spaced", "result.work"),
        ("consumer.typed_collection", "worker.work"),
    ] {
        assert!(
            catalog
                .edge_qualnames(function, "calls")
                .iter()
                .any(|target| target == "models.Worker.work"),
            "{function} should resolve {expression} to Worker.work"
        );
    }
    assert_eq!(
        catalog.coverage_line("consumer.py", 18, "result.work"),
        "resolved"
    );
    assert_eq!(
        catalog.coverage_line("consumer.py", 23, "worker.work"),
        "resolved"
    );
    assert_eq!(
        catalog.coverage_line("consumer.py", 9, "result.work"),
        "unresolved",
        "an untyped async receiver must remain unknown"
    );
    assert_eq!(
        catalog.coverage_line("consumer.py", 14, "result.work"),
        "unresolved",
        "a rebound async receiver must remain unknown"
    );
}
