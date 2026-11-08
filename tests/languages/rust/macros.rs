use super::{coverage, linked};

#[test]
fn declarative_macros_keep_their_declared_external_identity() {
    for (label, source, names, expect_external) in [
        (
            "imported",
            r#"
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
    tracing::info!("hello");
    log::info!("hello");
}
"#,
            &[
                "format",
                "vec",
                "write",
                "json",
                "bail",
                "ensure",
                "anyhow::bail",
                "anyhow::ensure",
                "serde_json::json",
                "tracing::info",
                "log::info",
            ][..],
            true,
        ),
        (
            "unimported",
            r#"
fn run_unimported_macros() {
    let _j = json!({ "key": "value" });
    let _b = bail!("operation failed");
    let _e = ensure!(true);
}
"#,
            &["json", "bail", "ensure"][..],
            false,
        ),
    ] {
        let graph = linked("macros.rs", source);
        for &name in names {
            let row = coverage(&graph, name);
            assert_ne!(
                row.status, "unresolved",
                "{label} macro {name} must use its declarative signature: {row:#?}"
            );
            if expect_external {
                assert_eq!(
                    row.status, "external",
                    "macro {name} status mismatch: {row:#?}"
                );
            }
        }
        if expect_external {
            for provider in ["tracing", "log"] {
                let expression = format!("{provider}::info");
                let row = coverage(&graph, &expression);
                assert_eq!(row.status, "external");
                assert!(row.evidence.contains(&format!("macro {provider}::")));
            }
        }
    }
}

#[test]
fn iterator_chains_and_local_adapters_preserve_only_proven_receivers() {
    let chain_source = r#"
fn run_iterator_chain() {
    let numbers = Vec::new();
    let _collected = numbers.into_iter().filter(|x| true).map(|x| x).collect();
    let _deep = numbers.into_iter().filter(|_| true).map(|x| x).take(5).enumerate().skip(1).step_by(2).collect();
    let _from_macro = vec![1, 2, 3].into_iter().filter(|x| true).map(|x| x).collect();
}
"#;
    let chain_graph = linked("chain.rs", chain_source);
    for expr in [
        "numbers.into_iter().filter(|x| true).map(|x| x).collect",
        "numbers.into_iter().filter(|_| true).map(|x| x).take(5).enumerate().skip(1).step_by(2).collect",
        "vec![1, 2, 3].into_iter().filter(|x| true).map(|x| x).collect",
    ] {
        let collect_cov = coverage(&chain_graph, expr);
        assert_ne!(
            collect_cov.status, "unresolved",
            "collect() on chained iterator receiver must not be unresolved: {collect_cov:#?}"
        );
    }

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
    let over_eight_graph = linked("over_eight.rs", over_eight_source);
    for (expr, expected) in [
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).collect", "external"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).collect", "external"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
        ("numbers.into_iter().map(|x| x).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).take(1).collect", "unresolved"),
    ] {
        let cov = coverage(&over_eight_graph, expr);
        assert_eq!(
            cov.status, expected,
            "chain call {expr} expected status {expected}: {cov:#?}"
        );
    }
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
            let r4_graph = linked("r4.rs", &r4_source);
            let r4_cov = coverage(&r4_graph, "item.finish");
            assert_ne!(
                r4_cov.status, "resolved",
                "item.finish on non-iterator B from {iter_name}.{adapter} must not resolve to A.finish: {r4_cov:#?}"
            );
        }
    }
}

#[test]
fn local_macro_definitions_precede_external_identity_and_type_inference() {
    let source = r#"
struct A;
macro_rules! json { () => { 1 }; }
macro_rules! vec { () => { A }; }
macro_rules! format { () => { A }; }
fn run() {
    let _value = json!();
    let vector = vec!();
    vector.len();
    let formatted = format!();
    formatted.len();
}
"#;
    let graph = linked("local_macro.rs", source);
    assert!(graph.facts.errors.is_empty(), "{:?}", graph.facts.errors);
    for name in ["json", "vec", "format"] {
        let row = coverage(&graph, name);
        assert_ne!(
            row.status, "external",
            "local macro {name}!() must not inherit an external identity: {row:#?}"
        );
    }
    for expression in ["vector.len", "formatted.len"] {
        let row = coverage(&graph, expression);
        assert_ne!(
            row.status, "external",
            "local macro output {expression} must not fabricate a stdlib type: {row:#?}"
        );
    }
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
            let graph = linked("probe.rs", &source);
            assert!(
                graph.facts.errors.is_empty(),
                "{name}/{label}: {:?}",
                graph.facts.errors
            );
            let row = coverage(&graph, "item.len");
            assert_ne!(
                row.status, "external",
                "local imported macro {name}/{label} fabricated stdlib type: {row:?}"
            );
        }
    }
}

