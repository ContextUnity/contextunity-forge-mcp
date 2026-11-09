---
name: contextunity-forge
description: Load before repository code discovery, symbol lookup, dependency tracing, impact analysis, architectural verification, or ACDD task operations. Routes structural search and task coordination through Forge MCP.
---

# ContextUnity Forge MCP

Use Forge's indexed code graph, AST search, documentation graph, and task tools.
Prefer registered MCP tools; use the CLI for operations it owns. Fall back to
local search only when Forge is unavailable. The active registry and schemas are
the authority; see the [tool catalog](references/tool-catalog.md) for signatures
and [routing examples](references/agents.md.example) for repository integration.

## Orient and select the workspace

Start with `code_map_overview()` and confirm the root, components, languages, and
coverage before claiming a symbol or dependency is absent. Pass `worktree` to
select another checkout; omission keeps the canonical server root regardless of
client cwd or active task claims.

Worktree selectors resolve explicit paths first, then exact directory name,
literal or zero-normalized numeric prefix, suffix, and substring. The first
nonempty tier must be unique. Selected worktrees use their own adapter and index;
Git-linked worktrees share the resolved task store. CLI navigation uses `--root`;
CLI task claim/next uses an existing `--worktree` path, not MCP name aliases.

Use [adapter defaults](references/adapters.md) and the
[adapter example](references/forge-mcp.yaml.example) for roots, linked workspaces,
documentation paths, and response policy. Include both `src` and `crates` in
multi-crate workspace roots.

## Unified parameter conventions

| Intent | Parameters and examples |
| --- | --- |
| Find symbols | `code_map_search(pattern="Router", path="src")`; `query` aliases `pattern`. Use `takes:Type`, `returns:Type`, decorators, or `prefix*` to narrow discovery. |
| Resolve a symbol | `selector` accepts an exact ID or name, `path:name`, `path:known_kind:name`, `path:line`, or `path#Lline`. Use a separate `path` to disambiguate. |
| Match syntax | `ast_grep_search(pattern="fn $NAME($$$ARGS) {}", language="rust", path="crates")`; `query` aliases `pattern`. Supply `language` whenever `path` is a directory or omitted; inference requires a recognized file extension. CLI requires `--lang`. |
| Find documentation | `search_docs(query="routing", path="docs", include_excerpt=true)`; `pattern` aliases `query`, and `path` aliases `component`. |
| Read documentation | `get_doc(path="docs/reference/tasks.md", detail="compact")`; `path` and `selector` alias `path_or_id`. Omit `section` for an outline; specify an exact section title to read its content. |
| Diagnose | `code_map_analyze(path="src")`; `path` and `selector` alias `target`. Omit the target for workspace totals. `lint=true` reads stored syntax diagnostics; `include_cycles=true` opts into cycles. |
| Query SQL | `code_map_query(sql="SELECT count(*) FROM nodes")` implies `operation="sql"`. The explicit form is `code_map_query(operation="sql", selector="SELECT ...")`; `query` also accepts SQL. |
| Query graph | `code_map_query(operation="implementations", selector="LanguageLinker")` or `code_map_query(operation="untested", target="src")`; `query` aliases the selector. |
| Find duplicates | `code_map_query(operation="duplicates", level="symbol", min_lines=10, target="crates")`; `level` accepts `"symbol"` (functions/methods body clones) or `"file"` (identical files by digest). `min_lines` defaults to 10. `target` or `path` restricts search scope. |

Use one spelling for each aliased parameter in a request. The schemas advertise
aliases as optional properties; handlers reject a nonempty canonical value combined
with an alias. For SQL, an explicit `sql` field takes
precedence over `query`, then `selector`. A SELECT/WITH statement in `operation`,
or in `query` with an omitted/blank operation, also selects SQL mode. SQL accepts
one read-only SELECT/WITH statement; writes and additional statements are rejected.
These forms retain the same query budgets and pagination.

The SQL schema is `nodes(node_id, id, kind, name, qualname, path_id, line, end_line, is_test, language, generated, details, node_hash, owner_path_id)`, `path_dictionary(path_id, path)`, `edges(src_hash, kind, dst_hash, path_id, line, evidence_id, confidence_id, occurrence_count)`, `files(path, status, digest, size, language, is_test, generated, path_hash)`, and `doc_sections(doc_id, path, section_title, doc_type, content, invariants, referenced_symbols, is_invariant)`. Paths and endpoints use integer IDs/hashes; there are no compatibility views or `nodes.path`, `edges.src`, or `edges.dst` columns.

```sql
SELECT n.name, p.path, n.line
FROM nodes n JOIN path_dictionary p ON p.path_id = n.path_id
ORDER BY p.path, n.line, n.id;

SELECT src.name AS source, e.kind, dst.name AS target
FROM edges e
JOIN nodes src ON e.src_hash = src.node_hash
JOIN nodes dst ON e.dst_hash = dst.node_hash
ORDER BY src.id, e.kind, dst.id;
```

