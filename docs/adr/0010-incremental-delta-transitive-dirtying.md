---
title: "ADR 0010: Incremental Delta Invalidation and Transitive Export Dirtying"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0010: Incremental Delta Invalidation and Transitive Export Dirtying

## Status
Accepted

## Context
Re-indexing the entire repository after a single-file edit repeats work and can leave cross-file symbol references and caller edges stale when updates are scoped too narrowly.

## Decision
1. **Scoped Invalidation Perimeter**:
   - When a set of files changes, the delta pipeline identifies:
     - The directly modified files.
     - Files importing changed public symbols or module exports.
     - Reverse dependencies affected by signature or export changes.
2. **Selective Relinking**:
   - Re-parse and extract AST facts only for files within the invalidated perimeter.
   - Retain cached nodes, edges, facts, and leaf digests for unaffected files.
3. **Atomic delta publication and snapshot admission**:
   - Delta updates apply within a SQLite transaction and publish the updated graph and Merkle output root as one committed snapshot. MCP readers validate the database file identity during admission and again after a query; changed snapshots invalidate the cached reader.

## Consequences
- Cross-file symbol resolution remains consistent with a fresh cold build.
- MCP readers use the newly published facts after their snapshot admission observes the updated database identity.
