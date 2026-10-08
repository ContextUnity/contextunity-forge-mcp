use super::*;
use super::super::support::Workspace;

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
    use contextunity_forge_mcp::db::{reader, writer};

    let base = Workspace::new();
    for (name, result) in [("first", "/first"), ("second", "/second")] {
        let root = base.join(name);
        for (path, source) in [
            ("server/api-proxy-target.ts", format!("export function resolveBackendTarget() {{ return '{result}'; }}")),
            ("server/api-proxy-target.test.mjs", "import { resolveBackendTarget } from './api-proxy-target.ts'; resolveBackendTarget();".to_owned()),
            ("server/run-generated.mjs", "import { generatedTarget } from './dist/generated.mjs'; generatedTarget();".to_owned()),
            ("server/dist/generated.mjs", "export function generatedTarget() { return '/generated'; }".to_owned()),
        ] {
            let target = root.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, source).unwrap();
        }
    }
    let workspace = base.join("linked");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("forge-mcp.yaml"), format!(
        "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
        base.join("first").display(), base.join("second").display(),
    )).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
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
        assert!(!generated_statuses.is_empty() && generated_statuses.iter().all(|status| status == "unresolved"),
            "a scanner-excluded artifact is not an indexed provider in {name}: {generated_statuses:?}");
    }
    drop(conn);
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
    use contextunity_forge_mcp::db::{reader, writer};

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
    let facts: BTreeMap<String, Facts> = files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ast::extract(path, "javascript", source).unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
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

    let workspace = Workspace::new();
    for (path, source) in files {
        std::fs::write(workspace.join(path), source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let persisted_calls: i64 = conn
        .query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.mjs' AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='multiply' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='provider.cjs' AND dst.kind='function' AND dst.line=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        persisted_calls, 1,
        "persisted call must target the exported function"
    );
    let persisted_status: String = conn
        .query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.mjs' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='multiply'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted_status, "resolved");
    drop(conn);
}

#[test]
fn commonjs_default_function_expression_export_binds_esm_and_require_calls() {
    use contextunity_forge_mcp::db::{reader, writer};

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
    let facts: BTreeMap<String, Facts> = files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ast::extract(path, "javascript", source).unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
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

    let workspace = Workspace::new();
    for (path, source) in files {
        std::fs::write(workspace.join(path), source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let persisted_calls: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id) IN ('consumer.mjs', 'consumer.cjs') AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='increment' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='provider.cjs' AND dst.kind='function'",
        [],
        |row| row.get(0),
    ).unwrap();
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
        let guarded: BTreeMap<String, Facts> = [
            ("provider.cjs", provider_source),
            ("consumer.cjs", consumer_source),
        ].into_iter().map(|(path, source)| {
            (path.to_owned(), ast::extract(path, "javascript", source).unwrap())
        }).collect();
        let guarded_graph = linker::link(&guarded);
        assert!(guarded_graph.coverage.iter().any(|row| {
            row.path == "consumer.cjs" && row.expression == "increment"
                && matches!(row.status.as_str(), "unresolved" | "ambiguous")
        }), "source order and multiple default providers must fail closed: {guarded_graph:#?}");
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
    use contextunity_forge_mcp::{
        core::commitments,
        db::{reader, writer},
    };
    use std::{fs, path::PathBuf};

    let root = Workspace::new();
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
        connection.prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.path_id=(SELECT path_id FROM path_dictionary WHERE path='consumer.ts') AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>()
    };
    assert_eq!(targets(&delta), ["second.ts"]);
    let cold = root.join(".forge/cold.sqlite");
    writer::build(&root, &cold, None).unwrap();
    let cold = reader::open(&cold, &root).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(targets(&delta), targets(&cold));
    drop(delta);
    drop(cold);
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
fn commonjs_capture_keeps_linked_projects_isolated() {
    use contextunity_forge_mcp::db::{reader, writer};

    let base = Workspace::new();
    for name in ["first", "second"] {
        let root = base.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("provider.js"),
            "function work() {} module.exports = { work };",
        )
        .unwrap();
        std::fs::write(root.join("consumer.cjs"), "const local = require('./provider'); const config = { enabled: true }; function run() { local.work(); }").unwrap();
    }
    let linked = base.join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(linked.join("forge-mcp.yaml"), format!(
        "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
        base.join("first").display(), base.join("second").display()
    )).unwrap();
    let database = linked.join(".forge/code-map.sqlite");
    writer::build(&linked, &database, None).unwrap();
    let conn = reader::open(&database, &linked).unwrap();
    for name in ["first", "second"] {
        let owner = format!("[{name}]/consumer.cjs");
        let target = format!("[{name}]/provider.js");
        let edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='local.work' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND dst.name='work'",
            rusqlite::params![owner, target], |row| row.get(0),
        ).unwrap();
        assert_eq!(edges, 1, "CommonJS capture crossed linked project {name}");
        let all_edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='local.work'",
            rusqlite::params![owner], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            all_edges, 1,
            "CommonJS capture selected another project provider"
        );
    }
    drop(conn);
}

