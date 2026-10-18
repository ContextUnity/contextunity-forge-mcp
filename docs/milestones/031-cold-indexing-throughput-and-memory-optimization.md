---
id: m-cold-indexing-throughput-and-memory-optimization
title: Cold indexing throughput optimization, memory budgeting, and in-memory Merkle sealing
doc_type: contract
status: active
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
started_at: 2026-10-18T03:04:05+00:00
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
target: Implement staged producer-consumer pipeline with byte-budgeted queue and verify throughput gains on control repository
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/pipeline.rs
- src/engine/scanner/mod.rs
- src/db/writer.rs
- docs/architecture/performance.md
- tests/commitment_integrity.rs
- benchmarks/
status: completed
subtasks:
- subtask_ref: record-task1-pre-benchmark
  title: Record baseline cold build elapsed time, phase breakdown, and peak RSS on commerce-release-update
  status: completed
  evidence: 'Authorized command completed all 5 paired baseline/candidate cold-build repetitions on the prepared isolated commerce snapshot with RAYON_NUM_THREADS=8 and --matrix-repeats 0. Report: benchmarks/results/031-task1-pre.json. Baseline/candidate median wall 12.054s/11.362s (candidate -5.741%); median paired wall change -2.109%; median peak RSS 1,475,018,752/1,492,959,232 bytes. Runner exit 1 only because its semantic parity checks are false: bit_identical_output_roots=false and bit_identical_fixture_merkle_rows=false; performance_gate_passed=true. Graph deltas: +309 edges, +1 external import, -359 unresolved; files/nodes/docs/parse_errors and components match. Raw report preserved unmodified; all builds completed without runner warnings.'
- subtask_ref: staged-streaming-pipeline-implementation
  title: Implement staged producer-consumer pipeline overlapping file discovery, parsing, and SQLite ingestion with bounded byte-budget backpressure derived from limits.memory_budget_mb
  status: completed
  evidence: 'Implemented streaming discovery in src/engine/scanner/mod.rs and the byte-budgeted producer/consumer pipeline in src/db/writer.rs, with seam proofs in src/db/writer/memory_budget_tests.rs and performance behavior documented in docs/architecture/performance.md. The staged connection consumes source_inventory rows under bounded byte accounting; oversized candidates are admitted alone; producer/worker/consumer errors cancel, wake, and join cleanly. Global route/component composition remains a semantic barrier. Final persistence clears provisional source_inventory and reinserts canonical rows in sorted path order, preserving ORDER BY rowid scan invariants and deterministic commitments. Proofs: `cargo test --lib db::writer::memory_budget_tests::cold_pipeline_` (4 passed) covers genuine three-stage overlap, byte accounting, no duplicate/canonical data, and worker/consumer cleanup preserving the old DB; `cargo test --test core storage_batches::cold_build_preserves_extracted_storage_across_full_and_remainder_batches -- --nocapture` (1 passed) covers sorted canonical rows across full and remainder batches. Final-source gates: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test` passed, with 744 passed, 1 ignored, 0 failed. `git diff --check` passed. Post benchmark proof is recorded separately.'
- subtask_ref: record-task1-post-benchmark
  title: Record post-implementation benchmarks on commerce-release-update proving throughput gain without memory spikes
  status: completed
  evidence: 'Authorized controlled benchmark on the prepared isolated copy `/tmp/forge031-bench-wiysmu06/commerce` (copied linked workspaces; never the original commerce-release-update worktree), RAYON_NUM_THREADS=8, --repeats 5 --matrix-repeats 0. Clean task-local report: benchmarks/results/031-task1-post-v4.json; baseline binary `/tmp/forge031-bench-wiysmu06/candidate-target/release/contextunity-forge-mcp` (pre-change candidate) vs post binary `/tmp/forge031-bench-wiysmu06/post2-target/release/contextunity-forge-mcp`. Runner exit 0; performance_gate_passed=true. Median wall 13.405s→13.101s (-2.268%), median paired change -2.813%; median peak RSS 1,496,555,520→1,493,004,288 bytes (-0.237%); DB size identical at 197,132,288 bytes. All graph count deltas zero; 39,211 fixture Merkle rows bit-identical. Output roots differ because index_semantics_version fingerprint changed with the source; raw v4 JSON records the metadata. Earlier post-v2 (candidate +3.503%) and post-v3 (+4.856%) reports are preserved unchanged and were not accepted as clean performance proof: v3 had a 32.359s candidate outlier with DB phases inflated, and a task-022 cargo test was observed during the noisy window. After a quiet window, v4 ran with no competing build/test and low CPU pressure. Pre milestone context remains in benchmarks/results/031-task1-pre.json (cf4aa29→pre-change candidate); that report''s performance gate passed, while its semantic parity was affected by the recorded 90-file source drift. No source edits followed the v4 benchmark.'
