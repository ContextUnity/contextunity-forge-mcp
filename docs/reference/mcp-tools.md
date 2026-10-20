---
doc_type: api
title: MCP tool reference
---

# MCP tool reference

The server exposes 20 tools over standard input and output. Start with `code_map_overview` to identify the indexed workspace. Search for a symbol, inspect its exact identifier, then use graph tools at a narrow scope. Check the reported path and line in source before acting on a structural inference.

## Description layers

A client receives three texts for this server. Keep each fact in one layer.

| Layer | Source | Write |
| --- | --- | --- |
| Server instructions | `instructions` on the MCP server handler | Cross-tool budget and the first call: 64 KiB, 2 second SQLite budget, start at `code_map_overview`, continue a page with `offset` and `generation`, prefer `detail=compact`. |
| Tool description | `description` on each tool | What that call returns, plus one limit the schema cannot express. |
| Input schema | Description on each input field | Accepted values, defaults, and pairings such as nonzero `offset` with `generation`. |

Selector grammar, SQL shape, proof objects, and examples stay in this page and in `forge_guide`. Verify a changed sentence against the handler in `src/mcp/tools.rs` and the query implementation it calls. Connection and the first-call check stay in [MCP setup](mcp-setup.md).

## Tools

| Tool | Purpose and main input |
| --- | --- |
| `code_map_overview` | Workspace, index, language and documentation summary; request `coverage_by_workspace` for persisted primary and linked workspace coverage counts. |
| `code_map_search` | Search symbols by `pattern`; by default, if exact name or qualified-name matches exist within the active filters, only those matches are returned. Otherwise, prefixes and natural-language terms use ranked FTS5 BM25 exploratory search. Hits include an indexed `signature` and `inspect_selector` arguments for `code_map_inspect` with `show_source:true`. Previews appear automatically when the total is at most three; set `include_preview:true` for larger result sets. Supports `include_docs`, `group_by_file`, prefix patterns such as `parse*`, and optional file or directory `path` scope. |
| `code_map_inspect` | Resolve a `selector` and return a compact signature, docstring, receiver-aware container, call counts, and direct caller/callee preview. Paged `documented_in` links and bounded `governing_invariants` add verified documentation context. Set `include_coverage=true` for paged resolution evidence. |
| `get_code_snippet` | Read indexed source around a `selector` with `leading_lines` and `max_body_lines` bounds. |
| `code_map_explain` | Explain direct inbound or outbound relationships for a `selector`; edge rows include both public endpoint IDs and direction relative to the selected node. Full detail also includes stored edge evidence. |
| `code_map_impact` | Find inbound (default) or outbound dependencies for a `selector`, or analyze one Git change view (`staged`, `unstaged`, or `head`); provide exactly one of `selector` and `changes`. Selector results also page affected documentation and governing invariants. Depth defaults to one edge. |
| `code_map_tests` | Find test relationships for a narrow `selector`; `direction` accepts `inbound` or `outbound`. Inbound results identify verified test cases and shortest exact `calls` paths, with a bounded lexical fallback when no call path exists. Outbound results follow exact `calls` edges. Results classify test cases, helpers, and fixtures; `possible_calls` do not establish coverage. |
| `code_map_prove_removal` | Estimate what a `selector` removal could affect; structural evidence, not a proof of runtime safety. |
| `code_map_query` | Route an `operation` to graph queries or paged read-only SQL (`operation="sql"`). |
| `code_map_analyze` | Read workspace or path diagnostics and paged declared-public production symbols not reached from an indexed test case through exact `calls`; exact-file diagnostic rows include a source snippet and a reason when persisted evidence identifies one. Exact Markdown paths can also return paged `doc_diagnostics` for broken links, stale source ranges, and dead symbol claims. Request stored syntax diagnostics with `lint:true` or opt into cycle analysis with `include_cycles`. |
| `ast_grep_search` | Match a language-specific AST `pattern` in admitted source. Indexed file lists and FTS terms narrow candidate files before Tree-sitter parses their contents. |
| `search_docs` | Search indexed Markdown by `query` and optional `doc_type`; `include_excerpt=true` adds a bounded match excerpt. |
| `get_doc` | Retrieve a Markdown document by `path_or_id` and optionally a heading `section`; full responses include source locations, citation links, uniquely verified referenced symbols, and a paged `referenced_symbol_candidates` collection for ambiguous eligible claims. Unscoped claims match symbols in the source document's workspace; an explicit normalized file link can cross workspaces. Compact responses omit candidate details and return a hint to request `detail="full"`. |
| `session_checkpoint` | `list` checkpoint names and serialized byte sizes; `save`, `get`, or `delete` a named local JSON checkpoint. |
| `forge_guide` | Return the built-in usage guide. |
| `task_list` | List ready tasks in the primary repository by default; select repository/all, task `status`, `milestone_ref`, `milestone_status`, or stage explicitly. Task status accepts `ready`, `in_progress`, `blocked`, `parked`, `completed`, `cancelled`, or `all`; `parked` selects work waiting for a scope owner with retained worker/worktree affinity. Cancelled tasks retain history and leave dependencies unsatisfied; `all` includes them. Milestone status defaults to `active`, except a targeted milestone reference defaults to all statuses. Subtasks are compact by default; request `detail: "full"` for titles and evidence. |
| `task_claim` | Atomically claim the current task gate for a worker and worktree. |
| `task_submit` | Submit a revision-bound JSON `evidence` object. Passing the active profile's terminal delivery gate (default ID: `deliver`) writes the validated task receipt and context rollup. |
| `task_manage` | Create/sync specifications, inspect/context, delete, extend scope, manage subtasks, reset/reopen, or atomically dispatch with `action: "next"`, `worker_id`, and `worktree`. Next returns the claimed gate with its context bundle, prioritizing independent delivery/review, the worker's unblocked parked work, then ordinary ready work by wave and task ID. See the [action schema](tasks.md#five-flat-mcp-tools). |
| `task_blackboard` | Post, read, or inspect milestone-, task-, and subtask-scoped SQLite messages. `scope` selects the hierarchy level; `milestone_ref`, `task_id`, and `subtask_ref` identify or constrain it, with fail-closed resolution for omitted ambiguous context. `post` requires `topic` and `payload`, accepts optional `author`, and returns `{id}`. `read` accepts `topic`, `limit` (default 10, maximum 50), and `offset`; it returns newest-first payload-free `{messages}` and `{pagination}` from the resolved context. `inspect` with `message_id` searches configured task workspaces and returns the first matching message including its payload, ignoring optional `scope`, `task_id`, `milestone_ref`, and `subtask_ref`; without `message_id`, it returns the scoped read page. |

