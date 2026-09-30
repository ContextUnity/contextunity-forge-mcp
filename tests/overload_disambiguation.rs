#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::{ast, linker},
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_overload_disambiguation_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn implicit_fields_does_not_create_phantom_fields_for_methods() {
    let python = r#"
class Category:
    def move(self, target):
        return target

    def process(self):
        self.move("up")
        self.real_field = 42
        self.move = "override"
"#;

    let facts = ast::extract("category.py", "python", python).unwrap();
    let move_fields: Vec<_> = facts
        .nodes
        .iter()
        .filter(|n| n.kind == "field" && n.name == "move")
        .collect();
    assert_eq!(
        move_fields.len(),
        0,
        "Method calls or assignments matching method name must NOT create phantom fields"
    );

    let real_field = facts
        .nodes
        .iter()
        .find(|n| n.kind == "field" && n.name == "real_field");
    assert!(real_field.is_some(), "Real assignment must create a field");
}

#[test]
fn overload_and_stub_node_details_flagged_correctly() {
    let python = r#"
from typing import overload, TYPE_CHECKING

class Service:
    @overload
    def run(self, flag: int) -> int: ...

    @overload
    def run(self, flag: str) -> str:
        pass

    def run(self, flag):
        return flag

if TYPE_CHECKING:
    def type_check_only_helper():
        pass
"#;

    let facts = ast::extract("service.py", "python", python).unwrap();
    let runs: Vec<_> = facts.nodes.iter().filter(|n| n.name == "run").collect();
    assert_eq!(runs.len(), 3);

    // Overloads have is_overload: true and is_stub: true
    assert_eq!(runs[0].details["is_overload"], true);
    assert_eq!(runs[0].details["is_stub"], true);
    assert_eq!(runs[1].details["is_overload"], true);
    assert_eq!(runs[1].details["is_stub"], true);

    // Runtime implementation does NOT have is_overload or is_stub
    assert!(runs[2].details.get("is_overload").is_none());
    assert!(runs[2].details.get("is_stub").is_none());

    // Helper inside TYPE_CHECKING has is_stub: true
    let helper = facts
        .nodes
        .iter()
        .find(|n| n.name == "type_check_only_helper")
        .unwrap();
    assert_eq!(helper.details["is_stub"], true);

    let ts = r#"
class TsService {
    compute(x: number): number;
    compute(x: string): string;
    compute(x: any): any {
        return x;
    }
}
"#;
    let ts_facts = ast::extract("service.ts", "typescript", ts).unwrap();
    let computes: Vec<_> = ts_facts.nodes.iter().filter(|n| n.name == "compute").collect();
    assert_eq!(computes.len(), 3);

    // Signatures without body have is_overload: true and is_stub: true
    assert_eq!(computes[0].details["is_overload"], true);
    assert_eq!(computes[0].details["is_stub"], true);
    assert_eq!(computes[1].details["is_overload"], true);
    assert_eq!(computes[1].details["is_stub"], true);

    // Method with body does NOT have is_overload or is_stub
    assert!(computes[2].details.get("is_overload").is_none());
    assert!(computes[2].details.get("is_stub").is_none());
}

#[test]
fn linker_resolves_calls_to_runtime_implementation_without_ambiguity() {
    let python_service = r#"
from typing import overload

@overload
def compute(x: int) -> int: ...

@overload
def compute(x: str) -> str: ...

def compute(x):
    return x

class Calculator:
    @overload
    def calc(self, x: int) -> int: ...

    @overload
    def calc(self, x: str) -> str: ...

    def calc(self, x):
        return x

    def run(self):
        self.calc(10)

def caller(c: Calculator):
    compute(10)
    c.calc(10)
"#;

    let facts = ast::extract("calc.py", "python", python_service).unwrap();
    let mut all = BTreeMap::new();
    all.insert("calc.py".to_string(), facts);

    let graph = linker::link(&all);

    // 1. Direct function call to overloaded function
    let compute_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "compute")
        .expect("Must have coverage for compute call");
    assert_eq!(
        compute_cov.status, "resolved",
        "Overloaded function call must resolve to runtime implementation"
    );

    // 2. Method call on self
    let self_calc_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "self.calc")
        .expect("Must have coverage for self.calc call");
    assert_eq!(
        self_calc_cov.status, "resolved",
        "self.calc call must resolve to runtime implementation"
    );

    // 3. Method call on typed parameter
    let client_calc_cov = graph
        .coverage
        .iter()
        .find(|c| c.expression == "c.calc")
        .expect("Must have coverage for c.calc call");
    assert_eq!(
        client_calc_cov.status, "resolved",
        "c.calc call on typed receiver must resolve to runtime implementation"
    );
}

#[test]
fn reader_disambiguation_pipeline_resolves_selectors() {
    let workspace = Workspace::new();
    let python = r#"
from typing import overload

class Category:
    @overload
    def move(self, dir: str) -> bool: ...

    def move(self, dir):
        # Full runtime implementation
        print("moving")
        return True

def caller(c: Category):
    c.move("left")
"#;

    workspace.write("cat.py", python);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();

    // Selecting "Category.move" must resolve directly to the runtime method without error
    let node = reader::select(&conn, "Category.move").expect("Category.move must resolve without ambiguous error");
    assert_eq!(node["name"], "move");
    assert_eq!(node["kind"], "method");
    assert_eq!(node["line"], 8, "Must resolve to runtime implementation on line 8");

    // Also inspect must succeed
    let inspected = reader::inspect(&conn, "Category.move", false).expect("inspect Category.move must succeed");
    assert_eq!(inspected["node"]["id"], node["id"]);
}
