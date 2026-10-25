use super::*;

#[test]
fn persisted_module_factory_bindings_reach_nested_methods_with_verified_provenance() {
    let source = r#"import logging
import sqlite3
from typing import Mapping
logger = logging.getLogger('worker')
row: Mapping[str, str] = {'name': 'Ada'}
rows: list[Mapping[str, str]] = []
cursor: sqlite3.Cursor = unknown_factory()
class Worker:
    def run(self):
        logger.info('ready')
        row.get('name')
        for item in rows:
            item.get('name')
        item.get('after')
        cursor.execute('select 1')
        row.missing_method()
        cursor.unsupported_method()
    def shadow(self, logger):
        logger.warning('shadowed')
    def rebound(self):
        logger = unknown_factory()
        logger.error('unknown')
"#;
    let (_workspace, db) = build_workspace(&[
        ("app.py", source),
        ("provider.py", "class Service:\n    def other(self): pass\n"),
    ]);
    let info = persisted_coverage(&db, "app.py", "logger.info");
    assert_eq!(info.len(), 1, "{info:?}");
    assert_eq!(info[0].0, "external", "{info:?}");
    assert_eq!(
        info[0].1, "builtin:logging.Logger built-in: logger.info",
        "{info:?}"
    );
    let mapping = persisted_coverage(&db, "app.py", "row.get");
    assert_eq!(mapping.len(), 1, "{mapping:?}");
    assert_eq!(mapping[0].0, "external", "{mapping:?}");
    let loop_element = persisted_coverage(&db, "app.py", "item.get");
    assert_eq!(loop_element.len(), 2, "{loop_element:?}");
    assert_eq!(loop_element[0].0, "external", "{loop_element:?}");
    assert_eq!(loop_element[1].0, "unresolved", "{loop_element:?}");
    let cursor = persisted_coverage(&db, "app.py", "cursor.execute");
    assert_eq!(cursor.len(), 1, "{cursor:?}");
    assert_eq!(cursor[0].0, "external", "{cursor:?}");
    for expression in [
        "row.missing_method",
        "cursor.unsupported_method",
        "logger.warning",
        "logger.error",
    ] {
        let coverage = persisted_coverage(&db, "app.py", expression);
        assert_eq!(coverage.len(), 1, "{expression}: {coverage:?}");
        assert_eq!(coverage[0].0, "unresolved", "{expression}: {coverage:?}");
    }
}
#[test]
fn persisted_untyped_logger_parameters_without_provider_stay_unresolved() {
    let (_workspace, db) = build_workspace(&[(
        "unproven.py",
        "def run(logger, log):\n    logger.info('unknown')\n    log.warning('unknown')\n",
    )]);
    let observed: Vec<_> = ["logger.info", "log.warning"]
        .into_iter()
        .map(|expression| {
            let coverage = persisted_coverage(&db, "unproven.py", expression);
            assert_eq!(coverage.len(), 1, "{expression}: {coverage:?}");
            (expression, coverage[0].0.clone())
        })
        .collect();
    assert_eq!(
        observed,
        [
            ("logger.info", "unresolved".to_owned()),
            ("log.warning", "unresolved".to_owned())
        ]
    );
}
#[test]
fn canonical_logging_factories_preserve_aliases_and_logger_origin() {
    let source = "import logging as log\nfrom logging import getLogger as factory\na = log.getLogger(__name__)\nb = factory('worker')\na.info('ready')\nb.warning('ready')\ndef run():\n    a.error('captured')\n";
    let facts = ast::extract("logging_factories.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([(
        "logging_factories.py".to_owned(),
        facts,
    )]));
    for expression in ["a.info", "b.warning", "a.error"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "external", "{expression}");
        assert_eq!(
            coverage.evidence,
            format!("builtin:logging.Logger built-in: {expression}")
        );
    }
}
#[test]
fn logging_factory_local_providers_and_mutation_boundaries_preserve_contracts() {
    let provider = "class Logger:\n    def info(self, message): pass\ndef getLogger(name) -> Logger:\n    return Logger()\n";
    let consumer = "import logging\nlogger = logging.getLogger('local')\nlogger.info('ready')\n";
    let all = BTreeMap::from([
        (
            "logging.py".to_owned(),
            ast::extract("logging.py", "python", provider).unwrap(),
        ),
        (
            "consumer.py".to_owned(),
            ast::extract("consumer.py", "python", consumer).unwrap(),
        ),
    ]);
    let target = all["logging.py"]
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with("Logger.info"))
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&all);
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == "logger.info" && edge.dst == target));
    for source in [
        "import logging\nlogging.getLogger = replacement\nlogger = logging.getLogger('changed')\nlogger.info('ready')\n",
        "import logging\nconsume(logging)\nlogger = logging.getLogger('escaped')\nlogger.info('ready')\n",
        "import logging\nnamespace = (logging)\nlogger = logging.getLogger('escaped')\nlogger.info('ready')\n",
        "import logging\nclass Custom: pass\nlogging.setLoggerClass(Custom)\nlogger = logging.getLogger('custom')\nlogger.info('ready')\n",
        "import logging as log\nfrom logging import setLoggerClass as setter\nsetter(Custom)\nlogger = log.getLogger('custom')\nlogger.info('ready')\n",
        "import logging\nnamespace = lambda: logging\nlogger = logging.getLogger('escaped')\nlogger.info('ready')\n",
        "from logging import getLogger as factory\nfactory = replacement\nlogger = factory('changed')\nlogger.info('ready')\n",
        "import logging\ndef run(logging):\n    logger = logging.getLogger('parameter')\n    logger.info('ready')\n",
    ] {
        let facts = ast::extract("logging_boundary.py", "python", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("logging_boundary.py".to_owned(), facts)]));
        let coverage = graph.coverage.iter().find(|coverage| coverage.expression == "logger.info").unwrap();
        assert_eq!(coverage.status, "unresolved", "{source}");
    }
}
#[test]
fn straightline_local_factory_returns_have_bounded_inferred_proof() {
    let source="class Client:\n    def execute(self): pass\ndef direct(): return Client()\ndef alias():\n    result=Client()\n    copy=result\n    return copy\ndef mutated():\n    result=Client()\n    result=unknown()\n    return result\ndef branch(condition):\n    if condition: return Client()\n    return Client()\ndef recursive(): return recursive()\ndef run():\n    direct().execute()\n    alias().execute()\n    mutated().execute()\n    branch(True).execute()\n    recursive().execute()\n";
    let facts = ast::extract("inferred_returns.py", "python", source).unwrap();
    for name in ["direct", "alias", "mutated", "recursive"] {
        let node = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, node);
        let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
        assert!(flow.return_value.is_some());
        assert!(flow.return_position.is_some());
    }
    let branch = facts
        .nodes
        .iter()
        .find(|node| node.name == "branch")
        .unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, branch);
    let flow: ValueFlowFacts =
        serde_json::from_value(details["value_flow"].clone()).unwrap_or_default();
    assert!(flow.return_value.is_none());
    let graph = linker::link(&BTreeMap::from([("inferred_returns.py".to_owned(), facts)]));
    for expression in ["direct().execute", "alias().execute"] {
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
        assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == expression
            && edge.confidence == "inferred"));
    }
    for expression in [
        "mutated().execute",
        "branch(True).execute",
        "recursive().execute",
    ] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            "unresolved",
            "{expression}"
        );
    }
}
#[test]
fn named_type_aliases_preserve_nominal_proof_and_unknown_alias_boundaries() {
    let source="from typing import TypeAlias, TypeAliasType\nclass Client:\n    def execute(self): pass\ntype Modern = Client\nLegacy: TypeAlias = Client\nRuntime = TypeAliasType('Runtime', Client)\ntype Maybe = Client | None\ntype Recursive = Recursive\ndef run(modern: Modern, legacy: Legacy, runtime: Runtime, maybe: Maybe, recursive: Recursive):\n    modern.execute()\n    legacy.execute()\n    runtime.execute()\n    maybe.execute()\n    recursive.execute()\n";
    let facts = ast::extract("aliases.py", "python", source).unwrap();
    for name in ["Modern", "Legacy", "Runtime"] {
        let node = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, node);
        let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
        assert!(matches!(&flow.alias_type,Some(TypeExpr::Named{name}) if name=="Client"));
    }
    let graph = linker::link(&BTreeMap::from([("aliases.py".to_owned(), facts)]));
    for expression in ["modern.execute", "legacy.execute", "runtime.execute"] {
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
    for expression in ["maybe.execute", "recursive.execute"] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            "unresolved",
            "{expression}"
        );
    }
    let rebound="from typing import TypeAlias\nclass Client:\n    def execute(self): pass\nLegacy: TypeAlias = Client\nLegacy = unknown()\ndef run(value: Legacy): value.execute()\n";
    let facts = ast::extract("rebound_alias.py", "python", rebound).unwrap();
    let graph = linker::link(&BTreeMap::from([("rebound_alias.py".to_owned(), facts)]));
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "value.execute")
            .unwrap()
            .status,
        "unresolved"
    );
}
#[test]
fn external_factory_requires_a_return_contract_before_receiver_inference() {
    let source="from unknown_sdk import factory\ndef run():\n    result = factory()\n    result.execute()\n    factory().execute()\n";
    let facts = ast::extract("external_factory.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("external_factory.py".to_owned(), facts)]));
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "factory")
            .unwrap()
            .status,
        "external"
    );
    for expression in ["result.execute", "factory().execute"] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            "unresolved",
            "{expression}"
        );
    }
}
#[test]
fn nested_python_calls_follow_only_unique_callable_returns() {
    let source = "class Client:\n    def execute(self): pass\ndef make():\n    return Client()\ndef choose():\n    return make\ndef run():\n    choose()().execute()\ndef shadow():\n    choose = unknown_factory()\n    choose()().execute()\ndef unknown():\n    return unknown_factory()\ndef uncertain():\n    unknown()().execute()\n";
    let facts = ast::extract("nested_calls.py", "python", source).unwrap();
    let target = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with("Client.execute"))
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("nested_calls.py".to_owned(), facts)]));
    for (line, expression, expected) in [
        (8, "choose()().execute", "resolved"),
        (11, "choose()().execute", "unresolved"),
        (15, "unknown()().execute", "unresolved"),
    ] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.line == line && coverage.expression == expression)
                .unwrap()
                .status,
            expected,
            "{expression} at {line}"
        );
    }
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.path == "nested_calls.py"
        && edge.line == 8
        && edge.evidence == "choose()().execute"
        && edge.dst == target));
}
#[test]
fn imported_constructor_provenance_is_checked_at_assignment_position() {
    let mut facts = BTreeMap::new();
    facts.insert(
        "client.py".to_owned(),
        ast::extract(
            "client.py",
            "python",
            "class Client:\n    def execute(self): pass\n",
        )
        .unwrap(),
    );
    facts.insert("consumer.py".to_owned(),ast::extract("consumer.py","python","def good():\n    from client import Client\n    local=Client(); local.execute()\ndef bad():\n    local=Client(); from client import Client; local.execute()\n").unwrap());
    let graph = linker::link(&facts);
    let calls: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| coverage.path == "consumer.py" && coverage.expression == "local.execute")
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].status, "resolved");
    assert_eq!(calls[1].status, "unresolved");
}
