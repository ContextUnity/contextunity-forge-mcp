use super::*;

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
