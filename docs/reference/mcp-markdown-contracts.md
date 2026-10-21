---
title: "Markdown and JSON presentation contracts"
doc_type: reference
---

# Markdown and JSON presentation contracts

[ADR 0019](../adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md)
owns the output decision.
[Milestone 054](../milestones/054-markdown-first-dual-surface-and-agent-ergonomics.md)
owns implementation and acceptance.

## Shared format selection

MCP tools accept optional `format: "markdown" | "json"`. CLI commands accept
global `--format markdown|json`. Both default to Markdown and reject unknown
formats before running a query or mutation. Serve mode continues to use JSON-RPC
framing; this flag changes operation content, not the transport protocol.
For every shared operation, route equivalent validated input and semantic data
through the same formatter. Compare Markdown content exactly and JSON after
parsing. Normalize only transport framing, trailing newline, transport-specific
worker identity, and values that actually changed between requests.
MCP graph operations may append a top-level server-owned `freshness` envelope
after the callback in [`read_with_adapter`](../src/mcp/server.rs#L442-L479).
Parity checks assert the emitted envelope separately, then normalize only that
exact top-level block before comparing domain Markdown or parsed JSON. Preserve
nested and arbitrary domain freshness fields.

JSON retains structured domain values. Compaction applies only to explicitly
recognized operation envelopes. Preserve SQL rows and arbitrary checkpoint content
as opaque user data, including keys named `generation`, empty arrays, zero, and
false. Keep existing semantic error codes and validation behavior.

The first formatter task wires every public input/schema/argument allowlist and
response boundary, including inputs without PageInput (tasks, guide, checkpoint).
A generic readable rendering preserves fields for domains whose dedicated
presentation arrives in later tasks. Domain formatter tasks then replace that
presentation without changing the semantic result.

## Metadata and continuation

At final projection, omit inactive generation/freshness display hashes and empty
optional presentation sections. Preserve meaningful counts, empty-result outcomes,
false completeness flags, diagnostics, and requested detail. This is not a
recursive field-name filter.

Every active collection has a usable continuation hint containing its original
request scope, next offset, and accepted generation. Independent nested pages
remain independently recoverable. Source previews use `source_offset` and
`generation`, even when all graph collections are complete.
Generation is an opaque accepted token, not necessarily a 64-character string.
A Git-change generation binds the change view and must not be replaced with the
index generation.

Apply byte-bound trimming before deciding which tokens are inactive. If trimming
turns a complete page into a continuing page, emit its token and next offset.
Never skip a removed tail item. Preserve existing single-oversized-item errors,
source bounds, timeout recovery, and wire-size enforcement. Markdown escaping
and JSON-RPC escaping count toward the existing response budget.

Durable gate/content/candidate hashes and receipts are evidence, not display
metadata. Task and checkpoint content retains all data admitted by its operation.

## Code discovery

### Inspect and source snippets

Render symbol name/kind, selected path and actual declaration line, signature,
container when verified, docstring when requested, governing invariants/ADRs,
verified documentation links, and available relation categories. Preserve the
distinction between verified documentation and ambiguous candidates. Empty
optional relation headings are omitted.

Render verified DocGraph governing rules for inspect and impact in one
`> [!IMPORTANT] Governing Invariants` callout, with a bullet for each supplied
`INV-*` rule and its source link. Explain uses the same presentation when verified
governing rules are supplied. Omit the callout when empty. Preserve direct versus
one-hop provenance, existing omitted counts/source references and recovery;
do not add presentation truncation or promote candidate documentation to rules.
These bounded navigation summaries never replace complete task authority.

Render requested source in a language fence with actual line numbers and the
producer's range/truncation state. Honor `show_source`, `show_doc`,
`max_body_lines`, `leading_lines`, `source_offset`, `detail`, and
`include_coverage`; do not rename `show_source` to `include_source`.
A source continuation includes the selected symbol and accepted generation.
Source-disabled output still includes the available signature.

### Search

Render a pattern/count heading and numbered symbol entries with name links,
actual coordinates, kind, visibility/container when supplied, signature,
production/test classification, first docstring sentence, exact-match reason,
and verified graph pulse (callers, covering tests, implementation relations).
Render linked invariants and ADRs only from verified metadata.

Search enrichment missing from compact baseline results is produced by the
discovery owner through bounded existing graph/query seams. Do not infer
production/test status, seam coverage, exact matches, or traits from display names.
Preserve existing exact-priority ranking, grouping, preview bounds, and pagination.
An empty result states the request and offers a supported narrower/wider search
step; it does not claim the workspace contains no such symbol beyond index scope.

### Overview

Render selected workspace root, actual worktree/branch when supplied, graph
counts, language counts, compiled profiles, freshness warnings, documentation
hubs, and active milestone progress. Use task-store/manifest authority for
milestone state and configured admitted documentation roots for hubs.

Resolution health is `resolved / (resolved + unresolved + ambiguous) * 100`.
Aggregate the matching coverage scope consistently; exclude external imports.
Use unavailable when the denominator is zero, not an invented 100%.
Do not describe all nodes as code symbols or combine an aggregate unresolved
count that already includes ambiguity with ambiguous counts a second time.

Add granular counts using the same scoped coverage records and canonical
`diagnostic_reason` classification as exact-file diagnostics. The four 053
reasons are `dynamic_callback`, `unresolved_receiver_type`, `framework_magic`,
and `external_unindexed`; preserve `ambiguous_candidates`, unknown reasons and
recorded status separately rather than forcing every record into those four.
The overview producer currently lacks these aggregate reason counts; the
discovery task supplies bounded aggregation, not totals from a visible page.
If a count cannot be established within the selected scope/budget, render
`unavailable` with the cause. Emit zero only when the producer proves zero.

| Requested label | Supported meaning and unit |
| --- | --- |
| External crates | Show `external_imports` as external **references**; package/crate count is unavailable without manifest-backed identity aggregation |
| Dynamic callbacks | Records whose canonical reason is `dynamic_callback`; do not merge unresolved receivers |
| Framework magic | Records explicitly classified `framework_magic` by persisted evidence |
| Unindexed | Show `external_unindexed` diagnostic **references**, retaining their status; this is not a census of unindexed files/packages |

External-status references stay outside the health denominator and outside the
unresolved/ambiguous reason subtotal. An external-status record can itself carry
`external_unindexed`; its reason does not reclassify its status. Keep the
`unresolved_receiver_type` count visible as its own category.

Discover actual Cargo crates, Python project/Poetry packages, npm packages and
npm/pnpm/yarn workspaces, and Go modules from their manifests. Entries contain
name, ecosystem/unit, selected workspace-relative root, and description when
declared. Directory components stay visibly distinct from manifest-backed
packages. Respect configured roots, ignore policy, file size bounds, linked
workspace ownership, and symlink guards; do not traverse outside admitted roots.
Malformed or unsupported manifest data produces an explicit bounded diagnostic
or unavailable field rather than a fabricated package. Component collection
pages retain continuation under the shared response policy.

### AST search

Render pattern/language/count, each actual file range, source snippet, captures,
and enclosing symbol when provided. Preserve exact/lower-bound totals,
`computation_truncated` warnings and active continuation; do not render
incomplete computation as exhaustive search.

Unsupported-pattern diagnostics retain the producer's code, pattern span when
available, language, reason, supported examples, and next step in a caution
callout. Use `AstSearchCapabilities.valid_examples` and
`diagnostic_hint` from the selected profile, not Rust examples for every
language. Unavailable source spans are not invented.

## Traversal and diagnostics

### Explain and impact

Explain renders the selected signature/docstring, governing rules and verified
documentation, inbound/outbound relation groups, available call sites and types,
coverage caveats, and source continuation. Architectural or public-boundary
badges require producer evidence; signatures alone do not establish a boundary.

Label an edge `[proven call]` only when its actual kind is `calls` with exact
evidence. Label `possible_calls` as `[heuristic / candidate]`, retaining kind,
confidence, ambiguity and incomplete-coverage information. Use the real edge
path/line for `file:///...#L...` call-site links when supplied; a declaration
coordinate is not a call-site coordinate. Dynamic/name/lexical matches and
structural relations never acquire a proven-call badge. Deeper paths with only
`via` IDs/edge kinds keep their available evidence without invented locations
or a claim that every edge on the path is exact.

Impact renders requested direction/depth/mode, target, depth-grouped reached
nodes, actual call/edge coordinates, available docstrings and classification,
governing invariants, affected documents, and incompleteness. An empty traversal
reports no indexed matching dependencies within the selected bounds; it does not
prove an unreferenced terminal leaf or safe removal.

Git-change impact supports existing `changes: staged|unstaged|head`, exclusive
with selector. Render `changes.files`, `changed_symbols`, `nodes`,
`tests`, `unmapped_changes`, and `complete`. Preserve unmapped reasons,
rename/deletion details, query bounds, and restart guidance when the Git view
changes. Affected tests are indexed evidence, not a claim that executing them
is sufficient proof. Keep the change fingerprint needed for each nested page.

### Tests

Render the selected symbol and direction, verified `test_role`, discovery
method, unresolved-reference caveats, and `connection.relation` /
`connection.call_path`. Lexical fallback remains explicitly unverified and
does not acquire a graph call path. Inbound results are verified test cases;
helpers can appear on the path. Outbound results are production dependencies.

Render verified `connection.call_path` in its producer order as a compact arrow
breadcrumb: test case -> intermediate helpers -> selected target. Link each
node using verified symbol coordinates, annotate `test_case`/`test_helper`/
`test_fixture` only when supplied by the role producer, and mark the selected
target explicitly. The baseline path carries IDs; the traversal task hydrates
only returned path IDs through bounded existing symbol queries. Missing roles
or coordinates remain unavailable; names never establish either. Retain the
graph/lexical discovery method, unresolved caveat and every page continuation.

The baseline has no `positive_seam` / `negative_seam` intent producer.
By explicit user decision, intent semantics, producer and proof remain unresolved
for post-054 architecture / next planning. M054 renders verified roles and call
paths without adding intent inference or a declaration producer. Do not fabricate
intent badges from names, docstrings, assertions, or roles.

### Analyze, queries, and removal

Render existing analysis and syntax diagnostics with their category, actual
source/document coordinate, evidence, and supported remediation. Preserve
untested public-surface and documentation-drift result semantics, diagnostic
limits, candidate-versus-verified distinction, and selected path scope.
Use actual `target` arguments: an omitted target is not an admitted
`code_map_analyze()` example.

Generic graph queries, SQL results, and removal proof share core rendering and
parity. Preserve SQL values and removal uncertainty, including fail-closed
assessment, unresolved references and parse errors. Markdown changes no query
authorization or removal proof semantics.

## Task lifecycle

Use headings for task/stage/revision/status/worktree and the admitted contract:
target, dependencies, write scope, invariants, subtasks and DoD. Preserve the
complete active `workflow_guidance`, stage rules, proof policies, role/model
guidance, exact scoped commands, evidence templates and claim identities.
Preserve rejected findings, candidate bindings, and durable receipts.

Honor existing ADR 0016 stage tailoring and `bundle: false`; do not add raw
historical archives to lean operations. Full inspect remains the on-demand
history operation and preserves its available gate/finding/attempt evidence.
Render task lists with actual status and dependencies; cancellation is not
completion. Submit output retains acceptance/rejection, task identity,
stage/revision transition, evidence binding and remediation.

A contextual next-step line can replace duplicated prose only when it preserves
all distinct operative instructions. Do not substitute hardcoded gate lists,
invented `--proof` flags, made-up claim-force operations, or commit rules.
Use returned workflow guidance and actual `--evidence` commands.

Embedded blackboard context in claim/context and contract-stage bundles uses
the same blocker callout specified below. Preserve the message identity,
author, scope, gate, timestamp, supplied body and existing full-body recovery.
The lifecycle task owns this embedded presentation; the coordination task owns
standalone blackboard presentation. Existing stage selection and ADR 0016
summary bounds remain binding; formatting adds no new truncation.

## Coordination and documentation

Blackboard supports `post`, `read`, and `inspect`; topics are `draft`,
`notes`, `findings`, `blockers`, `decisions`, and `deferred`.
Group read results by their actual topic while preserving chronological order,
message identity, exact hierarchy scope, gate, author, timestamp and payload.
Render each `topic: blockers` message as `> [!CAUTION] BLOCKER (#<id>) by <author>`,
followed by scope, gate, timestamp and the supplied message body. Other topics
remain visibly distinct and do not acquire blocker severity. Preserve complete
IDs/authors/gates and body content; do not shorten them for display. Read pages
currently contain metadata without payload; use the existing bounded inspect
operation for a body, or display its exact recovery request when unavailable.
Existing claim summaries may bound body text; preserve that indication and
`task_blackboard(action: "inspect", message_id: <id>)` full-body recovery.
Grouping does not invent reply-parent edges or unresolved/resolved blocker
state. Preserve every read-page continuation and explicit summary truncation.

Documentation search renders path/section/type, actual excerpt when requested,
verified invariants/symbols and read instructions. Get-document uses
`path_or_id` and actual section selector, renders source Markdown with available
metadata/outline, and preserves full/compact detail and section continuation.
Keep actual headings, content, and candidate ambiguity.

Checkpoint actions are `list|get|save|delete`. List returns names and stored
byte sizes; save/delete return action and name. The store does not supply save
timestamps or file counts. Get returns arbitrary saved JSON values exactly,
rendered losslessly in Markdown (a JSON fence for nontext data is acceptable).
Do not compact user fields or invent restore effects. Preserve checkpoint
validation, atomic writes, existing 1 MiB storage limit, and separate
nonpaginated oversized-response error.

## Producer and consumer bindings

| Surface | Live producer / adapter | Presentation consumer |
| --- | --- | --- |
| Format and compaction | Shared `src/core/format/` | `src/mcp/response.rs`, CLI stdout; `src/mcp/metadata.rs` retains compatibility exports |
| Request validation | `src/mcp/tools/inputs.rs`, `src/mcp/tools.rs`, `src/cli/mod.rs` | All existing shared operations |
| Symbols and source | `src/db/symbols.rs`, `src/db/symbols/` | Inspect/search/snippet Markdown and JSON |
| Overview | `src/db/reader.rs`, scanner/config roots and task state | Overview Markdown and JSON |
| Resolution reasons | `src/db/reader.rs` canonical `diagnostic_reason`; discovery supplies same-scope bounded aggregate counts | Overview reason breakdown, with external reference counts separate |
| Ecosystem manifests | Discovery task supplies bounded producer; existing directory components alone are insufficient | Overview component entries |
| AST | `src/cli/ast.rs`, `src/engine/ast/`, `src/engine/languages/` | AST results and language-specific alerts |
| Relations / Git impact | `src/db/traversal.rs`, `src/db/symbols/test_discovery.rs` | Explain/impact/tests/query/removal |
| Test breadcrumbs | `connection.call_path` IDs plus bounded hydration from verified symbol/role data | Linked ordered test -> helper -> target breadcrumb |
| Diagnostics / docs | `src/db/reader.rs`, `src/engine/docs.rs` | Analyze/search-doc/get-doc |
| Tasks | `src/engine/tasks/`, `src/db/tasks_store/`, `src/cli/task.rs` | Task tools, CLI tasks, workflow agents |
| Blocker alerts | Canonical `blockers` topic, stored message/inspect payload and existing context summaries | Standalone blackboard and embedded claim/contract callouts |
| Checkpoints | `src/cli/guide.rs` | MCP checkpoint and CLI checkpoint |
| Test intent | No baseline producer; deferred to post-054 architecture / next planning | No inferred badges |

## Presentation examples

### Verified call-path fixture

This illustrative fixture is created by the acceptance test at a temporary root;
`/tmp/forge-orders-fixture` is its example root. Coordinates below refer to these
exact fixture declarations, not declarations in the Forge repository.
`src/orders.py` contains:

```python
def ProcessOrder():
    return "processed"
```

`tests/test_orders.py` contains:

```python
from src.orders import ProcessOrder

def setup_mock_driver():
    return ProcessOrder()

def test_order_processing():
    assert setup_mock_driver() == "processed"
```

After the producer verifies both calls and the test roles, render:

```markdown
Discovery: graph — verified exact calls
[test_order_processing](file:///tmp/forge-orders-fixture/tests/test_orders.py#L6) (test_case) -> [setup_mock_driver](file:///tmp/forge-orders-fixture/tests/test_orders.py#L3) (test_helper) -> [ProcessOrder](file:///tmp/forge-orders-fixture/src/orders.py#L1) (target)
```

The test resolves each link against its actual temporary root and verifies the
line/name. A lexical fallback shows `Discovery: lexical_fallback (unverified)`
and has no fabricated breadcrumb. A node without a verified role has no role badge.

### Governing rules and edge evidence

This repository example uses the verified rule for
`src/engine/linker/resolution.rs:23` and its actual rule source:

```markdown
> [!IMPORTANT] Governing Invariants
> - INV-DOC-FIRST-MATCH-TIER: Candidate resolution returns the first non-empty eligible declaration tier. [Source](file:///home/oleksii/ContextUnity/projects/contextunity-forge-mcp/docs/AGENTS.md#L92)
```

For the live exact call to `diagnostic_reason`, the edge coordinate differs from
the caller's declaration coordinate:

```markdown
- [annotate_diagnostic_page](file:///home/oleksii/ContextUnity/projects/contextunity-forge-mcp/src/db/reader.rs#L1824) [proven call] — calls diagnostic_reason at [src/db/reader.rs:1834](file:///home/oleksii/ContextUnity/projects/contextunity-forge-mcp/src/db/reader.rs#L1834)
```

A `possible_calls` fixture renders `[heuristic / candidate]` and its actual
supplied call-site link; it never reuses this proven-call label.

### Resolution-health fixture

For one fully counted fixture scope with 80 resolved, 15 unresolved,
5 ambiguous and 12 external-status references, render:

```markdown
Resolution Health: 80.0% (80 / 100 non-external references)
Resolved: 80; unresolved: 15; ambiguous: 5
External references: 12 (excluded from health); external crate count: unavailable
Non-external diagnostic references:
- Dynamic callbacks (dynamic_callback): 6
- Unresolved receiver type (unresolved_receiver_type): 4
- Framework magic (framework_magic): 3
- Unindexed external references (external_unindexed, status=unresolved): 2
- Ambiguous candidates (ambiguous_candidates): 5
Unindexed package/file inventory: unavailable
```

The four named unresolved reasons sum to 15; ambiguity contributes five more
records to the denominator. The 12 external references are a separate status
population, not 12 crates or additional unresolved records. An absent reason
aggregate renders unavailable rather than the fixture's numbers.

### Blocker message fixture

The same illustrative message is rendered in standalone blackboard and embedded
claim/contract context; other topics remain in their own topic groups:

```markdown
> [!CAUTION] BLOCKER (#42) by codex-builder
> Scope: task fixture-task; gate: contract; timestamp: 2026-10-21T10:00:00Z
> The verified fixture call path is missing its helper node. Preserve its actual role before submitting the contract proof.
```

For a metadata-only read or an existing bounded summary, retain its exact message
ID and `task_blackboard(action: "inspect", message_id: 42)` recovery. The alert
does not assert that a stored blocker has an independently tracked resolution state.

## Proof sensitivity

Use production CLI and stdio seams, real SQLite fixtures, and existing shared
test clients. Test omission/default, explicit Markdown, explicit JSON, invalid
format rejection before mutation, all active continuations, stale-generation
rejection, byte-limited tail trimming, source-only continuation, nested independent
pages, and opaque user content whose keys resemble metadata. Deliberately removing
a required token or operative rule must fail its seam assertion.

Discovery proof counts all fixture records in the selected workspace/path scope,
checks all four canonical 053 reasons plus ambiguity/unknown and separate external
status, and verifies the percentage on a nonzero and zero denominator. Adding
external records must not change health; substituting visible-page totals must
fail. Inspect/impact rule callout tests verify source links, omitted navigation
metadata and preservation of every supplied rule, with no empty callout.
Traversal proof checks the illustrative breadcrumb's order, role provenance and
actual coordinates, and exact `calls` versus `possible_calls`, lexical and
dynamic evidence; upgrading a candidate or dropping a helper must fail.
Lifecycle and coordination proof cover blockers and nonblockers in standalone
read/inspect plus embedded contract claim, including long IDs/authors, multiline
body, gate, existing summary recovery and page continuation. Relabeling a
nonblocker or dropping blocker body/recovery must fail.

Domain tests compare field meaning and real coordinates rather than frozen
invented symbol names or line numbers. Final parity tests cover every registered
tool and its existing CLI equivalent, including error outcomes, without adding
new transports. Run the full affected targets and strict Clippy under
[TESTS.md](../../TESTS.md).
