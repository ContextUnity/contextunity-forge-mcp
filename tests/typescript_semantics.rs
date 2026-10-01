#![cfg(feature = "lang-typescript")]

use contextunity_forge_mcp::{
    core::models::Facts,
    engine::{ast, linker},
};
use std::collections::BTreeMap;

fn calls(files: &[(&str, &str)], expression: &str) -> Vec<String> {
    let facts: BTreeMap<String, Facts> = files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ast::extract(
                    path,
                    if path.ends_with(".js") {
                        "javascript"
                    } else {
                        "typescript"
                    },
                    source,
                )
                .unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
    graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == expression)
        .map(|edge| {
            facts
                .values()
                .flat_map(|facts| &facts.nodes)
                .find(|node| node.id == edge.dst)
                .unwrap()
                .path
                .clone()
        })
        .collect()
}

fn resolves(source: &str, expression: &str) -> bool {
    let facts = ast::extract("service.ts", "typescript", source).unwrap();
    linker::link(&BTreeMap::from([("service.ts".to_owned(), facts)]))
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == expression)
}

#[test]
fn named_and_star_barrels_resolve_confirmed_exports() {
    for barrel in [
        "export { work as alias } from './provider';",
        "export * from './provider';",
    ] {
        let name = if barrel.contains("alias") {
            "alias"
        } else {
            "work"
        };
        let consumer =
            format!("import {{ {name} }} from './barrel'; function run() {{ {name}(); }}");
        assert_eq!(
            calls(
                &[
                    ("provider.ts", "export function work() {}"),
                    ("barrel.ts", barrel),
                    ("consumer.ts", &consumer),
                ],
                name,
            ),
            ["provider.ts"]
        );
    }
}

#[test]
fn commonjs_object_exports_resolve_local_functions() {
    let facts: BTreeMap<String, Facts> = [
        (
            "provider.js",
            "function work() {} module.exports = { renamed: work };",
        ),
        (
            "consumer.js",
            "const { renamed } = require('./provider'); function run() { renamed(); }",
        ),
    ]
    .iter()
    .map(|(path, source)| {
        (
            (*path).to_owned(),
            ast::extract(path, "javascript", source).unwrap(),
        )
    })
    .collect();
    let graph = linker::link(&facts);
    let import_target = graph
        .edges
        .iter()
        .find(|edge| {
            edge.kind == "imports"
                && edge.evidence == "renamed"
                && edge.dst.starts_with("ts:provider.js")
        })
        .map(|edge| {
            facts
                .values()
                .flat_map(|facts| &facts.nodes)
                .find(|node| node.id == edge.dst)
                .unwrap()
                .name
                .as_str()
        });
    assert_eq!(import_target, Some("work"));
}

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
fn platform_member_contracts_require_unshadowed_known_members() {
    for (source, expression, expected) in [
        (
            "function run() { Object.assign({}, {}); JSON.parse('{}'); }",
            "Object.assign",
            "resolved",
        ),
        (
            "function run(Object) { Object.assign({}, {}); }",
            "Object.assign",
            "unresolved",
        ),
        (
            "function run() { Object.unconfirmed(); }",
            "Object.unconfirmed",
            "unresolved",
        ),
    ] {
        let facts = ast::extract("platform.ts", "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("platform.ts".to_owned(), facts)]));
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            expected
        );
    }
}

#[test]
fn conversion_globals_resolve_for_javascript_and_typescript_with_local_precedence() {
    for (extension, language) in [("ts", "typescript"), ("js", "javascript")] {
        let globals_path = format!("globals.{extension}");
        let local_path = format!("local.{extension}");
        let shadowed_path = format!("shadowed.{extension}");
        let facts: BTreeMap<String, Facts> = [
            (
                globals_path.as_str(),
                "function run() { String(value); Number(value); Boolean(value); }",
            ),
            (
                local_path.as_str(),
                "function local() { function String(value) { return value; } String(value); }",
            ),
            (
                shadowed_path.as_str(),
                "function shadowed(Boolean) { Boolean(value); }",
            ),
        ]
        .into_iter()
        .map(|(path, source)| {
            (
                path.to_owned(),
                ast::extract(path, language, source).unwrap(),
            )
        })
        .collect();
        let local_string = facts[&local_path]
            .nodes
            .iter()
            .find(|node| node.kind == "function" && node.name == "String")
            .unwrap()
            .id
            .clone();
        let graph = linker::link(&facts);

        for expression in ["String", "Number", "Boolean"] {
            assert!(
                graph.coverage.iter().any(|coverage| {
                    coverage.path == globals_path
                        && coverage.expression == expression
                        && coverage.status == "resolved"
                        && coverage.evidence
                            == format!("standard library or built-in callee: {expression}")
                }),
                "{language} {expression}"
            );
        }
        assert!(
            graph.edges.iter().any(|edge| {
                edge.path == local_path
                    && edge.kind == "calls"
                    && edge.evidence == "String"
                    && edge.dst == local_string
            }),
            "{language} local String declaration"
        );
        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == shadowed_path
                    && coverage.expression == "Boolean"
                    && coverage.status == "unresolved"
            }),
            "{language} shadowed unknown Boolean parameter"
        );
    }
}

