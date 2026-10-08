#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::ast;

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