#[test]
fn local_macro_imports_resolve_only_in_their_scope() {
    for (label, source, expression) in [
        ("qualified", "mod serde_json { macro_rules! json { () => { 1 }; } pub(crate) use json; } fn run() { serde_json::json!(); }", "serde_json::json"),
        ("import", "mod local { macro_rules! json { () => { 1 }; } pub(crate) use json; } use local::json; fn run() { json!(); }", "json"),
        ("renamed-import", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } use local::custom as json; fn run() { json!(); }", "json"),
        ("crate-import", "mod local { macro_rules! json { () => { 1 }; } pub(crate) use json; } use crate::local::json; fn run() { json!(); }", "json"),
        ("module-alias", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom as json; } use local as serde_json; fn run() { serde_json::json!(); }", "serde_json::json"),
        ("parent-module", "mod outer { macro_rules! json { () => { 1 }; } mod inner { fn run() { json!(); } } }", "json"),
        ("parent-control", "mod outer { macro_rules! custom { () => { 1 }; } mod inner { fn run() { custom!(); } } }", "custom"),
    ] {
        let graph = linked("probe.rs", source);
        assert!(graph.facts.errors.is_empty(), "{label}: {:?}", graph.facts.errors);
        let row = coverage(&graph, expression);
        assert_ne!(row.status, "external", "{label}: local macro misclassified external: {row:?}");
        assert_eq!(row.status, "resolved", "{label}: local macro should resolve locally: {row:?}");
    }
    for (label, source, expression) in [
        ("direct-import", "mod local { pub fn available() {} } use local::missing; fn run() { missing(); }", "missing"),
        ("module-alias", "mod local { pub fn available() {} } use local as alias; fn run() { alias::missing(); }", "alias::missing"),
        ("macro-member", "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } use local as serde_json; fn run() { serde_json::json!(); }", "serde_json::json"),
    ] {
        let graph = linked("probe.rs", source);
        assert!(graph.facts.errors.is_empty(), "{label}: {:?}", graph.facts.errors);
        let row = coverage(&graph, expression);
        assert_ne!(row.status, "resolved", "{label}: invented member should not be resolved: {row:?}");
    }
    let source = "mod local { macro_rules! custom { () => { 1 }; } pub(crate) use custom; } mod child { use crate::local::custom as json; fn scoped() { json!(); } }\nfn run() { json!(); }";
    let graph = linked("probe.rs", source);
    assert!(graph.facts.errors.is_empty(), "{:?}", graph.facts.errors);
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

#[test]
fn unrelated_namespaces_and_later_macros_do_not_suppress_stdlib_results() {
    for (label, source) in [
        ("sibling-macro", "mod other { macro_rules! vec { () => { 1 }; } } fn run() { let item = vec![1]; item.len(); }"),
        ("value-namespace", "use std::fmt::format; fn run() { let item = format!(\"hello\"); item.len(); }"),
        ("later-macro", "fn run() { let item = vec![1]; item.len(); } macro_rules! vec { () => { 1 }; }"),
        ("foreign-function", "mod local { extern \"C\" { pub fn custom(); } } use local::custom as vec; fn run() { let item = vec![1]; item.len(); }"),
        ("foreign-static", "mod local { extern \"C\" { pub static custom: u8; } } use local::custom as format; fn run() { let item = format!(\"hello\"); item.len(); }"),
        ("extern-crate", "mod local { pub extern crate core as custom; } use local::custom as vec; fn run() { let item = vec![1]; item.len(); }"),
    ] {
        let graph = linked("probe.rs", source);
        assert!(graph.facts.errors.is_empty(), "{label}: {:?}", graph.facts.errors);
        let row = coverage(&graph, "item.len");
        assert_eq!(row.status, "external", "{label}: proven std macro output suppressed: {row:?}");
    }
    let source = r#"
fn run() {
    let _json = json();
    let _format = format();
    let _vec = vec();
    let _write = write();
    let _println = println();
}
"#;
    let graph = linked("ordinary_functions.rs", source);
    for name in ["json", "format", "vec", "write", "println"] {
        let row = coverage(&graph, name);
        assert_eq!(
            row.status, "unresolved",
            "ordinary function call {name}() must remain unresolved: {row:#?}"
        );
    }
}
