---
doc_type: api
title: CLI reference
---

# CLI reference

`contextunity-forge-mcp` provides a command-line interface and a stdio MCP server. Run `contextunity-forge-mcp --help` or append `--help` to any command for accepted arguments. Commands other than `serve` print readable Markdown by default. Pass global `--format json` to retain machine-readable JSON output.

## Global options

| Option | Behavior |
| --- | --- |
| `--root <PATH>` | Workspace for all commands. Defaults to the current directory; `FORGE_WORKSPACE_ROOT` can set it. |
| `--db <PATH>` | Code-index path, relative to the selected root. Defaults to `<root>/.forge/code-map.sqlite`; `FORGE_DB` can set it. Does not select the task store. |
| `--config <PATH>` | Root-confined Forge YAML configuration; `FORGE_CONFIG` can set it. Explicit missing files fail. Defaults to `<root>/forge-mcp.yaml` when present. |
| `--profile <PATH>` | Root-confined gate profile, overriding the selected config's `acdd_profile`; `FORGE_PROFILE` can set it. Explicit missing files fail. |
| `--format markdown|json` | Output format for command results. Defaults to `markdown`; invalid values fail before a command runs. |

`build`, `scan`, and `delta` also accept a positional workspace path. Either form selects the workspace; when both are supplied, they must resolve to the same directory.

Selections resolve once per invocation. Flags override their environment variables.
Profile selection is explicit profile, selected configuration's `acdd_profile`,
repository `.forge/acdd/profile.yaml`, then the embedded profile. Canonical
profile references retain pin validation; task and milestone pins still apply.
Configuration and profile paths resolve under the chosen root; absolute paths
must remain confined there. A selected config without `acdd_profile` falls back
to the repository/embedded profile, not another configuration file.
Task-store resolution continues to use `tasks_db` and the primary Git worktree.
Owned runner children receive selected paths without mutating the parent
environment. Concurrent in-process invocations retain separate selections.

`serve` retains the selected config and profile for its lifetime, including
module visibility, task loaders, request policy, refreshes and reloads. Selected
worktrees rebase root-relative settings. `delta --config` validates the selected
adapter and uses the guarded rebuild path when policy changes; without an
explicit config, delta retains the persisted index adapter. Missing or malformed
explicit selections fail before index publication.

## Inventory and indexing

```sh
contextunity-forge-mcp scan .
contextunity-forge-mcp build .
contextunity-forge-mcp build /path/to/repository --output /path/to/index.sqlite
contextunity-forge-mcp delta . src/module.py src/other.py
```

`scan` reports admitted files without writing an index. `build` creates a full index. `delta` updates the listed changed or deleted paths in an existing index. `build` and `scan` accept `--adapter <PATH>` for a configuration file inside the workspace; reads use the selected configuration. Conflicting `--adapter` and `--config` paths fail.

`query`, `docs`, and `ast grep` open the index through `open_or_rebuild`: a missing or schema-incompatible index is built automatically. An existing index is not refreshed just because source files changed; run `build` or `delta` to update those results.

## File and symbol reads

```sh
contextunity-forge-mcp read notes.txt --start-line 20 --line-count 40
contextunity-forge-mcp read notes.txt --mode search --pattern 'timeout' --context 2
contextunity-forge-mcp read notes.txt --mode search --pattern 'error|warning' --regex
contextunity-forge-mcp read docs/reference/tasks.md --mode outline
contextunity-forge-mcp read src/engine/reader.rs --selector forge_read_tool
contextunity-forge-mcp read notes.txt --mode delta --baseline HEAD
contextunity-forge-mcp read notes.txt --mode raw --output-file exports/read.txt
```

