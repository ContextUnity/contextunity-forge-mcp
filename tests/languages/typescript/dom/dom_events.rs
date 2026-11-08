use super::*;
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
        let graph = linked("callback-dom.ts", "typescript", source);
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
        let graph = linked("listener.ts", "typescript", source);
        for expression in [
            "event.preventDefault",
            "event.stopPropagation",
            "event.target?.addEventListener",
        ] {
            let rows: Vec<_> = graph
                .coverage
                .iter()
                .filter(|row| row.expression == expression)
                .collect();
            assert!(
                !rows.is_empty() && rows.iter().all(|row| row.status == expected),
                "{source}: {expression}: {graph:#?}"
            );
            if expected == "external" {
                assert!(
                    rows.iter().all(|row| row.evidence.contains("builtin:web_api")),
                    "{rows:#?}"
                );
            }
        }
    }
}

#[test]
fn escaped_dom_event_property_retains_shadowed_global_facts() {
    let source = "function run(document) { document.on\\u0063lick = event => event.preventDefault(); } // require";
    let graph = linked("escaped-event.ts", "typescript", source);
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
        let graph = linked("dom-factory.ts", "typescript", source);
        let rows: Vec<_> = graph
            .coverage
            .iter()
            .filter(|row| row.expression == "e.stopPropagation")
            .collect();
        assert!(
            !rows.is_empty() && rows.iter().all(|row| row.status == expected),
            "{source}: {rows:#?}"
        );
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
    let graph = linked("test.ts", "typescript", source);

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
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
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
    workspace.write("drag.ts", source);
    let conn = workspace.build_and_open();

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
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "dom.ts",
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
    );
    let conn = workspace.build_and_open();
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
    let w = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        (
            "dom.js",
            "function run() { const element = document.querySelector('#root'); element.addEventListener('click', () => {}); const child = element.querySelector('.child'); child.getAttribute('id'); }",
        ),
        (
            "fetch.js",
            "async function run() { const response = await fetch('/api'); response.json(); response.text(); }",
        ),
        (
            "fetch_bad_property_call.js",
            "async function run() { const response = await fetch('/api'); response.status(); }",
        ),
        (
            "dom_shadow.js",
            "function run(document) { const element = document.querySelector('#root'); element.addEventListener('click', () => {}); }",
        ),
        (
            "dom_reassigned.js",
            "function run() { document = {}; const element = document.querySelector('#root'); element.addEventListener('click', () => {}); }",
        ),
        (
            "fetch_shadow.js",
            "async function run(fetch) { const response = await fetch('/api'); response.json(); }",
        ),
        (
            "fetch_local_function.js",
            "async function fetch() { return {}; } async function run() { const response = await fetch('/api'); response.json(); }",
        ),
        (
            "fetch_import.js",
            "import { fetch } from './transport.js'; async function run() { const response = await fetch('/api'); response.json(); }",
        ),
        ("transport.js", "export async function fetch() { return {}; }"),
        (
            "fetch_rebound.js",
            "async function run() { fetch = async () => ({}); const response = await fetch('/api'); response.json(); }",
        ),
        (
            "fetch_plain.js",
            "function run() { const pending = fetch('/api'); pending.json(); }",
        ),
        (
            "fetch_reassigned.js",
            "async function run() { let response = await fetch('/api'); response = {}; response.json(); }",
        ),
        (
            "fetch_unknown.js",
            "async function run() { const response = await fetch('/api'); response.unsupported(); }",
        ),
        (
            "fetch_local.js",
            "class Response { json() {} } async function run() { const response = new Response(); response.json(); }",
        ),
        (
            "nested_page.js",
            "function outer() { const page = {}; function inner() { page.locator('button'); } inner(); }",
        ),
        (
            "nested_page_late.js",
            "function outer() { function inner() { page.locator('button'); } const page = {}; inner(); }",
        ),
    ] {
        w.write(path, source);
    }
    w.build();
    let conn = w.open();
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
