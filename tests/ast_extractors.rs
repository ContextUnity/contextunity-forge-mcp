#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::ast;
#[cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-html",
    feature = "lang-vue",
    feature = "lang-toml"
))]
use contextunity_forge_mcp::engine::languages;
#[cfg(feature = "lang-typescript")]
use contextunity_forge_mcp::engine::linker;
#[cfg(feature = "lang-typescript")]
use std::collections::BTreeMap;

#[cfg(feature = "lang-python")]
#[test]
fn test_python_ast_extractor() {
    let source = r#"
class ServiceClient:
    """Service client docstring."""
    def __init__(self, host: str):
        self.host = host

    def fetch(self, endpoint: str) -> dict:
        return self._send_request(endpoint)

    def _send_request(self, endpoint: str) -> dict:
        return {"status": "ok"}

def standalone_helper(x: int) -> int:
    return x * 2
"#;
    let facts = ast::extract("test.py", "python", source).expect("python extraction failed");
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "class" && n.name == "ServiceClient"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "fetch"));
    let fetch = facts
        .nodes
        .iter()
        .find(|n| n.name == "fetch")
        .expect("fetch method missing");
    assert_eq!(fetch.details["receiver_type"], "ServiceClient");
    assert_eq!(fetch.details["is_method"], true);
    assert_eq!(fetch.details["is_static"], false);
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "standalone_helper"));
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_ast_extractor() {
    let source = r#"
use std::collections::HashMap;

pub struct Config {
    pub host: String,
    pub port: u16,
}

pub enum Status {
    Active,
    Inactive,
}

impl Config {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}
"#;
    let facts = ast::extract("test.rs", "rust", source).expect("rust extraction failed");
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "struct" && n.name == "Config"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "enum" && n.name == "Status"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "new"));
    let constructor = facts
        .nodes
        .iter()
        .find(|n| n.name == "new")
        .expect("new method missing");
    assert_eq!(constructor.details["receiver_type"], "Config");
    assert_eq!(constructor.details["is_method"], true);
    assert_eq!(constructor.details["is_static"], true);
}

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

