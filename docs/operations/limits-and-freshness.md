---
doc_type: guide
title: Limits and index freshness
---

# Limits and index freshness

Forge bounds file admission, graph traversal, queries, and tool responses. A rejected request returns an error with the relevant limit; narrow the scope or page through results.

## Current limits

| Area | Limit | How to proceed |
| --- | --- | --- |
| Scan inventory | 100,000 files; 5 MiB per file; 500 MiB total | Reduce configured roots or ignore generated files. |
| MCP page size | Default 30; range 1–100 | Use `offset` or the returned continuation. |
| MCP response bytes | Configurable 1,024–65,536; default 65,536 | Request fewer rows or less detail. |
| Graph depth | At most 16 | Start at depth 1 and expand only where needed. |
| Deep graph frontier | More than 1,000 immediate links blocks depth above 1 | Narrow the selector or retry with depth 1. |
| Removal scope | More than 10,000 nodes is rejected | Analyze a smaller file, package, or symbol. |
| Direct test links | More than 1,000 links is rejected | Narrow the selected scope. |
| AST search | `offset + limit` at most 10,000 matches; 1,000,000 steps and 2 seconds per file; 5-second MCP request budget | Constrain the path and pattern. |
| SQL analysis | 2-second query progress budget; 8 MiB converted result budget | Page results and limit selected columns. |
| Cycle analysis | At most 1,000,000 selected nodes for a path target; 500,000 edges across the graph | Omit cycles if the graph exceeds the edge budget. |
| Checkpoint value | 1 MiB | Save a smaller JSON value. |

These are protective ceilings, not latency promises. A query may finish earlier, return a partial page, or fail on a narrower budget. The configured response limit is independent of the index size.

## Keeping the index current

The default database is `<root>/.forge/code-map.sqlite`; override it with `--db` or `FORGE_DB`. The default workspace is `.`; use `--root` or `FORGE_WORKSPACE_ROOT` for another root. `build` creates a full index. `delta` updates changed and deleted files. The MCP server validates workspace identity and adapter configuration on each request, then updates or rebuilds stale data. It reuses a successful source-inventory check for up to two seconds when the adapter digest is unchanged, so an external source edit can remain unreflected during that interval. CLI read commands use the existing index, so refresh it with `build` or `delta` after source edits.

If a source file has changed since it was indexed, `get_code_snippet` rejects stale content by digest. Refresh the index and retry. Response settings such as page size and source preview length do not require a rebuild; changes to admitted roots, linked workspaces, or enabled language profiles do.

## Query and source safety

`code_map_analyze` accepts one read-only `SELECT` or `WITH` statement and rejects write and administrative SQL. It runs against a read-only connection. Set `include_cycles=true` only when cycle findings are needed; cycle analysis has separate graph limits. A structural result is evidence from indexed source, not a guarantee about runtime behavior.

Source bodies are omitted from MCP responses by default. Opt in through the configured source preview settings or request `get_code_snippet`. `leading_lines` ranges from 0 to 20 and `max_body_lines` from 1 to 100. Pagination and response byte limits still apply. See [MCP tools](../reference/mcp-tools.md) for the request fields.

Local checkpoints live in `.forge/checkpoints.json`. A name must be 1–200 UTF-8 bytes and cannot contain `..`, `/`, `\`, or a NUL byte. `save` requires valid JSON content; `get` and `delete` report missing names as errors.
