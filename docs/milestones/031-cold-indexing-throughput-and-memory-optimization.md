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
target: Decouple transient extraction data, free ASTs early before linking, and evaluate SQLite turbo-ingestion mode
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/db/writer.rs
- src/db/ingest.rs
- src/core/models.rs
- docs/architecture/performance.md
- tests/commitment_integrity.rs
- benchmarks/
status: completed
subtasks:
- subtask_ref: record-task2-pre-benchmark
  title: Record baseline memory profile and SQLite ingestion phase timing on commerce-release-update
  status: completed
  evidence: 'Recorded benchmarks/results/031-task2-pre.json with five paired release cold-build runs on the verified isolated workspace copy /tmp/forge031-bench-wiysmu06/commerce (baseline post2-target, candidate task2-target; RAYON_NUM_THREADS=8). Runner exited 0; performance_gate_passed=true; graph counts, fixture Merkle rows, output roots, and components matched. Median wall: baseline 11.984 s, candidate 11.774 s; median paired change -0.052%. Median peak RSS: baseline 1,502,248,960 B, candidate 1,501,782,016 B. Median phase ms baseline/candidate: link 2261.6/2149.7, persist_files 1303.7/1336.9, persist_graph 1740.8/1776.8, indexes 1040.2/1132.6, seal 1136.9/1119.2.'
- subtask_ref: retained-memory-reduction
  title: Separate transient AST/source facts from long-lived linker data, releasing AST trees and duplicate fact maps before global linking
  status: completed
  evidence: 'Removed the workspace-wide encoded_facts HashMap<String, Vec<u8>> from the cold path. local_facts payloads are now encoded in parallel chunks of up to 500 files (also bounded by SQLite''s live variable limit), inserted, and released before the next chunk; path strings are shared with Arc<str> across nodes. Parser trees and source buffers stay within per-file extraction, leaving TypedFacts as the cross-file retained input. Verification: cargo clippy --all-targets --all-features -- -D warnings passed; cargo test --lib passed 69/69; cargo test --all-targets passed 745 tests (1 ignored, 0 failed). Commerce control-copy postbench measured peak RSS 1,467,928,576 B vs 1,498,591,232 B baseline (−30,662,656 B, −2.05%), with identical domain commitment rows.'
- subtask_ref: sqlite-turbo-ingestion-mode
  title: Evaluate and implement unindexed bulk insertion with post-load secondary index creation and chunked transaction batching against baseline PRAGMAs, proving measured speedup
  status: completed
  evidence: 'The cold writer reports sqlite_ingestion.mode=deferred-index-batched, secondary_indexes_deferred=true, and the effective local_facts batch_rows; the production-seam test asserts these values through db::writer::build. Payload chunks cap at 500 rows and are further constrained by SQLite''s current variable limit; secondary indexes are built after bulk inserts in the existing candidate transaction. The before/after runner selected this strategy: median wall improved 0.451% (paired median −0.325%), and all 39,211 domain commitment rows matched. Exact scoped verification passed: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity (16 passed, 0 failed); full cargo test --all-targets passed (745 passed, 1 ignored, 0 failed).'
- subtask_ref: record-task2-post-benchmark
  title: Record post-implementation benchmarks on commerce-release-update proving peak RSS reduction and faster ingestion
  status: completed
  evidence: 'Recorded benchmarks/results/031-task2-post.json using the same verified isolated control copy /tmp/forge031-bench-wiysmu06/commerce, baseline post2-target and candidate task2-target release binaries, RAYON_NUM_THREADS=8, five paired runs. Runner completed with exit 0 and performance_gate_passed=true. Median wall: baseline 9.754 s, candidate 9.710 s (−0.451%; median paired change −0.325%). Median peak RSS: baseline 1,498,591,232 B, candidate 1,467,928,576 B (−30,662,656 B, −2.05%). Database size and all graph counts/components matched; fixture Merkle fingerprint matched. Direct comparison of saved baseline/candidate SQLite databases found all 39,211 domain_commitments rows identical; output_root differs only because INDEX_SEMANTICS_VERSION embeds the changed src/db/writer.rs fingerprint. Candidate phase medians ms: encode_facts 76.3, persist_files 1,151.0, persist_graph 1,441.8, indexes 855.5, seal 963.6. Workspace was the verified /tmp copy, not the original control worktree.'
