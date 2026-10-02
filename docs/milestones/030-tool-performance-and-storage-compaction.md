---
id: m-tool-performance-and-storage-compaction
title: "Tool performance hardening and SQLite database compaction"
doc_type: contract
status: active
depends_on:
  - m-language-semantics-and-resolution-coverage:completed
owners:
  - src/mcp/
  - src/db/
  - src/core/
  - src/cli/
  - src/engine/
  - benchmarks/
  - tests/
invariants:
  - "INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors."
  - "INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking under a 30ms latency budget."
  - "INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression."
  - "INV-COLD-BUILD-THROUGHPUT: Cold build maintains >= 400 files/sec (<= 2.5s per 1,000 files)."
---

# Tool performance hardening and SQLite database compaction

## Outcome and purpose

Deliver sub-30ms MCP tool query response times, localize `code_map_prove_removal` to target candidate dependencies instead of globally blocking on unrelated workspace errors, enable two-tier hybrid search with native SQLite FTS5 BM25, accelerate test discovery and AST grep, maintain database storage density <= 45 KiB per file via Zstandard fact compression, and sustain cold build throughput >= 400 files/sec (<= 2.5s per 1,000 files).

## Tasks in this milestone

### task: prove-removal-target-scoping

```yaml
task_ref: prove-removal-target-scoping
target: "Локалізувати перевірку code_map_prove_removal до залежностей цільового символу"
proof_policy: seam-test-first
scope:
  - src/db/traversal.rs
  - src/mcp/tools.rs
  - tests/
status: completed
receipt:
  passed_at: "2026-10-01T19:30:00Z"
  evidence:
    - "cargo test --test impact_context (3/3 passed)"
    - "cargo test --test tool_evolution (17/17 passed)"
    - "cargo test --test commitment_integrity (12/12 passed)"
    - "cargo clippy --all-targets --all-features -- -D warnings (0 warnings)"
    - "cargo test --all-targets (400 passed, 0 failed)"
    - "tests/impact_context.rs: removal_assessment_names_each_indexed_blocker_without_changing_verdict passes with target_safe_to_remove"
```

Replace global workspace queries (`SELECT count(*) FROM resolution_coverage`, `SELECT count(*) FROM errors`) in `src/db/traversal.rs` with target-scoped evaluation. Check unresolved references only where `expression == target_name` or within files that statically depend on the target symbol's module. Report `target_safe_to_remove: bool` independently of unrelated global workspace health.

### task: fts5-bm25-hybrid-ranking

```yaml
task_ref: fts5-bm25-hybrid-ranking
target: "Реалізувати гібридне ранжування з нативним SQLite FTS5 BM25 у code_map_search"
proof_policy: seam-test-first
scope:
  - src/core/schema.rs
  - src/db/symbols.rs
  - src/mcp/tools.rs
  - tests/
status: completed
receipt:
  measured_at: "2026-10-02T01:08:51.856644Z"
  implementation: "Native BM25, exact +1000, prefix +500, graph +50 within the fixed top-50 candidate window, include_docs=false, and grouped file output are implemented. When the bounded FTS count probe reaches 1,001 matches, total is null; has_more uses a limit+1 lookahead."
  median_ms: 19.4284625
  p95_ms: 23.436819
  acceptance: "Met: symbol, text, and prefix search p95 are below 30ms."
  evidence:
    - "benchmarks/milestone030_results.json: 30 measured calls per scenario, first call excluded"
    - "The same database reports text p95 24.24903ms and prefix p95 17.235928ms."
```

Enable SQLite native C-level `bm25(node_search)` scoring by configuring `node_search` virtual table with column detail. Implement two-tier hybrid ranking in `src/db/symbols.rs` (exact matches and prefix matches +1000/+500, FTS5 BM25 natural language score, graph degree boost +50). Separate documentation matches from code symbol search by default (`include_docs: false`) and support `group_by_file: bool` for LLM token economy.

#### Detailed Execution Plan
1. **Optimize Broad Symbol Query Plan (`p95`: 57.28 ms -> < 30 ms)**:
   - In `src/db/symbols.rs`, broad symbol queries generate a CTE with `UNION` between structural matches and FTS BM25 candidates:
     `WITH fts AS MATERIALIZED (...), candidates AS (...)`.
   - Applying early pushdown bounds (`LIMIT 500`) directly inside candidate CTE branches avoids materializing thousands of low-relevance full-text candidates prior to scoring.
   - Route case-insensitive exact queries through the existing FTS5 candidate index, then apply exact name and qualified-name filters. Keep binary-collation B-tree indexes for structural candidates.
