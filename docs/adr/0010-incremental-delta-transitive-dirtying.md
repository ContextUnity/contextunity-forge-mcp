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
Full cold builds on large codebases take several seconds. When an editor or agent edits a single file, re-indexing the entire repository wastes CPU cycles and introduces latency. Naive single-file updates leave cross-file symbol references and caller edges stale.

## Decision
1. **Scoped Invalidation Perimeter**:
   - When a set of files changes, the delta pipeline identifies:
     - The directly modified files.
     - Files importing changed public symbols or module exports.
     - Reverse dependencies affected by signature or export changes.
2. **Selective Relinking**:
   - Re-parse and extract AST facts only for files within the invalidated perimeter.
   - Retain cached nodes, edges, facts, and leaf digests for unaffected files.
3. **Transaction and Generation Advancement**:
   - Delta updates apply within a single SQLite transaction, updating modified rows and incrementing the database generation counter atomically.

## Consequences
- Sub-50ms delta update times for typical single-file edits.
- Cross-file symbol resolution remains consistent with a fresh cold build.
- MCP readers immediately observe updated facts upon generation advancement.
