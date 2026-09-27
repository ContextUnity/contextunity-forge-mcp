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
        .any(|n| n.kind == "function" && n.name == "fetch"));
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
        .any(|n| n.kind == "function" && n.name == "new"));
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
