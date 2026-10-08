#![cfg(feature = "lang-python")]
use contextunity_forge_mcp::{
    core::semantic::{TypeExpr, ValueExpr, ValueFlowFacts},
    engine::{ast, linker},
};
use std::collections::BTreeMap;

use crate::common::Workspace;

fn build_workspace(files: &[(&str, &str)]) -> (Workspace, rusqlite::Connection) {
    let workspace = Workspace::new();
    for (path, source) in files {
        workspace.write(path, source);
    }
    workspace.build();
    let connection = workspace.open();
    (workspace, connection)
}

fn persisted_coverage(
    db: &rusqlite::Connection,
    path: &str,
    expression: &str,
) -> Vec<(String, String)> {
    let mut statement = db
        .prepare(
            "SELECT coverage.status, evidence.evidence \
             FROM resolution_coverage AS coverage \
             JOIN path_dictionary AS paths ON paths.path_id = coverage.path_id \
             JOIN coverage_expressions AS expressions ON expressions.expression_id = coverage.expression_id \
             JOIN coverage_evidence AS evidence ON evidence.evidence_id = coverage.evidence_id \
             WHERE paths.path = ?1 AND expressions.expression = ?2 \
             ORDER BY coverage.line, coverage.status",
        )
        .unwrap();
    statement
        .query_map([path, expression], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn persisted_edge(
    db: &rusqlite::Connection,
    owner_path: &str,
    kind: &str,
    expression: &str,
    target_path: &str,
    target_qualname: &str,
) -> bool {
    db.query_row(
        "SELECT EXISTS( \
         SELECT 1 FROM edge_occurrences AS occurrence \
         JOIN path_dictionary AS owner ON owner.path_id = occurrence.owner_id \
         JOIN coverage_evidence AS evidence ON evidence.evidence_id = occurrence.evidence_id \
         JOIN nodes AS target ON target.node_hash = occurrence.dst_hash \
         WHERE owner.path = ?1 AND occurrence.kind = ?2 AND evidence.evidence = ?3 \
         AND (SELECT path FROM path_dictionary WHERE path_id=target.path_id) = ?4 AND target.qualname = ?5)",
        rusqlite::params![owner_path, kind, expression, target_path, target_qualname],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn loop_rebinding_keeps_the_prior_receiver_from_leaking_past_the_loop() {
    let (_workspace, db) = build_workspace(&[(
        "loop.py",
        "class Worker:\n    def work(self): pass\ndef run(values):\n    current = Worker()\n    current.work()\n    for current in values:\n        pass\n    current.work()\n",
    )]);
    let calls = persisted_coverage(&db, "loop.py", "current.work");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].0, "resolved", "{calls:?}");
    assert_eq!(calls[1].0, "unresolved", "{calls:?}");
}

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
fn persisted_imported_typed_dict_returns_keep_finite_mapping_members() {
    for root in ["named", "synapse"] {
        let schema = format!("from typing import TypedDict\nclass {root}Node(TypedDict, total=False):\n    name: str\n");
        let provider = format!("from . import schema as types\ndef load() -> types.{root}Node:\n    return {{'name': 'Ada'}}\n");
        let consumer = "from .provider import load\nfrom .fake import load_fake\nnode = load()\nnode.get('name')\nload().get('name')\nnode.unsupported_method()\nfake = load_fake()\nfake.get('name')\n";
        let (_workspace, db) = build_workspace(&[
            ("pkg/__init__.py", ""),
            ("pkg/schema.py", &schema),
            ("pkg/provider.py", &provider),
            ("pkg/fake.py", "class TypedDict: pass\nclass Fake(TypedDict): pass\ndef load_fake() -> Fake:\n    return Fake()\n"),
            ("pkg/consumer.py", consumer),
        ]);
        let get = persisted_coverage(&db, "pkg/consumer.py", "node.get");
        assert_eq!(get.len(), 1, "{root}: {get:?}");
        assert_eq!(get[0].0, "external", "{root}: {get:?}");
        assert!(
            get[0].1.contains("Python standard library"),
            "{root}: {get:?}"
        );
        let direct = persisted_coverage(&db, "pkg/consumer.py", "load().get");
        assert_eq!(direct.len(), 1, "{root}: {direct:?}");
        assert_eq!(direct[0].0, "external", "{root}: {direct:?}");
        let unknown = persisted_coverage(&db, "pkg/consumer.py", "node.unsupported_method");
        assert_eq!(unknown.len(), 1, "{root}: {unknown:?}");
        assert_eq!(unknown[0].0, "unresolved", "{root}: {unknown:?}");
        let fake = persisted_coverage(&db, "pkg/consumer.py", "fake.get");
        assert_eq!(fake.len(), 1, "{root}: {fake:?}");
        assert_eq!(fake[0].0, "unresolved", "{root}: {fake:?}");
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
fn persisted_imported_factory_return_reaches_only_its_own_project_provider() {
    let provider = r#"class Base:
    def base(self): pass
class Service(Base):
    def execute(self): pass
    def replay(self):
        super().base()
def make_service() -> Service:
    return Service()
"#;
    let consumer = r#"from provider import make_service
service = make_service()
changed = make_service()
changed = unknown_factory()
mystery = unknown_factory()
if condition:
    conditional = make_service()
class Runner:
    def run(self):
        service.execute()
        make_service().execute()
        service.absent()
        changed.execute()
        mystery.execute()
        conditional.execute()
    def shadow(self, service):
        service.execute()
"#;
    let (_workspace, db) =
        build_workspace(&[("provider.py", provider), ("consumer.py", consumer)]);
    assert!(persisted_edge(
        &db,
        "consumer.py",
        "imports",
        "make_service",
        "provider.py",
        "provider.make_service"
    ));
    let service = persisted_coverage(&db, "consumer.py", "service.execute");
    assert_eq!(service.len(), 2, "{service:?}");
    assert_eq!(service[0].0, "resolved", "{service:?}");
    assert_eq!(service[1].0, "unresolved", "{service:?}");
    assert!(persisted_edge(
        &db,
        "consumer.py",
        "calls",
        "service.execute",
        "provider.py",
        "provider.Service.execute"
    ));
    let computed = persisted_coverage(&db, "consumer.py", "make_service().execute");
    assert_eq!(computed.len(), 1, "{computed:?}");
    assert_eq!(computed[0].0, "resolved", "{computed:?}");
    assert!(persisted_edge(
        &db,
        "consumer.py",
        "calls",
        "make_service().execute",
        "provider.py",
        "provider.Service.execute"
    ));
    let inherited = persisted_coverage(&db, "provider.py", "super().base");
    assert_eq!(inherited.len(), 1, "{inherited:?}");
    assert_eq!(inherited[0].0, "resolved", "{inherited:?}");
    assert!(persisted_edge(
        &db,
        "provider.py",
        "calls",
        "super().base",
        "provider.py",
        "provider.Base.base"
    ));
    for expression in [
        "service.absent",
        "changed.execute",
        "mystery.execute",
        "conditional.execute",
    ] {
        let coverage = persisted_coverage(&db, "consumer.py", expression);
        assert_eq!(coverage.len(), 1, "{expression}: {coverage:?}");
        assert_eq!(coverage[0].0, "unresolved", "{expression}: {coverage:?}");
    }
}

#[test]
fn collection_literals_propagate_intrinsic_types_through_aliases_returns_and_fields() {
    let source = "class dict:\n    def custom(self): pass\nclass list: pass\nclass set: pass\nclass tuple: pass\nraw: dict = {}\nalias = raw\nalias.get('key')\nitems: list[dict] = []\nitems.append(1)\nunique = {1}\nunique.add(2)\nordered = (1,)\nordered.count(1)\ncustom = dict()\ncustom.custom()\ndef make():\n    return {'key': 1}\ncreated = make()\ncreated.get('key')\nclass Store:\n    def __init__(self): self.data = {}\nstore = Store()\nstore.data.get('key')\nraw = unknown()\nraw.get('key')\n";
    let facts = ast::extract("literal_collections.py", "python", source).unwrap();
    let module = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    let flow: ValueFlowFacts =
        serde_json::from_value(module.details["value_flow"].clone()).unwrap();
    for (name, spelling) in [
        ("raw", "{}"),
        ("items", "[]"),
        ("unique", "{...}"),
        ("ordered", "()"),
    ] {
        assert_eq!(
            flow.bindings
                .iter()
                .find(|binding| binding.name == name)
                .unwrap()
                .value,
            ValueExpr::Construct {
                callee: spelling.to_owned()
            }
        );
    }
    let mut keys = Vec::new();
    flow.reference_keys(|key| keys.push(key));
    assert_eq!(keys, ["dict", "raw", "dict", "make", "Store", "unknown"]);
    let mut durable_keys = Vec::new();
    ValueFlowFacts::reference_keys_from_details(&module.details, |key| durable_keys.push(key));
    assert_eq!(durable_keys, keys);
    let graph = linker::link(&BTreeMap::from([(
        "literal_collections.py".to_owned(),
        facts,
    )]));
    for expression in [
        "alias.get",
        "items.append",
        "unique.add",
        "ordered.count",
        "created.get",
        "store.data.get",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("Python standard library"),
            "{expression}: {coverage:#?}"
        );
    }
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.expression == "custom.custom" && coverage.status == "resolved"));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == "custom.custom"));
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "raw.get")
            .unwrap()
            .status,
        "unresolved"
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
fn container_aliases_preserve_arguments_and_require_builtin_base_proof() {
    let source="from typing import Optional, Union\nclass Client:\n    def execute(self): pass\nclass Box:\n    def get(self): pass\nclass list:\n    def append(self, value): pass\ntype JsonValue = str | int | None\ntype JsonDict = dict[str, JsonValue]\ntype Maybe = Optional[Client]\ntype Either = Union[Client, str]\ntype Custom = Box[str]\ntype Shadowed = list[Client]\ndef use(data: JsonDict, maybe: Maybe, either: Either, custom: Custom, shadowed: Shadowed):\n    data.get('key')\n    maybe.execute()\n    either.execute()\n    custom.get()\n    shadowed.append(1)\n";
    let facts = ast::extract("containers.py", "python", source).unwrap();
    let alias = facts
        .nodes
        .iter()
        .find(|node| node.name == "JsonDict")
        .unwrap();
    let flow: ValueFlowFacts = serde_json::from_value(alias.details["value_flow"].clone()).unwrap();
    assert!(
        matches!(flow.alias_type,Some(TypeExpr::Applied{base,args}) if base=="dict" && args==vec![TypeExpr::Named{name:"str".into()},TypeExpr::Named{name:"JsonValue".into()}])
    );
    let graph = linker::link(&BTreeMap::from([("containers.py".to_owned(), facts)]));
    let data_get = graph
        .coverage
        .iter()
        .find(|coverage| coverage.expression == "data.get")
        .unwrap();
    assert_eq!(data_get.status, "external", "{data_get:#?}");
    assert!(data_get.evidence.contains("Python standard library"));
    for expression in [
        "maybe.execute",
        "either.execute",
        "custom.get",
        "shadowed.append",
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
fn straightline_local_factory_returns_have_bounded_inferred_proof() {
    let source="class Client:\n    def execute(self): pass\ndef direct(): return Client()\ndef alias():\n    result=Client()\n    copy=result\n    return copy\ndef mutated():\n    result=Client()\n    result=unknown()\n    return result\ndef branch(condition):\n    if condition: return Client()\n    return Client()\ndef recursive(): return recursive()\ndef run():\n    direct().execute()\n    alias().execute()\n    mutated().execute()\n    branch(True).execute()\n    recursive().execute()\n";
    let facts = ast::extract("inferred_returns.py", "python", source).unwrap();
    for name in ["direct", "alias", "mutated", "recursive"] {
        let node = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let flow: ValueFlowFacts =
            serde_json::from_value(node.details["value_flow"].clone()).unwrap();
        assert!(flow.return_value.is_some());
        assert!(flow.return_position.is_some());
    }
    let branch = facts
        .nodes
        .iter()
        .find(|node| node.name == "branch")
        .unwrap();
    let flow: ValueFlowFacts =
        serde_json::from_value(branch.details["value_flow"].clone()).unwrap_or_default();
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
        let flow: ValueFlowFacts =
            serde_json::from_value(node.details["value_flow"].clone()).unwrap();
        assert!(matches!(flow.alias_type,Some(TypeExpr::Named{name}) if name=="Client"));
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
fn global_and_nonlocal_receivers_require_interprocedural_state_proof() {
    let source="class Client:\n    def execute(self): pass\nshared = Client()\ndef global_user():\n    global shared\n    shared.execute()\n    shared = Client()\n    shared.execute()\ndef outer():\n    local = Client()\n    def inner():\n        nonlocal local\n        local.execute()\n        local = Client()\n        local.execute()\n";
    let facts = ast::extract("scope.py", "python", source).unwrap();
    for name in ["global_user", "inner"] {
        let node = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let flow: ValueFlowFacts =
            serde_json::from_value(node.details["value_flow"].clone()).unwrap();
        assert_eq!(flow.bindings.len(), 2);
        assert!(flow
            .bindings
            .iter()
            .all(|binding| matches!(binding.value, ValueExpr::Unknown)));
    }
    let graph = linker::link(&BTreeMap::from([("scope.py".to_owned(), facts)]));
    let calls: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| {
            matches!(
                coverage.expression.as_str(),
                "shared.execute" | "local.execute"
            )
        })
        .collect();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|coverage| coverage.status == "unresolved"));
}