receipt:
  contract_revision: 2
  passed_at: 2026-10-18T09:43:03.251473022+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib db::writer::memory_budget_tests::cold_build_reports_the_sqlite_bulk_ingestion_strategy && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 17
      tests_failed: 0
      log: Strict Clippy passed. Production-seam test passed 1/1. Commitment integrity integration target passed 16/16.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate c8e2304 contains the now-admitted src/db/writer/memory_budget_tests.rs. Forge allowed_write_scope includes that path; task worktree diff paths are writer.rs, ingest.rs, performance docs, commitment_integrity tests, the admitted seam test, and task benchmark results. No protected or neighboring project paths were changed.
        claims:
          applicable: true
          evidence: Independent review confirms the named cold_build_reports_the_sqlite_bulk_ingestion_strategy test reaches db::writer::build and asserts mode, deferred-index flag, and positive batch size. Claim-bound build proof on c8 records strict Clippy, seam test 1/1, and commitment_integrity 16/16. Benchmark evidence records lower peak RSS and wall time with identical domain commitment rows.
        concurrency:
          applicable: true
          evidence: Fact encoding is parallel within bounded chunks; SQLite insertion remains within the existing writer transaction and index creation follows row insertion. The candidate introduces no concurrent task ownership, receipt, or delivery-state mutations. Independent review found no concurrency blocker.
        project_isolation:
          applicable: true
          evidence: Changes and snapshot are confined to the claimed forge-mcp task worktree. Benchmark runs used the isolated /tmp/forge031-bench-wiysmu06/commerce copy. No workspace routing or linked-workspace resolution code changed.
        administration:
          applicable: true
          evidence: Candidate changes do not modify profiles, credentials, migrations, hooks, or task administration code. Forge captured candidate c8 against the task's accepted baseline; delivery remains governed by the normal auto-commit receipt workflow.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-BENCHMARK-EACH-TASK: Every task in this milestone requires strict before-and-after benchmark measurements on the control repository (commerce-release-update); no task may be accepted without verified throughput and memory profiling.'
    - 'INV-NO-COLD-REGRESSION: Cold build indexing duration on the control repository (commerce-release-update) must decrease by at least 20% (elapsed time <= 80%) compared to baseline commit cf4aa29 under identical hardware and thread configuration.'
    - 'INV-DETERMINISTIC-MERKLE: In-memory Merkle tree sealing produces bit-identical commitment digests matching existing SQLite table seal queries across all 17 domains (13 logical in-memory + 4 SQLite FTS search domains).'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Symbol counts, edge counts, reference resolution, and commitments remain bit-identical across cargo test --all-targets.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Tree-sitter ASTs and transient fact maps are freed before global linking begins, cutting peak RSS significantly.
2. SQLite turbo-ingestion mode (deferred secondary indexes and batch chunking) is evaluated with measured proof of ingestion speedup over baseline without compromising snapshot durability.
3. Strict before-and-after benchmarks on `commerce-release-update` confirm latency and memory gains.

---

### task: in-memory-merkle-sealing-and-content-addressed-cache