#[test]
fn callable_globals_respect_lexical_scope_and_imported_targets() {
    for (extension, language) in [("ts", "typescript"), ("js", "javascript")] {
        let scope_path = format!("scope.{extension}");
        let no_keys_path = format!("no-keys.{extension}");
        let static_keys_path = format!("static-keys.{extension}");
        let shadowed_path = format!("shadowed-object.{extension}");
        let provider_path = format!("provider.{extension}");
        let consumer_path = format!("consumer.{extension}");
        let facts: BTreeMap<String, Facts> = [
            (
                scope_path.as_str(),
                "function run() { String(value); } function localScope() { function String(value) {} function inner() { String(value); } }",
            ),
            (
                no_keys_path.as_str(),
                "class Object {} function run() { Object.keys(value); }",
            ),
            (
                static_keys_path.as_str(),
                "class Object { static keys(value) {} } function run() { Object.keys(value); }",
            ),
            (
                shadowed_path.as_str(),
                "function byParameter(Object) { Object.keys(value); } function outer(Object) { function inner() { Object.keys(value); } }",
            ),
            (
                provider_path.as_str(),
                "export function String(value) {}",
            ),
            (
                consumer_path.as_str(),
                "import { String } from './provider'; function direct() { String(value); } function outer() { function inner() { String(value); } }",
            ),
        ]
        .into_iter()
        .map(|(path, source)| {
            (
                path.to_owned(),
                ast::extract(path, language, source).unwrap(),
            )
        })
        .collect();
        let local_string = facts[&scope_path]
            .nodes
            .iter()
            .find(|node| node.kind == "function" && node.name == "String")
            .unwrap()
            .id
            .clone();
        let static_keys = facts[&static_keys_path]
            .nodes
            .iter()
            .find(|node| node.kind == "method" && node.name == "keys")
            .unwrap()
            .id
            .clone();
        let imported_string = facts[&provider_path]
            .nodes
            .iter()
            .find(|node| node.kind == "function" && node.name == "String")
            .unwrap()
            .id
            .clone();
        let graph = linker::link(&facts);

        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == scope_path
                    && coverage.expression == "String"
                    && coverage.status == "resolved"
                    && coverage.evidence == "standard library or built-in callee: String"
            }),
            "{language} global String in sibling scope"
        );
        assert!(
            graph.edges.iter().any(|edge| {
                edge.path == scope_path
                    && edge.kind == "calls"
                    && edge.evidence == "String"
                    && edge.dst == local_string
            }),
            "{language} captured local String"
        );
        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == no_keys_path
                    && coverage.expression == "Object.keys"
                    && coverage.status == "unresolved"
            }),
            "{language} local Object without keys"
        );
        assert!(
            graph.edges.iter().any(|edge| {
                edge.path == static_keys_path
                    && edge.kind == "calls"
                    && edge.evidence == "Object.keys"
                    && edge.dst == static_keys
            }),
            "{language} local Object.keys static method"
        );
        assert_eq!(
            graph
                .coverage
                .iter()
                .filter(|coverage| {
                    coverage.path == shadowed_path
                        && coverage.expression == "Object.keys"
                        && coverage.status == "unresolved"
                })
                .count(),
            2,
            "{language} parameter and captured unknown Object"
        );
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.path == consumer_path
                        && edge.kind == "calls"
                        && edge.evidence == "String"
                        && edge.dst == imported_string
                })
                .count(),
            2,
            "{language} direct and captured imported String calls"
        );
    }
}