#[test]
fn builtin_type_members_are_finite_and_type_specific() {
    let profile = contextunity_forge_mcp::engine::languages::by_id("python").unwrap();
    for (receiver, member) in [
        ("str", "strip"),
        ("list", "append"),
        ("dict", "get"),
        ("set", "add"),
        ("tuple", "count"),
        ("int", "bit_length"),
        ("float", "hex"),
    ] {
        assert!(profile.builtin_member(receiver, member));
    }
    for (receiver, member) in [
        ("str", "append"),
        ("dict", "add"),
        ("Unknown", "strip"),
        ("list", "invented_method"),
    ] {
        assert!(!profile.builtin_member(receiver, member));
    }
    let source = "def use(text: str, items: list, mapping: dict):\n    text.strip()\n    items.append(1)\n    mapping.get('key')\n    text.append(1)\n    converted = str(unknown())\n    converted.strip()\n";
    let facts = ast::extract("builtins.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("builtins.py".to_owned(), facts)]));
    for expression in [
        "text.strip",
        "items.append",
        "mapping.get",
        "converted.strip",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("Python standard library"),
            "{expression}: {coverage:#?}"
        );
    }
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "text.append")
            .unwrap()
            .status,
        "unresolved"
    );
}

#[test]
fn binding_facts_preserve_same_line_order_and_every_invalidation() {
    let source="class Client:\n    def execute(self): pass\ndef run(condition):\n    before.execute(); before = Client(); before.execute()\n    alias = before\n    declared: Client = unknown_factory()\n    annotation_only: Client\n    if condition:\n        before = unknown_factory()\n    alias += unknown\n    first, second = unknown\n    del declared\n";
    let facts = ast::extract("flow.py", "python", source).unwrap();
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let flow: ValueFlowFacts = serde_json::from_value(run.details["value_flow"].clone()).unwrap();
    let before: Vec<_> = flow
        .bindings
        .iter()
        .filter(|binding| binding.name == "before")
        .collect();
    assert_eq!(before.len(), 2);
    assert!(!before[0].conditional);
    assert!(before[1].conditional);
    assert!(matches!(&before[0].value,ValueExpr::Call{callee} if callee=="Client"));
    assert!(before[0].position.column > source.lines().nth(3).unwrap().find("before =").unwrap());
    let declared: Vec<_> = flow
        .bindings
        .iter()
        .filter(|binding| binding.name == "declared")
        .collect();
    assert!(
        matches!(&declared[0].value,ValueExpr::Annotated{type_expr:TypeExpr::Named{name}} if name=="Client")
    );
    assert!(matches!(declared[1].value, ValueExpr::Unknown));
    assert!(flow
        .bindings
        .iter()
        .any(|binding| binding.name == "annotation_only"
            && matches!(binding.value, ValueExpr::Unknown)));
    for name in ["first", "second"] {
        assert!(flow
            .bindings
            .iter()
            .any(|binding| binding.name == name && matches!(binding.value, ValueExpr::Unknown)));
    }
}