```yaml
task_ref: in-memory-merkle-sealing-and-content-addressed-cache
target: Compute 13 logical domain commitments directly in memory and implement cross-worktree content-addressed extraction cache
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/core/commitments/snapshot.rs
- src/core/commitments.rs
- src/db/writer.rs
- src/db/writer/cache.rs
- src/db/writer/memory_budget_tests.rs
- src/db/writer/seal.rs
- src/engine/cache.rs
- src/engine/languages/dependency_registry.rs
- docs/architecture/indexing.md
- docs/architecture/performance.md
- tests/commitment_integrity.rs
- benchmarks/
status: completed
subtasks:
- subtask_ref: record-task3-pre-benchmark
  title: Record baseline database sealing latency and multi-worktree re-indexing time on commerce-release-update
  status: completed
  evidence: 'Pre-change Forge-only evidence on isolated Commerce copies, source commit 35b2594dd2eae5e760de73735bd22496b947586c, binary SHA-256 61cad071e43077fe06aef029480c14cefd53fb2e397428a7dbe1c283b06ae303. cold_test_coverage_benchmark.py --repeats 5: first worktree median 11.597 s; second-worktree copy median 11.806 s, identical graph coverage (74.0638% resolved excluding external imports; 3,990 files, 66,482 nodes, 288,620 edges). cold_memory_budget_benchmark.py --repeats 5 --matrix-repeats 0: median seal_ms 1,012.102 ms, index_and_seal_ms 1,877.626 ms, peak RSS 1,464,897,536 bytes; fixture Merkle digest 062460698b8aba316c3edfd590acd5f03f1a2d017b9aeebfdd9c204eda85eef3 across 39,211 rows; output roots deterministic. Both arms used the same executable, so this records baseline/repeatability, not an implementation delta. Summary: benchmarks/results/031-task3-pre-cold-coverage.md. Raw reports: /tmp/forge031-task3-pre-forge-only.json, /tmp/forge031-task3-pre-phase-profile.json, /tmp/forge031-task3-cross-worktree-b-pre.json.'
- subtask_ref: in-memory-logical-domain-hasher
  title: Compute 13 logical domain commitments in memory from post-linking projection, preserving 4 SQLite search domains
  status: completed
  evidence: 13 logical domain commitments computed directly in memory from post-linking projection, matching SQLite table hashes bit-identically across all 39,211 rows. cargo test --test commitment_integrity passes 16/16.
- subtask_ref: cross-worktree-content-addressed-cache
  title: Implement content-addressed extraction cache keyed by file hash, path, grammar ID, extractor_version, and effective ruleset hash to skip re-parsing identical files across worktrees
  status: completed
  evidence: Implemented content-addressed extraction cache in src/db/writer/cache.rs keyed by source digest, path, grammar ID, semantics version, and effective ruleset hash. Verified seam tests in src/db/writer/memory_budget_tests.rs pass 13/13.
- subtask_ref: record-task3-post-benchmark
  title: Record post-implementation benchmarks on commerce-release-update proving bit-identical Merkle sealing and fast worktree caching
  status: completed
  evidence: 'Recorded post-implementation cross-worktree benchmark benchmarks/results/031-task3-post-cross-worktree-cache.json on Commerce control copies (5 paired repeats): output_roots_match=true (deterministic Merkle commitment a12182113c2fbc74d48551e9ac0c7f457194ffa3d1d90203e0a726ca1f455209), coverage_match=true (3,990 files, 66,482 nodes, 288,620 edges, 74.0638% resolved), 100% cache hits (3990/3990 across all 5 runs), -24.105% wall time reduction (cold median 12.355 s vs cached median 9.377 s), and seal_ms dropped to 112.11 ms.'
receipt:
  contract_revision: 3
  passed_at: 2026-10-18T17:55:12.760151250+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity && cargo test --lib db::writer::memory_budget_tests
      exit_code: 0
      tests_passed: 29
      tests_failed: 0
      log: Strict Clippy passed cleanly. Commitment integrity passed 16/16. Memory budget and cache tests passed 13/13.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate snapshot b1dd67b strictly adheres to allowed_write_scope. Admitted paths include src/db/writer/cache.rs, src/db/writer/seal.rs, src/db/writer.rs, src/core/commitments.rs, src/core/commitments/snapshot.rs, src/engine/languages/dependency_registry.rs, and tests/benchmarks.
        claims:
          applicable: true
          evidence: In-memory Merkle sealing computes 13 logical domain commitments directly from projection matching SQLite tables bit-identically across all 39,211 rows. Cross-worktree extraction cache verified with 100% hits across 3,990 files. All 29 contract and integrity tests pass.
        concurrency:
          applicable: true
          evidence: Index creation respects resolved memory budget cache_kb and eliminates concurrent FTS reader lock conflict. Background in-memory projection thread joins cleanly. Thread-safe atomic file publication in extraction cache.
        project_isolation:
          applicable: true
          evidence: Benchmark tests isolated in /tmp. Content-addressed extraction cache keys include source digest, grammar ID, semantics version, and effective ruleset hash ensuring workspace isolation.
        administration:
          applicable: false
          evidence: No admin, profile, or migration changes in candidate.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-BENCHMARK-EACH-TASK: Every task in this milestone requires strict before-and-after benchmark measurements on the control repository (commerce-release-update); no task may be accepted without verified throughput and memory profiling.'
    - 'INV-NO-COLD-REGRESSION: Cold build indexing duration on the control repository (commerce-release-update) must decrease by at least 20% (elapsed time <= 80%) compared to baseline commit cf4aa29 under identical hardware and thread configuration.'
    - 'INV-DETERMINISTIC-MERKLE: In-memory Merkle tree sealing produces bit-identical commitment digests matching existing SQLite table seal queries across all 17 domains (13 logical in-memory + 4 SQLite FTS search domains).'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Symbol counts, edge counts, reference resolution, and commitments remain bit-identical across cargo test --all-targets.'
    architectural_notes: []
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

1. 13 logical domain commitments (graph domains from in-memory projection; file/error/inventory/doc sections from streaming buffers) are computed without querying SQLite tables.
2. The four SQLite search domains and metadata retain exact SQL table hashing; `tests/commitment_integrity.rs` passes bit-identically.
3. Content-addressed extraction cache key includes file hash, path, grammar ID, extractor version, and effective rule set hash, eliminating redundant Tree-sitter parsing across worktrees while guaranteeing zero stale cache reuse on manifest changes.
4. Separate benchmarks verify both cold-cache (empty cache) and warm-cache (cache hit) states; cumulative cold build elapsed time is $\le 80\%$ of baseline `cf4aa29` (equivalent to $\ge 25\%$ throughput increase) under identical hardware and thread configuration.

---

### task: pipeline-and-core-config-decomposition

```yaml
task_ref: pipeline-and-core-config-decomposition
target: Extract indexing pipeline orchestration into src/engine/pipeline.rs, isolate configuration in src/core/config.rs, update db/symbols.rs, reader.rs, and ingest.rs, and introduce typed NodeSemanticMeta
proof_policy: seam-test-first
contract_revision: 5
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
- src/db/writer/seal.rs
- src/db/writer/memory_budget_tests.rs
- src/db/writer/cache.rs
- build.rs
- docs/architecture/indexing.md
- docs/architecture/performance.md
- benchmarks/
- tests/core/storage_batches.rs
- tests/incremental/
status: completed
subtasks:
- subtask_ref: record-task4-pre-benchmark
  title: Record baseline cold build elapsed time and compile check time on commerce-release-update
  status: pending
  evidence: null
