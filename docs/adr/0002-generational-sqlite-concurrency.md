---
title: "ADR 0002: SQLite Snapshot Admission and Reader Isolation"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0002: SQLite Snapshot Admission and Reader Isolation

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
2. **Snapshot admission and cross-process fencing**:
   - The server reuses one reader connection behind a mutex and checks the canonical workspace, adapter digest and roots, plus the main database and non-empty WAL file identity on each admission.
   - A shared filesystem lock fences a read transaction against a rebuild or delta writer holding the matching exclusive lock.
   - After the query, the server checks database identity while the read transaction is still active, then ends the transaction. A changed or unverifiable snapshot invalidates the cached connection and returns an error.
3. **Verified snapshot receipt**:
   - The private receipt binds the canonical workspace and database paths, the Merkle output root, and the recorded database file identity. The graph database does not use a `meta` generation counter or event-only cache invalidation.

## Consequences
- Reads within one server instance are serialized by its connection mutex. Cross-process writers and readers coordinate through shared and exclusive filesystem locks.
- Public MCP requests load one adapter snapshot before admission and reuse it for response policy and enforcement. Direct `Server::read` calls load their own adapter. Database identity checks occur on every admission; the inventory freshness TTL avoids repeating the full workspace scan while the cached snapshot remains valid.
- The SQLite busy timeout is a lock-wait safety setting, not a latency guarantee.
