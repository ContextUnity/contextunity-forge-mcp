---
name: contextunity-forge
description: Load for repository onboarding to Forge/ACDD, code discovery, symbol lookup, dependency tracing, impact analysis, architectural verification, or ACDD task operations. Routes setup, structural search, and task coordination through Forge MCP.
---

# ContextUnity Forge MCP

Use Forge's indexed code graph, AST search, documentation graph, and task tools.
Prefer registered MCP tools; use the CLI for operations it owns. Fall back to
local search only when Forge is unavailable. The active registry and schemas are
the authority; see the [tool catalog](references/tool-catalog.md) for signatures
and [routing examples](references/agents.md.example) for repository integration.

For a request to onboard or configure a repository for Forge/ACDD, follow the
[repository onboarding guide](references/onboarding.md). It adapts the companion
examples to the repository's actual tooling, instructions, and delivery goals.

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

**Known symbol reads**: When the unique name is already known, use
`forge_read(selector="SymbolName")` directly; add the owner path with
`forge_read(path="...", selector="SymbolName")` to restrict resolution. For
example, prefer `forge_read(selector="render_role_prompt")` over first calling
`code_map_search(pattern="render_role_prompt")` and then reading a line range:
the direct snippet checks the indexed digest and returns the complete symbol body
within the response budget. If it is paged, continue with its `source_offset`
and `generation` until complete. Search when the name is unknown or resolution
needs disambiguation; inspect the returned node to confirm the intended owner.
MCP also accepts `forge_read(symbol="SymbolName")` as an alias. Two non-null
`symbol` and `selector` values conflict even if one is empty; `null` is absent.
Path restriction and paging are unchanged.

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

### Code clone and duplicate detection

Use `code_map_query(operation="duplicates", level="symbol", min_lines=10, target="crates")` to identify duplicated code across the workspace:
- `level="symbol"`: Finds function and method clones. Exact matches use SHA-256 hashes of literal symbol source slices; near-clones use normalized line similarity clustering among symbols with matching names.
- `level="file"`: Finds byte-identical duplicate files by SHA-256 content digest.
- `min_lines`: Filters candidates by minimum lines of code (default: 10).
- `target` or `path`: Restricts detection scope to a specific directory or file.
- Results provide grouped locations, line counts, duplicate counts, and actionable architectural `suggestion` strings indicating how and where to centralize or deduplicate code.
- CLI equivalent: `contextunity-forge-mcp query run duplicates --level symbol --min-lines 10`.

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
[canonical ACDD profile example](references/acdd_profile.yaml.example), select it
through the repository adapter's `acdd_profile` (or the owning milestone's
`acdd_profile`), and validate it before claiming work. Start with `version: 2`;
with `inherit: default` (the default when `inherit` is omitted), repository profiles
overlay `acdd.default.yaml` by merging `roles`, `commands`, `guidance`, `policies`,
and `scope_args_by_pattern` by key and recursively merging `gates:` by gate `id`
and `- tasks:` container before normalization (`inherit: none` compiles a standalone
workflow without base inheritance). Define review
contours through `guidance.contour.<name>` and reference `guidance.contours` on
review gates. Compose milestone `gates` with a pre-task `blueprint_synthesis` container
(`synthesize` $\to$ `review`) and a nested `- tasks:` container (`contract`, `build`,
`review`, conditional `seam` (`integrate` $\to$ `review`), and `deliver`). Gate
verification modes (`ProofKind`) are inferred automatically from structural gate fields
(`receipt`, `expected_failure`, `command`, `policy`/`contours`), while custom JSON
schemas are declared in the reusable top-level `proofs:` catalog (`proof: <name>`, e.g.
`proof: blueprint`) or inline on the gate via `proof: { scheme: { ... } }`. When
a pre-task root gate passes with `proof.tasks[<task_ref>]`, the engine records each
task's slice in `gate_transitions` (`subject = <full_task_id>`) and injects only that
task's slice into `context_bundle` upon `task_claim`. Command gates and reusable templates
supply verification and retry contours. Use the
[ACDD domain profile examples catalog](references/acdd-profile-examples/README.md)
for domain-specific starting points; verify examples against the live schema.
Dedicated standalone authoring workflows (`author-milestone.workflow.yaml`, `author-plan.workflow.yaml`, `author-workflow.workflow.yaml`) under `docs/workflows/` (and mirrored in `references/acdd-profile-examples/`) execute as taskless root-gate workflows (`inherit: none`, `save: none`) to synthesize, review, and preflight manifests, plans, and workflows before execution.
Custom profiles (never the embedded default profile) may also configure optional
`roles.<name>.runner` and `roles.<name>.prompt_template` strings using `{task_id}`,
`{stage}`, `{role}`, `{worktree}`, `{model}`, `{reasoning}`, `{role_guidance}`,
and `{goal}` slots; when configured, `task_claim` renders them in
`workflow_guidance.role_runner` (`runner` + `rendered_prompt`). Gates with
`independent_from` enforce independence at both `task_claim` and `task_submit`
by comparing `worker_id` with the accepted predecessor's worker. Optional
session metadata comes from an explicit in-process identity or a nonempty
`FORGE_AGENT_SESSION_ID`; it does not decide gate independence. Configured role
runners retain distinct session fingerprints for isolation and rejection resumption.

Explicit gate `tools` are requirements validated against enabled modules.
Empty tool lists receive module-filtered advisory navigation tools for contract,
command, and review proofs. Keep optional advice out of hard requirements when a
workflow must run with navigation disabled.

Claim the current gate, stay within admitted scope, and use `extend_scope`
autonomously before expanding edits into unadmitted files within module roots; do not halt execution or wait for human permission for routine scope extensions. Run iterative checks with `task_manage(action="verify", task_id="...", test="...", seam="...")`
or CLI `task verify <TASK_ID> [--test <file.rs::test_fn>] [--seam <target>]`; omit submit evidence to execute or reuse fresh
claim-bound command proof.
Typed contract and command proof require that observation and match its
seam reference, exit code, and counts. After preparing the binding, run
`milestone verify <id> --target-base <base>` for the first applicable root command.
Initial handoff requires its latest passing transition and matching successful trace,
as well as the current binding, completed task receipts or cancelled task states,
and independent passing review. Copy the observed command and counts into
`IntegrationEvidence`; external assertions alone cannot substitute for the trace.
Refresh verification and review when bound content changes. Validated archive or
journal retries follow their recovery rules without requiring pruned live rows.
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
scope justifications. Use `notes` only for non-obvious technical context that
the next worker needs; do not duplicate routine gate status, successful tests,
or SHAs unless they add context. Required verification and content-binding
records still follow their governing workflow.

Before rejecting a review, post structured `findings` with the affected code,
the observed issue, and a concrete remediation. Then submit the rejection with
the Blackboard message ID referenced in its findings. The ID supplements the
required typed findings payload and claim-bound evidence; it does not replace
them. MCP uses `task_submit(..., action="reject", findings=<JSON>)`; CLI uses
`task submit TASK_ID --stage STAGE --action reject --evidence '<JSON_OBJECT>'
--findings '<JSON>'`. Respect the active profile's evidence and routing rules.

Read applicable messages after claim and inspect IDs for full payloads. A
`blueprint_synthesis` architect records `topic="decisions"`; build drafts may
target `gate="build"`. Active `decisions`, `findings`, and `blockers` remain
eligible across gates in `context_bundle.blackboard`, preserving the original
gate within response bounds. Other topics follow untargeted/current-gate
selection. Record accepted decisions and deferred work with an owner/destination
before delivery; blackboard content supplements authority and never replaces
proof.
