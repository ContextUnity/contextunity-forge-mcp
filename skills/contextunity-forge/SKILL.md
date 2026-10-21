---
name: contextunity-forge
description: Load before repository code discovery, symbol lookup, dependency tracing, impact analysis, architectural verification, or ACDD task operations. Routes structural search and task coordination through Forge MCP.
---

# ContextUnity Forge MCP

Use Forge's semantic code graph, architectural inspection, impact analysis, and ACDD task
coordination in indexed repositories.

## Documentation ownership

Keep pages about current behavior or commands. Follow the [documentation layout](references/documentation-layout.md).
When starting docs in another repository, adapt the [docs README](references/docs-readme.md.example)
and [docs rules](references/docs-agents.md.example). The repository root `AGENTS.md` owns local
verification commands and Git permissions. The Forge repository owns schemas in `docs/reference/`
and indexed-page markup in `docs/AGENTS.md`.

## Forge-first tool selection

- Use registered Forge MCP tools as the primary path for discovery, architecture inspection,
  documentation search, and task lifecycle. Use the CLI where it owns the operation; fall back to
  scripts or raw text search only when Forge is unavailable.
- Use the active server registry and schemas. Common registrations are `contextunity-forge` and
  `contextunity-forge-mcp`; see [routing examples](references/agents.md.example). Start with
  `code_map_overview()` and confirm `workspace_root`, coverage, and components.
- Pass `worktree` when needed. Every MCP tool, including task tools, `forge_guide`, and `session_checkpoint`, uses the same request-local resolver. Omitted `worktree` always retains the canonical server root, independent of active claims.
- Selectors resolve in this order: `"."` or an absolute/server-root-relative explicit path; exact directory name; literal prefix or pure-digit milestone prefix after leading-zero normalization; literal suffix; literal substring. Candidate paths are canonicalized and deduplicated. The first nonempty tier must be unique; ambiguity returns `WORKTREE_AMBIGUOUS`, and a missing or empty selector returns `WORKTREE_NOT_FOUND`.
- Selected worktrees use their own adapter/configuration, index, milestone manifests, and checkpoints. Git-linked worktrees share the task database through task-store resolution; standalone repositories remain independent unless explicitly configured to share one. Selecting the server root retains a custom MCP `--db` path. MCP requests do not carry client cwd.
- Keep normal adapter, task-store, and index safety validation in place; selecting a worktree does not bypass configuration or reserved-database checks.
- These human-friendly selectors apply to MCP requests, not CLI commands. CLI queries select a workspace with `--root`; CLI task claim/next use an existing `--worktree` directory path.
- Milestone `init`, `list`, `show`, `handoff-context`, and `handoff` are CLI-only. `task next` and
  MCP `task_manage(action="next", ...)` atomically claim the next eligible task and return claim
  context or `status: "idle"`; parked tasks resume in their retained worktree.
- `task show` aliases `task inspect`; inspect/submit accept `--task`, sync accepts `--file`, and
  `task list --detail compact|full` supports `--full` only without `--detail`. See the
  [tool catalog](references/tool-catalog.md) for dispatch order and signatures.
- MCP AST search infers language only from a recognized extension; otherwise supply a compiled
  profile ID. CLI `ast grep` requires `--lang`. On Unix, `contextunity-forge-mcp reload` sends
  `SIGHUP` and replaces running servers without breaking client connections.

## Query and response bounds

Each SQLite query has a 2-second budget; unbounded queries fail fast. Responses cap at 64 KiB. Pages
default to 30 items and allow at most 100. Continue when `next_offset > 0` by passing that offset and the
prior generation token. MCP tools and CLI commands default to Markdown; pass `format: "json"` to an MCP
tool or global `--format json` to the CLI when a caller needs the JSON payload. Invalid format values
fail before an operation runs. Prefer compact detail on broad queries; use full detail for a narrow entity.
On budget errors, narrow the selector/path or reduce depth.

`code_map_impact` accepts exactly one symbol selector or Git change mode (`staged`, `unstaged`,
`head`). Its continuation generation binds the graph and captured change snapshot. Check
`unmapped_changes` and `complete` before treating a change view as exhaustive.

## Discovery and verification workflow

- Orient with `code_map_overview`; verify the active worktree and indexed coverage before claiming
  something is absent.
- Locate with `code_map_search` (compact for broad searches); use `code_map_inspect` for the exact
  declaration, signature, docs, and invariants.
- Use `code_map_explain` for ownership/callers/callees and depth-1 `code_map_impact` before edits or
  removals.
- Use `code_map_tests` to trace exact calls from classified test cases. Helpers and fixtures may
  occur in a path but are not coverage roots; `possible_calls` and lexical suggestions do not prove
  coverage, and a lexical suggestion has no `call_path`. Markdown graph results show the ordered
  test-to-target breadcrumb with verified links and producer-supplied roles; lexical suggestions are
  explicitly unverified.
