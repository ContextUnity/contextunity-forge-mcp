---
id: m-mcp-output-compaction-and-agent-ergonomics
title: "MCP output compaction, token density, and agent ergonomics"
doc_type: contract
status: planned
depends_on:
  - m-tool-performance-and-storage-compaction:completed
owners:
  - src/mcp/
  - src/db/
  - src/core/
  - tests/
  - benchmarks/
invariants:
  - "INV-TOKEN-DENSITY: Compact MCP responses omit default and zero-value fields (is_test: 0, generated: 0, null continuation values), reducing JSON token payload by >= 35%."
  - "INV-ACTIONABLE-SELECTORS: Search and inspect tools supply unambiguous, ready-to-call selector hints for follow-up navigation."
  - "INV-IDEMPOTENT-PAGING: Paging envelope compaction preserves deterministic continuation semantics and 8-character generation validation."
  - "INV-SPARSE-RELATIONS: Structural tools (inspect, explain) omit empty relation arrays, surfacing only populated graph edges."
---

# MCP output compaction, token density, and agent ergonomics

## Outcome and purpose

Maximize LLM context window efficiency, eliminate repetitive token waste, and optimize agent decision speed across all 15 Forge MCP tools. By pruning default/zero-value attributes (`is_test: 0`, `generated: 0`, redundant `qualname == name`, `null` continuation fields), collapsing trivial `freshness` telemetry for synchronized indexes, stripping empty relation arrays in `code_map_explain`, and injecting copy-pasteable navigation hints (`inspect_hint`, enclosing scope context in snippets), agents achieve faster reasoning cycles and consume >= 35% fewer tokens per code-graph investigation turn.

---

## Tasks in this milestone

### task: zero-value-and-default-field-omission

```yaml
task_ref: zero-value-and-default-field-omission
target: "Усунути дефолтні та null-поля (is_test: 0, generated: 0, redundant qualname, null continuation) з виводу пошуку та пагінації"
proof_policy: seam-test-first
scope:
  - src/db/paging.rs
  - src/db/symbols.rs
  - src/mcp/response.rs
  - tests/
status: planned
```

1. **Paging envelope pruning**:
   - In `src/db/paging.rs::value()`, omit `continuation_hint` and `next_offset` when `has_more == false` (e.g. serialize only when non-null or use a dedicated lean paging struct).
2. **Symbol node compaction**:
   - In `src/db/symbols.rs`, suppress `is_test: 0` and `generated: 0`. Only emit `"is_test": true` or `"generated": true` when non-zero.
   - When `qualname == name` (top-level classes, functions, modules), omit `qualname` from compact projections to prevent duplicate string emissions.
3. **Target verification**:
   - Prove token reduction on 30-item symbol pages via unit tests in `tests/mcp_context.rs`.

---

### task: lean-freshness-and-metadata-compaction

```yaml
task_ref: lean-freshness-and-metadata-compaction
target: "Компактизувати envelope freshness для синхронізованих станів та усунути діагностичний шум"
proof_policy: seam-test-first
scope:
  - src/mcp/metadata.rs
  - src/mcp/server.rs
  - src/mcp/response.rs
  - tests/
status: planned
```

1. **Minimalist synchronized freshness**:
   - When index status is `source_inventory_matched` and `refresh == "none"`, emit a lean freshness footprint:
     ```json
     {"status": "matched", "generation": "b484bce6"}
     ```
   - Omit `files_checked: 4280` and `refresh: "none"` unless explicitly requested via an options flag or when actual reindexing occurred (`refresh != "none"` or `status != "matched"`).
2. **Top-level generation consolidation**:
   - Ensure `generation` is emitted once deterministically at the end of the payload without redundant repetition across multiple sibling sub-envelopes when not paginating.

---

### task: sparse-relations-and-empty-aspect-pruning

```yaml
task_ref: sparse-relations-and-empty-aspect-pruning
target: "Прибирати порожні масиви зв'язків у code_map_explain та порожні секції документів у code_map_inspect"
proof_policy: seam-test-first
scope:
  - src/db/traversal.rs
  - src/db/reader.rs
  - src/mcp/tools.rs
  - tests/
status: planned
```

1. **Sparse explain output**:
   - In `code_map_explain`, evaluate candidate relation collections (`calls`, `callers`, `implements`, `implementors`, `overrides`, `dependencies`, `unwired`).
   - If a relation category has 0 items, omit the entire key from the response instead of outputting `{"items": [], "total": 0, "offset": 0, ...}`.
   - Provide a top-level summary array of available non-empty relation kinds (e.g. `"relations": ["calls", "callers"]`).
2. **Lean inspect output**:
   - In `code_map_inspect`, omit `documents: {"total": 0}` if `show_doc == false` or no linked architectural invariant sections exist.
   - Omit empty `coverage` objects when no coverage issues exist for the target scope.

---

### task: actionable-agent-navigation-and-entrypoints

```yaml
task_ref: actionable-agent-navigation-and-entrypoints
target: "Додати готові селектори переходів у пошук та карту основних точок входу (entrypoints) в overview"
proof_policy: seam-test-first
scope:
  - src/db/reader.rs
  - src/db/symbols.rs
  - src/mcp/tools.rs
  - tests/
status: planned
```

1. **Canonical selector hints**:
   - In `code_map_search` results, include a lightweight `inspect_selector` attribute providing the exact string needed for `code_map_inspect` (e.g. `Server.admit` or `fn:src/mcp/server.rs:357:admit`).
2. **Ambiguous selector resolution guidance**:
   - When a selector fails with ambiguity, format the error message with direct, copy-pasteable tool call examples for each candidate.
3. **Workspace entrypoints aspect**:
   - In `code_map_overview`, introduce an optional aspect `"entrypoints"` that discovers and highlights primary application roots (e.g., CLI `main`, server entrypoints, exported FastMCP tool definitions, FastAPI routes).

---

### task: snippet-enclosing-context-and-source-ergonomics

```yaml
task_ref: snippet-enclosing-context-and-source-ergonomics
target: "Збагатити get_code_snippet заголовком контексту охоплюючого символу та 1-індексованими координатами"
proof_policy: seam-test-first
scope:
  - src/db/symbols.rs
  - src/mcp/tools.rs
  - tests/
status: planned
```

1. **Scope breadcrumb header**:
   - In `get_code_snippet`, include an enclosing scope banner (e.g. `enclosing_symbol: "impl Server"` or `header: "struct Server in src/mcp/server.rs"`) so LLMs immediately understand the structural context of the extracted block without a preceding `inspect` call.
2. **Compact coordinate formatting**:
   - Represent contiguous line spans as `path:start_line-end_line` (e.g. `src/mcp/server.rs:357-385`) alongside standard offset tokens.
