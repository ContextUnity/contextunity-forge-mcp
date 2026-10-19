---
title: "Performance Guidance, Search Architecture, and Storage Compaction"
doc_type: architecture
---

# Performance Guidance, Search Architecture, and Storage Compaction

## Performance Objectives and Recommended Values

These values are recommendations for profiling and product planning, not repository-wide acceptance gates. Milestone 030 is the sole current contract that admits performance budgets. On the Commerce reference repository, a cold build around 10 seconds is a preferred aspiration.

| Operation / Tool | Recommended median / p95 | Architectural Mechanism |
| :--- | :--- | :--- |
| **Tool Response Baseline** | about 10 / 30 ms | Inventory admission caching and connection reuse |
| **`code_map_search`** | about 30 / 30 ms | SQLite FTS5 BM25 index + exact prefix matching |
| **`ast_grep_search`** | about 30 / 30 ms | Indexed selection for declaration-name queries; bounded source scans for general structural patterns |
| **`code_map_tests`** | about 50 / 50 ms | Bounded graph traversal + lexical fallback |
| **`code_map_prove_removal`** | about 30 / 30 ms | Target-scoped dependency evaluation |

## Cold Build Streaming and Memory Budget

`db::writer::build` overlaps file discovery from `scanner::stream_for_build`, source acquisition and extraction, and SQLite `source_inventory` ingestion. A bounded producer queue applies backpressure while a shared byte budget accounts for queued and active extraction items. Each item's estimate is its scanned source size multiplied by the extraction expansion factor, with saturating arithmetic. A single item larger than the configured budget proceeds only when no other item holds a permit.

The SQLite consumer releases an item's permit after its source inventory row has been inserted. Extracted `TypedFacts` remain available for the cross-file linker and component pass. After global route and component composition, the build encodes each file's canonical facts once in parallel. The final persistence transaction clears provisional source inventory rows, then `persist_files` writes canonical rows in sorted path order, preserving deterministic row order and uniqueness. Scan, worker, or SQLite errors cancel the pipeline, wake blocked stages, join workers, discard the staged database, and leave the previously published database intact.

The build report's `pipeline` object exposes `budget_bytes`, `peak_in_flight_bytes`, `max_item_bytes`, `oversized_admissions`, `extracted_files`, and `ingested_files`. Byte metrics describe the extraction estimate held by permits; they do not represent process RSS. Top-level `scan_ms` measures producer wall time, including time spent waiting for backpressure. `extract_ms` sums worker service time, `sqlite_ingest_ms` measures consumer service time, and `encode_facts_ms` measures the one-time parallel canonical encoding after the composition barrier. Scan, extraction, and SQLite ingestion overlap; encoding follows the semantic barrier.

## Key Architectural Systems

### 1. MCP Admission Inventory Scan Debouncing
By default, Forge verifies workspace freshness before executing tool requests. Full filesystem walks (`source_inventory`) can add measurable overhead on large workspaces.
- **Debounced Scan Window**: `Server` caches a successful inventory admission for five seconds. Its read boundary reports `inventory_scan_ms: 0.0` on cached admission and retains the timestamp of the last scan. Adapter loading and database identity checks still occur before the cache can be used. MCP responses omit internal timing fields.
- **Invalidation**: Database identity, workspace root, and adapter changes invalidate the cached admission. After the TTL expires, `scan_reusing` checks the full admitted inventory and detects additions, deletions, and edits.

### 2. Two-Tier Hybrid Search (FTS5 BM25)
`code_map_search` combines structural exact matching with full-text lexical ranking:
- **Tier 1 (Exact & Prefix)**: Exact name or qualified-name matches receive the strongest score; name prefixes receive a smaller boost. Exact-only requests retain case-insensitive matching and the direct binary-equality fallback for patterns that do not produce FTS tokens.
- **Tier 2 (Full-Text BM25)**: `node_search` indexes names, qualified names, paths, docstrings, and distinct module reference expressions. The FTS branch scores/orders candidates before its dynamic cap (`max(500, offset + limit + 1)`); structural name/qualified-name candidates are scored and ordered in a separate CTE. A 1,001-row probe controls whether the exact total is returned, while graph connectivity boosts only the top-50 candidate window. Search reports exact matching totals through that count horizon and an unavailable total beyond it, including on empty continuation pages.
- **Documentation and grouping**: Code search excludes Markdown by default. `include_docs` admits documentation; `group_by_file` places each file path once around its returned symbols.

