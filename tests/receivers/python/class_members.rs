use super::*;

#[test]
fn inferred_python_members_require_a_unique_class_provider() {
    let source = "flag = bool(input())\nother = bool(input())\nclass Stable:\n    def render(self): return 'stable'\nclass Dynamic:\n    if flag:\n        def render(self): return 'first'\n    else:\n        def render(self): return 'second'\n    if other:\n        def render(self): return 'third'\ndef use():\n    stable = Stable()\n    stable.render()\n    dynamic = Dynamic()\n    dynamic.render()\n    Dynamic.render(Dynamic())\n";
    let facts = ast::extract("providers.py", "python", source).unwrap();
    let stable = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with("Stable.render"))
        .unwrap()
        .id
        .clone();
    assert_eq!(
        facts
            .nodes
            .iter()
            .filter(|node| node.qualname.ends_with("Dynamic.render"))
            .count(),
        3
    );
    let graph = linker::link(&BTreeMap::from([("providers.py".to_owned(), facts)]));
    for (expression, expected) in [
        ("stable.render", "resolved"),
        ("dynamic.render", "ambiguous"),
        ("Dynamic.render", "ambiguous"),
    ] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            expected,
            "{expression}"
        );
    }
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.path == "providers.py"
        && edge.line == 14
        && edge.evidence == "stable.render"
        && edge.dst == stable));
}
#[test]
fn later_unconditional_python_method_overrides_conditional_providers() {
    let source = "flag = bool(input())\nclass Dynamic:\n    if flag:\n        def render(self): return 'first'\n    else:\n        def render(self): return 'second'\nclass Replaced:\n    if flag:\n        def render(self): return 'first'\n    else:\n        def render(self): return 'second'\n    def render(self): return 'final'\nclass FieldOverridden:\n    def render(self): return 'callable'\n    render = None\ndef use():\n    dynamic = Dynamic()\n    dynamic.render()\n    replaced = Replaced()\n    replaced.render()\n    field = FieldOverridden()\n    field.render()\n";
    let (_workspace, db) = build_workspace(&[("override.py", source)]);
    assert_eq!(
        persisted_coverage(&db, "override.py", "dynamic.render")[0].0,
        "ambiguous"
    );
    assert_eq!(
        persisted_coverage(&db, "override.py", "replaced.render")[0].0,
        "resolved"
    );
    assert_eq!(
        persisted_coverage(&db, "override.py", "field.render")[0].0,
        "unresolved"
    );
    let mut statement = db
        .prepare(
            "SELECT target.line FROM edge_occurrences AS occurrence \
         JOIN path_dictionary AS owner ON owner.path_id = occurrence.owner_id \
         JOIN coverage_evidence AS evidence ON evidence.evidence_id = occurrence.evidence_id \
         JOIN nodes AS target ON target.node_hash = occurrence.dst_hash \
         WHERE owner.path = ?1 AND occurrence.kind = 'calls' AND evidence.evidence = ?2 \
         AND (SELECT path FROM path_dictionary WHERE path_id=target.path_id) = ?1 AND target.qualname = ?3 ORDER BY target.line",
        )
        .unwrap();
    let targets: Vec<usize> = statement
        .query_map(
            ["override.py", "replaced.render", "override.Replaced.render"],
            |row| row.get(0),
        )
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(targets, [12], "{targets:?}");
}
