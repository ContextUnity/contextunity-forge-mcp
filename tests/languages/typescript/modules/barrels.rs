use super::*;
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
fn javascript_local_imports_require_indexed_project_targets() {
    let owner = crate::languages::PersistedWorkspaceHarness::new();
    let first = crate::languages::PersistedWorkspaceHarness::new();
    let second = crate::languages::PersistedWorkspaceHarness::new();
    for (result, project) in [("/first", &first), ("/second", &second)] {
        for (path, source) in [
            (
                "server/api-proxy-target.ts",
                format!("export function resolveBackendTarget() {{ return '{result}'; }}"),
            ),
            (
                "server/api-proxy-target.test.mjs",
                "import { resolveBackendTarget } from './api-proxy-target.ts'; resolveBackendTarget();".to_owned(),
            ),
            (
                "server/run-generated.mjs",
                "import { generatedTarget } from './dist/generated.mjs'; generatedTarget();".to_owned(),
            ),
            (
                "server/dist/generated.mjs",
                "export function generatedTarget() { return '/generated'; }".to_owned(),
            ),
        ] {
            project.write(path, &source);
        }
    }
    owner.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
            first.root().display(),
            second.root().display(),
        ),
    );
    let conn = owner.build_and_open();
    for name in ["first", "second"] {
        let owner = format!("[{name}]/server/api-proxy-target.test.mjs");
        let target = format!("[{name}]/server/api-proxy-target.ts");
        for kind in ["imports", "calls"] {
            let targets: Vec<String> = conn
                .prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind=?2 AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='resolveBackendTarget' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
                .unwrap()
                .query_map(rusqlite::params![owner, kind], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert_eq!(targets, [target.as_str()], "{kind} must stay in {name}");
        }
        let statuses: Vec<String> = conn.prepare(
            "SELECT status FROM resolution_coverage c WHERE (SELECT path FROM path_dictionary WHERE path_id=c.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id)='resolveBackendTarget' AND c.line=1",
        ).unwrap().query_map([&owner], |row| row.get(0)).unwrap()
            .map(Result::unwrap).collect();
        assert!(
            !statuses.is_empty() && statuses.iter().all(|status| status == "resolved"),
            "{owner}: {statuses:?}"
        );
        let generated_owner = format!("[{name}]/server/run-generated.mjs");
        let generated_statuses: Vec<String> = conn.prepare(
            "SELECT status FROM resolution_coverage c WHERE (SELECT path FROM path_dictionary WHERE path_id=c.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id)='generatedTarget' AND c.line=1",
        ).unwrap().query_map([&generated_owner], |row| row.get(0)).unwrap()
            .map(Result::unwrap).collect();
        assert!(
            !generated_statuses.is_empty() && generated_statuses.iter().all(|status| status == "unresolved"),
            "a scanner-excluded artifact is not an indexed provider in {name}: {generated_statuses:?}"
        );
    }
    drop(conn);
}

#[test]
fn commonjs_object_exports_resolve_local_functions() {
    let facts: BTreeMap<String, Facts> = extracted_files(
        &[
            (
                "provider.js",
                "function work() {} module.exports = { renamed: work };",
            ),
            (
                "consumer.js",
                "const { renamed } = require('./provider'); function run() { renamed(); }",
            ),
        ],
        Some("javascript"),
    );
    let graph = ExtractorHarness::link_graph(&facts);
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
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == "calls"
                    && edge.path == "consumer.js"
                    && edge.evidence == "renamed"
                    && facts["provider.js"]
                        .nodes
                        .iter()
                        .any(|node| node.id == edge.dst && node.name == "work")
            })
            .count(),
        1,
        "{graph:#?}"
    );
    assert!(graph.coverage.iter().any(|row| {
        row.path == "consumer.js" && row.expression == "renamed" && row.status == "resolved"
    }));
}

