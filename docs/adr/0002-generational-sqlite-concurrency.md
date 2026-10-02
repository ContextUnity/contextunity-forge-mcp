---
title: "ADR 0002: Generational SQLite Concurrency and Reader Isolation"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0002: Generational SQLite Concurrency and Reader Isolation

## Status
Accepted

## Context
Forge MCP serves concurrent JSON-RPC tool calls from AI coding assistants while running background cold builds and incremental delta indexing. Database lock contention during writes and file descriptor scanning during cache invalidation degrade throughput and reader stability.

## Decision
1. **WAL Mode & Connection Pragmas**:
   - Storage connections initialize with:
     ```sql
     PRAGMA journal_mode = WAL;
     PRAGMA busy_timeout = 5000;
     PRAGMA synchronous = NORMAL;
     PRAGMA mmap_size = 268435456;
     ```
2. **Generational Locking**:
   - Maintain a 64-bit monotonically increasing generation counter in the `meta` table.
   - Readers acquire RAII read guards tracking the active generation.
   - When a delta or rebuild increments the generation counter, readers discard statement caches and re-bind connections to the new generation on the subsequent request.
3. **Internal Event-Driven Invalidation**:
   - Cache invalidation relies entirely on the internal generation counter, eliminating operating system process directory scans.

## Consequences
- Readers execute concurrently with write transactions without query lockouts.
- Cache validation overhead remains negligible (< 1 µs generation check).
- Multi-worktree concurrent operations queue smoothly up to 5 seconds during write contention.