### AST and test discovery
`tests_paged` bounds graph traversal at depth four and uses lexical test-name matching when the graph has no inbound tests. `ast_grep_search` parses declaration names through the extraction language profile and selects matching symbols through the name index and `files`. Other structural patterns scan the bounded set of indexed files for the requested language and path; `node_search` omits arbitrary function-body literals, so it cannot safely exclude files from general AST searches. The matcher verifies each source digest before matching with tree-sitter and reports its computation limit explicitly.

### 3. Localized Removal Proof
`code_map_prove_removal` evaluates safety strictly within the target's dependency subgraph:
- **Target-Scoped Evaluation**: Checks unresolved calls and references only where the referenced expression matches the target or exists within modules directly depending on the target.
- **Independent Safety Flag**: Returns `safe_to_remove: bool` from the selected nodes' incoming dependencies and target-scoped unresolved references and parse errors. It does not include unrelated workspace-wide diagnostics.

### Linker lookup storage

Per-file import maps borrow scope and alias names instead of cloning them for each import. Qualified symbol lookup reuses local scratch strings across lexical scope walks and imported member candidates; delta top-level checks compare a stripped module prefix without constructing a qualified name.

Delta streams each admitted non-component resolution identity tuple into a truncated SHA-256 digest and compares sorted fixed-size digest sets. This avoids materializing JSON strings for every node during the comparison. Import-frontier facts are cached by owner path and reused during the delta; direct importers are refreshed, while later hops expand only when indexed exports or return contracts prove exposure, with unavailable or ambiguous facts handled conservatively.

Value-flow scopes borrow binding names from typed facts or the original node details when flows are decoded locally. A single write stays inline in `BindingList::One`; a second write promotes the history to `Many` while preserving source order and reaching-write behavior. Compact coverage borrows static diagnostic literals with `Cow<'static, str>`. Conversion to the public graph retains its owned evidence strings and serialization format.

The linker regression seam counts successful allocations of selected name and scope sizes during linking in a private one-thread Rayon pool. Parsing, pool setup, warmup, serialization, and assertions are outside the measured operation. This verifies the specific lookup storage changes without setting a whole-linker allocation or latency budget.

### 4. Database Storage Compaction and Density Guidance
Around 45 KiB per source file or 3 KiB per node are useful reference values when comparing candidates. These are not general acceptance gates; milestone 030 owns its storage gate.
- **Zstandard Compression**: `local_facts` stores complete AST and analysis facts as buffered streaming level-1 Zstd frames. A ratio around 3.5:1 and compressed fact storage around 15 KiB per source file are recommended references. Navigation rows omit `value_flow`, `bindings`, `rebindings`, `param_types`, and empty optional fields; delta resolution contracts hydrate complete facts from `local_facts`.
- **Zero-Redundancy Persistence**: Rely on bidirectional indexed queries on `edges` (`src_hash`, `dst_hash`) without duplicating reverse dependency tables. Ingestion hot paths stream batches without ad-hoc per-edge heap allocations.
- **Index Compaction**: Use SQLite `WITHOUT ROWID` tables for junction tables (`edge_occurrences`, `dependencies`, `shared_owners`).
- **Dictionary Storage**: `nodes` stores path and owner IDs from `path_dictionary`. File and directory filters use the dictionary's indexed text paths and join node rows through `path_id`; lexical path ranges apply to dictionary text. Graph evidence and confidence share `coverage_evidence`. `shared_keys` references node qualified names by node ID; node mutation materializes the original key before its source changes. Readers query canonical tables directly, and commitment hashers resolve dictionary IDs through preloaded maps.
- **Commitment Storage**: `domain_commitments` stores a domain, owner path ID, and 32-byte digest. Cold builds commit imported rows, checkpoint, and create ordinary indexes before sealing. Sufficiently large candidates with multiple Rayon threads hash independent domains through read-only connections; one transaction installs commitments and the root. Delta sealing retains its transaction boundary.
- **Phase Timing**: `indexes_ms` and `seal_ms` measure their individual wall times. `index_and_seal_ms` measures their combined wall time; `persist_ms` includes that interval.

