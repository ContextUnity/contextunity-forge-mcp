#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

#[cfg(feature = "lang-python")]
mod scope_allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Probe;

    thread_local! {
        static SELECTED: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    #[global_allocator]
    static ALLOCATOR: Probe = Probe;

    fn record(size: usize, align: usize, pointer: *mut u8) {
        if align == 1 && !pointer.is_null() {
            let _ = SELECTED.try_with(|selected| {
                let (expected, count) = selected.get();
                if expected != 0 && size == expected {
                    selected.set((expected, count + 1));
                }
            });
        }
    }

    unsafe impl GlobalAlloc for Probe {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, size) };
            record(size, layout.align(), pointer);
            pointer
        }
    }

    pub(super) fn measure<T>(size: usize, operation: impl FnOnce() -> T) -> (T, usize) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                SELECTED.with(|selected| selected.set((0, 0)));
            }
        }
        SELECTED.with(|selected| {
            assert_eq!(selected.get().0, 0);
            selected.set((size, 0));
        });
        let reset = Reset;
        let result = operation();
        let count = SELECTED.with(|selected| selected.get().1);
        drop(reset);
        (result, count)
    }
}

#[cfg(feature = "lang-go")]
#[test]
fn test_go_ast_extractor() {
    let source = r#"
package main

import "fmt"

type Server struct {
    port int
}

func (s *Server) Start() {
    fmt.Println(s.port)
}

func Run() {
    s := &Server{port: 8080}
    s.Start()
}
"#;
    let facts = crate::languages::harness::ExtractorHarness::for_lang("go")
        .at_path("main.go")
        .extract(source);
    assert!(facts.nodes.iter().any(|n| n.name == "Server"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "Start"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "Run"));
}

#[cfg(feature = "lang-python")]
#[test]
fn python_member_access_callee_does_not_emit_duplicate_reference() {
    let source = r#"
class ServiceClient:
    def fetch(self, endpoint: str) -> dict:
        return self._send_request(endpoint)

    def _send_request(self, endpoint: str) -> dict:
        return {"status": "ok"}
"#;
    let facts = crate::languages::harness::ExtractorHarness::for_lang("python")
        .at_path("service.py")
        .extract(source);
    let send_request_refs: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.expression == "self._send_request")
        .collect();
    assert_eq!(
        send_request_refs.len(),
        1,
        "Expected exactly 1 relation for self._send_request, got {:?}",
        send_request_refs
    );
    assert_eq!(send_request_refs[0].kind, "calls");

    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let scope = format!("project_{}", "s".repeat(211));
    workspace.write(
        format!("{scope}/pyproject.toml"),
        "[project]\nname = 'scope-probe'\nversion = '0.1.0'\n",
    );
    let dependencies =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(workspace.root()),
        );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let mut scope_copies = Vec::new();
    for count in [1, 32] {
        let path = format!("{scope}/service.py");
        let source = format!(
            "class ServiceClient:\n    def fetch(self, endpoint: str) -> dict:\n{}    def _send_request(self, endpoint: str) -> dict:\n        return {{'status': 'ok'}}\n",
            "        self._send_request(endpoint)\n".repeat(count),
        );
        let facts = crate::languages::harness::ExtractorHarness::for_lang("python")
            .at_path(&path)
            .extract(&source);
        assert_eq!(
            facts
                .references
                .iter()
                .filter(|reference| reference.expression == "self._send_request")
                .count(),
            count,
        );
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "_send_request")
            .unwrap()
            .id
            .clone();
        let all = std::collections::BTreeMap::from([(path, facts)]);
        let (graph, copies) = pool.install(|| {
            let _ = contextunity_forge_mcp::engine::linker::link_with_registry(
                &all,
                None,
                Some(workspace.root()),
                &dependencies,
            );
            scope_allocations::measure(scope.len(), || {
                contextunity_forge_mcp::engine::linker::link_with_registry(
                    &all,
                    None,
                    Some(workspace.root()),
                    &dependencies,
                )
            })
        });
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == target)
                .count(),
            count,
            "every member call must retain its exact target",
        );
        assert_eq!(
            graph
                .coverage
                .iter()
                .filter(|coverage| coverage.expression == "self._send_request"
                    && coverage.status == "resolved")
                .count(),
            count,
        );
        scope_copies.push(copies);
    }
    assert!(
        scope_copies[1] <= scope_copies[0],
        "assertion failed: package-scope preparation must be per file, not per member reference: {scope_copies:?}",
    );
}
