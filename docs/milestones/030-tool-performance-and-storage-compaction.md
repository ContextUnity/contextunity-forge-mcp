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

Deliver sub-30ms MCP tool query response times, localize `code_map_prove_removal` to target candidate dependencies instead of globally blocking on unrelated workspace errors, enable two-tier hybrid search with native SQLite FTS5 BM25, accelerate test discovery and AST grep, maintain database storage density <= 45 KiB per file via Zstandard fact compression, and sustain cold build throughput >= 400 files/sec (<= 2.5s per 1,000 files). After the cold-build budget is met, index deduplicated identifier and string-literal tokens from function and method bodies on those nodes' existing `node_search` documents.

## Tasks in this milestone

### task: prove-removal-target-scoping

```yaml
task_ref: prove-removal-target-scoping
target: "Localize code_map_prove_removal checks strictly to target symbol dependencies"
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
target: "Implement hybrid ranking with native SQLite FTS5 BM25 in code_map_search"
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
target: "Accelerate code_map_tests (bounded max_depth=4) and ast_grep_search (FTS file pre-filtering)"
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
target: "Eliminate 30-50ms filesystem scan overhead prior to each MCP tool invocation"
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
target: "Ensure database storage density <= 45 KiB per file (< 3.0 KiB per node) via Zstandard compression of facts_blob"
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
target: "Ensure cold build throughput >= 400 files/sec (build time < 10.0s per 4k files) via parallel Merkle seal, index tuning, and optimized serialization"
proof_policy: seam-test-first
scope:
  - src/core/commitments.rs
  - src/core/models.rs
  - src/db/delta.rs
  - src/db/ingest.rs
  - src/db/symbols.rs
  - src/db/writer.rs
  - src/engine/ast/mod.rs
  - src/engine/ast/routes.rs
  - src/engine/languages/html.rs
  - src/engine/languages/mod.rs
  - src/engine/languages/python.rs
  - src/engine/languages/python/lazy_exports.rs
  - src/engine/languages/python/render_context.rs
  - src/engine/languages/python/value_flow.rs
  - src/engine/languages/typescript.rs
  - src/engine/languages/vue/template.rs
  - src/engine/linker.rs
  - src/engine/scanner.rs
  - tests/ast_extractors.rs
  - tests/core_basics/tasks.rs
  - tests/commitment_integrity.rs
  - tests/python_semantics.rs
  - tests/query_context/search_and_paging.rs
  - tests/scanner_guard_limits.rs
  - tests/typescript_semantics.rs
status: active
receipt:
  measured_at: "2026-10-06"
  command: "./target/release/contextunity-forge-mcp build /home/oleksii/ContextUnity/worktrees/commerce-release-update --output /tmp/commerce-bench-final.sqlite --verbose"
  files: 3901
  nodes: 65551
  edges: 302113
  elapsed_ms: 13870.83
  link_ms: 2542.43
  extract_ms: 4396.47
  persist_ms: 5841.86
  rows_ms: 3677.92
  bulk_paths_ms: 70.28
  persist_files_ms: 1452.51
  fts_insert_ms: 193.55
  doc_fts_insert_ms: 22.72
  persist_graph_ms: 1894.10
  indexes_ms: 1472.77
  index_and_seal_ms: 2096.53
  seal_ms: 623.71
  verify_ms: 468.86
  database_bytes: 187498496
  density_kib_per_file: 46.94
  density_kib_per_node: 2.79
  resolution_coverage: "251155 / 294460 (85.29%); unresolved=43247; ambiguous=58"
  output_generation: "7ea6992fd332250f57b689304dd29fdf0da16ff3c636d5076a8d435e516c0828"
  benchmark_source_state: "Before the async Future-boundary correction in ValueFlowFacts and ValueExpr; final source was not re-indexed under the per-turn profiling cap."
  host_context: "16 CPUs; load average 6.27 at build start; no cargo/rustc jobs remained after release compilation"
  acceptance: "Open: the pre-correction candidate's 13.871s and 281.2 files/sec miss < 10.0s and >= 400 files/sec; indexes_ms=1472.77 exceeds 900 ms and storage is 46.94 KiB/file, above 45 KiB. Its coverage was 85.29%; final-source coverage remains unmeasured. Final-source commitment_integrity passes 12/12."
  evidence:
    - "The fresh Commerce build contains 3,901 files, 65,551 nodes and 302,113 edges; database size is 187,498,496 bytes."
    - "This pre-correction build classifies 251,155/294,460 references (85.29%) and records the Merkle root above; it is not a coverage receipt for final source. Its exact grant.get rows at lines 93,94,96,99,100 are external, while Category.add_root at line 259 remains unresolved pending normalized treebeard inheritance evidence."
    - "After this build, the first cargo test --all-targets run exposed two regressions in the async Future boundary; the fix keeps ordinary async() calls unknown and exposes the annotation only for await. Final-source tests pass: Python semantics 25/25, typed receiver resolution 75/75, full suite 594/0/3, commitment_integrity 12/12, strict clippy 0 warnings, release build succeeds."
    - "No cargo/rustc jobs overlapped the controlled build; the host had a 6.27 load average at its start. The single result is retained with that contention context rather than treated as a quiet-host baseline."
    - "Eliminated redundant full-source file_search FTS table and duplicate disk re-reads during cold build."
    - "Eliminated heavy HashSet 5-tuple deduplication in persist_graph."
    - "Node navigation serialization excludes value_flow, bindings, rebindings and param_types; compressed local facts retain complete analysis."
    - "Existing staged MEMORY/OFF import, final integrity verification and WAL checkpoint remain intact."
    - "tests/core_basics.rs: navigation_storage_preserves_compressed_analysis_and_delta_calls"
subtasks:
  - subtask_ref: ast-visitor-context-reuse
    title: "Reuse one SyntaxContext and the borrowed shadowed-require set across each Extraction::visit node; preserve extracted imports, calls, mutations, routes, and Merkle determinism while Commerce extract_ms stays <= 4,900 ms"
    status: pending
  - subtask_ref: language-scope-fact-fast-paths
    title: "Collect TypeScript DOM callback scope facts only when addEventListener, .on<event>, or require can use them; collect Python TYPE_CHECKING aliases and django.shortcuts presence during the module pass; preserve the exact status and edge-key sets with Commerce resolution coverage >= 85.2%"
    status: pending
  - subtask_ref: route-client-callee-fast-guard
    title: "For AST call expressions, parse route arguments only for fetch, axios, http, client, $, jQuery, requests, or a qualified call whose immediate receiver matches those names; preserve explicit Django route and client-call results"
    status: pending
  - subtask_ref: thread-local-parser-pool
    title: "Reuse one configured Tree-sitter parser per built-in LanguageProfile slot without removing and reinserting HashMap entries per file; unknown profile keys continue through the extensible fallback and every grammar keeps its own parser"
    status: pending
  - subtask_ref: coverage-stream-sort-and-path-cache
    title: "Persist sorted resolution_coverage rows with parallel sorting for collections >= 8,192, reuse path_id while adjacent rows share c.path, and accumulate owner-language counts without a per-row tree insertion; preserve exact row counts and cold/delta parity while persist_ms stays <= 4,000 ms"
    status: pending
  - subtask_ref: owner-language-expression-prefilter
    title: "Read the distinct expression_id set from coverage_owner_language once and filter expression commitments in memory instead of rescanning the 294k-row sidecar; cargo test --test commitment_integrity remains 12/12 and repeated cold builds produce identical Merkle roots"
    status: pending
  - subtask_ref: empty-exact-search-fast-path
    title: "When exact name search produces no FTS tokens, query the existing nodes name and qualname indexes directly while retaining kind, path, documentation, and paging filters; exact search remains <= 10 ms"
    status: pending
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

### task: function-body-search-tokens

```yaml
task_ref: function-body-search-tokens
target: "Index deduplicated identifier and string-literal tokens from each function and method body onto that node's existing node_search document"
proof_policy: seam-test-first
scope:
  - src/db/ingest.rs
  - src/cli/ast.rs
  - tests/
  - benchmarks/
