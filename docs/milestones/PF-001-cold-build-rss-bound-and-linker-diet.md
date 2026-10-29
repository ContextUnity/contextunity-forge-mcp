---
id: m-framework-indexing-regression-repair
title: Cold-build performance, RSS bound, HTML endpoint resolution repair, and Cargo workspace split
doc_type: contract
status: planned
depends_on:
- m-declarative-framework-manifests-and-ecosystem-packs
owners:
- src/core/
- src/db/writer.rs
- src/db/writer/
- src/engine/ast/
- src/engine/languages/
- src/engine/linker.rs
- src/engine/linker/
- src/engine/pipeline.rs
- Cargo.toml
- crates/
- tests/languages/
- tests/incremental/
- tests/commitment_integrity/
- docs/architecture/
- docs/reference/
invariants:
- 'INV-REPAIR-COVERAGE: Performance repairs preserve correct resolution statuses, exact endpoint targets, cold/delta equivalence, persisted facts, and deterministic commitments.'
- 'INV-REPAIR-DECLARATIVE: Framework conventions remain in manifests; extractors and shared handlers contain only universal language or rule-processing logic.'
- 'INV-REPAIR-PERFORMANCE: Preserve single-pass extraction, typed analysis facts, shared prepared rule/index ownership, bounded persistence, and no source rereads during delta, as required by the existing architecture and ADR 0012.'
- 'INV-WORKSPACE-BOUNDARIES: Splitting the repository into forge-core, forge-index, and forge-tasks enforces strict unidirectional dependency boundaries with zero circular crate dependencies.'
tasks:
- restore-framework-indexing-performance
- restore-default-html-endpoint-resolution
- split-cargo-workspace-into-independent-crates
---

# Cold-build performance, RSS bound, HTML endpoint resolution repair, and Cargo workspace split

## Outcome and boundaries

Repair the cold-build slowdown and HTML endpoint-resolution losses introduced
by the framework work, bound cold-build peak RSS, and split the Cargo workspace
into 3 independent crates: `forge-core`, `forge-index`, and `forge-tasks`.
Keep new SQL, topology, and search capabilities in
[LS-002](LS-002-sql-schema-indexing-entrypoints-and-topology-slices.md).

Use the existing production paths, repository verification commands, and shared
test suites. This milestone introduces no custom benchmark scripts or additional
measurement procedure. Run benchmarks only when explicitly requested by the user.
Follow [ADR 0012](../adr/0012-zero-regression-serialization-and-cold-build-latency.md)
and the [performance architecture](../architecture/performance.md).

Broader framework rule unification, parameterized route trie indexing across
all client languages, Tree-sitter kind-id dispatch, and linker symbol interning
are implemented directly in milestone 022.

## Tasks

### task: restore-framework-indexing-performance

```yaml
task_ref: restore-framework-indexing-performance
target: "Remove the cold-indexing performance regression introduced by framework extraction and linking changes while preserving their correct resolution results"
proof_policy: direct-proof
contract_revision: 1
scope:
  - src/core/typed_facts.rs
  - src/core/semantic.rs
  - src/db/writer.rs
  - src/db/writer/
  - src/engine/ast/
  - src/engine/languages/
  - src/engine/linker.rs
  - src/engine/linker/
  - src/engine/pipeline.rs
  - tests/languages/
  - tests/incremental/
  - tests/commitment_integrity/
  - docs/architecture/performance.md
  - docs/architecture/indexing.md
status: ready
subtasks:
  - subtask_ref: remove-introduced-redundant-work
    title: "Find and remove redundant traversal, rule processing, index construction, and fact conversion introduced by the framework changes; keep required analysis intact"
    status: ready
  - subtask_ref: preserve-resolution-and-persistence
    title: "Verify unchanged correct targets, cold/delta behavior, cache hydration, and commitments through existing shared seams"
    status: ready
```

#### Acceptance

1. The production cold build no longer has the introduced slowdown relative to
   the comparison binary on the same control repository and configuration.
2. Repairs remove demonstrated redundant work and preserve analysis precision;
   they do not disable rules, omit files or references, weaken validation, or
   trade coverage for speed.
3. Existing affected tests and strict Clippy pass. When the user requests a speed
   check, use the repository's existing benchmark tooling and ready binaries;
   report actual wall and CPU results without introducing a separate driver.

### task: restore-default-html-endpoint-resolution