#[test]
fn return_and_field_facts_exclude_uncertain_callable_semantics_and_nested_scopes() {
    let source="class Client:\n    def execute(self): pass\nclass Wrapper:\n    declared: Client\n    def __init__(self, client: Client):\n        self.client = client\n    def replace(self):\n        self.client = unknown()\ndef factory() -> Client: return Client()\nasync def asynchronous() -> Client: return Client()\ndef generator() -> Client:\n    yield Client()\n@decorate\ndef decorated() -> Client: return Client()\ndef outer():\n    def nested() -> Client: return Client()\n";
    let facts = ast::extract("fields.py", "python", source).unwrap();
    let flow = |name| {
        serde_json::from_value::<ValueFlowFacts>(
            facts
                .nodes
                .iter()
                .find(|node| node.name == name)
                .unwrap()
                .details["value_flow"]
                .clone(),
        )
        .unwrap_or_default()
    };
    assert!(matches!(flow("factory").return_type,Some(TypeExpr::Named{name}) if name=="Client"));
    for name in ["asynchronous", "generator", "decorated", "outer"] {
        assert!(flow(name).return_type.is_none(), "{name}");
    }
    assert!(
        matches!(&flow("Wrapper").fields[0].value,ValueExpr::Annotated{type_expr:TypeExpr::Named{name}} if name=="Client")
    );
    assert!(matches!(&flow("__init__").fields[0].value,ValueExpr::Alias{name} if name=="client"));
    assert!(matches!(
        flow("replace").fields[0].value,
        ValueExpr::Unknown
    ));
}

