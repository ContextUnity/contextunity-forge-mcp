use super::*;

#[test]
fn rust_local_constructor_binding_resolves_receiver_methods() {
    let path = "src/workspace.rs";
    let source = "pub struct Workspace {}\nimpl Workspace {\n    pub fn new() -> Self { Self {} }\n    pub fn open(&self) {}\n    pub fn create(&self) {}\n}\nfn run() {\n    let workspace = Workspace::new();\n    workspace.open();\n    workspace.create();\n}\n";
    let facts = ast::extract(path, "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let files = BTreeMap::from([(path.to_owned(), facts)]);
    let graph = linker::link(&files);
    let workspace = files[path]
        .nodes
        .iter()
        .find(|node| node.kind == "struct" && node.name == "Workspace")
        .unwrap();
    let constructor = files[path]
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "new")
        .unwrap();
    let self_references: Vec<_> = files[path]
        .references
        .iter()
        .filter(|reference| reference.kind == "references" && reference.expression == "Self")
        .collect();
    assert!(!self_references.is_empty(), "{:#?}", files[path]);
    for reference in self_references {
        assert!(
            graph.coverage.iter().any(|row| row.path == path
                && row.line == reference.line
                && row.expression == "Self"
                && row.status == "resolved"),
            "{graph:#?}"
        );
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "references"
                && edge.path == path
                && edge.line == reference.line
                && edge.evidence == "Self"
                && edge.src == constructor.id
                && edge.dst == workspace.id),
            "{graph:#?}"
        );
    }
    for (expression, method) in [
        ("Workspace::new", "new"),
        ("workspace.open", "open"),
        ("workspace.create", "create"),
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|coverage| coverage.expression == expression && coverage.status == "resolved"),
            "{expression}: {graph:#?}"
        );
        let target = files[path]
            .nodes
            .iter()
            .find(|node| node.kind == "method" && node.name == method)
            .unwrap();
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id
                && (expression == "Workspace::new" || edge.confidence == "inferred")),
            "{expression}: {graph:#?}"
        );
    }
}

#[test]
fn rust_generic_impl_methods_follow_the_declared_nominal_receiver() {
    let path = "src/store.rs";
    let source = "struct Store<T> { value: T }\nimpl<T> Store<T> { fn new(value: T) -> Self { Self { value } } fn read(&self) -> &T { &self.value } fn requires_clone(&self) where T: Clone {} fn constrained<U: Clone>(&self, _value: U) {} }\nimpl<T> Store<Vec<T>> { fn specialized(&self) {} }\ntrait Reading { fn default_constrained<U: Clone>(&self, _value: U) {} }\nimpl<T> Reading for Store<T> {}\nstruct Limited<T>(T);\nimpl<T: Clone> Limited<T> { fn checked(&self) {} }\nstruct Borrowed<'a, T> { slice: &'a T }\nimpl<'a, T> Borrowed<'a, T> { fn inspect(&self) -> &'a T { self.slice } }\nfn run<'a>(store: &Store<u8>, limited: &Limited<u8>, borrowed: &Borrowed<'a, u8>) { store.read(); Store::new(1u8).read(); borrowed.inspect(); store.specialized(); store.requires_clone(); store.constrained(1u8); store.default_constrained(1u8); limited.checked(); }\nstruct Other;\nfn unknown(other: &Other) { other.read(); }";
    let facts = ast::extract(path, "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let method = facts
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "read")
        .unwrap();
    let method_id = method.id.clone();
    let inspect_id = facts
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "inspect")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
    for (expression, expected_dst) in [
        ("store.read", &method_id),
        ("Store::new(1u8).read", &method_id),
        ("borrowed.inspect", &inspect_id),
    ] {
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && &edge.dst == expected_dst),
            "{expression}: {graph:#?}"
        );
    }
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.expression == "other.read" && row.status == "unresolved"),
        "{graph:#?}"
    );
    for expression in [
        "store.specialized",
        "store.requires_clone",
        "store.constrained",
        "store.default_constrained",
        "limited.checked",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "unresolved"),
            "{expression}: {graph:#?}"
        );
    }
}

#[test]
fn rust_structural_method_initializers_use_nominal_return_contracts() {
    let source = "struct Service;\nimpl Service { fn rebuild<T>(&self) -> Self { Self {} } fn work(&self) {} }\nfn run(client: &Service) { let made = client.rebuild::<u8>(); made.work(); }";
    let facts = ast::extract("src/service.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, run);
    let made = details["value_flow"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["name"] == "made")
        .unwrap();
    assert_eq!(made["value"]["kind"], "call");
    assert_eq!(made["value"]["callee"], "client.rebuild");
    let facts = BTreeMap::from([("src/service.rs".to_owned(), facts)]);
    let graph = linker::link(&facts);
    for (expression, target) in [("client.rebuild", "rebuild"), ("made.work", "work")] {
        let target = facts["src/service.rs"]
            .nodes
            .iter()
            .find(|node| node.name == target)
            .unwrap();
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == target.id),
            "{facts:#?}\n{graph:#?}"
        );
    }
}