```yaml
task_ref: restore-default-html-endpoint-resolution
target: "Restore correct default-pack resolution of the 14 real Commerce HTMX endpoint references that changed from resolved to unresolved"
proof_policy: direct-proof
contract_revision: 1
scope:
  - src/engine/ast/routes.rs
  - src/engine/languages/html.rs
  - src/engine/languages/html/
  - src/engine/languages/manifests/
  - src/engine/languages/semantic.rs
  - src/engine/linker/framework_relations.rs
  - src/engine/linker/html_templates.rs
  - src/engine/linker/resolution.rs
  - tests/languages/html/framework_relations.rs
  - docs/reference/framework-manifests.md
  - docs/architecture/indexing.md
status: ready
subtasks:
  - subtask_ref: trace-default-pack-route-loss
    title: "Trace the real lost references from HTML extraction through default manifests and verified route providers to coverage and calls_endpoint edges"
    status: ready
  - subtask_ref: restore-generic-parameterized-url-linking
    title: "Repair universal rule handling or bundled declarations so verified parameterized HTMX requests resolve without project-local workaround manifests"
    status: ready
```

#### Acceptance

1. Verify all 14 real lost occurrences and their expected backend targets: four
   channel actions in `channels/settings.html`, six image requests in
   `products/editor/_images.html`, three review actions in
   `taxonomy_review/_drawer.html`, and one pane request in
   `technologies/_set_detail.html`.
2. The standard bundled configuration resolves those requests to the correct
   endpoints with `calls_endpoint` edges; local `.forge/frameworks` overrides
   retain their declared behavior.
3. Preserve HTTP-method checks, verified provider identity, literal path
   boundaries, and ambiguity handling. Unsupported dynamic expressions remain
   unresolved; matching an arbitrary dynamic URL to an arbitrary route is not
   an accepted repair.
4. Add necessary cases to the existing shared HTML framework seam. Verify cold,
   delta, and persisted/reopened results without creating one test per defect.

### task: split-cargo-workspace-into-independent-crates

```yaml
task_ref: split-cargo-workspace-into-independent-crates
target: "PF-001-T05: Split the monolithic crate into three independent Cargo workspace crates: forge-core, forge-index, and forge-tasks to decouple domains, enforce unidirectional dependency boundaries, and accelerate incremental compilation"
proof_policy: direct-proof
contract_revision: 1
scope:
  - Cargo.toml
  - crates/forge-core/
  - crates/forge-index/
  - crates/forge-tasks/
  - src/
  - tests/
  - docs/architecture/performance.md
status: ready
subtasks:
  - subtask_ref: extract-forge-core-crate
    title: "Extract domain models, string interner, configuration, and Merkle commitments into forge-core crate with zero parser or database dependencies"
    status: ready
  - subtask_ref: extract-forge-index-crate
    title: "Extract Tree-sitter parsers, AST extraction pipelines, declarative ruleset engine, linker resolution, and SQLite storage into forge-index crate"
    status: ready
  - subtask_ref: extract-forge-tasks-crate
    title: "Extract ACDD task lifecycle, blackboard coordination, proof verification, and worktree automation into forge-tasks crate"
    status: ready
  - subtask_ref: update-workspace-dependencies-and-imports
    title: "Configure Cargo workspace root, wire crate dependencies, and update crate:: imports across all repository modules and test suites"
    status: ready
```

#### Acceptance

1. Cargo workspace root in `Cargo.toml` declares members `["crates/forge-core", "crates/forge-index", "crates/forge-tasks", "."]`.
2. `forge-core` contains fundamental domain types, interned identifiers (`PathId`, `SymbolId`), configuration, and Merkle commitments without dependencies on AST parsing or SQLite.
3. `forge-index` contains Tree-sitter parsers, AST extraction pipelines, declarative manifest engine, cross-language linkers, and SQLite index persistence, depending only on `forge-core`.
4. `forge-tasks` contains ACDD lifecycle management, blackboard coordination, proof verification, and worktree isolation, depending on `forge-core` and SQLite task schemas.
5. All tests, benchmarks, and CLI commands compile cleanly (`cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`), with incremental compilation times reduced significantly.

## Shared verification

Review the performance repairs and workspace split together and run affected
existing suites as one batch, following [TESTS.md](../../TESTS.md). Keep
implementation changes out of task planning and preserve the five existing
LS-002 feature contracts. Static ACDD seam verification belongs to AC-001.
