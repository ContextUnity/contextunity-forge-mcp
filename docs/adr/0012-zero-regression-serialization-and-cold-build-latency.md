---
title: "ADR 0012: Zero-Regression Serialization and Cold Build Latency Budget"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0012: Zero-Regression Serialization and Normalized Throughput Budgets

## Status
Accepted

## Context
ContextUnity Forge MCP must maintain linear, predictable performance across codebases of any size. As semantic analysis was enriched with interprocedural value-flow, receiver inference, and overload disambiguation, serializing transient analysis state into indexed tables and storing uncompressed AST facts caused write amplification and memory bloat. Architectural laws must be codified as invariant ratios per unit of code (per 1,000 files/nodes) rather than pinned to specific repositories.

## Decision

1. **Normalized Cold Build Throughput Budget**:
   - **End-to-End Build**: Total indexing throughput must maintain `>= 400 files/sec` (`<= 2.5s per 1,000 files` across extraction, cross-file linking, SQLite persistence, and Merkle root sealing).
   - **AST Extraction**: `>= 800 files/sec` (`<= 1.25s per 1,000 files`).
   - **Node Persistence**: `rows_ms` must remain `<= 1.5ms per 1,000 nodes`.
   - **Merkle Sealing**: `>= 100,000 entities/sec` (`<= 10ms per 1,000 entities`).

2. **Lean Node Projection Law**:
   - `nodes.details` serves strictly as an index-projection surface for symbol queries, search, and navigation (`signature`, `doc`, `receiver_name`, explicit decorators).
   - Prohibit serializing ephemeral interprocedural analysis trees, full value-flow control-flow graphs, or raw scope maps into `nodes.details`.
   - Average node details payload size is capped at `<= 120 bytes per node`.

3. **Storage Density & Compressed Fact Budget**:
   - Overall SQLite database footprint must remain `<= 45 KiB per indexed source file` (or `<= 3.0 KiB per indexed node`).
   - Intermediate file AST facts (`local_facts.facts_blob`) must use Zstandard dictionary compression with a target ratio of `>= 3.5:1`, capping fact storage at `<= 15 KiB per source file`.

4. **Bulk Ingestion SQLite Pragmas**:
   - Bulk cold build indexing and full re-indexing must run under asynchronous memory journal pragmas: `PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;`.
   - An explicit WAL checkpoint is executed upon ingestion completion immediately before Merkle tree sealing and verification.

5. **Normalized Interactive Tool Latency Ceiling**:
   - Exact/prefix symbol lookup (`code_map_search` with `exact=true`): `<= 10ms`.
   - Full-text & BM25 hybrid search (`code_map_search`): `<= 30ms`.
   - Structural symbol inspection (`code_map_inspect`, `code_map_explain`): `<= 25ms`.
   - Graph impact & test dependency traversal (`code_map_impact`, `code_map_tests`): `<= 50ms`.
   - Scoped removal safety proof (`code_map_prove_removal`): `<= 30ms`.

6. **Target-Scoped Evaluation Law**:
   - Localized interactive tools must never issue unindexed table scans (`LIKE '%...'`), unconstrained table counts (`SELECT count(*) FROM table`), or global workspace diagnostics during symbol-level operations.

7. **Prohibition Against Redundant Disk Re-Reads & Full-Source Mirroring**:
   - The scanner reads workspace files once during the AST extraction phase. Persistence pipelines (`persist_files`, `persist_graph`, etc.) are strictly prohibited from re-reading files from disk (`fs::read_to_string`).
   - SQLite is a structural index and graph storage engine, not a raw source code mirror. Creating virtual FTS tables to store full, uncompressed file source text is prohibited. All lexical and token search must route through existing symbol indices (`node_search`), path indices (`files`), or tree-sitter AST extractors.

8. **Zero-Allocation Hot-Path Law in Graph Persistence**:
   - Loops iterating over high-cardinality collections (edges, occurrences, dependencies) must never allocate ad-hoc heap collections (e.g. `HashSet` of multi-field tuples) or perform complex multi-segment hashing per record. Deduplication must be stream-oriented, batch-oriented, or handled via ordered sorting without CPU cache thrashing.

9. **B-Tree Index Lean Law**:
   - Creating secondary B-Tree indexes on text fields already indexed by FTS5 virtual tables (e.g. creating `COLLATE NOCASE` indexes on `name` or `qualname`) is strictly prohibited.
   - Bulk B-tree indexing latency during cold build must not exceed `<= 1.0s per 100,000 entities` (`indexes_ms <= 1000ms`).

10. **Prohibition of Correlated Subqueries on Broad Candidate Sets**:
   - Graph degree, in-degree connectivity, or edge existence scoring (`graph_boost`) must NEVER run as correlated subqueries against high-cardinality tables (`edges`, `edge_occurrences`) over unconstrained candidate CTEs.
   - Any graph-degree scoring must evaluate strictly on the final bounded candidate window (`LIMIT 30` or `LIMIT 50`).

11. **Bounded Profiling and Honest Receipts**:
   - Record measured metrics honestly in milestone receipts without spinning in recursive profiling loops (cap profiling iterations to <= 3 per turn).
   - If an acceptance budget remains open due to physical or external bottlenecks, document the measured finding transparently in the receipt and hand off rather than stalling execution.

## Consequences
- Throughput and resource consumption scale linearly and predictably with repository size.
- Database storage footprint remains bounded within strict density envelopes (<= 45 KiB/file) without raw source duplication.
- Eliminates write amplification, redundant disk I/O, and cache thrashing during high-volume node ingestion.
- Merkle tree determinism and index verification integrity remain 100% preserved.
