use super::*;

#[test]
fn return_and_field_facts_exclude_uncertain_callable_semantics_and_nested_scopes() {
    let source="class Client:\n    def execute(self): pass\nclass Wrapper:\n    declared: Client\n    def __init__(self, client: Client):\n        self.client = client\n    def replace(self):\n        self.client = unknown()\ndef factory() -> Client: return Client()\nasync def asynchronous() -> Client: return Client()\ndef generator() -> Client:\n    yield Client()\n@decorate\ndef decorated() -> Client: return Client()\ndef outer():\n    def nested() -> Client: return Client()\n";
    let facts = ast::extract("fields.py", "python", source).unwrap();
    let flow = |name| {
        let node = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, node);
        serde_json::from_value::<ValueFlowFacts>(details["value_flow"].clone()).unwrap_or_default()
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
