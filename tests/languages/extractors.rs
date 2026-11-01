#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::{ast, languages::rust, linker};
#[cfg(feature = "lang-rust")]
use forge_index::languages::rust::kind;
#[cfg(all(feature = "lang-rust", not(test)))]
use forge_index::linker::rust::resolution::is_registered_cargo_crate;

#[cfg(feature = "lang-rust")]
#[cfg(test)]
fn is_registered_cargo_crate(crate_name: &str) -> bool {
    matches!(
        crate_name,
        "rusqlite"
            | "tree-sitter"
            | "tree_sitter"
            | "serde"
            | "serde_json"
            | "tokio"
            | "anyhow"
            | "clap"
            | "tracing"
            | "rayon"
            | "tempfile"
            | "regex"
            | "thiserror"
            | "parking_lot"
            | "crossbeam"
            | "log"
    )
}
#[cfg(feature = "lang-rust")]
use forge_index::linker::value_flow::ValueFlowIndex;

#[cfg(feature = "lang-rust")]
struct SeamValueFlowResolver;

#[cfg(feature = "lang-rust")]
impl<'a> forge_index::linker::value_flow::SemanticResolver<'a> for SeamValueFlowResolver {
    fn resolve_symbol(
        &self,
        _scope: &'a contextunity_forge_mcp::core::models::Node,
        _name: &str,
        _at: contextunity_forge_mcp::core::semantic::SourcePosition,
        _role: forge_index::linker::value_flow::SymbolRole,
    ) -> forge_index::linker::value_flow::Symbol<'a> {
        forge_index::linker::value_flow::Symbol::Unknown
    }
}

#[cfg(feature = "lang-python")]
mod scope_allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Probe;

    thread_local! {
        static SELECTED: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    #[global_allocator]
    static ALLOCATOR: Probe = Probe;

    fn record(size: usize, align: usize, pointer: *mut u8) {
        if align == 1 && !pointer.is_null() {
            let _ = SELECTED.try_with(|selected| {
                let (expected, count) = selected.get();
                if expected != 0 && size == expected {
                    selected.set((expected, count + 1));
                }
            });
        }
    }

    unsafe impl GlobalAlloc for Probe {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, size) };
            record(size, layout.align(), pointer);
            pointer
        }
    }

    pub(super) fn measure<T>(size: usize, operation: impl FnOnce() -> T) -> (T, usize) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                SELECTED.with(|selected| selected.set((0, 0)));
            }
        }
        SELECTED.with(|selected| {
            assert_eq!(selected.get().0, 0);
            selected.set((size, 0));
        });
        let reset = Reset;
        let result = operation();
        let count = SELECTED.with(|selected| selected.get().1);
        drop(reset);
        (result, count)
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_ast_extractor() {
    let empty_catalog = std::collections::BTreeMap::new();
    let seam_resolver = SeamValueFlowResolver;
    let _seam_index =
        ValueFlowIndex::build_with_limits(&empty_catalog, &seam_resolver, Default::default());
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
    assert!(constructor.details.get("is_method").is_none());
    assert_eq!(constructor.details["is_static"], true);

    #[cfg(feature = "lang-rust")]
    {
        let rust_kind = kind("function_item");
        assert_eq!(rust_kind, Some("function"));
        let _ = rust::language();
        let _ = is_registered_cargo_crate("rusqlite");
    }

    // Contract baseline for LS-001: Rust std prelude trait resolution and declarative manifests.
    // Standard prelude traits (Clone, Default, Iterator) must resolve or classify as external via rust-std.toml.
    let std_source = r#"
fn run() {
    let _default: Result<i32, ()> = Result::default();
    let text = String::from("hello");
    let _cloned = text.clone();
    let _default_trait: i32 = Default::default();
    let numbers = Vec::new();
    let _iter = numbers.iter();
}
"#;
    let std_facts =
        ast::extract("std_prelude.rs", "rust", std_source).expect("std_prelude extraction failed");
    let std_graph = linker::link(&std::collections::BTreeMap::from([(
        "std_prelude.rs".to_owned(),
        std_facts,
    )]));
    let default_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Result::default")
        .expect("missing coverage for Result::default");
    assert_eq!(
        default_coverage.status, "unresolved",
        "Result does not implement Default: {default_coverage:#?}"
    );
    let string_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "String::from")
        .expect("missing coverage for String::from");
    assert_eq!(
        string_coverage.status, "external",
        "String::from must resolve to external Rust standard library prelude via rust-std.toml: {string_coverage:#?}"
    );
    let clone_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "text.clone")
        .expect("missing coverage for text.clone");
    assert_eq!(
        clone_coverage.status, "external",
        "text.clone must resolve to external Rust standard library prelude via rust-std.toml: {clone_coverage:#?}"
    );
    let default_trait_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Default::default")
        .expect("missing coverage for Default::default");
    assert_eq!(
        default_trait_coverage.status, "external",
        "Default::default must resolve to external Rust standard library prelude via rust-std.toml: {default_trait_coverage:#?}"
    );
    let vec_new_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Vec::new")
        .expect("missing coverage for Vec::new");
    assert_eq!(
        vec_new_coverage.status, "external",
        "Vec::new must resolve to external: {vec_new_coverage:#?}"
    );
    let iter_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "numbers.iter")
        .expect("missing coverage for numbers.iter");
    assert_eq!(
        iter_coverage.status, "external",
        "numbers.iter must resolve to external: {iter_coverage:#?}"
    );

    // Verify Cargo third-party ecosystem crate routing (Task 2 contract)
    let cargo_source = r#"
use rusqlite::Connection;
use tree_sitter::Parser;

fn run_cargo_crates() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("SELECT 1", []).unwrap();
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
}
"#;
    // Cargo authority follows the imported identity and declared member, never a
    // homonymous short type name or an assumed constructor convention.
    for (label, import, constructor, member, authorized) in [
        (
            "canonical",
            "use rusqlite::Connection;",
            "Connection::open_in_memory()",
            "execute",
            true,
        ),
        (
            "alias",
            "use rusqlite::Connection as Db;",
            "Db::open_in_memory()",
            "execute",
            true,
        ),
        (
            "declared",
            "use rusqlite::Connection;",
            "Connection::open_with_flags(\"db\", flags)",
            "execute",
            true,
        ),
        (
            "homonym",
            "use unregistered_crate::Connection;",
            "Connection::open_in_memory()",
            "execute",
            false,
        ),
        (
            "suffix",
            "use unregistered_crate::rusqlite::Connection;",
            "Connection::open_in_memory()",
            "execute",
            false,
        ),
        (
            "undeclared",
            "use rusqlite::Connection;",
            "Connection::with_capacity(12)",
            "execute",
            false,
        ),
        (
            "std-file-result",
            "use std::fs::File;",
            "File::open(\"input\")",
            "metadata",
            true,
        ),
        (
            "builtin-string-result",
            "use std::str::FromStr;",
            "String::from_str(\"input\")",
            "len",
            true,
        ),
        (
            "imported-string-result",
            "use std::string::String as Text; use std::str::FromStr;",
            "Text::from_str(\"input\")",
            "len",
            true,
        ),
        (
            "tempfile-named",
            "use tempfile::NamedTempFile;",
            "NamedTempFile::new()",
            "path",
            true,
        ),
        (
            "clap-command",
            "use clap::Command;",
            "Command::new(\"app\")",
            "get_matches",
            true,
        ),
        (
            "regex-match",
            "use regex::Regex;",
            "Regex::new(\"pattern\")",
            "is_match",
            true,
        ),
    ] {
        let source =
            format!("{import}\nfn run() {{ let conn = {constructor}.unwrap(); conn.{member}(); }}");
        for path in ["identity.rs", "src/identity.rs"] {
            let facts = ast::extract(path, "rust", &source).unwrap();
            let graph = linker::link(&std::collections::BTreeMap::from([(path.into(), facts)]));
            let coverage = graph
                .coverage
                .iter()
                .find(|row| row.expression == format!("conn.{member}"))
                .unwrap();
            assert_eq!(
                coverage.status == "external",
                authorized,
                "{label} at {path}: {coverage:#?}"
            );
            if authorized {
                assert_eq!(
                    coverage.status, "external",
                    "{label} at {path}: {coverage:#?}"
                );
            }
        }
    }
    // Prove negative isolation: conflicting imported alias and explicitly local module shadowing
    for (label, source, expected_expr) in [
        (
            "duplicate-rust-alias",
            "use regex::Regex as Re; use unregistered_crate::Other as Re; fn run() { let r = Re::new(\"x\").unwrap(); r.is_match(\"x\"); }",
            "r.is_match",
        ),
        (
            "local-missing-type",
            "mod regex {} fn run(r: regex::Missing) { r.is_match(\"x\"); }",
            "r.is_match",
        ),
    ] {
        for path in ["negative.rs", "src/negative.rs"] {
            let facts = ast::extract(path, "rust", source).unwrap();
            let graph = linker::link(&std::collections::BTreeMap::from([(path.into(), facts)]));
            let coverage = graph.coverage.iter().find(|row| row.expression == expected_expr);
            assert!(
                coverage.is_none() || coverage.unwrap().status == "unresolved",
                "{label} at {path}: expected unresolved, got {coverage:#?}"
            );
        }
    }

    let cargo_facts = ast::extract("cargo_test.rs", "rust", cargo_source).unwrap();
    let cargo_graph = linker::link(&std::collections::BTreeMap::from([(
        "cargo_test.rs".to_owned(),
        cargo_facts,
    )]));
    let execute_cov = cargo_graph
        .coverage
        .iter()
        .find(|c| c.expression == "conn.execute")
        .expect("missing coverage for conn.execute");
    assert_eq!(
        execute_cov.status, "external",
        "conn.execute must resolve to external via rust-cargo.toml declarative pack: {execute_cov:#?}"
    );
    assert!(
        execute_cov.evidence.contains("rusqlite::Connection")
            && execute_cov
                .evidence
                .contains("receiver origin statically inferred"),
        "conn.execute must retain its resolved import origin: {execute_cov:#?}"
    );
    let parser_cov = cargo_graph
        .coverage
        .iter()
        .find(|c| c.expression == "parser.set_language")
        .expect("missing coverage for parser.set_language");
    assert_eq!(
        parser_cov.status, "external",
        "parser.set_language must resolve to external via rust-cargo.toml declarative pack: {parser_cov:#?}"
    );
    assert!(
        parser_cov.evidence.contains("tree_sitter::Parser")
            && parser_cov
                .evidence
                .contains("receiver origin statically inferred"),
        "parser.set_language must retain its resolved import origin: {parser_cov:#?}"
    );

    let iso_source = r#"
