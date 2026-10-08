use super::super::support::Workspace;
use super::*;

#[test]
fn local_constructors_and_aliases_resolve_real_methods() {
    assert!(resolves(
        "class Worker { work() {} } function run() { const client = new Worker(); client.work(); }",
        "client.work",
    ));
    assert!(resolves(
        "class Worker { work() {} } function run() { const client: Worker = new Worker(); const alias = client; alias.work(); }",
        "alias.work",
    ));
}

#[test]
fn object_methods_require_actual_declarations_and_stable_bindings() {
    assert!(resolves(
        "function run() { const client = {work() {}}; client.work(); }",
        "client.work"
    ));
}

#[test]
fn inherited_methods_select_actual_overrides_and_reject_parent_cycles() {
    assert!(resolves("class Base { work() {} } class Worker extends Base {} function run() { const client = new Worker(); client.work(); }", "client.work"));
    assert!(resolves("class Base { work() {} } class Worker extends Base { work() {} } function run() { const client = new Worker(); client.work(); }", "client.work"));
    let facts = ast::extract("service.ts", "typescript", "class Base { work() {} } class Worker extends Base { work() {} } function run() { const client = new Worker(); client.work(); }").unwrap();
    let expected = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with(".Worker.work"))
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("service.ts".to_owned(), facts)]));
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.evidence == "client.work"
        && edge.dst == expected));
    assert!(!resolves("class A extends B {} class B extends A { work() {} } function run() { const client = new A(); client.work(); }", "client.work"));
}

#[test]
fn value_flow_admits_initializers_only_after_complete_declarations() {
    for source in [
        "class Worker { work() {} } function run() { let client: Worker; client.work(); }",
        "class Worker { work() {} } function run() { const client: Worker = client.work(); }",
    ] {
        assert!(!resolves(source, "client.work"), "{source}");
    }
    assert!(resolves(
        "class Worker { work() {} } function run() { const client = new Worker(); client.work(); }",
        "client.work"
    ));
}

#[test]
fn finite_javascript_static_globals_respect_local_shadowing() {
    let source = "function run() { Object.hasOwn({}, 'x'); Promise.all([]); Date.now(); CSS.escape('x'); encodeURIComponent('x'); }";
    let facts = ast::extract("globals.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("globals.js".to_owned(), facts)]));
    for expression in [
        "Object.hasOwn",
        "Promise.all",
        "Date.now",
        "CSS.escape",
        "encodeURIComponent",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "external"),
            "{expression}: {graph:#?}"
        );
    }
    let shadow = "function run(Object, Promise, Date, CSS, encodeURIComponent) { Object.hasOwn({}, 'x'); Promise.all([]); Date.now(); CSS.escape('x'); encodeURIComponent('x'); }";
    let facts = ast::extract("shadow.js", "javascript", shadow).unwrap();
    let graph = linker::link(&BTreeMap::from([("shadow.js".to_owned(), facts)]));
    for expression in [
        "Object.hasOwn",
        "Promise.all",
        "Date.now",
        "CSS.escape",
        "encodeURIComponent",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "unresolved"),
            "shadowed {expression}: {graph:#?}"
        );
    }
}