See [repository tasks](tasks.md) for schemas, receipts, configuration, and CLI parity, and the `contextunity-forge` skill for the task lifecycle.

## Worktree selection

Code navigation and documentation tools accept `worktree`: an absolute path,
a path relative to the server root, an exact worktree directory name, or a unique
hyphen-delimited suffix. For `.worktrees/049-admission-integrity-database-guards-and-lifecycle-hardening`,
both the full directory name and `lifecycle-hardening` select that tree.
Short names search Git's registered worktrees and immediate children of the
repository's `.worktrees/` directory. Existing explicit paths take precedence,
then exact names, then suffixes. Multiple matches return `WORKTREE_AMBIGUOUS`.

With no argument, a server rooted in a linked Git worktree retains that root.
A server in the primary tree selects the one physical directory used by active
task claims when workspace configuration is loaded or `forge-mcp.yaml` exists;
an empty `{}` configuration enables this lookup with default task settings.
Without configuration, the server retains its root and does not consult claims.
Duplicate claims for that directory count once. Multiple active
directories require explicit selection; no active directory uses the server root.
Use `worktree: "."` to explicitly select the server root. Missing or empty explicit
values fail with `WORKTREE_NOT_FOUND`. Each selected tree uses its own index;
selecting the server root retains a custom `--db` path. Task tools, checkpoints,
and `forge_guide` retain the server root.