Missing-table or missing-column SQL errors retain the SQLite error and append a schema hint.

## Read symbols and trace relationships

Reuse sufficient search previews. Use `code_map_inspect` for a declaration,
signature, docstring, and compact call summary; use `code_map_explain` for paged
incoming/outgoing relationships. Request `include_coverage=true` when resolution
evidence matters.

**Symbol reads via `selector`**: ALWAYS prefer `forge_read(path="...", selector="Symbol")` or `forge_read(selector="path:Symbol")` to read entire functions, structs, or classes directly. It returns the complete symbol body with verified digest and enclosure context in one call.

For arbitrary line bounds, `forge_read` accepts `start_line` and `line_count`, plus ergonomic variations: `end_line` computes the inclusive `line_count` from `start_line` (default 1) and rejects zero or an end before the start, `lines="100-200"` parses range bounds into `start_line` and `line_count`, `lines=50` or `max_lines=50` aliases `line_count`, `file_path` aliases `path`, and `query` aliases `pattern`. Workspace-root absolute paths are automatically stripped to relative workspace paths. When reading lines enclosed inside an indexed function, struct, or method, `forge_read` emits an enclosing symbol hint in the output footer with the copy-ready `selector` call. Use `mode="search"` with `pattern` for literal/regex search, `mode="delta"` for Git changes, and `mode="outline"` for Markdown or code declarations.
CLI parity is `contextunity-forge-mcp read notes.txt --mode lines`. Snippets default to the complete symbol within the
serialized byte budget; `max_body_lines` sets an explicit smaller bound. Inspect
and explain retain configured preview defaults. Core text reads work without codemap. Respect `sensitive` path blocking and
confined `output_file` exports. Follow the conditional `Hints` footer only when
the enabled graph or active task confirms the suggested operation. Use `ctx_read`
for a single source line too large for the MCP budget.

Run depth-1 `code_map_impact` before interface edits or removals. Git change views
accept exactly one of `selector` or `changes="staged"|"unstaged"|"head"`; check
`complete` and `unmapped_changes` before treating the view as exhaustive.
`code_map_prove_removal` supplies indexed removal evidence, not runtime proof.

`code_map_tests` follows exact `calls` paths from classified test cases. Helpers
may occur inside a path but are not coverage roots. `possible_calls`, lexical
suggestions, and an absent indexed test call do not prove runtime coverage.

## Documentation and graph provenance

Triage with `search_docs(query="...", include_excerpt=true)`, inspect an outline
with `get_doc(path="...", detail="compact")`, then read a specific
`section="..."`. Full content is the default for `get_doc`.

Confined Markdown links/wikilinks create `cites_doc`/`cited_by_doc` only when one
indexed section resolves. Code claims use the document's workspace unless an
explicit normalized source-file path confines the target. Eligible declarations
resolve by the first nonempty tier: normalized qualname, exact name, then
qualified-name segment suffix. Only one target in that tier verifies a claim.

Full document results expose ambiguous `referenced_symbol_candidates`;
compact results omit them and hint at full detail. Candidates and suggestions
are navigation only. Verified references appear in `documented_in`; impact
returns `affected_docs`, including retained source-file claims after removal.
An `INV-*` governance edge requires a unique code claim within the invariant
node's line span. An explicit normalized source-file link confines resolution;
an unscoped code claim uses the document's workspace and the same uniqueness rule.
Archived documents under any `archive/` directory (`docs/milestones/archive/`,
`docs/plans/archive/`, `**/archive/**`) are excluded from default documentation
search and hubs. Search them with an explicit archive `component`/`path`, or
read them with `get_doc`. They emit no `documents`, `references_doc`, `governs`,
or `governed_by` edges. Task claim/context bundles intersect all indexed
non-archive documentation across the repository via `governs` and `documents`
edges while excluding `archive/**`, `docs/milestones/**` (already in
`context_bundle.contract`), and `docs/contracts/**` (already in
`context_bundle.governing_invariants`). Bounded governing-rule projections
supplement task authority and proof.

Follow the [documentation layout](references/documentation-layout.md); adapt the
[docs README](references/docs-readme.md.example) and
[docs rules](references/docs-agents.md.example) when setting up a repository.
Read [framework pack guidance](references/framework-packs.md) before changing
declarative extraction rules; keep API identities in rule data and preserve
canonical typed facts and exact-target evidence.

## Response bounds and continuation

SQLite queries have a 2-second budget. Responses cap at 96 KiB unless configured
lower; serialized metadata and Markdown/JSON escaping count toward that limit.
Pages default to 30 items and allow 1–100. Prefer compact detail for discovery and
full detail for one selected entity. MCP defaults to Markdown; use `format="json"`
or CLI `--format json` for structured consumption.

Continue collection pages with `next_offset` and the returned `generation`.
Source continuation uses `next_source_offset` as `source_offset` with that page's
explicit generation. Missing or stale tokens require a restart; no generation is
inferred. Source pages contain complete lines and verify the on-disk digest.
Task-list generations bind task state separately from code-index generations.
On computation limits, narrow scope/depth; on byte limits, reduce detail or page
size. Inspect a truncated blackboard message by its returned message ID.

