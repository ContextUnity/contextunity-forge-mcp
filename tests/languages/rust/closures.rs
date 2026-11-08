use super::{coverage, linked};

#[test]
fn iterator_callbacks_inherit_item_receivers() {
    let iter_source = r#"
        struct Item; impl Item { fn finish(&self) {} }
        fn f(items: Vec<Item>) {
            items.iter().map(|entry| entry.finish());
            items.iter().filter(|entry| { entry.finish(); true });
        }
    "#;
    let path = "src/probe_iter.rs";
    let iter_graph = linked(path, iter_source);
    let facts = &iter_graph.facts;
    let target = facts
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.qualname.ends_with(".Item.finish"))
        .unwrap()
        .id
        .clone();
    let mut references: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.expression == "entry.finish")
        .map(|reference| (reference.line, reference.column, reference.kind.as_str()))
        .collect();
    references.sort_unstable();
    assert_eq!(references, [(4, 37, "calls"), (5, 42, "calls")]);
    let mut coverage: Vec<_> = iter_graph
        .coverage
        .iter()
        .filter(|c| c.expression == "entry.finish")
        .map(|c| (c.path.as_str(), c.line, c.status.as_str()))
        .collect();
    coverage.sort_unstable();
    assert_eq!(coverage, [(path, 4, "resolved"), (path, 5, "resolved")]);
    let mut calls: Vec<_> = iter_graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == "entry.finish")
        .map(|edge| (edge.path.as_str(), edge.line, edge.dst.as_str()))
        .collect();
    calls.sort_unstable();
    assert_eq!(
        calls,
        [(path, 4, target.as_str()), (path, 5, target.as_str())]
    );
}

#[test]
fn tuple_payload_patterns_keep_exact_receivers() {
    for (path, source, expected) in [
        (
            "src/probe_tup_ret.rs",
            r#"
                struct A; impl A { fn finish(&self) {} }
                struct B; impl B { fn close(&self) {} }
                fn make() -> (A, B) { (A, B) }
                fn f() { let (a, b) = make(); a.finish(); b.close(); }
            "#,
            &[
                ("a.finish", "resolved", None),
                ("b.close", "resolved", None),
            ][..],
        ),
        (
            "src/probe_tup_rest.rs",
            r#"
                struct A;
                struct B; impl B { fn finish(&self) {} }
                struct C; impl C { fn finish(&self) {} }
                fn f() { let (.., last) = (A {}, B {}, C {}); last.finish(); }
            "#,
            &[(
                "last.finish",
                "resolved",
                Some("src.probe_tup_rest.C.finish"),
            )][..],
        ),
    ] {
        let graph = linked(path, source);
        for &(expression, status, evidence) in expected {
            let row = coverage(&graph, expression);
            assert_eq!(
                row.status, status,
                "{expression} from tuple payload pattern: {row:#?}"
            );
            if let Some(evidence) = evidence {
                assert!(
                    row.evidence.contains(evidence),
                    "{expression} must select the final tuple payload: {row:#?}"
                );
            }
        }
    }
}