#[cfg(feature = "lang-typescript")]
#[test]
fn test_typescript_ast_extractor() {
    let source = r#"
import { Router } from "express";

export interface User {
    id: string;
    name: string;
}

export class UserService {
    getUser(id: string): User {
        return { id, name: "Alice" };
    }
}

export const processUser = (u: User) => {
    console.log(u);
};
"#;
    let facts =
        ast::extract("test.ts", "typescript", source).expect("typescript extraction failed");
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "class" && n.name == "UserService"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "getUser"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "processUser"));
    let process_user = facts
        .nodes
        .iter()
        .find(|n| n.name == "processUser")
        .expect("processUser function missing");
    assert!(process_user.details.get("is_method").is_none());
    assert!(process_user.details.get("is_static").is_none());
    assert!(process_user.details.get("receiver_type").is_none());
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_tsx_parser_slot_reuses_cleanly_across_files() {
    for (path, source, expected_name) in [
        (
            "First.tsx",
            "export const First = () => <main><span /></main>;",
            "First",
        ),
        (
            "Second.tsx",
            "export function Second() { return <button />; }",
            "Second",
        ),
    ] {
        let facts = ast::extract(path, "typescript", source).expect("TSX extraction failed");
        assert!(facts.errors.is_empty(), "{path}: {:?}", facts.errors);
        assert!(facts
            .nodes
            .iter()
            .any(|node| node.name == expected_name && node.kind == "function"));
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_profile_parser_reuse_tracks_the_selected_grammar() {
    let profile =
        contextunity_forge_mcp::engine::languages::by_id("vue").expect("Vue profile is compiled");
    for (path, source, expected_name) in [
        (
            "First.vue",
            "export function First() { return 1; }",
            "First",
        ),
        (
            "Second.tsx",
            "export function Second() { return <button />; }",
            "Second",
        ),
        ("Third.js", "export function Third() { return 1; }", "Third"),
    ] {
        let mut facts = contextunity_forge_mcp::core::models::Facts::default();
        contextunity_forge_mcp::engine::languages::parse_file(
            profile, path, source, path, &mut facts,
        )
        .expect("Vue profile parse failed");
        assert!(facts.errors.is_empty(), "{path}: {:?}", facts.errors);
        assert!(facts
            .nodes
            .iter()
            .any(|node| node.name == expected_name && node.kind == "function"));
    }
}

#[cfg(feature = "lang-go")]
#[test]
fn test_go_ast_extractor() {
    let source = r#"
package main

import "fmt"

type Server struct {
    port int
}

func (s *Server) Start() {
    fmt.Println(s.port)
}

func Run() {
    s := &Server{port: 8080}
    s.Start()
}
"#;
    let facts = ast::extract("main.go", "go", source).expect("go extraction failed");
    assert!(facts.nodes.iter().any(|n| n.name == "Server"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "Start"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "Run"));
}

#[cfg(feature = "lang-python")]
#[test]
fn python_routes_preserve_http_decorators_and_django_patterns_as_mapping_calls_remain_calls() {
    let source = "def auth(): pass\ndef handler(): pass\napp.get('/items', auth, handler)\nrouter.post('^item$', handler)\npath('items/', handler)\nre_path('^items/', handler)\ndjango.urls.path('', handler)\n@bp.route('/multi', methods=['GET', 'PATCH'])\ndef decorated(): pass\n@mapping.get('/lookup')\ndef mapped(): pass\nconfig.get('section', handler)\napp.get('setting', handler)\nconfig.get('/setting', handler)\napp.get('/number', 0)\napp.get('/negative', -1)\napp.get('/boolean', True)\napp.get('/none', None)\napp.get('/string', 'handler')\napp.get('/list', [handler])\napp.get('/tuple', (handler,))\napp.get('/mapping', {'handler': handler})\n";
    let facts = ast::extract("routes.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let mut routes: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "route")
        .map(|node| node.name.as_str())
        .collect();
    routes.sort_unstable();
    assert_eq!(
        routes,
        [
            "ANY ",
            "ANY ^items/",
            "ANY items/",
            "GET /items",
            "GET /multi",
            "PATCH /multi",
            "POST ^item$"
        ]
    );
    let items = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /items")
        .unwrap();
    assert_eq!(
        items.details["handlers"],
        serde_json::json!(["auth", "handler"])
    );
    let ordinary_calls: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.kind == "calls" && reference.expression == "config.get")
        .map(|reference| reference.line)
        .collect();
    assert_eq!(ordinary_calls, [12, 14]);
    for name in ["GET /multi", "PATCH /multi"] {
        let route = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let handler = facts
            .nodes
            .iter()
            .find(|node| node.name == "decorated")
            .unwrap();
        assert!(facts
            .edges
            .iter()
            .any(|edge| edge.src == route.id && edge.dst == handler.id && edge.kind == "handles"));
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_routes_preserve_middleware_and_framework_decorators_with_finite_receivers() {
    let source = "function auth() {}\nfunction view() {}\napp.get('/items', auth, view);\nserver.put('^items$', view);\napi.delete('/items', view);\nblueprint.patch('/items', view);\nroute.options('/items', view);\nrouter.head('/items', view);\napp.get('/asserted', view as Handler);\napp.get('/non-null', (view!));\napp.get('/type-assertion', <Handler>view);\napp.get('/string-as', 'view' as unknown as Handler);\nconst routes = [{path: 'settings', component: view}];\nclass Controller { @Get('/box') show() {} }\ncache.get('/setting', view);\napp.get('setting', view);\nget('/setting', view);\napp.get('/number', 0);\napp.get('/negative', -1);\napp.get('/boolean', true);\napp.get('/null', null);\napp.get('/string', 'view');\napp.get('/template', `view`);\napp.get('/array', [view]);\napp.get('/object', {view});\nconst values = [{path: '/primitive', component: false}];\nfunction configure(options) {}\nconfigure({path: '/settings', component: view});\naxios . get('/users');\nclient .post('/api/orders');\n";
    let facts = ast::extract("routes.ts", "typescript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let mut routes: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "route")
        .map(|node| node.name.as_str())
        .collect();
    routes.sort_unstable();
    assert!(routes.contains(&"ANY settings"));
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.kind == "calls" && reference.expression == "configure"));
    for expression in ["GET /users", "POST /api/orders"] {
        assert!(
            facts.references.iter().any(|reference| {
                reference.kind == "calls_endpoint" && reference.expression == expression
            }),
            "missing endpoint reference {expression}: {:?}",
            facts.references
        );
    }
    assert_eq!(
        routes,
        [
            "ANY settings",
            "DELETE /items",
            "GET /asserted",
            "GET /box",
            "GET /items",
            "GET /non-null",
            "GET /type-assertion",
            "HEAD /items",
            "OPTIONS /items",
            "PATCH /items",
            "PUT ^items$"
        ]
    );
    let items = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /items")
        .unwrap();
    assert_eq!(
        items.details["handlers"],
        serde_json::json!(["auth", "view"])
    );
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.kind == "calls"
            && reference.expression == "cache.get"
            && reference.line == 15));
    let decorated = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /box")
        .unwrap();
    let handler = facts.nodes.iter().find(|node| node.name == "show").unwrap();
    assert!(facts
        .edges
        .iter()
        .any(|edge| edge.src == decorated.id && edge.dst == handler.id && edge.kind == "handles"));
    let graph = linker::link(&BTreeMap::from([("routes.ts".to_owned(), facts.clone())]));
    let view = facts.nodes.iter().find(|node| node.name == "view").unwrap();
    for name in ["GET /asserted", "GET /non-null", "GET /type-assertion"] {
        let route = facts.nodes.iter().find(|node| node.name == name).unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|edge| edge.src == route.id && edge.dst == view.id && edge.kind == "handles"));
    }
}

#[cfg(feature = "lang-python")]
#[test]
fn python_member_access_callee_does_not_emit_duplicate_reference() {
    let source = r#"
class ServiceClient:
    def fetch(self, endpoint: str) -> dict:
        return self._send_request(endpoint)

    def _send_request(self, endpoint: str) -> dict:
        return {"status": "ok"}
"#;
    let facts = ast::extract("service.py", "python", source).expect("python extraction failed");
    let send_request_refs: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.expression == "self._send_request")
        .collect();
    assert_eq!(
        send_request_refs.len(),
        1,
        "Expected exactly 1 relation for self._send_request, got {:?}",
        send_request_refs
    );
    assert_eq!(send_request_refs[0].kind, "calls");
}