fn check_string_trait_isolation() {
    let s = String::new();
    s.clone();
    s.next();
}
"#;
    let iso_facts = ast::extract("iso_test.rs", "rust", iso_source).unwrap();
    let iso_graph = linker::link(&std::collections::BTreeMap::from([(
        "iso_test.rs".to_owned(),
        iso_facts,
    )]));
    let next_cov = iso_graph
        .coverage
        .iter()
        .find(|c| c.expression == "s.next")
        .expect("missing coverage for s.next");
    assert_eq!(
        next_cov.status, "unresolved",
        "s.next must remain unresolved because String does not implement Iterator: {next_cov:#?}"
    );
    for (adapter, preserves_item) in [
        ("filter(|_| true)", true),
        ("take(1)", true),
        ("skip(1)", true),
        ("step_by(1)", true),
        ("collect::<Vec<_>>().into_iter()", true),
        ("map(|_| B)", false),
        ("map(convert)", false),
        ("enumerate()", false),
        ("map(|item| item)", false),
    ] {
        let source = format!(
            "struct A; impl A {{ fn finish(&self) {{}} }}
             struct B; impl B {{ fn finish(&self) {{}} }}
             fn convert(_: A) -> B {{ B }}
             trait Finish {{ fn finish(&self); }}
             impl Finish for (usize, A) {{ fn finish(&self) {{}} }}
             fn run(items: Vec<A>) {{
                 let mut adapted = items.into_iter().{adapter};
                 let item = adapted.next().unwrap();
                 item.finish();
             }}"
        );
        let facts = ast::extract("iterator.rs", "rust", &source).unwrap();
        assert!(facts.errors.is_empty(), "{adapter}: {facts:#?}");
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "finish" && node.details["receiver_type"] == "A")
            .unwrap()
            .id
            .clone();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "iterator.rs".to_owned(),
            facts,
        )]));
        assert_eq!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == "item.finish"
                && edge.dst == target),
            preserves_item,
            "{adapter}: {graph:#?}"
        );
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == "item.finish")
            .unwrap();
        assert_eq!(
            coverage.status,
            if preserves_item {
                "resolved"
            } else {
                "unresolved"
            },
            "{adapter}: {coverage:#?}"
        );
    }

    // Task 3 Contract: Declarative macro classification and bounded receiver repair (LS-001)
    // 1. Declarative standard and Cargo macro classification: format!, json!, bail!, ensure!, vec!, write!
    let macro_contract_source = r#"
use anyhow::bail;
use anyhow::ensure;
use serde_json::json;

fn run_macros() {
    let _f = format!("hello {}", 42);
    let _v = vec![1, 2, 3];
    let _w = write!(f, "data");
    let _j = json!({ "key": "value" });
    let _b = bail!("operation failed");
    let _e = ensure!(true);
    let _qb = anyhow::bail!("qualified bail");
    let _qe = anyhow::ensure!(true);
    let _qj = serde_json::json!({ "status": 200 });
}
"#;
    let macro_facts = ast::extract("macros.rs", "rust", macro_contract_source).unwrap();
    let macro_graph = linker::link(&std::collections::BTreeMap::from([(
        "macros.rs".to_owned(),
        macro_facts,
    )]));
    for macro_name in [
        "format",
        "vec",
        "write",
        "json",
        "bail",
        "ensure",
        "anyhow::bail",
        "anyhow::ensure",
        "serde_json::json",
    ] {
        let cov = macro_graph
            .coverage
            .iter()
            .find(|c| c.expression == macro_name)
            .unwrap_or_else(|| panic!("missing coverage for macro {macro_name}"));
        assert_ne!(
            cov.status, "unresolved",
            "macro {macro_name} must not be unresolved: {cov:#?}"
        );
        assert_eq!(
            cov.status, "external",
            "macro {macro_name} status mismatch: {cov:#?}"
        );
    }

    // Unimported Cargo ecosystem macros (json!, bail!, ensure!) must classify via declarative macro signatures
    let unimported_macro_source = r#"
fn run_unimported_macros() {
    let _j = json!({ "key": "value" });
    let _b = bail!("operation failed");
    let _e = ensure!(true);
}
"#;
    let unimported_facts =
        ast::extract("unimported_macros.rs", "rust", unimported_macro_source).unwrap();
    let unimported_graph = linker::link(&std::collections::BTreeMap::from([(
        "unimported_macros.rs".to_owned(),
        unimported_facts,
    )]));
    for name in ["json", "bail", "ensure"] {
        let cov = unimported_graph
            .coverage
            .iter()
            .find(|c| c.expression == name)
            .unwrap_or_else(|| panic!("missing coverage for unimported {name}"));
        assert_ne!(
            cov.status, "unresolved",
            "unimported macro {name} must be classified through declarative macro signatures: {cov:#?}"
        );
    }

    // Imported local macros (named, renamed, crate) must not manufacture Vec/String stdlib receiver provenance
    for name in ["vec", "format"] {
        for (label, import, definition) in [
            (
                "named",
                format!("use local::{name};"),
                format!("macro_rules! {name} {{ () => {{ A }}; }} pub(crate) use {name};"),
            ),
            (
                "renamed",
                format!("use local::custom as {name};"),
                "macro_rules! custom { () => { A }; } pub(crate) use custom;".to_owned(),
            ),
            (
                "crate",
                format!("use crate::local::{name};"),
                format!("macro_rules! {name} {{ () => {{ A }}; }} pub(crate) use {name};"),
            ),
        ] {
            let source = format!(
                "struct A; mod local {{ {definition} }} {import} fn run() {{ let item = {name}!(); item.len(); }}"
            );
            let facts = ast::extract("probe.rs", "rust", &source).unwrap();
            assert!(
                facts.errors.is_empty(),
                "{name}/{label}: {:?}",
                facts.errors
            );
            let graph = linker::link(&std::collections::BTreeMap::from([(
                "probe.rs".to_owned(),
                facts,
            )]));
            let row = graph
                .coverage
                .iter()
                .find(|r| r.expression == "item.len")
                .unwrap();
            assert_ne!(
                row.status, "external",
                "local imported macro {name}/{label} fabricated stdlib type: {row:?}"
            );
        }
    }

    // 2. Bounded iterator receiver propagation: chained .iter().filter().map() must bind intermediate receivers and resolve .collect()
    let chain_source = r#"
