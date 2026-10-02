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

> [!IMPORTANT]
> Invariant: Every capability exposed through the MCP JSON-RPC protocol maintains
> equivalent CLI command coverage under `contextunity-forge-mcp`. Both surfaces
> share the same underlying storage, analysis, and execution engines (see [ADR 0001](../adr/0001-dual-surface-parity.md)).

- `src/cli/` owns command parsing, build commands, and command output.
- `src/mcp/` owns the 15 tool schemas, request validation, freshness checks, and bounded responses.
- `src/engine/scanner.rs` owns admitted file inventory, roots, ignores, linked workspaces, and scan limits.
- `src/engine/languages/` and `src/engine/ast/` extract syntax facts for enabled language profiles. A `LanguageProfile` owns parsing and import normalization; a `LanguageLinker` owns language-specific import, receiver, and incremental-linking rules. The shared linker builds namespace indexes and graph edges through those contracts. `src/engine/docs.rs` extracts Markdown documents and heading sections.
- The linker connects facts using indexed files and configured workspace boundaries. An edge may remain unresolved or external when its target cannot be established statically.
- `src/db/` stores generations, nodes, edges, documents, and source metadata; its readers implement selectors, traversal, search, and analysis. Local syntax facts use Zstandard-compressed JSON, reverse dependency lookups use the target index on `dependencies`, and repeated resolution evidence uses a shared dictionary.

A root workspace owns the index under its `.forge` directory by default. Linked workspaces add files to that same index when enabled and available. Each linked workspace has its own roots and ignore settings. A source root is an inventory boundary; it does not by itself declare an import prefix. Use the actual package and module structure when interpreting imports.

`build` creates a full index. `delta` updates it from changed and deleted files. `scan` reports admitted files without creating an index. The MCP server checks indexed ownership, source inventory, adapter configuration, and file changes before serving database-backed requests. When the indexed state is stale, it updates affected files or rebuilds as needed. CLI readers use the index already on disk; run `build` or `delta` after editing source before relying on CLI results.

A full build validates a staged database before publication. Forge readers hold a shared generation lock during each query; the builder takes an exclusive lock before replacing the index. The MCP server reuses its SQLite connection between requests, starts a read transaction for each request, and discards the connection when the database identity changes. It reuses a successful source-inventory check for up to two seconds when the adapter digest is unchanged. On Linux, the builder checks for open handles from older processes immediately before replacement and rejects detected ones.

## Resolving a query

`code_map_search` ranks exact names and direct name prefixes first, exact qualified names next, and natural-language terms with SQLite FTS5 BM25. Graph-connected nodes and module exports receive a ranking boost. Markdown nodes are excluded unless `include_docs` is true; `group_by_file` groups the returned page by path. `code_map_inspect` resolves a selector to one node and its immediate evidence. `code_map_explain`, `code_map_impact`, and related tools traverse indexed edges with depth and size limits. `code_map_tests` bounds graph traversal to four steps and uses indexed test-file names when inbound graph links produce no tests. `ast_grep_search` reads indexed paths, uses FTS terms to narrow candidate files, then verifies each source digest before parsing. `search_docs` and `get_doc` read indexed Markdown, including sections extracted from headings. `code_map_analyze` executes a bounded read-only query over the index.

Selectors must identify one target. For example, `package/settings.py:load_config` names a function in a file, while `package/settings.py#L12` resolves the narrowest indexed symbol at that line. If a bare name matches more than one target, the resolver reports ambiguity and requires a path or exact symbol identifier. See the [MCP tool reference](../reference/mcp-tools.md) for selector syntax and response bounds.

## Extending a language profile

Language extraction belongs to its profile and the shared AST layer. Add a profile through the language registry and compile-time feature configuration, then add a `LanguageLinker` only when that language needs import, receiver, or incremental-linking behavior beyond the generic implementation. Python package initializers, paired declaration files, re-exports, and external import aliases live in the Python linker. Verify facts against parser fixtures and linker tests. A profile does not change the selector or database contracts. Keep unresolved references visible instead of guessing a target from a name alone.

## Static value flow

Language profiles produce typed value-flow facts through `LanguageProfile::value_flow`. The shared schema in `src/core/semantic.rs` records source positions, assignments, annotations, fields, aliases, and return contracts. The internal `TypedFacts` envelope stores these facts separately from dynamic node details. The linker borrows them directly. Serialization views preserve the public `details.value_flow` representation, property order, compressed local facts, and commitment bytes. Legacy hydration retains malformed metadata and original encoding. Embedded JavaScript uses the same typed AST extraction contract with source offsets and explicit owners.

`src/engine/linker/value_flow.rs` reduces facts against canonical import and symbol admission in `semantic_context.rs`. It builds receiver and computed-call caches before parallel reference linking. Language member indexes supply Python inheritance, Rust inherent and visible trait implementations, and TypeScript class/interface inheritance. Inferred edges always point to actual indexed declarations; external evidence retains its import provenance.

Assignments take effect after their right-hand expression completes. Unknown writes and ambiguous declarations block stale proofs. Explicit return annotations take precedence over bounded summaries of complete straight-line returns. Inference records dependencies on return, binding, and field summaries and reevaluates expressions when those summaries change, preserving owner order and the bounded iteration count. Alias cycles, unsupported dynamic values, and exhausted inference limits retain uncertainty. Reference linking uses cached receivers; it does not parse JSON or consult disk or SQL for each method call.

The internal graph uses static tags for known edge kinds, confidence values, and coverage statuses, with owned strings for arbitrary values. Public graph adapters retain the existing string fields. SQLite persistence uses bounded multi-value batches, shared key ownership, and reusable search buffers; serialization and graph ordering use the same string values across both representations.

Canonical HTML classic scopes preserve their empty `bindings` and `fields` JSON slots as null and decode those slots as empty sections on both public and cached paths. Other scopes retain strict value-flow decoding. Coverage commitment leaves preload dictionaries and stream borrowed canonical rows; noncanonical SQLite storage retains the generic encoding path. Partial sealing loads dictionaries for the selected owners.

Incremental dependency keys include value-flow types, constructors, aliases, return expressions, computed-call hints, and export facts. A changed provider expands the consumer frontier before the language linker hydrates required facts. Full and incremental indexing share the same reducer and symbol admission rules.

Body edits refresh the changed owner's facts, call edges, coverage, and commitments. Consumer invalidation compares semantic return and field contracts, signatures, exports, and import identities rather than body-local positions. Changes to inferred factory types, scoped imports, or inference validity limits expand the consumer frontier; equivalent internal call changes retain the existing consumer links.

`FORGE_PROFILE_LINKER=1` writes phase timings to stderr for namespace/export indexing, inheritance, semantic admission, value flow, reference linking, and graph sorting. It leaves graph facts and commitments unchanged.