### 5. Serialization and Throughput Guidance (ADR 0012)
See [ADR 0012: Lean Serialization and Measured Performance](../adr/0012-zero-regression-serialization-and-cold-build-latency.md):
- **Lean Node Details**: Restrict `nodes.details` to directly indexed navigation fields (`signature`, `doc`, `receiver_name`, explicit decorators). Around 120 bytes per node is a recommended payload reference. Report `rows_ms` according to its actual phase scope; it includes broader persistence work rather than only node insertion.
- **Bulk SQLite Ingestion Pragmas**: Cold indexing uses `PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;` on an isolated candidate database. It seals and commits the candidate, restores WAL mode, verifies SQLite integrity, and checkpoints WAL before atomic publication.
- **Bounded cold fact writes**: Cold builds serialize each complete `local_facts` payload in parallel chunks instead of retaining a workspace-wide second map of compressed payloads. The writer holds at most 500 compressed facts per chunk and inserts each row batch in the existing write transaction, bounded by SQLite's live parameter limit; secondary indexes are created after the bulk load. `sqlite_ingestion` reports the active mode, deferred-index behavior, and effective row-batch size. Graph endpoint caching shares one path allocation across nodes in the same file, avoiding per-node path string copies.
- **Cold-build phase timing**: `encode_facts_ms` totals the per-file serialization and compression time performed while `local_facts` rows are batched. `batch_rows` reports the effective local-facts row cap, not the size of every other table's row batch.
- **Throughput Recommendation**: Around 400 files/sec end-to-end and 800 files/sec for extraction are useful initial comparison values. The 10-second Commerce cold-build aspiration helps prioritize phase-level work; it is a gate only where a milestone explicitly admits it.

## Cold Indexing Throughput and Memory Optimizations (Milestone 031)

Milestone 031 (`m-cold-indexing-throughput-and-memory-optimization`) addressed cold-build latency, peak memory spikes, and CPU load on large codebases (demonstrated on the Commerce reference repository of 3,990 files and 130,079 resolved edges). The following architectural mechanisms were delivered:

### 1. Staged Producer-Consumer Pipeline and Byte-Budget Backpressure
- **Pipeline Architecture**: `crate::engine::scanner::stream_for_build` (`src/engine/scanner/mod.rs`) and `crate::engine::pipeline::run_cold_pipeline` (`src/engine/pipeline.rs`) overlap filesystem discovery, Rayon worker extraction, and SQLite `source_inventory` ingestion.
- **Byte Accounting**: Ingestion queue bounds operate on dynamic estimated byte budgets derived from `ScannerLimits.memory_budget_mb` (`src/core/config.rs`) or the `FORGE_MEMORY_BUDGET_MB` environment variable (`MemoryPermit` in `src/db/writer.rs`). Each file's permit accounts for its scanned file size multiplied by the extraction expansion factor. A single oversized item is admitted only when no other item holds an active permit, preventing memory exhaustion on massive generated or bundled files while guaranteeing progress without deadlocks.