2. **Verification Gate**:
   - `python3 benchmarks/milestone030_benchmark.py --root ... --db ...` proves all search scenarios achieve `p95 < 30 ms`; `code_map_tests` remains below 50ms.
   - Exact search uses the existing `node_search` FTS5 index for case-insensitive candidates. `nodes_data` keeps binary-collation B-tree indexes without duplicate NOCASE indexes.

---

```yaml
task_ref: test-discovery-and-ast-grep-acceleration
target: "Прискорити code_map_tests (обмежена глибина max_depth=4) та ast_grep_search (попередній FTS-фільтр файлів)"
proof_policy: seam-test-first
scope:
  - src/db/symbols.rs
  - src/cli/ast.rs
  - tests/
status: completed
receipt:
  measured_at: "2026-10-01T21:05:00Z"
  tests_p95_ms: 9.671382
  ast_workspace_p95_ms: 17.932487
  evidence:
    - "ast_grep_search prefilters candidates using node_search and files table, skipping unnecessary file parsing"
    - "Depth-four traversal and lexical fallback remain covered by the existing test-discovery contracts."
    - "benchmarks/milestone030_results.json: 30 measured calls per scenario, first call excluded"
```

In `src/db/symbols.rs` (`tests`), add recursion depth control (`max_depth = 4`) and lexical fallback for unlinked tests to prevent runaway graph traversals (reducing tail latency from 1,657ms to <50ms). In `src/cli/ast.rs`, replace full-disk scanning with pre-filtered candidate files from SQLite `files` and FTS5 token search, dropping p95 latency from 1,960ms to <30ms.

### task: mcp-inventory-scan-debouncing

```yaml
task_ref: mcp-inventory-scan-debouncing
target: "Усунути 30-50 мс оверхед сканування файлової системи перед кожним MCP інструментом"
proof_policy: seam-test-first
scope:
  - src/mcp/tools.rs
  - src/mcp/server.rs
  - src/engine/scanner.rs
  - tests/
status: completed
receipt:
  measured_at: "2026-10-01T20:34:57Z"
  ttl_seconds: 5
  cached_inventory_scan_ms: 0.0
  evidence:
    - "tests/mcp_freshness.rs: cached Server read admission reports inventory_scan_ms=0.0 and preserves checked_at."
    - "Freshness and source-only adapter tests verify refresh after the five-second TTL."
    - "MCP responses omit internal timing fields; the wire benchmark does not claim a measured scan duration."
```

Debounce and cache filesystem inventory scans between MCP invocations. Verify file modification times with coarse polling intervals (5–10s) instead of walking thousands of directories on every single JSON-RPC read query.

### task: sqlite-storage-compaction-zstd

```yaml
task_ref: sqlite-storage-compaction-zstd
target: "Забезпечити щільність збереження бази даних <= 45 KiB на файл (< 3.0 KiB на вузол) через Zstandard компресію facts_blob"
proof_policy: seam-test-first
scope:
  - src/core/schema.rs
  - src/core/typed_facts.rs
  - src/db/writer.rs
  - src/db/reader.rs
  - tests/commitment_integrity.rs
status: completed
receipt:
  measured_at: "2026-10-02T01:08:51.856644Z"
  database_bytes: 176717824
  database_mib: 168.53125
  density_kib_per_file: 43.491935
  density_kib_per_node: 2.863714
  facts_compression_ratio: 11.083
  acceptance: "Met: database size is below 180 MiB and both storage density limits are met."
  evidence:
    - "The schema-v9 database contains 3,968 files, 60,263 nodes and 291,696 edges."
    - "Zstandard level-1 streaming compression preserves the complete durable facts payload."
    - "cargo test --test commitment_integrity: 12 passed, 0 failed."
    - "reverse_dependencies is absent from schema; storage uses normalized WITHOUT ROWID junction tables."
```

#### Detailed Execution Plan
1. **Zstandard Compression in `src/core/typed_facts.rs`**:
   - `local_facts.facts_blob` uses streaming level-1 Zstandard compression; the fresh schema-v9 database is **168.53 MiB** with an observed facts compression ratio of **11.083:1**.
   - Preserve complete facts during encode/decode and keep the measured database below **180 MiB**, `<= 45 KiB/file`, and `<= 3 KiB/node`.
2. **Verification Gate**:
   - Validate via `ls -lh /tmp/bench-cru-cold.db` (< 180 MiB).
   - Ensure delta hydration and Merkle root sealing remain identical (`cargo test --test commitment_integrity`).

---

### task: cold-build-latency-and-serialization-optimization

