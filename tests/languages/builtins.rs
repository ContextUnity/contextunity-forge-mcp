#![cfg(any(feature = "lang-python", feature = "lang-rust"))]

use contextunity_forge_mcp::engine::{ast, languages, linker};
use std::collections::BTreeMap;

fn assert_builtin_calls(path: &str, language: &str, names: &[&str], source: &str) {
    let profile = languages::require(language).unwrap();
    for name in names {
        assert!(profile.builtin(name), "missing {language} builtin {name}");
    }
    let facts = ast::extract(path, language, source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts)]));
    for name in names {
        let coverage = graph
            .coverage
            .iter()
            .find(|c| c.expression == *name)
            .unwrap_or_else(|| panic!("missing call coverage for {name}: {:?}", graph.coverage));
        assert_eq!(coverage.status, "external", "{name}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("Python standard library"),
            "{name}: {coverage:#?}"
        );
    }
}

#[cfg(feature = "lang-python")]
#[test]
fn python_standard_functions_types_and_exceptions_resolve() {
    let names = [
        "isinstance",
        "issubclass",
        "len",
        "range",
        "enumerate",
        "zip",
        "print",
        "open",
        "iter",
        "next",
        "super",
        "any",
        "all",
        "dict",
        "list",
        "set",
        "tuple",
        "str",
        "int",
        "float",
        "bool",
        "bytes",
        "Exception",
        "ValueError",
        "TypeError",
        "KeyError",
        "RuntimeError",
    ];
    let source = format!(
        "def run():\n{}",
        names
            .iter()
            .map(|name| format!("    {name}()\n"))
            .collect::<String>()
    );
    assert_builtin_calls("builtins.py", "python", &names, &source);
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_prelude_variants_and_macros_have_stdlib_provenance() {
    let names = [
        "Some", "Ok", "Err", "vec", "format", "println", "eprintln", "panic",
    ];
    let source = r#"fn run() {
    let _some = Some(1);
    let _ok: Result<i32, ()> = Ok(1);
    let _err: Result<(), i32> = Err(1);
    let _items = vec![1];
    let _text = format!("{}", 1);
    println!("{}", 1);
    eprintln!("{}", 1);
    if false { panic!(); }
}
"#;
    let profile = languages::require("rust").unwrap();
    assert!(profile.builtin("None"));
    let facts = ast::extract("builtins.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph = linker::link(&BTreeMap::from([("builtins.rs".to_owned(), facts)]));
    for name in names {
        assert!(profile.builtin(name), "missing Rust builtin {name}");
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == name)
            .unwrap_or_else(|| panic!("missing {name}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{name}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("Rust standard library"),
            "{name}: {coverage:#?}"
        );
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_stdlib_calls_are_external_and_local_shadow_is_resolved() {
    let cases = [
        (
            "prelude.rs",
            "fn run() { let text = String::from(\"hello\"); drop(text); }",
            "String::from",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "string_capacity.rs",
            "fn run() { let _text = String::with_capacity(8); }",
            "String::with_capacity",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "vec_from.rs",
            "fn run() { let _items = Vec::from([1, 2]); }",
            "Vec::from",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "box_from.rs",
            "fn run() { let _item = Box::from(1); }",
            "Box::from",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "option_clone.rs",
            "fn run() { let value = Some(1); let _copy = Option::clone(&value); }",
            "Option::clone",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "option_from.rs",
            "fn run() { let _value: Option<i32> = Option::from(1); }",
            "Option::from",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "result_clone.rs",
            "fn run() { let value: Result<i32, ()> = Ok(1); let _copy = Result::clone(&value); }",
            "Result::clone",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "box.rs",
            "fn run() { let _value = Box::new(1); }",
            "Box::new",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "vec.rs",
            "fn run() { let _values: Vec<u8> = Vec::new(); }",
            "Vec::new",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "drop.rs",
            "fn run() { let value = 1; drop(value); }",
            "drop",
            "external",
            Some("Rust standard library"),
            false,
        ),
        (
            "aliased.rs",
            "use std::sync::Arc as Shared;\nfn run() { let _value = Shared::new(1); }",
            "Shared::new",
            "external",
            Some("std.sync.Arc at line 1"),
            false,
        ),
        (
            "imported.rs",
            "use std::path::PathBuf;\nfn run() { let _path = PathBuf::from(\"file\"); }",
            "PathBuf::from",
            "external",
            Some("std.path.PathBuf at line 1"),
            false,
        ),
        (
            "unimported.rs",
            "fn run() { let _value = Arc::new(1); }",
            "Arc::new",
            "unresolved",
            None,
            false,
        ),
        (
            "no_constructor.rs",
            "fn run() { let _value = bool::new(); }",
            "bool::new",
            "unresolved",
            None,
            false,
        ),
        (
            "no_result_default.rs",
            "fn run() { let _value: Result<i32, ()> = Result::default(); }",
            "Result::default",
            "unresolved",
            None,
            false,
        ),
        (
            "local.rs",
            "struct String;\nimpl String { fn from(_: &str) -> Self { Self } }\nfn run() { let _text = String::from(\"hello\"); }",
            "String::from",
            "resolved",
            None,
            true,
        ),
    ];
    for (path, source, expression, status, origin, local_target) in cases {
        let facts = ast::extract(path, "rust", source).unwrap();
        assert!(facts.errors.is_empty(), "{path}: {facts:#?}");
        let graph = linker::link(&BTreeMap::from([(path.to_owned(), facts.clone())]));
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("{path}: missing {expression}: {graph:#?}"));
        assert_eq!(coverage.status, status, "{path}: {coverage:#?}");
        if let Some(origin) = origin {
            assert!(coverage.evidence.contains(origin), "{path}: {coverage:#?}");
        }
        if local_target {
            let target = facts.nodes.iter().find(|node| node.name == "from").unwrap();
            assert!(
                graph.edges.iter().any(|edge| edge.kind == "calls"
                    && edge.evidence == expression
                    && edge.dst == target.id),
                "{path}: {graph:#?}"
            );
        }
    }
}
