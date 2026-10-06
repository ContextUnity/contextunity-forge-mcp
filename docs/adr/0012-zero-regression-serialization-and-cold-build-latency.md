---
title: "ADR 0012: Lean Serialization and Measured Performance"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0012: Lean Serialization and Measured Performance

## Status

Accepted

## Context

Interprocedural value flow, receiver inference, and overload resolution add temporary analysis data that is useful during indexing but does not belong in the persistent navigation projection. Uncompressed AST facts, repeated source reads, row-at-a-time writes, and broad query work can increase build time, storage, and MCP latency. Architecture should preserve complete analysis facts while keeping durable projections compact and performance work evidence-led.

## Decision

1. **Lean node projection**: `nodes.details` contains fields needed for navigation and direct queries. Do not persist complete value-flow graphs, raw scope maps, or other temporary compiler state there. Keep complete durable file facts in the compressed fact store.
2. **Compressed facts**: Store `local_facts.facts_blob` using Zstandard and preserve decode, delta hydration, and commitment semantics. Density and compression ratios are measured on representative workspaces; numeric targets are recommendations except where an active milestone explicitly admits them.
3. **Bulk SQLite ingestion**: Isolated cold-build candidates may use non-syncing memory-journal pragmas during import. Validate and publish the finished candidate through the existing integrity, WAL, and atomic replacement flow.
4. **Batch and stream high-cardinality writes**: Use bounded multi-value batches, flush remaining buffers, stream canonical bytes into incremental hashers, and resolve commitment dictionary IDs through preloaded maps. Avoid ad-hoc per-row allocations and broad joins on commitment paths.
5. **Target-scoped interactive work**: Localized tools evaluate indexed target candidates and bounded dependency subgraphs. Do not use unindexed wildcard scans, workspace-wide counts, or global diagnostics to answer symbol-level questions.
6. **Avoid redundant source I/O and duplication**: Read each source file during extraction and carry the resulting facts through persistence. Do not duplicate full, uncompressed source text into SQLite or an FTS table. Add token projections only when search completeness, storage impact, and query cost are measured.
7. **Bound expensive ranking work**: Apply graph-connectivity scoring only to a bounded candidate window. Keep candidate filters and paging semantics in the same query contract.
8. **Measure before optimizing**: Profile distinct extraction, linking, persistence, indexing, sealing, and verification phases. Report nested and aggregate timings accurately; `rows_ms` must not be described as node-only timing unless its implementation measures only node writes. Use representative cold and warm MCP runs and retain host context for noisy measurements.
9. **Recommended values**: For planning, around 400 files/sec cold-build throughput, 800 files/sec extraction throughput, 120 bytes/node details, 45 KiB/file database density, and 30 ms full-text search are useful starting points. A cold build around 10 seconds on the Commerce reference repository is preferred. These are guidance rather than repository-wide gates; milestone 030 owns its explicit numeric acceptance criteria.

## Consequences

- Persistent navigation data stays separate from full analysis facts.
- Batch persistence, dictionary-backed commitments, and target-scoped query plans remain the architectural direction.
- Performance comparisons identify their corpus, command, source state, host context, and phase accounting so receipts can be interpreted accurately.
- Numeric performance expectations remain flexible outside an explicitly scoped milestone, allowing measurement to guide architecture without creating contradictory global gates.