#[test]
fn conflicting_exports_cycles_private_and_type_exports_fail_closed() {
    for files in [
        vec![
            ("first.ts", "export function work() {}"),
            ("second.ts", "export function work() {}"),
            (
                "barrel.ts",
                "export * from './first'; export * from './second';",
            ),
            (
                "consumer.ts",
                "import {work} from './barrel'; function run() { work(); }",
            ),
        ],
        vec![
            ("first.ts", "export function work() {}"),
            ("second.ts", "export function work() {}"),
            ("indirect.ts", "export * from './second';"),
            (
                "barrel.ts",
                "export * from './first'; export * from './indirect';",
            ),
            ("outer.ts", "export {work} from './barrel';"),
            (
                "consumer.ts",
                "import {work} from './outer'; function run() { work(); }",
            ),
        ],
        vec![
            ("a.ts", "export {work} from './b';"),
            ("b.ts", "export {work} from './a';"),
            (
                "consumer.ts",
                "import {work} from './a'; function run() { work(); }",
            ),
        ],
        vec![
            ("provider.ts", "function privateWork() {}"),
            (
                "barrel.ts",
                "export {privateWork as work} from './provider';",
            ),
            (
                "consumer.ts",
                "import {work} from './barrel'; function run() { work(); }",
            ),
        ],
        vec![
            ("provider.ts", "export class Worker {}"),
            (
                "barrel.ts",
                "export type {Worker as work} from './provider';",
            ),
            (
                "consumer.ts",
                "import {work} from './barrel'; function run() { work(); }",
            ),
        ],
    ] {
        assert!(calls(&files, "work").is_empty(), "{files:?}");
    }
}

#[test]
fn object_methods_require_actual_declarations_and_stable_bindings() {
    assert!(resolves(
        "function run() { const client = {work() {}}; client.work(); }",
        "client.work"
    ));
}