#[test]
fn javascript_doc_types_supply_explicit_parameter_and_return_contracts() {
    for (source, expression) in [
        ("class Worker { work() {} }\n/** @param {Worker} client */\nfunction run(client) { client.work(); }", "client.work"),
        ("class Worker { work() {} }\n/** @returns {Worker} */\nfunction make() { return new Worker(); }\nfunction run() { const client = make(); client.work(); }", "client.work"),
    ] {
        let facts = ast::extract("service.js", "javascript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("service.js".to_owned(), facts)]));
        assert!(graph.edges.iter().any(|edge| edge.kind == "calls" && edge.evidence == expression), "{source}");
    }
}

#[test]
fn computed_receivers_use_constructors_and_explicit_factory_returns() {
    assert!(resolves(
        "class Worker { work() {} } function run() { (new Worker()).work(); }",
        "(new Worker()).work"
    ));
    assert!(resolves("class Worker { work() {} } function make(): Worker { return new Worker(); } function run() { make().work(); }", "make().work"));
    assert!(!resolves(
        "class Worker { work() {} } function run() { Worker().work(); }",
        "Worker().work"
    ));
}

#[test]
fn declaration_docs_preserve_annotation_precedence_and_unknown_returns() {
    for (path, language, declaration, expected) in [
        (
            "service.js",
            "javascript",
            "/**\n * @param {Worker} client\n * @returns {Worker}\n */\nfunction make(client) { client.work(); return client; }",
            "resolved",
        ),
        (
            "service.ts",
            "typescript",
            "/**\n * @param {Other} client\n * @returns {Other}\n */\nfunction make(client: Worker): Worker { client.work(); return client; }",
            "resolved",
        ),
        (
            "service.js",
            "javascript",
            "/**\n * @param {Worker} client\n * @returns {Worker}\n * @returns {Other}\n */\nfunction make(client) { client.work(); return client; }",
            "unresolved",
        ),
        (
            "service.js",
            "javascript",
            "/**\n * @param {Worker} client\n * @returns {Worker}\n */\nasync function make(client) { client.work(); return client; }",
            "unresolved",
        ),
    ] {
        let source = format!("class Worker {{ work() {{}} }} class Other {{}}\n{declaration}\nconst output = make(new Worker()); output.work();");
        let facts = ast::extract(path, language, &source).unwrap();
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
        for (expression, status) in [("client.work", "resolved"), ("output.work", expected)] {
            let rows: Vec<_> = graph.coverage.iter().filter(|row| row.expression == expression).collect();
            assert!(!rows.is_empty(), "{source}: {expression}");
            assert!(rows.iter().all(|row| row.status == status), "{source}: {rows:#?}");
        }
    }
}

#[test]
fn straight_line_factory_returns_prove_chains_and_reject_cycles_or_branches() {
    assert!(resolves("class Worker { work() {} } function make() { return new Worker(); } function wrap() { const result = make(); return result; } function run() { const client = wrap(); client.work(); }", "client.work"));
    assert!(resolves("class Worker { work() {} } const make = () => new Worker(); function run() { const client = make(); client.work(); }", "client.work"));
    for source in [
        "class Worker { work() {} } function make() { return wrap(); } function wrap() { return make(); } function run() { const client = make(); client.work(); }",
        "class Worker { work() {} } function make(flag) { if (flag) return new Worker(); return unknown(); } function run() { const client = make(true); client.work(); }",
        "class Worker { work() {} } function make(flag) { if (flag) return new Worker(); } function run() { const client = make(true); client.work(); }",
    ] { assert!(!resolves(source, "client.work"), "{source}"); }
}

#[test]
fn javascript_stable_lexical_receiver_reaches_nested_callback() {
    let cases = [
        (
            "stable.js",
            "javascript",
            "class Service { run() {} } function outer() { const receiver = new Service(); function callback() { receiver.run(); } callback(); }",
            "resolved",
        ),
        (
            "rebound.js",
            "javascript",
            "class Service { run() {} } function outer() { let receiver = new Service(); function callback() { receiver.run(); } receiver = unknown(); callback(); }",
            "unresolved",
        ),
        (
            "shadowed.js",
            "javascript",
            "class Service { run() {} } function outer() { const receiver = new Service(); function callback(receiver) { receiver.run(); } callback({}); }",
            "unresolved",
        ),
        (
            "classic.html",
            "html",
            "<script>class Service { run() {} } const receiver = new Service();</script>\n<script>function callback() { receiver.run(); } callback();</script>",
            "resolved",
        ),
    ];
    for (path, language, source, status) in cases {
        let facts = ast::extract(path, language, source).unwrap();
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts.clone())]));
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "receiver.run" && row.status == status),
            "{path}: expected {status} from lexical receiver provenance, got {:#?}",
            graph.coverage
        );
        let targets: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.evidence == "receiver.run")
            .filter_map(|edge| facts.nodes.iter().find(|node| node.id == edge.dst))
            .collect();
        if status == "resolved" {
            assert_eq!(
                targets.len(),
                1,
                "{path}: callback must call one local method"
            );
            assert_eq!(targets[0].name, "run");
            assert_eq!(targets[0].path, path);
        } else {
            assert!(
                targets.is_empty(),
                "{path}: unstable or shadowed receiver cannot call a local method"
            );
        }
    }
}

