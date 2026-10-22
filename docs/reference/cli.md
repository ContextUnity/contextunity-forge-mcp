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
| `--db <PATH>` | Index path for commands that read or write the database. Defaults to `<root>/.forge/code-map.sqlite`; `FORGE_DB` can set it. |
| `--format markdown|json` | Output format for command results. Defaults to `markdown`; invalid values fail before a command runs. |

`build`, `scan`, and `delta` also accept a positional workspace path. Either form selects the workspace; when both are supplied, they must resolve to the same directory.

## Inventory and indexing

```sh
contextunity-forge-mcp scan .
contextunity-forge-mcp build .
contextunity-forge-mcp build /path/to/repository --output /path/to/index.sqlite
contextunity-forge-mcp delta . src/module.py src/other.py
```

`scan` reports admitted files without writing an index. `build` creates a full index. `delta` updates the listed changed or deleted paths in an existing index. `build` and `scan` accept `--adapter <PATH>` for a configuration file inside the workspace; normal reads use the workspace's default adapter file.

`query`, `docs`, and `ast grep` open the index through `open_or_rebuild`: a missing or schema-incompatible index is built automatically. An existing index is not refreshed just because source files changed; run `build` or `delta` to update those results.

## Queries

```sh
contextunity-forge-mcp query overview
contextunity-forge-mcp query search 'parse*' --kind function --limit 30
contextunity-forge-mcp query inspect 'src/module.py:parse' --show-source
contextunity-forge-mcp query explain 'src/module.py:parse'
contextunity-forge-mcp query impact 'src/module.py:parse' --depth 1
contextunity-forge-mcp query impact --changes staged --depth 1
contextunity-forge-mcp query tests 'src/module.py:parse' --direction inbound
contextunity-forge-mcp query remove 'src/module.py:parse'
contextunity-forge-mcp query analyze 'SELECT name FROM nodes ORDER BY name'
contextunity-forge-mcp query run slice 'src/module.py:parse' --depth 1 --limit 30
contextunity-forge-mcp --format json query overview
```

`query impact` defaults to one edge and accepts exactly one symbol selector or `--changes staged|unstaged|head`; `--path` can restrict a change view to a workspace-relative path. Use `--limit` from 1 to 100 (default 100) for page size and `--offset` to request a later page; a nonzero offset requires the preceding page's `--generation`. A change view returns changed symbols, impacted nodes and tests, plus explicit unmapped changes. It maps ranges only when the selected Git postimage matches the indexed file, and sets `complete` to false when some changes cannot be mapped. Its continuation generation binds the graph and captured Git change snapshot. Depth-one results include the selected edge on each non-seed node; full detail includes stored edge evidence. `query run impact` keeps its depth 2 default; both accept `--depth` to override it. `query tests` also accepts `--direction outbound` for dependencies of a test. `query run` accepts `overview`, `inspect`, `explain`, `impact`, `slice`, and `unwired`. `query analyze` accepts an indexed path, an empty target for the workspace, or one read-only `SELECT`/`WITH` statement. Its diagnostic form computes cycles; the MCP form requires `include_cycles: true`.

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
`claim`, `submit`, `blackboard`, `extend-scope`, `delete`, `reset`, `cleanup`, and the
`migrate preview/apply/verify` administration commands. These operations use
`tasks_db` from `forge-mcp.yaml`; global `--db` selects only the code index.
`task submit --evidence` accepts a JSON object with claim-bound identity and
typed proof. The `contextunity-forge` skill completes a task with:

CLI task `--worktree` inputs are existing directory paths; the MCP
human-friendly exact-name, numeric-prefix, suffix, and substring selectors do
not apply to CLI commands. CLI query commands choose a workspace with `--root`.

```sh
contextunity-forge-mcp task list [--repository NAME|all] [--milestone REF] [--status STATUS] [--stage STAGE] [--milestone-status active|planned|completed|all] [--planned|--completed|--all] [--full]
```

Task list defaults to tasks from active milestones and compact subtask
references/statuses. Use `--milestone-status planned|completed|all` or the
mutually exclusive `--planned`, `--completed`, and `--all` shorthands to select
other milestone sets. A targeted `--milestone REF` defaults to all milestone
statuses. Use `--full` to include subtask titles and verification evidence.
`--detail compact|full` selects the same subtask detail explicitly; use either
`--full` or `--detail` in one invocation.
Task `--status` accepts `ready` (default), `in_progress`, `blocked`, `parked`,
`completed`, `cancelled`, or `all`. Use `--status parked` to list tasks retaining their worker
and worktree while waiting for a scope owner; `--status all` includes them.

```sh
contextunity-forge-mcp task claim TASK_ID --stage deliver --worker REVIEWER --worktree PATH [--bundle]
contextunity-forge-mcp task submit TASK_ID --stage deliver --action pass --evidence '<JSON_OBJECT>'
contextunity-forge-mcp task context TASK_ID
```

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
Task commands print the shared lifecycle Markdown by default. It keeps the
current task status, dependencies, requested subtask detail, active guidance,
inspect history, findings, candidate bindings, and receipts; use `--format json`
when a script needs the semantic response. See the
[task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle).

