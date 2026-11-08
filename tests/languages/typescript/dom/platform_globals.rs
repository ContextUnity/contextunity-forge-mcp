use super::*;
#[test]
fn node_and_web_callable_globals_have_external_origin() {
    let source = r#"function run() {
        Buffer.from('data');
        console.log('data');
        setTimeout(() => {}, 0);
        fetch('/api');
    }"#;
    let graph = linked("runtime.js", "javascript", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
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
    let graph = linked("runtime.js", "javascript", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let coverage = graph
        .coverage
        .iter()
        .find(|row| row.expression == "process.env")
        .unwrap_or_else(|| panic!("missing process.env: {graph:#?}"));
    assert_eq!(coverage.status, "external", "{coverage:#?}");
    assert!(coverage.evidence.contains("builtin:node"), "{coverage:#?}");

    let source = "function run(process) { return process.env; }";
    let graph = linked("shadow.js", "javascript", source);
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
    let graph = linked("web.js", "javascript", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
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
        let graph = linked("shadow.js", "javascript", source);
        let facts = &graph.facts;
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
        let graph = linked("runtime.cjs", "javascript", &source);
        let facts = &graph.facts;
        assert!(facts.errors.is_empty(), "{source}: {:?}", facts.errors);
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
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "runner.cjs",
        "const { test } = require('node:test');\ntest('case', () => {});\nfunction run(test) { test(); }\n",
    );
    let conn = workspace.build_and_open();
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
        let graph = linked("platform.ts", "typescript", source);
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
                ExtractorHarness::for_lang(language)
                    .at_path(path)
                    .extract(source),
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
        let graph = ExtractorHarness::link_graph(&facts);

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
        let graph = linked("platform.ts", "typescript", source);
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
fn commonjs_and_dom_dispatch_preserves_callback_provenance_in_both_orders() {
    for (path, language) in [("listener.js", "javascript"), ("listener.ts", "typescript")] {
        for (listener, expected) in [
            (
                "document.addEventListener('click', event => event.preventDefault());",
                "external",
            ),
            (
                "function run(document) { document.addEventListener('click', event => event.preventDefault()); }",
                "unresolved",
            ),
        ] {
            for source in [
                format!("require('fs'); {listener}"),
                format!("{listener} require('fs');"),
            ] {
                let graph = linked(path, language, &source);
                let rows: Vec<_> = graph
                    .coverage
                    .iter()
                    .filter(|row| row.expression == "event.preventDefault")
                    .collect();
                assert!(!rows.is_empty(), "{path}: {source}");
                assert!(
                    rows.iter().all(|row| row.status == expected),
                    "{path}: {source}: {rows:#?}"
                );
            }
        }
    }
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
        let graph = linked(path, language, source);
        let facts = &graph.facts;
        assert!(facts.errors.is_empty(), "{facts:#?}");
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
        let graph = linked(path, language, source);
        let facts = &graph.facts;
        assert!(facts.errors.is_empty(), "{path}: {facts:#?}");
        for (expression, origin) in cases {
            assert!(
                graph.coverage.iter().any(|row| row.expression == expression
                    && row.status == "external"
                    && row.evidence.contains(origin)),
                "{path}: {expression}: {graph:#?}"
            );
        }
    }

    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("web.ts", source);
    let conn = workspace.build_and_open();
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
}

#[test]
fn html_table_element_type_and_members_use_verified_dom_origin() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "table.ts",
            "function inspect(table: HTMLTableElement) { table.querySelectorAll('tr'); table.unknownMember(); }",
        ),
        (
            "local.ts",
            "class HTMLTableElement { querySelectorAll() {} } function inspect(table: HTMLTableElement) { table.querySelectorAll(); }",
        ),
        (
            "untyped.ts",
            "function inspect(table) { table.querySelectorAll('tr'); }",
        ),
    ] {
        workspace.write(path, source);
    }
    let conn = workspace.build_and_open();
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
    assert_eq!(
        calls(
            &[(
                "local.ts",
                "class Document { querySelector() {} } function run(document: Document) { document.querySelector(); }"
            )],
            "document.querySelector"
        ),
        ["local.ts"]
    );
    let graph = linked(
        "shadow.ts",
        "typescript",
        "function run(fetch, document, window, localStorage, sessionStorage, URL, FormData, HTMLElement) { fetch('/api'); document.querySelector('div'); window.addEventListener('click', handler); localStorage.getItem('key'); sessionStorage.clear(); URL('/api'); FormData(); HTMLElement(); }",
    );
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
        let source = format!(
            "{prefix} class Worker {{ work() {{}} }} function run(items: {array_type}) {{ items.forEach(item => item.work()); items.map(item => item.missing()); }}"
        );
        let graph = linked("callback.ts", "typescript", &source);
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
        let source = format!(
            "class Worker {{ work() {{}} }} function run(items: Array<Worker>, flag: boolean) {{ {mutation} items.map(item => item.work()); }}"
        );
        let graph = linked("rebound.ts", "typescript", &source);
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "item.work" && row.status == "unresolved"),
            "{mutation}: {graph:#?}"
        );
    }
    let spaced_reassignment = "class Worker { work() {} } function run(items: Array<Worker>) { items = unknown(); items . map(item => item.work()); }";
    let graph = linked("spaced-rebound.ts", "typescript", spaced_reassignment);
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
        let graph = linked(path, "typescript", source);
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| row.expression == "item.work" && row.status == "unresolved"),
            "{path}: Unicode grammar trivia must preserve array callback rebound facts: {graph:#?}"
        );
        assert!(
            !graph
                .edges
                .iter()
                .any(|edge| edge.kind == "calls" && edge.evidence == "item.work"),
            "{path}: stale Worker[] inference must not produce a call edge"
        );
    }
    let spaced_shadow = "class Worker { work() {} } class Array<T> {} function run(items: Array<Worker>) { items . map(item => item.work()); }";
    let graph = linked("spaced-shadow.ts", "typescript", spaced_shadow);
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| row.expression == "item.work" && row.status == "unresolved"),
        "spaced member access with a local Array binding: {graph:#?}"
    );
}