status: blocked
depends_on:
  - cold-build-latency-and-serialization-optimization
invariants:
  - "INV-NO-SOURCE-MIRROR: node_search stores deduplicated tokens, not raw function source, comments, or a second full-text table."
  - "INV-SEARCH-BUDGET: On commerce-release-update, code_map_search text p95 stays under 30 ms and storage stays <= 45 KiB per file and <= 3 KiB per node."
  - "INV-AST-PREFILTER: ast_grep_search skips a file for absent literal tokens only when node_search indexes every syntax location that the active language profile permits the pattern to match. When the index omits a permitted location, use a bounded candidate fallback or return an explicit budget outcome; never report a false zero-match result."
subtasks:
  - subtask_ref: body-identifier-hit
    title: "code_map_search({pattern: 'coverage_owner_language', path: '<file>', exact: false}) returns the function or method whose body contains that identifier, with match_reason indexed_text on that node"
    status: pending
  - subtask_ref: string-literal-hit
    title: "code_map_search({pattern: 'coverage_owner_language', path: '<file>', exact: false}) returns the function or method whose body contains the string literal coverage_owner_language, with match_reason indexed_text on that node"
    status: pending
  - subtask_ref: prefilter-sees-body-tokens
    title: "ast_grep_search keeps the node_search file prefilter: a file with none of the pattern literal tokens is not read, and a file whose only token hit is on a function or method body is read"
    status: pending
  - subtask_ref: density-latency-cold-build
    title: "The commerce-release-update cold build stays <= 45 KiB/file, <= 3 KiB/node, code_map_search text p95 < 30 ms, and cold build within the milestone budgets; commitment_integrity stays deterministic"
    status: pending
