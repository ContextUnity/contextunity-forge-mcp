#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::models::{Facts, Graph},
    engine::{ast, docs, linker},
};
use std::collections::{BTreeMap, BTreeSet};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct LookupAllocationProbe;

thread_local! {
    static LOOKUP_ALLOCATION: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

#[global_allocator]
static LOOKUP_ALLOCATOR: LookupAllocationProbe = LookupAllocationProbe;

fn record_lookup_allocation(size: usize, align: usize, pointer: *mut u8) {
    if align == 1 && !pointer.is_null() {
        let _ = LOOKUP_ALLOCATION.try_with(|probe| {
            let (selected_size, count) = probe.get();
            if selected_size != 0 && size == selected_size {
                probe.set((selected_size, count + 1));
            }
        });
    }
}

unsafe impl GlobalAlloc for LookupAllocationProbe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record_lookup_allocation(layout.size(), layout.align(), pointer);
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record_lookup_allocation(layout.size(), layout.align(), pointer);
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        record_lookup_allocation(size, layout.align(), pointer);
        pointer
    }
}

fn measured_lookup_allocations<T>(size: usize, operation: impl FnOnce() -> T) -> (T, usize) {
    struct ResetProbe;
    impl Drop for ResetProbe {
        fn drop(&mut self) {
            LOOKUP_ALLOCATION.with(|probe| probe.set((0, 0)));
        }
    }
    LOOKUP_ALLOCATION.with(|probe| {
        assert_eq!(probe.get().0, 0, "allocation measurements must not nest");
        probe.set((size, 0));
    });
    let reset = ResetProbe;
    let result = operation();
    let allocations = LOOKUP_ALLOCATION.with(|probe| probe.get().1);
    drop(reset);
    (result, allocations)
}

#[test]
fn linker_borrows_arena_keys_and_reuses_qualified_lookup_storage() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let alias = "a".repeat(257);
    let parameter = "p".repeat(263);
    let function = "q".repeat(49);
    let mut failures = Vec::new();
    for (label, source, allocation_size, permitted_allocations, expression, status) in [
        (
            "import alias",
            format!("from a import Foo as {alias}\n"),
            alias.len(),
            0,
            "Foo",
            "resolved",
        ),
        (
            "single parameter binding",
            format!("def run({parameter}, hidden):\n    hidden()\n"),
            parameter.len(),
            0,
            "hidden",
            "unresolved",
        ),
        (
            "qualified lexical lookup",
            format!("def {function}():\n    return absent()\n"),
            "buf.".len() + function.len(),
            1,
            "absent",
            "unresolved",
        ),
    ] {
        let facts = BTreeMap::from([
            (
                "a.py".to_owned(),
                ast::extract("a.py", "python", "class Foo:\n    pass\n").unwrap(),
            ),
            (
                "buf.py".to_owned(),
                ast::extract("buf.py", "python", &source).unwrap(),
            ),
        ]);
        let baseline = pool.install(|| linker::link(&facts));
        let (graph, allocations) =
            pool.install(|| measured_lookup_allocations(allocation_size, || linker::link(&facts)));
        assert_eq!(
            serde_json::to_value((&graph.edges, &graph.coverage)).unwrap(),
            serde_json::to_value((&baseline.edges, &baseline.coverage)).unwrap(),
            "{label}: allocation optimization must preserve graph ordering and evidence"
        );
        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == "buf.py"
                    && coverage.expression == expression
                    && coverage.status == status
            }),
            "{label}: the measured production path must retain its resolution status"
        );
        if label == "import alias" {
            assert!(facts["buf.py"]
                .references
                .iter()
                .any(|reference| reference.alias.as_deref() == Some(alias.as_str())));
            assert!(graph
                .edges
                .iter()
                .any(|edge| edge.path == "buf.py" && edge.kind == "imports"));
        } else if label == "single parameter binding" {
            assert!(facts["buf.py"].nodes.iter().any(|node| {
                crate::common::assertions::projected_node_details(&facts["buf.py"], node)
                    ["bindings"]
                    .as_array()
                    .is_some_and(|bindings| bindings.iter().any(|binding| binding == &parameter))
            }));
            assert!(graph.coverage.iter().any(|coverage| {
                coverage.expression == "hidden"
                    && coverage.evidence
                        == "callee is shadowed by a parameter or local binding of unknown callable identity"
            }));
        } else {
            assert!(facts["buf.py"]
                .nodes
                .iter()
                .any(|node| { node.name == function && node.qualname.len() == allocation_size }));
            assert!(!graph.edges.iter().any(|edge| edge.kind == "calls"));
        }
        if allocations > permitted_allocations {
            failures.push(format!(
                "{label}: {allocations} name/scope-sized allocations ({allocation_size} bytes); permitted {permitted_allocations}"
            ));
        }
    }
    for (source, expected_lines, unresolved_line) in [
        (
            "from a import Foo\ndef run():\n    receiver = Foo()\n    receiver.method()\n",
            vec![4],
            None,
        ),
        (
            "from a import Foo\ndef run():\n    receiver = Foo()\n    receiver.method()\n    receiver = unknown()\n    receiver.method()\n    receiver = Foo()\n    receiver.method()\n",
            vec![4, 8],
            Some(6),
        ),
    ] {
        let facts = BTreeMap::from([
            (
                "a.py".to_owned(),
                ast::extract("a.py", "python", "class Foo:\n    def method(self): pass\n")
                    .unwrap(),
            ),
            (
                "buf.py".to_owned(),
                ast::extract("buf.py", "python", source).unwrap(),
            ),
        ]);
        let graph = pool.install(|| linker::link(&facts));
        let method = facts["a.py"]
            .nodes
            .iter()
            .find(|node| node.name == "method")
            .unwrap();
        let lines: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == method.id)
            .map(|edge| edge.line)
            .collect();
        assert_eq!(lines, expected_lines);
        if let Some(line) = unresolved_line {
            assert!(graph.coverage.iter().any(|coverage| {
                coverage.line == line
                    && coverage.expression == "receiver.method"
                    && coverage.status == "unresolved"
            }));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

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
fn documentary_links_require_unique_candidates_and_keep_exact_precedence() {
    let mut facts = fixture();
    assert_eq!(
        facts
            .values()
            .flat_map(|f| &f.nodes)
            .filter(|n| n.name == "method")
            .count(),
        2
    );
    let graph = linker::link_owners(&facts, Some(&BTreeSet::from(["docs/api.md".to_owned()])));
    assert!(!graph
        .edges
        .iter()
        .any(|e| matches!(e.kind.as_str(), "documents" | "references_doc")));
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
    facts.remove("b.py");
    facts.get_mut("docs/api.md").unwrap().docs =
        docs::extract("docs/api.md", "# API\n`Foo.method`\n", 0.).unwrap();
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
