use super::support::Workspace;
use contextunity_forge_mcp::engine::{ast, languages};
use contextunity_forge_mcp::{
    cli,
    core::response::{Detail, QueryOptions, ResponsePolicy},
    db::reader,
};

#[cfg(feature = "lang-rust")]
#[test]
fn ast_search_matches_root_rejected_rust_method_call_fragment() {
    let matches = ast::search(
        "fn invoke() { client.fetch(value); }",
        "src/lib.rs",
        "rust",
        "client.fetch($ARG)",
        10,
    )
    .expect("Rust method-call fragment should be searchable");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["text"], "client.fetch(value)");
    assert_eq!(matches[0]["captures"]["ARG"], "value");
}

#[cfg(feature = "lang-rust")]
#[test]
fn ast_search_matches_root_rejected_rust_macro_statement_fragment() {
    let matches = ast::search(
        "fn run() { let value = print!(\"ready\"); }",
        "src/lib.rs",
        "rust",
        "let value = print!(\"ready\");",
        10,
    )
    .expect("Rust macro statement fragment should be searchable");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["text"], "let value = print!(\"ready\");");
}

#[cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-html",
    feature = "lang-vue"
))]
#[test]
fn ast_search_profile_capabilities_cover_complete_declaration_and_attribute_patterns() {
    let mut cases: Vec<(&str, &str, &str, &str)> = Vec::new();

    #[cfg(feature = "lang-rust")]
    {
        cases.extend([
            (
                "rust",
                "src/lib.rs",
                "fn target() { call(value); }",
                "fn target() { call($VALUE); }",
            ),
            (
                "rust",
                "src/lib.rs",
                "fn target() { call(value); }",
                "fn target()",
            ),
            (
                "rust",
                "src/lib.rs",
                "struct Config { field: u32 }",
                "struct Config",
            ),
            ("rust", "src/lib.rs", "enum Status { Ready }", "enum Status"),
            (
                "rust",
                "src/lib.rs",
                "impl Config { fn run() {} }",
                "impl Config",
            ),
            (
                "rust",
                "src/lib.rs",
                "#[derive(Debug)] struct Item { field: u32 }",
                "#[derive(Debug)] struct Item {}",
            ),
        ]);
        let capabilities = languages::require("rust")
            .unwrap()
            .ast_search_capabilities();
        assert!(capabilities.fragment_probe.is_some());
        assert!(capabilities.attribute_kinds.contains(&"attribute_item"));
        for kind in [
            "block",
            "field_declaration_list",
            "enum_variant_list",
            "declaration_list",
        ] {
            assert!(capabilities.declaration_body_kinds.contains(&kind));
        }
    }

    #[cfg(feature = "lang-python")]
    {
        cases.extend([
            (
                "python",
                "service.py",
                "def target():\n    return value\nclass Widget:\n    pass\n",
                "def target():",
            ),
            (
                "python",
                "service.py",
                "def target():\n    return value\nclass Widget:\n    pass\n",
                "class Widget:",
            ),
            (
                "python",
                "service.py",
                "def use(client):\n    client.fetch(value)\n",
                "client.fetch($ARG)",
            ),
            (
                "python",
                "service.py",
                "@property\ndef value(self):\n    return 1\n",
                "@property\ndef value(self):\n    return 1",
            ),
        ]);
        let capabilities = languages::require("python")
            .unwrap()
            .ast_search_capabilities();
        assert!(capabilities.attribute_kinds.contains(&"decorator"));
        assert!(capabilities.declaration_body_kinds.contains(&"block"));
    }

    #[cfg(feature = "lang-typescript")]
    {
        cases.extend([
            (
                "typescript",
                "service.ts",
                "function target() { return value; }",
                "function target()",
            ),
            (
                "typescript",
                "service.ts",
                "function use(client) { client.fetch(value); }",
                "client.fetch($ARG)",
            ),
            (
                "typescript",
                "service.ts",
                "class Widget { run() {} }",
                "class Widget",
            ),
            (
                "typescript",
                "service.ts",
                "interface Contract { value: string }",
                "interface Contract",
            ),
            (
                "typescript",
                "service.ts",
                "@sealed class Widget {}",
                "@sealed class Widget {}",
            ),
        ]);
        let capabilities = languages::require("typescript")
            .unwrap()
            .ast_search_capabilities();
        assert!(capabilities.attribute_kinds.contains(&"decorator"));
        assert!(capabilities
            .declaration_body_kinds
            .contains(&"statement_block"));
        assert!(capabilities.declaration_body_kinds.contains(&"class_body"));
        assert!(capabilities
            .declaration_body_kinds
            .contains(&"interface_body"));
    }

    #[cfg(feature = "lang-html")]
    {
        cases.push((
            "html",
            "index.html",
            "<main><button disabled></button></main>",
            "<main><button disabled></button></main>",
        ));
        let capabilities = languages::require("html")
            .unwrap()
            .ast_search_capabilities();
        assert!(capabilities.attribute_kinds.contains(&"attribute"));
        assert!(capabilities.fragment_probe.is_none());
    }

    #[cfg(feature = "lang-vue")]
    {
        cases.push((
            "vue",
            "src/Widget.vue",
            "export function target() { return 1; }",
            "export function target() { return 1; }",
        ));
        cases.push((
            "vue",
            "src/Widget.vue",
            "export function use(client) { client.fetch(value); }",
            "client.fetch($ARG)",
        ));
        let capabilities = languages::require("vue").unwrap().ast_search_capabilities();
        assert!(capabilities.attribute_kinds.contains(&"decorator"));
    }

    for (language, path, source, pattern) in cases {
        let matches = ast::search(source, path, language, pattern, 10)
            .unwrap_or_else(|error| panic!("{language} pattern {pattern:?}: {error}"));
        assert!(
            !matches.is_empty(),
            "{language} pattern {pattern:?} did not match"
        );
    }
}