Automatic claim selection rejects a configured task database that is a directory
or special file before opening SQLite. Explicit worktree selection remains
available without consulting task claims.

The MCP request does not carry the client's current working directory. Set the
server's `cwd`/`--root` to bind a session to a worktree, or pass `worktree` per call.

## Selectors and graph queries

Use a symbol identifier returned by search whenever possible. A file path selects a module or file. Selectors accept an exact ID, `path:name` or `path::name`, `path:known_kind:name` when the middle token is a registered node kind, and `path:line` or `path#Lline` for the narrowest symbol covering that line. Relative paths may use `./`, `file:`, or `file://` prefixes. A bare name can be ambiguous when a module and symbol share that name; specify the path. Markdown belongs in `get_doc` and `search_docs`.

`code_map_query` supports `overview`, `inspect`, `explain`, `impact`, `slice`, `unwired`, and `sql`. Its `impact` operation keeps the depth 2 default; the dedicated `code_map_impact` tool defaults to depth 1. Aliases: `doctor` for overview, and `search`, `discover`, or `find` for symbol search. `unwired` lists functions and methods with no indexed static caller and is not a dead-code proof. Compact `inspect` and `explain` operations return the same symbol summary as their dedicated tools; set `include_coverage=true` to add resolution coverage. For `operation="sql"`, put exactly one read-only `SELECT` or `WITH` statement in `selector`; secure SQL validation, query budget, and paged `QueryOptions` apply. Pass `limit`, `offset`, and `generation` as tool arguments. Impact direction defaults to `inbound`; `outbound` traverses dependencies used by the selected symbol. See [CLI reference](cli.md) for equivalent command forms.

Depth is limited to 16. A deep traversal whose first frontier exceeds 1,000 links is rejected; retry with depth 1 or a narrower selector. Removal analysis rejects scopes above 10,000 nodes. Graph results describe indexed, statically resolved relationships.

`code_map_search` accepts `query` as an alias for `pattern`. Its `path` accepts an exact indexed file or a directory whose descendants should be searched; `path: ""` leaves results unfiltered. Use `pattern: "*"` with a nonempty `path` or `kind` to list all symbols in that scope, including when `exact: true`. Unscoped `*` and repeated wildcard-only patterns remain invalid. For example, `{"query":"*","path":"src","kind":"function"}` lists functions under `src` with normal pagination.

`code_map_impact` and node-scoped `code_map_query` operations `impact` and `slice` mark the selected node with `is_seed=true`; at depth greater than one, other nodes include one `via` predecessor ID and edge kind along a shortest indexed path. Page totals and offsets include the seed. A directory selector in `code_map_query(operation="slice")` lists its indexed nodes with `graph_depth_applied=0` and exact selectors for subsequent graph traversal. A unique directory suffix is accepted; ambiguous suffixes return candidate full paths. `code_map_tests` follows exact `calls` edges and returns shortest `call_path` IDs for verified graph evidence. Inbound results include only `test_case` roots; helpers and fixtures can appear within a path but are not treated as test roots. If no verified path exists, the bounded lexical fallback may suggest a test case without a `call_path`. `possible_calls`, imports, and lexical matches do not establish graph coverage. Traversal is bounded by graph node, frontier, and depth budgets; an exceeded budget returns an error instead of an incomplete coverage claim. `code_map_analyze` pages `untested_public_surface` for declared-public production symbols in the selected path scope that no indexed test case reaches through exact `calls` edges; tests outside the selected production path can cover symbols within it. Public visibility describes each language's declaration rules, not transitive package or crate export reachability. `code_map_prove_removal.assessment` leads with a verdict and candidate-scoped blocking reasons; `safe_to_remove` is separate from workspace-wide unresolved and parse-error diagnostics.

`code_map_explain` accepts `incoming`, `outgoing`, or `both`; `inbound` and `outbound` are aliases. Compact inspect and explain results include the exact indexed signature and docstring, a lexical container resolved from method receiver metadata when available, inbound/outbound distinct-edge and occurrence counts, and up to five direct callers/callees per direction. Set `include_coverage=true` to add paged resolution coverage, or `show_doc=false` to omit linked document results.

