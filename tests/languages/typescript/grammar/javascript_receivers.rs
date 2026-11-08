use super::*;
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
        let graph = linked(path, language, source);
        assert!(
            graph.statuses("receiver.run").contains(&status),
            "{path}: expected {status} from lexical receiver provenance, got {:#?}",
            graph.coverage
        );
        let targets: Vec<_> = graph.call_targets("receiver.run");
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
    let graph = linked("service.js", "javascript", source);
    for expression in ["service.run", "new Service().run", "this.finish"] {
        let targets: Vec<_> = graph.call_targets(expression);
        assert_eq!(targets.len(), 1, "{expression}: {graph:#?}");
        assert_eq!(targets[0].kind, "method", "{expression}: {targets:#?}");
        assert!(
            graph.statuses(expression).contains(&"resolved"),
            "{expression}: {graph:#?}"
        );
    }
    assert!(
        graph
            .call_targets("run")
            .iter()
            .any(|node| node.kind == "function" && node.name == "run"),
        "ordinary run() must keep its lexical function provider: {graph:#?}"
    );

    let unknown = "class Service { finish() {} }\nfunction Other() {}\nOther.prototype.run = function() {}\nfunction use() { const service = new Service(); service.run(); }";
    let graph = linked("unknown.js", "javascript", unknown);
    assert!(
        graph.statuses("service.run").contains(&"unresolved"),
        "{graph:#?}"
    );

    for source in [
        "Service.prototype.run = function() {}; class Service {} const service = new Service(); service.run();",
        "class Service {} const Alias = Service; Alias.prototype.run = function() {}; const service = new Service(); service.run();",
    ] {
        let graph = linked("guarded.js", "javascript", source);
        assert!(
            graph.statuses("service.run").contains(&"unresolved"),
            "prototype provider must be lexically visible and exact: {source}: {graph:#?}"
        );
    }

    let legacy = r#"function Legacy() {}
Legacy.prototype.finish = function() {};
Legacy.prototype.run = function() { const callback = () => this.finish(); callback(); };
function use() { const receiver = new Legacy(); receiver.run(); }
"#;
    let graph = linked("legacy.js", "javascript", legacy);
    for expression in ["receiver.run", "this.finish"] {
        assert!(
            graph
                .call_targets(expression)
                .iter()
                .any(|node| node.kind == "method"),
            "{expression}: {graph:#?}"
        );
    }

    let rebound = "class Service {}\nService.prototype.run = function() {};\nService.prototype.run = null;\nfunction use() { new Service().run(); }";
    let graph = linked("rebound.js", "javascript", rebound);
    assert!(
        graph.statuses("new Service().run").contains(&"unresolved"),
        "{graph:#?}"
    );

    let deleted = "class Service {}\nService.prototype.run = function() {};\ndelete Service.prototype.run;\nfunction use() { new Service().run(); }";
    let graph = linked("deleted.js", "javascript", deleted);
    assert!(
        graph.statuses("new Service().run").contains(&"unresolved"),
        "deleted prototype member must lose its provider: {graph:#?}"
    );

    let replaced = "function Service() {}\nService.prototype.run = function() {};\nService.prototype = {};\nfunction use() { new Service().run(); }";
    let graph = linked("replaced.js", "javascript", replaced);
    assert!(
        graph.statuses("new Service().run").contains(&"unresolved"),
        "replaced prototype must lose its prior method provider: {graph:#?}"
    );
}

#[test]
fn javascript_prototype_identifier_rhs_requires_a_unique_stable_function() {
    let source = "function Service() {} function implementation(value) { return value; } Service.prototype.run = implementation; new Service().run(1);";
    let graph = linked("named.js", "javascript", source);
    let facts = &graph.facts;
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
        let graph = linked("guarded.js", "javascript", source);
        assert!(
            graph
                .coverage("new Service().run")
                .any(|row| { matches!(row.status.as_str(), "unresolved" | "ambiguous") }),
            "unproven implementation binding must fail closed: {source}: {graph:#?}"
        );
    }
}

#[test]
fn javascript_prototype_arrow_rhs_is_callable_without_inheriting_receiver_this() {
    let source = "class Service { finish() {} } Service.prototype.run = () => this.finish(); new Service().run();";
    let graph = linked("arrow.js", "javascript", source);
    assert!(
        graph.statuses("new Service().run").contains(&"resolved"),
        "prototype arrow must remain callable: {graph:#?}"
    );
    assert!(
        graph.statuses("this.finish").contains(&"unresolved"),
        "prototype arrow retains lexical this instead of Service receiver: {graph:#?}"
    );
}