```yaml
task_ref: cold-build-latency-and-serialization-optimization
target: "Забезпечити швидкість холодного білду >= 400 файлів/сек (час білду < 10.0с на 4k файлів) через паралельний Merkle seal, тюнінг індексів та серіалізації"
proof_policy: seam-test-first
scope:
  - src/core/commitments.rs
  - src/core/models.rs
  - src/db/writer.rs
  - tests/commitment_integrity.rs
status: active
receipt:
  measured_at: "2026-10-02T01:08:51Z"
  elapsed_ms: 10493.734431
  rows_ms: 2878.126176
  persist_files_ms: 1169.082527
  persist_graph_ms: 1030.285428
  indexes_ms: 1014.846299
  seal_ms: 639.091697
  acceptance: "Open: final build is 10.494s and index creation is 1.015s; targets are < 10.0s and < 900ms."
  evidence:
    - "The final fresh build contains 3,968 files, 60,263 nodes and 291,696 edges; database size is 176,717,824 bytes."
    - "Eliminated redundant full-source file_search FTS table and duplicate disk re-reads during cold build."
    - "Eliminated heavy HashSet 5-tuple deduplication in persist_graph."
    - "Node navigation serialization excludes value_flow, bindings, rebindings and param_types; compressed local facts retain complete analysis."
    - "Existing staged MEMORY/OFF import, final integrity verification and WAL checkpoint remain intact."
    - "tests/core_basics.rs: navigation_storage_preserves_compressed_analysis_and_delta_calls"
```

#### Detailed Execution Plan
1. **Parallel Domain Merkle Tree Sealing (`seal_ms`: 2,637 ms -> < 1,000 ms)**:
   - In `src/core/commitments.rs`, `seal(&tx)` currently queries and streams SHA256 hashes sequentially across 10 domain tables (`files`, `nodes`, `edges`, `edge_occurrences`, `dependencies`, `owned_search`, `shared_owners`, `errors`, `resolution_coverage`, `doc_sections`).
   - Use Rayon parallel hashing across independent table read streams or in-memory domain buffers to cut `seal_ms` from 2.64s to < 1.0s (saving ~1.6s).
2. **SQLite B-Tree Indexing Optimization (`indexes_ms`: 1,587 ms -> < 900 ms)**:
   - In `src/db/writer.rs`, build indexes with `PRAGMA temp_store = MEMORY; PRAGMA cache_size = -262144;` and restore the bounded connection cache after the phase.
   - Keep the four required binary-collation node indexes and remove redundant NOCASE name indexes.
3. **Streamlined Node Persistence (`persist_files_ms`: 2,036 ms -> < 1,400 ms)**:
   - Avoid intermediate JSON string allocations when streaming `navigation_details()` into SQLite parameter buffers.
4. **Target Metrics**:
   - Total cold build: **< 10.0 seconds** (down from 15.71s).
   - `seal_ms`: **< 1,000 ms** (down from 2,637 ms).
   - `persist_ms`: **< 4,000 ms** (down from 5,771 ms).

---

## Verification and measurement receipt

The milestone remains **active** because cold-build latency and index creation miss their budgets. Search, test discovery, indexed AST candidate selection, cached inventory admission, and storage density meet their measured budgets.

Final verification after the last production and test edit:

- `cargo test --test commitment_integrity`: **12 passed, 0 failed**.
- `cargo clippy --all-targets --all-features -- -D warnings`: **0 warnings**.
- `cargo test --all-targets`: **402 passed, 0 failed, 3 ignored**, across all test targets.
- `cargo build --release`: succeeds.

The [cold-build receipt](../../benchmarks/milestone030_cold_build.json) records 3,968 files, 60,263 nodes and 291,696 edges. The [runner](../../benchmarks/milestone030_benchmark.py) uses real stdio MCP requests against `/home/oleksii/ContextUnity/worktrees/commerce-release-update`; it excludes the first call and uses nearest-rank p95.

| Scenario | Median (ms) | p95 (ms) | Budget | Status |
|---|---:|---:|---:|---|
| Broad symbol search | 19.4284625 | 23.436819 | <30 ms | Met |
| Text search | 17.2120125 | 24.24903 | <30 ms | Met |
| Prefix search | 14.3311565 | 17.235928 | <30 ms | Met |
| Test discovery | 3.4136615 | 3.785287 | <50 ms | Met |
| Workspace AST search | 15.4276135 | 17.794481 | <30 ms | Met |

The final cold build is 10.494 seconds and the database is 168.53 MiB. When the bounded FTS count probe reaches 1,001 matches, the response sets `total` to null while `has_more` uses the `limit + 1` lookahead. The cold-build receipt remains open until the cold-build and index budgets are met.
