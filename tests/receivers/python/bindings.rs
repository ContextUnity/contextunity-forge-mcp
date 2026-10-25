use super::*;

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
fn global_and_nonlocal_receivers_require_interprocedural_state_proof() {
    let source="class Client:\n    def execute(self): pass\nshared = Client()\ndef global_user():\n    global shared\n    shared.execute()\n    shared = Client()\n    shared.execute()\ndef outer():\n    local = Client()\n    def inner():\n        nonlocal local\n        local.execute()\n        local = Client()\n        local.execute()\n";
    let facts = ast::extract("scope.py", "python", source).unwrap();
    for name in ["global_user", "inner"] {
        let node = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, node);
        let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
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
fn binding_facts_preserve_same_line_order_and_every_invalidation() {
    let source="class Client:\n    def execute(self): pass\ndef run(condition):\n    before.execute(); before = Client(); before.execute()\n    alias = before\n    declared: Client = unknown_factory()\n    annotation_only: Client\n    if condition:\n        before = unknown_factory()\n    alias += unknown\n    first, second = unknown\n    del declared\n";
    let facts = ast::extract("flow.py", "python", source).unwrap();
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, run);
    let flow: ValueFlowFacts = serde_json::from_value(details["value_flow"].clone()).unwrap();
    let before: Vec<_> = flow
        .bindings
        .iter()
        .filter(|binding| binding.name == "before")
        .collect();
    assert_eq!(before.len(), 2);
    assert!(!before[0].conditional);
    assert!(before[1].conditional);
    assert!(matches!(&before[0].value,ValueExpr::Call{callee,..} if callee=="Client"));
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