`inspect` and `explain` include paged `analysis_gaps` for the selected symbol's source span, with reason, path, line, scope symbol, and resolver evidence. The reason `trait_object_dispatch` marks Rust trait-object calls whose concrete implementation depends on runtime dispatch. `macro_expansion_not_analyzed` marks an indexed Rust macro invocation; its edge to the macro declaration remains available for navigation. Gap pages use the query's `limit`, `offset`, and `generation`.

`calls` edges identify exact indexed calls. `possible_calls` edges identify bounded local branch-join targets and carry confidence `possible`; candidate sets contain at most eight declarations, and overflow is recorded as incomplete coverage. Explain pages preserve the edge kind and paging fields such as `has_more` and `next_offset`. Inbound impact follows `possible_calls`, and removal analysis lists a possible caller as an incoming dependency that blocks `safe_to_remove`.

For Python `from package import child`, a unique indexed child module contributes an import edge to that module and can resolve calls through its alias. A package can also expose a symbol through one explicit relative import in `__init__.py`; the graph links that import to the indexed declaration. When matching `.py` and `.pyi` modules exist, imports select the runtime `.py` module. If its indexed declarations omit a symbol, the matching type stub can supply that declaration; call evidence identifies the stub target. Other multiple child candidates remain ambiguous.

`code_map_analyze` groups unresolved references by recorded resolver cause and counts affected files. It also groups parse error records by language and affected files, and pages `untested_public_surface` by the requested path scope. A call through a proven external import has status `external` and contributes to `external_imports` instead of unresolved coverage. Use an exact indexed file as `target` to page individual locations; `detail="full"` includes the underlying resolver evidence. Test coverage is based only on exact `calls` paths from `test_case` roots; ambiguous `possible_calls`, helpers, fixtures, and lexical fallback suggestions do not remove a declared-public symbol from the untested page.

For an exact indexed file, diagnostic locations include the source line as `snippet` when the file still matches its indexed digest. Their `reason` is derived from persisted resolver evidence only when a recognized cause is available; otherwise it is `null`. Full detail retains the existing evidence fields.

Declared-public rules are language-specific: Python includes module-level names without a leading underscore, excluding nested declarations; Rust includes declarations written with unrestricted `pub`, even inside a private module; TypeScript includes local declarations in explicit named or default module exports and public/default-public members of exported classes, excluding private, protected, and `#private` members. The report does not infer whole-project re-export reachability, honor Python `__all__`, or treat type-only declarations as runtime call targets. Profiles without a declared-public rule leave visibility unknown.

Compact and full `code_map_explain` edge rows include both public endpoint IDs and direction relative to the selected node; full detail also includes stored edge evidence. `code_map_inspect` reports an empty linked-document set as `{ "total": 0 }`. `code_map_overview` provides the workspace root, schema version, counts, and page generation. Its `coverage_by_workspace` aspect pages counts from persisted primary and linked workspace metadata; linked paths retain their workspace identity after reopening the index.

Depth-one `code_map_impact` results include the exact selected edge on each non-seed node, with both endpoint IDs, kind, path, line, direction, confidence, and occurrence count; full detail includes stored edge evidence. At greater depths, nodes retain their shortest `via` predecessor.

For a Git change view, pass `changes:"staged"`, `changes:"unstaged"`, or `changes:"head"` instead of a symbol selector. The response includes the changed files and paged `changed_symbols`, impacted `nodes`, related `tests`, and `unmapped_changes`. A range maps only when its selected Git postimage matches the indexed file; deleted, binary, rename-only, unindexed, stale, or non-symbol ranges remain explicit unmapped results and make `complete` false. The continuation `generation` binds both the graph and captured change snapshot, so a staged or working-tree change invalidates an older token even when the graph itself is unchanged.

## Result size and source