### 2. Lean SymbolCatalog Decomposition and Streaming Storage Payloads
- **Decoupled Linking State**: Historically, `TypedFacts` retained full AST details (`value_flow`, `bindings`, `rebindings`, `param_types`, and span details) in a workspace-wide map during global cross-file linking. Milestone 031 replaced this with `SymbolCatalog` and `CatalogSymbol` (`src/core/semantic.rs`, `src/engine/linker/compact.rs`, `src/engine/linker.rs`).
- **One-Way Stripping**: Workers invoke `SymbolCatalog::insert_typed(..., false)`, preserving only essential navigation symbols and reference targets for `link_compact_graph`.
- **Precomputed Staged Storage**: Heavyweight storage rows are precomputed during extraction and Zstd-compressed into `PreparedCatalogStorage` within `StagedFilePayload` (`src/db/writer.rs`).
- **Bounded Chunk Hydration**: In `populate_owned` (`src/db/writer.rs`, `src/db/ingest.rs`), `LoadedCatalogFile` structures are loaded and persisted in bounded parallel chunks of up to 500 files (`MAX_MULTI_VALUE_BATCH_ROWS = 500`). Each payload chunk is dropped immediately after SQLite insertion, preventing simultaneous coexistence of raw ASTs and database rows.

### 3. Allocator Arena Trimming
- **Glibc Arena Reclamation**: Multi-threaded Rayon workloads cause glibc thread arenas to retain fragmented memory long after transient ASTs and serialization buffers are freed.
- **Explicit Trimming**: `trim_allocator` (`libc::malloc_trim(0)`) is invoked in `src/engine/pipeline.rs` and `src/db/writer.rs` at the boundary between extraction/staging and persistence, and again after bulk row insertions. This directly drops Commerce peak RSS from `~1,503.5 MB` down to **`766–785 MB`**, consistently respecting the `< 800 MB` milestone ceiling (-48% peak memory reduction).

### 4. SQLite Turbo-Ingestion Mode
- **Isolated Staged PRAGMAs**: `prepare_pipeline_schema` (`src/db/writer.rs`) configures candidate SQLite databases with:
  ```sql
  PRAGMA journal_mode = MEMORY;
  PRAGMA synchronous = OFF;
  PRAGMA temp_store = MEMORY;
  PRAGMA cache_size = -<cache_kb>;
  PRAGMA cache_spill = ON;
  ```
  where `cache_kb` is bounded by memory limits (`sqlite_cache_size_kib`).
- **Deferred Secondary Indexes (`deferred-index-batched`)**: Secondary table indexes (`idx_nodes_path_id`, `idx_nodes_kind`, `idx_nodes_canonical`, `idx_edges_src_hash`, `idx_edges_dst_hash`, etc.) are created only *after* bulk rows have been inserted via multi-value batches.
- **Safe WAL Restoration**: Before atomic file publication, `populate_owned` restores `PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;` and forces a checkpoint via `PRAGMA wal_checkpoint(TRUNCATE);`, ensuring published databases are immediately durable and concurrent-reader safe.

### 5. Hybrid In-Memory Merkle Commitment Sealing
- **In-Memory Leaf Hashing**: Prior implementations re-scanned entire SQLite tables to compute Merkle leaf commitments across 17 domains. Milestone 031 implemented hybrid in-memory hashing in `seal::file_domain_leaves` and `seal::logical_domain_leaves` (`src/db/writer/seal.rs`, `src/core/commitments.rs`, `src/core/commitments/snapshot.rs`).
- **13 Logical Domains**: Commitments for `files`, `source_inventory`, `nodes`, `edges`, `edge_occurrences`, `dependencies`, `resolution_coverage`, `coverage_language_counts`, `coverage_owner_language`, `doc_sections`, `shared_owners`, `errors`, and `local_facts` are computed directly from in-memory projections and streaming buffers. This cut `seal_ms` from `~1,012 ms` down to `~75–112 ms`.
- **4 FTS5 Search Domains via C-API Tokenizer**: `node_search_postings`, `node_search_config`, `doc_search_postings`, and `doc_search_config` are tokenized in a single stream using SQLite's native C-API FTS5 tokenizer and verified against `docsize` tables, maintaining 100% bit-identical `forge-owner-sha256-v2` roots across all 17 domains without table re-scans.

