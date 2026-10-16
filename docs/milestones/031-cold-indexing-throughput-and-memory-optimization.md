---
id: m-cold-indexing-throughput-and-memory-optimization
title: Cold indexing throughput optimization, memory budgeting, and in-memory Merkle sealing
doc_type: contract
status: planned
depends_on:
- m-language-parsers-and-declarative-framework-packs
owners:
- src/core/commitments.rs
- src/core/commitments/
- src/engine/pipeline.rs
- src/engine/cache.rs
- src/db/writer.rs
- src/db/ingest.rs
- docs/architecture/
- benchmarks/
invariants:
- 'INV-BENCHMARK-EACH-TASK: Every task in this milestone requires strict before-and-after benchmark measurements on the control repository (commerce-release-update); no task may be accepted without verified throughput and memory profiling.'
- 'INV-NO-COLD-REGRESSION: Cold build indexing duration on the control repository (commerce-release-update) must decrease by at least 20% (elapsed time <= 80%) compared to baseline commit cf4aa29 under identical hardware and thread configuration.'
- 'INV-DETERMINISTIC-MERKLE: In-memory Merkle tree sealing produces bit-identical commitment digests matching existing SQLite table seal queries across all 17 domains (13 logical in-memory + 4 SQLite FTS search domains).'
- 'INV-ZERO-SEMANTIC-REGRESSION: Symbol counts, edge counts, reference resolution, and commitments remain bit-identical across cargo test --all-targets.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
---

# Cold indexing throughput optimization, memory budgeting, and in-memory Merkle sealing

## Outcome and purpose

Deliver maximum cold build indexing throughput and memory efficiency on the control repository `commerce-release-update` under strict benchmark-driven verification:
1. Implement a staged producer-consumer pipeline with byte-budgeted backpressure queue overlapping disk I/O, Rayon tree-sitter parsing, and SQLite ingestion.
2. Implement retained-memory reduction (freeing ASTs and transient facts before global linking) and SQLite turbo-ingestion mode (deferred secondary index creation and memory pragmas).
3. Implement hybrid in-memory Merkle snapshot sealing for the 13 logical domains, and a content-addressed extraction cache across worktrees for identical file contents.
4. Enforce mandatory before-and-after benchmarks on `commerce-release-update` for every single task in this milestone.

---

## Tasks in this milestone

### task: staged-producer-consumer-pipeline-and-memory-budget

```yaml
task_ref: staged-producer-consumer-pipeline-and-memory-budget
target: "Implement staged producer-consumer pipeline with byte-budgeted queue and verify throughput gains on control repository"
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/engine/pipeline.rs
  - src/engine/scanner/mod.rs
  - src/db/writer.rs
  - docs/architecture/performance.md
  - tests/commitment_integrity.rs
  - benchmarks/
status: ready
subtasks:
  - subtask_ref: record-task1-pre-benchmark
    title: "Record baseline cold build elapsed time, phase breakdown, and peak RSS on commerce-release-update"
    status: pending
  - subtask_ref: staged-streaming-pipeline-implementation
    title: "Implement staged producer-consumer pipeline overlapping file discovery, parsing, and SQLite ingestion with bounded byte-budget backpressure derived from limits.memory_budget_mb"
    status: pending
  - subtask_ref: record-task1-post-benchmark
    title: "Record post-implementation benchmarks on commerce-release-update proving throughput gain without memory spikes"
    status: pending
```

#### Acceptance

1. Cold build pipeline concurrently overlaps file scanning, Tree-sitter parsing, and SQLite row ingestion.
2. Queue bounds use estimated byte budgets derived from `limits.memory_budget_mb` / `FORGE_MEMORY_BUDGET_MB` to prevent memory exhaustion on large generated files.
3. Pre- and post-benchmarks on `commerce-release-update` are recorded, demonstrating measurable throughput improvement with zero semantic regression.

---

### task: retained-memory-reduction-and-turbo-ingestion

```yaml
task_ref: retained-memory-reduction-and-turbo-ingestion
target: "Decouple transient extraction data, free ASTs early before linking, and evaluate SQLite turbo-ingestion mode"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/db/writer.rs
  - src/db/ingest.rs
  - src/core/models.rs
  - docs/architecture/performance.md
  - tests/commitment_integrity.rs
  - benchmarks/
status: ready
subtasks:
  - subtask_ref: record-task2-pre-benchmark
    title: "Record baseline memory profile and SQLite ingestion phase timing on commerce-release-update"
    status: pending
  - subtask_ref: retained-memory-reduction
    title: "Separate transient AST/source facts from long-lived linker data, releasing AST trees and duplicate fact maps before global linking"
    status: pending
  - subtask_ref: sqlite-turbo-ingestion-mode
    title: "Evaluate and implement unindexed bulk insertion with post-load secondary index creation and chunked transaction batching against baseline PRAGMAs, proving measured speedup"
    status: pending
  - subtask_ref: record-task2-post-benchmark
    title: "Record post-implementation benchmarks on commerce-release-update proving peak RSS reduction and faster ingestion"
    status: pending
```

#### Acceptance

1. Tree-sitter ASTs and transient fact maps are freed before global linking begins, cutting peak RSS significantly.
2. SQLite turbo-ingestion mode (deferred secondary indexes and batch chunking) is evaluated with measured proof of ingestion speedup over baseline without compromising snapshot durability.
3. Strict before-and-after benchmarks on `commerce-release-update` confirm latency and memory gains.