Source options on `code_map_inspect`, `code_map_explain`, and `get_code_snippet`
clamp `leading_lines` above 20 to 20 and requested `max_body_lines` to 1–500.
When omitted, the body limit uses `response.source_context.max_body_lines` (200 by default).
Negative values and overflowing source offsets remain invalid. Adapter
`response.source_context.max_body_lines` values are strictly validated from 1 to 500.

Paged tools accept `limit` (default 30, range 1–100) and `offset`. Continue an ordinary page with its `next_offset` and `generation`; ordinary pages omit the repeated continuation text. A completed zero-result page contains `total` and `items` without pagination fields. Computation and byte limits retain their truncation status and recovery hint, including when no items were returned. The response byte budget is configurable from 1,024 to 65,536 bytes.

Code source previews from `code_map_search` appear automatically when the total hit count is at most three. For larger results, set `include_preview:true`; each response attaches previews to at most three hits, with at most 20 lines and 12 KiB combined preview text. Every search hit includes an indexed signature and an `inspect_selector` object; pass that object as the arguments to `code_map_inspect` to request the source preview directly. Search previews and snippets check that on-disk source still matches the indexed digest. `get_code_snippet` supports `leading_lines` from 0 to 20 and clamps requested `max_body_lines` to 1–500. If source changed, refresh the index before relying on the result.

## AST pattern capabilities

`ast_grep_search.language` accepts a compiled profile ID such as `python`, `rust`, or `typescript`. Omit it when `path` names a file with a recognized extension: `{"pattern":"class Widget","path":"src/example.py"}` infers Python. An explicit language takes precedence; directory scopes require an explicit language. A null or empty language also requests inference. When inference fails, the tool returns the compiled language list and usage examples.

Call `forge_guide` with `topic:"ast"` for the profile matrix and syntax examples. Profiles declare these independent capabilities:

| Capability | Behavior |
| --- | --- |
| `complete_syntax` | Parses a standalone pattern accepted at the grammar root; every profile supports this. |
| `declaration_without_body` | Matches declarations after removing profile-declared function, type, class, interface, or implementation bodies. |
| `fragment_probe` | Wraps root-rejected expressions, calls, macros, or assignments in a profile-provided probe container and unwraps the target syntax node. |
| `attribute` | Matches declared decorators or annotations layered over declarations. |

The registered matrix is: Rust supports all four capabilities; Python and TypeScript/JavaScript support `complete_syntax`, `declaration_without_body`, and `attribute`; HTML and Vue support `complete_syntax`. `$NAME` captures one syntax node. `$$$SEQ` captures a sequence of zero or more syntax nodes. Python declaration patterns accept `class Widget`, `class Widget:`, `def target($$$)`, and `def target($$$):`. Rust accepts a parameterless function header such as `fn target`, completed as an empty parameter list with a wildcard body.

Unsupported fragments and invalid syntax produce an MCP tool error with the diagnostic code, profile language, one-based line and column range, repair hint, and indented valid examples. The error text presents these fields directly without appending a raw JSON dump.

## SQL and checkpoints

`code_map_query(operation="sql")` accepts exactly one `SELECT` or `WITH` statement. The connection is read-only, and write or administrative SQL is rejected. It has a query time budget; large result sets should be paged. For structural cycle analysis, use `code_map_analyze` with `include_cycles=true`. Cycles are static graph findings.