#[test]
fn rust_associated_generic_factory_uses_declared_return_for_member_chains() {
    let source = "struct Factory; struct Service;\nimpl Factory { fn build<T>() -> Service { Service } }\nimpl Service { fn new() -> Self { Self } fn work(&self) {} fn self_calls(&self) { Self::new().work(); Self::default(); Self::missing(); } }\nfn run() { Factory::build::<u8>().work(); let built = Factory::build::<u16>(); built.work(); Service::new().work(); }\nfn missing() { Factory::missing().work(); }\nstruct Ambiguous; trait FirstFactory { fn create() -> Ambiguous; } trait SecondFactory { fn create() -> Ambiguous; }\nimpl FirstFactory for Ambiguous { fn create() -> Ambiguous { Ambiguous } }\nimpl SecondFactory for Ambiguous { fn create() -> Ambiguous { Ambiguous } }\nimpl Ambiguous { fn self_call() { Self::create(); } }";
    let facts = ast::extract("src/service.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let facts = BTreeMap::from([("src/service.rs".to_owned(), facts)]);
    let graph = linker::link(&facts);
    let work = facts["src/service.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "work" && node.kind == "method")
        .unwrap();
    let new = facts["src/service.rs"]
        .nodes
        .iter()
        .find(|node| node.name == "new" && node.kind == "method")
        .unwrap();
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == work.id && edge.line == 4)
            .count(),
        3,
        "{graph:#?}"
    );
    assert!(!graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.dst == work.id && edge.line == 5));
    assert!(graph.coverage.iter().any(|coverage| coverage.line == 5
        && coverage.expression.contains("Factory::missing")
        && coverage.status == "unresolved"));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == "Self::new" && edge.dst == new.id));
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.dst == work.id && edge.line == 3),
        "{graph:#?}"
    );
    for expression in ["Self::default", "Self::missing"] {
        assert!(
            graph.coverage.iter().any(
                |coverage| coverage.expression == expression && coverage.status == "unresolved"
            ),
            "{expression}: {graph:#?}"
        );
        assert!(
            !graph
                .edges
                .iter()
                .any(|edge| edge.kind == "calls" && edge.evidence == expression),
            "{expression}: {graph:#?}"
        );
    }
    assert!(graph.coverage.iter().any(|coverage| coverage.expression == "Self::create"
        && coverage.status == "ambiguous"), "{graph:#?}");
    assert!(
        !graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == "Self::create"),
        "{graph:#?}"
    );
}

#[test]
fn rust_declared_fields_resolve_nominal_parameters_and_tuple_receivers() {
    let source = "struct Service;\nimpl Service { fn work(&self) {} }\nstruct Named { id: String, nodes: Vec<Service>, service: Service }\nstruct Tuple(#[doc = \"service\"] pub Service, pub(crate) String, Vec<Service>);\nfn named(node: &Named) { node.id.as_str(); node.nodes.iter(); node.service.work(); }\nfn tuple(holder: &Tuple) { holder.0.work(); holder.1.as_str(); holder.2.iter(); }\nimpl Tuple { fn run(&self) { self.0.work(); } }";
    let facts = ast::extract("src/fields.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    let work = facts.nodes.iter().find(|node| node.name == "work").unwrap();
    let work_id = work.id.clone();
    let tuple = facts
        .nodes
        .iter()
        .find(|node| node.name == "Tuple" && node.kind == "struct")
        .unwrap();
    let details = crate::common::assertions::projected_node_details(&facts, tuple);
    let fields = details["value_flow"]["fields"].as_array().unwrap();
    assert_eq!(
        fields
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["0", "1", "2"]
    );
    assert_eq!(fields[0]["value"]["type_expr"]["name"], "Service");
    assert_eq!(fields[1]["value"]["type_expr"]["name"], "String");
    assert_eq!(fields[2]["value"]["type_expr"]["base"], "Vec");
    assert_eq!(
        fields[2]["value"]["type_expr"]["args"][0]["name"],
        "Service"
    );
    let graph = linker::link(&BTreeMap::from([("src/fields.rs".to_owned(), facts)]));
    for expression in ["node.service.work", "holder.0.work", "self.0.work"] {
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == expression
                && edge.dst == work_id),
            "{graph:#?}"
        );
    }
    for expression in [
        "node.id.as_str",
        "node.nodes.iter",
        "holder.1.as_str",
        "holder.2.iter",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "external"),
            "{graph:#?}"
        );
    }
}

#[test]
fn rust_generic_field_roots_preserve_declared_unknown_boundaries() {
    let source = "struct Service;\ntrait Bound { type Item; }\nimpl Bound for Service { type Item = Service; }\ntype T = Service;\nstruct Named<T: Bound = Service> { direct: T, associated: T::Item, fixed: Service }\nstruct Tuple<T: Bound = Service>(T, T::Item, Service);";
    let facts = ast::extract("src/generic_fields.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");
    for name in ["Named", "Tuple"] {
        let owner = facts
            .nodes
            .iter()
            .find(|node| node.name == name && node.kind == "struct")
            .unwrap();
        let details = crate::common::assertions::projected_node_details(&facts, owner);
        let fields = details["value_flow"]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3, "{facts:#?}");
        for field in &fields[..2] {
            assert_eq!(field["value"]["type_expr"]["kind"], "unknown", "{facts:#?}");
        }
        assert_eq!(fields[2]["value"]["type_expr"]["name"], "Service");
    }
}
