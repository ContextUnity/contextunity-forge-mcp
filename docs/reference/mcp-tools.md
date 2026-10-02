---
doc_type: api
title: MCP tool reference
---

# MCP tool reference

The server exposes 20 tools over standard input and output. Start with `code_map_overview` to identify the indexed workspace. Search for a symbol, inspect its exact identifier, then use graph tools at a narrow scope. Check the reported path and line in source before acting on a structural inference.

## Tools

| Tool | Purpose and main input |
| --- | --- |
| `code_map_overview` | Workspace, index, language and documentation summary. |
| `code_map_search` | Search symbols by `pattern`; exact names and direct prefixes rank first, exact qualified names follow, and natural-language terms use FTS5 BM25. Set `exact:true` for indexed, case-insensitive name or qualified-name equality without FTS. Supports `include_docs`, `group_by_file`, prefix patterns such as `parse*`, and optional file or directory `path` scope. |
| `code_map_inspect` | Resolve a `selector` and return a compact signature, docstring, receiver-aware container, call counts, and direct caller/callee preview. Set `include_coverage=true` for paged resolution evidence. |
| `get_code_snippet` | Read indexed source around a `selector` with `leading_lines` and `max_body_lines` bounds. |
| `code_map_explain` | Explain direct inbound or outbound relationships for a `selector`; set `direction` if needed. |
| `code_map_impact` | Find inbound (default) or outbound dependencies for a `selector`; set `depth`. Each reached node identifies one shortest predecessor edge. |
| `code_map_tests` | Find test relationships for a `selector`; `direction` accepts `inbound` or `outbound`. Results distinguish graph evidence from the bounded lexical fallback for inbound searches. |
| `code_map_prove_removal` | Estimate what a `selector` removal could affect; structural evidence, not a proof of runtime safety. |
| `code_map_query` | Route an `operation` to graph queries or paged read-only SQL. |
| `code_map_analyze` | Read workspace or path diagnostics, run read-only SQL, or request stored syntax diagnostics with `lint:true`; `include_cycles` opts into cycle analysis. |
| `ast_grep_search` | Match a language-specific AST `pattern` in admitted source. Indexed file lists and FTS terms narrow candidate files before Tree-sitter parses their contents. |
| `search_docs` | Search indexed Markdown by `query` and optional `doc_type`; `include_excerpt=true` adds a bounded match excerpt. |
| `get_doc` | Retrieve a Markdown document by `path_or_id` and optionally a heading `section`. |
| `session_checkpoint` | `list` checkpoint names and serialized byte sizes; `save`, `get`, or `delete` a named local JSON checkpoint. |
| `forge_guide` | Return the built-in usage guide. |
| `task_list` | List ready tasks in the primary repository by default; select repository/all, status, milestone, or stage explicitly. |
| `task_claim` | Atomically claim the current task gate for a worker and worktree. |
| `task_submit` | Submit a revision-bound JSON `evidence` object. Passing `deliver/v1` writes the validated task receipt and context rollup. |
| `task_manage` | Create, sync, inspect, delete, or extend task scope; workspace selects linked specification sync. |
| `task_blackboard` | Post or read task-scoped SQLite messages. `post` requires `task_id`, `topic`, and `payload`, accepts optional `author`, and returns `{id}`; `read` accepts optional `topic` and `limit`, and returns `{messages}`. |

See [repository tasks](tasks.md) for schemas, receipts, configuration, and CLI parity, and [ACDD](acdd.md) for the task lifecycle.

## Selectors and graph queries

Use a symbol identifier returned by search whenever possible. A file path selects a module or file. `path:symbol` and `path::symbol` select a symbol in that path; `path:12` and `path#L12` select the narrowest symbol covering line 12. Relative paths may use `./`, `file:`, or `file://` prefixes. A bare name can be ambiguous when a module and symbol share that name; specify the path. Markdown belongs in `get_doc` and `search_docs`.

`code_map_query` supports `overview`, `inspect`, `explain`, `impact`, `slice`, `unwired`, and `sql`. Compact `inspect` and `explain` operations return the same symbol summary as their dedicated tools; set `include_coverage=true` to add resolution coverage. For `operation="sql"`, put exactly one read-only `SELECT` or `WITH` statement in `selector`; the same secure SQL validation, query budget, and paged `QueryOptions` used by `code_map_analyze` apply. Pass `limit`, `offset`, and `generation` as tool arguments. Impact direction defaults to `inbound`; `outbound` traverses dependencies used by the selected symbol. See [CLI reference](cli.md) for equivalent command forms.

Depth is limited to 16. A deep traversal whose first frontier exceeds 1,000 links is rejected; retry with depth 1 or a narrower selector. Removal analysis rejects scopes above 10,000 nodes. Graph results describe indexed, statically resolved relationships.

