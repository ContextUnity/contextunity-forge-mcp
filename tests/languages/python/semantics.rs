use super::*;

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
    let facts = ast::extract("decode.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let local_get = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "decode.LocalMapping.get")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("decode.py".to_owned(), facts)]));
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
    let w = PythonWorkspace::new();
    let source = "import logging\nlogging.basicConfig(level=logging.INFO)\nlogger = logging.getLogger(__name__)\nlogger.info('i')\nlogger.warning('w')\nlogger.error('e')\nlogger.debug('d')\nlogger.exception('x')\nlogger.process('x')\ndef unknown(logger):\n    logger.info('x')\n";
    w.write("src/factory.py", source);
    w.write("src/rebound.py", "import logging\nlogging = object()\nlogger = logging.getLogger(__name__)\nlogger.info('x')\n");
    let db = w.0.join(".forge/code-map.sqlite");
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
    };
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let cold = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    check(&cold);
    drop(cold);
    w.write("src/factory.py", &format!("{source}# index delta\n"));
    contextunity_forge_mcp::db::writer::delta(&w.0, &db, &[PathBuf::from("src/factory.py")])
        .unwrap();
    let delta = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    check(&delta);
}

#[test]
fn with_statement_as_target_uses_the_context_value() {
    let source = "class Conn:\n    def execute(self):\n        return 1\n\ndef use():\n    with Conn() as db:\n        db.execute()\n    db.execute()\n";
    let facts = ast::extract("ctx.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let execute = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "ctx.Conn.execute")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("ctx.py".to_owned(), facts)]));
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
fn stdlib_sqlite_and_logger_adapter_receivers_resolve_persisted_coverage() {
    let w = PythonWorkspace::new();
    let source = "import sqlite3\nimport logging\n\ndef run(conn: sqlite3.Connection, adapter: logging.LoggerAdapter):\n    conn.execute('SELECT 1')\n    conn.commit()\n    conn.rollback()\n    conn.cursor()\n    conn.close()\n    conn.unknown_method()\n    adapter.info('i')\n    adapter.warning('w')\n    adapter.error('e')\n    adapter.debug('d')\n    adapter.critical('c')\n    adapter.exception('x')\n    adapter.unknown_call()\n";
    w.write("src/service.py", source);
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    for (line, method) in [
        (5, "execute"),
        (6, "commit"),
        (7, "rollback"),
        (8, "cursor"),
        (9, "close"),
    ] {
        let expression = format!("conn.{method}");
        let (status, evidence) = persisted_coverage(&reader, "src/service.py", line, &expression);
        assert_eq!(status, "external", "{expression}: {evidence}");
        assert!(
            evidence.contains("builtin:sqlite3"),
            "{expression}: {evidence}"
        );
    }
    assert_eq!(
        persisted_status(&reader, "src/service.py", 10, "conn.unknown_method"),
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
        let (status, evidence) = persisted_coverage(&reader, "src/service.py", line, &expression);
        assert_eq!(status, "external", "{expression}: {evidence}");
        assert!(
            evidence.contains("builtin:logging.LoggerAdapter"),
            "{expression}: {evidence}"
        );
    }
    assert_eq!(
        persisted_status(&reader, "src/service.py", 17, "adapter.unknown_call"),
        "unresolved"
    );
}

#[test]
fn inherited_self_member_resolves_cross_file_mixin_methods() {
    let w = PythonWorkspace::new();
    w.write("src/pkg/__init__.py", "");
    w.write(
        "src/pkg/connection.py",
        "class SqliteConnectionMixin:\n    def _get_connection(self):\n        return self\n",
    );
    w.write(
        "src/pkg/projection.py",
        "from .connection import SqliteConnectionMixin\nclass SqliteCellEdgeProjectionLayer(SqliteConnectionMixin):\n    pass\n",
    );
    w.write(
        "src/pkg/validation.py",
        "from .projection import SqliteCellEdgeProjectionLayer\nclass SqliteCellEdgeValidationLayer(SqliteCellEdgeProjectionLayer):\n    pass\n",
    );
    w.write(
        "src/pkg/mutations.py",
        "from .validation import SqliteCellEdgeValidationLayer\nclass SqliteCellEdgeMutationLayer(SqliteCellEdgeValidationLayer, UnindexedBase):\n    def execute(self):\n        self._get_connection()\n        self._missing_method()\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    let (status, evidence) =
        persisted_coverage(&reader, "src/pkg/mutations.py", 4, "self._get_connection");
    assert_eq!(status, "resolved", "{evidence}");
    assert_eq!(
        persisted_status(&reader, "src/pkg/mutations.py", 5, "self._missing_method"),
        "unresolved"
    );
}

#[test]
fn call_return_type_and_context_manager_propagation_resolves_persisted_coverage() {
    let w = PythonWorkspace::new();
    w.write(
        "src/db_factory.py",
        "import sqlite3\n\ndef get_db() -> sqlite3.Connection:\n    return sqlite3.connect(':memory:')\n\ndef unannotated_db():\n    return sqlite3.connect(':memory:')\n",
    );
    w.write(
        "src/consumer.py",
        "import sqlite3\nfrom db_factory import get_db, unannotated_db\n\ndef run():\n    with get_db() as db:\n        db.execute('SELECT 1')\n    with unannotated_db() as raw:\n        raw.execute('SELECT 2')\n    conn = get_db()\n    conn.execute('SELECT 3')\n",
    );
    w.write(
        "src/mixin_consumer.py",
        "import sqlite3\nclass BaseRepo:\n    def get_conn(self) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\nclass ChildRepo(BaseRepo):\n    def perform(self):\n        with self.get_conn() as db:\n            db.execute('SELECT 4')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    // 1. with get_db() as db: db.execute(...) -> external builtin:sqlite3
    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 6, "db.execute");
    assert_eq!(status, "external", "db.execute: {evidence}");
    assert!(
        evidence.contains("builtin:sqlite3"),
        "db.execute evidence: {evidence}"
    );

    // 2. with unannotated_db() as raw: raw.execute(...) -> stays unresolved
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 8, "raw.execute"),
        "unresolved"
    );

    // 3. conn = get_db(); conn.execute(...) -> external builtin:sqlite3
    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 10, "conn.execute");
    assert_eq!(status, "external", "conn.execute: {evidence}");
    assert!(
        evidence.contains("builtin:sqlite3"),
        "conn.execute evidence: {evidence}"
    );

    // 4. with self.get_conn() as db: db.execute(...) in subclass -> external builtin:sqlite3
    let (status, evidence) = persisted_coverage(&reader, "src/mixin_consumer.py", 9, "db.execute");
    assert_eq!(status, "external", "self.get_conn() db.execute: {evidence}");
    assert!(
        evidence.contains("builtin:sqlite3"),
        "self.get_conn() db.execute evidence: {evidence}"
    );
}