`read` uses the same layered reader as MCP `forge_read`. The positional path is
required; `--selector` selects a symbol within that file. Alternatively, pass a
symbol selector as the positional value with `--mode snippet`. Auto mode selects
snippet for `--selector`, search for `--pattern`, and lines otherwise.
Core text modes run without an index. Markdown outline requires docs; code
outline and snippet require codemap. Use `--max-body-lines` and `--leading-lines`
to bound symbol output, then `--source-offset` with the previous `--generation`
for continuation. See the [reader contract](mcp-tools.md#layered-file-reader) for
text bounds, exports, sensitive paths, task badges, and conditional hints.

## Queries

```sh
contextunity-forge-mcp query overview
contextunity-forge-mcp query overview --aspects entrypoints
contextunity-forge-mcp query overview --aspects entrypoints_with_tests
contextunity-forge-mcp query overview --aspects topology
contextunity-forge-mcp query search 'parse*' --kind function --limit 30
contextunity-forge-mcp query search 'takes:GateProfile'
contextunity-forge-mcp query search 'returns:Result'
contextunity-forge-mcp query inspect 'src/module.py:parse' --show-source
contextunity-forge-mcp query explain 'src/module.py:parse'
contextunity-forge-mcp query impact 'src/module.py:parse' --depth 1
contextunity-forge-mcp query impact --changes staged --depth 1
contextunity-forge-mcp query tests 'src/module.py:parse' --direction inbound
contextunity-forge-mcp query remove 'src/module.py:parse'
contextunity-forge-mcp query analyze 'SELECT name FROM nodes ORDER BY name'
contextunity-forge-mcp query run slice 'src/module.py:parse' --depth 1 --limit 30
contextunity-forge-mcp query run slice src/db --limit 30
contextunity-forge-mcp query run untested src/core --limit 30
contextunity-forge-mcp query run implementations LanguageLinker --limit 30
contextunity-forge-mcp query run duplicates --level symbol --min-lines 10 --limit 30
contextunity-forge-mcp query run duplicates src --level file --limit 30
contextunity-forge-mcp --format json query overview
```

`query overview --aspects` accepts comma-separated aspect names. `entrypoints` excludes tests; `entrypoints_with_tests` includes them in the same `entrypoints` result. Repeat the chosen aspect and use `--limit`, `--offset`, and the previous page's `--generation` to continue a bounded aspect page.

`topology` is opt-in and returns paged module statistics and dependency edges;
see the [MCP reference](mcp-tools.md#selectors-and-graph-queries) for grouping
and role rules. Directory slices include bounded inbound and outbound boundary
summaries. CLI and MCP read the same index and return the same graph data.

`query impact` defaults to one edge and accepts exactly one symbol selector or `--changes staged|unstaged|head`; `--path` can restrict a change view to a workspace-relative path. Use `--limit` from 1 to 100 (default 100) for page size and `--offset` to request a later page; a nonzero offset requires the preceding page's `--generation`. A change view returns changed symbols, impacted nodes and tests, plus explicit unmapped changes. It maps ranges only when the selected Git postimage matches the indexed file, and sets `complete` to false when some changes cannot be mapped. Its continuation generation binds the graph and captured Git change snapshot. Depth-one results include the selected edge on each non-seed node; full detail includes stored edge evidence. `query run impact` keeps its depth 2 default; both accept `--depth` to override it. `query tests` also accepts `--direction outbound` for dependencies of a test. `query run` accepts `overview`, `inspect`, `explain`, `impact`, `slice`, `unwired`, `sql`, `untested`, `implementations`, and `duplicates`. `query analyze` accepts an indexed path, an empty target for the workspace, or one read-only `SELECT`/`WITH` statement. Its diagnostic form computes cycles; the MCP form requires `include_cycles: true`.

The CLI `query inspect --show-source` returns the indexed symbol's source range after checking the file digest. Use `--source-offset` with `--generation` from the previous source page to continue. CLI and MCP source continuations always require that explicit token; missing or stale tokens return an error and a restart hint. Source pages stop at complete-line boundaries; an individually oversized line returns a budget error rather than partial code. See [limits and freshness](../runbooks/limits-and-freshness.md).

`--format` is global and can be placed before or after a subcommand. Markdown
uses the shared operation-aware renderer and keeps traversal evidence visible:
explain distinguishes exact calls from candidate relationships, deeper impact
paths retain their available predecessor evidence, and graph-backed test results
show ordered, linked test-to-target paths with only producer-supplied roles.
Lexical test suggestions remain explicitly unverified. JSON mode preserves the
returned semantic value for clients that parse stdout.

`query inspect` and `query explain` include paged `analysis_gaps` for the selected source span. Each gap names its reason, path, line, scope symbol, and resolver evidence. Explain edge rows retain both public endpoint IDs, kind, path, line, and direction relative to the query; full detail also includes stored evidence. Rust trait-object calls report `trait_object_dispatch`; indexed macro invocations report `macro_expansion_not_analyzed` while retaining their macro-declaration navigation edge. Exact call edges use `calls`; bounded branch-join navigation uses `possible_calls` with confidence `possible`. Inbound impact follows possible calls, and removal analysis treats candidate callers as incoming dependencies.

## Documentation and syntax search

```sh
contextunity-forge-mcp docs search 'dependency' --doc-type architecture
contextunity-forge-mcp docs get 'docs/architecture/indexing.md' --section 'Ownership and data flow'
contextunity-forge-mcp ast grep 'print($VALUE)' --lang python --path src
```

`docs` reads indexed documents. `ast grep` reads admitted file paths from the index, narrows them with FTS terms, and parses matching source after checking its indexed digest. Both can trigger the missing or incompatible-index rebuild described above; `ast grep` also requires a compiled language profile. `docs search` accepts `--component` and `--limit`; `ast grep` accepts `--limit`.

For `ast grep`, a `--limit` from 1 through 100 (default 100) returns the same
JSON `matches` page object as MCP AST search, including applicable continuation
and total-status fields. CLI limits 101..10000 retain the aggregate `matches`
array, `limit`, and `truncated` result. That larger CLI-only range is outside
shared page parity.

## Repository tasks

Use [repository tasks](tasks.md) for `task list`, `create`, `sync`, `inspect`,
`claim`, `run`, `verify`, `submit`, `blackboard`, `extend-scope`, `delete`, `reset`, `cleanup`, and the
`migrate preview/apply/verify` administration commands. These operations use
`tasks_db` from `forge-mcp.yaml`; global `--db` selects only the code index.
`task submit --evidence` accepts a JSON object with claim-bound identity and
typed proof. The `contextunity-forge` skill completes a task with:

CLI task `--worktree` inputs are existing directory paths; the MCP
human-friendly exact-name, numeric-prefix, suffix, and substring selectors do
not apply to CLI commands. CLI query commands choose a workspace with `--root`.

```sh
contextunity-forge-mcp task list [--repository NAME|all] [--milestone REF] [--status STATUS] [--stage STAGE] [--milestone-status active|planned|completed|all] [--planned|--completed|--all] [--detail compact|full] [--limit 1..100] [--offset N --generation TOKEN]
```

Task list defaults to ready tasks from the unique active milestone in the selected
workspace. No active milestone returns an empty page; multiple active milestones
require `--milestone REF` or an explicit `--milestone-status`. Compact subtask
references/statuses are the default. Use `--milestone-status planned|completed|all` or the
mutually exclusive `--planned`, `--completed`, and `--all` shorthands to select
other milestone sets. A targeted `--milestone REF` defaults to all milestone
statuses. Use `--full` to include subtask titles and verification evidence.
`--detail compact|full` selects the same subtask detail explicitly; use either
`--full` or `--detail` in one invocation. `--limit` accepts 1–100 and defaults
to 30. `--offset` defaults to 0; every nonzero offset requires the previous
page's `--generation`. Pagination binds the workspace, filters, and task/subtask
state and is independent of code-index changes. Responses retain whole task rows;
byte trimming reports the actual `returned`, `byte_omitted`, and `next_offset`.
Task `--status` accepts `ready` (default), `in_progress`, `blocked`, `parked`,
`completed`, `cancelled`, or `all`. Use `--status parked` to list tasks retaining their worker
and worktree while waiting for a scope owner; `--status all` includes them.

```sh
contextunity-forge-mcp task claim TASK_ID --stage deliver --worker REVIEWER --worktree PATH [--bundle]
contextunity-forge-mcp task submit TASK_ID --stage deliver --action pass --evidence '<JSON_OBJECT>'
contextunity-forge-mcp task context TASK_ID
```

Verify a claimed command gate iteratively, then submit its cached proof:

```sh
contextunity-forge-mcp task verify TASK_ID [--test <file.rs::test_fn>] [--seam <target>] [--timeout SECONDS]
contextunity-forge-mcp task submit TASK_ID --stage build --action pass [--timeout SECONDS]
```

`verify` executes the registered command in the claimed worktree and preserves
the task stage. When `--test` is provided, it narrows execution strictly to the
specified test target using the profile's registered command and `scope_args_by_pattern`.
When optional `--seam` is provided alongside `--test`, it verifies CodeMap reachability
to the seam target; when omitted, the test function existence is verified without requiring
a static reachability edge into production scope.
Submission without `--evidence` reuses fresh claim-bound proof
or executes the command when needed. Delivery infers proof from the accepted
candidate snapshot without running a command. Review, seam-test, and schema-defined
proofs require explicit evidence.
`--timeout` overrides the profile's command timeout within its configured maximum
and conflicts with `--evidence`. See [command verification](tasks.md#iterative-command-verification)
for timeout, scope-warning, and diagnostic behavior.

`task show` is an alias for `task inspect`. Inspect, show, and submit accept
`--task TASK_ID` in place of the positional task ID. `task sync` accepts
`--file PATH` in place of its positional milestone reference. Supply one selector
form per invocation:

```sh
contextunity-forge-mcp task show --task TASK_ID
contextunity-forge-mcp task submit --task TASK_ID --stage build --action pass --evidence '<JSON_OBJECT>'
contextunity-forge-mcp task sync --file docs/milestones/010-example.md
```

Dispatch the highest-priority eligible gate atomically:

```sh
contextunity-forge-mcp task next --worker WORKER --worktree PATH [--workspace WS]
```

The command returns the claim and context bundle, or `status: "idle"` when no
work is eligible. Independent delivery precedes review, then the worker's
unblocked parked work, then ordinary ready work by wave and task ID. A parked
task resumes in its preserved worktree. See [task dispatch](tasks.md#five-flat-mcp-tools).

`task claim` includes context by default. Context follows the active gate:
contract/build receive symbols and test seams; review receives the candidate
snapshot; delivery receives snapshot and milestone references; completed tasks
include the receipt. See
[task context](tasks.md#unified-task-context-bundle) for fields and limits.

### Headless task execution

```sh
contextunity-forge-mcp --root PATH task run TASK_ID --in-place
```

`task run` executes the task's pinned profile in the selected root. `--in-place`
explicitly selects this behavior; the command creates no worktree or branch.
It resumes the persisted applicable gate, follows profile transitions and
conditions, and returns the completed task with its durable receipt. Repeating
the command for a completed task returns that receipt without rerunning gates.

Supply the coordinator's genuine session through `FORGE_AGENT_SESSION_ID`.
Each configured role runner receives a
separate session selected before its gate claim; that same identity is passed
in its arguments and child environment for submission. The runner receives the
prepared role prompt and complete claim context. A role takes precedence when
a gate declares both a role and a command. Role runners submit their own typed
or observed evidence through normal task operations; process success alone
does not prove a gate passed. Child output streams to stderr, preserving stdout
for the final Markdown or JSON response. A role process has a one-hour timeout,
including output drainage.

Command-only gates execute registered verification and submit claim-bound
proof automatically. Delivery gates use the accepted candidate and the existing
receipt/commit path, including configured reviewer separation, immutable
candidate checks and subtask validation. Every subtask must be completed with
nonblank verification evidence or explicitly cancelled. Incomplete subtasks,
missing evidence and candidate drift retain their normal delivery errors.

Accepted rejections follow the profile's routing and attempt limits. Roles with
`reuse_on_reject` enabled resume the accepted target-stage worker and session,
with structured rejection findings in the next claim context. Missing retained
identity fails closed. Process failure, timeout, missing accepted submission,
unconfigured runners and exhausted attempts stop execution. Active foreign
claims remain unchanged; an already claimed mechanical gate resumes only for
the same coordinator, session and worktree. Use task inspection to diagnose a
stopped run before performing the normal task recovery operations.

### Task response format

Task commands print the shared lifecycle Markdown by default. It keeps the
current task status, dependencies, requested subtask detail, active guidance,
inspect history, findings, candidate bindings, and receipts; use `--format json`
when a script needs the semantic response. See the
[task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle).

These commands deliver one task. `milestone handoff` below closes the whole
milestone. Exchange temporary task context with:

```sh
contextunity-forge-mcp task blackboard post [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--recipient-task-id TASK_ID] [--subtask-ref REF] --topic TOPIC --payload TEXT [--author WORKER]
contextunity-forge-mcp task blackboard read [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] [--topic TOPIC] [--limit N] [--offset N]
contextunity-forge-mcp task blackboard inspect MESSAGE_ID
```

Post accepts a task ID as a positional source scope or resolves context from
`--scope`, `--milestone-ref`, and `--subtask-ref`; `--recipient-task-id` independently
addresses a target task in that milestone. The stored message keeps the source
`task_id` and target `recipient_task_id` separate. Post returns a message ID. An
omitted author uses the resolved task owner or `cli`. Read resolves the same
three levels and returns newest-first, payload-free summaries with pagination
metadata. The default page contains 10 messages and the maximum is 50.
`inspect MESSAGE_ID` searches configured task workspaces and returns the first
matching message including its payload.

Markdown reads group messages by their actual topic and retain newest-first
order within each group. Blockers preserve message identity, author, scope, gate,
timestamp, and available body. Payload-free summaries include the exact inspect
request.

With scope and keys omitted, blackboard commands select the unique in-progress
task; multiple in-progress tasks fail closed. If none is in progress, they fall
back to the unique active milestone and fail closed if it is missing or
ambiguous. Explicit milestone scope includes only milestone-level messages.

Manage iterative subtasks within an admitted task:

```sh
contextunity-forge-mcp task subtask add TASK_ID SUBTASK_REF "Description of subtask" [--workspace WS]
contextunity-forge-mcp task subtask update TASK_ID SUBTASK_REF --status ready|in_progress|completed|cancelled|blocked [--evidence "Test notes"] [--workspace WS]
contextunity-forge-mcp task subtask list TASK_ID [--workspace WS]
contextunity-forge-mcp task reset TASK_ID
contextunity-forge-mcp task reopen TASK_ID
contextunity-forge-mcp task context TASK_ID
contextunity-forge-mcp task extend-scope TASK_ID PATH1 [PATH2...]
contextunity-forge-mcp task delete TASK_ID [--force]
contextunity-forge-mcp task cleanup
```

Subtasks default to `ready`; legacy `pending` aliases `ready` and responses use
the canonical spelling. `parked` is valid only for top-level tasks. Cancelled
subtasks remain visible without satisfying task dependencies. Task cancellation
is imported from a task's `status: cancelled` specification through `task sync`;
see [task lifecycle](tasks.md#subtasks-and-iterative-deepening) for retention and reopening.

Subtasks allow tracking fine-grained discoveries, checklists, and verification steps
without altering the parent task contract digest or inflating the milestone queue.
Resetting or reopening a completed task clears terminal receipt state from both
SQLite and milestone Markdown, resets the task to the first gate in its active profile
(default ID: `contract`), and retains existing subtask history so new work and audits can proceed.

`task extend-scope TASK_ID PATH...` dynamically extends the admitted write scope of an in-progress task within repository perimeters. `task delete TASK_ID [--force]` removes a task from SQLite (`--milestone REF` deletes all tasks for a milestone; `--force` bypasses confirmation). `task cleanup` purges expired tasks and orphaned checkpoint refs.

## Structured plans

```sh
contextunity-forge-mcp --root PATH plan list
contextunity-forge-mcp --root PATH plan show NAME
contextunity-forge-mcp --root PATH --format json plan run NAME --in-place
contextunity-forge-mcp --root PATH --config config/custom.yaml --profile profiles/plan.yaml plan run NAME
contextunity-forge-mcp --root PATH plan clear NAME
contextunity-forge-mcp --root PATH plan archive NAME
```

Plans are Markdown documents with `doc_type: plan`, discovered recursively in
configured `plans` directories (default `docs/plans`), excluding `archive`.
Select a unique filename stem or discovered root-relative path. `list` and
`show` perform no gate execution. Discovery rejects symlinks and escaping paths.
`show` exposes validated steps, scopes, statuses and dependency edges.

`plan clear <selector>` purges Git working snapshot references (`refs/forge/snapshots/{namespace}/plan-*`) and deletes SQLite execution traces matching literal `path#` and `slug#`, leaving the plan Markdown manifest untouched.

`plan archive <selector>` enforces ADR 0005 symlink confinement on destination ancestor directories (`docs/plans/archive`), fails closed if the destination archive file already exists (`PLAN_ARCHIVE_EXISTS`), prunes snapshot references and traces, and renames the plan manifest into `docs/plans/archive/`.

Use numbered step headings (`## 1. Step name`) and explicit bullet fields:
`- **Target**: goal`, `- **Scope**: ["src/module.rs"]`,
`- **Status**: ready`, and optional `- **Depends On**: ["Prior step name"]`.
Dependencies name step titles without the numeric heading prefix. Optional
`Evidence` records verification; `Subtasks` is an inline YAML list using
`subtask_ref`, `title`, `status`, and `evidence`. Duplicate titles, unknown
references, dependency cycles, unsafe scopes and blocked steps fail before
execution. Cancelled steps cannot satisfy dependencies.

`plan run` always executes in-place; `--in-place` explicitly selects the same
mode. The default projects the selected profile's task gates to `save: snapshot`.
An explicit profile must already contain only task gates and use snapshot
persistence; root gates and commits are refused. Steps follow the canonical
pass/fail graph, conditions and retry limits. Agent roles return typed decisions;
command gates collect actual process observations. Immutable gates reject
candidate drift. Delivery reuses the task subtask guard: unfinished subtasks fail
with `TASK_SUBTASKS_INCOMPLETE`, and completed subtasks with blank evidence fail
with `TASK_SUBTASK_EVIDENCE_MISSING`; cancelled subtasks are permitted.

Successful steps update source `Status` and `Evidence` and retain named Git
snapshots, including configured pre/post gate snapshots. Execution preserves
HEAD and staged content, creates no worktree or task registry, and refuses
concurrent plan-source changes. A failed snapshot rolls back only this runner's
unchanged source update. Completed steps with evidence are not replayed. JSON
results include the path, steps and snapshots. A failure retains work for repair.

## Standalone workflows

```sh
contextunity-forge-mcp --root PATH workflow list
contextunity-forge-mcp --root PATH workflow show NAME
contextunity-forge-mcp --root PATH --format json workflow run NAME
contextunity-forge-mcp --root PATH workflow clear NAME
```

Workflows are validated profile documents in `docs/workflows/*.yaml` or `*.yml`.
Select a workflow by its unique filename stem or discovered root-relative path.
Discovery is read-only; missing, ambiguous, or escaping paths fail.

`workflow clear <selector>` resolves the workflow name or path (failing closed with `WORKFLOW_AMBIGUOUS` on multiple candidates), verifies that `.forge/runs` and its contents contain no symlinks (ADR 0005), deletes run logs matching `{slug}.log` or timestamp-delimited `{slug}-<timestamp>.log` / `{slug}_<timestamp>.log` while strictly preserving neighbor logs, and purges SQLite execution traces matching `{slug}#`.

`workflow run` executes command-owned and agent-owned root gates in-place with `save: none`.
It follows the profile's pass, failure, condition, and retry edges, drains both
process streams, and terminates the owned POSIX process group on timeout.
Each gate can set `max_attempts`; a gate without an explicit retry allowance
executes at most once. Failed or exhausted gates prevent successor execution.
Output beyond the bounded capture fails proof collection.
Successful JSON results contain `status: completed`, the selected `path`, and
in-memory gate `outcomes`. Execution creates no task store, Git repository,
snapshot, commit, or persistent execution log.

Agent gates require a configured role runner. Prompts carry the profile, gate,
goal, scope, actual independent session and structured findings. Runners send
progress to stderr and exactly one JSON decision to stdout: `gate`, `session_id`,
`decision` (`pass` or `reject`), typed `proof`, and structured `findings` on
rejection. Exit zero without a decision cannot pass. Declared independence is
checked against observed predecessor sessions; configured reuse resumes the
rejected target's session with findings. Typed command proof must agree with
commands Forge actually observes. The workflow document supplies its own graph
and roles.

Task containers, task receipts and persistence other than `none` are refused.
Command-only gates with unsupported expected-failure, policy or independence
requirements fail closed with `WORKFLOW_PROOF_REQUIRED`. Runner-invoked operations
retain their own effects.

## Repository milestones

`milestone init`, `draft`, `list`, `show`, `handoff-context`, `handoff`, `clear`, and `db-recover` are CLI-only. The MCP interface
provides task operations and has no milestone lifecycle tools.

```sh
contextunity-forge-mcp milestone list [--archive] [--limit N] [--status planned|active|completed|cancelled|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone init <manifest.md> [--in-place]
contextunity-forge-mcp milestone run <id-or-number> --target-base <git-selector> [--in-place]
contextunity-forge-mcp milestone draft [--num 011] [--slug short-name] [--title "Title"] [--plan docs/plans/proposal.md] [--dir docs/milestones] [--desc "Purpose"] [--depends-on m-prior] [--active]
contextunity-forge-mcp milestone handoff-context <id-or-number> --target-base <git-selector>
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <git-selector>] --evidence-file <external-review.json>
contextunity-forge-mcp milestone clear <id-or-number>
contextunity-forge-mcp milestone db-recover
```

`milestone list` renders Markdown sections for active and planned milestones,
with status, start time, completion, clickable document paths, and next-step
commands. Completed and cancelled sections appear when matching documents are
included. `--format json` preserves the `milestones` array and its row fields;
top-level `documents` contains aligned path coordinates, and `archive_total` /
`archive_displayed` report matching versus displayed archived documents.
The default list reads current milestone files; `--archive` includes archived
files, and `--status completed|cancelled|all` also includes archives.
`--limit` bounds only the archived portion: default 10, clamped to 1..=100,
selected in configured document path order. Active and planned documents are not
limited. Archive completion is completed task blocks / all task blocks, including
cancelled tasks in the denominator. These task states come from fenced YAML
`task_ref` blocks, not frontmatter counters or SQLite. Current document progress
continues to use SQLite. Markdown reports the archive counts and suggests
`milestone show <ID> --full`, `task list --milestone <ID> --status all`, and
archive/status filters. Status values are `planned`, `active`, `completed`,
`cancelled`, and `all`. `milestone show`
resolves a full ID, tagged prefix (e.g. `P-001`, `EC-001`), or numeric file prefix and returns frontmatter, outcomes, and task metadata. `--full` includes
the complete Markdown document.

`milestone init <manifest.md>` validates the existing milestone manifest and
repository declarations before creating task state or worktrees. It checks every
`required_skills` entry against repository and host skill installations and
rejects same-wave task scopes that overlap. On success it synchronizes tasks,
stores the synthesized contract matrix, and posts a bounded summary to the
milestone blackboard. Without `--in-place`, it prepares a
`milestone/<id>` branch and `.worktrees/<id>` checkout in each declared
repository. `--in-place` keeps the current checkout and still runs every
validation and persistence step. Preflight errors use the stable
`MILESTONE_PREFLIGHT_FAILED` diagnostic.

`milestone run` validates topological task waves and scope collisions before
dispatch. It uses the existing task lifecycle and accepted receipts, waits for
completion of each wave, and refuses cancelled dependencies. `--in-place`
serializes writers in the current checkout. The default requires a clean
integrated checkout, creates an owned task worktree from its current HEAD,
fast-forwards each delivered commit before the next task, and removes only
successfully integrated owned worktrees. Failure preserves the branch/worktree;
an active claim blocks provisioning another checkout until recovered.
On restart, a completed delivery with a commit must be an ancestor of the
integrated HEAD before it can satisfy a wave or dependency barrier, including
when resuming with `--in-place`. `MILESTONE_DELIVERY_NOT_INTEGRATED` identifies
the task, delivery commit and preserved worktree. Reconcile that accepted
delivery into the integrated checkout before resuming; the runner preserves
the existing history, worktree and proofs.

Root gates follow the selected canonical graph around the task container.
Post-task command gates execute their exact registered verification cursors.
Bound observations in existing transition records support restart without
replaying accepted tasks or root decisions; changed authority, profile or
candidate content requires new observations.
Pre-task observations include committed and non-ignored uncommitted content.
Task delivery changes that content, so a restart observes pre-task gates again
while retaining completed task receipts and avoiding new claims for those tasks.
Root role runners return typed
proof and actual session identity. An independent integration reviewer also
returns `integration_review` and the exact supplied `binding`, with its actual
session as `worker_id`. Forge verifies session separation from accepted builders.

The runner calls `handoff-context`, validates observed verification counts and
current-content independent review, then invokes the normal handoff pipeline.
A graph that skips independent review fails with `HANDOFF_REVIEW_REQUIRED`;
stale bindings or failed verification cannot archive the milestone. Configure
review and verification on the successful route. This command archives the
milestone; it does not land its branch on main.

`milestone draft` creates a numbered file in a configured milestone directory.
`--dir` selects the destination; without it, a plan beside a `milestones` directory
selects that directory, and other invocations use the first resolved configured directory
in path order.
An omitted `--num` selects the largest current or archived number in the destination
plus ten, padded to at least three digits. `--plan` imports plan metadata and notes. Piped stdin
supplies a description and task blocks. Planned milestones omit `started_at`;
`--active` records the current time. The command returns the file path and
task sync and claim guidance.
For compatibility, `milestone init` without a manifest path continues to accept
the former numbered-draft options; new draft workflows use `milestone draft`.

`milestone handoff-context` prepares canonical content and accepted-task proof
identities for external review and verification against an explicit target base.
For initial handoff, `milestone verify <ID> --target-base <BASE>` runs the first
applicable registered root command and records its transition and trace. The
submitted verification command and applicable counts must match that observation.
`milestone handoff --evidence-file` validates the resulting typed JSON record,
requires every task to be completed with accepted proofs or identically cancelled
in the manifest and task store without a completion receipt, plus a clean
committed implementation, and archives the
milestone with the supplied evidence. The independent reviewer differs from all
completed-task builders. Cancelled tasks retain their history without completion
credit or implementation-scope permission. The manual `handoff` command consumes
external review and verification evidence, and runs any selected registered
project or lint commands; `milestone run` executes the configured roles and
verification before invoking that same pipeline. Validated journal or archive
retries use the [recovery rules](tasks.md#milestone-cli-lifecycle).

See [integration evidence](tasks.md#milestone-cli-lifecycle) for the exact fields,
content projection, proof retention, diagnostics, and optional commit pointers.
Rewriting branch SHAs alone does not invalidate unchanged content evidence.
Changed final or target-base content needs fresh review and verification. An
optional `--commit` resolves a Git selector whose content must match the current
clean worktree HEAD. Commit the archived manifest and working-file removal after
handoff; that archive commit is not recorded in its own receipt.

`milestone db-recover` enrolls healthy active SQL or completes a committed pending
checkpoint publication without replaying its mutation. With healthy SQL, it also
automatically rewinds orphaned intents to their prior acknowledged keeper and
releases interrupted submit claims without accepting their provisional proofs.
Archives without `handoff.proof_shas` are obsolete and skipped.
For missing/corrupt SQL,
it discovers linked worktrees/configured roots, preseeds archive tombstones, and
imports validated active task keepers. It preserves original main/WAL/SHM bytes
and reports their backup directory. Unknown/index/future stores, intents without
healthy SQL authority, incomplete cohorts and missing legacy checkpoints fail closed. See
[active recovery](tasks.md#active-task-checkpoints-and-database-recovery) for
acknowledgment fencing, healthy-first enrollment and recovery limits.

`milestone clear <id-or-number>` acquires an exclusive physical database fence (`tasks.receipt.lock`), checks that no active checkpoints are pending publication (`TASK_CHECKPOINT_PENDING`), resolves the milestone (failing closed with `MILESTONE_AMBIGUOUS` on multiple candidates), deletes SQLite tasks strictly scoped to the milestone prefix (`substr(task_id, 1, length(?1)) = ?1`), purges blackboard entries, provenance records (`command_execution_proofs`, `gate_execution_attempts`, `gate_execution_runs`), gate execution traces, and gate transitions, and prunes scoped Git state references (`refs/forge/state/<namespace>/<id>/*`) via atomic `git update-ref --stdin`.

## Universal clear dispatch

```sh
contextunity-forge-mcp --root PATH clear milestone <id-or-number>
contextunity-forge-mcp --root PATH clear plan <selector> [--archive]
contextunity-forge-mcp --root PATH clear workflow <selector>
```

`clear` routes lifecycle cleanup across milestones, plans, and workflows under a unified top-level command.
- `clear milestone <id-or-number>`: Invokes milestone clear to purge tasks, blackboard messages, gate execution traces, provenance records, and Git state refs.
- `clear plan <selector> [--archive]`: Clears working snapshots and traces; pass `--archive` to also relocate the manifest to `docs/plans/archive/`.
- `clear workflow <selector>`: Invokes workflow clear to purge run logs and traces.

## Server, guide, and checkpoints

```sh
contextunity-forge-mcp --root /path/to/repository serve
contextunity-forge-mcp reload
contextunity-forge-mcp guide init
contextunity-forge-mcp guide query
contextunity-forge-mcp guide lifecycle --format json
contextunity-forge-mcp guide gates
contextunity-forge-mcp guide subtasks
contextunity-forge-mcp guide code
contextunity-forge-mcp guide docs
contextunity-forge-mcp checkpoint save --name repository/project/milestone/tasks/task/build --content '{"status":"in-progress"}'
contextunity-forge-mcp checkpoint get --name repository/project/milestone/tasks/task/build
contextunity-forge-mcp checkpoint list
contextunity-forge-mcp checkpoint delete --name repository/project/milestone/tasks/task/build
```

Markdown checkpoint lists show returned names and serialized byte sizes. Save
and delete show the actual action and name; get preserves the saved JSON value,
including user fields that resemble Forge metadata.

`serve` uses stdin and stdout for MCP messages; the [MCP setup guide](mcp-setup.md) covers client configuration and automatic indexing. On Unix, `SIGHUP` replaces the server process with the executable currently installed at its path while preserving its PID and standard streams. On Linux, `reload` signals running `serve` processes owned by the current user and launched from the same executable path; it reports the signaled PIDs or fails when none are found. Start `serve` with this version once before using `reload`, then run `reload` after installing subsequent replacement binaries. An in-flight MCP request can be interrupted during replacement. The replacement restores that process's negotiated client handshake, so the client retries the interrupted request on the same connection.

`guide query` lists available topics. `lifecycle`, `acdd`, `gates`, `subtasks`,
`code` and `docs` provide steps, MCP/CLI examples and references; `ast`, `adapter`,
`init` and `validate` remain available. The CLI and MCP share guide content,
formatting and capability filtering. Disabled topics return empty guidance;
unknown topics fail. Explicit `--config` controls the selected module policy.
Omitted format returns Markdown; `--format json` returns structured guidance.
See the [topic and capability reference](mcp-tools.md#built-in-guide).

`guide ast` prints profile capabilities and wildcard conventions. `guide init`
creates `forge-mcp.yaml`, requires `--force` to replace it, and can bootstrap a
workspace with an invalid existing adapter. `guide validate` scans using the
selected adapter. Checkpoints accept valid JSON state addressed by the full gate
breadcrumb and persist in the selected task-store project's SQLite database.

## Validation and migration

```sh
contextunity-forge-mcp validate [PATH]
contextunity-forge-mcp migrate preview [MILESTONE_REF]
contextunity-forge-mcp migrate apply [MILESTONE_REF]
contextunity-forge-mcp migrate verify [MILESTONE_REF]
```

- `validate [PATH]`: Validates an explicit ACDD profile or milestone manifest document against schema and topological constraints. When `PATH` is omitted, scans and validates the active profile (`.forge/acdd/profile.yaml`) and all open milestone manifests across configured milestone directories.
- `migrate preview [MILESTONE_REF]`: Inspects milestone contract structure and reports required schema migrations without mutating files. When `MILESTONE_REF` is omitted, previews all discovered milestones.
- `migrate apply [MILESTONE_REF]`: Reconciles and upgrades milestone Markdown manifests to the current contract schema.
- `migrate verify [MILESTONE_REF]`: Verifies that milestone manifests conform strictly to current ACDD contract requirements.
