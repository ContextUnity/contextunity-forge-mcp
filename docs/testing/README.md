---
title: "Testing and verification"
doc_type: guide
---

# Testing and verification

## Test ownership

Read [tests/AGENTS.md](../../tests/AGENTS.md) before placing, moving, or changing
tests. It owns domain grouping, public seams, and benchmark placement.
[Root instructions](../../AGENTS.md) own engineering requirements;
[`TESTS.md`](../../TESTS.md) owns the required test and lint commands.

## Rust verification

Run from the repository root with the current Cargo manifest. Match the command
to the proof boundary. [`TESTS.md`](../../TESTS.md) owns the exact required
commands and the milestone handoff gate.

| Stage | Verification |
| --- | --- |
| Contract and build | Run the named test through its existing domain executable, for example `cargo test --test acdd tasks::TEST_NAME`. `seam-test-first` requires red then green. `direct-proof` and `deferred-final-test` accept exit code 0 at contract. All policies require passing build proof. |
| Task build and review | Run the affected domain executable, for example `cargo test --test acdd`, plus strict Clippy. Rerun focused checks after candidate changes. |
| Milestone deferred final test | Follow [`TESTS.md`](../../TESTS.md#milestone-handoff-gate) once after all task changes and the end-to-end test are in place. |

Confirm a filtered test runs at least one test; an unmatched filter can exit 0
without proving the contract. Verify the intended failure on the red run and
the actual pass count on the green run.

Use the appropriate domain suite for a task outside `acdd` (`core`, `languages`,
`framework_manifests`, `linker`, `incremental`, `mcp`). Run `cargo test --test commitment_integrity`
when changing commitment or storage invariants. When test targets or suites
change, update the affected instructions during `build` per the
[ACDD contract](../reference/acdd.md#documentation-and-instructions-co-evolution).

## Engine test module filters

The engine test suite follows a two-tier architecture: keeping a small number of top-level Cargo targets (`tests/*.rs`) to avoid linker bloat, while partitioning test cases by domain and subsystem boundaries for instantaneous targeted execution.

Run targeted test groups using the submodule path as the filter:

### 1. Framework Manifests (`tests/framework_manifests.rs`)

```bash
# Run entire framework manifest test suite
cargo test --test framework_manifests

# Targeted submodule filters
cargo test --test framework_manifests manifests::fastapi::
cargo test --test framework_manifests manifests::dependencies::
cargo test --test framework_manifests manifests::dependency_discovery::
cargo test --test framework_manifests manifests::commonjs::
cargo test --test framework_manifests manifests::cross_pack_relations::
cargo test --test framework_manifests manifests::activation::
cargo test --test framework_manifests manifests::finite_operations::
cargo test --test framework_manifests manifests::manifest_deltas::
```

### 2. Language Semantics & Templates (`tests/languages.rs`)

```bash
# Run entire languages test suite
cargo test --test languages

# HTML and template subtrees (tests/languages/html/)
cargo test --test languages html::django_templates::
cargo test --test languages html::jinja_templates::
cargo test --test languages html::htmx_relations::
cargo test --test languages html::alpine_relations::
cargo test --test languages html::browser_templates::

# TypeScript framework subtrees (tests/languages/typescript/frameworks/)
cargo test --test languages typescript::frameworks::nestjs::
cargo test --test languages typescript::frameworks::nuxt::
cargo test --test languages typescript::frameworks::vue_runtime::
```

### 3. Linker & Receiver Flow (`tests/linker.rs`)

```bash
# Run entire linker test suite
cargo test --test linker

# Rust receiver subtrees (tests/receivers/rust/)
cargo test --test linker typed_receivers::rust::field_chains::
cargo test --test linker typed_receivers::rust::trait_dispatch::
cargo test --test linker typed_receivers::rust::deref_pointers::
cargo test --test linker typed_receivers::rust::module_paths::

# Python receiver subtrees (tests/receivers/python/)
cargo test --test linker typed_receivers::python::c3_mro::

# Linker value-flow suite (tests/linker/value_flow.rs)
cargo test --test linker value_flow::
```

The test roots remain `tests/linker.rs`, `tests/languages.rs`, and
`tests/framework_manifests.rs`; the nested modules do not add Cargo targets.
Check that a filtered command reports at least one test, since Cargo accepts
filters that match no test names. Subtree invariants are documented in each
directory's local `AGENTS.md`.

Performance-sensitive changes also use the benchmark procedure in root
instructions. Record the exact binary, workspace inventory, commands, and results
with the owning work. Documentation changes use metadata, link, scan-admission,
and retrieval checks as their focused proof.

## Semantic resolution evidence

Exercise receiver and candidate contracts through the linker and, when they
affect persisted or public results, through indexing and query readers. Keep
exact call edges distinct from possible-target evidence. Assert both positive
resolution and negative cases for unknown alternatives, unsupported syntax,
incomplete joins, and candidate overflow; do not use name-only fixtures as proof
of a unique target. Query tests cover paged possible calls and analysis gaps;
impact tests cover candidate callers and removal safety. The Milestone 046 admission baseline is recorded in
[ADR 0008](../adr/0008-bounded-static-value-flow.md).

For language resolution and cross-language framework composition, run the full
`cargo test --test linker`, `cargo test --test languages`, and
`cargo test --test framework_manifests` targets. The `incremental` target checks
that cached and cold graph output remain equivalent.

The [`linker` suite](../../tests/linker.rs) registers public reducer cases in
[`value_flow.rs`](../../tests/linker/value_flow.rs), including write replay,
factory-return propagation, cycles, and constructor boundaries. Private reducer
implementation checks remain in the library unit tests.
