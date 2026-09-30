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
        assert_eq!(coverage.status, "resolved", "{name}");
        assert_eq!(
            coverage.evidence,
            format!("standard library or built-in callee: {name}")
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
fn rust_standard_types_variants_and_macros_resolve() {
    let types = [
        "Path", "PathBuf", "Vec", "String", "Option", "Result", "HashMap", "BTreeMap", "BTreeSet",
        "HashSet", "Ok", "Err", "Some",
    ];
    let macros = ["format", "println", "eprintln", "panic", "vec"];
    let mut source = String::from("fn run() {\n");
    for name in types {
        source.push_str(&format!("    {name}();\n"));
    }
    for name in macros {
        source.push_str(&format!("    {name}!();\n"));
    }
    source.push_str("}\n");
    let mut names = types.to_vec();
    names.extend(macros);
    assert_builtin_calls("builtins.rs", "rust", &names, &source);
    assert!(languages::require("rust").unwrap().builtin("None"));
}
