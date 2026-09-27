# Operational Boundaries & Performance Targets

This document outlines the operational limits, engine constraints, and performance targets of ContextUnity Forge MCP.

## Operational Boundaries

- **Static vs Runtime Analysis**: The index captures static syntax and resolvable relationships from Tree-sitter AST and import facts. Dynamic dispatch, runtime reflection, receiver typing, and external unindexed sources can remain unresolved. Empty caller results alone do not establish that removal is safe.
- **Delta Transactions**: Delta updates affected graph contributions within an atomic SQLite write-ahead log (WAL) transaction. Its runtime includes repository inventory checks, dependency resolution, commitment verification, and persistence.
- **Ambiguity & Disambiguation**:
  - Symbols with identical language-neutral stems (e.g. `util.py` and `util.ts`) across different languages or modules remain visible in resolution coverage.
  - Python receiver reassignment and dynamic monkey-patching have confidence boundaries: verify critical call sites in source using `get_code_snippet` or `ctx_read`.
- **Query Limits & Timeouts**:
  - **SQLite Progress Budget**: 2.0 seconds execution budget. Unbounded or runaway recursive queries are interrupted and return an actionable retry error.
  - **Max Output Ceiling**: Responses have a hard ceiling of 64 KiB (customizable via adapter policy).
  - **Row & Value Limits**: SQLite scalar values and serialized results are capped at 8 MiB. SQL analysis queries return at most 1,000 rows. Public `--limit` values range from 1 to 10,000 (default 30).
  - **Graph Depth**: Traversal operations (`impact`, `slice`) accept depths up to 16. Depths > 16 are rejected.
  - **High-Degree Node Guard**: Nodes with more than 1,000 immediate graph links reject deep traversals (`depth > 1`) before recursion. Callers must use `depth=1` and page direct links.
  - **Removal Scope**: `code_map_prove_removal` rejects scopes exceeding 10,000 nodes to prevent unbounded locking.
- **SQL Analysis Security**:
  - Only read-only `SELECT` or `WITH` queries are permitted.
  - Stacked queries (containing `;`) and write or administrative statements (`DROP`, `INSERT`, `UPDATE`, `DELETE`, `ATTACH`, `PRAGMA`) are strictly rejected.
- **Cypher Runtime**:
  - `cypher` supports `MATCH (n) RETURN n`, `MATCH (n:kind) RETURN n`, and `MATCH (a)-[e]->(b) RETURN a,e,b`. Pass `limit` and `depth` as tool arguments, not inside the Cypher text.
- **Checkpoints**:
  - Stored in `<root>/.forge/checkpoints.json`.
  - Keys are limited to 1–200 alphanumeric/dash characters; path traversal attempts (e.g. `../../etc/passwd`) are rejected. Value content is limited to 1 MiB per checkpoint.

## Performance Targets

These are acceptance targets under release profile (`opt-level=3`, ThinLTO) with warm disk caches:

| Operation | Target |
| --- | --- |
| Cold indexing (~3,000 files) | < 15,000 ms |
| Single-file incremental delta (`audit.py`) | < 2,000 ms |
| MCP query latency (95th percentile) | < 10 ms |
| Native process startup | < 5 ms |
| Active stdio resident memory | < 30 MB |

Benchmark measurements on active ContextUnity source trees are documented in [docs/measurements/security-audit-speed.md](measurements/security-audit-speed.md).
