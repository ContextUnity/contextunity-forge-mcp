use crate::languages::harness::ExtractorHarness;

#[test]
fn receiver_identity_preserves_proven_constructors_and_fields() {
    let source = r#"
struct Service;
impl Service { fn finish(&self) {} fn len(&self) {} }
struct Opaque;
fn build() -> Service { Service }
fn unknown() -> Opaque { Opaque }
fn proven() { build().finish(); build().len(); }
fn unproven(workspace: Opaque, root: Opaque, conn: Opaque, row: Opaque, client: Opaque, profile: Opaque, m: Opaque) {
    workspace.write(); root.join(); conn.query_row(); row.get(); client.call(); profile.id(); m.unwrap(); unknown().unwrap();
}
"#;
    let graph = ExtractorHarness::for_lang("rust")
        .at_path("dynamic_receivers.rs")
        .linked(source);
    let facts = &graph.facts;
    let targets: Vec<String> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "method" && matches!(node.name.as_str(), "finish" | "len"))
        .map(|node| node.id.clone())
        .collect();
    assert_eq!(targets.len(), 2);
    for expression in ["build().finish", "build().len"] {
        let reference = facts
            .references
            .iter()
            .find(|r| r.expression == expression)
            .unwrap();
        assert!(reference.receiver_hint.is_some(), "{reference:#?}");
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "resolved", "{coverage:#?}");
    }
    for expression in [
        "workspace.write",
        "root.join",
        "conn.query_row",
        "row.get",
        "client.call",
        "profile.id",
        "m.unwrap",
        "unknown().unwrap",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "unresolved", "{coverage:#?}");
    }
    for target in targets {
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == target)
                .count(),
            1
        );
    }
    let fields = [
        "ordinary",
        "iter",
        "next",
        "get",
        "unwrap",
        "arg",
        "query_row",
        "filter_map",
        "as_slice",
    ];
    let mut source = String::from("#[derive(Clone, Copy)] struct Leaf; impl Leaf { fn finish(&self) {} } struct Other; impl Other { fn stop(&self) {} } struct Holder {");
    for field in fields {
        source.push_str(&format!("{field}: Leaf,"));
    }
    source.push_str(
        "} impl Holder { fn iter(&self) -> Other { Other } } fn probe(holder: Holder) {\n",
    );
    for field in fields {
        source.push_str(&format!(
            "holder.{field}.finish(); let assigned_{field} = holder.{field}; assigned_{field}.finish();\n"
        ));
    }
    source.push_str("holder.iter().stop(); }");
    let graph = ExtractorHarness::for_lang("rust")
        .at_path("field_adapter_names.rs")
        .linked(&source);
    let finish = graph
        .facts
        .nodes
        .iter()
        .find(|node| node.name == "finish")
        .unwrap()
        .id
        .clone();
    let stop = graph
        .facts
        .nodes
        .iter()
        .find(|node| node.name == "stop")
        .unwrap()
        .id
        .clone();
    for field in fields {
        for expression in [
            format!("holder.{field}.finish"),
            format!("assigned_{field}.finish"),
        ] {
            let row = graph
                .coverage
                .iter()
                .find(|row| row.expression == expression)
                .unwrap();
            assert_eq!(row.status, "resolved", "{row:#?}");
        }
    }
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == finish)
            .count(),
        fields.len() * 2
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == stop)
            .count(),
        1,
        "method application must retain its own return type"
    );
}
