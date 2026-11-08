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
    assert!(resolves(
        "class Base { work() {} } class Worker extends Base {} function run() { const client = new Worker(); client.work(); }",
        "client.work"
    ));
    let source = "class Base { work() {} } class Worker extends Base { work() {} } function run() { const client = new Worker(); client.work(); }";
    let graph = linked("service.ts", "typescript", source);
    let facts = &graph.facts;
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == "client.work")
    );
    let expected = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with(".Worker.work"))
        .unwrap()
        .id
        .clone();
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == "client.work" && edge.dst == expected)
    );
    assert!(!resolves(
        "class A extends B {} class B extends A { work() {} } function run() { const client = new A(); client.work(); }",
        "client.work"
    ));
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
fn javascript_doc_types_supply_explicit_parameter_and_return_contracts() {
    for (source, expression) in [
        (
            "class Worker { work() {} }\n/** @param {Worker} client */\nfunction run(client) { client.work(); }",
            "client.work",
        ),
        (
            "class Worker { work() {} }\n/** @returns {Worker} */\nfunction make() { return new Worker(); }\nfunction run() { const client = make(); client.work(); }",
            "client.work",
        ),
    ] {
        let graph = linked("service.js", "javascript", source);
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.kind == "calls" && edge.evidence == expression),
            "{source}"
        );
    }
}

#[test]
fn computed_receivers_use_constructors_and_explicit_factory_returns() {
    assert!(resolves(
        "class Worker { work() {} } function run() { (new Worker()).work(); }",
        "(new Worker()).work"
    ));
    assert!(resolves(
        "class Worker { work() {} } function make(): Worker { return new Worker(); } function run() { make().work(); }",
        "make().work"
    ));
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
        let source = format!(
            "class Worker {{ work() {{}} }} class Other {{}}\n{declaration}\nconst output = make(new Worker()); output.work();"
        );
        let graph = linked(path, language, &source);
        for (expression, status) in [("client.work", "resolved"), ("output.work", expected)] {
            let rows: Vec<_> = graph
                .coverage
                .iter()
                .filter(|row| row.expression == expression)
                .collect();
            assert!(!rows.is_empty(), "{source}: {expression}");
            assert!(rows.iter().all(|row| row.status == status), "{source}: {rows:#?}");
        }
    }
}

#[test]
fn straight_line_factory_returns_prove_chains_and_reject_cycles_or_branches() {
    assert!(resolves(
        "class Worker { work() {} } function make() { return new Worker(); } function wrap() { const result = make(); return result; } function run() { const client = wrap(); client.work(); }",
        "client.work"
    ));
    assert!(resolves(
        "class Worker { work() {} } const make = () => new Worker(); function run() { const client = make(); client.work(); }",
        "client.work"
    ));
    for source in [
        "class Worker { work() {} } function make() { return wrap(); } function wrap() { return make(); } function run() { const client = make(); client.work(); }",
        "class Worker { work() {} } function make(flag) { if (flag) return new Worker(); return unknown(); } function run() { const client = make(true); client.work(); }",
        "class Worker { work() {} } function make(flag) { if (flag) return new Worker(); } function run() { const client = make(true); client.work(); }",
    ] {
        assert!(!resolves(source, "client.work"), "{source}");
    }
}

#[test]
fn generic_containers_prove_only_documented_container_members() {
    let members = "push pop shift unshift slice splice indexOf includes join map filter forEach reduce find findIndex some every flat flatMap";
    let member_calls = members
        .split_whitespace()
        .map(|member| format!("items.{member}();"))
        .collect::<String>();
    for annotation in ["Array<Worker>", "Worker[]", "Array<Array<Worker>>"] {
        let source = format!(
            "class Worker {{ work() {{}} }} function run() {{ const items: {annotation} = []; {member_calls} items.work(); }}"
        );
        let graph = linked("generic.ts", "typescript", &source);
        for member in members.split_whitespace() {
            assert!(
                graph
                    .coverage(&format!("items.{member}"))
                    .any(|row| row.status == "external" && row.evidence.contains("JavaScript/Web platform")),
                "{annotation}: {member}"
            );
        }
        assert!(!graph.edges.iter().any(|edge| edge.evidence == "items.work"));
    }
    let source =
        "class Array<T> { custom() {} } function run() { const items: Array<string> = unknown(); items.map(); }";
    let graph = linked("shadow.ts", "typescript", source);
    assert!(graph.statuses("items.map").contains(&"unresolved"));
    let source = "function Array<T>() { return unknown(); } function run() { const items: Array<string> = unknown(); items.map(); }";
    let graph = linked("function-shadow.ts", "typescript", source);
    assert!(graph.statuses("items.map").contains(&"unresolved"));
}

#[test]
fn async_and_generator_factories_do_not_expose_wrapped_return_members() {
    assert!(resolves(
        "class Worker { work() {} } function make() { return new Worker(); } function run() { const client = make(); client.work(); }",
        "client.work"
    ));
    for factory in [
        "async function make() { return new Worker(); }",
        "async function make(): Worker { return new Worker(); }",
        "const make = async () => new Worker();",
        "function* make(): Worker { return new Worker(); }",
        "class Factory { static async make(): Worker { return new Worker(); } } const make = Factory.make;",
    ] {
        let source = format!(
            "class Worker {{ work() {{}} }} {factory} function run() {{ const client = make(); client.work(); }}"
        );
        assert!(!resolves(&source, "client.work"), "{factory}");
    }
}