#[test]
fn typed_local_bindings_aliases_and_sync_returns_resolve_with_inferred_evidence() {
    let source="class Client:\n    def execute(self): pass\ndef make() -> Client: return Client()\ndef run():\n    local = Client(); local.execute()\n    alias = local\n    alias.execute()\n    declared: Client = unknown_factory()\n    declared.execute()\n    result = make()\n    result.execute()\n    return make().execute()\n";
    let facts = ast::extract("resolved_flow.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("resolved_flow.py".to_owned(), facts)]));
    for expression in [
        "local.execute",
        "alias.execute",
        "declared.execute",
        "result.execute",
        "make().execute",
    ] {
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
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.confidence == "inferred"),
            "{expression}"
        );
    }
}

#[test]
fn preceding_calls_conditional_rebindings_and_unknown_rhs_remain_unresolved() {
    let source="class Client:\n    def execute(self): pass\ndef run(condition):\n    local.execute(); local = Client()\n    local = unknown_factory()\n    local.execute()\n    branch = Client()\n    if condition: branch = Client()\n    branch.execute()\n    uninitialized: Client\n    uninitialized.execute()\n";
    let facts = ast::extract("unknown_flow.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("unknown_flow.py".to_owned(), facts)]));
    for coverage in graph.coverage.iter().filter(|coverage| {
        matches!(
            coverage.expression.as_str(),
            "local.execute" | "branch.execute" | "uninitialized.execute"
        )
    }) {
        assert_eq!(
            coverage.status, "unresolved",
            "{}:{}",
            coverage.expression, coverage.line
        );
    }
}

#[test]
fn guarded_python_bindings_resolve_only_inside_their_branch() {
    let source = "class Client:\n    def execute(self): pass\ndef run(value, condition):\n    if condition:\n        value = Client()\n        value.execute()\n    else:\n        value = unknown_factory()\n        value.execute()\n    value.execute()\n";
    let facts = ast::extract("guarded.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("guarded.py".to_owned(), facts)]));
    let calls: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| coverage.expression == "value.execute")
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls
            .iter()
            .map(|call| (call.line, call.status.as_str()))
            .collect::<Vec<_>>(),
        vec![(6, "resolved"), (9, "unresolved"), (10, "unresolved")]
    );
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
fn inferred_python_members_require_a_unique_class_provider() {
    let source = "flag = bool(input())\nother = bool(input())\nclass Stable:\n    def render(self): return 'stable'\nclass Dynamic:\n    if flag:\n        def render(self): return 'first'\n    else:\n        def render(self): return 'second'\n    if other:\n        def render(self): return 'third'\ndef use():\n    stable = Stable()\n    stable.render()\n    dynamic = Dynamic()\n    dynamic.render()\n    Dynamic.render(Dynamic())\n";
    let facts = ast::extract("providers.py", "python", source).unwrap();
    let stable = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with("Stable.render"))
        .unwrap()
        .id
        .clone();
    assert_eq!(
        facts
            .nodes
            .iter()
            .filter(|node| node.qualname.ends_with("Dynamic.render"))
            .count(),
        3
    );
    let graph = linker::link(&BTreeMap::from([("providers.py".to_owned(), facts)]));
    for (expression, expected) in [
        ("stable.render", "resolved"),
        ("dynamic.render", "ambiguous"),
        ("Dynamic.render", "ambiguous"),
    ] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            expected,
            "{expression}"
        );
    }
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.path == "providers.py"
        && edge.line == 14
        && edge.evidence == "stable.render"
        && edge.dst == stable));
}