#[test]
fn typescript_package_exports_paths_respect_workspace_boundaries() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        ("package.json", r#"{"private":true,"workspaces":["components/*","apps/*"]}"#),
        ("components/tool/package.json", r#"{"name":"@suite/tool","exports":{"./entry":{"import":"./src/entry.ts"}}}"#),
        ("components/tool/src/entry.ts", "export function shared() {}"),
        ("packages/tool/package.json", r#"{"name":"@suite/tool","exports":{"./entry":"./src/decoy.ts"}}"#),
        ("packages/tool/src/decoy.ts", "export function shared() {}"),
        ("apps/one/package.json", r#"{"name":"one","dependencies":{"@suite/tool":"workspace:*"}}"#),
        ("apps/one/tsconfig.json", r#"{
          // Paths are local to this app.
          "compilerOptions": {"baseUrl": ".", "paths": {"@local/*": ["src/*",],},},
        }"#),
        ("apps/one/src/helper.ts", "export function helper() {}"),
        ("apps/one/alternative/helper.ts", "export function helper() {}"),
        ("apps/one/src/main.ts", "import { helper } from '@local/helper'; import { shared } from '@suite/tool/entry'; helper(); shared();"),
        ("apps/two/tsconfig.json", r#"{"compilerOptions":{"baseUrl":".","paths":{"@local/*":["src/*"]}}}"#),
        ("apps/two/src/helper.ts", "export function helper() {}"),
        ("apps/two/src/main.ts", "import { helper } from '@local/helper'; helper();"),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let call_targets = |path: &str| -> Vec<String> {
        conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
            .unwrap()
            .query_map([path], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        call_targets("apps/one/src/main.ts"),
        ["apps/one/src/helper.ts", "components/tool/src/entry.ts",]
    );
    assert_eq!(
        call_targets("apps/two/src/main.ts"),
        ["apps/two/src/helper.ts"]
    );
    drop(conn);
    std::fs::write(
        workspace.join("apps/one/tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@local/*":["alternative/*"]}}}"#,
    )
    .unwrap();
    writer::delta(
        &workspace,
        &database,
        &[std::path::PathBuf::from("apps/one/tsconfig.json")],
    )
    .unwrap();
    let delta = reader::open(&database, &workspace).unwrap();
    contextunity_forge_mcp::core::commitments::verify(&delta).unwrap();
    let changed_targets: Vec<String> = delta.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/one/src/main.ts' AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        changed_targets,
        [
            "apps/one/alternative/helper.ts",
            "components/tool/src/entry.ts"
        ]
    );
    drop(delta);
}

#[test]
fn package_export_wildcards_select_declared_workspace_and_specific_subpath() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        ("package.json", r#"{"private":true,"workspaces":["components/*","apps/*"]}"#),
        ("components/tool/package.json", r#"{"name":"@suite/tool","exports":{"./features/*":"./src/features/*.ts","./features/special/*":{"import":"./src/special/*.ts"}}}"#),
        ("components/tool/src/features/helper.ts", "export function feature() {}"),
        ("components/tool/src/features/special/action.ts", "export function special() {}"),
        ("components/tool/src/special/action.ts", "export function special() {}"),
        ("packages/tool/package.json", r#"{"name":"@suite/tool","exports":{"./features/*":"./src/decoy/*.ts"}}"#),
        ("packages/tool/src/decoy/helper.ts", "export function feature() {}"),
        ("apps/one/src/main.ts", "import { feature } from '@suite/tool/features/helper'; import { special } from '@suite/tool/features/special/action'; feature(); special();"),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let targets: Vec<String> = conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/one/src/main.ts' AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        targets,
        [
            "components/tool/src/features/helper.ts",
            "components/tool/src/special/action.ts"
        ]
    );
    drop(conn);
}

#[test]
fn tsconfig_extends_inherits_alias_origin_and_rebuilds_on_base_change() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        (
            "configs/tsconfig.base.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["../shared/*"]}}}"#,
        ),
        (
            "configs/tsconfig.app.json",
            r#"{"extends":"./tsconfig.base.json"}"#,
        ),
        (
            "apps/one/tsconfig.json",
            r#"{"extends":"../../configs/tsconfig.app.json"}"#,
        ),
        ("shared/helper.ts", "export function helper() {}"),
        ("shared/alternate/helper.ts", "export function helper() {}"),
        (
            "apps/one/src/main.ts",
            "import { helper } from '@shared/helper'; helper();",
        ),
        (
            "apps/two/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["src/*"]}}}"#,
        ),
        ("apps/two/src/helper.ts", "export function helper() {}"),
        (
            "apps/two/src/main.ts",
            "import { helper } from '@shared/helper'; helper();",
        ),
        (
            "apps/three/tsconfig.json",
            r#"{"extends":"../../configs/tsconfig.app.json","compilerOptions":{"baseUrl":"../../../outside","paths":{"@shared/*":["src/*"]}}}"#,
        ),
        (
            "apps/three/src/main.ts",
            "import { helper } from '@shared/helper'; helper();",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let targets = |path: &str| -> Vec<String> {
        let conn = reader::open(&database, &workspace).unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)").unwrap();
        let rows = stmt
            .query_map([path], |row| row.get::<_, String>(0))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(targets("apps/one/src/main.ts"), ["shared/helper.ts"]);
    assert_eq!(targets("apps/two/src/main.ts"), ["apps/two/src/helper.ts"]);
    let invalid = reader::open(&database, &workspace).unwrap();
    let statuses: Vec<String> = invalid.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='apps/three/src/main.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='helper'")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert!(
        !statuses.is_empty(),
        "invalid config must retain visible coverage"
    );
    assert!(
        statuses
            .iter()
            .all(|status| matches!(status.as_str(), "external" | "unresolved")),
        "escaping baseUrl cannot inherit a local provider: {statuses:?}"
    );
    drop(invalid);
    std::fs::write(
        workspace.join("configs/tsconfig.base.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["../shared/alternate/*"]}}}"#,
    )
    .unwrap();
    writer::delta(
        &workspace,
        &database,
        &[std::path::PathBuf::from("configs/tsconfig.base.json")],
    )
    .unwrap();
    assert_eq!(
        targets("apps/one/src/main.ts"),
        ["shared/alternate/helper.ts"]
    );
    assert_eq!(targets("apps/two/src/main.ts"), ["apps/two/src/helper.ts"]);
    let conn = reader::open(&database, &workspace).unwrap();
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn pnpm_workspace_globs_select_declared_package_exports_and_delta() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        (
            "package.json",
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - 'components/*'\n  - 'apps/*'\n  - '!components/excluded'\n",
        ),
        (
            "components/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/entry.ts"}}"#,
        ),
        (
            "components/tool/src/entry.ts",
            "export function shared() {}",
        ),
        (
            "components/excluded/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/excluded.ts"}}"#,
        ),
        (
            "components/excluded/src/excluded.ts",
            "export function shared() {}",
        ),
        (
            "packages/tool/package.json",
            r#"{"name":"@suite/tool","exports":{"./entry":"./src/decoy.ts"}}"#,
        ),
        ("packages/tool/src/decoy.ts", "export function shared() {}"),
        (
            "apps/one/package.json",
            r#"{"name":"one","dependencies":{"@suite/tool":"workspace:*"}}"#,
        ),
        (
            "apps/one/src/main.ts",
            "import { shared } from '@suite/tool/entry'; shared();",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let targets = || -> Vec<String> {
        let conn = reader::open(&database, &workspace).unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/one/src/main.ts' AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)").unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(targets(), ["components/tool/src/entry.ts"]);
    std::fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n  - 'apps/*'\n",
    )
    .unwrap();
    writer::delta(
        &workspace,
        &database,
        &[std::path::PathBuf::from("pnpm-workspace.yaml")],
    )
    .unwrap();
    assert_eq!(targets(), ["packages/tool/src/decoy.ts"]);
    let conn = reader::open(&database, &workspace).unwrap();
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn tsconfig_paths_choose_first_indexed_alternative_and_relink_on_new_file() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        (
            "apps/one/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["missing/*","src/*"]}}}"#,
        ),
        ("apps/one/src/thing.ts", "export function thing() {}"),
        ("apps/two/missing/thing.ts", "export function thing() {}"),
        (
            "apps/one/src/main.ts",
            "import { thing } from '@lib/thing'; thing();",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let targets = || -> Vec<String> {
        let conn = reader::open(&database, &workspace).unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/one/src/main.ts' AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)").unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(targets(), ["apps/one/src/thing.ts"]);
    std::fs::create_dir_all(workspace.join("apps/one/missing")).unwrap();
    std::fs::write(
        workspace.join("apps/one/missing/thing.ts"),
        "export function thing() {}",
    )
    .unwrap();
    writer::delta(
        &workspace,
        &database,
        &[std::path::PathBuf::from("apps/one/missing/thing.ts")],
    )
    .unwrap();
    assert_eq!(targets(), ["apps/one/missing/thing.ts"]);
    let conn = reader::open(&database, &workspace).unwrap();
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn type_only_imports_resolve_indexed_local_interface_provider() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    for (path, source) in [
        ("app/types.ts", "export interface ColumnStateItem { colId: string }"),
        ("app/consumer.ts", "import type { ColumnStateItem } from './types';\nexport function accept(value: ColumnStateItem): void {}\nColumnStateItem();"),
        ("other/types.ts", "export interface ColumnStateItem { decoy: boolean }"),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let statuses: Vec<String> = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/consumer.ts' AND line <= 2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='ColumnStateItem' ORDER BY line")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert!(
        !statuses.is_empty(),
        "type-only import must have persisted coverage"
    );
    assert!(
        statuses.iter().all(|status| status == "resolved"),
        "type-only import statuses: {statuses:?}"
    );
    let targets: Vec<String> = conn.prepare("SELECT DISTINCT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/consumer.ts' AND e.kind='imports' AND dst.name='ColumnStateItem' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(targets, ["app/types.ts"]);
    let invalid_call: String = conn.query_row("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/consumer.ts' AND line=3 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='ColumnStateItem'", [], |row| row.get(0)).unwrap();
    assert_eq!(invalid_call, "unresolved");
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}

#[test]
fn exported_enum_members_require_exact_indexed_provider() {
    let files = [
        ("app/colors.ts", "export enum Color { Red, Blue = 2 } const local = Color.Blue;"),
        ("app/use.ts", "import { Color } from './colors'; const selected = Color.Red; const missing = Color.Green; function shadow(Color) { return Color.Red; }"),
        ("app/call.ts", "import { Color } from './colors';\nColor.Red();"),
        ("app/type_use.ts", "import type { Color } from './colors';\ntype Choice = Color;\nconst invalid = Color.Red;\nconst bare = Color;\nfunction consume(value) {} consume(Color);"),
        ("other/colors.ts", "export enum Color { Green }"),
    ];
    let facts: BTreeMap<String, Facts> = files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ast::extract(path, "typescript", source).unwrap(),
            )
        })
        .collect();
    let provider = &facts["app/colors.ts"];
    assert!(
        provider
            .nodes
            .iter()
            .any(|node| node.kind == "enum" && node.name == "Color"),
        "{:#?}",
        provider.nodes
    );
    for name in ["Red", "Blue"] {
        assert!(
            provider.nodes.iter().any(|node| node.kind == "field"
                && node.name == name
                && node.qualname.ends_with(&format!("Color.{name}"))),
            "{name}: {:#?}",
            provider.nodes
        );
    }
    let graph = linker::link(&facts);
    assert!(
        graph.coverage.iter().any(|row| row.path == "app/colors.ts"
            && row.expression == "Color.Blue"
            && row.status == "resolved"),
        "{:#?}",
        graph.coverage
    );
    assert!(
        graph.coverage.iter().any(|row| row.path == "app/use.ts"
            && row.expression == "Color.Red"
            && row.status == "resolved"),
        "{:#?}",
        graph.coverage
    );
    assert!(
        graph.coverage.iter().any(|row| row.path == "app/use.ts"
            && row.expression == "Color.Green"
            && row.status == "unresolved"),
        "{:#?}",
        graph.coverage
    );
    assert!(
        graph.coverage.iter().any(|row| row.path == "app/use.ts"
            && row.expression == "Color.Red"
            && row.status == "unresolved"),
        "{:#?}",
        graph.coverage
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.path == "app/type_use.ts"
                && row.expression == "Color.Red"
                && row.status == "unresolved"),
        "{:#?}",
        graph.coverage
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.path == "app/type_use.ts"
                && row.line == 4
                && row.expression == "Color"
                && row.status == "unresolved"),
        "{:#?}",
        graph.coverage
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.path == "app/type_use.ts"
                && row.line == 5
                && row.expression == "Color"
                && row.status == "unresolved"),
        "{:#?}",
        graph.coverage
    );
    for line in [4, 5] {
        let runtime: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| {
                row.path == "app/type_use.ts" && row.line == line && row.expression == "Color"
            })
            .collect();
        assert!(
            !runtime.is_empty() && runtime.iter().all(|row| row.status == "unresolved"),
            "{runtime:#?}"
        );
        assert!(!graph.edges.iter().any(|edge| {
            edge.path == "app/type_use.ts"
                && edge.line == line
                && edge.kind == "references"
                && edge.evidence == "Color"
        }));
    }
    let selected: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.path == "app/use.ts" && edge.kind == "references" && edge.evidence == "Color.Red"
        })
        .collect();
    assert_eq!(selected.len(), 1, "{selected:#?}");
    assert!(provider
        .nodes
        .iter()
        .any(|node| node.id == selected[0].dst && node.name == "Red"));
    let callable: Vec<_> = graph
        .coverage
        .iter()
        .filter(|row| row.path == "app/call.ts" && row.line == 2 && row.expression == "Color.Red")
        .collect();
    assert!(
        !callable.is_empty() && callable.iter().all(|row| row.status == "unresolved"),
        "{callable:#?}"
    );
    assert!(!graph
        .edges
        .iter()
        .any(|edge| { edge.path == "app/call.ts" && edge.line == 2 && edge.kind == "calls" }));

    use contextunity_forge_mcp::db::{reader, writer};
    let workspace = Workspace::new();
    for (path, source) in files {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let mut stmt = conn.prepare("SELECT line,status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/type_use.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Color' AND line IN (4,5) ORDER BY line").unwrap();
    let persisted: Vec<(usize, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for line in [4, 5] {
        let rows: Vec<_> = persisted.iter().filter(|(at, _)| *at == line).collect();
        assert!(
            !rows.is_empty() && rows.iter().all(|(_, status)| status == "unresolved"),
            "{persisted:?}"
        );
    }
    let runtime_edges: usize = conn.query_row("SELECT COUNT(*) FROM edge_occurrences e WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/type_use.ts' AND e.line IN (4,5) AND e.kind='references'", [], |row| row.get(0)).unwrap();
    assert_eq!(runtime_edges, 0);
    let mut callable_stmt = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/call.ts' AND line=2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Color.Red'").unwrap();
    let callable_rows: Vec<String> = callable_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        !callable_rows.is_empty() && callable_rows.iter().all(|status| status == "unresolved"),
        "{callable_rows:?}"
    );
    let callable_edges: usize = conn.query_row("SELECT COUNT(*) FROM edge_occurrences e WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/call.ts' AND e.line=2 AND e.kind='calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(callable_edges, 0);
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(stmt);
    drop(callable_stmt);
    drop(conn);
}

#[test]
fn typescript_export_type_reexport_import_targets() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    std::fs::write(
        workspace.join("types.ts"),
        "export type Foo = { id: string };\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("generated.ts"),
        "export interface JsonValue { value: string; }\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("barrel.ts"),
        r#"
import type { JsonValue } from "./generated";
export type { JsonValue };
export type { Foo } from "./types";
export type { Outside } from "outside-package";
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.join("consumer.ts"),
        r#"
import type { Foo, JsonValue, Outside } from "./barrel";
function test(f: Foo, j: JsonValue, o: Outside) {}
"#,
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();

    let coverage = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| row.get(0),
        ).unwrap()
    };

    assert_eq!(coverage("consumer.ts", "Foo"), "resolved");
    assert_eq!(coverage("consumer.ts", "JsonValue"), "resolved");
    assert_eq!(coverage("consumer.ts", "Outside"), "unresolved");

    let edge_dst = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)=?2",
            rusqlite::params![path, expression],
            |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(edge_dst("consumer.ts", "Foo"), "types.ts");
    assert_eq!(edge_dst("consumer.ts", "JsonValue"), "generated.ts");

    drop(conn);
}

#[test]
fn export_type_keeps_classes_and_value_export_drops_type_aliases() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
    std::fs::write(
        workspace.join("box.ts"),
        "export class Box { id = 1; }\nexport enum Color { Red = 1 }\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("barrel.ts"),
        "export type { Box, Color } from \"./box\";\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("only.ts"),
        "export type Foo = { id: string };\nexport { Foo };\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("consumer.ts"),
        "import type { Box, Color } from \"./barrel\";\nfunction use(box: Box, color: Color) {}\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("use.js"),
        "import { Foo } from \"./only\";\nFoo();\n",
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            rusqlite::params![expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(coverage("Box"), "resolved");
    assert_eq!(coverage("Color"), "resolved");
    let foo: String = conn
        .query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='use.js' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Foo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(foo, "unresolved");
    let box_dst: String = conn
        .query_row(
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.ts' AND e.kind='references' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)='Box'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(box_dst, "box.ts");
    drop(conn);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_typescript_mjs_cjs_and_package_exports() {
    let w = Workspace::new();
    w.write(
        "packages/math/package.json",
        r#"{
  "name": "@acme/math",
  "exports": {
    ".": "./src/index.mjs",
    "./calculator": "./src/calc.cjs"
  }
}
"#,
    );
    w.write(
        "packages/math/src/index.mjs",
        r#"export function add(a, b) {
    return a + b;
}
"#,
    );
    w.write(
        "packages/math/src/calc.cjs",
        r#"exports.multiply = function(a, b) {
    return a * b;
};
"#,
    );
    w.write(
        "packages/app/src/main.ts",
        r#"import { add } from "@acme/math";
import { multiply } from "@acme/math/calculator";

export function run() {
    return add(2, 3);
}
"#,
    );

    let conn = w.build();

    let modules: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT n.id, n.qualname, p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='module'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("MODULES: {:?}", modules);
    let cov: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id), status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("COVERAGE: {:?}", cov);

    // Verify .mjs and .cjs were scanned and extracted
    let mjs_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE path LIKE '%.mjs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mjs_count, 1);

    let cjs_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE path LIKE '%.cjs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cjs_count, 1);

    // Verify import edges resolved via package.json exports
    let import_edges: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='imports'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert!(
        import_edges
            .iter()
            .any(|(_, dst)| dst.contains("index.mjs")),
        "expected import edge to index.mjs: {:?}",
        import_edges
    );
    assert!(
        import_edges.iter().any(|(_, dst)| dst.contains("calc.cjs")),
        "expected import edge to calc.cjs: {:?}",
        import_edges
    );
}