```

`exact: true` stays a name and qualified-name lookup. Fragment parsing for `ast_grep_search` stays outside this task. The existing underscore splitter and OR query stay; path-scoped search is the acceptance, because a workspace-wide OR of `coverage`, `owner`, and `language` is not a unique hit.

---

## Verification and measurement receipt

The milestone remains **active** because the latest cold-build latency, index creation, and storage density miss their budgets. `function-body-search-tokens` stays blocked until that task is completed. Search, test discovery, indexed AST candidate selection, and cached inventory admission meet their measured budgets. The 2026-10-02 storage receipt below met its density target for that candidate; the current 2026-10-06 integrated candidate is 46.94 KiB/file and exceeds the limit.

Verification for the current candidate after the latest production edit:

- `cargo test --test commitment_integrity`: **12 passed, 0 failed**.
- `cargo clippy --all-targets --all-features -- -D warnings`: **0 warnings**.
- `cargo test --all-targets`: **594 passed, 0 failed, 3 ignored** after the async Future-boundary correction.
- `cargo build --release`: succeeds.

The [cold-build receipt](../../benchmarks/milestone030_cold_build.json) records 3,968 files, 60,263 nodes and 291,696 edges. The [runner](../../benchmarks/milestone030_benchmark.py) uses real stdio MCP requests against `/home/oleksii/ContextUnity/worktrees/commerce-release-update`; it excludes the first call and uses nearest-rank p95.

| Scenario | Median (ms) | p95 (ms) | Budget | Status |
|---|---:|---:|---:|---|
| Broad symbol search | 19.4284625 | 23.436819 | <30 ms | Met |
| Text search | 17.2120125 | 24.24903 | <30 ms | Met |
| Prefix search | 14.3311565 | 17.235928 | <30 ms | Met |
| Test discovery | 3.4136615 | 3.785287 | <50 ms | Met |
| Workspace AST search | 15.4276135 | 17.794481 | <30 ms | Met |

The earlier cold-build receipt measured 10.494 seconds and 168.53 MiB for its 3,968-file candidate. The current integrated candidate is the 2026-10-06 receipt above: 13.871 seconds, 187,498,496 bytes, and 46.94 KiB/file for 3,901 files. The earlier receipt is historical and does not satisfy the current cold-build, indexing, or density gates. When the bounded FTS count probe reaches 1,001 matches, the response sets `total` to null while `has_more` uses the `limit + 1` lookahead. The cold-build receipt remains open until the cold-build and index budgets are met.
