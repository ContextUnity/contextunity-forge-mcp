# Language Profile Tests

`../profiles.rs` owns registry and module-convention contracts and the shared
persisted graph selector.

- `baseline.rs`: public facts, typed profile hooks, and persisted commitments.
- `cache.rs`: grammar cache isolation by profile and actual grammar.
- `legacy_hooks.rs`: legacy declaration and exact root preparation hooks.
- `forwarding/`: Rust forwarding mutations and Cargo root transitions.
- `imports.rs`: persisted language and workspace import identity.
- `workspaces.rs`: package roots, Go siblings, macros, and receiver origins.
- `popular.rs`: optional-language persisted calls, coverage, diagnostics, and deltas.

Parser fixtures stay in RAM. Root, workspace, persistence, delta, and commitment
contracts retain their production pipeline. Each forwarding cold comparison
builds a new database from the current physical source tree.

Run `cargo test --test languages languages::profiles::` and
`cargo clippy --test languages --all-features -- -D warnings`.