#[test]
fn later_unconditional_python_method_overrides_conditional_providers() {
    let source = "flag = bool(input())\nclass Dynamic:\n    if flag:\n        def render(self): return 'first'\n    else:\n        def render(self): return 'second'\nclass Replaced:\n    if flag:\n        def render(self): return 'first'\n    else:\n        def render(self): return 'second'\n    def render(self): return 'final'\nclass FieldOverridden:\n    def render(self): return 'callable'\n    render = None\ndef use():\n    dynamic = Dynamic()\n    dynamic.render()\n    replaced = Replaced()\n    replaced.render()\n    field = FieldOverridden()\n    field.render()\n";
    let (_workspace, db) = build_workspace(&[("override.py", source)]);
    assert_eq!(
        persisted_coverage(&db, "override.py", "dynamic.render")[0].0,
        "ambiguous"
    );
    assert_eq!(
        persisted_coverage(&db, "override.py", "replaced.render")[0].0,
        "resolved"
    );
    assert_eq!(
        persisted_coverage(&db, "override.py", "field.render")[0].0,
        "unresolved"
    );
    let mut statement = db
        .prepare(
            "SELECT target.line FROM edge_occurrences AS occurrence \
         JOIN path_dictionary AS owner ON owner.path_id = occurrence.owner_id \
         JOIN coverage_evidence AS evidence ON evidence.evidence_id = occurrence.evidence_id \
         JOIN nodes AS target ON target.node_hash = occurrence.dst_hash \
         WHERE owner.path = ?1 AND occurrence.kind = 'calls' AND evidence.evidence = ?2 \
         AND (SELECT path FROM path_dictionary WHERE path_id=target.path_id) = ?1 AND target.qualname = ?3 ORDER BY target.line",
        )
        .unwrap();
    let targets: Vec<usize> = statement
        .query_map(
            ["override.py", "replaced.render", "override.Replaced.render"],
            |row| row.get(0),
        )
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(targets, [12], "{targets:?}");
}

