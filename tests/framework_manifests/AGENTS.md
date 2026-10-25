# Framework Manifests Test Suite

Subtree tests for declarative framework packs, TOML manifests, and syntax operations.

## Scope and Structure

The modules follow the current test boundaries; keep unrelated test groups
separate and do not create empty per-framework modules.

- `activation.rs` and `activation/user_manifests.rs`: bundled-pack activation, user-manifest validation, and typed rule formats.
- `fastapi.rs`: the standalone FastAPI dependency-injection relation contract.
- `cross_pack_relations.rs`: the persisted multi-pack integration contract.
- `finite_operations.rs` and its children: one sequential persisted contract; five operation cases, callable-import transitions, and linked-workspace target-context coverage share the original test wrapper.
- `dependencies.rs` and `dependency_discovery.rs`: package metadata and lockfile discovery.
- `commonjs.rs`: CommonJS import/export provenance and delta behavior.
- `manifest_deltas.rs`: manifest-only and manifest-change delta behavior.
- `support.rs`: helpers shared by these manifest tests.

Most framework-operation cases are generic and table-driven; keep them under
their semantic operation tests rather than giving them pack-specific labels.

## Shared Helpers and Fixtures

- `tests/common/`: Shared workspace RAII harnesses and temp directory isolation.
- `tests/languages/support.rs`: `Workspace::new()`, file writing, commitment verification.
- `tests/framework_manifests/support.rs`: Manifest-suite SQL and canonical-edge helpers.
- `tests/framework_manifests/finite_operations/support.rs`: Shared operation builders and cold/delta parity.

## Verification Commands

Run full framework manifest suite:
```bash
cargo test --test framework_manifests
```

Run targeted manifest modules:
```bash
cargo test --test framework_manifests fastapi::
cargo test --test framework_manifests activation::
cargo test --test framework_manifests finite_operations::
```

## Rules and Invariants

- Table-driven tests over function duplication: use structured cases (`for case in cases`) for parameter permutations.
- Drive assertions through real public seams (live SQLite and reader APIs).
- Split operation or workflow cases strictly by logical domain boundaries
  and distinct contracts; keep shared persistence and transition logic once.
- `cross_pack_relations.rs` remains an integrated fixture because its relations
  share one persisted graph contract.
- Ensure tests fail on assertion failure (exit code 101) for contract gate evidence.