### 6. Cross-Worktree Content-Addressed Binary Extraction Cache
- **Extraction Cache v3**: `src/db/writer/cache.rs` provides cross-worktree AST and typed-facts reuse for unmodified files.
- **Cache Key Identity**: Keys are strictly derived from source SHA-256 digest, relative path, Tree-sitter grammar ID, `INDEX_SEMANTICS_VERSION`, and `effective_ruleset_hash` (including bundled framework manifests via `build.rs` and `src/engine/languages/dependency_registry.rs`).
- **Binary DTO Format**: Custom binary serialization eliminates JSON encoding and Zstd decompression overhead during cache hits.
- **Empirical Impact**: On warm re-indexing of 3,990 files, user CPU drops by **63%** (`44.49s` $\to$ `16.34s`, CPU load `374%` $\to$ `215%`) and wall-clock time drops to **`8.57s`** at `746 MB` Peak RSS.

---

## Empirically Evaluated and Rejected Optimizations

During Milestone 031, multiple plausible optimization hypotheses were empirically evaluated on the control repository (`commerce-release-update`, 3,990 files, 3.3M AST nodes) using hardware `perf record`, paired repeat benchmarks, and GNU `time -v`. The following approaches were definitively rejected based on measured regressions or invariant violations:

| Candidate Optimization | Evaluated Scope / Symbol | Measured Result | Rejection Rationale |
| :--- | :--- | :--- | :--- |
| **Uncompressed In-Memory `PreparedCatalogStorage` (`direct-rows`)** | `StagedFilePayload` in `src/db/writer.rs` | Wall: `9.907s`, Peak RSS: **`869.5 MB`** | Violated strict `< 800 MB` memory ceiling (+85 MB overshoot). Zstd frame compression on staged rows is mandatory. |
| **Length-Prefixed Binary Packing (`packed-rows`)** | `prepare_catalog_storage` & `LoadedCatalogFile` | Paired user CPU: **`+1.38%`**, CPU load: **`+2.02%`** | Manual `u32` packing and scalar node cloning from `SymbolCatalog` increased allocation and slicing overhead compared to direct JSON serialization. |
| **Temporary SQLite Staging Table (`catalog_payloads`)** | Intermediate table in staged SQLite candidate | Wall time regressed by **`+6.2%`** | SQLite B-tree write and deserialize overhead exceeded in-memory compressed chunk queuing. |
| **Concurrent Merkle Leaf Hashing with SQLite Index Creation** | `seal::logical_domain_leaves` overlapped with `CREATE INDEX` | Wall time regressed by **`+3.8%`** | Parallel Rayon CPU hashing and SQLite B-tree sort contended for memory bandwidth and allocator mutexes. |
| **Parallel Posting Sorting (`par_sort_unstable_by_key`) & Removal of `term_map` in FTS Sealing** | `src/core/commitments.rs` | Deterministic root mismatch (`INV-DETERMINISTIC-MERKLE` failed) | FTS5 C-API emits tokens in document order. Monotonic dictionary sorting `(term, docid, col, pos)` is required for `forge-owner-sha256-v2` bit identity. |
| **Tree-Sitter `QueryCursor` (S-Expression byte-code engine)** | `benchmarks/tree_cursor_probe.rs`, `tree_sitter::QueryCursor` | **`2.90x slower`** (`5.18s` vs `1.78s` for 3.3M nodes) | General AST queries (`"(_) @node"`) incur heavy query-state machine allocations and capture tracking compared to flat cursor iteration. |
| **Leaf-Count Pre-check Guard (`named_child_count() == 0`)** | `src/engine/ast/visitor.rs` (`walk`) | **`5.89% slower`** (`1.89s` vs `1.78s`) | Calling `ts_node_named_child_count` before `goto_first_child` adds a redundant C FFI transition per node. |
| **Lazy `symbol_name` Parent Node Lookup** | `src/engine/ast/mod.rs` | Wall: **`+1.47%`**, User CPU: **`+2.33%`** | Deferring parent lookup via `or_else` did not eliminate AST traversal and disrupted branch prediction in extraction hot loops. |
| **Filtering Python Initializers (`__init__.py`) or Config Files** | `src/engine/scanner/mod.rs` | Broken re-exports and Merkle root divergence | Commerce contains 202 `__init__.py` files containing package exports and module docstrings; skipping them breaks 130,079 graph edges. |