`code_map_search.path` accepts an exact indexed file or a directory whose descendants should be searched. `code_map_impact` and node-scoped `code_map_query` operations `impact` and `slice` mark the selected node with `is_seed=true`; at depth greater than one, other nodes include one `via` predecessor ID and edge kind along a shortest indexed path. Page totals and offsets include the seed. A directory selector in `code_map_query(operation="slice")` lists its indexed nodes with `graph_depth_applied=0` and exact selectors for subsequent graph traversal. A unique directory suffix is accepted; ambiguous suffixes return candidate full paths. `code_map_tests` puts direct edge witnesses before `scope_or_transitive` results, which identify a relationship through a broader scope or multiple graph steps. Inbound searches cap graph walking at four steps and search test-file names when the graph returns none. `code_map_prove_removal.assessment` leads with a verdict and candidate-scoped blocking reasons; `safe_to_remove` is separate from workspace-wide unresolved and parse-error diagnostics.

`code_map_explain` accepts `incoming`, `outgoing`, or `both`; `inbound` and `outbound` are aliases. Compact inspect and explain results include the exact indexed signature and docstring, a lexical container resolved from method receiver metadata when available, inbound/outbound distinct-edge and occurrence counts, and up to five direct callers/callees per direction. Set `include_coverage=true` to add paged resolution coverage, or `show_doc=false` to omit linked document results.

For Python `from package import child`, a unique indexed child module contributes an import edge to that module and can resolve calls through its alias. A package can also expose a symbol through one explicit relative import in `__init__.py`; the graph links that import to the indexed declaration. When matching `.py` and `.pyi` modules exist, imports select the runtime `.py` module. If its indexed declarations omit a symbol, the matching type stub can supply that declaration; call evidence identifies the stub target. Other multiple child candidates remain ambiguous.

`code_map_analyze` groups unresolved references by recorded resolver cause and counts affected files. It also groups parse error records by language and affected files. A call through a proven external import has status `external` and contributes to `external_imports` instead of unresolved coverage. Use an exact indexed file as `target` to page individual locations; `detail="full"` includes the underlying resolver evidence.

Compact `code_map_explain` edge rows identify the other endpoint relative to the selected node; full detail includes both endpoints. `code_map_inspect` reports an empty linked-document set as `{ "total": 0 }`. `code_map_overview` provides the workspace root, schema version, counts, and page generation.

## Result size and source

Paged tools accept `limit` (default 30, range 1–100) and `offset`. Continue an ordinary page with its `next_offset` and `generation`; ordinary pages omit the repeated continuation text. A completed zero-result page contains `total` and `items` without pagination fields. Computation and byte limits retain their truncation status and recovery hint, including when no items were returned. The response byte budget is configurable from 1,024 to 65,536 bytes.

Code source previews are off by default. Request them explicitly, or call `get_code_snippet`. Snippets check that the on-disk source still matches the indexed digest. `leading_lines` ranges from 0 to 20; `max_body_lines` ranges from 1 to 100. If source changed, refresh the index before relying on the result.

## SQL and checkpoints

`code_map_analyze` accepts exactly one `SELECT` or `WITH` statement. The connection is read-only, and write or administrative SQL is rejected. It has a query time budget; large result sets should be paged. Cycle analysis is omitted unless `include_cycles=true`. Cycles are static graph findings.

A checkpoint saves a JSON value under a local name in `.forge/checkpoints.json`. `save` needs both a name and valid JSON content. `list` returns each name with its serialized JSON byte size; `get` loads the value. Names are 1–200 UTF-8 bytes and cannot contain `..`, `/`, `\`, or a NUL byte. Values are limited to 1 MiB. A checkpoint is local session data, not part of the source index.

## Syntax diagnostics on demand

Call `code_map_analyze` with `lint:true` and an indexed file or directory in `target`; an empty target covers indexed sources in the workspace. This reads the syntax diagnostics already stored by compiled language profiles. It does not run another parser, an external linter, or additional work during indexing. The usual source freshness check still applies to MCP reads.

```json
{"target":"src","lint":true,"limit":30}
```

Results include a rule ID, severity, language, path, line, and message, with pagination and generation checks. Coverage describes indexed sources and language-specific limits; excluded or unsupported files are not checked. An empty diagnostics page does not certify semantic correctness or compliance with style rules. Vue coverage is limited to its embedded scripts; HTML template expressions and styles are not validated.

Syntax lint mode does not execute SQL or cycle analysis. Use a separate request for those operations. Ruff, Clippy, application-specific rules, and automatic fixes are outside this mode.

## Documentation queries

Markdown and MDX in admitted roots are indexed as documents. Optional frontmatter `doc_type` and `title` help `search_docs`. `get_doc` addresses the document path and an exact heading section; a missing path or section is an error. Its default MCP response includes section content. Set `detail="compact"` explicitly for a section outline without content; `search_docs` keeps compact discovery results by default. Set `include_excerpt=true` to add a short excerpt centered on a search match.
