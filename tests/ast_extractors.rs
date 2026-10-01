#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::ast;

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
    let mut routes: Vec<_> = facts.nodes.iter().filter(|node| node.kind == "route").map(|node| node.name.as_str()).collect();
    routes.sort_unstable();
    assert_eq!(routes, ["ANY ", "ANY ^items/", "ANY items/", "GET /items", "GET /multi", "PATCH /multi", "POST ^item$"]);
    let items = facts.nodes.iter().find(|node| node.name == "GET /items").unwrap();
    assert_eq!(items.details["handlers"], serde_json::json!(["auth", "handler"]));
    let ordinary_calls: Vec<_> = facts.references.iter().filter(|reference| reference.kind == "calls" && reference.expression == "config.get").map(|reference| reference.line).collect();
    assert_eq!(ordinary_calls, [12, 14]);
    for name in ["GET /multi", "PATCH /multi"] {
        let route = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let handler = facts.nodes.iter().find(|node| node.name == "decorated").unwrap();
        assert!(facts.edges.iter().any(|edge| edge.src == route.id && edge.dst == handler.id && edge.kind == "handles"));
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_routes_preserve_middleware_and_framework_decorators_with_finite_receivers() {
    let source = "function auth() {}\nfunction view() {}\napp.get('/items', auth, view);\nserver.put('^items$', view);\napi.delete('/items', view);\nblueprint.patch('/items', view);\nroute.options('/items', view);\nrouter.head('/items', view);\nconst routes = [{path: 'settings', component: view}];\nclass Controller { @Get('/box') show() {} }\ncache.get('/setting', view);\napp.get('setting', view);\nget('/setting', view);\napp.get('/number', 0);\napp.get('/negative', -1);\napp.get('/boolean', true);\napp.get('/null', null);\napp.get('/string', 'view');\napp.get('/template', `view`);\napp.get('/array', [view]);\napp.get('/object', {view});\nconst values = [{path: '/primitive', component: false}];\n";
    let facts = ast::extract("routes.ts", "typescript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let mut routes: Vec<_> = facts.nodes.iter().filter(|node| node.kind == "route").map(|node| node.name.as_str()).collect();
    routes.sort_unstable();
    assert_eq!(routes, ["ANY settings", "DELETE /items", "GET /box", "GET /items", "HEAD /items", "OPTIONS /items", "PATCH /items", "PUT ^items$"]);
    let items = facts.nodes.iter().find(|node| node.name == "GET /items").unwrap();
    assert_eq!(items.details["handlers"], serde_json::json!(["auth", "view"]));
    assert!(facts.references.iter().any(|reference| reference.kind == "calls" && reference.expression == "cache.get" && reference.line == 11));
    let decorated = facts.nodes.iter().find(|node| node.name == "GET /box").unwrap();
    let handler = facts.nodes.iter().find(|node| node.name == "show").unwrap();
    assert!(facts.edges.iter().any(|edge| edge.src == decorated.id && edge.dst == handler.id && edge.kind == "handles"));
}
