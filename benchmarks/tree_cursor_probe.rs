use std::{hint::black_box, path::PathBuf, time::Instant};
use tree_sitter::{Node, Parser, Query, QueryCursor, TreeCursor};

fn first_named<const LEAF_GUARD: bool>(cursor: &mut TreeCursor<'_>) -> bool {
    if LEAF_GUARD && cursor.node().named_child_count() == 0 {
        return false;
    }
    if !cursor.goto_first_child() {
        return false;
    }
    loop {
        if cursor.node().is_named() {
            return true;
        }
        if !cursor.goto_next_sibling() {
            cursor.goto_parent();
            return false;
        }
    }
}

fn walk<'tree, const LEAF_GUARD: bool>(
    root: Node<'tree>,
    mut visit: impl FnMut(Node<'tree>, bool),
) {
    let mut cursor = root.walk();
    visit(root, true);
    if !first_named::<LEAF_GUARD>(&mut cursor) {
        visit(root, false);
        return;
    }
    loop {
        let node = cursor.node();
        visit(node, true);
        if first_named::<LEAF_GUARD>(&mut cursor) {
            continue;
        }
        visit(node, false);
        loop {
            let mut moved = cursor.goto_next_sibling();
            while moved && !cursor.node().is_named() {
                moved = cursor.goto_next_sibling();
            }
            if moved {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
            let parent = cursor.node();
            visit(parent, false);
            if parent.id() == root.id() {
                return;
            }
        }
    }
}

fn main() {
    let paths =
        std::fs::read_to_string(std::env::args().nth(1).expect("path list")).expect("read paths");
    let language = tree_sitter_python::language();
    let query = Query::new(&language, "(_) @node").expect("compile query");
    let mut parser = Parser::new();
    parser.set_language(&language).expect("Python grammar");
    let mut files = 0;
    let mut named_nodes = 0;
    let mut manual_ns = 0;
    let mut query_ns = 0;
    let mut guarded_ns = 0;
    let mut query_mismatches = 0;
    let mut guarded_mismatches = 0;
    let mut cursor = QueryCursor::new();
    for path in paths.lines().map(PathBuf::from) {
        if path.extension().is_none_or(|ext| ext != "py") {
            continue;
        }
        let source = std::fs::read(&path).expect("source");
        let tree = parser.parse(&source, None).expect("parse");
        let root = tree.root_node();
        let mut expected = Vec::new();
        let mut events = Vec::new();
        walk::<false>(root, |node, entering| {
            events.push((node.id(), entering));
            if entering {
                expected.push(node.id());
            }
        });
        let mut guarded_events = Vec::new();
        walk::<true>(root, |node, entering| {
            guarded_events.push((node.id(), entering))
        });
        if guarded_events != events {
            guarded_mismatches += 1;
            if guarded_mismatches <= 4 {
                eprintln!("guarded event order differs: {}", path.display());
            }
        }
        let actual: Vec<_> = cursor
            .matches(&query, root, source.as_slice())
            .map(|found| found.captures[0].node.id())
            .collect();
        if actual != expected {
            query_mismatches += 1;
            if query_mismatches <= 4 {
                eprintln!(
                    "capture order differs: {}, query={}, cursor={}",
                    path.display(),
                    actual.len(),
                    expected.len()
                );
            }
        }
        files += 1;
        named_nodes += expected.len();
        for repeat in 0..6 {
            let mut manual = || {
                let started = Instant::now();
                walk::<false>(root, |node, entering| {
                    black_box((node.id(), entering));
                });
                manual_ns += started.elapsed().as_nanos();
            };
            let mut guarded = || {
                let started = Instant::now();
                walk::<true>(root, |node, entering| {
                    black_box((node.id(), entering));
                });
                guarded_ns += started.elapsed().as_nanos();
            };
            let mut captures = || {
                let started = Instant::now();
                for found in cursor.matches(&query, root, source.as_slice()) {
                    black_box(found.captures[0].node.id());
                }
                query_ns += started.elapsed().as_nanos();
            };
            match repeat {
                0 => {
                    manual();
                    captures();
                    guarded();
                }
                1 => {
                    captures();
                    guarded();
                    manual();
                }
                2 => {
                    guarded();
                    manual();
                    captures();
                }
                3 => {
                    guarded();
                    captures();
                    manual();
                }
                4 => {
                    captures();
                    manual();
                    guarded();
                }
                _ => {
                    manual();
                    guarded();
                    captures();
                }
            }
        }
    }
    let query_identical = query_mismatches == 0;
    let guarded_identical = guarded_mismatches == 0;
    println!("{{\"python_files\":{files},\"named_nodes\":{named_nodes},\"repeats\":6,\"manual_balanced_ns\":{manual_ns},\"guarded_balanced_ns\":{guarded_ns},\"query_entries_only_ns\":{query_ns},\"capture_preorder_identical\":{query_identical},\"guarded_events_identical\":{guarded_identical},\"query_mismatch_files\":{query_mismatches},\"guarded_mismatch_files\":{guarded_mismatches}}}");
}
