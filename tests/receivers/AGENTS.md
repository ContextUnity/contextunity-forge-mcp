# Value Flow Receivers Test Suite

Subtree tests for receiver inference, type propagation, and method dispatch.

## Routing and Architecture

Tests are divided by language backend into focused subdirectories:

- `rust/`: Rust field chains, trait dispatch, pointer receivers, builtin receivers, value-flow cases, and cross-file module paths.
- `python/`: Python bindings, branch joins, factories, collections, member resolution, and inheritance cases.

## Non-Negotiable Invariants

- Public value-flow integration cases live here; private reducer unit tests stay beside source code.
- Every receiver claim must test exact resolution separately from possible targets.
- Exercise complete joins, incomplete joins, and unknown alternatives; possible targets must never appear as exact `calls` edges.
- Split test files strictly by logical domain and contract boundaries;
  avoid artificial splits solely for line count.

The module path filter uses the backend module in `typed.rs`; focused commands
such as `rust::field_chains::` and `python::branch_joins::` run inside the
existing `linker` Cargo target.

## Verification Commands

Run full receiver test suite:
```bash
cargo test --test linker receivers::
```

Run language-specific receiver suite:
```bash
cargo test --test linker rust::
cargo test --test linker python::
```
