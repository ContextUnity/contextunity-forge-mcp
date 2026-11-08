# Python Framework Receiver Tests

`frameworks.rs` loads the Python framework contract groups:

- `import_provenance.rs`: external imports, aliases, and local framework members.
- `routing.rs`: route decorators and mapping calls.
- `manifest_packs.rs`: Starlette and FastAPI manifest relations.
- `gridviewspec.rs`: GridViewSpec manifest bindings.

`django.rs` owns persisted Django/DRF member providers, finite return status,
receiver authority and class identity, and unique ViewSet router handlers.

`lineage.rs` owns imported framework providers, local protocol targets, unproved
receiver negatives, and linked-project identity isolation. Its source workspaces
remain physical and independently built through the production writer.

The read-only Django fixture is built once through the production writer; every
test receives an independent SQLite backup. Assertions remain in domain tests.
Keep framework mutation and fresh cold comparisons on their physical workspaces.

Run `cargo test --test languages languages::python::frameworks::` and
`cargo clippy --test languages --all-features -- -D warnings`.
