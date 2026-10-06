---
title: "SQLite Snapshot Admission, Locking, and Reader Isolation"
doc_type: architecture
---

# SQLite Snapshot Admission, Locking, and Reader Isolation

## Concurrency model

Forge MCP accepts concurrent MCP JSON-RPC requests while background cold builds and incremental delta indexing may run in other processes.

### SQLite Pragmas & WAL Mode

Storage connections are initialized with:
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 268435456;
PRAGMA busy_timeout = 5000;
```

Graph write transactions (`BEGIN IMMEDIATE`) are owned by `src/db/writer.rs` and `src/db/delta.rs`; task-state writes use their separate `src/db/tasks_store.rs` transaction boundary. In the current MCP server, one reusable SQLite reader connection is guarded by a mutex, so each read callback holds that slot through its query transaction and reads within one server instance are serialized. The current-thread Tokio runtime also runs synchronous tool handlers serially. A reader pool alone would not make public MCP requests execute concurrently.

## Snapshot admission and reader fencing

The MCP server keeps one reusable SQLite reader connection behind a mutex. `call_tool` loads one adapter snapshot for the request's response policy, admission, and final response enforcement. `Server::admit` consumes that snapshot, compares the cached connection's workspace and adapter identity, and checks the database file identity. It rereads the adapter only while validating a source scan or retrying admission after a rebuild. The identity includes the main database and any non-empty WAL file. The server reuses an inventory freshness result only within its TTL; database identity checks still happen on every query.

Before executing a query, the server acquires a shared filesystem lock for the database snapshot. Builders and delta writers use the matching exclusive lock. After the query, it checks database identity while the read transaction is still active, then rolls back; if the identity changed or could not be checked, it discards the connection and asks the caller to retry. A private verified receipt binds the canonical workspace and database paths, Merkle output root, and file identity. The graph database does not use a `meta` generation counter or event-only invalidation.

A bounded read-only pool requires a matching change to request execution and proof of a material concurrent benefit without a single-request or startup regression. Milestone 030 retains the serialized reader on the measured Commerce workload.
