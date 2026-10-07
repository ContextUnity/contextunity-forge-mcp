---
title: "Architectural Decisions"
doc_type: guide
---

# Architectural Decisions

Accepted decisions live here as owner-approved ADR files with stable IDs, status, governing constraints, and links to affected contracts. Decisions are ordered by foundational architectural significance.

## Accepted Decisions

1. [ADR 0001: Dual-Surface Parity Between CLI and MCP](0001-dual-surface-parity.md)
2. [ADR 0002: SQLite Snapshot Admission and Reader Isolation](0002-generational-sqlite-concurrency.md)
3. [ADR 0003: Two-Phase Staged Indexing and Atomic Publication](0003-two-phase-staged-publication.md)
4. [ADR 0004: Deterministic Merkle Tree Commitments for Index Integrity](0004-deterministic-merkle-commitments.md)
5. [ADR 0005: Filesystem Root Guard and Symlink Traversal Safety](0005-filesystem-root-guard-and-symlink-safety.md)
6. [ADR 0006: Read-Only SQL Execution Sandbox and Injection Guards](0006-read-only-sql-execution-guards.md)
7. [ADR 0007: Fail-Closed Selector Disambiguation Pipeline](0007-fail-closed-selector-disambiguation.md)
8. [ADR 0008: Bounded Static Value Flow and Receiver Resolution](0008-bounded-static-value-flow.md)
9. [ADR 0009: Manifest-Driven External Origin Classification](0009-manifest-external-origin-classification.md)
10. [ADR 0010: Incremental Delta Invalidation and Transitive Export Dirtying](0010-incremental-delta-transitive-dirtying.md)
11. [ADR 0011: Bounded MCP Response Budgets and Continuation Paging](0011-bounded-mcp-response-budgets.md)
12. [ADR 0012: Lean Serialization and Measured Performance](0012-zero-regression-serialization-and-cold-build-latency.md)
13. [ADR 0013: Data-Driven Framework Manifests](0013-data-driven-framework-manifests.md)
14. [ADR 0014: Task Hierarchy, Visibility, and Blackboard Coordination](0014-milestone-task-subtask-hierarchy-and-visibility.md)

Decision amendments require explicit architectural admission. A superseding ADR links the accepted decision it replaces and the affected current architecture in [Architecture](../architecture/README.md).