#[test]
fn changing_a_transitive_barrel_matches_cold_resolution() {
    use contextunity_forge_mcp::{
        core::commitments,
        db::{reader, writer},
    };
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "forge_ts_exports_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    for (path, source) in [
        ("first.ts", "export function work() {}"),
        ("second.ts", "export function work() {}"),
        ("inner.ts", "export {work} from './first';"),
        ("outer.ts", "export {work} from './inner';"),
        (
            "consumer.ts",
            "import {work} from './outer'; function run() { work(); }",
        ),
    ] {
        fs::write(root.join(path), source).unwrap();
    }
    let db = root.join(".forge/index.sqlite");
    writer::build(&root, &db, None).unwrap();
    fs::write(root.join("inner.ts"), "export {work} from './second';").unwrap();
    writer::delta(&root, &db, &[PathBuf::from("inner.ts")]).unwrap();
    let delta = reader::open(&db, &root).unwrap();
    commitments::verify(&delta).unwrap();
    let targets = |connection: &rusqlite::Connection| {
        connection.prepare("SELECT dst.path FROM edges e JOIN nodes src ON src.id=e.src_public_id JOIN nodes dst ON dst.id=e.dst_public_id WHERE src.path='consumer.ts' AND e.kind='calls' ORDER BY dst.path").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>()
    };
    assert_eq!(targets(&delta), ["second.ts"]);
    let cold = root.join(".forge/cold.sqlite");
    writer::build(&root, &cold, None).unwrap();
    let cold = reader::open(&cold, &root).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(targets(&delta), targets(&cold));
    drop(delta);
    drop(cold);
    fs::remove_dir_all(root).unwrap();
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
fn typed_platform_members_are_finite_and_respect_local_shadowing() {
    for (source, expression, expected) in [
        (
            "function run(value: string) { value.trim(); }",
            "value.trim",
            "resolved",
        ),
        (
            "function run(value: string) { value.arbitrary(); }",
            "value.arbitrary",
            "unresolved",
        ),
        (
            "class String { trim() {} } function run(value: String) { value.trim(); }",
            "value.trim",
            "resolved",
        ),
    ] {
        let facts = ast::extract("platform.ts", "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("platform.ts".to_owned(), facts)]));
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            expected
        );
        if source.starts_with("function") {
            assert!(!graph
                .edges
                .iter()
                .any(|edge| edge.kind == "calls" && edge.evidence == expression));
        }
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
fn namespace_imports_only_expose_proven_runtime_exports() {
    let files = [
        ("provider.ts", "function privateWork() {} export function publicWork() {} export interface Contract { run(): void; }"),
        ("consumer.ts", "import * as api from './provider'; function run() { api.privateWork(); api.publicWork(); api.Contract(); }"),
    ];
    assert!(calls(&files, "api.privateWork").is_empty());
    assert_eq!(calls(&files, "api.publicWork"), ["provider.ts"]);
    assert!(calls(&files, "api.Contract").is_empty());
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
fn generic_containers_prove_only_documented_container_members() {
    let members = "push pop shift unshift slice splice indexOf includes join map filter forEach reduce find findIndex some every flat flatMap";
    let member_calls = members.split_whitespace().map(|member| format!("items.{member}();")).collect::<String>();
    for annotation in ["Array<Worker>", "Worker[]", "Array<Array<Worker>>"] {
        let source = format!("class Worker {{ work() {{}} }} function run() {{ const items: {annotation} = []; {member_calls} items.work(); }}");
        let facts = ast::extract("generic.ts", "typescript", &source).unwrap();
        let graph = linker::link(&BTreeMap::from([("generic.ts".into(), facts)]));
        for member in members.split_whitespace() { assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == format!("items.{member}") && row.status == "resolved"),
            "{annotation}: {member}"
        ); }
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
fn namespace_factory_summaries_require_public_exports() {
    let private = [
        ("provider.ts", "export class Worker { work() {} } function make(): Worker { return new Worker(); }"),
        ("consumer.ts", "import * as api from './provider'; function run() { const client = api.make(); client.work(); }"),
    ];
    assert!(calls(&private, "client.work").is_empty());
    let public = [
        ("provider.ts", "export class Worker { work() {} } export function make(): Worker { return new Worker(); }"),
        private[1],
    ];
    assert_eq!(calls(&public, "client.work"), ["provider.ts"]);
}

#[test]
fn finite_browser_globals_resolve_in_javascript_and_typescript() {
    let expressions = "fetch alert confirm prompt document.querySelector document.querySelectorAll document.getElementById document.getElementsByClassName document.getElementsByTagName document.createElement document.createTextNode document.addEventListener document.removeEventListener window.addEventListener window.removeEventListener window.dispatchEvent window.setTimeout window.clearTimeout window.setInterval window.clearInterval window.requestAnimationFrame window.cancelAnimationFrame localStorage.getItem localStorage.setItem localStorage.removeItem localStorage.clear sessionStorage.getItem sessionStorage.setItem sessionStorage.removeItem sessionStorage.clear console.debug console.info console.trace console.table";
    let source = r#"function run() { const handler = () => {};
        fetch('/api'); alert('message'); confirm('message'); prompt('message');
        document.querySelector('div'); document.querySelectorAll('div'); document.getElementById('item'); document.getElementsByClassName('item'); document.getElementsByTagName('div');
        document.createElement('div'); document.createTextNode('text'); document.addEventListener('click', handler); document.removeEventListener('click', handler);
        window.addEventListener('click', handler); window.removeEventListener('click', handler); window.dispatchEvent(new Event('change'));
        window.setTimeout(handler, 0); window.clearTimeout(1); window.setInterval(handler, 0); window.clearInterval(1); window.requestAnimationFrame(handler); window.cancelAnimationFrame(1);
        localStorage.getItem('key'); localStorage.setItem('key', 'value'); localStorage.removeItem('key'); localStorage.clear();
        sessionStorage.getItem('key'); sessionStorage.setItem('key', 'value'); sessionStorage.removeItem('key'); sessionStorage.clear();
        console.debug('message'); console.info('message'); console.trace('message'); console.table([]);
    }"#;
    for (path, language) in [("browser.ts", "typescript"), ("browser.js", "javascript")] {
        let facts = ast::extract(path, language, source).unwrap();
        assert!(facts.errors.is_empty(), "{facts:#?}");
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
        for expression in expressions.split_whitespace() {
            assert!(graph.coverage.iter().any(|row| row.expression == expression && row.status == "resolved"), "{language}: {expression}");
        }
    }
}

#[test]
fn browser_names_preserve_local_providers_and_unknown_parameter_boundaries() {
    assert_eq!(calls(&[("local.ts", "function fetch() {} function run() { fetch(); }")], "fetch"), ["local.ts"]);
    assert_eq!(calls(&[("local.ts", "class Document { querySelector() {} } function run(document: Document) { document.querySelector(); }")], "document.querySelector"), ["local.ts"]);
    let facts = ast::extract("shadow.ts", "typescript", "function run(fetch, document, window, localStorage, sessionStorage) { fetch('/api'); document.querySelector('div'); window.addEventListener('click', handler); localStorage.getItem('key'); sessionStorage.clear(); }").unwrap();
    let graph = linker::link(&BTreeMap::from([("shadow.ts".to_owned(), facts)]));
    for expression in ["fetch", "document.querySelector", "window.addEventListener", "localStorage.getItem", "sessionStorage.clear"] {
        assert!(graph.coverage.iter().any(|row| row.expression == expression && row.status == "unresolved"), "{expression}");
    }
}
