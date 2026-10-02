---
title: "Architecture index"
doc_type: architecture
---

# Architecture

The architecture documentation describes verified system structure, contracts, and performance envelopes:

- [Indexing and resolution](indexing.md): Scanner, AST fact extraction, symbol linking, and storage boundaries.
- [SQLite concurrency and isolation](concurrency.md): Reader isolation, generation locking, RAII guards, and WAL mode pragmas.
- [Engine modularity and traits](modularity.md): `LanguageProfile`, `TemplatePreprocessor`, and `LanguageLinker` extensibility contracts.
- [Performance budgets and compaction](performance.md): Latency budgets, FTS5 BM25 hybrid search, inventory debouncing, and Zstd storage compaction.

Related architectural decisions are recorded in [Architectural Decisions](../adr/README.md).
