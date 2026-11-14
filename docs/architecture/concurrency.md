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

Graph write transactions (`BEGIN IMMEDIATE`) are owned by `crates/forge-index/src/db/writer.rs` and `crates/forge-index/src/db/delta.rs`; task-state writes use the separate `crates/forge-tasks` storage boundary. Task orchestration lives in `crates/forge-tasks/src/engine/`. The MCP surface registers 20 tools. A mutex guards the reader slot through each query transaction, serializing reads within one server instance. The current-thread Tokio runtime also runs synchronous tool handlers serially. A reader pool alone would not make public MCP requests execute concurrently.

## Snapshot admission and reader fencing

The MCP server keeps a reusable SQLite reader behind a mutex. `call_tool` loads one adapter snapshot for the request's response policy, admission, and final response enforcement. `Server::admit` consumes that snapshot, compares the cached slot's workspace and adapter identity, and checks the database file identity. It rereads the adapter only while validating a source scan or retrying admission after a rebuild. The identity includes the main database and any non-empty WAL file. The server reuses an inventory freshness result only within its TTL; database identity checks still happen on every query. Writers release idle in-process readers under the exclusive generation lock before publication.

Before executing a query, the server acquires a shared filesystem lock for the database snapshot. Builders and delta writers use the matching exclusive lock. After the query, it checks database identity while the read transaction is still active, then rolls back; if the identity changed or could not be checked, it discards the connection and asks the caller to retry. A private verified receipt binds the canonical workspace and database paths, Merkle output root, and file identity. The graph database does not use a `meta` generation counter or event-only invalidation.

A bounded read-only pool requires a matching change to request execution and proof of a material concurrent benefit without a single-request or startup regression. Milestone 030 retains the serialized reader on the measured Commerce workload.