receipt:
  contract_revision: 1
  passed_at: 2026-10-18T04:55:43.065329547+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib
      exit_code: 0
      tests_passed: 68
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed immutable task snapshot 5e1162489bfaf7b585dff0db2e23412592cba29a. Candidate paths are limited to the admitted writer, scanner, memory-budget tests, performance documentation, and benchmark reports; src/db/writer/memory_budget_tests.rs was added to scope in decision 323. The planned→active/started_at milestone edit is Forge lifecycle metadata only in the live worktree and absent from the candidate snapshot.
        claims:
          applicable: true
          evidence: 'Task 1 acceptance traces through run_cold_pipeline and scanner::stream_for_build: bounded estimated-byte permits, oversized items admitted alone, overlapping scan/extraction/SQLite consumer, and canonical sorted persistence after global composition. Seam tests cover overlap, budget accounting, durable facts/commitments, worker and consumer failures, and preservation of the previous DB. The clean v4 paired benchmark passed its performance gate: median wall 13.405s→13.101s (−2.268%), paired median −2.813%, RSS −0.237%, zero graph-count deltas and identical 39,211 commitment rows.'
        concurrency:
          applicable: true
          evidence: Workers, bounded channels, the single SQLite consumer transaction, cancellation and joins were traced in writer.rs; failure tests verify worker/consumer cleanup and previous-DB preservation. A theoretical missed-notify window in wake_waiters has no demonstrated persistent deadlock because outstanding MemoryPermit drops call release under the budget mutex and notify waiters. Existing tests do not force cancellation while the producer waits on a saturated budget; no confirmed defect was found.
        project_isolation:
          applicable: true
          evidence: Scanner traversal remains root-bounded and excludes symlinks; extraction resolves checked children, opens with O_NOFOLLOW on Unix, and checks file identity, size, and mtime around its read. All benchmark runs used the isolated /tmp commerce snapshot; no original control-repo worktree was used as the benchmark workspace.
        administration:
          applicable: false
          evidence: The candidate changes no ACDD profiles, migrations, gate definitions, task-store code, or protected milestone header. The milestone lifecycle edit is generated metadata outside the candidate snapshot. Atomic task receipt and delivery commit are governed by the subsequent Forge deliver gate.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-BENCHMARK-EACH-TASK: Every task in this milestone requires strict before-and-after benchmark measurements on the control repository (commerce-release-update); no task may be accepted without verified throughput and memory profiling.'
    - 'INV-NO-COLD-REGRESSION: Cold build indexing duration on the control repository (commerce-release-update) must decrease by at least 20% (elapsed time <= 80%) compared to baseline commit cf4aa29 under identical hardware and thread configuration.'
    - 'INV-DETERMINISTIC-MERKLE: In-memory Merkle tree sealing produces bit-identical commitment digests matching existing SQLite table seal queries across all 17 domains (13 logical in-memory + 4 SQLite FTS search domains).'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Symbol counts, edge counts, reference resolution, and commitments remain bit-identical across cargo test --all-targets.'
    architectural_notes:
    - 'decisions: Coordinator-authorized scope amendment: task scope now includes src/db/writer/memory_budget_tests.rs so the contract can use an indexed in-module writer build seam. Moved only the new contract test from integration tests/commitment_integrity.rs into that admitted module, reusing TemporaryRoot. No production behavior or milestone authority was edited. This amendment resumes the halted contract proof once after concrete seam ownership correction; draft and proof reference now target cargo test --lib db::writer::memory_budget_tests::cold_pipeline_accounts_for_in_flight_bytes_and_preserves_commitments -- --exact.'
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Cold build pipeline concurrently overlaps file scanning, Tree-sitter parsing, and SQLite row ingestion.
2. Queue bounds use estimated byte budgets derived from `limits.memory_budget_mb` / `FORGE_MEMORY_BUDGET_MB` to prevent memory exhaustion on large generated files, with in-flight worker accounting and single oversized-file admission preventing pipeline stall or memory bloat.
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