#[test]
fn python_builtin_type_members_require_unshadowed_type_names() {
    let source = "def standard():\n    dict.fromkeys(('a',))\n    str.maketrans('a', 'b')\ndef shadow(dict, str):\n    dict.fromkeys(('a',))\n    str.maketrans('a', 'b')\n";
    let facts = ast::extract("builtin_type_members.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([(
        "builtin_type_members.py".to_owned(),
        facts,
    )]));
    for (line, expression, expected) in [
        (2, "dict.fromkeys", "external"),
        (3, "str.maketrans", "external"),
        (5, "dict.fromkeys", "unresolved"),
        (6, "str.maketrans", "unresolved"),
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.line == line && coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, expected, "{expression} at {line}");
        if expected == "external" {
            assert!(
                coverage
                    .evidence
                    .contains("Python standard library built-in"),
                "{}",
                coverage.evidence
            );
        }
    }
}

#[test]
fn python_namespace_builtins_require_unshadowed_names() {
    let source = "def standard():\n    globals()\n    locals()\n    input()\ndef shadow(globals, locals, input):\n    globals()\n    locals()\n    input()\n";
    let facts = ast::extract("namespace_builtins.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([(
        "namespace_builtins.py".to_owned(),
        facts,
    )]));
    for (line, expression, expected) in [
        (2, "globals", "external"),
        (3, "locals", "external"),
        (4, "input", "external"),
        (6, "globals", "unresolved"),
        (7, "locals", "unresolved"),
        (8, "input", "unresolved"),
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.line == line && coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, expected, "{expression} at {line}");
        if expected == "external" {
            assert!(
                coverage
                    .evidence
                    .contains("Python standard library built-in"),
                "{}",
                coverage.evidence
            );
        }
    }
}

