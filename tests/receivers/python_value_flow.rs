#![cfg(feature = "lang-python")]
use contextunity_forge_mcp::{
    core::semantic::{TypeExpr, ValueExpr, ValueFlowFacts},
    engine::{ast, linker},
};
use std::collections::BTreeMap;

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
    assert_eq!(keys, ["raw", "dict", "make", "Store", "unknown"]);
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
        "custom.custom",
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
    }
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
    for (expression, line) in [("a.info", 1), ("b.warning", 2), ("a.error", 1)] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "external", "{expression}");
        assert!(
            coverage.evidence.contains("logging.Logger"),
            "{}",
            coverage.evidence
        );
        assert!(
            coverage.evidence.contains(&format!("line {line}")),
            "{}",
            coverage.evidence
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
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "data.get")
            .unwrap()
            .status,
        "resolved"
    );
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
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            "resolved"
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