#[test]
fn loop_iterable_annotation_element_inference_resolves_persisted_coverage() {
    let w = PythonWorkspace::new();
    w.write(
        "src/loop_service.py",
        "from typing import Sequence, Mapping, Iterable\n\ndef process_list(items: list[dict]):\n    for row in items:\n        row.get('key')\n\ndef process_seq(items: Sequence[Mapping]):\n    for row in items:\n        row.get('key')\n\ndef process_iter(items: Iterable[dict]):\n    for row in items:\n        row.get('key')\n\ndef process_untyped(items):\n    for row in items:\n        row.get('key')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    // 1. list[dict] -> row.get is external builtin:dict
    let (status, evidence) = persisted_coverage(&reader, "src/loop_service.py", 5, "row.get");
    assert_eq!(status, "external", "process_list row.get: {evidence}");
    assert!(
        evidence.contains("Python standard library"),
        "process_list evidence: {evidence}"
    );

    // 2. Sequence[Mapping] -> row.get is external builtin:Mapping
    let (status, evidence) = persisted_coverage(&reader, "src/loop_service.py", 9, "row.get");
    assert_eq!(status, "external", "process_seq row.get: {evidence}");
    assert!(
        evidence.contains("Python standard library"),
        "process_seq evidence: {evidence}"
    );

    // 3. Iterable[dict] -> row.get is external builtin:dict
    let (status, evidence) = persisted_coverage(&reader, "src/loop_service.py", 13, "row.get");
    assert_eq!(status, "external", "process_iter row.get: {evidence}");
    assert!(
        evidence.contains("Python standard library"),
        "process_iter evidence: {evidence}"
    );

    // 4. untyped -> row.get stays unresolved
    assert_eq!(
        persisted_status(&reader, "src/loop_service.py", 17, "row.get"),
        "unresolved"
    );
}

#[test]
fn loop_element_types_stop_at_rebind_and_shadowed_sequence() {
    let w = PythonWorkspace::new();
    w.write(
        "src/loop_guard.py",
        "class Sequence:\n    pass\n\ndef rebound(items: list[dict]):\n    items = None\n    for row in items:\n        row.get('key')\n\ndef shadowed(items: Sequence[dict]):\n    for row in items:\n        row.get('key')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    assert_eq!(
        persisted_status(&reader, "src/loop_guard.py", 7, "row.get"),
        "unresolved"
    );
    assert_eq!(
        persisted_status(&reader, "src/loop_guard.py", 11, "row.get"),
        "unresolved"
    );
}

#[test]
fn nested_self_and_unbound_cls_do_not_take_the_enclosing_class() {
    let w = PythonWorkspace::new();
    w.write(
        "src/repo.py",
        "import sqlite3\n\nclass Repo:\n    def connect(cls) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\n    def save(self) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\n    def run(self):\n        def helper(self):\n            nested = self.save()\n            nested.execute('SELECT 1')\n        borrowed = cls.connect()\n        borrowed.execute('SELECT 2')\n\n    def make(cls):\n        owned = cls.connect()\n        owned.execute('SELECT 3')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    assert_eq!(
        persisted_status(&reader, "src/repo.py", 13, "nested.execute"),
        "unresolved"
    );
    assert_eq!(
        persisted_status(&reader, "src/repo.py", 15, "borrowed.execute"),
        "unresolved"
    );
    let (status, evidence) = persisted_coverage(&reader, "src/repo.py", 19, "owned.execute");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("builtin:sqlite3"), "{evidence}");
}