- Read a bounded `get_code_snippet` preview, then use `ctx_read` for exact source ranges before
  editing. Inspect/snippet default to 5 leading and 200 body lines; requested body lines clamp to
  1..500 within byte caps.
- Claim the active ACDD gate before task work, implement its accepted contract, and submit through
  its claim-bound evidence template.

## ACDD authority and scope

ACDD connects Git milestone contracts (`docs/milestones/*.md`) to `.forge/tasks.sqlite`; rebuilding the code index does not
reset task state. The [task reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/tasks.md) owns operation/evidence schemas. The active profile and claim's
`workflow_guidance` own gates/order, proof policy, roles/models, review contours, and profile-specific
behavior. Do not infer a fixed workflow from this skill. Use [references/acdd_profile.yaml.example](references/acdd_profile.yaml.example) only as a configuration
example.

The repository `AGENTS.md` names this skill, local verification commands, and Git permissions; it
must not duplicate this procedure. Install the skill at `~/.agents/skills/contextunity-forge/SKILL.md` or `.agents/skills/contextunity-forge/SKILL.md`. Claim/inspect
return `TASK_GUIDANCE_MISSING` if configured guidance is missing, does not name the skill, or the skill is not
installed.

When a task changes observable behavior, interfaces, workflows, configuration, or operational
assumptions, update its documentation and agent instructions during build. Include those files in
initial scope or admit them with `extend_scope` before editing; do not defer changes caused by the
task's own contract.

Task claims inject these guardrails into `workflow_guidance.rules`:

- `PROTECTED`: milestone `id`, `invariants`, `owners`, `depends_on`, and `acdd_profile` cannot
  change while tasks are active.
- `ISOLATION`: work only in the assigned dedicated worktree.
- `IMMUTABILITY`: do not change admitted scope after review passes or during delivery.
- `INDEPENDENCE`: reviewer `worker_id` differs from the accepted builder.
- `NO_PREFLIGHT_SUBAGENTS`: start active-gate work without preflight agents.
- `NEVER_EDIT_RECEIPTS`: never edit milestone receipt/status blocks;
  `task_submit(stage="deliver")` manages receipts.

The embedded roles are `contract_author`, `builder`, and `independent_reviewer`. The author posts the build draft to
task blackboard with `topic=draft` and `gate=build`; the same independent reviewer continues through
review and delivery. Use the claim-bound `expected_evidence_template` and registered scoped commands. Contract/build
context can include `external_callers`, direct/two-hop `covering_tests`, and ADR summaries. Seams must reach an
in-scope production symbol; out-of-scope indexed callers on changed public signatures can fail with
`TASK_SEAM_DISCONNECTED` or `TASK_SCOPE_INCOMPLETE`. For admitted greenfield files absent from disk, the contract author adds
minimal compiling stubs and module/import declarations before index refresh.

