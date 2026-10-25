# Python Value Flow and MRO Test Suite

Subtree tests for Python receiver binding, branch joins, factories, collections, and class members.

## Scope and Modules

The current cases are grouped by their actual contracts and logical domains; keep each contract coherent:

- `c3_mro.rs`: Existing imported-factory inheritance and `super()` baselines.
- `bindings.rs`, `branch_joins.rs`: Binding provenance, invalidation, and joined receiver states.
- `factories.rs`, `collections.rs`: Callable return contracts and finite collection members.
- `builtin_members.rs`, `class_members.rs`, `fields.rs`, `external_api.rs`: Member/provider boundaries.
- `return_boundaries.rs`: Async, decorated-factory, and annotated-return boundaries.

The current suite has no test for C3 precedence or a mixin class. Do not describe
the two inheritance baselines as proving C3 ordering, and do not create a
`mixins.rs` module until a real mixin contract is admitted.

## Shared Helpers and Fixtures

- `tests/common/`: Shared temp workspace harness and assertions.
- `tests/languages/support.rs`: Python fixture builders and dependency registries.

## Verification Commands

Run all Python receiver tests:
```bash
cargo test --test linker python::
```

Run targeted module:
```bash
cargo test --test linker python::c3_mro::
cargo test --test linker python::branch_joins::
cargo test --test linker python::factories::
```

## Rules and Invariants

- Preserve exact imported-provider identity through inheritance and `super()` calls.
- Ensure type-only imports under `TYPE_CHECKING` guards are marked as type hints, not runtime callables.
- Keep test files focused. Split only where a real contract boundary makes the suite easier to understand or run.
