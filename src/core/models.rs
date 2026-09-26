use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub qualname: String,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    pub is_test: bool,
    pub language: String,
    pub generated: bool,
    pub details: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub src: String,
    pub dst: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
    pub evidence: String,
    pub confidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub source: String,
    pub expression: String,
    pub kind: String,
    pub line: usize,
    pub alias: Option<String>,
    pub module: Option<String>,
    #[serde(default)]
    pub dynamic: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub path: String,
    pub line: usize,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSection {
    pub doc_id: String,
    pub path: String,
    pub section_title: String,
    pub doc_type: String,
    pub content: String,
    pub invariants: Vec<String>,
    pub referenced_symbols: Vec<String>,
    pub mtime: f64,
    pub size: u64,
    pub is_invariant: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Facts {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub references: Vec<Reference>,
    pub docs: Vec<DocSection>,
    pub errors: Vec<Diagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    pub path: String,
    pub line: usize,
    pub expression: String,
    pub status: String,
    pub evidence: String,
}
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub edges: Vec<Edge>,
    pub coverage: Vec<Coverage>,
}
pub fn module_name(path: &str) -> String {
    let path = path.rsplit_once('.').map_or(path, |(p, _)| p);
    path.trim_end_matches("/__init__")
        .trim_end_matches("/index")
        .replace('/', ".")
}
pub fn is_test(path: &str) -> bool {
    path.split('/').any(|p| p == "tests" || p == "test")
        || path.rsplit('/').next().is_some_and(|p| {
            p.starts_with("test_")
                || p.ends_with("_test.go")
                || p.contains(".test.")
                || p.contains(".spec.")
        })
}