A checkpoint saves a JSON value under a local name in `.forge/checkpoints.json`. `save` needs both a name and valid JSON content. `list` returns each name with its serialized JSON byte size; `get` loads the value. Names are 1–200 UTF-8 bytes and cannot contain `..`, `/`, `\`, or a NUL byte. Values are limited to 1 MiB. A checkpoint is local session data, not part of the source index.

## Syntax diagnostics on demand

Call `code_map_analyze` with `lint:true` and an indexed file or directory in `target`; an empty target covers indexed sources in the workspace. This reads the syntax diagnostics already stored by compiled language profiles. It does not run another parser, an external linter, or additional work during indexing. The usual source freshness check still applies to MCP reads.

```json
{"target":"src","lint":true,"limit":30}
```

Results include a rule ID, severity, language, path, line, and message, with pagination and generation checks. Coverage describes indexed sources and language-specific limits; excluded or unsupported files are not checked. An empty diagnostics page does not certify semantic correctness or compliance with style rules. Vue coverage is limited to its embedded scripts; HTML template expressions and styles are not validated.

Syntax lint mode does not execute cycle analysis. Use `code_map_query` for custom SQL or a separate `code_map_analyze` request for cycles. Ruff, Clippy, application-specific rules, and automatic fixes are outside this mode.

## Documentation queries

Markdown and MDX in admitted roots are indexed as documents, including repository `AGENTS.md` files and configured milestone and plan documents. Optional frontmatter `doc_type`, `title`, and `status` describe a section; missing status stays unspecified. Natural-language searches ignore common English stop words and match a section when any remaining term appears. Section-title matches receive three times the BM25 weight of body and invariant text. A query containing only stop words returns an empty result page. `get_doc` addresses the document path and an exact heading section; a missing path or section is an error. Its default MCP response includes section content and line bounds. Set `detail="compact"` explicitly for a section outline without content; `search_docs` keeps compact discovery results by default. Set `include_excerpt=true` to add a short excerpt centered on a search match.

Relative Markdown links and wikilinks create `cites_doc` and `cited_by_doc` edges only when their confined target path and heading resolve to one indexed section. `get_doc` pages those citations and `referenced_symbols`; a symbol is listed only when the indexed declaration is uniquely verified. For each documentary symbol claim, candidate resolution applies eligibility before priority: an explicit normalized file target must match that file, while an unscoped claim is limited to the source document's workspace. Document, invariant, and component nodes are excluded; module and file declarations remain eligible. Within eligible declarations, the first non-empty tier wins in this order: exact normalized qualified name (`::` and `.` are equivalent), exact declared name, then qualified-name segment suffix. Only a singleton winner in that selected tier can be a verified reference; lower-priority matches are not combined with it.

Ambiguous winners appear in the full response's paged `referenced_symbol_candidates` collection. Each row contains a `source` object (`doc_id`, `path`, claim `name`, and `line`) and a `target` object (`id`, `name`, `qualname`, `kind`, `path`, and `line`). Candidate pages use the normal `limit`, `offset`, and `generation` fields. Candidates are navigational only, not verified documentation: they create no `documents`, `references_doc`, `governs`, or `governed_by` edges and do not count as verified coverage. A union of candidate targets is not verified coverage. For example, an unqualified `validate` claim in a document indexed from the same `[library]` workspace can show three eligible module candidates, such as `[library]/src/validate.py`, `[library]/src/validate/__init__.py`, and `[library]/src/validate/validate.py`; link the claim to the intended file or qualify it so one declaration wins in the first non-empty tier.

Compact responses omit candidate data and return a hint to request full detail; the CLI `docs get` command requests full detail. A source link with a line range can retain its documented file claim after the declaration is removed, so `code_map_impact` can still report the affected document without treating a suggested replacement as verified.

`code_map_inspect` returns paged `documented_in` sections for verified symbol references. `code_map_impact` returns paged `affected_docs` for a selected path or symbol. Exact Markdown `code_map_analyze` responses may include paged `doc_diagnostics`: `broken_doc_link` for a link that does not resolve, `dead_doc_symbol` for a claim with no matching declaration (with an optional `suggested_symbol`), and `stale_doc_sections` when an in-bounds source range is disjoint from the unique current symbol range. Suggestions are remediation hints, not graph edges; ambiguous, unsafe, malformed, or out-of-bounds claims do not prove stale documentation.

Lines that declare `INV-*` rules are indexed as invariant nodes and linked to uniquely referenced symbols by `governs` and `governed_by` edges. Inspect, impact, and task context can surface verified governing rules with a single-line rule and source path/line. Navigation includes at most five direct rules and five IDs governing one-hop callers or callees; omitted counts and source references keep the response bounded. This graph context does not replace or truncate task contract invariants, profile guidance, decisions, commands, or proof receipts.
