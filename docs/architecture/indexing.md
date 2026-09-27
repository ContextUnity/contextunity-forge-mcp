---
doc_type: architecture
title: Indexing architecture
---

# Indexing architecture

Forge builds a local SQLite graph from admitted source and Markdown. The CLI creates or updates the index; the MCP server checks freshness before reads. Readers resolve selectors and return bounded graph or document results.

```text
CLI build / scan / delta             MCP request
          |                               |
          +---- scanner and adapter ------+
                       |
             language and Markdown extraction
                       |
                    linker
                       |
                SQLite index
                       |
             CLI and MCP readers
```

## Ownership and data flow

- `src/cli/` owns command parsing, build commands, and command output.
- `src/mcp/` owns the 15 tool schemas, request validation, freshness checks, and bounded responses.
- `src/engine/scanner.rs` owns admitted file inventory, roots, ignores, linked workspaces, and scan limits.
- `src/engine/languages/` and `src/engine/ast/` extract syntax facts for enabled language profiles. `src/engine/docs.rs` extracts Markdown documents and heading sections.
- The linker connects facts using the indexed files and configured workspace boundaries. An edge may remain unresolved or external when its target cannot be established statically.
- `src/db/` stores generations, nodes, edges, documents, and source metadata; its readers implement selectors, traversal, search, and analysis.

A root workspace owns the index under its `.forge` directory by default. Linked workspaces add files to that same index when enabled and available. Each linked workspace has its own roots and ignore settings. A source root is an inventory boundary; it does not by itself declare an import prefix. Use the actual package and module structure when interpreting imports.

`build` creates a full index. `delta` updates it from changed and deleted files. `scan` reports admitted files without creating an index. The MCP server checks indexed ownership, source inventory, adapter configuration, and file changes before serving database-backed requests. When the indexed state is stale, it updates affected files or rebuilds as needed. CLI readers use the index already on disk; run `build` or `delta` after editing source before relying on CLI results.

## Resolving a query

`code_map_search` finds candidate symbols. `code_map_inspect` resolves a selector to one node and its immediate evidence. `code_map_explain`, `code_map_impact`, and related tools traverse indexed edges with depth and size limits. `search_docs` and `get_doc` read indexed Markdown, including sections extracted from headings. `code_map_analyze` executes a bounded read-only query over the index.

Selectors must identify one target. For example, `package/settings.py:load_config` names a function in a file, while `package/settings.py#L12` resolves the narrowest indexed symbol at that line. If a bare name matches more than one target, the resolver reports ambiguity and requires a path or exact symbol identifier. See the [MCP tool reference](../reference/mcp-tools.md) for selector syntax and response bounds.

## Extending a language profile

Language extraction belongs to its profile and the shared AST layer. Add a profile through the language registry and compile-time feature configuration, then verify its facts against parser fixtures and linker tests. A profile does not change the selector or database contracts. Keep unresolved references visible instead of guessing a target from a name alone.
