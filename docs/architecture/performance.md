---
title: "Performance Budgets, Search Architecture, and Storage Compaction"
doc_type: architecture
---

# Performance Budgets, Search Architecture, and Storage Compaction

## Performance Objectives & Latency Budgets

To ensure instant responsiveness for AI agent workflows, Forge MCP maintains strict latency budgets across all MCP tools:

| Operation / Tool | Target Median | Target p95 | Architectural Mechanism |
| :--- | :--- | :--- | :--- |
| **Tool Response Baseline** | < 10 ms | < 30 ms | Inventory scan debouncing & connection caching |
| **`code_map_search`** | < 30 ms | < 30 ms | SQLite FTS5 BM25 index + exact prefix matching |
| **`ast_grep_search`** | < 30 ms | < 30 ms | Candidate file pre-filtering via indexed symbol table |
| **`code_map_tests`** | < 50 ms | < 50 ms | Bounded traversal depth (`max_depth = 4`) + lexical fallback |
| **`code_map_prove_removal`** | < 30 ms | < 30 ms | Target-scoped dependency evaluation |

## Key Architectural Systems

### 1. MCP Admission Inventory Scan Debouncing
By default, Forge verifies workspace freshness before executing tool requests. Full filesystem walks (`source_inventory`) over thousands of paths add 28–59 ms of unnecessary overhead per request.
- **Debounced Scan Window**: `Server` caches a successful inventory admission for five seconds. Its read boundary reports `inventory_scan_ms: 0.0` on cached admission and retains the timestamp of the last scan. MCP responses omit internal timing fields.
- **Invalidation**: Database identity, workspace root, and adapter changes invalidate the cached admission. After the TTL expires, `scan_reusing` checks the full admitted inventory and detects additions, deletions, and edits.

### 2. Two-Tier Hybrid Search (FTS5 BM25)
`code_map_search` combines structural exact matching with full-text lexical ranking:
- **Tier 1 (Exact & Prefix)**: Exact name or qualified-name matches receive 1000 points; name prefixes receive 500 points. Exact-only requests use the case-insensitive symbol indexes.
- **Tier 2 (Full-Text BM25)**: `node_search` indexes names, qualified names, paths, docstrings, and distinct module reference expressions. Each FTS and structural candidate branch applies the complete hybrid score and path/kind/document filters before its limit. The limit is at least 500 and grows with the requested offset and page size; the separate count retains the complete matching total. Inbound semantic connectivity adds a bounded 50-point bonus; path, line, and ID resolve score ties.
- **Documentation and grouping**: Code search excludes Markdown by default. `include_docs` admits documentation; `group_by_file` places each file path once around its returned symbols.

### AST and test discovery
`tests_paged` bounds graph traversal at depth four and uses lexical test-name matching when the graph has no inbound tests. `ast_grep_search` parses declaration names through the extraction language profile and selects matching symbols through the name index and `files`. Other patterns use literal tokens in the existing `node_search`; patterns without usable literals retain indexed-file candidates. The AST matcher reads and verifies each candidate source digest before matching with tree-sitter.

### 3. Localized Removal Proof
`code_map_prove_removal` evaluates safety strictly within the target's dependency subgraph:
- **Target-Scoped Evaluation**: Checks unresolved calls and references only where the referenced expression matches the target or exists within modules directly depending on the target.
- **Independent Safety Flag**: Returns `target_safe_to_remove: bool` independently of unrelated workspace-wide parser errors or unlinked third-party globals.

### 4. Database Storage Compaction & Density Budget
To maintain an overall database storage density of `<= 45 KiB per source file` (or `<= 3.0 KiB per node`):
- **Zstandard Compression**: `local_facts` stores complete AST and analysis facts as buffered streaming level-1 Zstd frames with a target ratio of `>= 3.5:1`, keeping compressed fact storage `<= 15 KiB per source file`. Navigation rows omit `value_flow`, `bindings`, `rebindings`, `param_types`, and empty optional fields; delta resolution contracts hydrate complete facts from `local_facts`.
- **Zero-Redundancy Persistence**: Rely on bidirectional indexed queries on `edges_raw` (`src_hash`, `dst_hash`) without duplicating reverse dependency tables. Ingestion hot paths stream batches without ad-hoc per-edge heap allocations.
- **Index Compaction**: Use SQLite `WITHOUT ROWID` tables for junction tables (`edge_occurrences_raw`, `dependencies_raw`, `shared_owners_raw`).
- **Dictionary Projections**: `nodes_data` stores path IDs from `path_dictionary`; the writable `nodes` view exposes the original text columns. Graph evidence and confidence share `coverage_evidence`. `shared_keys_raw` references node qualified names by node ID; node mutation materializes the original key before its source changes. Text views preserve query results, and commitment hashers resolve dictionary IDs through preloaded maps.
- **Commitment Storage**: `domain_commitments_raw` stores a domain, owner path ID, and 32-byte digest. Its text projection exposes canonical leaf keys and hexadecimal digests. Cold builds commit imported rows, checkpoint, and create ordinary indexes before sealing. Candidates with at least 8,192 nodes and multiple Rayon threads hash independent domains through read-only connections; one transaction installs commitments and the root. Delta sealing retains its transaction boundary.
- **Phase Timing**: `indexes_ms` and `seal_ms` measure their individual wall times. `index_and_seal_ms` measures their combined wall time; `persist_ms` includes that interval.

### 5. Zero-Regression Serialization & Normalized Throughput Budgets (ADR 0012)
See [ADR 0012: Zero-Regression Serialization and Normalized Throughput Budgets](../adr/0012-zero-regression-serialization-and-cold-build-latency.md):
- **Lean Node Details**: Restrict `nodes.details` to directly indexed navigation fields (`signature`, `doc`, `receiver_name`, explicit decorators), maintaining an average payload size of `<= 120 bytes per node` and capping `rows_ms` at `<= 1.5ms per 1,000 nodes`.
- **Bulk SQLite Ingestion Pragmas**: Cold indexing uses `PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;` on an isolated candidate database. It seals and commits the candidate, restores WAL mode, verifies SQLite integrity, and checkpoints WAL before atomic publication.
- **Throughput Rate Gate**: Total cold build throughput must maintain `>= 400 files/sec` (`<= 2.5s per 1,000 files`). Any drop below 400 files/sec is an unverified regression and a hard blocker.
