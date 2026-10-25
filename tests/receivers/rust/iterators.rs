use super::super::*;

#[test]
fn rust_standard_collection_adapters_preserve_element_receivers() {
    let source = r#"
        #[derive(Clone, Copy)]
        struct Service;
        impl Service { fn work(&self) {} }

        fn cloned(items: Vec<Service>) {
            let cloned_payload = items.iter().cloned().next().unwrap();
            cloned_payload.work();
        }

        fn dereferenced(option: Option<Box<Service>>) {
            let dereferenced_payload = option.as_deref().unwrap();
            dereferenced_payload.work();
        }

        fn owned(items: Vec<Service>) {
            let owned_payload = items.into_iter().next().unwrap();
            owned_payload.work();
        }

        fn slice(items: &[Service]) {
            let slice_payload = items.iter().copied().next().unwrap();
            slice_payload.work();
        }

        fn borrowed_slice(items: &[Service]) {
            let borrowed_payload = items.into_iter().next().unwrap();
            borrowed_payload.work();
        }

        fn option_cloned(value: Option<&Service>) {
            let cloned_option_payload = value.cloned().unwrap();
            cloned_option_payload.work();
        }

        fn option_copied(value: Option<&Service>) {
            let copied_option_payload = value.copied().unwrap();
            copied_option_payload.work();
        }

        fn mutable_option_cloned(mutable_cloned_value: Option<&mut Service>) {
            let mutable_payload = mutable_cloned_value.cloned().unwrap();
            mutable_payload.work();
        }

        fn mutable_option_copied(mutable_copied_value: Option<&mut Service>) {
            let mutable_copied_payload = mutable_copied_value.copied().unwrap();
            mutable_copied_payload.work();
        }

        mod custom {
            struct Service;
            impl Service { fn work(&self) {} }
            struct Vec<T>(T);

            fn run(items: Vec<Service>) {
                let payload = items.iter().cloned().next().unwrap();
                payload.work();
            }
        }
    "#;
    let facts = ast::extract("src/iterators.rs", "rust", source).unwrap();
    assert!(facts.errors.is_empty(), "{facts:#?}");

    let work = facts
        .nodes
        .iter()
        .find(|node| {
            node.kind == "method" && node.name == "work" && !node.qualname.contains("custom")
        })
        .unwrap();
    let work_id = work.id.clone();
    let graph = linker::link(&BTreeMap::from([("src/iterators.rs".to_owned(), facts)]));

    for expression in [
        "cloned_payload.work",
        "dereferenced_payload.work",
        "owned_payload.work",
        "slice_payload.work",
        "borrowed_payload.work",
        "cloned_option_payload.work",
        "copied_option_payload.work",
    ] {
        assert!(
            graph.edges.iter().any(|edge| {
                edge.kind == "calls" && edge.evidence == expression && edge.dst == work_id
            }),
            "{expression}: {graph:#?}"
        );
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| { row.expression == expression && row.status == "resolved" }),
            "{expression}: {graph:#?}"
        );
    }
    for expression in [
        "items.iter().cloned",
        "items.iter().cloned().next",
        "option.as_deref",
        "items.iter().copied",
        "items.iter().copied().next",
        "items.into_iter().next",
    ] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| { row.expression == expression && row.status == "external" }),
            "standard adapter {expression} was not recognized: {graph:#?}"
        );
    }
    for expression in ["mutable_cloned_value.cloned", "mutable_copied_value.copied"] {
        assert!(
            graph
                .coverage
                .iter()
                .any(|row| { row.expression == expression && row.status == "unresolved" }),
            "mutable references gained by-value adapter support: {graph:#?}"
        );
    }

    assert!(
        !graph
            .edges
            .iter()
            .any(|edge| { edge.kind == "calls" && edge.evidence == "payload.work" }),
        "local Vec gained standard collection adapter behavior: {graph:#?}"
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "payload.work" && row.status == "unresolved" }),
        "local Vec payload should remain unresolved: {graph:#?}"
    );
    assert!(
        !graph
            .edges
            .iter()
            .any(|edge| { edge.kind == "calls" && edge.evidence == "mutable_payload.work" }),
        "mutable Option reference gained cloned adapter behavior: {graph:#?}"
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|row| { row.expression == "mutable_payload.work" && row.status == "unresolved" }),
        "mutable Option reference should remain unresolved: {graph:#?}"
    );
    assert!(
        !graph
            .edges
            .iter()
            .any(|edge| { edge.kind == "calls" && edge.evidence == "mutable_copied_payload.work" }),
        "mutable Option reference gained copied adapter behavior: {graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|row| {
            row.expression == "mutable_copied_payload.work" && row.status == "unresolved"
        }),
        "mutable Option reference should remain unresolved: {graph:#?}"
    );
}