#[test]
fn callback_and_parser_homonyms_do_not_acquire_external_types() {
    let unrelated_callback = r#"
        fn unrelated<F: Fn(i32)>(callback: F) { callback(1); }
        fn f() { unrelated(|row| row.get(0)); }
    "#;
    let parser_homonym = r#"
        use pulldown_cmark::Parser;
        fn f(parser: &mut Parser) {
            let tree = parser.parse("x", None).unwrap();
            let root = tree.root_node(); root.kind();
        }
    "#;
    let local_query_map = r#"
        struct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }
        struct Local; impl Local { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } }
        fn f(val: Local) { val.query_map(|entry| entry.get(0)); }
    "#;
    let local_connection = r#"
        struct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }
        struct Connection;
        impl Connection { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } }
        fn f(db: Connection) { db.query_map(|entry| entry.get(0)); }
    "#;
    for (label, source, expression, forbidden_evidence) in [
        (
            "unrelated-callback",
            unrelated_callback,
            "row.get",
            None,
        ),
        (
            "parser-homonym",
            parser_homonym,
            "root.kind",
            None,
        ),
        (
            "local-query-map",
            local_query_map,
            "entry.get",
            None,
        ),
        (
            "local-connection",
            local_connection,
            "entry.get",
            Some("rusqlite::Row"),
        ),
        (
            "nested-local",
            "use rusqlite::Result;\nstruct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }\nfn f() { struct Connection; impl Connection { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } } fn run(c: Connection) { c.query_map(|entry| entry.get(0)); } }",
            "entry.get",
            Some("rusqlite::Row"),
        ),
        (
            "qualified-local",
            "use rusqlite::Result;\nstruct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }\nmod other { pub struct Connection; impl Connection { pub fn query_map<F: Fn(super::LocalItem)>(&self, f: F) { f(super::LocalItem {}); } } }\nfn f(c: other::Connection) { c.query_map(|entry| entry.get(0)); }",
            "entry.get",
            Some("rusqlite::Row"),
        ),
    ] {
        let graph = linked("probe.rs", source);
        let row = coverage(&graph, expression);
        assert_ne!(
            row.status, "external",
            "{label}: local or unrelated homonym became an external type: {row:#?}"
        );
        if let Some(evidence) = forbidden_evidence {
            assert!(
                !row.evidence.contains(evidence),
                "{label}: evidence must not mention {evidence}: {row:#?}"
            );
        }
    }
}

#[test]
fn rusqlite_and_tree_sitter_chains_inherit_external_receivers() {
    for (label, source, expected) in [
        (
            "rusqlite",
            r#"
                use rusqlite::Connection;
                fn f(conn: &Connection) {
                    let mut statement = conn.prepare("SELECT 1").unwrap();
                    statement.execute([]);
                    statement.query_map([], |entry| entry.get(0));
                }
            "#,
            &[
                ("conn.prepare", "external"),
                ("statement.execute", "external"),
                ("entry.get", "external"),
            ][..],
        ),
        (
            "tree-sitter",
            r#"
                use tree_sitter::Parser;
                fn f(parser: &mut Parser) {
                    let tree = parser.parse("x", None).unwrap();
                    let root = tree.root_node(); root.kind();
                    let child = root.child_by_field_name("body").unwrap(); child.kind();
                }
            "#,
            &[
                ("tree.root_node", "external"),
                ("root.kind", "external"),
                ("child.kind", "external"),
            ][..],
        ),
    ] {
        let graph = linked("probe.rs", source);
        for &(expression, status) in expected {
            let row = coverage(&graph, expression);
            assert_eq!(row.status, status, "{label}: {row:#?}");
        }
    }
}

#[test]
fn scoped_tuple_and_loop_bindings_keep_their_own_receivers() {
    let while_let_source = r#"
        struct Before; impl Before { fn finish(&self) {} }
        struct After; impl After { fn finish(&self) {} }
        fn f(option: Option<After>) {
            let entry = Before {};
            while let Some(entry) = option {
                entry.finish();
                break;
            }
            entry.finish();
        }
    "#;
    let while_let_graph = linked("src/probe_while_let.rs", while_let_source);
    let finishes: Vec<_> = while_let_graph
        .coverage
        .iter()
        .filter(|c| c.expression == "entry.finish")
        .collect();
    assert_eq!(finishes.len(), 2, "expected 2 entry.finish calls");
    assert!(
        finishes[0]
            .evidence
            .contains("src.probe_while_let.After.finish"),
        "inner call must resolve to After: {:?}",
        finishes[0]
    );
    assert!(
        finishes[1]
            .evidence
            .contains("src.probe_while_let.Before.finish"),
        "post-loop call must resolve to Before: {:?}",
        finishes[1]
    );

    let tuple_shadow_source = r#"
        struct A; impl A { fn finish(&self) {} }
        struct B; impl B { fn finish(&self) {} }
        fn make() -> (A, A) { (A {}, A {}) }
        fn f() {
            let make = || (B {}, B {});
            let (entry, _) = make();
            entry.finish();
        }
    "#;
    let tuple_shadow_graph = linked("src/probe_tup_shadow.rs", tuple_shadow_source);
    let shadow_finish = coverage(&tuple_shadow_graph, "entry.finish");
    assert!(
        !shadow_finish
            .evidence
            .contains("src.probe_tup_shadow.A.finish"),
        "shadowed make() must NOT link to top-level A.finish: {shadow_finish:#?}"
    );

    let nested_fn_source = r#"
        struct A; impl A { fn finish(&self) {} }
        struct B; impl B { fn finish(&self) {} }
        fn make() -> (A, A) { (A {}, A {}) }
        fn f() {
            fn make() -> (B, B) { (B {}, B {}) }
            let (entry, _) = make();
            entry.finish();
        }
    "#;
    let nested_fn_graph = linked("src/probe_nested_fn.rs", nested_fn_source);
    let nested_finish = coverage(&nested_fn_graph, "entry.finish");
    assert!(
        !nested_finish
            .evidence
            .contains("src.probe_nested_fn.A.finish"),
        "nested fn make() must NOT link to top-level A.finish: {nested_finish:#?}"
    );
    assert!(
        nested_finish
            .evidence
            .contains("src.probe_nested_fn.B.finish"),
        "nested fn make() must link to inner B.finish: {nested_finish:#?}"
    );
}