#[test]
fn commonjs_function_expression_export_binds_named_import() {
    let files = [
        (
            "provider.cjs",
            "exports.multiply = function(a, b) { return a * b; };",
        ),
        (
            "consumer.mjs",
            "import { multiply } from './provider.cjs'; function run() { multiply(2, 3); }",
        ),
    ];
    let facts: BTreeMap<String, Facts> = extracted_files(&files, Some("javascript"));
    let graph = ExtractorHarness::link_graph(&facts);
    let provider = &facts["provider.cjs"];
    let call_targets: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.kind == "calls" && edge.path == "consumer.mjs" && edge.evidence == "multiply"
        })
        .filter_map(|edge| provider.nodes.iter().find(|node| node.id == edge.dst))
        .collect();
    assert_eq!(call_targets.len(), 1, "{graph:#?}");
    assert_eq!(call_targets[0].kind, "function");
    assert_eq!(call_targets[0].line, 1);
    assert!(call_targets[0].details["signature"]
        .as_str()
        .is_some_and(|signature| signature.starts_with("function(a, b)")));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "imports"
            && edge.path == "consumer.mjs"
            && provider.nodes.iter().any(|node| node.id == edge.dst)
    }));
    assert!(graph.coverage.iter().any(|row| {
        row.path == "consumer.mjs" && row.expression == "multiply" && row.status == "resolved"
    }));

    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in files {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    let persisted_calls: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.mjs' AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='multiply' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='provider.cjs' AND dst.kind='function' AND dst.line=1",
        [],
    );
    assert_eq!(
        persisted_calls, 1,
        "persisted call must target the exported function"
    );
    let persisted_status: String = query_scalar(
        &conn,
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.mjs' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='multiply'",
        [],
    );
    assert_eq!(persisted_status, "resolved");
    drop(conn);
}

#[test]
fn commonjs_default_function_expression_export_binds_esm_and_require_calls() {
    let files = [
        (
            "provider.cjs",
            "module.exports = function(value) { return value + 1; };",
        ),
        (
            "consumer.mjs",
            "import increment from './provider.cjs'; increment(2);",
        ),
        (
            "consumer.cjs",
            "const increment = require('./provider.cjs'); increment(2);",
        ),
    ];
    let facts: BTreeMap<String, Facts> = extracted_files(&files, Some("javascript"));
    let graph = ExtractorHarness::link_graph(&facts);
    let provider = &facts["provider.cjs"];
    for path in ["consumer.mjs", "consumer.cjs"] {
        assert!(
            graph.edges.iter().any(|edge| {
                edge.kind == "calls"
                    && edge.path == path
                    && edge.evidence == "increment"
                    && provider
                        .nodes
                        .iter()
                        .any(|node| node.id == edge.dst && node.kind == "function")
            }),
            "{path} lost the exact CommonJS function provider: {graph:#?}"
        );
        assert!(
            graph.coverage.iter().any(|row| {
                row.path == path && row.expression == "increment" && row.status == "resolved"
            }),
            "{path}: {graph:#?}"
        );
    }

    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in files {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    let persisted_calls: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id) IN ('consumer.mjs', 'consumer.cjs') AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='increment' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='provider.cjs' AND dst.kind='function'",
        [],
    );
    assert_eq!(
        persisted_calls, 2,
        "both import styles must persist the exact callable provider"
    );
    drop(conn);

    for (provider_source, consumer_source) in [
        (
            "module.exports = function(value) { return value; };",
            "increment(1); const increment = require('./provider.cjs');",
        ),
        (
            "module.exports = function(value) { return value; }; module.exports = function(value) { return value + 1; };",
            "const increment = require('./provider.cjs'); increment(1);",
        ),
    ] {
        let guarded: BTreeMap<String, Facts> = extracted_files(
            &[("provider.cjs", provider_source), ("consumer.cjs", consumer_source)],
            Some("javascript"),
        );
        let guarded_graph = ExtractorHarness::link_graph(&guarded);
        assert!(
            guarded_graph.coverage.iter().any(|row| {
                row.path == "consumer.cjs"
                    && row.expression == "increment"
                    && matches!(row.status.as_str(), "unresolved" | "ambiguous")
            }),
            "source order and multiple default providers must fail closed: {guarded_graph:#?}"
        );
    }
}

