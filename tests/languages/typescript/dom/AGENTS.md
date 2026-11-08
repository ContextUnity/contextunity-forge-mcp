# DOM Factory Tests

`factories.rs` owns persisted DOM factory return provenance, finite members,
literal arguments, shadowing, configured properties, and mapping disable/restore.
`typed.rs` separates typed, untyped, local-class, and optional-access linker
assertions in RAM from the corresponding persisted receiver contracts.
Other DOM parser and receiver contracts remain in `../dom.rs`.

Read-only factory tests receive independent SQLite backups of one real indexed
fixture. Mapping mutations retain their own filesystem workspace and real delta.
Do not use the persisted seed as a cold build or syntax fixture.

Run `cargo test --test languages languages::typescript::dom::` and
`cargo clippy --test languages --all-features -- -D warnings`.