fn run_iterator_chain() {
    let numbers = Vec::new();
    let _collected = numbers.into_iter().filter(|x| true).map(|x| x).collect();
    let _deep = numbers.into_iter().filter(|_| true).map(|x| x).take(5).enumerate().skip(1).step_by(2).collect();
    let _from_macro = vec![1, 2, 3].into_iter().filter(|x| true).map(|x| x).collect();
}
"#;
    let chain_facts = ast::extract("chain.rs", "rust", chain_source).unwrap();
    let chain_graph = linker::link(&std::collections::BTreeMap::from([(
        "chain.rs".to_owned(),
        chain_facts,
    )]));
    for expr in [
        "numbers.into_iter().filter(|x| true).map(|x| x).collect",
        "numbers.into_iter().filter(|_| true).map(|x| x).take(5).enumerate().skip(1).step_by(2).collect",
        "vec![1, 2, 3].into_iter().filter(|x| true).map(|x| x).collect",
    ] {
        let collect_cov = chain_graph
            .coverage
            .iter()
            .find(|c| c.expression == expr)
            .unwrap_or_else(|| panic!("missing coverage for {expr}"));
        assert_ne!(
            collect_cov.status, "unresolved",
            "collect() on chained iterator receiver must not be unresolved: {collect_cov:#?}"
        );
    }

    // Verify R2 remediation: ordinary function calls json() without macro ! must remain unresolved
    let fn_call_source = r#"
fn run_unresolved_function() {
    let _call = json();
}
"#;
    let fn_call_facts = ast::extract("fn_call.rs", "rust", fn_call_source).unwrap();
    let fn_call_graph = linker::link(&std::collections::BTreeMap::from([(
        "fn_call.rs".to_owned(),
        fn_call_facts,
    )]));
    let fn_cov = fn_call_graph
        .coverage
        .iter()
        .find(|c| c.expression == "json")
        .expect("missing coverage for json call");
    assert_eq!(
        fn_cov.status, "unresolved",
        "ordinary function call json() must remain unresolved: {fn_cov:#?}"
    );

    // Verify R1 boundary: 7 and 8 calls propagate, while 9+ calls exhaust and leave collect() unresolved
    let over_eight_source = r#"
fn run_over_eight_chain() {
    let numbers = Vec::new();
    let _call7 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).collect();
    let _call8 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).collect();
    let _over9 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).collect();
    let _over10 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect();
    let _over19 = numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect();
}
"#;
    let over_eight_facts = ast::extract("over_eight.rs", "rust", over_eight_source).unwrap();
    let over_eight_graph = linker::link(&std::collections::BTreeMap::from([(
        "over_eight.rs".to_owned(),
        over_eight_facts,
    )]));
    for (expr, expected) in [
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).collect", "external"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).collect", "external"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
    ] {
        let cov = over_eight_graph
            .coverage
            .iter()
            .find(|c| c.expression == expr)
            .unwrap_or_else(|| panic!("missing coverage for {expr}"));
        assert_eq!(
            cov.status, expected,
            "chain call {expr} expected status {expected}: {cov:#?}"
        );
    }

    // Verify R4: local take() returning another type must NOT preserve original receiver across iterator spellings
    for iter_name in ["iter", "into_iter", "iter_mut"] {
        for adapter in ["take(1)", "skip(1)", "step_by(1)", "filter(|_| true)"] {
            let r4_source = format!(
                "struct A; struct B;
                 impl A {{
                     fn {iter_name}(&self) -> A {{ A }}
                     fn take(&self, _: usize) -> B {{ B }}
                     fn skip(&self, _: usize) -> B {{ B }}
                     fn step_by(&self, _: usize) -> B {{ B }}
                     fn filter(&self, _: fn(&A) -> bool) -> B {{ B }}
                     fn finish(&self) {{}}
                 }}
                 fn run(a: A) {{
                     let item = a.{iter_name}().{adapter};
                     item.finish();
                 }}"
            );
            let r4_facts = ast::extract("r4.rs", "rust", &r4_source).unwrap();
            let r4_graph = linker::link(&std::collections::BTreeMap::from([(
                "r4.rs".to_owned(),
                r4_facts,
            )]));
            let r4_cov = r4_graph
                .coverage
                .iter()
                .find(|c| c.expression == "item.finish")
                .expect("missing coverage for item.finish");
            assert_ne!(
                r4_cov.status, "resolved",
                "item.finish on non-iterator B from {iter_name}.{adapter} must not resolve to A.finish: {r4_cov:#?}"
            );
        }
    }

    // Verify R2: local macro_rules! definition must preserve local identity and not become external serde_json
    let local_macro_source = r#"
macro_rules! json {
    () => { 1 };
}
fn run_local_macro() {
    let _val = json!();
}
"#;
    let local_macro_facts = ast::extract("local_macro.rs", "rust", local_macro_source).unwrap();
    let local_macro_graph = linker::link(&std::collections::BTreeMap::from([(
        "local_macro.rs".to_owned(),
        local_macro_facts,
    )]));
    let local_cov = local_macro_graph
        .coverage
        .iter()
        .find(|c| c.expression == "json")
        .expect("missing coverage for local json call");
    assert_ne!(
        local_cov.status, "external",
        "local macro json!() must not be classified as external serde_json: {local_cov:#?}"
    );

    // Verify R2: standard macro spellings called as ordinary functions must remain unresolved
    let std_fn_calls_source = r#"
