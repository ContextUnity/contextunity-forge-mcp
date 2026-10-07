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
fn javascript_local_imports_require_indexed_project_targets() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let base = std::env::temp_dir().join(format!(
        "forge_mjs_typescript_import_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(base).unwrap();
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    let workspace = std::env::temp_dir().join(format!(
        "forge_commonjs_function_export_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn commonjs_default_function_expression_export_binds_esm_and_require_calls() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

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

    let workspace = std::env::temp_dir().join(format!(
        "forge_commonjs_default_function_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();

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
fn node_and_web_callable_globals_have_external_origin() {
    let source = r#"function run() {
        Buffer.from('data');
        console.log('data');
        setTimeout(() => {}, 0);
        fetch('/api');
    }"#;
    let facts = ast::extract("runtime.js", "javascript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("runtime.js".to_owned(), facts)]));
    for (expression, origin) in [
        ("Buffer.from", "builtin:node"),
        ("console.log", "builtin:web_api"),
        ("setTimeout", "JavaScript/Web platform"),
        ("fetch", "builtin:web_api"),
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing {expression}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains(origin),
            "{expression}: {coverage:#?}"
        );
    }
}

#[test]
fn node_process_environment_has_external_origin() {
    let source = "function run() { const environment = process.env; return environment; }";
    let facts = ast::extract("runtime.js", "javascript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("runtime.js".to_owned(), facts)]));
    let coverage = graph
        .coverage
        .iter()
        .find(|row| row.expression == "process.env")
        .unwrap_or_else(|| panic!("missing process.env: {graph:#?}"));
    assert_eq!(coverage.status, "external", "{coverage:#?}");
    assert!(coverage.evidence.contains("builtin:node"), "{coverage:#?}");

    let source = "function run(process) { return process.env; }";
    let facts = ast::extract("shadow.js", "javascript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("shadow.js".to_owned(), facts)]));
    let coverage = graph
        .coverage
        .iter()
        .find(|row| row.expression == "process.env")
        .unwrap_or_else(|| panic!("missing shadowed process.env: {graph:#?}"));
    assert_eq!(coverage.status, "unresolved", "{coverage:#?}");
}

#[test]
fn web_constructors_have_external_origin() {
    let source = r#"function run() {
        new URL('/api', 'https://example.test');
        new Response('ok');
        new Request('https://example.test');
        new Headers();
    }"#;
    let facts = ast::extract("web.js", "javascript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("web.js".to_owned(), facts)]));
    for expression in ["URL", "Response", "Request", "Headers"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing {expression}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("builtin:web_api"),
            "{expression}: {coverage:#?}"
        );
    }

    for (source, expected_status, local_target) in [
        (
            "class URL { constructor(value) {} } function run() { new URL('/api'); }",
            "resolved",
            true,
        ),
        (
            "function run(URL) { new URL('/api'); }",
            "unresolved",
            false,
        ),
    ] {
        let facts = ast::extract("shadow.js", "javascript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("shadow.js".to_owned(), facts.clone())]));
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == "URL")
            .unwrap_or_else(|| panic!("missing shadowed URL: {graph:#?}"));
        assert_eq!(coverage.status, expected_status, "{source}: {coverage:#?}");
        if local_target {
            assert!(graph.edges.iter().any(|edge| {
                edge.kind == "calls"
                    && edge.evidence == "URL"
                    && facts.nodes.iter().any(|node| {
                        node.id == edge.dst && node.kind == "class" && node.name == "URL"
                    })
            }));
        }
    }
}

#[test]
fn required_node_modules_have_import_provenance() {
    for (module, binding, call, arguments) in [
        ("node:fs", "fs", "fs.readFileSync", "'file.txt', 'utf8'"),
        ("node:path", "path", "path.join", "'a', 'b'"),
    ] {
        let source = format!(
            "const {binding} = require('{module}'); {binding}.{member}({arguments});",
            member = call.rsplit('.').next().unwrap()
        );
        let facts = ast::extract("runtime.cjs", "javascript", &source).unwrap();
        assert!(facts.errors.is_empty(), "{source}: {:?}", facts.errors);
        let graph = linker::link(&BTreeMap::from([("runtime.cjs".to_owned(), facts)]));
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == call)
            .unwrap_or_else(|| panic!("missing {call}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{call}: {coverage:#?}");
        assert!(coverage.evidence.contains(module), "{call}: {coverage:#?}");
    }
}

#[test]
fn javascript_named_node_builtin_call_keeps_import_origin_and_shadow_boundary() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_node_named_builtin_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("runner.cjs"),
        "const { test } = require('node:test');\ntest('case', () => {});\nfunction run(test) { test(); }\n",
    ).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |line: i64| -> (String, String) {
        conn.query_row(
            "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=c.evidence_id) FROM resolution_coverage c WHERE (SELECT path FROM path_dictionary WHERE path_id=c.path_id)='runner.cjs' AND c.expression_id=(SELECT expression_id FROM coverage_expressions WHERE expression='test') AND c.line=?1",
            [line],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };
    let imported = coverage(2);
    assert_eq!(imported.0, "external", "named Node import: {imported:?}");
    assert!(
        imported.1.contains("builtin:node import node:test"),
        "named Node import: {imported:?}"
    );
    let shadowed = coverage(3);
    assert_eq!(
        shadowed.0, "unresolved",
        "shadowed local parameter: {shadowed:?}"
    );
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
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
            "external",
        ),
        (
            "function run() { Object.assign({}, {}); JSON.parse('{}'); }",
            "JSON.parse",
            "external",
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
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, expected);
        if expected == "external" {
            assert!(coverage.evidence.contains("JavaScript/Web platform"));
        }
    }
}

#[test]
fn conversion_globals_are_external_with_local_precedence() {
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
                        && coverage.status == "external"
                        && coverage.evidence.contains("JavaScript/Web platform")
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
            "external",
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
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, expected);
        if expected == "external" {
            assert!(coverage.evidence.contains("JavaScript/Web platform"));
        }
        if source.starts_with("function") {
            assert!(!graph
                .edges
                .iter()
                .any(|edge| edge.kind == "calls" && edge.evidence == expression));
        }
    }
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
fn commonjs_and_dom_dispatch_preserves_callback_provenance_in_both_orders() {
    for (path, language) in [("listener.js", "javascript"), ("listener.ts", "typescript")] {
        for (listener, expected) in [
            ("document.addEventListener('click', event => event.preventDefault());", "external"),
            ("function run(document) { document.addEventListener('click', event => event.preventDefault()); }", "unresolved"),
        ] {
            for source in [format!("require('fs'); {listener}"), format!("{listener} require('fs');")] {
                let facts = ast::extract(path, language, &source).unwrap();
                let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
                let rows: Vec<_> = graph.coverage.iter().filter(|row| row.expression == "event.preventDefault").collect();
                assert!(!rows.is_empty(), "{path}: {source}");
                assert!(rows.iter().all(|row| row.status == expected), "{path}: {source}: {rows:#?}");
            }
        }
    }
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
fn finite_browser_globals_are_external_in_javascript_and_typescript() {
    let expressions = "fetch alert confirm prompt document.querySelector document.querySelectorAll document.getElementById document.getElementsByClassName document.getElementsByTagName document.createElement document.createTextNode document.addEventListener document.removeEventListener window.addEventListener window.removeEventListener window.dispatchEvent window.setTimeout window.clearTimeout window.setInterval window.clearInterval window.requestAnimationFrame window.cancelAnimationFrame localStorage.getItem localStorage.setItem localStorage.removeItem localStorage.clear sessionStorage.getItem sessionStorage.setItem sessionStorage.removeItem sessionStorage.clear console.debug console.info console.trace console.table URL FormData HTMLElement";
    let source = r#"function run() { const handler = () => {};
        fetch('/api'); alert('message'); confirm('message'); prompt('message');
        document.querySelector('div'); document.querySelectorAll('div'); document.getElementById('item'); document.getElementsByClassName('item'); document.getElementsByTagName('div');
        document.createElement('div'); document.createTextNode('text'); document.addEventListener('click', handler); document.removeEventListener('click', handler);
        window.addEventListener('click', handler); window.removeEventListener('click', handler); window.dispatchEvent(new Event('change'));
        window.setTimeout(handler, 0); window.clearTimeout(1); window.setInterval(handler, 0); window.clearInterval(1); window.requestAnimationFrame(handler); window.cancelAnimationFrame(1);
        localStorage.getItem('key'); localStorage.setItem('key', 'value'); localStorage.removeItem('key'); localStorage.clear();
        sessionStorage.getItem('key'); sessionStorage.setItem('key', 'value'); sessionStorage.removeItem('key'); sessionStorage.clear();
        console.debug('message'); console.info('message'); console.trace('message'); console.table([]);
        URL('/path', 'https://example.test'); FormData(); HTMLElement();
    }"#;
    for (path, language) in [("browser.ts", "typescript"), ("browser.js", "javascript")] {
        let facts = ast::extract(path, language, source).unwrap();
        assert!(facts.errors.is_empty(), "{facts:#?}");
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
        for expression in expressions.split_whitespace() {
            assert!(
                graph.coverage.iter().any(|row| row.expression == expression
                    && row.status == "external"
                    && row.evidence.contains(
                        if matches!(
                            expression.split('.').next(),
                            Some(
                                "document"
                                    | "window"
                                    | "localStorage"
                                    | "sessionStorage"
                                    | "fetch"
                                    | "FormData"
                                    | "HTMLElement"
                                    | "URL"
                            )
                        ) || (language == "javascript" && expression.starts_with("console."))
                        {
                            "builtin:web_api"
                        } else {
                            "JavaScript/Web platform"
                        }
                    )),
                "{language}: {expression}"
            );
        }
    }
}

#[test]
fn web_and_dom_builtins_resolve_with_external_provenance() {
    let source = r#"function run() {
        document.querySelector('button'); document.getElementById('button');
        window.addEventListener('click', () => {});
        fetch('/api'); new Headers(); new Request('/api'); new Response();
    }"#;
    let cases = [
        ("document.querySelector", "builtin:web_api"),
        ("document.getElementById", "builtin:web_api"),
        ("window.addEventListener", "builtin:web_api"),
        ("fetch", "builtin:web_api"),
        ("Headers", "builtin:web_api"),
        ("Request", "builtin:web_api"),
        ("Response", "builtin:web_api"),
    ];
    for (path, language) in [("web.ts", "typescript"), ("web.js", "javascript")] {
        let facts = ast::extract(path, language, source).unwrap();
        assert!(facts.errors.is_empty(), "{path}: {facts:#?}");
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
        for (expression, origin) in cases {
            assert!(
                graph.coverage.iter().any(|row| row.expression == expression
                    && row.status == "external"
                    && row.evidence.contains(origin)),
                "{path}: {expression}: {graph:#?}"
            );
        }
    }

    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};
    let workspace = std::env::temp_dir().join(format!(
        "forge_web_builtins_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("web.ts"), source).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    for (expression, origin) in [
        ("document.querySelector", "builtin:web_api"),
        ("fetch", "builtin:web_api"),
    ] {
        let row: (String, String) = conn.query_row(
            "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='web.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            [expression], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(row.0, "external", "persisted {expression}: {row:?}");
        assert!(row.1.contains(origin), "persisted {expression}: {row:?}");
    }
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
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
    use std::time::{SystemTime, UNIX_EPOCH};
    let workspace = std::env::temp_dir().join(format!(
        "forge_returned_object_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
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
    use std::time::{SystemTime, UNIX_EPOCH};

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
    let workspace = std::env::temp_dir().join(format!(
        "forge_frozen_factory_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typescript_optional_call_keeps_its_persisted_expression() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_typescript_optional_call_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn commonjs_capture_keeps_linked_projects_isolated() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let base = std::env::temp_dir().join(format!(
        "forge_commonjs_linked_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn dom_factory_return_receivers() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_dom_factory_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    for (path, source) in [
        ("factory.ts", "function run() { const button = document.createElement('button'); button.addEventListener('click', () => {}); const alias = button; alias.addEventListener('click', () => {}); const root = document.querySelector('#root'); if (root) root.querySelector('.child'); const found = document.getElementById('root'); if (found) found.getAttribute('id'); button.unknownMethod(); }"),
        ("shadow.ts", "function run(document) { const button = document.createElement('button'); button.addEventListener('click', () => {}); }"),
        ("reassigned_document.ts", "function run() { document = {}; const button = document.createElement('button'); button.addEventListener('click', () => {}); }"),
        ("reassigned_result.ts", "function run() { let button = document.createElement('button'); button = {}; button.addEventListener('click', () => {}); }"),
        ("shadow.js", "function run(document) { document.addEventListener('click', event => event.preventDefault()); }"),
        ("unknown_factory.ts", "function run() { const button = document.makeElement('button'); button.addEventListener('click', () => {}); }"),
        ("untyped.ts", "function run() { const button = {}; button.addEventListener('click', () => {}); }"),
        ("local.ts", "class Element { addEventListener() {} } function run() { const button = new Element(); button.addEventListener('click'); }"),
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
    for (path, expression) in [
        ("factory.ts", "button.addEventListener"),
        ("factory.ts", "alias.addEventListener"),
        ("factory.ts", "root.querySelector"),
        ("factory.ts", "found.getAttribute"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(row.0, "external", "{path}:{expression} {row:?}");
        assert!(
            row.1.contains("builtin:web_api"),
            "{path}:{expression} {row:?}"
        );
    }
    for (path, expression) in [
        ("factory.ts", "button.unknownMethod"),
        ("shadow.ts", "button.addEventListener"),
        ("reassigned_document.ts", "button.addEventListener"),
        ("reassigned_result.ts", "button.addEventListener"),
        ("unknown_factory.ts", "button.addEventListener"),
        ("untyped.ts", "button.addEventListener"),
        ("shadow.js", "document.addEventListener"),
        ("shadow.js", "event.preventDefault"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(row.0, "unresolved", "{path}:{expression} {row:?}");
    }
    assert_eq!(
        coverage("local.ts", "button.addEventListener").0,
        "resolved"
    );
    let local_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='local.ts' AND e.kind='calls' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='local.ts' AND dst.kind='method' AND dst.name='addEventListener'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        local_edge, 1,
        "local Element method keeps its indexed provider"
    );
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typed_dom_receiver_members() {
    let source = "function run(root: Element, event: Event, doc: Document) { root.querySelectorAll('.item'); event.stopPropagation(); doc.querySelectorAll('.item'); root.unknownMember(); doc.closest('.item'); }";
    let facts = ast::extract("dom.ts", "typescript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("dom.ts".to_owned(), facts)]));
    for expression in [
        "root.querySelectorAll",
        "event.stopPropagation",
        "doc.querySelectorAll",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing {expression}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("builtin:web_api"),
            "{expression}: {coverage:#?}"
        );
    }
    for expression in ["root.unknownMember", "doc.closest"] {
        let unknown = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing unsupported {expression}: {graph:#?}"));
        assert_eq!(unknown.status, "unresolved", "{expression}: {unknown:#?}");
    }

    let source =
        "function run(root, event) { root.querySelectorAll('.item'); event.stopPropagation(); }";
    let facts = ast::extract("untyped.ts", "typescript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("untyped.ts".to_owned(), facts)]));
    for expression in ["root.querySelectorAll", "event.stopPropagation"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing untyped {expression}: {graph:#?}"));
        assert_eq!(coverage.status, "unresolved", "{expression}: {coverage:#?}");
    }

    let source = "class Element { querySelectorAll() {} } function run(root: Element) { root.querySelectorAll(); }";
    let facts = ast::extract("local.ts", "typescript", source).unwrap();
    let files = BTreeMap::from([("local.ts".to_owned(), facts)]);
    let graph = linker::link(&files);
    let coverage = graph
        .coverage
        .iter()
        .find(|row| row.expression == "root.querySelectorAll")
        .unwrap_or_else(|| panic!("missing local Element method: {graph:#?}"));
    assert_eq!(coverage.status, "resolved", "{coverage:#?}");
    assert!(
        graph.edges.iter().any(|edge| {
            edge.kind == "calls"
                && edge.evidence == "root.querySelectorAll"
                && files["local.ts"].nodes.iter().any(|node| {
                    node.id == edge.dst && node.kind == "method" && node.name == "querySelectorAll"
                })
        }),
        "local Element method call must retain its indexed provider: {graph:#?}"
    );

    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};
    let workspace = std::env::temp_dir().join(format!(
        "forge_typed_dom_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    for (path, source) in [
        ("dom.ts", "function run(root: Element, event: Event, doc: Document) { root.querySelectorAll('.item'); event.stopPropagation(); doc.querySelectorAll('.item'); root.unknownMember(); doc.closest('.item'); }"),
        ("untyped.ts", "function run(root, event) { root.querySelectorAll('.item'); event.stopPropagation(); }"),
        ("local.ts", "class Element { querySelectorAll() {} } function run(root: Element) { root.querySelectorAll(); }"),
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
    for expression in [
        "root.querySelectorAll",
        "event.stopPropagation",
        "doc.querySelectorAll",
    ] {
        let row = coverage("dom.ts", expression);
        assert_eq!(row.0, "external", "persisted {expression}: {row:?}");
        assert!(
            row.1.contains("builtin:web_api"),
            "persisted {expression}: {row:?}"
        );
    }
    for (path, expression) in [
        ("dom.ts", "root.unknownMember"),
        ("dom.ts", "doc.closest"),
        ("untyped.ts", "root.querySelectorAll"),
        ("untyped.ts", "event.stopPropagation"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(
            row.0, "unresolved",
            "persisted {path}:{expression}: {row:?}"
        );
    }
    let local = coverage("local.ts", "root.querySelectorAll");
    assert_eq!(
        local.0, "resolved",
        "persisted local Element method: {local:?}"
    );
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn nested_dom_members_follow_verified_types_and_optional_access() {
    let source = r#"
        function root(): Element { return document.getElementById('root')!; }
        function run(element: Element, doc: Document, win: Window) {
            element.parentElement?.querySelector('.item');
            element.ownerDocument?.querySelector('.item');
            doc.documentElement?.getAttribute('id');
            win.document?.getElementById('root');
            const fromReturn = root();
            fromReturn.parentElement?.matches('.item');
            element.parentElement?.invented();
            unknown.parentElement?.querySelector('.item');
        }
    "#;
    let facts = ast::extract("nested.ts", "typescript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("nested.ts".to_owned(), facts)]));
    for expression in [
        "element.parentElement?.querySelector",
        "element.ownerDocument?.querySelector",
        "doc.documentElement?.getAttribute",
        "win.document?.getElementById",
        "fromReturn.parentElement?.matches",
    ] {
        let row = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing {expression}: {graph:#?}"));
        assert_eq!(row.status, "external", "{expression}: {row:#?}");
        assert!(
            row.evidence.contains("builtin:web_api"),
            "{expression}: {row:#?}"
        );
    }
    for expression in [
        "element.parentElement?.invented",
        "unknown.parentElement?.querySelector",
    ] {
        let row = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing {expression}: {graph:#?}"));
        assert_eq!(row.status, "unresolved", "{expression}: {row:#?}");
    }

    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};
    let workspace = std::env::temp_dir().join(format!(
        "forge_nested_dom_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("nested.ts"), source).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let persisted = |expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='nested.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            [expression], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };
    for expression in [
        "element.parentElement?.querySelector",
        "element.ownerDocument?.querySelector",
        "doc.documentElement?.getAttribute",
        "win.document?.getElementById",
        "fromReturn.parentElement?.matches",
    ] {
        let row = persisted(expression);
        assert_eq!(row.0, "external", "persisted {expression}: {row:?}");
        assert!(
            row.1.contains("builtin:web_api"),
            "persisted {expression}: {row:?}"
        );
    }
    for expression in [
        "element.parentElement?.invented",
        "unknown.parentElement?.querySelector",
    ] {
        let row = persisted(expression);
        assert_eq!(row.0, "unresolved", "persisted {expression}: {row:?}");
    }
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typescript_package_exports_paths_respect_workspace_boundaries() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_package_paths_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn package_export_wildcards_select_declared_workspace_and_specific_subpath() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_export_wildcards_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn tsconfig_extends_inherits_alias_origin_and_rebuilds_on_base_change() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_extends_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn pnpm_workspace_globs_select_declared_package_exports_and_delta() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_pnpm_workspace_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn tsconfig_paths_choose_first_indexed_alternative_and_relink_on_new_file() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_paths_fallback_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn html_table_element_type_and_members_use_verified_dom_origin() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_html_table_type_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (path, source) in [
        ("table.ts", "function inspect(table: HTMLTableElement) { table.querySelectorAll('tr'); table.unknownMember(); }"),
        ("local.ts", "class HTMLTableElement { querySelectorAll() {} } function inspect(table: HTMLTableElement) { table.querySelectorAll(); }"),
        ("untyped.ts", "function inspect(table) { table.querySelectorAll('tr'); }"),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |path: &str, expression: &str| -> Vec<(String, String)> {
        let mut stmt = conn.prepare("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2").unwrap();
        let rows = stmt
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    for expression in ["HTMLTableElement", "table.querySelectorAll"] {
        let rows = coverage("table.ts", expression);
        assert!(
            rows.iter()
                .any(|(status, evidence)| status == "external"
                    && evidence.contains("builtin:web_api")),
            "{expression}: {rows:?}"
        );
    }
    let unknown = coverage("table.ts", "table.unknownMember");
    assert!(
        !unknown.is_empty() && unknown.iter().all(|(status, _)| status == "unresolved"),
        "{unknown:?}"
    );
    assert!(coverage("local.ts", "table.querySelectorAll")
        .iter()
        .any(|(status, _)| status == "resolved"));
    let untyped = coverage("untyped.ts", "table.querySelectorAll");
    assert!(
        !untyped.is_empty() && untyped.iter().all(|(status, _)| status == "unresolved"),
        "{untyped:?}"
    );
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn type_only_imports_resolve_indexed_local_interface_provider() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_type_only_import_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn nuxt_autoimports_require_declared_project_and_preserve_local_shadow() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_nuxt_autoimports_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (path, source) in [
        (
            "nuxt/package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        (
            "nuxt/nuxt.config.ts",
            "export default defineNuxtConfig({});",
        ),
        (
            "nuxt/src/auto.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
        (
            "nuxt/src/local.ts",
            "function computed() {} function ref() {} computed(); ref();",
        ),
        ("plain/package.json", r#"{"dependencies":{"vue":"3.5.0"}}"#),
        (
            "plain/src/plain.ts",
            "const value = computed(() => 1); const state = ref(0);",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |path: &str, expression: &str| -> Vec<(String, String)> {
        let mut stmt = conn.prepare("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2").unwrap();
        let rows = stmt
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    for expression in ["computed", "ref"] {
        let auto = coverage("nuxt/src/auto.ts", expression);
        assert!(
            auto.iter().any(|(status, evidence)| status == "external"
                && evidence.contains("builtin:nuxt_autoimport")),
            "Nuxt {expression}: {auto:?}"
        );
        assert!(
            coverage("nuxt/src/local.ts", expression)
                .iter()
                .any(|(status, _)| status == "resolved"),
            "local {expression}"
        );
        let plain = coverage("plain/src/plain.ts", expression);
        assert!(
            !plain.is_empty() && plain.iter().all(|(status, _)| status == "unresolved"),
            "plain {expression}: {plain:?}"
        );
    }
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn nuxt_components_import_links_tsx_to_vue_only_for_declared_nuxt() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_nuxt_components_import_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (path, source) in [
        (
            "nuxt/package.json",
            r#"{"dependencies":{"nuxt":"3.21.9","vue":"3.5.0"}}"#,
        ),
        (
            "nuxt/nuxt.config.ts",
            "export default defineNuxtConfig({});",
        ),
        (
            "nuxt/components/base/WidgetCard.vue",
            "<template><article>Card</article></template>",
        ),
        (
            "nuxt/src/page.tsx",
            "import { BaseWidgetCard } from '#components';\n",
        ),
        ("plain/package.json", r#"{"dependencies":{"vue":"3.5.0"}}"#),
        (
            "plain/components/base/WidgetCard.vue",
            "<template><article>Card</article></template>",
        ),
        (
            "plain/src/page.vue",
            "<template><BaseWidgetCard /></template>",
        ),
    ] {
        let target = workspace.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, source).unwrap();
    }
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let statuses = |path: &str| -> Vec<String> {
        conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='BaseWidgetCard' ORDER BY line,status")
            .unwrap()
            .query_map([path], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let nuxt_import_targets: Vec<String> = conn
        .prepare("SELECT DISTINCT target_path.path FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN path_dictionary owner ON owner.path_id=e.owner_id JOIN path_dictionary target_path ON target_path.path_id=dst.path_id WHERE owner.path='nuxt/src/page.tsx' AND e.kind='imports' ORDER BY target_path.path")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let nuxt_statuses = statuses("nuxt/src/page.tsx");
    let plain_statuses = statuses("plain/src/page.vue");
    let expected_target = ["nuxt/components/base/WidgetCard.vue"];
    assert!(
        nuxt_statuses.iter().any(|status| status == "resolved")
            && nuxt_import_targets == expected_target
            && plain_statuses == ["unresolved"],
        "#components must resolve through the declared Nuxt linker while a plain Vue tag stays unresolved; nuxt statuses={nuxt_statuses:?}, import targets={nuxt_import_targets:?}, plain statuses={plain_statuses:?}"
    );
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn browser_names_preserve_local_providers_and_unknown_parameter_boundaries() {
    assert_eq!(
        calls(
            &[(
                "local.ts",
                "function fetch() {} function run() { fetch(); }"
            )],
            "fetch"
        ),
        ["local.ts"]
    );
    assert_eq!(calls(&[("local.ts", "class Document { querySelector() {} } function run(document: Document) { document.querySelector(); }")], "document.querySelector"), ["local.ts"]);
    let facts = ast::extract("shadow.ts", "typescript", "function run(fetch, document, window, localStorage, sessionStorage, URL, FormData, HTMLElement) { fetch('/api'); document.querySelector('div'); window.addEventListener('click', handler); localStorage.getItem('key'); sessionStorage.clear(); URL('/api'); FormData(); HTMLElement(); }").unwrap();
    let graph = linker::link(&BTreeMap::from([("shadow.ts".to_owned(), facts)]));
    for expression in [
        "fetch",
        "document.querySelector",
        "window.addEventListener",
        "localStorage.getItem",
        "sessionStorage.clear",
        "URL",
        "FormData",
        "HTMLElement",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == expression && row.status == "unresolved"),
            "{expression}"
        );
    }
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
    use std::time::{SystemTime, UNIX_EPOCH};
    let workspace = std::env::temp_dir().join(format!(
        "forge_typescript_enum_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typed_array_callback_inherits_unique_element_provider() {
    for (array_type, prefix, expected) in [
        ("Worker[]", "", "resolved"),
        ("Array<Worker>", "", "resolved"),
        ("Array<Worker>", "class Array<T> {}", "unresolved"),
        (
            "Array<Worker>",
            "type Array<T> = { value: T };",
            "unresolved",
        ),
        (
            "Array<Worker>",
            "import { Array } from './custom';",
            "unresolved",
        ),
        ("Box<Worker>", "class Box<T> {}", "unresolved"),
    ] {
        let source = format!("{prefix} class Worker {{ work() {{}} }} function run(items: {array_type}) {{ items.forEach(item => item.work()); items.map(item => item.missing()); }}");
        let facts = ast::extract("callback.ts", "typescript", &source).unwrap();
        let graph = linker::link(&BTreeMap::from([("callback.ts".to_owned(), facts)]));
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "item.work" && row.status == expected),
            "{array_type} {prefix}: {graph:#?}"
        );
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.evidence == "item.work")
                .count(),
            usize::from(expected == "resolved"),
            "{array_type} {prefix}: {graph:#?}"
        );
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "item.missing" && row.status == "unresolved"),
            "{array_type} {prefix}: {graph:#?}"
        );
    }
    for mutation in ["items = unknown();", "if (flag) items = unknown();"] {
        let source = format!("class Worker {{ work() {{}} }} function run(items: Array<Worker>, flag: boolean) {{ {mutation} items.map(item => item.work()); }}");
        let facts = ast::extract("rebound.ts", "typescript", &source).unwrap();
        let graph = linker::link(&BTreeMap::from([("rebound.ts".to_owned(), facts)]));
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "item.work" && row.status == "unresolved"),
            "{mutation}: {graph:#?}"
        );
    }
    let spaced_reassignment = "class Worker { work() {} } function run(items: Array<Worker>) { items = unknown(); items . map(item => item.work()); }";
    let facts = ast::extract("spaced-rebound.ts", "typescript", spaced_reassignment).unwrap();
    let graph = linker::link(&BTreeMap::from([("spaced-rebound.ts".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.expression == "item.work" && row.status == "unresolved"),
        "spaced member access: {graph:#?}"
    );
    for (path, source) in [
        (
            "unicode-space-rebound.ts",
            "class Worker { work() {} } function run(items: Worker[]) { items = unknown(); items.map\u{00a0}(item => item.work()); } // require",
        ),
        (
            "unicode-bom-rebound.ts",
            "class Worker { work() {} } function run(items: Worker[]) { items = unknown(); items.map\u{feff}(item => item.work()); } // require",
        ),
        (
            "unicode-word-joiner-rebound.ts",
            "class Worker { work() {} } function run(items: Worker[]) { items = unknown(); items.map\u{2060}(item => item.work()); } // require",
        ),
        (
            "unicode-zero-width-space-rebound.ts",
            "class Worker { work() {} } function run(items: Worker[]) { items = unknown(); items.map\u{200b}(item => item.work()); } // require",
        ),
        (
            "unicode-line-separator-rebound.ts",
            "class Worker { work() {} } function run(items: Worker[]) { items = unknown(); items.// gap\u{2028}map(item => item.work()); } // require",
        ),
        (
            "unicode-paragraph-separator-rebound.ts",
            "class Worker { work() {} } function run(items: Worker[]) { items = unknown(); items.// gap\u{2029}map(item => item.work()); } // require",
        ),
    ] {
        let facts = ast::extract(path, "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "item.work" && row.status == "unresolved"),
            "{path}: Unicode grammar trivia must preserve array callback rebound facts: {graph:#?}"
        );
        assert!(!graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == "item.work"),
            "{path}: stale Worker[] inference must not produce a call edge"
        );
    }
    let spaced_shadow = "class Worker { work() {} } class Array<T> {} function run(items: Array<Worker>) { items . map(item => item.work()); }";
    let facts = ast::extract("spaced-shadow.ts", "typescript", spaced_shadow).unwrap();
    let graph = linker::link(&BTreeMap::from([("spaced-shadow.ts".to_owned(), facts)]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.expression == "item.work" && row.status == "unresolved"),
        "spaced member access with a local Array binding: {graph:#?}"
    );
}

#[test]
fn array_callback_dom_inference_respects_shadowed_document() {
    let cases = [
        (
            "function run(document: unknown) { document.querySelectorAll('x').forEach(e => e.querySelector('.x')); }",
            "unresolved",
        ),
        (
            "function run() { document.querySelectorAll('x').forEach(e => e.querySelector('.x')); }",
            "external",
        ),
        (
            "function run(root: Element) { root.querySelectorAll('x').forEach(e => e.querySelector('.x')); }",
            "external",
        ),
        (
            "function run(root: Element) { root.\u{00a0}querySelectorAll('x').forEach(e => e.querySelector('.x')); }",
            "external",
        ),
    ];

    for (source, expected) in cases {
        let facts = ast::extract("callback-dom.ts", "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("callback-dom.ts".to_owned(), facts)]));
        let rows: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| row.expression == "e.querySelector")
            .collect();
        assert!(!rows.is_empty(), "{source}: {graph:#?}");
        assert!(
            rows.iter().all(|row| row.status == expected),
            "{source}: {rows:#?}"
        );
    }
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
fn dom_event_listener_callback_parameters_follow_verified_receiver_origin() {
    for (source, expected) in [
        (
            "document.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); });",
            "external",
        ),
        (
            "const button = document.getElementById('submit'); const values = [1, 2].map(value => value + 1); button.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); });",
            "external",
        ),
        (
            "function run(document) { document.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); }",
            "unresolved",
        ),
        (
            "function run(target) { target.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); }",
            "unresolved",
        ),
        (
            "function run(target: Element) { target.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); }",
            "external",
        ),
        (
            "function run(target: Element) { target.\u{00a0}addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); }",
            "external",
        ),
        (
            "class Element { addEventListener() {} } function run(target: Element) { target.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); }",
            "unresolved",
        ),
        (
            "class Event { preventDefault() {} stopPropagation() {} } document.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); });",
            "unresolved",
        ),
        (
            "document = {}; document.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); });",
            "unresolved",
        ),
        (
            "class View { target: HTMLInputElement; constructor(target: HTMLInputElement) { this.target = target; } bind() { this.target.addEventListener('input', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); } }",
            "external",
        ),
        (
            "class View { target: HTMLDivElement; bind() { this.target.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); } }",
            "external",
        ),
        (
            "class HTMLInputElement { addEventListener() {} } class View { target: HTMLInputElement; bind() { this.target.addEventListener('input', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); } }",
            "unresolved",
        ),
        (
            "class View { target: unknown; bind() { this.target.addEventListener('input', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); } }",
            "unresolved",
        ),
        (
            "class View { target: HTMLInputElement; static bind() { this.target.addEventListener('input', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); } }",
            "unresolved",
        ),
        (
            "class View { target: HTMLInputElement; bind() { function nested() { this.target.addEventListener('input', event => { event.preventDefault(); event.stopPropagation(); event.target?.addEventListener('focus', () => {}); }); } } }",
            "unresolved",
        ),
    ] {
        let facts = ast::extract("listener.ts", "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("listener.ts".to_owned(), facts)]));
        for expression in ["event.preventDefault", "event.stopPropagation", "event.target?.addEventListener"] {
            let rows: Vec<_> = graph.coverage.iter().filter(|row| row.expression == expression).collect();
            assert!(!rows.is_empty() && rows.iter().all(|row| row.status == expected), "{source}: {expression}: {graph:#?}");
            if expected == "external" {
                assert!(rows.iter().all(|row| row.evidence.contains("builtin:web_api")), "{rows:#?}");
            }
        }
    }
}

#[test]
fn escaped_dom_event_property_retains_shadowed_global_facts() {
    let source = "function run(document) { document.on\\u0063lick = event => event.preventDefault(); } // require";
    let facts = ast::extract("escaped-event.ts", "typescript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("escaped-event.ts".to_owned(), facts)]));
    let rows: Vec<_> = graph
        .coverage
        .iter()
        .filter(|row| row.expression == "event.preventDefault")
        .collect();
    assert!(!rows.is_empty(), "{graph:#?}");
    assert!(
        rows.iter().all(|row| row.status == "unresolved"),
        "{rows:#?}"
    );
}

#[test]
fn dom_event_callback_follows_lexical_dom_factory_chain() {
    for (source, expected) in [
        (
            "function bind() { const gridDiv = document.getElementById('grid'); if (!gridDiv) return; const wrapper = gridDiv.querySelector<HTMLElement>('.header'); if (!wrapper) return; wrapper.addEventListener('click', (e) => { e.stopPropagation(); }); }",
            "external",
        ),
        (
            "function bind() { const gridDiv = unknown(); const wrapper = gridDiv.querySelector<HTMLElement>('.header'); wrapper.addEventListener('click', (e) => { e.stopPropagation(); }); }",
            "unresolved",
        ),
        (
            "function bind() { let gridDiv = document.getElementById('grid'); gridDiv = unknown(); const wrapper = gridDiv.querySelector<HTMLElement>('.header'); wrapper.addEventListener('click', (e) => { e.stopPropagation(); }); }",
            "unresolved",
        ),
    ] {
        let facts = ast::extract("dom-factory.ts", "typescript", source).unwrap();
        let graph = linker::link(&BTreeMap::from([("dom-factory.ts".to_owned(), facts)]));
        let rows: Vec<_> = graph.coverage.iter()
            .filter(|row| row.expression == "e.stopPropagation")
            .collect();
        assert!(!rows.is_empty() && rows.iter().all(|row| row.status == expected),
            "{source}: {rows:#?}");
        if expected == "external" {
            assert!(rows.iter().all(|row| row.evidence.contains("builtin:web_api")));
        }
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
fn typescript_dom_platform_extensions_resolve_cleanly() {
    let source = r#"
function runUrl() {
    const url = new URL("https://example.com");
    url.searchParams.set("q", "test");
    url.searchParams.delete("q");
}

function runEvent(btn: Element) {
    btn.onclick = (e) => {
        e.stopPropagation();
        e.preventDefault();
    };
}

function runNullable(toolbar: Element | null, el: HTMLElement) {
    toolbar.querySelectorAll("input");
    el.classList.add("active");
    el.classList.remove("active");
}

class Panel {
    modeCheckboxes: HTMLInputElement[];
    constructor() {
        this.modeCheckboxes = [];
    }
    setup() {
        this.modeCheckboxes.forEach((checkbox) => {
            checkbox.addEventListener("click", (e) => {
                e.stopPropagation();
            });
        });
    }
}
"#;
    let facts = ast::extract("test.ts", "typescript", source).unwrap();
    let graph = linker::link(&BTreeMap::from([("test.ts".to_owned(), facts)]));

    for expression in [
        "url.searchParams.set",
        "url.searchParams.delete",
        "e.stopPropagation",
        "e.preventDefault",
        "toolbar.querySelectorAll",
        "el.classList.add",
        "el.classList.remove",
        "checkbox.addEventListener",
    ] {
        let matches: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| row.expression == expression)
            .collect();
        assert!(
            !matches.is_empty(),
            "expected coverage entry for {expression}"
        );
        for row in matches {
            assert_eq!(
                row.status, "external",
                "expected external status for {expression}, got {}: {row:#?}",
                row.status
            );
            assert!(
                row.evidence.contains("builtin:web_api"),
                "expected builtin:web_api evidence for {expression}, got {}",
                row.evidence
            );
        }
    }
}

#[test]
fn javascript_and_typescript_chained_call_return_value_flow() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_chained_call_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typescript_dom_event_and_storage_platform_types_and_naked_generic_parameter() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_platform_types_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    let source = r#"
function handleDrag(e: DragEvent, target: ParentNode, store: Storage) {
  e.preventDefault();
  target.querySelector(".item");
  store.getItem("token");
}

function identity<T>(value: T): T {
  return value;
}
"#;
    std::fs::write(workspace.join("drag.ts"), source).unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();

    for expression in ["DragEvent", "ParentNode", "Storage"] {
        let (status, evidence): (String, String) = conn.query_row(
            "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='drag.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            rusqlite::params![expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(status, "external", "{expression} should be external");
        assert!(
            evidence.contains("builtin:web_api"),
            "{expression} should have builtin:web_api"
        );
    }

    for expression in ["e.preventDefault", "target.querySelector", "store.getItem"] {
        let (status, evidence): (String, String) = conn.query_row(
            "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='drag.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            rusqlite::params![expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(status, "external", "{expression} should be external");
        assert!(
            evidence.contains("builtin:web_api"),
            "{expression} should have builtin:web_api"
        );
    }

    let count: i64 = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='drag.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='T'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(
        count, 0,
        "naked generic parameter T should have 0 coverage rows"
    );

    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typescript_export_type_reexport_import_targets() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_export_type_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn typescript_constructor_field_assignments_resolve_proven_member_calls() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ts_ctor_field_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn parent_and_child_nodes_keep_their_own_dom_members() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_parent_node_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("dom.ts"),
        r#"
function use(target: ParentNode, child: ChildNode, fragment: DocumentFragment) {
  target.querySelector(".item");
  target.setAttribute("id", "x");
  target.classList.add("on");
  child.querySelector(".item");
  child.removeChild(child);
  fragment.querySelector(".item");
  fragment.closest(".item");
}
"#,
    )
    .unwrap();
    let database = workspace.join(".forge/code-map.sqlite");
    writer::build(&workspace, &database, None).unwrap();
    let conn = reader::open(&database, &workspace).unwrap();
    let coverage = |expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='dom.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
            rusqlite::params![expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(coverage("target.querySelector"), "external");
    assert_eq!(coverage("target.setAttribute"), "unresolved");
    assert_eq!(coverage("target.classList.add"), "unresolved");
    assert_eq!(coverage("child.querySelector"), "unresolved");
    assert_eq!(coverage("child.removeChild"), "external");
    assert_eq!(coverage("fragment.querySelector"), "external");
    assert_eq!(coverage("fragment.closest"), "unresolved");
    drop(conn);
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn constructor_field_type_survives_later_assignment() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_ctor_keep_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn static_this_does_not_call_instance_methods() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_static_this_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn export_type_keeps_classes_and_value_export_drops_type_aliases() {
    use contextunity_forge_mcp::db::{reader, writer};
    use std::time::{SystemTime, UNIX_EPOCH};

    let workspace = std::env::temp_dir().join(format!(
        "forge_export_type_class_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
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
    std::fs::remove_dir_all(workspace).unwrap();
}