#[test]
fn annotated_python_receivers_follow_declared_inheritance_and_protocol_members() {
    let source = "from django.http import HttpRequest\nfrom typing import Protocol\nclass Request(HttpRequest): pass\nclass Authenticated(Request): pass\nclass Connection(Protocol):\n    def execute(self, sql): ...\ndef use(request: Authenticated, conn: Connection):\n    request.GET.get('key')\n    conn.execute('select 1')\n    request.GET.missing('key')\n";
    let facts = ast::extract("typed_members.py", "python", source).unwrap();
    let execute_node = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with("Connection.execute"))
        .unwrap();
    let execute = execute_node.id.clone();
    let execute_qualname = execute_node.qualname.clone();
    let graph = linker::link(&BTreeMap::from([("typed_members.py".to_owned(), facts)]));
    for (line, expression, expected) in [
        (8, "request.GET.get", "external"),
        (9, "conn.execute", "resolved"),
        (10, "request.GET.missing", "unresolved"),
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.line == line && coverage.expression == expression)
            .unwrap();
        assert_eq!(
            coverage.status, expected,
            "{expression} at {line}: {}",
            coverage.evidence
        );
        if expression == "request.GET.get" {
            assert!(
                coverage.evidence.contains("django.http.HttpRequest"),
                "{}",
                coverage.evidence
            );
        }
    }
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.path == "typed_members.py"
        && edge.line == 9
        && edge.evidence == "conn.execute"
        && edge.dst == execute));
    let (_workspace, db) = build_workspace(&[("typed_members.py", source)]);
    for (expression, expected) in [
        ("request.GET.get", "external"),
        ("conn.execute", "resolved"),
        ("request.GET.missing", "unresolved"),
    ] {
        let coverage = persisted_coverage(&db, "typed_members.py", expression);
        assert_eq!(coverage.len(), 1, "{expression}: {coverage:?}");
        assert_eq!(coverage[0].0, expected, "{expression}: {coverage:?}");
        if expression == "request.GET.get" {
            assert!(
                coverage[0].1.contains("django.http.HttpRequest"),
                "{coverage:?}"
            );
        }
    }
    assert!(persisted_edge(
        &db,
        "typed_members.py",
        "calls",
        "conn.execute",
        "typed_members.py",
        &execute_qualname
    ));
}