#[test]
fn javascript_prototype_call_requires_method_assignment_before_use() {
    let source =
        "function Service() {} new Service().run(); Service.prototype.run = function() {};";
    let graph = linked("early.js", "javascript", source);
    assert!(
        graph.statuses("new Service().run").contains(&"unresolved"),
        "use before prototype assignment must remain unresolved: {graph:#?}"
    );
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
    let graph = linked("objects.js", "javascript", source);
    let facts = &graph.facts;
    for expression in ["this.count", "this.reset"] {
        assert!(
            graph
                .coverage(expression)
                .any(|row| row.line == 4 && row.status == "resolved"),
            "{expression}: {graph:#?}"
        );
    }
    for line in [8, 10, 11] {
        assert!(
            graph
                .coverage("this.count")
                .any(|row| row.line == line && row.status == "unresolved"),
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
        graph
            .coverage("nav.reset")
            .any(|row| row.line == 9 && row.status == "resolved"),
        "returned-object method is unresolved: {graph:#?}"
    );
    assert!(
        graph
            .coverage("nav.count")
            .any(|row| row.line == 9 && row.status == "unresolved"),
        "field must not become callable: {graph:#?}"
    );
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("objects.js", source);
    workspace.build();
    let conn = workspace.open();
    let persisted: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='objects.js' AND line=4 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='this.count' AND status='resolved'",
        [],
    );
    assert_eq!(persisted, 1);
    let exact_edge: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='objects.js' AND e.line=4 AND e.kind='mutates' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='objects.js' AND dst.name='count' AND dst.kind='field' AND dst.line=3",
        [],
    );
    assert_eq!(exact_edge, 1);
    drop(conn);
    let source = "function make() { return { items: [], visible() { return this.items.filter(item => item); } }; }";
    let graph = linked("arrays.js", "javascript", source);
    assert!(
        graph.statuses("this.items.filter").contains(&"external"),
        "returned-object Array member lost its literal type: {graph:#?}"
    );
    let rebound = "function make() { return { items: [], replace(next) { this.items = next; }, visible() { return this.items.filter(item => item); } }; }";
    let graph = linked("rebound.js", "javascript", rebound);
    assert!(
        graph.statuses("this.items.filter").contains(&"unresolved"),
        "rebound object field acquired a stale Array type: {graph:#?}"
    );
    for source in [
        "const make = () => ({ count: 0, inc() { this.count += 1; } });",
        "function make() { return { count: 0, inc: function() { this.count += 1; } }; }",
    ] {
        let graph = linked("factory.js", "javascript", source);
        assert!(
            graph.statuses("this.count").contains(&"resolved"),
            "returned object factory lost its receiver: {source}: {graph:#?}"
        );
    }
}

#[test]
fn javascript_frozen_factory_object_preserves_unique_local_member() {
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
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("state.js", source);
    workspace.build();
    let conn = workspace.open();
    let edge_count: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='state.js' AND e.line=7 AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='state.get' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='state.js' AND dst.kind='function' AND dst.name='get' AND dst.line=3",
        [],
    );
    assert_eq!(
        edge_count, 1,
        "frozen factory member must retain its indexed provider"
    );
    assert_eq!(
        persisted_coverage(&conn, "state.js", Some(7), "state.get").0,
        "resolved"
    );
    assert_eq!(
        persisted_coverage(&conn, "state.js", Some(10), "inner.get").0,
        "unresolved",
        "shadowed Object has no built-in freeze contract"
    );
    drop(conn);
}

#[test]
fn object_destructuring_of_declared_function_returns_preserves_member_types() {
    let provider = "export class Worker { work() {} } export interface Result { first: Worker; second: Worker; } export function make(): Result { return { first: new Worker(), second: new Worker() }; }";
    let consumer = "import { make } from './provider'; const { first, second: alias } = make(); first.work(); alias.work(); const { missing } = make(); missing.work();";
    let files = [("provider.ts", provider), ("consumer.ts", consumer)];
    let (facts, graph) = linked_files(&files);
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
    let facts = ExtractorHarness::for_lang("typescript")
        .at_path("composer.ts")
        .extract(source);
    let translate = facts
        .nodes
        .iter()
        .find(|node| node.name == "translate" && node.kind == "function")
        .unwrap()
        .id
        .clone();
    let graph = ExtractorHarness::link_graph(&BTreeMap::from([("composer.ts".to_owned(), facts)]));
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
        let (_, graph) = linked_files(&[("provider.ts", provider), (path, consumer.as_str())]);
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
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "chain.js",
            "function run() {\n  const el = document.createElement('div').appendChild(document.createElement('span'));\n  document.querySelector('#root').classList.add('active');\n  el.classList.add('child');\n}\n",
        ),
        (
            "chain.ts",
            "function run() {\n  const el = document.createElement('div').appendChild(document.createElement('span'));\n  document.querySelector('#root').classList.add('active');\n  el.classList.add('child');\n}\n",
        ),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    for path in ["chain.js", "chain.ts"] {
        for expression in [
            "document.createElement('div').appendChild",
            "document.querySelector('#root').classList.add",
            "el.classList.add",
        ] {
            let row = persisted_coverage(&conn, path, None, expression);
            assert_eq!(row.0, "external", "{path}:{expression} status: {row:?}");
            assert!(
                row.1.contains("builtin:web_api"),
                "{path}:{expression} evidence: {row:?}"
            );
        }
    }
    drop(conn);
}
