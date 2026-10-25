---
id: arch-performance-guidance
title: "Performance Guidance, Search Architecture, and Storage Compaction"
doc_type: architecture
status: active
owners:
  - src/core/semantic.rs
  - src/core/typed_facts.rs
  - src/db/writer.rs
  - src/db/writer/cache.rs
  - src/db/writer/seal.rs
  - src/engine/ast/mod.rs
  - src/engine/ast/routes.rs
  - src/engine/ast/visitor.rs
  - src/engine/languages/dependency_registry.rs
  - src/engine/languages/mod.rs
  - src/engine/languages/python/projections.rs
  - src/engine/languages/python/syntax_operations.rs
  - src/engine/languages/typescript/projections.rs
  - src/engine/linker.rs
  - src/engine/linker/resolution/mod.rs
  - src/engine/linker/resolution/value_flow_phase.rs
  - src/engine/linker/semantic_context.rs
  - src/engine/pipeline.rs
invariants:
  - INV-PERF-CATALOG-FACT-STRIPPING
  - INV-PERF-BOUNDED-STAGING-AND-TRIM
  - INV-PERF-FLAT-TREE-CURSOR-WALK
  - INV-PERF-SINGLE-PASS-AST-EXTRACTION
  - INV-PERF-TYPED-SIDECAR-FACTS
  - INV-PERF-SHARED-RULE-CONTEXT-AND-DISPATCH
  - INV-PERF-ZERO-ALLOC-PROJECTION-PROOFS
  - INV-PERF-LANGUAGE-GATED-LINKER-RESOLUTION
  - INV-PERF-RELEASE-SINGLE-CGU
  - INV-PERF-SQLITE-INGESTION-PRAGMAS
  - INV-PERF-DETERMINISTIC-MERKLE-SEALING
  - INV-PERF-CACHE-KEY-BINDING
  - INV-PERF-STAGE-PAYLOAD-SPOOLING
  - INV-PERF-ZERO-CLONE-BATCHED-NODES
  - INV-PERF-DEFERRED-TRANSIENT-NODE-PATHS
  - INV-PERF-OCCURRENCE-ENDPOINT-HASH-REUSE
  - INV-PERF-IN-MEMORY-FILE-LANGUAGE-FALLBACK
  - INV-PERF-COMPACT-EDGE-COVERAGE-MODELS
  - INV-PERF-COMPACT-GRAPH-PATH-POOLING
  - INV-PERF-NORMALIZED-NODE-BINDING-VECTORS
  - INV-PERF-BORROWED-MAP-KEY-EXPORT-INDEXING
  - INV-PERF-ZERO-CLONE-PARALLEL-MODULE-NAME
  - INV-PERF-BORROWED-PATH-DICTIONARY-BULK-INSERT
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
- **Lean Node Details**: Restrict `nodes.details` to directly indexed navigation fields (`signature`, `doc`, `receiver_name`, explicit decorators). Report `rows_ms` according to its actual phase scope; it includes broader persistence work rather than only node insertion.
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
- **Catalog Admission**: Workers invoke `SymbolCatalog::insert_typed(..., false)` to omit storage-only detail fields and duplicate storage payloads. The linking catalog retains native binding metadata and typed flows needed for resolution.
- **Precomputed Staged Storage**: Heavyweight storage rows are precomputed during extraction and Zstd-compressed into `PreparedCatalogStorage` within `StagedFilePayload` (`src/db/writer.rs`).
- **Bounded Chunk Hydration**: In `populate_owned` (`src/db/writer.rs`, `src/db/ingest.rs`), `LoadedCatalogFile` structures are loaded and persisted in bounded parallel chunks of up to 500 files (`MAX_MULTI_VALUE_BATCH_ROWS = 500`). Each payload chunk is dropped immediately after SQLite insertion, preventing simultaneous coexistence of raw ASTs and database rows.

### 3. Allocator Arena Trimming

The ten direct allocator trims delivered in milestone 031 remain at their
indexing batch boundaries. MCP dispatch adds no per-request trim or duplicate
trim after a completed cold build or delta.