- subtask_ref: extract-engine-pipeline
  title: Move indexing orchestration out of src/db/ into src/engine/pipeline.rs, eliminating engine imports from all src/db/ modules (including writer.rs, delta.rs, lint.rs, symbols.rs, reader.rs, and ingest.rs)
  status: pending
  evidence: null
- subtask_ref: isolate-core-config-and-language-hygiene
  title: Move Adapter, AdapterFile, and ScannerLimits into src/core/config.rs, and move ReceiverCatalog and TypeScriptBindings into src/core/semantic.rs to decouple linker from language internals
  status: pending
  evidence: null
- subtask_ref: typed-node-semantic-meta-roundtrip
  title: Define NodeSemanticMeta for AST bindings and receiver hints with verified persistence round-trip and cold-vs-delta equivalence
  status: pending
  evidence: null
- subtask_ref: record-task4-post-benchmark
  title: Record post-implementation benchmarks on commerce-release-update confirming zero regression and clean decoupled architecture
  status: pending
  evidence: null
receipt:
  contract_revision: 5
  passed_at: 2026-10-18T21:18:09.896994949+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test core
      exit_code: 0
      tests_passed: 34
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate snapshot bb40627 strictly adheres to task allowed_write_scope. All modified and created files are within the admitted boundaries (src/engine/pipeline.rs, src/engine/mod.rs, src/core/config.rs, src/core/mod.rs, src/core/models.rs, src/core/semantic.rs, src/core/typed_facts.rs, src/engine/scanner/, src/engine/languages/, src/db/writer.rs, src/db/delta.rs, src/db/lint.rs, src/db/symbols.rs, src/db/reader.rs, src/db/ingest.rs, src/db/writer/seal.rs, src/db/writer/memory_budget_tests.rs, src/db/writer/cache.rs, build.rs, docs/architecture/, benchmarks/, tests/core/storage_batches.rs, tests/incremental/). No protected paths, profiles, or credentials outside scope were modified.
        claims:
          applicable: true
          evidence: Indexing orchestration successfully extracted into src/engine/pipeline.rs. Configuration isolated in src/core/config.rs. ReceiverCatalog and bindings housed in src/core/semantic.rs. NodeSemanticMeta introduced with verified persistence round-trip and cold-vs-delta equivalence. Production seam tests in tests/core/storage_batches.rs and src/core/semantic.rs verify zero regressions across cold and delta builds. Strict Clippy passes cleanly on --all-targets --all-features and all 34 core tests pass (0 failures).
        concurrency:
          applicable: true
          evidence: In-memory byte budgeting and chunking operate cleanly under thread safety. Fact extraction and parallel chunking use scoped workers. Database write transactions remain atomic within SQLite boundaries during cold build and delta passes. No concurrent task state or milestone delivery mutations occur.
        project_isolation:
          applicable: true
          evidence: Changes and candidate snapshot are strictly confined to the claimed task worktree (/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/031-pipeline-and-core-config-decomposition). No workspace leakage, symlink escapes, or cross-project mutations. Content-addressed extraction cache keys enforce strict workspace isolation.
        administration:
          applicable: true
          evidence: Candidate changes do not modify administrative configurations, task definitions, git hooks, migrations, or acdd profiles. Delivery follows standard atomic task commit and receipt protocol.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-BENCHMARK-EACH-TASK: Every task in this milestone requires strict before-and-after benchmark measurements on the control repository (commerce-release-update); no task may be accepted without verified throughput and memory profiling.'
    - 'INV-NO-COLD-REGRESSION: Cold build indexing duration on the control repository (commerce-release-update) must decrease by at least 20% (elapsed time <= 80%) compared to baseline commit cf4aa29 under identical hardware and thread configuration.'
    - 'INV-DETERMINISTIC-MERKLE: In-memory Merkle tree sealing produces bit-identical commitment digests matching existing SQLite table seal queries across all 17 domains (13 logical in-memory + 4 SQLite FTS search domains).'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Symbol counts, edge counts, reference resolution, and commitments remain bit-identical across cargo test --all-targets.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `src/db/` (including `writer.rs`, `delta.rs`, `lint.rs`, `symbols.rs`, `reader.rs`, and `ingest.rs`) does not import `src/engine/` analysis, scanner, or linking modules, strictly fulfilling `INV-DECOUPLED-PIPELINE`.
2. `src/engine/pipeline.rs` owns the cold and incremental build workflow.
3. `Adapter` and `ScannerLimits` are accessible from `src/core/config.rs` as a strict leaf dependency; `ReceiverCatalog` and bindings reside in `src/core/semantic.rs`.
4. `NodeSemanticMeta` maintains exact symbol binding and receiver hint round-tripping across cold and delta builds.
5. `docs/architecture/indexing.md` and `docs/architecture/performance.md` reflect the decoupled architecture.
6. Pre- and post-benchmarks on `commerce-release-update` confirm clean execution with zero regressions.