#[test]
fn static_this_and_nominal_calls_preserve_static_instance_boundaries() {
    let source = "class Worker { instance() {} static associated() {} static runStatic() { this.instance(); this.associated(); } runInstance() { this.instance(); this.associated(); } }";
    let graph = linked("static.ts", "typescript", source);
    let facts = &graph.facts;
    for edge in graph.edges.iter().filter(|edge| edge.kind == "calls") {
        let owner = facts.nodes.iter().find(|node| node.id == edge.src).unwrap();
        let target = facts.nodes.iter().find(|node| node.id == edge.dst).unwrap();
        assert_eq!(
            owner.details["is_static"], target.details["is_static"],
            "{} -> {}",
            owner.qualname, target.qualname
        );
    }
    assert_eq!(graph.edges.iter().filter(|edge| edge.kind == "calls").count(), 2);
    assert!(!resolves(
        "class Worker { work() {} make(): Worker { return new Worker(); } } function run() { const client = Worker.make(); client.work(); }",
        "client.work"
    ));
    assert!(resolves(
        "class Worker { work() {} static make(): Worker { return new Worker(); } } function run() { const client = Worker.make(); client.work(); }",
        "client.work"
    ));
}

#[test]
fn typescript_optional_call_keeps_its_persisted_expression() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "optional.ts",
        "const client: { run(): void } = { run() {} };\nclient?.run();",
    );
    workspace.build();
    let conn = workspace.open();
    let expressions: Vec<String> = conn.prepare(
        "SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id) FROM resolution_coverage c WHERE (SELECT path FROM path_dictionary WHERE path_id=c.path_id)='optional.ts' AND c.line=2",
    ).unwrap().query_map([], |row| row.get(0)).unwrap()
        .map(Result::unwrap).collect();
    assert!(
        expressions.iter().any(|expression| expression == "client?.run"),
        "TypeScript keeps the original optional call expression: {expressions:?}"
    );
    assert!(
        !expressions.iter().any(|expression| expression == "client.run"),
        "JavaScript-only normalization must not rewrite TypeScript coverage: {expressions:?}"
    );
    drop(conn);
}

#[test]
fn finite_standard_static_members_require_unshadowed_globals() {
    for (source, expected) in [
        (
            "function run() { Number.isFinite(1); Reflect.get({}, 'x'); crypto.randomUUID(); document.createElement('div'); window.addEventListener('click', () => {}); }",
            "external",
        ),
        (
            "function run(Number, Reflect, crypto, document, window) { Number.isFinite(1); Reflect.get({}, 'x'); crypto.randomUUID(); document.createElement('div'); window.addEventListener('click', () => {}); }",
            "unresolved",
        ),
    ] {
        let graph = linked("static.ts", "typescript", source);
        for expression in [
            "Number.isFinite",
            "Reflect.get",
            "crypto.randomUUID",
            "document.createElement",
            "window.addEventListener",
        ] {
            assert!(
                graph
                    .coverage
                    .iter()
                    .any(|row| row.expression == expression && row.status == expected),
                "{expression}: {graph:#?}"
            );
        }
    }
    let graph = ExtractorHarness::for_lang("typescript")
        .at_path("comment-require.ts")
        .linked("function run(document) { document.createElement('div'); } // require");
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "document.createElement" && row.status == "unresolved" }),
        "shadowed document must stay unresolved even when a comment mentions require: {graph:#?}"
    );
    let graph = ExtractorHarness::for_lang("typescript")
        .at_path("unknown.ts")
        .linked("Number.noSuchMember(); Reflect.noSuchMember(); crypto.noSuchMember();");
    for expression in ["Number.noSuchMember", "Reflect.noSuchMember", "crypto.noSuchMember"] {
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
fn typescript_constructor_field_assignments_resolve_proven_member_calls() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "table.ts",
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
    );
    workspace.build();
    let conn = workspace.open();

    // Verify tbody and activeRow are indexed as fields under TableView
    let mut stmt = conn.prepare("SELECT name, kind, qualname FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='table.ts') AND kind='field' ORDER BY name").unwrap();
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

    let (tbody_status, tbody_evidence) = persisted_coverage(&conn, "table.ts", None, "this.tbody.appendChild");
    assert_eq!(tbody_status, "external", "{tbody_evidence}");
    assert!(tbody_evidence.contains("builtin:web_api"), "{tbody_evidence}");

    let (active_status, active_evidence) = persisted_coverage(&conn, "table.ts", None, "this.activeRow.select");
    assert_eq!(active_status, "unresolved", "{active_evidence}");

    drop(stmt);
    drop(conn);
}

#[test]
fn constructor_field_type_survives_later_assignment() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "table.ts",
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
    );
    workspace.build();
    let conn = workspace.open();
    let (status, evidence) = persisted_coverage(&conn, "table.ts", None, "this.tbody.appendChild");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("builtin:web_api"), "{evidence}");
    drop(conn);
}

#[test]
fn static_this_does_not_call_instance_methods() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "view.ts",
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
    );
    workspace.build();
    let conn = workspace.open();
    assert_eq!(
        persisted_coverage(&conn, "view.ts", None, "leaked.appendChild").0,
        "unresolved"
    );
    let built = persisted_coverage(&conn, "view.ts", None, "built.appendChild");
    assert_eq!(built.0, "external", "{}", built.1);
    assert!(built.1.contains("builtin:web_api"), "{}", built.1);
    drop(conn);
}