## Test harness ownership

When present, read the target repository's `docs/architecture/test-harness.md`,
section "Rule 3: The Two-Caller Rule".
Shared disk setup lives in `tests/common`. Shared ACDD session, evidence, and
task completion live in `tests/acdd/support.rs`. A domain harness function needs
at least two callers. Load `test-suite-refactor` when moving setup out of a
`#[test]`.

## ACDD profiles and task operations

Workflow sequence, roles/models, proof gates, review contours, commands, and
persistence belong to the active embedded/repository profile. Read
`workflow_guidance` returned by `task_claim` and submit its claim-bound
`expected_evidence_template`; do not infer a fixed sequence from this skill.
Repository `AGENTS.md` owns verification commands and Git permissions.

To customize a repository or task workflow, copy the
[canonical v2 profile example](references/acdd_profile.yaml.example), select it
through the repository adapter's `acdd_profile` (or the owning milestone's
`acdd_profile`), and validate it before claiming work. Start with `version: 2`;
configure `modules`, worker `roles`, registered `commands`, path-scoped command
arguments in `scope_args_by_pattern`, and reusable `guidance`. Define review
contours through `guidance.contour.<name>` and reference `guidance.contours` on
review gates. Compose `gates` with an upfront `contract_synthesis` gate and a
nested `- tasks:` container for task gates. Command gates and reusable templates
supply verification and retry contours. Use the
[ACDD domain profile examples catalog](references/acdd-profile-examples/README.md)
for domain-specific starting points; verify examples against the live schema.
Custom profiles (never the embedded default profile) may also configure optional
`roles.<name>.runner` and `roles.<name>.prompt_template` strings using `{task_id}`,
`{stage}`, `{role}`, `{worktree}`, `{model}`, `{reasoning}`, `{role_guidance}`,
and `{goal}` slots; when configured, `task_claim` renders them in
`workflow_guidance.role_runner` (`runner` + `rendered_prompt`). Gates with
`independent_from` enforce independence at both `task_claim` and `task_submit`
across `worker_id` and persisted `session_fingerprint`. CLI and MCP calls obtain
the session fingerprint from `FORGE_AGENT_SESSION_ID` when it is set and nonempty.

Explicit gate `tools` are requirements validated against enabled modules.
Empty tool lists receive module-filtered advisory navigation tools for contract,
command, and review proofs. Keep optional advice out of hard requirements when a
workflow must run with navigation disabled.

Claim the current gate, stay within admitted scope, and use `extend_scope`
autonomously before expanding edits into unadmitted files within module roots; do not halt execution or wait for human permission for routine scope extensions. Run iterative checks with `task_manage(action="verify", task_id="...", test="...", seam="...")`
or CLI `task verify <TASK_ID> [--test <file.rs::test_fn>] [--seam <target>]`; omit submit evidence to execute or reuse fresh
claim-bound command proof.
Typed contract and command proof require that observation and match its
seam reference, exit code, and counts. `milestone verify` is an optional one-shot
run of the first applicable root command gate from the profile; it reports pass or
fail and records its command trace without creating a separate milestone repair or
review lifecycle. Milestone handoff validates the current content binding, completed
task receipts or cancelled task states, independent passing review, and successful
verification supplied in `IntegrationEvidence`. Refresh integration evidence after
changing the candidate. Handoff retries validate the accepted archive and evidence
without requiring a root verification checkpoint.
Command output schemas declare regex counts, noise, errors, and seam extraction
under `commands.<id>.output`. Passing runs and short
failures stay inline; truncated failures link worktree-confined trace logs.
Preserve candidate immutability after review and during
delivery; let `task_submit` manage receipts. `task_manage(action="next")` claims
eligible work atomically; CLI milestone init/list/show/handoff-context/verify/handoff
owns milestone operations. For schemas, recovery, delivery, and handoff, consult
the [task reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/tasks.md).
On `TASK_CHECKPOINT_PENDING`, use `milestone db-recover` and obtain a fresh claim;
preserve SQL/keeper state rather than repairing it manually.

## Blackboard

Use milestone scope for cross-task state and task/subtask scope for local
decisions. Topics are `draft`, `notes`, `findings`, `blockers`, `decisions`, and
`deferred`. Post material architectural decisions, findings, blockers, and
scope justifications; gate telemetry already records routine transitions.

Read applicable messages after claim and inspect IDs for full payloads. A
`contract_synthesis` architect records `topic="decisions"`; build drafts may
target `gate="build"`. Active `decisions`, `findings`, and `blockers` remain
eligible across gates in `context_bundle.blackboard`, preserving the original
gate within response bounds. Other topics follow untargeted/current-gate
selection. Record accepted decisions and deferred work with an owner/destination
before delivery; blackboard content supplements authority and never replaces
proof.