#[test]
fn javascript_prototype_assignment_and_arrow_this_use_exact_class_provider() {
    let source = r#"class Service { finish() {} }
function run() {}
Service.prototype.run = function() {
    const callback = () => this.finish();
    callback();
};
function use() { const service = new Service(); service.run(); new Service().run(); run(); }
"#;
    let facts = ast::extract("service.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("service.js".to_owned(), facts.clone())]));
    for expression in ["service.run", "new Service().run", "this.finish"] {
        let targets: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.evidence == expression)
            .filter_map(|edge| facts.nodes.iter().find(|node| node.id == edge.dst))
            .collect();
        assert_eq!(targets.len(), 1, "{expression}: {graph:#?}");
        assert_eq!(targets[0].kind, "method", "{expression}: {targets:#?}");
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| { row.expression == expression && row.status == "resolved" }),
            "{expression}: {graph:#?}"
        );
    }
    assert!(
        graph.edges.iter().any(|edge| {
            edge.kind == "calls"
                && edge.evidence == "run"
                && facts.nodes.iter().any(|node| {
                    node.id == edge.dst && node.kind == "function" && node.name == "run"
                })
        }),
        "ordinary run() must keep its lexical function provider: {graph:#?}"
    );

    let unknown = "class Service { finish() {} }\nfunction Other() {}\nOther.prototype.run = function() {}\nfunction use() { const service = new Service(); service.run(); }";
    let facts = ast::extract("unknown.js", "javascript", unknown).unwrap();
    let graph = linker::link(&BTreeMap::from([("unknown.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "service.run" && row.status == "unresolved" }),
        "{graph:#?}"
    );

    for source in [
        "Service.prototype.run = function() {}; class Service {} const service = new Service(); service.run();",
        "class Service {} const Alias = Service; Alias.prototype.run = function() {}; const service = new Service(); service.run();",
    ] {
        let facts = ast::extract("guarded.js", "javascript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("guarded.js".to_owned(), facts)]));
        assert!(graph.coverage.iter().any(|row| {
            row.expression == "service.run" && row.status == "unresolved"
        }), "prototype provider must be lexically visible and exact: {source}: {graph:#?}");
    }

    let legacy = r#"function Legacy() {}
Legacy.prototype.finish = function() {};
Legacy.prototype.run = function() { const callback = () => this.finish(); callback(); };
function use() { const receiver = new Legacy(); receiver.run(); }
"#;
    let facts = ast::extract("legacy.js", "javascript", legacy).unwrap();
    let graph = linker::link(&BTreeMap::from([("legacy.js".to_owned(), facts.clone())]));
    for expression in ["receiver.run", "this.finish"] {
        assert!(
            graph.edges.iter().any(|edge| {
                edge.kind == "calls"
                    && edge.evidence == expression
                    && facts
                        .nodes
                        .iter()
                        .any(|node| node.id == edge.dst && node.kind == "method")
            }),
            "{expression}: {graph:#?}"
        );
    }

    let rebound = "class Service {}\nService.prototype.run = function() {};\nService.prototype.run = null;\nfunction use() { new Service().run(); }";
    let facts = ast::extract("rebound.js", "javascript", rebound).unwrap();
    let graph = linker::link(&BTreeMap::from([("rebound.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "new Service().run" && row.status == "unresolved" }),
        "{graph:#?}"
    );

    let deleted = "class Service {}\nService.prototype.run = function() {};\ndelete Service.prototype.run;\nfunction use() { new Service().run(); }";
    let facts = ast::extract("deleted.js", "javascript", deleted).unwrap();
    let graph = linker::link(&BTreeMap::from([("deleted.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "new Service().run" && row.status == "unresolved" }),
        "deleted prototype member must lose its provider: {graph:#?}"
    );

    let replaced = "function Service() {}\nService.prototype.run = function() {};\nService.prototype = {};\nfunction use() { new Service().run(); }";
    let facts = ast::extract("replaced.js", "javascript", replaced).unwrap();
    let graph = linker::link(&BTreeMap::from([("replaced.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "new Service().run" && row.status == "unresolved" }),
        "replaced prototype must lose its prior method provider: {graph:#?}"
    );
}

#[test]
fn javascript_prototype_identifier_rhs_requires_a_unique_stable_function() {
    let source = "function Service() {} function implementation(value) { return value; } Service.prototype.run = implementation; new Service().run(1);";
    let facts = ast::extract("named.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("named.js".to_owned(), facts.clone())]));
    let implementation = facts
        .nodes
        .iter()
        .find(|node| node.kind == "function" && node.name == "implementation")
        .unwrap();
    assert!(
        graph.edges.iter().any(|edge| {
            edge.kind == "calls"
                && edge.evidence == "new Service().run"
                && edge.dst == implementation.id
        }),
        "prototype assignment must reach its exact lexical function: {graph:#?}"
    );

    for source in [
        "function Service() {} function implementation() {} function implementation() {} Service.prototype.run = implementation; new Service().run();",
        "function Service() {} function implementation() {} implementation = other; Service.prototype.run = implementation; new Service().run();",
        "function Service() {} Service.prototype.run = implementation; const implementation = function() {}; new Service().run();",
    ] {
        let facts = ast::extract("guarded.js", "javascript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("guarded.js".to_owned(), facts)]));
        assert!(graph.coverage.iter().any(|row| {
            row.expression == "new Service().run"
                && matches!(row.status.as_str(), "unresolved" | "ambiguous")
        }), "unproven implementation binding must fail closed: {source}: {graph:#?}");
    }
}

#[test]
fn javascript_prototype_arrow_rhs_is_callable_without_inheriting_receiver_this() {
    let source = "class Service { finish() {} } Service.prototype.run = () => this.finish(); new Service().run();";
    let facts = ast::extract("arrow.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("arrow.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "new Service().run" && row.status == "resolved" }),
        "prototype arrow must remain callable: {graph:#?}"
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "this.finish" && row.status == "unresolved" }),
        "prototype arrow retains lexical this instead of Service receiver: {graph:#?}"
    );
}

#[test]
fn javascript_prototype_call_requires_method_assignment_before_use() {
    let source =
        "function Service() {} new Service().run(); Service.prototype.run = function() {};";
    let facts = ast::extract("early.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("early.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "new Service().run" && row.status == "unresolved" }),
        "use before prototype assignment must remain unresolved: {graph:#?}"
    );
}

#[test]
fn generic_containers_prove_only_documented_container_members() {
    let members = "push pop shift unshift slice splice indexOf includes join map filter forEach reduce find findIndex some every flat flatMap";
    let member_calls = members
        .split_whitespace()
        .map(|member| format!("items.{member}();"))
        .collect::<String>();
    for annotation in ["Array<Worker>", "Worker[]", "Array<Array<Worker>>"] {
        let source = format!("class Worker {{ work() {{}} }} function run() {{ const items: {annotation} = []; {member_calls} items.work(); }}");
        let facts = ast::extract("generic.ts", "typescript", &source).unwrap();
        let graph = linker::link(&BTreeMap::from([("generic.ts".into(), facts)]));
        for member in members.split_whitespace() {
            assert!(
                graph
                    .coverage
                    .iter()
                    .any(|row| row.expression == format!("items.{member}")
                        && row.status == "external"
                        && row.evidence.contains("JavaScript/Web platform")),
                "{annotation}: {member}"
            );
        }
        assert!(!graph.edges.iter().any(|edge| edge.evidence == "items.work"));
    }
    let source = "class Array<T> { custom() {} } function run() { const items: Array<string> = unknown(); items.map(); }";
    let facts = ast::extract("shadow.ts", "typescript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("shadow.ts".into(), facts)]));
    assert!(graph
        .coverage
        .iter()
        .any(|row| row.expression == "items.map" && row.status == "unresolved"));
    let source = "function Array<T>() { return unknown(); } function run() { const items: Array<string> = unknown(); items.map(); }";
    let facts = ast::extract("function-shadow.ts", "typescript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("function-shadow.ts".into(), facts)]));
    assert!(graph
        .coverage
        .iter()
        .any(|row| row.expression == "items.map" && row.status == "unresolved"));
}

#[test]
fn async_and_generator_factories_do_not_expose_wrapped_return_members() {
    assert!(resolves("class Worker { work() {} } function make() { return new Worker(); } function run() { const client = make(); client.work(); }", "client.work"));
    for factory in [
        "async function make() { return new Worker(); }",
        "async function make(): Worker { return new Worker(); }",
        "const make = async () => new Worker();",
        "function* make(): Worker { return new Worker(); }",
        "class Factory { static async make(): Worker { return new Worker(); } } const make = Factory.make;",
    ] {
        let source = format!("class Worker {{ work() {{}} }} {factory} function run() {{ const client = make(); client.work(); }}");
        assert!(!resolves(&source, "client.work"), "{factory}");
    }
}

#[test]
fn static_this_and_nominal_calls_preserve_static_instance_boundaries() {
    let source = "class Worker { instance() {} static associated() {} static runStatic() { this.instance(); this.associated(); } runInstance() { this.instance(); this.associated(); } }";
    let facts = ast::extract("static.ts", "typescript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("static.ts".into(), facts.clone())]));
    for edge in graph.edges.iter().filter(|edge| edge.kind == "calls") {
        let owner = facts.nodes.iter().find(|node| node.id == edge.src).unwrap();
        let target = facts.nodes.iter().find(|node| node.id == edge.dst).unwrap();
        assert_eq!(
            owner.details["is_static"], target.details["is_static"],
            "{} -> {}",
            owner.qualname, target.qualname
        );
    }
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls")
            .count(),
        2
    );
    assert!(!resolves("class Worker { work() {} make(): Worker { return new Worker(); } } function run() { const client = Worker.make(); client.work(); }", "client.work"));
    assert!(resolves("class Worker { work() {} static make(): Worker { return new Worker(); } } function run() { const client = Worker.make(); client.work(); }", "client.work"));
}

#[test]
fn returned_object_literal_this_members_resolve_exact_same_object() {
    let source = r#"function make() {
  return {
    count: 0,
    inc() { this.count += 1; this.reset(); this.unknown(); },
    reset() { this.count = 0; }
  };
}
const ordinary = { count: 0, inc() { this.count += 1; } };
const nav = make(); nav.reset(); nav.count; nav.count();
function duplicate() { return { count: 0, count: 1, inc() { this.count += 1; } }; }
function branch(flag) { if (flag) return { count: 0 }; return { count: 1, inc() { this.count += 1; } }; }
"#;
    let facts = ast::extract("objects.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("objects.js".to_owned(), facts.clone())]));
    for expression in ["this.count", "this.reset"] {
        assert!(
            graph.coverage.iter().any(|row| {
                row.line == 4 && row.expression == expression && row.status == "resolved"
            }),
            "{expression}: {graph:#?}"
        );
    }
    for line in [8, 10, 11] {
        assert!(
            graph.coverage.iter().any(|row| {
                row.line == line && row.expression == "this.count" && row.status == "unresolved"
            }),
            "line {line}: {graph:#?}"
        );
    }
    assert!(
        graph.edges.iter().any(|edge| {
            edge.line == 4
                && edge.kind == "mutates"
                && edge.evidence == "this.count"
                && facts.nodes.iter().any(|node| {
                    node.id == edge.dst
                        && node.kind == "field"
                        && node.name == "count"
                        && node.line == 3
                })
        }),
        "missing exact returned-object field edge: {graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|row| {
            row.line == 9 && row.expression == "nav.reset" && row.status == "resolved"
        }),
        "returned-object method is unresolved: {graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|row| {
            row.line == 9 && row.expression == "nav.count" && row.status == "unresolved"
        }),
        "field must not become callable: {graph:#?}"
    );
    use contextunity_forge_mcp::db::{reader, writer};
    let workspace = Workspace::new();
    std::fs::write(workspace.join("objects.js"), source).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let persisted: i64 = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='objects.js' AND line=4 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='this.count' AND status='resolved'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(persisted, 1);
    let exact_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='objects.js' AND e.line=4 AND e.kind='mutates' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='objects.js' AND dst.name='count' AND dst.kind='field' AND dst.line=3",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(exact_edge, 1);
    drop(conn);
    let source = "function make() { return { items: [], visible() { return this.items.filter(item => item); } }; }";
    let facts = ast::extract("arrays.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("arrays.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "this.items.filter" && row.status == "external" }),
        "returned-object Array member lost its literal type: {graph:#?}"
    );
    let rebound = "function make() { return { items: [], replace(next) { this.items = next; }, visible() { return this.items.filter(item => item); } }; }";
    let facts = ast::extract("rebound.js", "javascript", rebound).unwrap();
    let graph = linker::link(&BTreeMap::from([("rebound.js".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "this.items.filter" && row.status == "unresolved" }),
        "rebound object field acquired a stale Array type: {graph:#?}"
    );
    for source in [
        "const make = () => ({ count: 0, inc() { this.count += 1; } });",
        "function make() { return { count: 0, inc: function() { this.count += 1; } }; }",
    ] {
        let facts = ast::extract("factory.js", "javascript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("factory.js".to_owned(), facts)]));
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| { row.expression == "this.count" && row.status == "resolved" }),
            "returned object factory lost its receiver: {source}: {graph:#?}"
        );
    }
}

#[test]
fn javascript_frozen_factory_object_preserves_unique_local_member() {
    use contextunity_forge_mcp::db::{reader, writer};

    let source = r#"function make(namespace) {
  return Object.freeze({
    get: function(key) { return namespace + key; }
  });
}
const state = make('grid');
state.get('offset');
function shadow(Object) {
  const inner = Object.freeze({ get: function() {} });
  inner.get();
}
"#;
    let workspace = Workspace::new();
    std::fs::write(workspace.join("state.js"), source).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let edge_count: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='state.js' AND e.line=7 AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='state.get' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='state.js' AND dst.kind='function' AND dst.name='get' AND dst.line=3",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(
        edge_count, 1,
        "frozen factory member must retain its indexed provider"
    );
    let coverage = |line: i64| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage c WHERE (SELECT path FROM path_dictionary WHERE path_id=c.path_id)='state.js' AND c.line=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id) IN ('state.get','inner.get')",
            [line],
            |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(coverage(7), "resolved");
    assert_eq!(
        coverage(10),
        "unresolved",
        "shadowed Object has no built-in freeze contract"
    );
    drop(conn);
}

#[test]
fn typescript_optional_call_keeps_its_persisted_expression() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    std::fs::write(
        workspace.join("optional.ts"),
        "const client: { run(): void } = { run() {} };\nclient?.run();",
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let expressions: Vec<String> = conn.prepare(
        "SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id) FROM resolution_coverage c WHERE (SELECT path FROM path_dictionary WHERE path_id=c.path_id)='optional.ts' AND c.line=2",
    ).unwrap().query_map([], |row| row.get(0)).unwrap()
        .map(Result::unwrap).collect();
    assert!(
        expressions
            .iter()
            .any(|expression| expression == "client?.run"),
        "TypeScript keeps the original optional call expression: {expressions:?}"
    );
    assert!(
        !expressions
            .iter()
            .any(|expression| expression == "client.run"),
        "JavaScript-only normalization must not rewrite TypeScript coverage: {expressions:?}"
    );
    drop(conn);
}

#[test]
fn finite_standard_static_members_require_unshadowed_globals() {
    for (source, expected) in [
        ("function run() { Number.isFinite(1); Reflect.get({}, 'x'); crypto.randomUUID(); document.createElement('div'); window.addEventListener('click', () => {}); }", "external"),
        ("function run(Number, Reflect, crypto, document, window) { Number.isFinite(1); Reflect.get({}, 'x'); crypto.randomUUID(); document.createElement('div'); window.addEventListener('click', () => {}); }", "unresolved"),
    ] {
        let facts = ast::extract("static.ts", "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("static.ts".to_owned(), facts)]));
        for expression in ["Number.isFinite", "Reflect.get", "crypto.randomUUID", "document.createElement", "window.addEventListener"] {
            assert!(graph.coverage.iter().any(|row| row.expression == expression && row.status == expected), "{expression}: {graph:#?}");
        }
    }
    let facts = ast::extract(
        "comment-require.ts",
        "typescript",
        "function run(document) { document.createElement('div'); } // require",
    )
    .unwrap();
    let graph = linker::link(&BTreeMap::from([("comment-require.ts".to_owned(), facts)]));
    assert!(
        graph.coverage.iter().any(|row| {
            row.expression == "document.createElement" && row.status == "unresolved"
        }),
        "shadowed document must stay unresolved even when a comment mentions require: {graph:#?}"
    );
    let facts = ast::extract(
        "unknown.ts",
        "typescript",
        "Number.noSuchMember(); Reflect.noSuchMember(); crypto.noSuchMember();",
    )
    .unwrap();
    let graph = linker::link(&BTreeMap::from([("unknown.ts".to_owned(), facts)]));
    for expression in [
        "Number.noSuchMember",
        "Reflect.noSuchMember",
        "crypto.noSuchMember",
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
fn object_destructuring_of_declared_function_returns_preserves_member_types() {
    let provider = "export class Worker { work() {} } export interface Result { first: Worker; second: Worker; } export function make(): Result { return { first: new Worker(), second: new Worker() }; }";
    let consumer = "import { make } from './provider'; const { first, second: alias } = make(); first.work(); alias.work(); const { missing } = make(); missing.work();";
    let files = [("provider.ts", provider), ("consumer.ts", consumer)];
    let facts: BTreeMap<_, _> = files
        .into_iter()
        .map(|(path, source)| {
            (
                path.to_owned(),
                ast::extract(path, "typescript", source).unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
    for expression in ["first.work", "alias.work"] {
        let rows: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| row.path == "consumer.ts" && row.expression == expression)
            .collect();
        assert!(
            !rows.is_empty() && rows.iter().all(|row| row.status == "resolved"),
            "{expression}: {graph:#?}"
        );
        assert!(
            graph.edges.iter().any(|edge| edge.path == "consumer.ts"
                && edge.evidence == expression
                && edge.kind == "calls"
                && facts["provider.ts"]
                    .nodes
                    .iter()
                    .any(|node| node.id == edge.dst && node.name == "work")),
            "{expression}: {graph:#?}"
        );
    }
    assert!(
        graph.coverage.iter().any(|row| row.path == "consumer.ts"
            && row.expression == "missing.work"
            && row.status == "unresolved"),
        "{graph:#?}"
    );

    let source = "function useComposer() { function translate(key: string) { return key; } return { translate }; } const { translate } = useComposer(); translate('save');";
    let facts = ast::extract("composer.ts", "typescript", source).unwrap();
    let translate = facts
        .nodes
        .iter()
        .find(|node| node.name == "translate" && node.kind == "function")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("composer.ts".to_owned(), facts)]));
    assert!(
        graph.edges.iter().any(|edge| edge.path == "composer.ts"
            && edge.kind == "calls"
            && edge.evidence == "translate"
            && edge.dst == translate),
        "{graph:#?}"
    );

    for (path, import_clause) in [
        (
            "type-statement.ts",
            "import type { make } from './provider';",
        ),
        (
            "type-specifier.ts",
            "import { type make } from './provider';",
        ),
    ] {
        let consumer = format!("{import_clause}\nconst {{ first }} = make(); first.work();");
        let facts: BTreeMap<_, _> = [("provider.ts", provider), (path, consumer.as_str())]
            .into_iter()
            .map(|(path, source)| {
                (
                    path.to_owned(),
                    ast::extract(path, "typescript", source).unwrap(),
                )
            })
            .collect();
        let graph = linker::link(&facts);
        for expression in ["make", "first.work"] {
            let rows: Vec<_> = graph
                .coverage
                .iter()
                .filter(|row| row.path == path && row.line == 2 && row.expression == expression)
                .collect();
            assert!(
                !rows.is_empty() && rows.iter().all(|row| row.status == "unresolved"),
                "{path}: {expression}: {graph:#?}"
            );
            assert!(
                !graph.edges.iter().any(|edge| edge.path == path
                    && edge.kind == "calls"
                    && edge.evidence == expression),
                "{path}: {expression}: {graph:#?}"
            );
        }
    }
}

#[test]
fn javascript_and_typescript_chained_call_return_value_flow() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        ("chain.js", "function run() {\n  const el = document.createElement('div').appendChild(document.createElement('span'));\n  document.querySelector('#root').classList.add('active');\n  el.classList.add('child');\n}\n"),
        ("chain.ts", "function run() {\n  const el = document.createElement('div').appendChild(document.createElement('span'));\n  document.querySelector('#root').classList.add('active');\n  el.classList.add('child');\n}\n"),
    ] {
        std::fs::write(workspace.join(path), source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };
    for path in ["chain.js", "chain.ts"] {
        for expression in [
            "document.createElement('div').appendChild",
            "document.querySelector('#root').classList.add",
            "el.classList.add",
        ] {
            let row = coverage(path, expression);
            assert_eq!(row.0, "external", "{path}:{expression} status: {row:?}");
            assert!(
                row.1.contains("builtin:web_api"),
                "{path}:{expression} evidence: {row:?}"
            );
        }
    }
    drop(conn);
}

#[test]
fn typescript_constructor_field_assignments_resolve_proven_member_calls() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    std::fs::write(
        workspace.join("table.ts"),
        r#"
class TableView {
  constructor(element: HTMLElement) {
    this.tbody = element;
    this.activeRow = null;
  }
  render() {
    this.tbody.appendChild(document.createElement("tr"));
    this.activeRow.select();
  }
}
"#,
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();

    // Verify tbody and activeRow are indexed as fields under TableView
    let mut stmt = conn
        .prepare("SELECT name, kind, qualname FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='table.ts') AND kind='field' ORDER BY name")
        .unwrap();
    let fields: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(
        fields,
        vec![
            (
                "activeRow".to_string(),
                "field".to_string(),
                "table.TableView.activeRow".to_string()
            ),
            (
                "tbody".to_string(),
                "field".to_string(),
                "table.TableView.tbody".to_string()
            ),
        ],
        "fields in nodes table: {fields:?}"
    );

    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };

    let (tbody_status, tbody_evidence) = coverage("table.ts", "this.tbody.appendChild");
    assert_eq!(tbody_status, "external", "{tbody_evidence}");
    assert!(
        tbody_evidence.contains("builtin:web_api"),
        "{tbody_evidence}"
    );

    let (active_status, active_evidence) = coverage("table.ts", "this.activeRow.select");
    assert_eq!(active_status, "unresolved", "{active_evidence}");

    drop(stmt);
    drop(conn);
}

#[test]
fn constructor_field_type_survives_later_assignment() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    std::fs::write(
        workspace.join("table.ts"),
        r#"
class TableView {
  constructor(element: HTMLElement) {
    this.tbody = element;
  }
  reset(element: HTMLElement) {
    this.tbody = element;
  }
  touch() {
    this.tbody += 1;
  }
  render() {
    this.tbody.appendChild(document.createElement("tr"));
  }
}
"#,
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let (status, evidence): (String, String) = conn
        .query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='table.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='this.tbody.appendChild'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("builtin:web_api"), "{evidence}");
    drop(conn);
}

#[test]
fn static_this_does_not_call_instance_methods() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    std::fs::write(
        workspace.join("view.ts"),
        r#"
class View {
  instanceMethod(): HTMLElement {
    return document.createElement("div");
  }
  static make(): HTMLElement {
    return document.createElement("div");
  }
  static run() {
    const leaked = this.instanceMethod();
    leaked.appendChild(document.createElement("span"));
    const built = this.make();
    built.appendChild(document.createElement("span"));
  }
}
"#,
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='view.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            rusqlite::params![expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(coverage("leaked.appendChild").0, "unresolved");
    let built = coverage("built.appendChild");
    assert_eq!(built.0, "external", "{}", built.1);
    assert!(built.1.contains("builtin:web_api"), "{}", built.1);
    drop(conn);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn test_typescript_ast_extractor() {
    let source = r#"
import { Router } from "express";

export interface User {
    id: string;
    name: string;
}

export class UserService {
    getUser(id: string): User {
        return { id, name: "Alice" };
    }
}

export const processUser = (u: User) => {
    console.log(u);
};
"#;
    let facts =
        ast::extract("test.ts", "typescript", source).expect("typescript extraction failed");
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "class" && n.name == "UserService"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "getUser"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "processUser"));
    let process_user = facts
        .nodes
        .iter()
        .find(|n| n.name == "processUser")
        .expect("processUser function missing");
    assert!(process_user.details.get("is_method").is_none());
    assert!(process_user.details.get("is_static").is_none());
    assert!(process_user.details.get("receiver_type").is_none());
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_tsx_parser_slot_reuses_cleanly_across_files() {
    for (path, source, expected_name) in [
        (
            "First.tsx",
            "export const First = () => <main><span /></main>;",
            "First",
        ),
        (
            "Second.tsx",
            "export function Second() { return <button />; }",
            "Second",
        ),
    ] {
        let facts = ast::extract(path, "typescript", source).expect("TSX extraction failed");
        assert!(facts.errors.is_empty(), "{path}: {:?}", facts.errors);
        assert!(facts
            .nodes
            .iter()
            .any(|node| node.name == expected_name && node.kind == "function"));
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_lexical_this_nested_arrows() {
    let w = Workspace::new();
    w.write(
        "counter.ts",
        r#"class Counter {
  ready = false;
  static active = true;
  mark() {}
  static reset() {}
  run() {
    const read = () => this.ready;
    const call = () => this.mark();
    const ordinary = function() { return this.ready; };
    const unknown = () => this.missing;
    read(); call(); ordinary(); unknown();
  }
  static check() {
    const read = () => this.active;
    const call = () => this.reset();
    const bad = () => this.ready;
    read(); call(); bad();
  }
  constructor() {
    const initialize = () => this.ready;
    initialize();
  }
  nested() {
    const outer = () => () => this.mark();
    outer()();
  }
  paired() {
    const both = () => { this.mark(); const handler = this.mark; };
    both();
  }
}
function outside() { const arrow = () => this.mark(); arrow(); }"#,
    );
    w.write(
        "other.ts",
        "class Counter { ready = true; missing = 1; mark() {} }",
    );
    w.write(
        "counter.js",
        "class Counter { ready = false; run() { const read = () => this.ready; read(); } }",
    );
    let conn = w.build();
    let coverage = |path: &str, line: i64, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=?2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?3",
            rusqlite::params![path, line, expression],
            |row| row.get(0),
        ).unwrap()
    };
    let cov_ev = |path: &str, line: i64, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=?2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?3",
            rusqlite::params![path, line, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };
    for (path, line, expression) in [
        ("counter.ts", 7, "this.ready"),
        ("counter.ts", 8, "this.mark"),
        ("counter.ts", 14, "this.active"),
        ("counter.ts", 15, "this.reset"),
        ("counter.ts", 20, "this.ready"),
        ("counter.ts", 24, "this.mark"),
        ("counter.js", 1, "this.ready"),
    ] {
        let (st, ev) = cov_ev(path, line, expression);
        assert_eq!(
            st, "resolved",
            "{path}:{line} {expression} (evidence: {ev})"
        );
    }
    for (line, expression) in [
        (9, "this.ready"),
        (10, "this.missing"),
        (16, "this.ready"),
        (32, "this.mark"),
    ] {
        assert_eq!(
            coverage("counter.ts", line, expression),
            "unresolved",
            "counter.ts:{line} {expression}"
        );
    }
    for (path, line, target, kind) in [
        ("counter.ts", 7, "ready", "field"),
        ("counter.ts", 8, "mark", "method"),
        ("counter.ts", 14, "active", "field"),
        ("counter.ts", 15, "reset", "method"),
        ("counter.ts", 20, "ready", "field"),
        ("counter.ts", 24, "mark", "method"),
        ("counter.js", 1, "ready", "field"),
    ] {
        let edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.line=?2 AND e.kind IN ('references','calls') AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?1 AND dst.name=?3 AND dst.kind=?4",
            rusqlite::params![path, line, target, kind],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(
            edges, 1,
            "arrow must link to exact same-file {kind} {path}:{line} {target}"
        );
    }
    for kind in ["calls", "references"] {
        let edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='counter.ts' AND e.line=28 AND e.kind=?1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='counter.ts' AND dst.kind='method' AND dst.name='mark'",
            [kind], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            edges, 1,
            "same-line captured arrow needs an exact {kind} edge to Counter.mark"
        );
    }
    let sibling_edges: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='counter.ts' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='other.ts'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        sibling_edges, 0,
        "same-name Counter in sibling file cannot provide this members"
    );
}
