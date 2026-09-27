---
doc_type: api
title: MCP tool reference
---

# MCP tool reference

The server exposes 15 tools over standard input and output. Start with `code_map_overview` to identify the indexed workspace. Search for a symbol, inspect its exact identifier, then use graph tools at a narrow scope. Check the reported path and line in source before acting on a structural inference.

## Tools

| Tool | Purpose and main input |
| --- | --- |
| `code_map_overview` | Workspace, index, language and documentation summary. |
| `code_map_search` | Search code symbols by `pattern`; supports an FTS prefix such as `parse*`. |
| `code_map_inspect` | Resolve a `selector` and show its symbol, file, and relationships. |
| `get_code_snippet` | Read indexed source around a `selector` with `leading_lines` and `max_body_lines` bounds. |
| `code_map_explain` | Explain direct inbound or outbound relationships for a `selector`; set `direction` if needed. |
| `code_map_impact` | Find inbound users affected by a `selector`; set `depth`. |
| `code_map_tests` | Find test relationships for a `selector`; `direction` accepts `inbound` or `outbound`. |
| `code_map_prove_removal` | Estimate what a `selector` removal could affect; structural evidence, not a proof of runtime safety. |
| `code_map_query` | Route an `operation` to graph queries or the supported Cypher subset. |
| `code_map_analyze` | Read workspace or path diagnostics through `target`, or run one read-only SQL statement; `include_cycles` opts into cycle analysis. |
| `ast_grep_search` | Match a language-specific AST `pattern` in admitted source. |
| `search_docs` | Search indexed Markdown by `query` and optional `doc_type`. |
| `get_doc` | Retrieve a Markdown document by `path_or_id` and optionally a heading `section`. |
| `session_checkpoint` | `list`, `save`, `get`, or `delete` a named local JSON checkpoint. |
| `forge_guide` | Return the built-in usage guide. |

## Selectors and graph queries

Use a symbol identifier returned by search whenever possible. A file path selects a module or file. `path:symbol` and `path::symbol` select a symbol in that path; `path:12` and `path#L12` select the narrowest symbol covering line 12. Relative paths may use `./`, `file:`, or `file://` prefixes. A bare name can be ambiguous when a module and symbol share that name; specify the path. Markdown belongs in `get_doc` and `search_docs`.

`code_map_query` supports `overview`, `inspect`, `explain`, `impact`, `slice`, `unwired`, and `cypher`. It also accepts `raw_cypher`, `doctor`, and `search` as aliases. A direct `MATCH ... RETURN ...` expression uses the supported Cypher subset. Pass `limit` as a tool argument, not a Cypher `LIMIT` clause. See [CLI reference](cli.md) for equivalent command forms.

Depth is limited to 16. A deep traversal whose first frontier exceeds 1,000 links is rejected; retry with depth 1 or a narrower selector. Removal analysis rejects scopes above 10,000 nodes. Graph results describe indexed, statically resolved relationships.

## Result size and source

Paged tools accept `limit` (default 30, range 1–100) and `offset`. A response may be truncated by its byte budget even when more rows exist; use the returned continuation information to request the next page. A `generation` can pin pages to one index generation. The response byte budget is configurable from 1,024 to 65,536 bytes.

Source text is off by default. Request it explicitly, or call `get_code_snippet`. Snippets check that the on-disk source still matches the indexed digest. `leading_lines` ranges from 0 to 20; `max_body_lines` ranges from 1 to 100. If source changed, refresh the index before relying on the result.

## SQL and checkpoints

`code_map_analyze` accepts exactly one `SELECT` or `WITH` statement. The connection is read-only, and write or administrative SQL is rejected. It has a query time budget; large result sets should be paged. Cycle analysis is omitted unless `include_cycles=true`. Cycles are static graph findings.

A checkpoint saves a JSON value under a local name in `.forge/checkpoints.json`. `save` needs both a name and valid JSON content. Names are 1–200 UTF-8 bytes and cannot contain `..`, `/`, `\`, or a NUL byte. Values are limited to 1 MiB. A checkpoint is local session data, not part of the source index.

## Documentation queries

Markdown and MDX in admitted roots are indexed as documents. Optional frontmatter `doc_type` and `title` help `search_docs`. `get_doc` addresses the document path and an exact heading section; a missing path or section is an error.
