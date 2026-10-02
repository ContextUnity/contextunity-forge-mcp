---
title: "SQLite Concurrency, Generation Locking, and Reader Isolation"
doc_type: architecture
---

# SQLite Concurrency, Generation Locking, and Reader Isolation

## Concurrency model

Forge MCP operates as an embedded Rust binary executing concurrent reads from MCP JSON-RPC handlers alongside background cold builds and incremental delta indexing.

### SQLite Pragmas & WAL Mode

Storage connections are initialized with:
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 268435456;
PRAGMA busy_timeout = 5000;
```

Write transactions (`BEGIN IMMEDIATE`) are isolated to `src/db/writer.rs`. Readers open shared, read-only connections with separate transaction slots.

## Generation Locking

To prevent reading a database while an active write transaction is partially applied or while the file is being rebuilt:

1. **Generation Counter**: A 64-bit monotonically increasing generation counter is stored in the `meta` table and memory cache.
2. **Reader Locks**: Readers in `src/mcp/server.rs` acquire an RAII read guard against the active generation. If the generation changes during traversal, the reader invalidates its cached statements and reopens the connection to the new generation.
3. **No Process Scans**: Cache invalidation avoids scanning `/proc/*/fd` or inspecting system file descriptors. Invalidation is purely event-driven and generation-checked.