#[test]
fn commonjs_namespace_require_links_exported_member() {
    let files = [
        (
            "provider.js",
            "function work() {} module.exports = { work };",
        ),
        (
            "consumer.js",
            "const provider = require('./provider'); function run() { provider.work(); }",
        ),
    ];
    assert_eq!(calls(&files, "provider.work"), ["provider.js"]);
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
        let facts: BTreeMap<String, Facts> = extracted_files(
            &[
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
                (provider_path.as_str(), "export function String(value) {}"),
                (
                    consumer_path.as_str(),
                    "import { String } from './provider'; function direct() { String(value); } function outer() { function inner() { String(value); } }",
                ),
            ],
            Some(language),
        );
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
        let graph = ExtractorHarness::link_graph(&facts);

        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == scope_path
                    && coverage.expression == "String"
                    && coverage.status == "external"
                    && coverage.evidence.contains("JavaScript/Web platform")
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
fn changing_a_transitive_barrel_matches_cold_resolution() {
    use contextunity_forge_mcp::core::commitments;
    let root = crate::languages::PersistedWorkspaceHarness::new();
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
        root.write(path, source);
    }
    root.build();
    root.write("inner.ts", "export {work} from './second';");
    root.delta(&["inner.ts"]);
    let delta = root.open();
    commitments::verify(&delta).unwrap();
    let targets = |connection: &rusqlite::Connection| {
        connection.prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.path_id=(SELECT path_id FROM path_dictionary WHERE path='consumer.ts') AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>()
    };
    assert_eq!(targets(&delta), ["second.ts"]);
    let cold = root.root().join(".forge/cold.sqlite");
    root.build_to(&cold);
    let cold = root.open_at(&cold);
    commitments::verify(&cold).unwrap();
    assert_eq!(targets(&delta), targets(&cold));
    drop(delta);
    drop(cold);
}

#[test]
fn namespace_imports_only_expose_proven_runtime_exports() {
    let files = [
        (
            "provider.ts",
            "function privateWork() {} export function publicWork() {} export interface Contract { run(): void; }",
        ),
        (
            "consumer.ts",
            "import * as api from './provider'; function run() { api.privateWork(); api.publicWork(); api.Contract(); }",
        ),
    ];
    assert!(calls(&files, "api.privateWork").is_empty());
    assert_eq!(calls(&files, "api.publicWork"), ["provider.ts"]);
    assert!(calls(&files, "api.Contract").is_empty());
}

#[test]
fn namespace_factory_summaries_require_public_exports() {
    let private = [
        (
            "provider.ts",
            "export class Worker { work() {} } function make(): Worker { return new Worker(); }",
        ),
        (
            "consumer.ts",
            "import * as api from './provider'; function run() { const client = api.make(); client.work(); }",
        ),
    ];
    assert!(calls(&private, "client.work").is_empty());
    let public = [
        (
            "provider.ts",
            "export class Worker { work() {} } export function make(): Worker { return new Worker(); }",
        ),
        private[1],
    ];
    assert_eq!(calls(&public, "client.work"), ["provider.ts"]);
}

#[test]
fn commonjs_capture_keeps_linked_projects_isolated() {
    let linked = crate::languages::PersistedWorkspaceHarness::new();
    let first = crate::languages::PersistedWorkspaceHarness::new();
    let second = crate::languages::PersistedWorkspaceHarness::new();
    for project in [&first, &second] {
        project.write(
            "provider.js",
            "function work() {} module.exports = { work };",
        );
        project.write(
            "consumer.cjs",
            "const local = require('./provider'); const config = { enabled: true }; function run() { local.work(); }",
        );
    }
    linked.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
            first.root().display(),
            second.root().display()
        ),
    );
    let conn = linked.build_and_open();
    for name in ["first", "second"] {
        let owner = format!("[{name}]/consumer.cjs");
        let target = format!("[{name}]/provider.js");
        let edges: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='local.work' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND dst.name='work'",
            rusqlite::params![owner, target],
        );
        assert_eq!(edges, 1, "CommonJS capture crossed linked project {name}");
        let all_edges: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='local.work'",
            rusqlite::params![owner],
        );
        assert_eq!(
            all_edges, 1,
            "CommonJS capture selected another project provider"
        );
    }
    drop(conn);
}
