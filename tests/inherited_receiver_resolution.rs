#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::models::Graph,
    engine::{ast, linker},
};
use std::collections::BTreeMap;

fn linked(
    files: &[(&str, &str)],
) -> (
    BTreeMap<String, contextunity_forge_mcp::core::models::Facts>,
    Graph,
) {
    let facts = files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ast::extract(path, "python", source).unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
    (facts, graph)
}

fn status<'a>(graph: &'a Graph, expression: &str) -> &'a str {
    &graph
        .coverage
        .iter()
        .find(|coverage| coverage.expression == expression)
        .unwrap()
        .status
}

#[test]
fn inherited_local_methods_use_python_c3_and_real_targets() {
    let (facts, graph) = linked(&[("service.py", "class Root:\n    def work(self): pass\nclass Left(Root): pass\nclass Right(Root):\n    def work(self): pass\nclass Child(Left, Right):\n    def run(self): self.work()\ndef typed(client: Child): client.work()\n")]);
    let target = facts["service.py"]
        .nodes
        .iter()
        .find(|node| node.qualname == "service.Right.work")
        .unwrap();
    for expression in ["self.work", "client.work"] {
        assert_eq!(
            status(&graph, expression),
            "resolved",
            "{facts:#?}\n{graph:#?}"
        );
        assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == expression
            && edge.dst == target.id
            && edge.confidence == "inferred"));
    }
}

#[test]
fn imported_base_and_overrides_preserve_real_method_identity() {
    let (facts, graph) = linked(&[("base.py", "class Parent:\n    def inherited(self): pass\n"), ("child.py", "from base import Parent as Base\nclass Child(Base):\n    def run(self): self.inherited()\n")]);
    let target = facts["base.py"]
        .nodes
        .iter()
        .find(|node| node.name == "inherited")
        .unwrap();
    assert_eq!(
        status(&graph, "self.inherited"),
        "resolved",
        "{facts:#?}\n{graph:#?}"
    );
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.evidence == "self.inherited"
            && edge.dst == target.id
            && edge.confidence == "inferred"));
}

#[test]
fn verified_framework_bases_supply_only_known_members_without_edges() {
    let (_, graph) = linked(&[("commands.py", "from django.core.management.base import BaseCommand\nfrom unittest import TestCase\nclass Command(BaseCommand):\n    def handle(self):\n        self.stdout.write('ok')\n        self.made_up()\nclass Case(TestCase):\n    def test_run(self): self.assertEqual(1, 1)\n")]);
    for expression in ["self.stdout.write", "self.assertEqual"] {
        assert_eq!(status(&graph, expression), "external");
        assert!(graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap()
            .evidence
            .starts_with("call through external import "));
        assert!(!graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == expression));
    }
    assert_eq!(status(&graph, "self.made_up"), "unresolved");
}

#[test]
fn django_manager_requires_verified_lineage_and_no_override() {
    let (_, graph) = linked(&[("models.py", "from django.db import models\nclass Record(models.Model): pass\nclass Custom(models.Model):\n    objects = factory()\nclass Plain: pass\ndef run():\n    Record.objects.filter(active=True)\n    Custom.objects.get(id=1)\n    Plain.objects.all()\n    Record.objects.made_up()\n")]);
    assert_eq!(status(&graph, "Record.objects.filter"), "external");
    for expression in [
        "Custom.objects.get",
        "Plain.objects.all",
        "Record.objects.made_up",
    ] {
        assert_eq!(status(&graph, expression), "unresolved");
    }
}

#[test]
fn unknown_cycles_and_inconsistent_mro_do_not_invent_inherited_members() {
    let (_, graph) = linked(&[("bad.py", "class A(B):\n    def work(self): pass\nclass B(A):\n    def run(self): self.work()\nclass Known:\n    def inherited(self): pass\nclass Unknown(Missing, Known):\n    def run(self): self.inherited()\nclass X:\n    def conflicted(self): pass\nclass Y: pass\nclass XY(X, Y): pass\nclass YX(Y, X): pass\nclass Inconsistent(XY, YX):\n    def run(self): self.conflicted()\n")]);
    assert_eq!(status(&graph, "self.work"), "unresolved");
    assert_eq!(status(&graph, "self.inherited"), "unresolved");
    assert_eq!(status(&graph, "self.conflicted"), "unresolved");
    assert!(!graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == "self.conflicted"));
}

