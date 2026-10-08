#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::models::{Facts, Graph},
    engine::{ast, docs, linker},
};
use std::collections::{BTreeMap, BTreeSet};

fn fixture() -> BTreeMap<String, Facts> {
    let mut facts = BTreeMap::new();
    for (path, source) in [
        ("a.py", "class Foo:\n    def method(self): pass\n"),
        ("b.py", "class Foo:\n    def method(self): pass\n"),
        ("consumer.py", "from a import Foo\ndef run(obj):\n    obj.method()\n    Foo.method()\n    Foo.method()\n"),
    ] {
        facts.insert(path.into(), ast::extract(path, "python", source).unwrap());
    }
    facts.insert(
        "docs/api.md".into(),
        Facts {
            docs: docs::extract("docs/api.md", "# API\n`Foo.method`\n", 0.).unwrap(),
            ..Facts::default()
        },
    );
    facts
}
fn calls(graph: &Graph) -> Vec<(&str, &str, usize)> {
    graph
        .edges
        .iter()
        .filter(|e| e.kind == "calls")
        .map(|e| (e.path.as_str(), e.dst.as_str(), e.line))
        .collect()
}

#[test]
fn imported_class_keeps_unknown_receivers_unknown_with_or_without_docs() {
    let mut facts = fixture();
    let graph = linker::link(&facts);
    let target = facts["a.py"]
        .nodes
        .iter()
        .find(|n| n.name == "method")
        .unwrap();
    assert_eq!(
        calls(&graph),
        [
            ("consumer.py", target.id.as_str(), 4),
            ("consumer.py", target.id.as_str(), 5)
        ]
    );
    assert!(graph
        .coverage
        .iter()
        .any(|c| c.expression == "obj.method" && c.status == "unresolved"));
    let owners = BTreeSet::from(["consumer.py".to_owned()]);
    let scoped = linker::link_owners(&facts, Some(&owners));
    assert_eq!(calls(&graph), calls(&scoped));
    facts.remove("docs/api.md");
    assert_eq!(calls(&graph), calls(&linker::link(&facts)));
}

#[test]
fn documentary_suffix_discovery_keeps_all_candidates_and_exact_precedence() {
    let mut facts = fixture();
    let graph = linker::link_owners(&facts, Some(&BTreeSet::from(["docs/api.md".to_owned()])));
    let mut targets: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.kind == "documents")
        .map(|e| &e.dst)
        .collect();
    targets.sort();
    let mut expected: Vec<_> = facts
        .values()
        .flat_map(|f| &f.nodes)
        .filter(|n| n.name == "method")
        .map(|n| &n.id)
        .collect();
    expected.sort();
    assert_eq!(targets, expected);
    facts.get_mut("docs/api.md").unwrap().docs =
        docs::extract("docs/api.md", "# API\n`a.Foo.method`\n", 0.).unwrap();
    let graph = linker::link(&facts);
    let targets: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.kind == "documents")
        .map(|e| &e.dst)
        .collect();
    assert_eq!(
        targets,
        [&facts["a.py"]
            .nodes
            .iter()
            .find(|n| n.name == "method")
            .unwrap()
            .id]
    );
}