These commands deliver one task. `milestone handoff` below closes the whole
milestone. Exchange temporary task context with:

```sh
contextunity-forge-mcp task blackboard post [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] --topic TOPIC --payload TEXT [--author WORKER]
contextunity-forge-mcp task blackboard read [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] [--topic TOPIC] [--limit N] [--offset N]
contextunity-forge-mcp task blackboard inspect MESSAGE_ID
```

Post accepts a task ID as a positional argument or resolves context from
`--scope`, `--milestone-ref`, and `--subtask-ref`; it returns a message ID. An
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

## Repository milestones

`milestone init`, `list`, `show`, `handoff-context`, and `handoff` are CLI-only. The MCP interface
provides task operations and has no milestone lifecycle tools.

```sh
contextunity-forge-mcp milestone list [--archive] [--status planned|active|completed|cancelled|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone init [--num 011] [--slug short-name] [--title "Title"] [--plan docs/plans/proposal.md] [--dir docs/milestones] [--desc "Purpose"] [--depends-on m-prior] [--active]
contextunity-forge-mcp milestone handoff-context <id-or-number> --target-base <git-selector>
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <git-selector>] --evidence-file <external-review.json>
```

`milestone list` returns a table and structured rows containing ID, title,
status, `started_at`, task states, and the SQLite completion ratio. The default
list reads current milestone files; `--archive` includes archived files, and
`--status completed|cancelled` includes matching archived files. Status values
are `planned`, `active`, `completed`, `cancelled`, and `all`. `milestone show`
resolves a full ID, tagged prefix (e.g. `P-001`, `EC-001`), or numeric file prefix and returns frontmatter, outcomes, and task metadata. `--full` includes
the complete Markdown document.

`milestone init` creates a numbered file in a configured milestone directory.
`--dir` selects the destination; without it, a plan beside a `milestones` directory
selects that directory, and other invocations use the first resolved configured directory
in path order.
An omitted `--num` selects the largest current or archived number in the destination
plus ten, padded to at least three digits. `--plan` imports plan metadata and notes. Piped stdin
supplies a description and task blocks. Planned milestones omit `started_at`;
`--active` records the current time. The command returns the file path and
task sync and claim guidance.

`milestone handoff-context` prepares canonical content and accepted-task proof
identities for external review and verification against an explicit target base.
`milestone handoff --evidence-file` validates the resulting typed JSON record,
requires every task to be completed with accepted proofs or identically cancelled
in the manifest and task store without a completion receipt, plus a clean
committed implementation, and archives the
milestone with the supplied evidence. The independent reviewer differs from all
completed-task builders. Cancelled tasks retain their history without completion
credit or implementation-scope permission. Forge launches no agents and runs no
verification commands.

See [integration evidence](tasks.md#milestone-cli-lifecycle) for the exact fields,
content projection, proof retention, diagnostics, and optional commit pointers.
Rewriting branch SHAs alone does not invalidate unchanged content evidence.
Changed final or target-base content needs fresh review and verification. An
optional `--commit` resolves a Git selector whose content must match the current
clean worktree HEAD. Commit the archived manifest and working-file removal after
handoff; that archive commit is not recorded in its own receipt.

## Server, guide, and checkpoints

```sh
contextunity-forge-mcp --root /path/to/repository serve
contextunity-forge-mcp reload
contextunity-forge-mcp guide init
contextunity-forge-mcp guide query
contextunity-forge-mcp checkpoint save --name review --content '{"status":"in-progress"}'
contextunity-forge-mcp checkpoint get --name review
contextunity-forge-mcp checkpoint list
contextunity-forge-mcp checkpoint delete --name review
```

Markdown checkpoint lists show returned names and serialized byte sizes. Save
and delete show the actual action and name; get preserves the saved JSON value,
including user fields that resemble Forge metadata.

`serve` uses stdin and stdout for MCP messages; the [MCP setup guide](mcp-setup.md) covers client configuration and automatic indexing. On Unix, `SIGHUP` replaces the server process with the executable currently installed at its path while preserving its PID and standard streams. On Linux, `reload` signals running `serve` processes owned by the current user and launched from the same executable path; it reports the signaled PIDs or fails when none are found. Start `serve` with this version once before using `reload`, then run `reload` after installing subsequent replacement binaries. An in-flight MCP request can be interrupted during replacement. The replacement restores that process's negotiated client handshake, so the client retries the interrupted request on the same connection. `guide` accepts `init`, `adapter`, `docs`, `acdd`, `query`, `ast`, and `validate`. `guide ast` prints the registered profile capability matrix and wildcard conventions. `guide init` creates `forge-mcp.yaml` and requires `--force` to replace it. Checkpoint content must be valid JSON; entries live in `.forge/checkpoints.json`.