---

## Model and Agent Invariants (Immutable Guidelines)

Autonomous agents, builders, and language models modifying Forge indexing or storage components must strictly adhere to the following invariants. These rules are tied directly to concrete code symbols and protected by compiler checks and regression seams:

> [!IMPORTANT]
> Invariant: Symbol Catalog Fact Stripping
>
> In `src/engine/pipeline.rs` (`run_cold_pipeline`) and `src/core/semantic.rs` (`SymbolCatalog::insert_typed`), agents must never retain whole-workspace `TypedFacts` during the linking phase. Extraction workers must strip AST details (`value_flow`, `bindings`, `rebindings`, `param_types`) by passing `retain_extended = false` to `insert_typed`. Full facts must only be encoded into `PreparedCatalogStorage` within worker tasks and dropped immediately.

> [!IMPORTANT]
> Invariant: Bounded Chunk Persistence and Allocator Trimming
>
> In `src/db/writer.rs` (`populate_owned`), `StagedFilePayload.storage_rows` must remain compressed with level-1 Zstandard during staging. Ingestion must process files in bounded batches of at most 500 rows (`MAX_MULTI_VALUE_BATCH_ROWS = 500`), removing payloads from memory prior to database insertion. Agents must preserve the explicit calls to `trim_allocator` (`libc::malloc_trim(0)`) after extraction and after persistence to prevent glibc multi-arena fragmentation from exceeding the 800 MB RSS ceiling.

> [!IMPORTANT]
> Invariant: Tree-Sitter AST Traversal Loop Integrity
>
> In `src/engine/ast/visitor.rs` (`walk`), tree traversal must remain a flat iterative loop over `tree_sitter::TreeCursor` (`goto_first_child`, `goto_next_sibling`, `goto_parent`). Agents must not replace `walk()` with `tree_sitter::QueryCursor` or introduce per-node FFI guards like `named_child_count() == 0`. The traversal must yield balanced `(node, entering)` events required by `SinglePassExtractor` and `ScopeAnalysis`.

> [!IMPORTANT]
> Invariant: SQLite Ingestion PRAGMAs and WAL Restoration Order
>
> In `src/db/writer.rs` (`prepare_pipeline_schema` and `populate_owned`), cold-indexing candidate databases must initialize with `journal_mode = MEMORY; synchronous = OFF; temp_store = MEMORY; cache_spill = ON;` and defer secondary index creation until after row ingestion (`sqlite_ingestion.mode = deferred-index-batched`). Before atomic publication, the writer must restore `journal_mode = WAL; synchronous = NORMAL;` and execute `PRAGMA wal_checkpoint(TRUNCATE);`. Changing pragma ordering or omitting WAL restoration causes reader lockouts and database corruption.

> [!IMPORTANT]
> Invariant: Deterministic 17-Domain Merkle Commitments
>
> In `src/db/writer/seal.rs` (`seal::file_domain_leaves`, `seal::logical_domain_leaves`) and `src/core/commitments.rs`, in-memory leaf hashing for the 13 logical domains and C-API tokenizer hashing for the 4 FTS domains must yield 100% bit-identical digests matching SQLite table scans (`forge-owner-sha256-v2`). Tokens must be ordered canonically by `(term, docid, col, pos)` using `term_map`. Any modification must pass `tests/commitment_integrity.rs`.

> [!IMPORTANT]
> Invariant: Strict Extraction Cache Key Binding
>
> In `src/db/writer/cache.rs` (`ExtractionCache`) and `src/engine/languages/dependency_registry.rs`, cache keys must strictly bind source SHA-256 digest, normalized relative path, Tree-sitter grammar ID, `INDEX_SEMANTICS_VERSION`, and `effective_ruleset_hash` (which incorporates bundled framework manifests via `build.rs`). Agents must never omit the ruleset hash or grammar identity from the cache key, as doing so leads to stale, invalid extraction across manifest or grammar upgrades.