---

### task: in-memory-merkle-sealing-and-content-addressed-cache

```yaml
task_ref: in-memory-merkle-sealing-and-content-addressed-cache
target: "Compute 13 logical domain commitments directly in memory and implement cross-worktree content-addressed extraction cache"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/core/commitments/snapshot.rs
  - src/core/commitments.rs
  - src/db/writer.rs
  - src/engine/cache.rs
  - docs/architecture/indexing.md
  - docs/architecture/performance.md
  - tests/commitment_integrity.rs
  - benchmarks/
status: ready
subtasks:
  - subtask_ref: record-task3-pre-benchmark
    title: "Record baseline database sealing latency and multi-worktree re-indexing time on commerce-release-update"
    status: pending
  - subtask_ref: in-memory-logical-domain-hasher
    title: "Compute 13 logical domain commitments in memory from post-linking projection, preserving 4 SQLite search domains"
    status: pending
  - subtask_ref: cross-worktree-content-addressed-cache
    title: "Implement content-addressed extraction cache keyed by file hash, path, grammar ID, extractor_version, and effective ruleset hash to skip re-parsing identical files across worktrees"
    status: pending
  - subtask_ref: record-task3-post-benchmark
    title: "Record post-implementation benchmarks on commerce-release-update proving bit-identical Merkle sealing and fast worktree caching"
    status: pending
```

#### Acceptance

1. 13 logical domain commitments (graph domains from in-memory projection; file/error/inventory/doc sections from streaming buffers) are computed without querying SQLite tables.
2. The four SQLite search domains and metadata retain exact SQL table hashing; `tests/commitment_integrity.rs` passes bit-identically.
3. Content-addressed extraction cache key includes file hash, path, grammar ID, extractor version, and effective rule set hash, eliminating redundant Tree-sitter parsing across worktrees while guaranteeing zero stale cache reuse on manifest changes.
4. Separate benchmarks verify both cold-cache (empty cache) and warm-cache (cache hit) states; cumulative cold build elapsed time is $\le 80\%$ of baseline `cf4aa29` (equivalent to $\ge 25\%$ throughput increase) under identical hardware and thread configuration.

---

### task: pipeline-and-core-config-decomposition

```yaml
task_ref: pipeline-and-core-config-decomposition
target: "Extract indexing pipeline orchestration into src/engine/pipeline.rs, isolate configuration in src/core/config.rs, update db/symbols.rs, reader.rs, and ingest.rs, and introduce typed NodeSemanticMeta"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/engine/pipeline.rs
  - src/engine/mod.rs
  - src/core/config.rs
  - src/core/mod.rs
  - src/core/models.rs
  - src/core/semantic.rs
  - src/core/typed_facts.rs
  - src/engine/scanner/mod.rs
  - src/engine/scanner/adapter.rs
  - src/engine/languages/
  - src/db/writer.rs
  - src/db/delta.rs
  - src/db/lint.rs
  - src/db/symbols.rs
  - src/db/reader.rs
  - src/db/ingest.rs
  - build.rs
  - docs/architecture/indexing.md
  - docs/architecture/performance.md
  - benchmarks/
  - tests/core/storage_batches.rs
  - tests/incremental/
status: ready
subtasks:
  - subtask_ref: record-task4-pre-benchmark
    title: "Record baseline cold build elapsed time and compile check time on commerce-release-update"
    status: pending
  - subtask_ref: extract-engine-pipeline
    title: "Move indexing orchestration out of src/db/ into src/engine/pipeline.rs, eliminating engine imports from all src/db/ modules (including writer.rs, delta.rs, lint.rs, symbols.rs, reader.rs, and ingest.rs)"
    status: pending
  - subtask_ref: isolate-core-config-and-language-hygiene
    title: "Move Adapter, AdapterFile, and ScannerLimits into src/core/config.rs, and move ReceiverCatalog and TypeScriptBindings into src/core/semantic.rs to decouple linker from language internals"
    status: pending
  - subtask_ref: typed-node-semantic-meta-roundtrip
    title: "Define NodeSemanticMeta for AST bindings and receiver hints with verified persistence round-trip and cold-vs-delta equivalence"
    status: pending
  - subtask_ref: record-task4-post-benchmark
    title: "Record post-implementation benchmarks on commerce-release-update confirming zero regression and clean decoupled architecture"
    status: pending
```

#### Acceptance

1. `src/db/` (including `writer.rs`, `delta.rs`, `lint.rs`, `symbols.rs`, `reader.rs`, and `ingest.rs`) does not import `src/engine/` analysis, scanner, or linking modules, strictly fulfilling `INV-DECOUPLED-PIPELINE`.
2. `src/engine/pipeline.rs` owns the cold and incremental build workflow.
3. `Adapter` and `ScannerLimits` are accessible from `src/core/config.rs` as a strict leaf dependency; `ReceiverCatalog` and bindings reside in `src/core/semantic.rs`.
4. `NodeSemanticMeta` maintains exact symbol binding and receiver hint round-tripping across cold and delta builds.
5. `docs/architecture/indexing.md` and `docs/architecture/performance.md` reflect the decoupled architecture.
6. Pre- and post-benchmarks on `commerce-release-update` confirm clean execution with zero regressions.