`malloc_trim(0)` attempts to release whole free pages; it does not free live
objects or guarantee a reduction in RSS. It is thread-safe but traverses arenas
under allocator locks and can issue `madvise`/`sbrk` calls. Reusing released pages
can require additional kernel work. These costs follow from the
[glibc implementation](https://github.com/bminor/glibc/blob/master/malloc/malloc.c)
and the [API contract](https://man7.org/linux/man-pages/man3/malloc_trim.3.html).
These costs keep trimming at batch boundaries; a lower RSS alone does not prove
better request latency.

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
- **Direct Binary Codecs**: [`BinaryEncode`](../../src/db/writer/cache.rs) and [`BinaryDecode`](../../src/db/writer/cache.rs) encode borrowed record fields without intermediate cloned DTOs. V4 cache frames carry the extraction key, payload length, checksum, and typed facts; non-V4 or corrupt entries are cache misses and are re-extracted.
- **Empirical Impact**: On warm re-indexing of 3,990 files, user CPU drops by **63%** (`44.49s` $\to$ `16.34s`, CPU load `374%` $\to$ `215%`) and wall-clock time drops to **`8.57s`** at `746 MB` Peak RSS.

### 7. Single-Pass AST Extraction and Native Typed Sidecar Facts
- **Incremental Lexical Route Binding**: [`RouteContextBuilder`](../../src/engine/ast/routes.rs) and [`SinglePassExtractor`](../../src/engine/ast/visitor.rs) collect lexical bindings, string/list/dict tables, and Python write assignments directly inside the primary `visitor::walk` pass instead of running a standalone `collect_lexical_bindings` pre-pass or recursive subtree walks per file.
- **Native Typed Fact Stores**: [`SemanticStore`](../../src/core/typed_facts.rs) and [`FlowStore`](../../src/core/typed_facts.rs) hold [`SemanticFact`](../../src/core/semantic.rs) and [`ValueFlowFacts`](../../src/core/semantic.rs) in native Rust structs (`TypedFacts`) during extraction and [`SymbolCatalog`](../../src/core/semantic.rs) linking. The local extraction cache stores typed facts directly in its V4 binary frame; public JSON projection remains a separate storage boundary.
- **Optional Scope Storage**: [`FrameworkScopeFacts`](../../src/core/semantic.rs) groups less common framework, template and collection vectors in an optional box. [`NodeSemanticEntry`](../../src/core/typed_facts.rs) holds admitted bindings, rebindings and parameter types natively. Public JSON remains flat, retains parameter order and preserves malformed raw metadata.
- **Single-Pass Linker Post-Processing**: [`link_compact_with_typed_registry`](../../src/engine/linker.rs) fuses value-flow propagation, uncertainty collection, and fault aggregation into a single pass over `all.values()`.

### 8. Shared Rule-IR Dispatch and Zero-Allocation Proof Lookups
- **Workspace-Scoped `Arc<LanguageContext>`**: [`DependencyRegistry`](../../src/engine/languages/dependency_registry.rs) builds [`LanguageContext`](../../src/engine/languages/mod.rs) per effective workspace/manifest scope and shares it via `Arc` across files without cloning `Vec<SyntaxRule>` per file. Python [`prepare_file`](../../src/engine/languages/python/syntax_operations.rs) prepares per-file render/import candidates while borrowing that shared context.
- **Grammar-Sized Tables**: [`CompiledSyntaxGrammar`](../../src/engine/languages/mod.rs) and [`CompiledRuleIR`](../../src/engine/languages/manifests.rs) dispatch by numeric kind ID through vectors sized by the actual grammar's node-kind count. Warm parser reuse also covers Vue script islands and HTML JavaScript fragment import scans through [`with_warm_parser`](../../src/engine/languages/mod.rs).
- **Indexed AST Kind & Callee Dispatch**: [`extract_projections`](../../src/engine/languages/typescript/projections.rs) and [`prepare`](../../src/engine/languages/python/projections.rs) pre-index declarative projection rules by AST node `kind` (`call_expression`, `jsx_Element`, `pair`, `decorator`) and callee/member name instead of scanning all rules linearly on every AST node.
- **Zero-Allocation Proof Maps**: Python projection proofs in [`observe_import`](../../src/engine/languages/python/projections.rs) and [`prepare`](../../src/engine/languages/python/projections.rs) store imported statement lines as `HashMap<String, HashSet<usize>>` and constructors as `HashMap<usize, HashMap<String, Constructor>>` so per-call and per-attribute `Proof::imported` / `Proof::api` checks query borrowed `&str` keys without allocating `String`s on the heap.

### 9. Language-Gated Linker Resolution and Semantic Context Memoization
- **Per-File Language Context Binding**: [`resolve`](../../src/engine/linker/resolution/mod.rs) binds `file_language_context = dependency_registry.language_context_for_path(path)` once at the top of the per-file `all.par_iter()` closure and reuses it across all candidate resolutions in that file.
- **Strict Language Profile Gating**: HTML template rule vectors and builtin template evidence construction in [`phase_value_flow`](../../src/engine/linker/resolution/value_flow_phase.rs) are gated on the active HTML profile so non-HTML files and non-HTML references incur no template-rule preparation or evidence formatting.
- **Shared Semantic Context Rule Tables**: [`Context`](../../src/engine/linker/semantic_context.rs) memoizes `Arc<ReceiverTypes>`, `Arc<ArgumentFactories>`, and `implicit_builtins` (`Arc<ImplicitBuiltinMap>`) per `(Arc::as_ptr(&rules), language)` key and Python `hierarchy_results` per `Arc::as_ptr(&rules)` (retaining pinned `Arc<LanguageContext>` clones for the pass lifetime to prevent ABA pointer reuse) instead of rescanning `syntax_rules` and cloning `HashMap<String, String>` for every file path in the workspace.
- **Single Codegen Unit Release Profile**: `Cargo.toml` configures `[profile.release]` with `codegen-units = 1` and `lto = "thin"` so hot loops across `ast`, `languages`, and `linker` inline across module boundaries.

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
| **Parallelizing Initial `evaluate` in `ValueFlowIndex`** | `src/engine/linker/value_flow/initialize.rs` | Order-dependent state divergence & non-deterministic resolution | Later bindings observe earlier writes and parent scope mutations in source order, and update shared state (`collection_elements`). The subsequent epoch reducer has a bounded iteration limit and cannot recover altered initial evaluation states. |

---

## Model and Agent Invariants (Immutable Guidelines)

Autonomous agents, builders, and language models modifying Forge indexing or storage components must strictly adhere to the following invariants. These rules are tied directly to concrete code symbols and protected by compiler checks and regression seams:

> [!IMPORTANT]
> Invariant: Symbol Catalog Fact Stripping
>
> In `src/engine/pipeline.rs` (`run_cold_pipeline`) and `src/core/semantic.rs` (`SymbolCatalog::insert_typed`), encode full storage facts into compressed `PreparedCatalogStorage` within extraction workers. Pass `retain_storage = false` for cold catalog admission to remove storage-only details and omit duplicate storage payloads. Preserve native binding metadata and typed flows required for linking; retain no additional workspace-wide full-facts copy.

- INV-PERF-CATALOG-FACT-STRIPPING: Remove storage-only details and duplicate storage payloads during cold catalog admission while retaining typed resolution facts. [`run_cold_pipeline`](../../src/engine/pipeline.rs) [`SymbolCatalog`](../../src/core/semantic.rs).

> [!IMPORTANT]
> Invariant: Bounded Chunk Persistence and Allocator Trimming
>
> In `src/db/writer.rs` (`populate_owned`), keep `StagedFilePayload.storage_rows` Zstandard-compressed during staging; `prepare_catalog_storage` uses compression level -3. Ingestion must process files in bounded batches of at most 500 rows (`MAX_MULTI_VALUE_BATCH_ROWS = 500`), removing payloads from memory prior to database insertion. Agents must preserve the explicit calls to `trim_allocator` (`libc::malloc_trim(0)`) after extraction and after persistence to prevent glibc multi-arena fragmentation from exceeding the 800 MB RSS ceiling.

- INV-PERF-BOUNDED-STAGING-AND-TRIM: Keep staged file rows Zstandard-compressed, drain payloads in bounded batches, and invoke `trim_allocator` at extraction and persistence boundaries to stay within the 800 MB RSS budget. [`StagedFilePayload`](../../src/db/writer.rs) [`populate_owned`](../../src/db/writer.rs).

> [!IMPORTANT]
> Invariant: Tree-Sitter AST Traversal Loop Integrity
>
> In `src/engine/ast/visitor.rs` (`walk`), tree traversal must remain a flat iterative loop over `tree_sitter::TreeCursor` (`goto_first_child`, `goto_next_sibling`, `goto_parent`). Agents must not replace `walk()` with `tree_sitter::QueryCursor` or introduce per-node FFI guards like `named_child_count() == 0`. The traversal must yield balanced `(node, entering)` events required by `SinglePassExtractor` and `ScopeAnalysis`.

- INV-PERF-FLAT-TREE-CURSOR-WALK: Traverse syntax trees with a flat iterative `TreeCursor` loop yielding balanced `(node, entering)` events without `QueryCursor` state machines or per-node `named_child_count` FFI pre-checks. [`walk`](../../src/engine/ast/visitor.rs) [`SinglePassExtractor`](../../src/engine/ast/visitor.rs).

> [!IMPORTANT]
> Invariant: Single-Pass AST Extraction Without Pre-Pass Walks
>
> In `src/engine/ast/visitor.rs` (`SinglePassExtractor`) and `src/engine/ast/routes.rs` (`RouteContextBuilder`, `RouteContext`), lexical route bindings, string/list/dict tables, and local write targets must be accumulated incrementally during the primary `visitor::walk` pass and finalized on exit via `RouteContextBuilder::finish`. Agents must never introduce standalone full-file AST pre-passes or recursive subtree walks per file or per node.

- INV-PERF-SINGLE-PASS-AST-EXTRACTION: Accumulate lexical route bindings and local write targets incrementally inside `SinglePassExtractor` via `RouteContextBuilder` rather than running separate full-tree AST pre-passes. [`SinglePassExtractor`](../../src/engine/ast/visitor.rs) [`RouteContextBuilder`](../../src/engine/ast/routes.rs) [`RouteContext`](../../src/engine/ast/routes.rs).

> [!IMPORTANT]
> Invariant: Native Typed Sidecar Facts Without Intermediate JSON
>
> In `src/core/typed_facts.rs` (`SemanticStore`, `FlowStore`, `TypedFacts`), keep semantic and value-flow facts in native typed stores during extraction and linking. Preserve the existing V3 JSON-value overlays at the binary cache boundary in `src/db/writer/cache.rs` and hydrate typed stores when loading. Public and legacy compatibility views retain their existing serialization and malformed-evidence behavior.

- INV-PERF-TYPED-SIDECAR-FACTS: Use native typed stores for in-memory extraction and linking, preserving the V3 cache and public compatibility boundaries. [`SemanticStore`](../../src/core/typed_facts.rs) [`FlowStore`](../../src/core/typed_facts.rs) [`TypedFacts`](../../src/core/typed_facts.rs).

> [!IMPORTANT]
> Invariant: Shared Workspace Rule-IR Context and Indexed AST Dispatch
>
> Share effective rule tables through `Arc<LanguageContext>` per workspace/manifest scope and index declarative candidates by AST kind and callee/member name. Keep source-dependent render/import preparation per file through `prepare_file`. Reuse the shared rule vectors rather than cloning `Vec<SyntaxRule>` per file or scanning all rules on every AST node.

- INV-PERF-SHARED-RULE-CONTEXT-AND-DISPATCH: Share compiled rule tables via `Arc<LanguageContext>` per workspace scope and index declarative projection rules by AST node kind and callee/member name instead of scanning all rules per node. [`DependencyRegistry`](../../src/engine/languages/dependency_registry.rs) [`LanguageContext`](../../src/engine/languages/mod.rs) [`extract_projections`](../../src/engine/languages/typescript/projections.rs).

> [!IMPORTANT]
> Invariant: Zero-Allocation Projection Proof Lookups
>
> In `src/engine/languages/python/projections.rs` (`observe_import`, `prepare`), per-call and per-attribute projection proofs (`Proof::imported`, `Proof::api`) must query two-level borrowed-key maps (`HashMap<String, HashSet<usize>>` for statement lines and `HashMap<usize, HashMap<String, Constructor>>` for constructors) and pre-indexed import heads. Agents must never allocate temporary owned `String` keys (`to_owned()`, `format!`) inside per-node or per-reference proof predicates.

- INV-PERF-ZERO-ALLOC-PROJECTION-PROOFS: Query two-level borrowed-key maps (`HashMap<String, HashSet<usize>>` and `HashMap<usize, HashMap<String, Constructor>>`) in extractor proof predicates without allocating owned `String` keys per call or reference. [`observe_import`](../../src/engine/languages/python/projections.rs) [`prepare`](../../src/engine/languages/python/projections.rs).

> [!IMPORTANT]
> Invariant: Language-Gated Linker Resolution and Memoized Semantic Context
>
> In `src/engine/linker/resolution/mod.rs` (`resolve`) and `resolution/value_flow_phase.rs` (`phase_value_flow`), `src/engine/linker/semantic_context.rs` (`Context`), and `src/engine/linker.rs` (`link_compact_with_typed_registry`), the linker must bind `language_context_for_path` once per file, gate language-specific template rules and evidence builders behind the active language profile, memoize `ReceiverTypes`, `ArgumentFactories`, and `implicit_builtins` per shared `Arc<LanguageContext>` (retaining pinned `Arc<LanguageContext>` clones for the pass lifetime), and fuse post-resolution `all.values()` passes.

- INV-PERF-LANGUAGE-GATED-LINKER-RESOLUTION: Bind `language_context_for_path` once per file in `resolve`, gate HTML template rule vectors and `template_evidence` behind active HTML guards, memoize `Context` rule tables per pinned `Arc<LanguageContext>`, and fuse post-resolution `all.values()` passes in `link_compact_with_typed_registry`. [`resolve`](../../src/engine/linker/resolution/mod.rs) [`phase_value_flow`](../../src/engine/linker/resolution/value_flow_phase.rs) [`Context`](../../src/engine/linker/semantic_context.rs) [`link_compact_with_typed_registry`](../../src/engine/linker.rs).

> [!IMPORTANT]
> Invariant: Single Codegen Unit Release Optimization
>
> In `Cargo.toml` (`[profile.release]`), production builds must keep `codegen-units = 1` and `lto = "thin"` enabled so cross-module hot paths in `SinglePassExtractor`, `DependencyRegistry`, and `resolution::resolve` inline fully. Increasing `codegen-units` degrades cold-build indexing throughput and inflates CPU cycles across AST and linker loops.

- INV-PERF-RELEASE-SINGLE-CGU: Keep `codegen-units = 1` and `lto = "thin"` in `[profile.release]` so cross-module AST extraction and linker resolution loops inline in release binaries. [`run_cold_pipeline`](../../src/engine/pipeline.rs) [`resolve`](../../src/engine/linker/resolution/mod.rs).

> [!IMPORTANT]
> Invariant: SQLite Ingestion PRAGMAs and WAL Restoration Order
>
> In `src/db/writer.rs` (`prepare_pipeline_schema` and `populate_owned`), cold-indexing candidate databases must initialize with `journal_mode = MEMORY; synchronous = OFF; temp_store = MEMORY; cache_spill = ON;` and defer secondary index creation until after row ingestion (`sqlite_ingestion.mode = deferred-index-batched`). Before atomic publication, the writer must restore `journal_mode = WAL; synchronous = NORMAL;` and execute `PRAGMA wal_checkpoint(TRUNCATE);`. Changing pragma ordering or omitting WAL restoration causes reader lockouts and database corruption.

- INV-PERF-SQLITE-INGESTION-PRAGMAS: Initialize staged SQLite databases with in-memory journal and deferred secondary indexes during bulk ingestion, then restore `WAL` mode and checkpoint before atomic publication. [`prepare_pipeline_schema`](../../src/db/writer.rs) [`populate_owned`](../../src/db/writer.rs).

> [!IMPORTANT]
> Invariant: Deterministic 17-Domain Merkle Commitments
>
> In `src/db/writer/seal.rs` (`seal::file_domain_leaves`, `seal::logical_domain_leaves`) and `src/core/commitments.rs`, in-memory leaf hashing for the 13 logical domains and C-API tokenizer hashing for the 4 FTS domains must yield 100% bit-identical digests matching SQLite table scans (`forge-owner-sha256-v2`). Tokens must be ordered canonically by `(term, docid, col, pos)` using `term_map`. Any modification must pass `tests/commitment_integrity.rs`.

- INV-PERF-DETERMINISTIC-MERKLE-SEALING: Compute 13 logical domain commitments from in-memory projections and 4 FTS5 domain commitments via the native C-API tokenizer with canonical `(term, docid, col, pos)` ordering so `forge-owner-sha256-v2` roots remain bit-identical. [`file_domain_leaves`](../../src/db/writer/seal.rs) [`logical_domain_leaves`](../../src/db/writer/seal.rs).

> [!IMPORTANT]
> Invariant: Strict Extraction Cache Key Binding
>
> In `src/db/writer/cache.rs` (`ExtractionCache`) and `src/engine/languages/dependency_registry.rs`, cache keys must strictly bind source SHA-256 digest, normalized relative path, Tree-sitter grammar ID, `INDEX_SEMANTICS_VERSION`, and `effective_ruleset_hash` (which incorporates bundled framework manifests via `build.rs`). Agents must never omit the ruleset hash or grammar identity from the cache key, as doing so leads to stale, invalid extraction across manifest or grammar upgrades.

- INV-PERF-CACHE-KEY-BINDING: Bind `ExtractionCache` keys to source SHA-256, normalized path, Tree-sitter grammar ID, `INDEX_SEMANTICS_VERSION`, and `effective_ruleset_hash`. [`ExtractionCache`](../../src/db/writer/cache.rs) [`DependencyRegistry`](../../src/engine/languages/dependency_registry.rs).

> [!IMPORTANT]
> Invariant: Interned Linker Keys Preserve Semantic Proof
>
> Build one borrowed `LinkerInterner` per linking pass. Use its `SymbolId` and `PathId` keys in prepared semantic tables and restore canonical strings when emitting graph results. Preserve declared-package activation and lexical import proof; file-import presence does not replace dependency activation.

- INV-PERF-INTERNER-KEYS: Preserve canonical graph identities and semantic proof while interning prepared linker keys. [`LinkerInterner`](../../src/engine/linker/interner.rs).

> [!IMPORTANT]
> Invariant: Binary Catalog Storage and Reference Occurrences Without JSON Round-Trips
>
> In `src/db/writer.rs` (`PreparedCatalogStorage`) and `src/core/semantic.rs` (`SymbolCatalog::insert_typed`, `decode_catalog_references`), persisted catalog payloads and reference occurrence blobs must use compact binary encoding (`encode_references` / `decode_references`) compressed with Zstd rather than `serde_json::to_vec` / `serde_json::from_slice`.

- INV-PERF-BINARY-CATALOG-STORAGE: Encode `PreparedCatalogStorage` and `SymbolCatalog` reference occurrences in binary format before Zstd compression instead of round-tripping through `serde_json`. [`SymbolCatalog`](../../src/core/semantic.rs) [`encode_references`](../../src/db/writer/cache.rs) [`decode_references`](../../src/db/writer/cache.rs).

> [!IMPORTANT]
> Invariant: Bounded Rust Receiver and Module Proof
>
> Resolve Rust receivers and module imports through indexed declarations, visible imports and bounded value flow. Explicit standard-wrapper projections and proven nominal `Deref::Target` implementations retain their own contracts. Missing, ambiguous, cyclic, inaccessible or exhausted proofs remain uncertain; imports and receiver names alone do not establish exact edges.

- INV-PERF-BOUNDED-RUST-PROOF: Preserve visibility, uniqueness and exhaustion boundaries for Rust receiver and module resolution. [`RustMembers`](../../src/engine/languages/rust/linker.rs) [`resolve_type`](../../src/engine/languages/rust/linker.rs).

> [!IMPORTANT]
> Invariant: No Nested Parallel Iterators Inside Chunked Pipelines
>
> In `src/db/writer.rs` (`prepare_catalog_rows`), file chunks are already dispatched across Rayon worker threads. Agents must never nest Rayon parallel iterators (`.par_iter()`) inside individual chunk workers. Inner loops must use standard sequential iteration (`.iter()`) to avoid thread pool starvation, thread parking, futex contention, and massive system CPU spikes.

- INV-PERF-NO-NESTED-PAR-ITER: Never nest Rayon `.par_iter()` inside already parallelized pipeline chunks; use sequential `.iter()` within worker tasks. [`prepare_catalog_rows`](../../src/db/writer.rs) [`persist_catalog_files`](../../src/db/ingest.rs).

> [!IMPORTANT]
> Invariant: Zero-Clone Ingestion for Navigation Details
>
> In `src/db/ingest.rs` (`flush_node_batch`), database batch writes must never clone `Node` instances or allocate intermediate JSON maps to strip transient fields (`"doc"`, `"docstring"`). Filter fields on-the-fly during serialization through borrowed adapters (`NavigationDetails`) directly referencing source nodes.

- INV-PERF-ZERO-NODE-CLONES-ON-INGEST: Filter navigation details during serialization through borrowed wrappers without cloning `Node` instances in batch write loops. [`flush_node_batch`](../../src/db/ingest.rs) [`NavigationDetails`](../../src/core/models.rs).

> [!IMPORTANT]
> Invariant: Manifest Syntax Rule Interning
>
> In `src/engine/linker/interner.rs` (`LinkerInterner`) and `src/engine/linker.rs`, declarative framework manifest rules must be interned once per language context rather than re-evaluating or cloning rules on every file path in the workspace.

- INV-PERF-MANIFEST-SYNTAX-INTERNING: Intern manifest syntax rules at language context scope instead of re-evaluating or allocating rules per file. [`LinkerInterner`](../../src/engine/linker/interner.rs).

> [!IMPORTANT]
> Invariant: Host Performance Profile and Governor Precondition
>
> Benchmarks and cold-build latency measurements reflect real algorithmic performance only when the host OS operates in `performance` platform profile (`powerprofilesctl set performance`, `energy_performance_preference: performance`). Running benchmarks under `power-saver` caps CPU clocks to low base frequencies (e.g. 400 MHz – 1.9 GHz instead of 4.5 GHz boost) and artificially inflates wall-clock and persistence timings by 1.8x–2.0x.

- INV-PERF-HOST-POWER-GOVERNOR-FOR-BENCHMARKS: Benchmark runs require verification that the host platform profile is not set to `power-saver` before attributing measured wall-clock regressions to code changes. [`cold_test_coverage_benchmark.py`](../../benchmarks/cold_test_coverage_benchmark.py).

> [!IMPORTANT]
> Invariant: Stage Payload Disk Spooling
>
> In `src/engine/pipeline.rs` (`consume_pipeline_results`) and `src/db/writer.rs` (`populate_owned`), cold-build extraction payloads (`encoded_facts` and `storage_rows`) must be spooled directly to stage-private temporary disk storage (`payloads.bin` under `.forge-stage-*`) during consumer ingestion rather than accumulated in heap memory as raw byte vectors (`StagedFilePayload`). `StagedFilePayload` retains only lightweight offset/length references (`StagedPayloadBlob`), releasing 50–60 MB of heap memory during the cross-file linker phase. The final persistence pass reads blobs sequentially in bounded 500-file chunks, preserving route composition invalidation, Merkle leaf commitments, and atomic stage cleanup.

- INV-PERF-STAGE-PAYLOAD-SPOOLING: Spool extraction payload blobs to stage-private disk storage during consumer ingestion and read by chunk during persistence to keep heap memory bounded during linking. [`consume_pipeline_results`](../../src/engine/pipeline.rs) [`populate_owned`](../../src/db/writer.rs).

> [!IMPORTANT]
> Invariant: Zero-Clone Ingestion for Batched Node Attributes
>
> In `src/db/ingest.rs` (`persist_files_impl`), `node_batch` rows must borrow string attributes (`id`, `kind`, `name`, `qualname`, `language`) as `&str` (`BorrowedSqlValue::Text`) directly from the active loaded chunk rather than allocating owned `String`s (`BorrowedSqlValue::OwnedText`). The lifetime of `node_batch` is strictly bounded by the loaded chunk, flushing before the chunk is dropped to eliminate hundreds of thousands of heap string clones while keeping dynamic details in `BorrowedSqlValue::OwnedJsonText`.

- INV-PERF-ZERO-CLONE-BATCHED-NODES: Borrow node string attributes directly from loaded file chunks in batch insertion loops without heap-cloning strings. [`persist_files_impl`](../../src/db/ingest.rs) [`BorrowedSqlValue`](../../src/db/batch.rs).

> [!IMPORTANT]
> Invariant: Deferred Transient Node Paths Cache
>
> In `src/db/writer.rs` (`populate_owned`) and `src/db/delta.rs` (`persist_delta`), the `node_paths` lookup cache must be constructed immediately before `persist_graph` using `Cow<'a, str>` borrowed keys from in-memory facts rather than cloning owning `String` node IDs across the entire workspace. The cache must be explicitly dropped immediately after graph persistence to reclaim memory before index and Merkle sealing.

- INV-PERF-DEFERRED-TRANSIENT-NODE-PATHS: Construct the `node_paths` cache immediately before graph persistence with borrowed `Cow<'a, str>` keys and drop it immediately after use. [`populate_owned`](../../src/db/writer.rs) [`persist_graph`](../../src/db/ingest.rs).

> [!IMPORTANT]
> Invariant: Edge Occurrence Endpoint Hash Reuse
>
> In `src/db/ingest.rs` (`persist_graph`, `persist_dependencies`), resolved endpoint hashes `(src_target, dst_target)` computed during the occurrence pass must be retained in an occurrence-aligned vector (`resolved_edge_paths`) and forwarded directly to `persist_dependencies`. Dependencies must reuse these pre-resolved endpoint path hashes rather than repeating `graph_endpoint_hashes` lookups or executing redundant SQLite queries.

- INV-PERF-OCCURRENCE-ENDPOINT-HASH-REUSE: Reuse resolved endpoint hashes from edge occurrence passes in dependency persistence to eliminate redundant hash-map lookups. [`persist_graph`](../../src/db/ingest.rs) [`persist_dependencies`](../../src/db/ingest.rs).

> [!IMPORTANT]
> Invariant: In-Memory File Language Fallback for Coverage
>
> In `src/db/ingest.rs` (`persist_graph`), resolution coverage owner languages must be resolved from known in-memory file entries (`known_file_entries`) before falling back to SQLite queries. Cold builds must avoid round-tripping missing owners through JSON serialization and SQLite `json_each(?1)` subqueries whenever file metadata is available in memory.

- INV-PERF-IN-MEMORY-FILE-LANGUAGE-FALLBACK: Resolve coverage owner languages from in-memory file entries to eliminate JSON-encoded SQLite subqueries. [`persist_graph`](../../src/db/ingest.rs).

> [!IMPORTANT]
> Invariant: Compact Edge and Coverage Models with Shared Paths and Tags
>
> In `src/core/models/compact_graph.rs`, internal `Edge` and `Coverage` graph models must store file paths as shared `Arc<str>` handles and evidence as `CompactTag` (with static strings for recurring diagnostics such as `"exact"`, `"inferred"`, `"references"`). Graph construction must share a single `path_arc` across all edges and coverage generated within each file resolver to eliminate millions of redundant heap string allocations while preserving public model representations, sort ordering, and bit-for-bit Merkle commitments.

- INV-PERF-COMPACT-EDGE-COVERAGE-MODELS: Compact internal edge and coverage models with shared `Arc<str>` paths and `CompactTag` evidence. [`Edge`](../../src/core/models/compact_graph.rs) [`Coverage`](../../src/core/models/compact_graph.rs).

> [!IMPORTANT]
> Invariant: Content-Keyed Path Pooling during Compact Graph Restoration
>
> In `src/engine/linker/compact.rs` (`GraphPart::into_graph`), restoring packed compressed graph parts must use a local content-keyed path pool (`HashMap<&str, Arc<str>>`) to reconstruct shared `Arc<str>` handles without re-allocating duplicate path strings across decoded edge and coverage records.

- INV-PERF-COMPACT-GRAPH-PATH-POOLING: Pool restored path strings into shared `Arc<str>` handles when unpacking compressed graph parts. [`GraphPart::into_graph`](../../src/engine/linker/compact.rs).

> [!IMPORTANT]
> Invariant: Normalized Vector Storage for Node Binding Indexes
>
> In `src/engine/linker.rs` and downstream resolution modules, `NodeBindingIndex` must store per-node symbol identifiers in a sorted, deduplicated `Vec<SymbolId>` rather than separate heap hash tables (`hashbrown::HashSet<SymbolId>`). Membership checks must perform binary searches (`binary_search(&id).is_ok()`). Normalization (`sort_unstable()` and `dedup()`) must occur once after collecting declarations, Python flow bindings, and branch joins before publishing immutable indexes.

- INV-PERF-NORMALIZED-NODE-BINDING-VECTORS: Store per-node binding identifiers in sorted, deduplicated vectors with binary search membership checks. [`NodeBindingIndex`](../../src/engine/linker.rs) [`has_indexed_symbol_id`](../../src/engine/linker/typescript/resolution.rs).

> [!IMPORTANT]
> Invariant: Borrowed Map Keys in Package Export Indexing
>
> In `src/engine/linker/traits.rs` (`LanguageLinker::package_exports_borrowed`) and `src/engine/linker/indexing.rs`, package export indexing across language linkers must accept borrowed map references (`&BTreeMap<&'a str, &'a Facts>`), eliminating redundant cloning of all workspace file paths into temporary maps.

- INV-PERF-BORROWED-MAP-KEY-EXPORT-INDEXING: Accept borrowed map keys in borrowed package export indexing to avoid workspace path string cloning. [`LanguageLinker::package_exports_borrowed`](../../src/engine/linker/traits.rs) [`package_exports`](../../src/engine/linker/indexing.rs).

> [!IMPORTANT]
> Invariant: Zero-Clone Parallel File Module Resolution
>
> In `src/engine/linker/resolution/mod.rs`, extracting file module qualified names for `FileResolver` must borrow `&'a str` directly (`Cow::Borrowed`) when a `"module"` node is present, allocating an owned string (`Cow::Owned`) only in dynamic fallback branches.

- INV-PERF-ZERO-CLONE-PARALLEL-MODULE-NAME: Borrow module names directly from module nodes without heap allocation during parallel file resolution. [`resolve`](../../src/engine/linker/resolution/mod.rs).

> [!IMPORTANT]
> Invariant: Zero-Clone Dictionary Bulk Ingestion
>
> In `src/db/dictionary.rs`, `PathDictionaryCache::bulk_insert` must accept `impl IntoIterator<Item = &'a str>` and push `BorrowedSqlValue::Text(path_ref)` into multi-value insertion batches, cloning strings only once for the in-memory dictionary cache.

- INV-PERF-BORROWED-PATH-DICTIONARY-BULK-INSERT: Borrow path string references in dictionary bulk insertion batches to avoid duplicate SQL string cloning. [`PathDictionaryCache::bulk_insert`](../../src/db/dictionary.rs).