#[cfg(feature = "lang-toml")]
#[test]
fn ast_search_profiles_with_only_complete_syntax_use_the_shared_default() {
    let capabilities = languages::require("toml")
        .unwrap()
        .ast_search_capabilities();
    assert!(capabilities.declaration_kinds.is_empty());
    assert!(capabilities.fragment_probe.is_none());
    let matches = ast::search(
        "edition = \"2021\"",
        "Cargo.toml",
        "toml",
        "edition = \"2021\"",
        10,
    )
    .unwrap();
    assert_eq!(matches.len(), 1);
}

#[cfg(feature = "lang-rust")]
#[test]
fn public_ast_search_keeps_rust_body_only_literal_candidates() {
    let workspace = Workspace::new();
    workspace.write("src/a_candidate.rs", "fn coverage_owner_language() {}\n");
    workspace.write(
        "src/z_body.rs",
        "fn other() { let value = \"coverage_owner_language\"; }\n",
    );
    workspace.build();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Compact),
        None,
    )
    .unwrap();

    let result = cli::ast::search_paged(
        &conn,
        &workspace.0,
        "\"coverage_owner_language\"",
        "rust",
        None,
        &options,
    )
    .unwrap();
    let items = result["matches"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{result}");
    assert_eq!(items[0]["path"], "src/z_body.rs");
    assert_eq!(items[0]["text"], "\"coverage_owner_language\"");
}

#[cfg(feature = "lang-rust")]
#[test]
fn public_ast_search_reports_structured_tree_sitter_pattern_diagnostics() {
    fn smallest_error(root: tree_sitter::Node<'_>) -> Option<std::ops::Range<usize>> {
        let mut stack = vec![root];
        let mut smallest = None;
        while let Some(node) = stack.pop() {
            if node.is_error() && node.start_byte() < node.end_byte() {
                let range = node.start_byte()..node.end_byte();
                if smallest
                    .as_ref()
                    .is_none_or(|current: &std::ops::Range<usize>| range.len() < current.len())
                {
                    smallest = Some(range);
                }
            }
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        smallest
    }

    let workspace = Workspace::new();
    workspace.write("src/lib.rs", "fn valid() {}\n");
    workspace.build();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Compact),
        None,
    )
    .unwrap();
    let pattern = "let = ;";
    let result =
        cli::ast::search_paged(&conn, &workspace.0, pattern, "rust", None, &options).unwrap();
    let mut parser = contextunity_forge_mcp::engine::ast::parser("rust", "src/lib.rs").unwrap();
    let tree = parser.parse(pattern, None).unwrap();
    let expected = smallest_error(tree.root_node()).expect("pattern has a nonempty ERROR node");
    let diagnostic = &result["diagnostic"];

    assert_eq!(diagnostic["code"], "unsupported_ast_pattern");
    assert_eq!(diagnostic["language"], "rust");
    assert_eq!(diagnostic["span"]["start_byte"], expected.start);
    assert_eq!(diagnostic["span"]["end_byte"], expected.end);
    assert!(
        diagnostic["span"]["end_byte"].as_u64().unwrap()
            > diagnostic["span"]["start_byte"].as_u64().unwrap()
    );
    assert!(!diagnostic["hint"].as_str().unwrap().is_empty());
    assert!(!diagnostic["examples"].as_array().unwrap().is_empty());
}