#[test]
fn external_ancestors_and_assignment_overrides_are_member_barriers() {
    let (_, graph) = linked(&[("barriers.py", "from vendor import Unknown\nfrom django.core.management.base import BaseCommand\nclass Local:\n    def work(self): pass\nclass Child(Unknown, Local):\n    def run(self): self.work()\nclass Command(BaseCommand):\n    stdout = custom\n    def run(self):\n        self.stdout.write('ok')\n        self.execute()\n    def execute(self): pass\n    execute = custom\n")]);
    for expression in ["self.work", "self.stdout.write", "self.execute"] {
        assert_eq!(status(&graph, expression), "unresolved", "{graph:#?}");
    }
    assert!(!graph.edges.iter().any(|edge| edge.kind == "calls"
        && matches!(
            edge.evidence.as_str(),
            "self.work" | "self.stdout.write" | "self.execute"
        )));
}

#[test]
fn computed_receivers_require_proven_classes_and_unshadowed_super() {
    let (facts, graph) = linked(&[("computed.py", "from pathlib import Path as FilePath\nimport pathlib as paths\nclass Parent:\n    def work(self): pass\nclass Builder(Parent):\n    def run(self): super().work()\nclass Rebound(Parent):\n    def run(self):\n        self = object()\n        super().work()\ndef run():\n    Builder().work()\n    FilePath(__file__).resolve()\n    paths.Path(__file__).read_text()\n    factory().work()\n    'hello'.upper()\ndef shadow(FilePath): FilePath(__file__).resolve()\n")]);
    let method = facts["computed.py"]
        .nodes
        .iter()
        .find(|node| node.qualname == "computed.Parent.work")
        .unwrap();
    for expression in ["Builder().work", "super().work"] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|coverage| coverage.expression == expression && coverage.status == "resolved"),
            "{graph:#?}"
        );
        assert!(graph.edges.iter().any(|edge| edge.evidence == expression
            && edge.dst == method.id
            && edge.confidence == "inferred"));
    }
    assert!(graph
        .coverage
        .iter()
        .any(|coverage| coverage.expression == "super().work" && coverage.status == "unresolved"));
    assert!(
        graph.coverage.iter().any(
            |coverage| coverage.expression == "FilePath(__file__).resolve"
                && coverage.status == "external"
        ),
        "{graph:#?}"
    );
    assert!(graph.coverage.iter().any(|coverage| coverage.expression
        == "FilePath(__file__).resolve"
        && coverage.status == "unresolved"));
    assert_eq!(status(&graph, "paths.Path(__file__).read_text"), "external");
    assert_eq!(status(&graph, "factory().work"), "unresolved");
    assert_eq!(status(&graph, "'hello'.upper"), "resolved");
}

#[test]
fn rebound_local_base_and_annotations_do_not_prove_receiver_identity() {
    let (_, graph) = linked(&[("rebound.py", "class Base:\n    def work(self): pass\nBase = factory()\nclass Child(Base):\n    def run(self): self.work()\ndef typed(client: Base): client.work()\ndef constructor(): Base().work()\n")]);
    for expression in ["self.work", "client.work", "Base().work"] {
        assert_eq!(status(&graph, expression), "unresolved", "{graph:#?}");
        assert!(!graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == expression));
    }
}

#[test]
fn a_later_external_factory_alias_does_not_reuse_path_constructor_proof() {
    let (_, graph) = linked(&[("aliases.py", "from pathlib import Path\nfrom unknown import factory as Path\ndef run(): Path(__file__).resolve()\n")]);
    assert_eq!(status(&graph, "Path(__file__).resolve"), "unresolved");
    assert!(!graph
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == "Path(__file__).resolve"));
}

#[test]
fn a_later_import_shadows_a_local_base_annotation_and_constructor() {
    let (_, graph) = linked(&[("shadow.py", "class Base:\n    def work(self): pass\nfrom vendor import Other as Base\nclass Child(Base):\n    def run(self): self.work()\ndef typed(client: Base): client.work()\ndef constructor(): Base().work()\n")]);
    for expression in ["self.work", "Base().work"] {
        assert_eq!(status(&graph, expression), "unresolved", "{graph:#?}");
    }
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| coverage.expression == "client.work"
                && coverage.status == "external"
                && coverage
                    .evidence
                    .contains("external import vendor at line 3")),
        "{graph:#?}"
    );
    for expression in ["self.work", "client.work", "Base().work"] {
        assert!(!graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == expression));
    }
}
