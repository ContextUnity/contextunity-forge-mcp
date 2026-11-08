use super::*;

const TYPED: &str = "function run(root: Element, event: Event, doc: Document) { root.querySelectorAll('.item'); event.stopPropagation(); doc.querySelectorAll('.item'); root.unknownMember(); doc.closest('.item'); }";

const UNTYPED: &str =
    "function run(root, event) { root.querySelectorAll('.item'); event.stopPropagation(); }";

const LOCAL: &str = "class Element { querySelectorAll() {} } function run(root: Element) { root.querySelectorAll(); }";

const NESTED: &str = r#"
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

fn observation<'a>(
    graph: &'a contextunity_forge_mcp::core::models::Graph,
    expression: &str,
) -> &'a contextunity_forge_mcp::core::models::Coverage {
    graph
        .coverage
        .iter()
        .find(|row| row.expression == expression)
        .unwrap_or_else(|| panic!("missing {expression}: {graph:#?}"))
}

#[test]
fn typed_receiver_members_require_verified_dom_types() {
    let source = TYPED;
    let graph = linked("dom.ts", "typescript", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    for expression in [
        "root.querySelectorAll",
        "event.stopPropagation",
        "doc.querySelectorAll",
    ] {
        let coverage = observation(&graph, expression);
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("builtin:web_api"),
            "{expression}: {coverage:#?}"
        );
    }
    for expression in ["root.unknownMember", "doc.closest"] {
        let unknown = observation(&graph, expression);
        assert_eq!(unknown.status, "unresolved", "{expression}: {unknown:#?}");
    }
}

#[test]
fn untyped_parameters_do_not_gain_dom_members() {
    let source = UNTYPED;
    let graph = linked("untyped.ts", "typescript", source);
    for expression in ["root.querySelectorAll", "event.stopPropagation"] {
        let coverage = observation(&graph, expression);
        assert_eq!(coverage.status, "unresolved", "{expression}: {coverage:#?}");
    }
}

#[test]
fn local_element_classes_keep_their_exact_method_provider() {
    let source = LOCAL;
    let graph = linked("local.ts", "typescript", source);
    let facts = &graph.facts;
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
                && facts.nodes.iter().any(|node| {
                    node.id == edge.dst && node.kind == "method" && node.name == "querySelectorAll"
                })
        }),
        "local Element method call must retain its indexed provider: {graph:#?}"
    );
}

#[test]
fn typed_untyped_and_local_receiver_statuses_survive_persistence() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in [
        ("dom.ts", TYPED),
        ("untyped.ts", UNTYPED),
        ("local.ts", LOCAL),
    ] {
        workspace.write(path, source);
    }
    workspace.build();
    let conn = workspace.open();
    for expression in [
        "root.querySelectorAll",
        "event.stopPropagation",
        "doc.querySelectorAll",
    ] {
        let row = persisted_coverage(&conn, "dom.ts", None, expression);
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
        let row = persisted_coverage(&conn, path, None, expression);
        assert_eq!(
            row.0, "unresolved",
            "persisted {path}:{expression}: {row:?}"
        );
    }
    let local = persisted_coverage(&conn, "local.ts", None, "root.querySelectorAll");
    assert_eq!(
        local.0, "resolved",
        "persisted local Element method: {local:?}"
    );
    drop(conn);
}

#[test]
fn nested_members_and_optional_access_follow_proven_types() {
    let source = NESTED;
    let graph = linked("nested.ts", "typescript", source);
    let facts = &graph.facts;
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    for expression in [
        "element.parentElement?.querySelector",
        "element.ownerDocument?.querySelector",
        "doc.documentElement?.getAttribute",
        "win.document?.getElementById",
        "fromReturn.parentElement?.matches",
    ] {
        let row = observation(&graph, expression);
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
        let row = observation(&graph, expression);
        assert_eq!(row.status, "unresolved", "{expression}: {row:#?}");
    }
}

#[test]
fn nested_member_status_and_evidence_survive_persistence() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("nested.ts", NESTED);
    workspace.build();
    let conn = workspace.open();
    for expression in [
        "element.parentElement?.querySelector",
        "element.ownerDocument?.querySelector",
        "doc.documentElement?.getAttribute",
        "win.document?.getElementById",
        "fromReturn.parentElement?.matches",
    ] {
        let row = persisted_coverage(&conn, "nested.ts", None, expression);
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
        let row = persisted_coverage(&conn, "nested.ts", None, expression);
        assert_eq!(row.0, "unresolved", "persisted {expression}: {row:?}");
    }
    contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
    drop(conn);
}