Task tools and CLI task commands show Markdown by default; request MCP
`format: "json"` or CLI `--format json` when consuming structured values. Read
claim/context sections for lifecycle state, active profile guidance, exact proof
commands/templates, scope, dependencies, and available task evidence. Embedded
`blockers` are marked with their original identity and gate; when a body summary
is truncated, follow its `task_blackboard(action="inspect", message_id=...)`
recovery before treating the summary as complete. The
[task lifecycle Markdown contract](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/mcp-markdown-contracts.md#task-lifecycle)
defines the rendered fields and preserved JSON behavior.

Lifecycle errors use `{CODE}: {task_id}. {facts}. Do: {action}` (or action/milestone-scoped equivalents). `AUTHORITY_GAP` distinguishes
milestone-frontmatter divergence from task-spec changes and gives exact remediation.

### Tests and scope

Drive admitted contracts through real collaborating components and the existing domain suites, not
synthetic stubs. Prefer existing test suites; inspect `covering_tests` during contract work and add cases
to existing domain suites where possible. Create a test file only when no existing suite covers the
module or a genuinely isolated regression harness is required. Use structured case tables, shared
fixtures/harnesses/temp directories, and `TESTS.md` plus `tests/AGENTS.md` for target placement and
commands.

Keep each task focused on one public production seam and add subtask cases there. Stay within
admitted scope; use `task_manage(action="extend_scope")` before editing outside it and follow the contract's scope
roots/revision rules.

Record discoveries with `task subtask add`, not a new root task. `subtask_dod` is advisory; status updates
record caller evidence and do not score it. Subtasks accept `ready`, `in_progress`, `completed`,
`cancelled`, and `blocked`; legacy `pending` aliases to `ready`. `parked` is for top-level
worktree affinity. Cancelled tasks retain history, do not satisfy dependencies, and leave dispatch;
follow the task reference for specification sync/reset/reopen, and reopen terminal parents before
editing subtasks.

The advisory completion criteria are bounded contract resolution, ADR/milestone alignment,
production-seam proof, preservation of untouched behavior, and fail-closed escalation when attempts
stall.

## Blackboard

- Use milestone scope for cross-task state, task scope for task results, and subtask scope for a
  local case. Topics are `draft`, `notes`, `findings`, `blockers`, `decisions`, and `deferred`.
- After claim, read applicable messages. `read` returns metadata; inspect message IDs for full
  payloads because summaries may be truncated or omitted.
- Markdown reads group messages by their actual topic and preserve the page's newest-first order.
  Blocker callouts retain message identity and available scope, gate, timestamp, and body; payload-free
  summaries carry the exact inspect request. Grouping creates no reply or resolution state.
- Builders post verification as `notes`; reviewers post findings or no-findings as `findings`;
  coordinators reconcile as `notes`. Post blockers when found and resolution when state changes.
- Publish material results, decisions, blockers, or ownership changes with contract
  revision/candidate, check/outcome, evidence, and next owner/action. Keep posts concise and preserve
  durable evidence outside temporary paths.
- Before handoff, post status, unresolved findings, evidence, and next action. Omit `gate` for
  cross-gate notes or target the next gate. Claims include untargeted/current-gate messages; read
  targeted items through `workflow_guidance.blackboard_messages`.
- Before delivery, record accepted decisions and deferred work with owner/destination. Delivery
  copies them to `architectural_notes` before clearing messages. Blackboard does not amend task
  authority or replace proof.

## Delivery and milestone handoff

The active profile controls commits: `auto_commit: true` makes Forge commit the delivered candidate and
receipt; do not duplicate it. With `false`, Forge records the receipt without a commit; create
exactly one scoped commit under repository rules.

Before handoff, follow the repository documentation scan. The coordinator runs `agent-instructions` against
the final diff and admitted docs, records the result on the milestone board, and reviews changes
before final verification; Forge launches no agent and adds no gate.

Commit the final implementation, prepare a binding, obtain independent integration review, and
verify the same content against the target base:

```sh
contextunity-forge-mcp milestone handoff-context <id> --target-base main > /tmp/handoff-context.json
# Coordinator supplies completed external review and verification evidence.
contextunity-forge-mcp milestone handoff <id> --evidence-file /tmp/handoff-evidence.json
```

Use the task reference for typed evidence/projection. Integration review must differ from every
accepted task builder. Preserve task proofs through rebase/cherry-pick/merge/squash; optional
navigation SHAs may change without reopening tasks. Changed content or authority needs fresh final
review and verification. Forge validates and archives the supplied record but runs no tests or
agents. Commit the archive and working manifest removal afterward under repository Git permissions.

## Adapters and framework packs

Use [adapter defaults](references/adapters.md) and the canonical [adapter example](references/forge-mcp.yaml.example) for roots, linked workspaces, ignores, task identity,
response limits, and no-config behavior.

Use [framework pack guidance](references/framework-packs.md) and its TOML/YAML examples for declarative rules. For language support, provide all
six `SyntaxGrammarSpec` categories and use shared `SinglePassExtractor` owner frames; keep language body events/linking
in existing owners, preserve public facts/commitments, do not add a recursive body collector, and
keep query experiments out of default indexing.

## Documentation graph and provenance

Confined Markdown links/wikilinks create `cites_doc`/`cited_by_doc` edges only when one section resolves.
Documentary code claims use the source document's workspace unless an explicit normalized file path
confines them elsewhere; invalid or unsafe paths never fall back globally. Document, invariant, and
component nodes cannot verify claims; module and file declarations remain eligible. For each
eligible claim occurrence, the first non-empty tier wins: exact normalized qualname (`::` and
`.` equivalent), exact declared name, then qualified-name segment suffix. Only one target in
that tier verifies the reference; lower tiers do not combine with it.

Full `get_doc` pages ambiguous selected-tier `referenced_symbol_candidates`; compact omits them and hints at full
detail. Candidates are navigation only: they do not create `documents`, `references_doc`, `governs`, or
`governed_by` edges and do not establish verified coverage. MCP and CLI `docs get` default to full.
Suggestions and token similarity are not verified references or governance. Inspect exposes verified
`documented_in`; impact exposes `affected_docs`, including retained source-file claims after declaration
removal. `INV-*` rules become invariant nodes. A `governs` edge requires a unique code claim with
an explicit normalized source-file target on the same source line as its invariant ID; uniqueness
alone is insufficient. Inspect, impact, and task context show at most five direct rules and five one-hop IDs
with source locations and omitted counts. This projection supplements and never replaces task
invariants, profile guidance, decisions, commands, or proof.

See the [tool catalog](references/tool-catalog.md) for tool signatures, selector forms, and full graph behavior.