#[test]
fn psycopg_alias_members_require_declared_dependency_and_finite_api() {
    let source = "from psycopg import AsyncConnection\ntype PgConnection = AsyncConnection[object]\nasync def run(conn: PgConnection):\n    await conn.execute('SELECT 1')\n    conn.cursor()\n    conn.unverified()\n";
    let manifest = "[project]\nname = 'typed-connection'\nversion = '0.1.0'\ndependencies = ['psycopg[binary]>=3.1.0']\n";
    for declared in [true, false] {
        let mut files = vec![("src/storage.py", source)];
        if declared {
            files.push(("pyproject.toml", manifest));
        }
        let (_workspace, db) = build_workspace(&files);
        for expression in ["conn.execute", "conn.cursor"] {
            let coverage = persisted_coverage(&db, "src/storage.py", expression);
            assert_eq!(coverage.len(), 1, "{declared} {expression}: {coverage:?}");
            assert_eq!(
                coverage[0].0,
                if declared { "external" } else { "unresolved" },
                "{declared} {expression}: {coverage:?}"
            );
            if declared {
                assert!(
                    coverage[0].1.contains("finite Python framework member"),
                    "{expression}: {coverage:?}"
                );
                assert!(
                    coverage[0].1.contains("psycopg.AsyncConnection"),
                    "{expression}: {coverage:?}"
                );
                assert!(
                    coverage[0].1.contains("line 1"),
                    "{expression}: {coverage:?}"
                );
            }
        }
        let unsupported = persisted_coverage(&db, "src/storage.py", "conn.unverified");
        assert_eq!(unsupported.len(), 1, "{declared}: {unsupported:?}");
        assert_eq!(
            unsupported[0].0, "unresolved",
            "{declared}: {unsupported:?}"
        );
    }
}

#[test]
fn explicit_and_unique_init_fields_resolve_but_mutated_fields_do_not() {
    let source="class Client:\n    def execute(self): pass\nclass Wrapper:\n    declared: Client\n    def __init__(self, client: Client):\n        self.client = client\nclass Mutated:\n    def __init__(self, client: Client): self.client = client\n    def replace(self): self.client = unknown_factory()\ndef use(wrapper: Wrapper, mutated: Mutated):\n    wrapper.client.execute()\n    wrapper.declared.execute()\n    mutated.client.execute()\n";
    let facts = ast::extract("field_flow.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("field_flow.py".to_owned(), facts)]));
    for expression in ["wrapper.client.execute", "wrapper.declared.execute"] {
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
            .find(|coverage| coverage.expression == "mutated.client.execute")
            .unwrap()
            .status,
        "unresolved"
    );
}

#[test]
fn async_generator_and_decorated_factory_calls_do_not_gain_return_type_proof() {
    let source="class Client:\n    def execute(self): pass\nasync def asynchronous() -> Client: return Client()\ndef generator() -> Client:\n    yield Client()\n@decorate\ndef decorated() -> Client: return Client()\ndef run():\n    asynchronous().execute()\n    generator().execute()\n    decorated().execute()\n";
    let facts = ast::extract("uncertain_returns.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([(
        "uncertain_returns.py".to_owned(),
        facts,
    )]));
    for expression in [
        "asynchronous().execute",
        "generator().execute",
        "decorated().execute",
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
fn method_return_annotation_proves_a_chain_only_for_a_known_receiver() {
    let source="class Client:\n    def execute(self): pass\nclass Response:\n    def request(self) -> Client: return Client()\ndef known(response: Response): return response.request().execute()\ndef unknown(response): return response.request().execute()\n";
    let facts = ast::extract("return_chain.py", "python", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("return_chain.py".to_owned(), facts)]));
    let calls: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| coverage.expression == "response.request().execute")
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].status, "resolved");
    assert_eq!(calls[1].status, "unresolved");
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