fn run_std_fn_calls() {
    let _f = format();
    let _v = vec();
    let _w = write();
    let _p = println();
}
"#;
    let std_fn_calls_facts = ast::extract("std_fn_calls.rs", "rust", std_fn_calls_source).unwrap();
    let std_fn_calls_graph = linker::link(&std::collections::BTreeMap::from([(
        "std_fn_calls.rs".to_owned(),
        std_fn_calls_facts,
    )]));
    for name in ["format", "vec", "write", "println"] {
        let std_cov = std_fn_calls_graph
            .coverage
            .iter()
            .find(|c| c.expression == name)
            .unwrap_or_else(|| panic!("missing coverage for std fn call {name}"));
        assert_eq!(
            std_cov.status, "unresolved",
            "ordinary function call {name}() must remain unresolved: {std_cov:#?}"
        );
    }

    // Verify R2 boundary tests: local macro imports/aliases precede external names
    for (label, source, expression) in [
        ("qualified", "mod serde_json { macro_rules! json { () => { 1 }; } pub(crate) use json; } fn run() { serde_json::json!(); }", "serde_json::json"),
        ("import", "mod local { macro_rules! json { () => { 1 }; } pub(crate) use json; } use local::json; fn run() { json!(); }", "json"),
        ("renamed-import", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } use local::custom as json; fn run() { json!(); }", "json"),
        ("crate-import", "mod local { macro_rules! json { () => { 1 }; } pub(crate) use json; } use crate::local::json; fn run() { json!(); }", "json"),
        ("module-alias", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom as json; } use local as serde_json; fn run() { serde_json::json!(); }", "serde_json::json"),
        ("parent-module", "mod outer { macro_rules! json { () => { 1 }; } mod inner { fn run() { json!(); } } }", "json"),
        ("parent-control", "mod outer { macro_rules! custom { () => { 1 }; } mod inner { fn run() { custom!(); } } }", "custom"),
    ] {
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{label}: {:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([("probe.rs".to_owned(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == expression).unwrap();
        assert_ne!(row.status, "external", "{label}: local macro misclassified external: {row:?}");
        assert_eq!(row.status, "resolved", "{label}: local macro should resolve locally: {row:?}");
    }

    // Verify R2-output: local macro result must not fabricate stdlib type
    for name in ["vec", "format"] {
        let source = format!("struct A; impl A {{ fn finish(&self) {{}} }} macro_rules! {name} {{ () => {{ A }}; }} fn run() {{ let item = {name}!(); item.len(); }}");
        let facts = ast::extract("probe.rs", "rust", &source).unwrap();
        assert!(facts.errors.is_empty(), "{:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "probe.rs".to_owned(),
            facts,
        )]));
        let row = graph
            .coverage
            .iter()
            .find(|r| r.expression == "item.len")
            .unwrap();
        assert_ne!(
            row.status, "external",
            "local {name} result must not fabricate stdlib type: {row:?}"
        );
    }

    // Verify qualified logging macro providers come from TOML
    {
        let source = "fn run() { tracing::info!(\"hello\"); log::info!(\"hello\"); }";
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "probe.rs".to_owned(),
            facts,
        )]));
        for provider in ["tracing", "log"] {
            let expression = format!("{provider}::info");
            let row = graph
                .coverage
                .iter()
                .find(|r| r.expression == expression)
                .unwrap();
            assert_eq!(row.status, "external");
            assert!(row.evidence.contains(&format!("macro {provider}::")));
        }
    }

    // Verify Review 42: nonexistent import member does not select different member
    for (label, source, expression) in [
        ("direct-import", "mod local { pub fn available() {} } use local::missing; fn run() { missing(); }", "missing"),
        ("module-alias", "mod local { pub fn available() {} } use local as alias; fn run() { alias::missing(); }", "alias::missing"),
        ("macro-member", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } use local as serde_json; fn run() { serde_json::json!(); }", "serde_json::json"),
    ] {
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{label}: {:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([("probe.rs".to_owned(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == expression).unwrap();
        assert_ne!(row.status, "resolved", "{label}: invented member should not be resolved: {row:?}");
    }

    // Verify Review 42: scoped macro import does not leak to unrelated outer scope
    {
        let source = "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } mod child { use crate::local::custom as json; fn scoped() { json!(); } }\nfn run() { json!(); }";
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "probe.rs".to_owned(),
            facts,
        )]));
        let row = graph
            .coverage
            .iter()
            .filter(|r| r.expression == "json")
            .max_by_key(|r| r.line)
            .unwrap();
        assert_ne!(
            row.status, "resolved",
            "child import must not supply exact target for outer json!"
        );
    }

    // Verify Review 42: unrelated macro or value import does not suppress std macro result
    for (label, source) in [
        ("sibling-macro", "mod other { macro_rules! vec { () => { 1 }; } } fn run() { let item = vec![1]; item.len(); }"),
        ("value-namespace", "use std::fmt::format; fn run() { let item = format!(\"hello\"); item.len(); }"),
        ("later-macro", "fn run() { let item = vec![1]; item.len(); } macro_rules! vec { () => { 1 }; }"),
        ("foreign-function", "mod local { extern \"C\" { pub fn custom(); } } use local::custom as vec; fn run() { let item = vec![1]; item.len(); }"),
        ("foreign-static", "mod local { extern \"C\" { pub static custom: u8; } } use local::custom as format; fn run() { let item = format!(\"hello\"); item.len(); }"),
        ("extern-crate", "mod local { pub extern crate core as custom; } use local::custom as vec; fn run() { let item = vec![1]; item.len(); }"),
    ] {
        let facts = ast::extract("probe.rs", "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{label}: {:?}", facts.errors);
        let graph = linker::link(&std::collections::BTreeMap::from([("probe.rs".to_owned(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == "item.len").unwrap();
        assert_eq!(row.status, "external", "{label}: proven std macro output suppressed: {row:?}");
    }

    // Seam Contract: closure-inference-and-tuple-destructuring (LS-001 Task 5)
    // 1. Tuple destructuring: let (file, code) = (NamedTempFile::new().unwrap(), 42); file.path();
    // 2. Additional workspace Cargo crates: WalkDir::new(".").into_iter();
    // 3. Closure parameter type inference: query_map(..., |row| row.get(...))
    assert!(
        contextunity_forge_mcp::engine::languages::rust::is_registered_rust_type(
            "walkdir::WalkDir"
        )
    );
    let seam_source = r#"
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
    let seam_facts = ast::extract("src/seam_destructuring.rs", "rust", seam_source).unwrap();
    let ast_engine_facts = ast::extract(
        "src/engine/ast.rs",
        "rust",
        "pub fn extract(_path: &str, _lang: &str, _code: &str) {}",
    )
    .unwrap();
    let seam_graph = linker::link(&std::collections::BTreeMap::from([
        ("src/seam_destructuring.rs".into(), seam_facts),
        ("src/engine/ast.rs".into(), ast_engine_facts),
    ]));
    let file_path_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression == "file.path")
        .expect("missing coverage for file.path");
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
    let stmt_exec_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression == "stmt.execute")
        .expect("missing coverage for stmt.execute");
    assert_eq!(
        stmt_exec_cov.status, "external",
        "stmt.execute from if-let Ok(mut stmt) pattern binding must resolve to external: {stmt_exec_cov:#?}"
    );
    let ast_extract_cov = seam_graph
        .coverage
        .iter()
        .find(|c| c.expression == "ast::extract")
        .expect("missing coverage for ast::extract");
    assert_eq!(
        ast_extract_cov.status, "external",
        "ast::extract without owning Cargo manifest and indexed src/lib.rs must remain external: {ast_extract_cov:#?}"
    );

    // Proven manifest-bound root crate normalization
    let temp_root =
        std::env::temp_dir().join(format!("forge_test_root_crate_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();
    std::fs::write(
        temp_root.join("Cargo.toml"),
        r#"[package]
name = "my_root_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(temp_root.join("src")).unwrap();
    std::fs::write(temp_root.join("src/lib.rs"), "pub mod engine;").unwrap();

    let root_caller_source = r#"
        use my_root_crate::engine::ast;
        fn run() {
            let _ = ast::extract("test.rs", "rust", "fn main() {}");
        }
    "#;
    let caller_facts = ast::extract("src/main.rs", "rust", root_caller_source).unwrap();
    let engine_ast_facts = ast::extract(
        "src/engine/ast.rs",
        "rust",
        "pub fn extract(_path: &str, _lang: &str, _code: &str) {}",
    )
    .unwrap();
    let lib_facts = ast::extract("src/lib.rs", "rust", "pub mod engine;").unwrap();
    let manifest_graph = linker::link_with_root(
        &std::collections::BTreeMap::from([
            ("src/main.rs".into(), caller_facts),
            ("src/engine/ast.rs".into(), engine_ast_facts),
            ("src/lib.rs".into(), lib_facts),
        ]),
        None,
        Some(&temp_root),
    );
    let resolved_cov = manifest_graph
        .coverage
        .iter()
        .find(|c| c.expression == "ast::extract")
        .expect("missing coverage for ast::extract with manifest");
    assert_eq!(
        resolved_cov.status, "resolved",
        "ast::extract with valid manifest and src/lib.rs must resolve locally: {resolved_cov:#?}"
    );
    let _ = std::fs::remove_dir_all(&temp_root);

    // Probe tests for LS001-T5 review findings R3, R4, R5
    // 1. Iterator closure inference from receiver
    let iter_source = r#"
        struct Item; impl Item { fn finish(&self) {} }
        fn f(items: Vec<Item>) {
            items.iter().map(|entry| entry.finish());
            items.iter().filter(|entry| { entry.finish(); true });
        }
    "#;
    let iter_facts = ast::extract("src/probe_iter.rs", "rust", iter_source).unwrap();
    let iter_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_iter.rs".into(),
        iter_facts,
    )]));
    for c in iter_graph
        .coverage
        .iter()
        .filter(|c| c.expression == "entry.finish")
    {
        assert_eq!(
            c.status, "resolved",
            "iterator entry.finish must resolve: {c:#?}"
        );
    }

    // 2. Tuple-return positional binding
    let tuple_ret_source = r#"
        struct A; impl A { fn finish(&self) {} }
        struct B; impl B { fn close(&self) {} }
        fn make() -> (A, B) { (A, B) }
        fn f() { let (a, b) = make(); a.finish(); b.close(); }
    "#;
    let tuple_ret_facts = ast::extract("src/probe_tup_ret.rs", "rust", tuple_ret_source).unwrap();
    let tuple_ret_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tup_ret.rs".into(),
        tuple_ret_facts,
    )]));
    let a_finish = tuple_ret_graph
        .coverage
        .iter()
        .find(|c| c.expression == "a.finish")
        .expect("a.finish");
    assert_eq!(
        a_finish.status, "resolved",
        "a.finish from tuple return must resolve: {a_finish:#?}"
    );
    let b_close = tuple_ret_graph
        .coverage
        .iter()
        .find(|c| c.expression == "b.close")
        .expect("b.close");
    assert_eq!(
        b_close.status, "resolved",
        "b.close from tuple return must resolve: {b_close:#?}"
    );

    // 3. Tuple-rest positional binding
    let tuple_rest_source = r#"
        struct A;
        struct B; impl B { fn finish(&self) {} }
        struct C; impl C { fn finish(&self) {} }
        fn f() { let (.., last) = (A {}, B {}, C {}); last.finish(); }
    "#;
    let tuple_rest_facts =
        ast::extract("src/probe_tup_rest.rs", "rust", tuple_rest_source).unwrap();
    let tuple_rest_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tup_rest.rs".into(),
        tuple_rest_facts,
    )]));
    let last_finish = tuple_rest_graph
        .coverage
        .iter()
        .find(|c| c.expression == "last.finish")
        .expect("last.finish");
    assert_eq!(
        last_finish.status, "resolved",
        "last.finish must resolve: {last_finish:#?}"
    );
    assert!(
        last_finish.evidence.contains("src.probe_tup_rest.C.finish"),
        "last.finish must resolve to C.finish, not B.finish: {last_finish:#?}"
    );

    // 4. Closure-homonym negative (unrelated callback must not resolve to rusqlite::Row)
    let closure_homonym_source = r#"
        fn unrelated<F: Fn(i32)>(callback: F) { callback(1); }
        fn f() { unrelated(|row| row.get(0)); }
    "#;
    let closure_homonym_facts =
        ast::extract("src/probe_closure_neg.rs", "rust", closure_homonym_source).unwrap();
    let closure_homonym_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_closure_neg.rs".into(),
        closure_homonym_facts,
    )]));
    let row_get = closure_homonym_graph
        .coverage
        .iter()
        .find(|c| c.expression == "row.get")
        .expect("row.get");
    assert_ne!(
        row_get.status, "external",
        "unrelated callback named row must NOT resolve to rusqlite::Row: {row_get:#?}"
    );

    // 5. Connection and statement query_map closure parameter inference
    let conn_source = r#"
        use rusqlite::Connection;
        fn f(conn: &Connection) {
            let mut statement = conn.prepare("SELECT 1").unwrap();
            statement.execute([]);
            statement.query_map([], |entry| entry.get(0));
        }
    "#;
    let conn_facts = ast::extract("src/probe_conn.rs", "rust", conn_source).unwrap();
    let conn_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_conn.rs".into(),
        conn_facts,
    )]));
    assert_eq!(
        conn_graph
            .coverage
            .iter()
            .find(|c| c.expression == "conn.prepare")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        conn_graph
            .coverage
            .iter()
            .find(|c| c.expression == "statement.execute")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        conn_graph
            .coverage
            .iter()
            .find(|c| c.expression == "entry.get")
            .unwrap()
            .status,
        "external"
    );

    // 6. Tree-sitter return inference
    let tree_source = r#"
        use tree_sitter::Parser;
        fn f(parser: &mut Parser) {
            let tree = parser.parse("x", None).unwrap();
            let root = tree.root_node(); root.kind();
            let child = root.child_by_field_name("body").unwrap(); child.kind();
        }
    "#;
    let tree_facts = ast::extract("src/probe_tree.rs", "rust", tree_source).unwrap();
    let tree_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tree.rs".into(),
        tree_facts,
    )]));
    assert_eq!(
        tree_graph
            .coverage
            .iter()
            .find(|c| c.expression == "tree.root_node")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        tree_graph
            .coverage
            .iter()
            .find(|c| c.expression == "root.kind")
            .unwrap()
            .status,
        "external"
    );
    assert_eq!(
        tree_graph
            .coverage
            .iter()
            .find(|c| c.expression == "child.kind")
            .unwrap()
            .status,
        "external"
    );

    // 7. Parser-homonym negative (pulldown_cmark::Parser must NOT resolve to tree-sitter Node)
    let parser_homonym_source = r#"
        use pulldown_cmark::Parser;
        fn f(parser: &mut Parser) {
            let tree = parser.parse("x", None).unwrap();
            let root = tree.root_node(); root.kind();
        }
    "#;
    let parser_homonym_facts =
        ast::extract("src/probe_parser_neg.rs", "rust", parser_homonym_source).unwrap();
    let parser_homonym_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_parser_neg.rs".into(),
        parser_homonym_facts,
    )]));
    let root_kind = parser_homonym_graph
        .coverage
        .iter()
        .find(|c| c.expression == "root.kind")
        .expect("root.kind");
    assert_ne!(
        root_kind.status, "external",
        "pulldown_cmark::Parser must not resolve to tree-sitter Node: {root_kind:#?}"
    );

    // 8. While-let shadow followed by outer use (loop binding must not leak outside loop)
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
    let while_let_facts = ast::extract("src/probe_while_let.rs", "rust", while_let_source).unwrap();
    let while_let_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_while_let.rs".into(),
        while_let_facts,
    )]));
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

    // 9. Tuple factory shadowed by local closure (must not link to top-level function's return type)
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
    let tuple_shadow_facts =
        ast::extract("src/probe_tup_shadow.rs", "rust", tuple_shadow_source).unwrap();
    let tuple_shadow_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_tup_shadow.rs".into(),
        tuple_shadow_facts,
    )]));
    let shadow_finish = tuple_shadow_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.finish")
        .expect("entry.finish");
    assert!(
        !shadow_finish
            .evidence
            .contains("src.probe_tup_shadow.A.finish"),
        "shadowed make() must NOT link to top-level A.finish: {shadow_finish:#?}"
    );

    // 9b. Nested function declaration shadows file-level function tuple factory
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
    let nested_fn_facts = ast::extract("src/probe_nested_fn.rs", "rust", nested_fn_source).unwrap();
    let nested_fn_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_nested_fn.rs".into(),
        nested_fn_facts,
    )]));
    let nested_finish = nested_fn_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.finish")
        .expect("entry.finish");
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

    // 10. Local query_map callback (must not infer external rusqlite::Row)
    let local_qm_source = r#"
        struct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }
        struct Local; impl Local { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } }
        fn f(val: Local) {
            val.query_map(|entry| entry.get(0));
        }
    "#;
    let local_qm_facts = ast::extract("src/probe_local_qm.rs", "rust", local_qm_source).unwrap();
    let local_qm_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_local_qm.rs".into(),
        local_qm_facts,
    )]));
    let local_entry_get = local_qm_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.get")
        .expect("entry.get");
    assert_ne!(
        local_entry_get.status, "external",
        "local query_map must NOT infer external rusqlite::Row: {local_entry_get:#?}"
    );

    // 10b. Local struct Connection query_map callback (must not infer external rusqlite::Row)
    let local_conn_source = r#"
        struct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }
        struct Connection;
        impl Connection { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } }
        fn f(db: Connection) {
            db.query_map(|entry| entry.get(0));
        }
    "#;
    let local_conn_facts =
        ast::extract("src/probe_local_conn.rs", "rust", local_conn_source).unwrap();
    let local_conn_graph = linker::link(&std::collections::BTreeMap::from([(
        "src/probe_local_conn.rs".into(),
        local_conn_facts,
    )]));
    let conn_entry_get = local_conn_graph
        .coverage
        .iter()
        .find(|c| c.expression == "entry.get")
        .expect("entry.get");
    assert_ne!(
        conn_entry_get.status, "external",
        "local struct Connection must NOT infer external rusqlite::Row: {conn_entry_get:#?}"
    );
    assert!(
        !conn_entry_get.evidence.contains("rusqlite::Row"),
        "evidence must NOT mention rusqlite::Row: {conn_entry_get:#?}"
    );

    // 11. Disabled Cargo pack in .forge/frameworks/rust-cargo.toml must not supply return policy
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!("forge-test-disabled-pack-{}", nonce));
    std::fs::create_dir_all(temp_root.join(".forge/frameworks")).unwrap();
    std::fs::write(
        temp_root.join(".forge/frameworks/rust-cargo.toml"),
        "enabled = false\n",
    )
    .unwrap();
    let r3_source = r#"
        use rusqlite::Connection;
        fn run(conn: Connection) {
            let mut statement = conn.prepare("SELECT 1").unwrap();
            statement.execute([]);
        }
    "#;
    let r3_facts = ast::extract("src/lib.rs", "rust", r3_source).unwrap();
    let r3_graph = linker::link_with_root(
        &std::collections::BTreeMap::from([("src/lib.rs".into(), r3_facts)]),
        None,
        Some(&temp_root),
    );
    let row = r3_graph
        .coverage
        .iter()
        .find(|r| r.expression == "statement.execute")
        .expect("statement.execute");
    assert!(
        !row.evidence.contains("rusqlite::Statement"),
        "disabled rust-cargo pack must NOT infer rusqlite::Statement: {row:#?}"
    );
    let _ = std::fs::remove_dir_all(&temp_root);

    // 11b. Disabled Cargo pack with whitespace (enabled  =  false) and qualified without import
    for (label, manifest, source) in [
        ("whitespace", "enabled  =  false\n", "use rusqlite::Connection;\nfn run(conn: Connection) { let s = conn.prepare(\"SELECT 1\").unwrap(); s.execute([]); }"),
        ("no-import", "enabled = false\n", "fn run(conn: rusqlite::Connection) { let s = conn.prepare(\"SELECT 1\").unwrap(); s.execute([]); }"),
    ] {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let temp_root = std::env::temp_dir().join(format!("forge-test-disabled-{}-{}", label, nonce));
        std::fs::create_dir_all(temp_root.join(".forge/frameworks")).unwrap();
        std::fs::write(temp_root.join(".forge/frameworks/rust-cargo.toml"), manifest).unwrap();
        let facts = ast::extract("src/lib.rs", "rust", source).unwrap();
        let graph = linker::link_with_root(
            &std::collections::BTreeMap::from([("src/lib.rs".into(), facts)]),
            None,
            Some(&temp_root),
        );
        let row = graph.coverage.iter().find(|r| r.expression == "s.execute").expect("s.execute");
        assert!(
            !row.evidence.contains("rusqlite::Statement"),
            "{label}: disabled rust-cargo pack must NOT infer rusqlite::Statement: {row:#?}"
        );
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    // 12. Lexical provenance: nested Connection or unrelated qualified connection with unrelated import
    for (label, source) in [
        ("nested-local", "use rusqlite::Result;\nstruct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }\nfn f() { struct Connection; impl Connection { fn query_map<F: Fn(LocalItem)>(&self, f: F) { f(LocalItem {}); } } fn run(c: Connection) { c.query_map(|entry| entry.get(0)); } }"),
        ("qualified-local", "use rusqlite::Result;\nstruct LocalItem; impl LocalItem { fn get(&self, _: i32) {} }\nmod other { pub struct Connection; impl Connection { pub fn query_map<F: Fn(super::LocalItem)>(&self, f: F) { f(super::LocalItem {}); } } }\nfn f(c: other::Connection) { c.query_map(|entry| entry.get(0)); }"),
    ] {
        let facts = ast::extract("src/lib.rs", "rust", source).unwrap();
        let graph = linker::link(&std::collections::BTreeMap::from([("src/lib.rs".into(), facts)]));
        let row = graph.coverage.iter().find(|r| r.expression == "entry.get").expect("entry.get");
        assert_ne!(row.status, "external", "{label}: entry.get must not be external: {row:#?}");
        assert!(!row.evidence.contains("rusqlite::Row"), "{label}: evidence must not mention rusqlite::Row: {row:#?}");
    }

    // Contract revision 2: effective class hierarchy return policy preservation (LS001-T5-R3)
    {
        let root = std::env::temp_dir().join(format!("seam-contract-r2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".forge/frameworks")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='seam-r2'\nversion='0.1.0'\n[dependencies]\nrusqlite='0.31'\n",
        )
        .unwrap();
        let manifest = r#"enabled = true
replace = true
[[class_hierarchy_receivers]]
bases = ['rusqlite::Connection', 'Connection']
provider = 'rusqlite'
members = ['prepare', 'prepare_cached']
[class_hierarchy_receivers.returns]
prepare_cached = { kind = 'receiver', base = 'rusqlite::Row' }
[[class_hierarchy_receivers]]
bases = ['rusqlite::Row', 'Row']
provider = 'rusqlite'
members = ['get']
"#;
        std::fs::write(root.join(".forge/frameworks/rust-cargo.toml"), manifest).unwrap();
        let source = "use rusqlite::Connection;\nfn f(conn: Connection) { let value = conn.prepare_cached(\"SELECT 1\").unwrap(); value.get(0); }";
        let facts = ast::extract("src/lib.rs", "rust", source).unwrap();
        let all = std::collections::BTreeMap::from([("src/lib.rs".into(), facts)]);
        let graph = linker::link_with_root(&all, None, Some(&root));
        let row = graph
            .coverage
            .iter()
            .find(|r| r.expression == "value.get")
            .unwrap();
        assert!(
            row.evidence.contains("rusqlite::Row"),
            "effective return policy for prepare_cached -> Row must resolve on value.get: {row:#?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    assert!(ast::parser("sql", "schema.sql").is_err());
    let mut public_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::parse_file(
        &contextunity_forge_mcp::engine::languages::sql::SQL,
        "public.sql",
        "CREATE TABLE public_test (id INT)",
        "public",
        &mut public_facts,
    )
    .unwrap();
    assert!(public_facts
        .nodes
        .iter()
        .any(|node| node.name == "public_test"));
    for (sql, name, relation) in [
        (
            "SELECT * FROM public.orders",
            "public.orders",
            "queries_table",
        ),
        (
            "SELECT * FROM orders, customers",
            "customers",
            "queries_table",
        ),
        (
            "BEGIN; PRAGMA x=1; SELECT * FROM orders",
            "orders",
            "queries_table",
        ),
        (
            "SET search_path TO shop; CREATE TABLE batch_table (id INT)",
            "batch_table",
            "defines_table",
        ),
        (
            "INSERT OR REPLACE INTO orders VALUES (1)",
            "orders",
            "mutates_table",
        ),
        ("UPDATE orders SET id = 1", "orders", "mutates_table"),
        ("COMMIT; SELECT * FROM orders", "orders", "queries_table"),
        ("ROLLBACK; UPDATE orders AS o SET id = 1", "orders", "mutates_table"),
        ("UPDATE ONLY orders o SET id = 1", "orders", "mutates_table"),
        ("WITH source AS (SELECT * FROM customers) UPDATE ONLY (orders) AS o SET id=1", "orders", "mutates_table"),
        ("SELECT * FROM orders o JOIN customers c ON c.id=o.id, inventory i", "inventory", "queries_table"),
        ("SELECT * FROM orders o, inventory i JOIN customers c ON c.id=i.id", "customers", "queries_table"),
        ("WITH temporary AS (SELECT * FROM orders) SELECT * FROM temporary JOIN inventory ON 1=1", "orders", "queries_table"),
        ("COMMIT; SELECT * FROM orders WHERE note = $$semi;colon$$", "orders", "queries_table"),
        ("SET x='semi;SELECT fake'; /* ; UPDATE ignored */ SELECT * FROM orders", "orders", "queries_table"),
        ("BEGIN; -- ; UPDATE ignored\nWITH rows AS (SELECT * FROM orders) SELECT * FROM rows", "orders", "queries_table"),
        ("SET x=$tag$; UPDATE ignored$tag$; SELECT * FROM orders", "orders", "queries_table"),
        ("DELETE FROM orders", "orders", "mutates_table"),
        (
            "CREATE TABLE IF NOT EXISTS orders (id INT)",
            "orders",
            "defines_table",
        ),
    ] {
        assert!(
            contextunity_forge_mcp::engine::languages::sql::extract_embedded_sql_tables(sql)
                .contains(&(name.to_owned(), relation))
        );
    }

    for (sql, expected) in [
        ("WITH temporary(id) AS (SELECT * FROM temporary) SELECT * FROM temporary", vec!["temporary"]),
        ("WITH RECURSIVE temporary(id) AS (SELECT * FROM temporary) SELECT * FROM temporary", vec![]),
        ("WITH temporary(id) AS (SELECT * FROM orders) SELECT * FROM public.temporary", vec!["orders", "public.temporary"]),
        ("WITH first_cte AS (SELECT * FROM orders), second_cte(id) AS (SELECT * FROM first_cte) SELECT * FROM second_cte", vec!["orders"]),
    ] {
        let actual = contextunity_forge_mcp::engine::languages::sql::extract_embedded_sql_tables(sql);
        assert_eq!(actual.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), expected, "{sql}");
    }

    // Exercise the real literal producers, including prose that previously
    // matched SQL keywords inside unrelated words.
    let mut embedded = vec![(
        "embedded.rs",
        "rust",
        r##"fn query() { let _ = r#"SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows"#; }
fn prose() { let _ = "information was updated and transformed"; }"##,
    )];
    #[cfg(feature = "lang-python")]
    embedded.push((
        "embedded.py",
        "python",
        "def query():\n    return \"SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows\"\ndef prose():\n    '''information was updated and transformed'''\n",
    ));
    #[cfg(feature = "lang-typescript")]
    embedded.push((
        "embedded.ts",
        "typescript",
        "function query() { return `SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows`; }\nfunction prose() { return 'information was updated and transformed'; }\n",
    ));
    for (path, language, source) in embedded {
        let facts = ast::extract(path, language, source).unwrap();
        let queries: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.kind == "queries_table")
            .collect();
        assert_eq!(queries.len(), 1, "{path}: {queries:?}");
        assert_eq!(queries[0].expression, "orders", "{path}");
        assert!(
            !facts.references.iter().any(|r| r.kind == "mutates_table"),
            "{path}"
        );
    }

    let workspace = super::support::Workspace::new();
    workspace.write("more_tables.sql", "CREATE TABLE inventory (id INTEGER); CREATE TABLE factory_orders (id INTEGER); CREATE TABLE temporary (id INTEGER);");
    workspace.write(
        "quoted_schema.sql",
        "CREATE TABLE \"quoted_table\" (id INTEGER);",
    );
    workspace.write(
        "case_schema.sql",
        "CREATE TABLE \"Foo\" (id INTEGER); CREATE TABLE \"foo\" (id INTEGER);",
    );
    workspace.write(
        "Cargo.toml",
        "[package]\nname = 'fixture'\nversion = '0.1.0'\n[dependencies]\nsqlx = '0.8'\n",
    );
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE customers (id INTEGER);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE shop_override (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id) ON UPDATE CASCADE, PRIMARY KEY (id));\nCREATE TABLE a.same (id INTEGER); CREATE TABLE b.same (id INTEGER); CREATE TABLE \"x.y\" (id INT); CREATE TABLE x.y (id INT); CREATE TABLE SET (id INT); CREATE TABLE CASCADE (id INT);\nCREATE VIEW order_view AS SELECT * FROM orders;\n");
    workspace.write(
        "extra_schema.sql",
        "CREATE TABLE shop_default (id INTEGER);",
    );
    workspace.write(
        "migration.sql",
        "ALTER TABLE orders ADD COLUMN note TEXT;\n",
    );
    workspace.write("queries.rs", "#[derive(sqlx::FromRow)] struct OrderRow { id: i64 }\nfn load() { let _ = \"SELECT * FROM orders\"; }\nfn save() { let _ = \"INSERT INTO orders (id) VALUES (1)\"; }\nfn remove() { let _ = \"DELETE FROM orders WHERE id = 1\"; }\nfn customers() { let _ = \"SELECT * FROM customers\"; }\nfn macro_load() { let _ = sqlx::query!(\"SELECT * FROM orders\"); }\nfn typed_load() { let _ = sqlx::query_as!(OrderRow, \"SELECT * FROM orders\"); }\nfn inline_schema() { let _ = \"CREATE TABLE local_data (id INTEGER)\"; }\n");
    workspace.write("foreign.rs", "struct Imported;\nuse external::Imported;\nfn foreign() { let _ = sqlx::query_as!(Imported, \"SELECT * FROM orders\"); }\n");
    workspace.write("qualified.rs", "fn exact() { let _ = \"SELECT * FROM a.same\"; }\nfn ambiguous_short() { let _ = \"SELECT * FROM same\"; }\nfn batch() { let _ = \"BEGIN; PRAGMA x=1; SELECT * FROM orders, customers\"; }\nfn inline_create() { let _ = \"CREATE TABLE c.inline_qual (id INT)\"; }\nfn inline_query() { let _ = \"SELECT * FROM inline_qual\"; }\n");
    workspace.write("upsert.rs", "fn upsert() { let _ = \"INSERT INTO orders VALUES (1) ON CONFLICT DO UPDATE SET id=1\"; }\n");
    workspace.write("patterns.rs", "fn rollback_update() { let _ = \"ROLLBACK; UPDATE ONLY orders AS o SET id=1\"; }\nfn mixed_from() { let _ = \"SELECT * FROM orders o JOIN customers c ON c.id=o.id, inventory i\"; }\nfn cte_query() { let _ = \"WITH temporary AS (SELECT * FROM orders) SELECT * FROM temporary JOIN inventory ON 1=1\"; }\n");
    #[cfg(feature = "lang-python")]
    workspace.write("identifiers.py", "def quoted_orders():\n    return 'SELECT * FROM \"orders\"'\ndef unquoted_table():\n    return 'SELECT * FROM quoted_table'\ndef cte_columns():\n    return 'WITH temporary(id) AS (SELECT id FROM orders) SELECT * FROM temporary'\n");
    workspace.write("ambiguous.rs", "mod a { struct Shared; }\nmod b { struct Shared; }\nfn ambiguous() { let _ = sqlx::query_as!(Shared, \"SELECT * FROM orders\"); }\n");
    workspace.write("shadow.rs", "mod sqlx {}\nstruct Shadow;\nfn shadow() { let _ = sqlx::query_as!(Shadow, \"SELECT * FROM orders\"); }\n");
    workspace.write("other.rs", "struct Other;\nfn other() { let _ = custom::query_as!(Other, \"SELECT * FROM orders\"); }\n");
    workspace.write(
        "derive_only.rs",
        "#[derive(sqlx::FromRow)] struct DeriveOnly { id: i64 }\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("queries.py", "from django.db import models\nfrom sqlalchemy.orm import DeclarativeBase\ndef py_load():\n    return \"SELECT * FROM orders\"\n\nclass Base(DeclarativeBase):\n    pass\nclass Order(Base):\n    __tablename__ = 'orders'\n\nclass FalseSqlAlchemy:\n    __tablename__ = 'orders'\n\nclass Item(models.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("quoted.py", "def quoted():\n    return 'SELECT * FROM \"x.y\"'\ndef qualified():\n    return 'SELECT * FROM x.y'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("case_query.py", "def upper_case():\n    return 'SELECT * FROM \"Foo\"'\ndef lower_case():\n    return 'SELECT * FROM \"foo\"'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory.py", "from sqlalchemy.orm import declarative_base as make_base\nBase = make_base()\nclass FactoryOrder(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory_qualified.py", "import sqlalchemy.orm as orm\nBase = orm.declarative_base()\nclass QualifiedFactory(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory_invalid.py", "from sqlalchemy.orm import declarative_base as make_base\nmake_base = object()\nBase = make_base()\nclass ReboundFactory(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(".forge/frameworks/other-tables.toml", "[[semantic_rules]]\nid = 'other-table'\nnamespace = 'other.tables'\nscope = 'project'\noperation = { kind = 'class_attribute_to_table', language = 'python', attribute = 'table_name', container = 'Config' }\n");
    #[cfg(feature = "lang-python")]
    workspace.write(".forge/frameworks/standalone-default.toml", "[[semantic_rules]]\nid = 'default'\nnamespace = 'other.default'\nscope = 'project'\noperation = { kind = 'class_default_table', language = 'python', bases = ['fixture.CustomBase'] }\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/default.py",
        "# class Commented(CustomBase): pass\n\"\"\"class Quoted(CustomBase): pass\"\"\"\nfrom fixture import CustomBase\nclass Empty:\n    pass\nclass Default(CustomBase):\n    pass\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "other_model.py",
        "class Generic:\n    class Config:\n        table_name = 'orders'\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'shop'\nversion = '0.1'\ndependencies = ['django', 'sqlalchemy']\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/models.py",
        "from django.db import models as dbmodels\nclass Cart(dbmodels.Model):\n    pass\nclass Override(dbmodels.Model):\n    class Meta:\n        db_table = 'orders'\nclass NonModel:\n    class Meta:\n        db_table = 'orders'\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("shop/reassigned.py", "from django.db import models as dbmodels\ndbmodels = object()\nclass Reassigned(dbmodels.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/fake.py",
        "from local import models\nclass Fake(models.Model):\n    pass\n",
    );
    #[cfg(feature = "lang-typescript")]
    workspace.write(
        "queries.ts",
        "function ts_load() { return \"SELECT * FROM orders\"; }\n",
    );
    let db = workspace.build();
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("quoted_orders", "orders", 1),
        ("unquoted_table", "quoted_table", 1),
        ("cte_columns", "temporary", 0),
        ("cte_columns", "orders", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("upper_case", "\"Foo\"", 1),
        ("upper_case", "\"foo\"", 0),
        ("lower_case", "\"foo\"", 1),
        ("lower_case", "\"Foo\"", 0),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "quoted case: {source} -> {target}");
    }
    let fake_cte: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='cte_query' AND d.name='temporary' AND e.kind='queries_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        fake_cte, 0,
        "CTE name must not be resolved to a physical table"
    );
    #[cfg(feature = "lang-python")]
    {
        let edge: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='FactoryOrder' AND d.name='factory_orders' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            edge, 1,
            "declarative_base factory must prove model ancestry"
        );
        let qualified: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='QualifiedFactory' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(qualified, 1, "qualified imported factory must be resolved");
        let rebound: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='ReboundFactory' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(rebound, 0, "rebound factory must fail closed");
    }
    let qualified: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('a.same','b.same')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(qualified, 2, "qualified tables retain distinct identities");
    let dotted: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('\"x.y\"','x.y')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dotted, 2, "quoted and qualified identities differ");
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("quoted", "\"x.y\"", 1),
        ("quoted", "x.y", 0),
        ("qualified", "x.y", 1),
        ("qualified", "\"x.y\"", 0),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let fake_dml: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name IN ('upsert','item') AND d.name IN ('SET','CASCADE') AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        fake_dml, 0,
        "UPDATE clauses are not standalone table writes"
    );
    for (source, target, expected) in [
        ("exact", "a.same", 1),
        ("exact", "b.same", 0),
        ("ambiguous_short", "a.same", 0),
        ("ambiguous_short", "b.same", 0),
        ("batch", "customers", 1),
        ("batch", "orders", 1),
        ("inline_query", "c.inline_qual", 1),
        ("mixed_from", "inventory", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let update_edge: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='rollback_update' AND d.name='orders' AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(update_edge, 1, "batch UPDATE with alias and ONLY must link");
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind = 'table' AND name = 'orders'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1, "SQL schema tables must be indexed");
    let inline: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND name='local_data'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inline, 1, "inline CREATE must synthesize a table");
    let inline_link: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='inline_schema' AND d.name='local_data' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(inline_link, 1, "inline CREATE must link its owner");
    let model: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='OrderRow' AND d.name='orders' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        model, 1,
        "query_as must link its unambiguous local result type"
    );
    let invalid: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('Shared','Imported','Shadow','Other','DeriveOnly') AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        invalid, 0,
        "ambiguous, imported, shadowed, unconfigured and derive-only identities must remain unresolved"
    );
    let mut expected = vec![
        ("order_view", "queries_table"),
        ("load", "queries_table"),
        ("save", "mutates_table"),
        ("remove", "mutates_table"),
        ("macro_load", "queries_table"),
        ("typed_load", "queries_table"),
        ("item", "references_table"),
    ];
    #[cfg(feature = "lang-python")]
    expected.push(("py_load", "queries_table"));
    #[cfg(feature = "lang-python")]
    expected.extend([
        ("Order", "defines_table"),
        ("Item", "defines_table"),
        ("Generic", "defines_table"),
    ]);
    #[cfg(feature = "lang-typescript")]
    expected.push(("ts_load", "queries_table"));
    for (source, kind) in expected {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='orders' AND e.kind=?2",
            [source, kind], |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1, "missing {kind} edge from {source}");
    }
    let columns: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='column' AND qualname IN ('orders.id', 'item.order_id')", [], |row| row.get(0)).unwrap();
    assert_eq!(columns, 2);
    let altered: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='column' AND qualname='orders.note' AND path_id=(SELECT path_id FROM path_dictionary WHERE path='migration.sql')", [], |row| row.get(0)).unwrap();
    assert_eq!(altered, 1, "ALTER TABLE must index the added column");
    #[cfg(feature = "lang-python")]
    {
        let defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Cart' AND d.name='shop_cart' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(defaults, 1, "Django default table should link");
        let other_default: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Default' AND d.name='shop_default' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            other_default, 1,
            "independent default-only manifest should link"
        );
        let empty_defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='Empty' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            empty_defaults, 0,
            "class without bases must not define a table"
        );
        let false_defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Fake' AND d.name='shop_fake' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(false_defaults, 0, "local models must not imply Django ORM");
        let override_edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Override' AND d.name='shop_override' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(override_edges, 0, "explicit Meta table overrides default");
        let invalid_models: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('NonModel','Reassigned','FalseSqlAlchemy') AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            invalid_models, 0,
            "unproven Django identities remain unresolved"
        );
    }
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(100),
        0,
        None,
        None,
    )
    .unwrap();
    let impact = contextunity_forge_mcp::db::traversal::traverse_with_options(
        &db,
        "schema.sql:table:orders",
        &contextunity_forge_mcp::db::traversal::TraversalOptions {
            depth: 1,
            inbound: true,
            mode: None,
            edge_types: None,
            page: &page,
        },
    )
    .unwrap();
    let reached = impact["nodes"]["items"].as_array().unwrap();
    assert!(
        reached.iter().any(|node| node["name"] == "order_view"),
        "SQL impact must include views: {impact}"
    );
    for name in ["load", "save", "remove", "item"] {
        assert!(
            reached.iter().any(|node| node["name"] == name),
            "SQL impact must include {name}: {impact}"
        );
    }
    #[cfg(feature = "lang-python")]
    for name in ["Order", "Item", "py_load"] {
        assert!(
            reached.iter().any(|node| node["name"] == name),
            "SQL impact must include {name}: {impact}"
        );
    }
    drop(db);
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id), PRIMARY KEY (id));\nCREATE VIEW order_view AS SELECT * FROM orders;\nCREATE TABLE customers (id INTEGER);\n");
    workspace.cold_parity("schema.sql");
    let delta = workspace.open();
    let cold = contextunity_forge_mcp::db::reader::open(
        &workspace.0.join(".forge/cold.sqlite"),
        &workspace.0,
    )
    .unwrap();
    let root = |conn: &rusqlite::Connection| -> String {
        conn.query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        root(&delta),
        root(&cold),
        "SQL cold and delta commitments must agree"
    );
    let without_sqlx = super::support::Workspace::new();
    without_sqlx.write("schema.sql", "CREATE TABLE orders (id INTEGER);\n");
    #[cfg(feature = "lang-python")]
    without_sqlx.write("inactive.py", "from sqlalchemy.orm import DeclarativeBase\nclass Inactive(DeclarativeBase):\n    __tablename__ = 'orders'\n");
    #[cfg(feature = "lang-python")]
    without_sqlx.write("inactive_factory.py", "from sqlalchemy.orm import declarative_base\nBase = declarative_base()\nclass InactiveFactory(Base):\n    __tablename__ = 'orders'\n");
    without_sqlx.write("query.rs", "struct NotActive;\nfn load() { let _ = sqlx::query_as!(NotActive, \"SELECT * FROM orders\"); }\n");
    let db = without_sqlx.build();
    let edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='NotActive' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(edges, 0, "undeclared SQLx must not activate its model rule");
    #[cfg(feature = "lang-python")]
    {
        let inactive: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('Inactive','InactiveFactory') AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            inactive, 0,
            "undeclared SQLAlchemy must not activate its model rule"
        );
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

    let workspace = super::support::Workspace::new();
    let scope = format!("project_{}", "s".repeat(211));
    workspace.write(
        format!("{scope}/pyproject.toml"),
        "[project]\nname = 'scope-probe'\nversion = '0.1.0'\n",
    );
    let dependencies =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(&workspace),
        );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let mut scope_copies = Vec::new();
    for count in [1, 32] {
        let path = format!("{scope}/service.py");
        let source = format!(
            "class ServiceClient:\n    def fetch(self, endpoint: str) -> dict:\n{}    def _send_request(self, endpoint: str) -> dict:\n        return {{'status': 'ok'}}\n",
            "        self._send_request(endpoint)\n".repeat(count),
        );
        let facts = ast::extract(&path, "python", &source).unwrap();
        assert_eq!(
            facts
                .references
                .iter()
                .filter(|reference| reference.expression == "self._send_request")
                .count(),
            count,
        );
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "_send_request")
            .unwrap()
            .id
            .clone();
        let all = std::collections::BTreeMap::from([(path, facts)]);
        let (graph, copies) = pool.install(|| {
            let _ = contextunity_forge_mcp::engine::linker::link_with_registry(
                &all,
                None,
                Some(&workspace),
                &dependencies,
            );
            scope_allocations::measure(scope.len(), || {
                contextunity_forge_mcp::engine::linker::link_with_registry(
                    &all,
                    None,
                    Some(&workspace),
                    &dependencies,
                )
            })
        });
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == target)
                .count(),
            count,
            "every member call must retain its exact target",
        );
        assert_eq!(
            graph
                .coverage
                .iter()
                .filter(|coverage| coverage.expression == "self._send_request"
                    && coverage.status == "resolved")
                .count(),
            count,
        );
        scope_copies.push(copies);
    }
    assert!(
        scope_copies[1] <= scope_copies[0],
        "assertion failed: package-scope preparation must be per file, not per member reference: {scope_copies:?}",
    );
}
