# Rust Value Flow and Method Semantics

Subtree tests for Rust receiver inference, chained field access, trait resolution, and module-qualified calls.

## Scope and Modules

Each file covers one receiver-resolution mechanism:

- `builtin_receivers.rs`: finite standard constructors, builtin members, generic arguments, and `str` members.
- `field_chains.rs`: Struct fields, tuple indices (`holder.0.work()`), and nominal parameter receivers.
- `trait_dispatch.rs`: Static trait dispatch, dynamic trait objects (`dyn Trait`), and UFCS calls.
- `deref_pointers.rs`: Smart pointer unwrapping (`Box`, `Arc`, `Rc`), `&T` references, and `Deref` coercions.
- `module_paths.rs`: Explicitly qualified cross-file calls (`super::`, `super::super::`, `crate::`, `self::`, and associated functions `Path::Type::fn`).
- `value_flow/`: Binding, structural-call, and lifetime-contract cases.

## Shared Helpers and Fixtures

- `tests/common/`: Shared temporary workspaces and SQLite verification.
- `tests/linker/`: Linker test fixtures, AST extractors, and graph verification utilities.

## Verification Commands

Run all Rust receiver tests:
```bash
cargo test --test linker rust::
```

Run targeted module:
```bash
cargo test --test linker rust::field_chains::
cargo test --test linker rust::trait_dispatch::
cargo test --test linker rust::deref_pointers::
cargo test --test linker rust::builtin_receivers::
cargo test --test linker rust::module_paths::
```

## Rules and Invariants

- Exact `calls` edges require proving concrete callee target identity.
- Verify that module-qualified cross-file calls do not regress to `callers: 0` or single-file filtering.
- Group test modules strictly by semantic dispatch domain (field chains,
  trait dispatch, deref pointers, module paths). Keep coherent contract cases intact.
