use super::*;
use super::super::support::Workspace;

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

    let workspace = Workspace::new();
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
    let workspace = Workspace::new();
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
}

#[test]
fn dom_factory_return_receivers() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
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
    let workspace = Workspace::new();
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
    let workspace = Workspace::new();
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
}

#[test]
fn html_table_element_type_and_members_use_verified_dom_origin() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
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
fn typescript_dom_event_and_storage_platform_types_and_naked_generic_parameter() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
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
}

#[test]
fn parent_and_child_nodes_keep_their_own_dom_members() {
    use contextunity_forge_mcp::db::{reader, writer};

    let workspace = Workspace::new();
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
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_web_factory_results_keep_proven_receivers() {
    let w = Workspace::new();
    for (path, source) in [
        ("dom.js", "function run() { const element = document.querySelector('#root'); element.addEventListener('click', () => {}); const child = element.querySelector('.child'); child.getAttribute('id'); }"),
        ("fetch.js", "async function run() { const response = await fetch('/api'); response.json(); response.text(); }"),
        ("fetch_bad_property_call.js", "async function run() { const response = await fetch('/api'); response.status(); }"),
        ("dom_shadow.js", "function run(document) { const element = document.querySelector('#root'); element.addEventListener('click', () => {}); }"),
        ("dom_reassigned.js", "function run() { document = {}; const element = document.querySelector('#root'); element.addEventListener('click', () => {}); }"),
        ("fetch_shadow.js", "async function run(fetch) { const response = await fetch('/api'); response.json(); }"),
        ("fetch_local_function.js", "async function fetch() { return {}; } async function run() { const response = await fetch('/api'); response.json(); }"),
        ("fetch_import.js", "import { fetch } from './transport.js'; async function run() { const response = await fetch('/api'); response.json(); }"),
        ("transport.js", "export async function fetch() { return {}; }"),
        ("fetch_rebound.js", "async function run() { fetch = async () => ({}); const response = await fetch('/api'); response.json(); }"),
        ("fetch_plain.js", "function run() { const pending = fetch('/api'); pending.json(); }"),
        ("fetch_reassigned.js", "async function run() { let response = await fetch('/api'); response = {}; response.json(); }"),
        ("fetch_unknown.js", "async function run() { const response = await fetch('/api'); response.unsupported(); }"),
        ("fetch_local.js", "class Response { json() {} } async function run() { const response = new Response(); response.json(); }"),
        ("nested_page.js", "function outer() { const page = {}; function inner() { page.locator('button'); } inner(); }"),
        ("nested_page_late.js", "function outer() { function inner() { page.locator('button'); } const page = {}; inner(); }"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    for (path, expression) in [
        ("dom.js", "element.addEventListener"),
        ("dom.js", "child.getAttribute"),
        ("fetch.js", "response.json"),
        ("fetch.js", "response.text"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(row.0, "external", "{path}:{expression}: {row:?}");
        assert!(
            row.1.contains("builtin:web_api"),
            "{path}:{expression}: {row:?}"
        );
    }
    for (path, expression) in [
        ("dom_shadow.js", "element.addEventListener"),
        ("dom_reassigned.js", "element.addEventListener"),
        ("fetch_shadow.js", "response.json"),
        ("fetch_local_function.js", "response.json"),
        ("fetch_import.js", "response.json"),
        ("fetch_rebound.js", "response.json"),
        ("fetch_plain.js", "pending.json"),
        ("fetch_reassigned.js", "response.json"),
        ("fetch_unknown.js", "response.unsupported"),
        ("fetch_bad_property_call.js", "response.status"),
        ("nested_page.js", "page.locator"),
        ("nested_page_late.js", "page.locator"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(row.0, "unresolved", "{path}:{expression}: {row:?}");
    }
    assert_eq!(coverage("fetch_local.js", "response.json").0, "resolved");
}