#[test]
fn tuple_destructuring_carries_real_external_receivers() {
    let source = r#"
        use tempfile::NamedTempFile;
        use walkdir::WalkDir;
        use rusqlite::Connection;
        fn run_seam(conn: &Connection) {
            let (file, code) = (NamedTempFile::new().unwrap(), 42);
            file.path();
            let walker = WalkDir::new(".").into_iter();
            let mut stmt = conn.prepare("SELECT 1").unwrap();
            let _ = stmt.query_map([], |row| {
                row.get::<_, i64>(0)
            });
        }
        fn run_match_and_if_let() {
            let conn_res = Connection::open_in_memory();
            let conn = match conn_res {
                Ok(c) => c,
                Err(_) => return,
            };
            if let Ok(mut stmt) = conn.prepare("SELECT 1") {
                stmt.execute([]).unwrap();
            }
        }
        fn run_root_crate_import() {
            use contextunity_forge_mcp::engine::ast;
            let _ = ast::extract("test.rs", "rust", "fn main() {}");
        }
    "#;
    let sources = [
        ("src/seam_destructuring.rs", source),
        (
            "src/engine/ast.rs",
            "pub fn extract(_path: &str, _lang: &str, _code: &str) {}",
        ),
    ];
    let facts = sources
        .iter()
        .map(|(path, source)| {
            let parsed = contextunity_forge_mcp::engine::ast::extract(path, "rust", source)
                .unwrap_or_else(|error| panic!("{path} (rust): {error}"));
            ((*path).to_owned(), parsed)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let seam_graph = contextunity_forge_mcp::engine::linker::link(&facts);
    let file_path_cov = coverage(&seam_graph, "file.path");
    assert_eq!(
        file_path_cov.status, "external",
        "file.path from tuple destructuring must resolve to external: {file_path_cov:#?}"
    );
    let row_get_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression.starts_with("row.get"))
        .expect("missing coverage for row.get");
    assert_eq!(
        row_get_cov.status, "external",
        "row.get from closure inference must resolve to external: {row_get_cov:#?}"
    );
    let stmt_exec_cov = coverage(&seam_graph, "stmt.execute");
    assert_eq!(
        stmt_exec_cov.status, "external",
        "stmt.execute from if-let Ok(mut stmt) pattern binding must resolve to external: {stmt_exec_cov:#?}"
    );
    let ast_extract_cov = coverage(&seam_graph, "ast::extract");
    assert_eq!(
        ast_extract_cov.status, "external",
        "ast::extract without owning Cargo manifest and indexed src/lib.rs must remain external: {ast_extract_cov:#?}"
    );
}

#[test]
fn walkdir_type_is_registered() {
    assert!(
        contextunity_forge_mcp::engine::languages::rust::is_registered_rust_type(
            "walkdir::WalkDir"
        )
    );
}
