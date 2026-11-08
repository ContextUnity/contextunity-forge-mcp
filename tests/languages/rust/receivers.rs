use super::{coverage, linked};

#[test]
fn computed_external_receivers_preserve_provider_evidence() {
    let source = r#"
            use std::process::Command;
            use serde_json::Value;
            use rusqlite::{Connection, Statement, Row};
            type Syntax<'a> = tree_sitter::Node<'a>;
            fn probe_chained(opt: Option<&str>) {
                let s = opt.unwrap();
                let is_emp = s.trim().is_empty();
                opt.and_then(|s| Some(s)).unwrap().trim().is_empty();
            }
            fn probe_external(cmd: &mut Command, json: &Value) {
                cmd.args(["x"]).output();
                cmd.arg("x").output();
                json.get("key").unwrap().as_str().unwrap().trim().is_empty();
                json.as_array().unwrap().is_empty();
            }
            fn probe_references(conn: &Connection, stmt: &Statement, row: &Row, node: &Syntax<'_>) {
                conn.execute("", []); stmt.column_count(); row.get(0); node.kind();
            }
        "#;
    let graph = linked("src/probe_task6_chain.rs", source);
    let is_empty_ref = graph
        .coverage
        .iter()
        .find(|c| c.expression.ends_with("is_empty"))
        .expect("is_empty coverage entry");
    assert_eq!(
            is_empty_ref.status, "external",
            "Task 6 Seam Contract: chained s.trim().is_empty() must classify as external Rust standard library built-in: {is_empty_ref:#?}"
        );
    assert!(
        is_empty_ref.evidence.contains("Rust standard library"),
        "evidence must confirm Rust standard library built-in: {is_empty_ref:#?}"
    );
    for expression in [
        "opt.and_then(|s| Some(s)).unwrap().trim().is_empty",
        "cmd.args([\"x\"]).output",
        "cmd.arg(\"x\").output",
        "json.get(\"key\").unwrap().as_str().unwrap().trim().is_empty",
        "json.as_array().unwrap().is_empty",
        "conn.execute",
        "stmt.column_count",
        "row.get",
        "node.kind",
    ] {
        let entry = coverage(&graph, expression);
        assert_eq!(entry.status, "external", "{entry:#?}");
    }
}

#[test]
fn and_then_resolves_only_identity_payload_callbacks() {
    for (label, expression, resolved) in [
        (
            "identity",
            "opt.and_then(|value| Some(value)).unwrap()",
            true,
        ),
        ("changed", "opt.and_then(|_| Some(B)).unwrap()", false),
        ("unknown", "opt.and_then(|_| opaque()).unwrap()", false),
        ("callback", "opt.and_then(callback).unwrap()", false),
        (
            "reassigned",
            "{ let mut value = opt.and_then(|v| Some(v)).unwrap(); value = opaque(); value }",
            false,
        ),
    ] {
        let source = format!("struct A; impl A {{ fn finish(&self) {{}} }} struct B; impl B {{ fn finish(&self) {{}} }} fn probe(opt: Option<A>) {{ let result = {expression}; result.finish(); }}");
        let graph = crate::languages::harness::ExtractorHarness::for_lang("rust")
            .at_path("callbacks.rs")
            .linked(&source);
        let caller = graph
            .facts
            .nodes
            .iter()
            .find(|node| node.name == "probe")
            .unwrap();
        let target = graph
            .facts
            .nodes
            .iter()
            .find(|node| node.qualname.ends_with(".A.finish"))
            .unwrap();
        let calls: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.src == caller.id && edge.dst == target.id)
            .collect();
        assert_eq!(
            calls.len(),
            usize::from(resolved),
            "{label}: {:#?}",
            graph.coverage
        );
    }
}

const PAYLOAD_CASES: &[(&str, &str, bool)] = &[
    ("filter", "filter(|_| true)", true),
    ("take", "take(1)", true),
    ("skip", "skip(1)", true),
    ("step_by", "step_by(1)", true),
    ("collect_into_iter", "collect::<Vec<_>>().into_iter()", true),
    ("map_constant", "map(|_| B)", false),
    ("map_conversion", "map(convert)", false),
    ("enumerate", "enumerate()", false),
    ("map_identity", "map(|item| item)", false),
];

const ITERATOR_MEMBERS: &[(&str, &str)] = &[
    ("filter", "|_| true"),
    ("map", "|item| item"),
    ("filter_map", "|item| Some(item)"),
    ("flat_map", "|item| Some(item)"),
    ("inspect", "|_| {}"),
    ("fuse", ""),
    ("collect", ""),
    ("enumerate", ""),
    ("take", "1"),
    ("skip", "1"),
    ("for_each", "|_| {}"),
];