#[test]
fn global_name_declaration_does_not_mask_same_named_instance_field() {
    let workspace = PythonWorkspace::new();
    workspace.write(
        "src/fields.py",
        "class Client:\n    def run(self):\n        pass\n\nclass Box:\n    def __init__(self):\n        global client\n        self.client = Client()\n\ndef invoke(box: Box):\n    box.client.run()\n",
    );
    workspace.write(
        "src/nonlocal_fields.py",
        "class LocalClient:\n    def run(self):\n        pass\n\ndef outer():\n    client = None\n    class Box:\n        def __init__(self):\n            nonlocal client\n            self.client = LocalClient()\n        def invoke(self):\n            self.client.run()\n",
    );
    let reader = workspace.build();

    let (status, evidence) = persisted_coverage(&reader, "src/fields.py", 11, "box.client.run");
    assert_eq!(status, "resolved", "{evidence}");
    let targets: Vec<String> = reader
        .prepare(
            "SELECT dst.qualname FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=?2 AND e.kind='calls' ORDER BY dst.qualname",
        )
        .unwrap()
        .query_map(rusqlite::params!["src/fields.py", 11], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        targets
            .iter()
            .any(|target| target == "src.fields.Client.run"),
        "{targets:#?}"
    );
    assert_eq!(
        persisted_status(&reader, "src/nonlocal_fields.py", 12, "self.client.run"),
        "resolved"
    );
}

#[test]
fn awaited_generic_returns_provide_only_proven_loop_element_types() {
    let workspace = PythonWorkspace::new();
    workspace.write(
        "src/providers.py",
        "def passthrough(fn):\n    return fn\n\nasync def typed_rows() -> list[dict[str, object]]:\n    return []\n\nasync def untyped_rows():\n    return []\n\n@passthrough\nasync def decorated_rows() -> list[dict[str, object]]:\n    return []\n",
    );
    workspace.write(
        "src/consumer.py",
        "from providers import typed_rows, untyped_rows, decorated_rows\n\nasync def run():\n    typed_items = await typed_rows()\n    for item in typed_items:\n        item.get('typed')\n    untyped_items = await untyped_rows()\n    for unknown in untyped_items:\n        unknown.get('unknown')\n    decorated_items = await decorated_rows()\n    for decorated in decorated_items:\n        decorated.get('decorated')\n",
    );
    let reader = workspace.build();

    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 6, "item.get");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("Python standard library"), "{evidence}");
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 9, "unknown.get"),
        "unresolved"
    );
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 12, "decorated.get"),
        "unresolved"
    );
}

#[test]
fn awaited_typed_receiver_methods_preserve_async_return_types() {
    let workspace = PythonWorkspace::new();
    workspace.write(
        "src/models.py",
        "class Worker:\n    def work(self) -> None:\n        pass\n\nclass Client:\n    async def fetch(self) -> Worker:\n        return Worker()\n\n    async def fetch_all(self) -> list[Worker]:\n        return []\n",
    );
    workspace.write(
        "src/consumer.py",
        "from models import Client\n\nasync def typed(client: Client):\n    result = await client.fetch()\n    result.work()\n\nasync def untyped(client):\n    result = await client.fetch()\n    result.work()\n\nasync def shadowed(client: Client):\n    client = object()\n    result = await client.fetch()\n    result.work()\n\nasync def spaced(client: Client):\n    result = await client . fetch()\n    result.work()\n\nasync def typed_collection(client: Client):\n    results = await client.fetch_all()\n    for worker in results:\n        worker.work()\n",
    );
    let reader = workspace.build();

    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 5, "result.work");
    assert_eq!(status, "resolved", "{evidence}");
    assert!(
        evidence.contains("src.models.Worker.work"),
        "expected exact Worker.work provider evidence, got {evidence}"
    );
    let (spaced_status, spaced_evidence) =
        persisted_coverage(&reader, "src/consumer.py", 18, "result.work");
    assert_eq!(spaced_status, "resolved", "{spaced_evidence}");
    assert!(
        spaced_evidence.contains("src.models.Worker.work"),
        "expected exact Worker.work provider evidence after whitespace normalization, got {spaced_evidence}"
    );
    let (collection_status, collection_evidence) =
        persisted_coverage(&reader, "src/consumer.py", 23, "worker.work");
    assert_eq!(collection_status, "resolved", "{collection_evidence}");
    assert!(
        collection_evidence.contains("src.models.Worker.work"),
        "expected list element type from the typed async receiver, got {collection_evidence}"
    );
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 9, "result.work"),
        "unresolved",
        "an untyped async receiver must remain unknown"
    );
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 14, "result.work"),
        "unresolved",
        "a rebound async receiver must remain unknown"
    );
}
