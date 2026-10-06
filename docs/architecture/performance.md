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
- **Throughput Recommendation**: Around 400 files/sec end-to-end and 800 files/sec for extraction are useful initial comparison values. The 10-second Commerce cold-build aspiration helps prioritize phase-level work; it is a gate only where a milestone explicitly admits it.