fn owned_iterator_graph() -> crate::languages::harness::LinkedFacts {
    let mut source = String::from(
        "struct Item; impl Item { fn finish(&self) {} }\n\
         struct A; impl A { fn finish(&self) {} }\n\
         struct B; impl B { fn finish(&self) {} }\n\
         fn convert(_: A) -> B { B }\n\
         trait Finish { fn finish(&self); }\n\
         impl Finish for (usize, A) { fn finish(&self) {} }\n",
    );
    for (index, (member, arguments)) in ITERATOR_MEMBERS.iter().enumerate() {
        source.push_str(&format!("fn probe_{index}(items: Vec<Item>) {{ let stream = items.into_iter(); stream.{member}({arguments}); }}\n"));
    }
    for (index, (_, adapter, _)) in PAYLOAD_CASES.iter().enumerate() {
        source.push_str(&format!(
            "fn payload_{index}(items: Vec<A>) {{ let mut adapted = items.into_iter().{adapter}; let item_{index} = adapted.next().unwrap(); item_{index}.finish(); }}\n"
        ));
    }
    source.push_str(
        "fn iterator_results(items: Vec<Item>) {\
         items.iter().flat_map(|item| [item]).count();\
         items.iter().inspect(|_| {}).count();\
         items.iter().fuse().count();\
         let changed = items.iter().flat_map(|_| [1]).collect();\
         for item in changed { item.finish(); }\
         }\n",
    );
    source.push_str("fn guards(option: Option<Item>, result: Result<Item, ()>, items: Vec<Item>) { option.is_some_and(|_| true); option.is_none_or(|_| true); result.is_ok_and(|_| true); result.is_err_and(|_| true); let slice = items.as_slice(); slice.len(); slice.is_empty(); let stream = items.into_iter(); stream.made_up(); }\n");
    source.push_str("struct Inner { nodes: Vec<Item> } struct Facts { inner: Inner } struct Root(std::path::PathBuf); fn fields(facts: &Facts, root: &Root) { for node in facts.inner.nodes.as_slice().iter() { node.finish(); } let nodes = facts.inner.nodes.iter().filter(|_| true).collect(); for collected in nodes { collected.finish(); } root.0.join(\"child\"); }\n");
    crate::languages::harness::ExtractorHarness::for_lang("rust")
        .at_path("owned_iterators.rs")
        .linked(&source)
}

#[test]
fn iterator_adapters_keep_receivers_and_reject_unknown_payloads() {
    let graph = owned_iterator_graph();
    for expression in ITERATOR_MEMBERS
        .iter()
        .map(|(member, _)| format!("stream.{member}"))
        .chain(
            [
                "option.is_some_and",
                "option.is_none_or",
                "result.is_ok_and",
                "result.is_err_and",
                "slice.len",
                "slice.is_empty",
            ]
            .map(str::to_owned),
        )
    {
        let observed = coverage(&graph, expression);
        assert_eq!(observed.status, "external", "{observed:#?}");
    }

    let unknown = coverage(&graph, "stream.made_up");
    assert_eq!(unknown.status, "unresolved", "{unknown:#?}");

    let target = graph
        .facts
        .nodes
        .iter()
        .find(|node| node.name == "finish" && node.details["receiver_type"] == "A")
        .unwrap();
    for (index, (label, adapter, preserves_item)) in PAYLOAD_CASES.iter().enumerate() {
        let source = format!(
            "struct A; impl A {{ fn finish(&self) {{}} }}\n\
             struct B; impl B {{ fn finish(&self) {{}} }}\n\
             fn convert(_: A) -> B {{ B }}\n\
             trait Finish {{ fn finish(&self); }}\n\
             impl Finish for (usize, A) {{ fn finish(&self) {{}} }}\n\
             fn run(items: Vec<A>) {{ let mut adapted = items.into_iter().{adapter}; let item = adapted.next().unwrap(); item.finish(); }}"
        );
        let facts = crate::languages::harness::ExtractorHarness::for_lang("rust")
            .at_path("iterator.rs")
            .extract(&source);
        assert!(facts.errors.is_empty(), "{adapter}: {facts:#?}");

        let expression = format!("item_{index}.finish");
        let observed = coverage(&graph, &expression);
        assert_eq!(
            observed.status,
            if *preserves_item {
                "resolved"
            } else {
                "unresolved"
            },
            "{label}: {observed:#?}"
        );
        let calls_a_target = graph.edges.iter().any(|edge| {
            edge.kind == "calls" && edge.evidence == expression && edge.dst == target.id
        });
        assert_eq!(calls_a_target, *preserves_item, "{label}: {graph:#?}");
    }

    let counts: Vec<_> = graph
        .coverage
        .iter()
        .filter(|row| row.expression.ends_with(".count"))
        .collect();
    assert_eq!(counts.len(), 3, "{graph:#?}");
    for row in counts {
        assert_eq!(row.status, "external", "{row:#?}");
    }
    let changed = coverage(&graph, "item.finish");
    assert_eq!(
        changed.status, "unresolved",
        "callback output must remain unknown: {changed:#?}"
    );

    for (expression, expected) in [
        ("node.finish", "resolved"),
        ("collected.finish", "resolved"),
        ("root.0.join", "external"),
    ] {
        let observed = coverage(&graph, expression);
        assert_eq!(observed.status, expected, "{observed:#?}");
    }
}

#[test]
fn query_callbacks_require_proven_rusqlite_receivers() {
    let source = "use rusqlite::{Connection, Statement}; struct Fake; impl Fake { fn query_row<F>(&self, callback: F) {} } fn probe(conn: &Connection, statement: &mut Statement, fake: Fake) { conn.query_row(\"sql\", [], |row| row.get(0)); conn.query_row::<String, _, _>(\"sql\", [], |typed| typed.get(0)); statement.query_map([], |r| r.get(0)); fake.query_row(|unproven| unproven.get(0)); }";
    let graph = linked("query_callbacks.rs", source);
    for (expression, expected) in [
        ("row.get", "external"),
        ("typed.get", "external"),
        ("r.get", "external"),
        ("unproven.get", "unresolved"),
    ] {
        let coverage = coverage(&graph, expression);
        assert_eq!(coverage.status, expected, "{coverage:#?}");
    }
}
