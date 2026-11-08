# Rust Language Tests

`mod.rs` provides the in-memory production linker and coverage selector.

- `syntax.rs`: AST declarations and value-flow projections in RAM.
- `closures.rs`: closure, tuple, binding, and callback inference in RAM.
- `receivers.rs`: computed receivers, iterators, guards, and callback payloads.
- `macros.rs`: declarative macro identity and receiver repair.
- `payloads.rs`: persisted deserialization payloads and effective Cargo policy.
- `workspace_factories.rs`: filesystem roots, crate visibility, canonical targets,
  cross-file factories, and persisted return policy.

Use `ExtractorHarness` for parser inputs. Preserve real filesystem and persisted
boundaries in workspace and policy tests. Read-only persisted baselines use one
production build and return an independent SQLite backup per test; mutation and
cold-build scenarios own fresh workspaces.

Run `cargo test --test languages languages::rust::` and
`cargo clippy --test languages --all-features -- -D warnings`.
