---
doc_type: api
title: CLI reference
---

# CLI reference

`contextunity-forge-mcp` provides a command-line interface and a stdio MCP server. Run `contextunity-forge-mcp --help` or append `--help` to any command for accepted arguments. Commands other than `serve` print JSON results.

## Global options

| Option | Behavior |
| --- | --- |
| `--root <PATH>` | Workspace for all commands. Defaults to the current directory; `FORGE_WORKSPACE_ROOT` can set it. |
| `--db <PATH>` | Index path for commands that read or write the database. Defaults to `<root>/.forge/code-map.sqlite`; `FORGE_DB` can set it. |

`build`, `scan`, and `delta` also accept a positional workspace path. Either form selects the workspace; when both are supplied, they must resolve to the same directory.

## Inventory and indexing

```sh
contextunity-forge-mcp scan .
contextunity-forge-mcp build .
contextunity-forge-mcp build /path/to/repository --output /path/to/index.sqlite
contextunity-forge-mcp delta . src/module.py src/other.py
```

`scan` reports admitted files without writing an index. `build` creates a full index. `delta` updates the listed changed or deleted paths in an existing index. `build` and `scan` accept `--adapter <PATH>` for a configuration file inside the workspace; normal reads use the workspace's default adapter file.

## Queries

```sh
contextunity-forge-mcp query overview
contextunity-forge-mcp query search 'parse*' --kind function --limit 30
contextunity-forge-mcp query inspect 'src/module.py:parse' --show-source
contextunity-forge-mcp query explain 'src/module.py:parse'
contextunity-forge-mcp query impact 'src/module.py:parse' --depth 1
contextunity-forge-mcp query tests 'src/module.py:parse' --direction inbound
contextunity-forge-mcp query remove 'src/module.py:parse'
contextunity-forge-mcp query analyze 'SELECT name FROM nodes ORDER BY name'
contextunity-forge-mcp query run slice 'src/module.py:parse' --depth 1 --limit 30
```

`query impact` defaults to one edge. `query run impact` keeps its depth 2 default; both accept `--depth` to override it. `query tests` also accepts `--direction outbound` for dependencies of a test. `query run` accepts `overview`, `inspect`, `explain`, `impact`, `slice`, and `unwired`. `query analyze` accepts an indexed path, an empty target for the workspace, or one read-only `SELECT`/`WITH` statement. Its diagnostic form computes cycles; the MCP form requires `include_cycles: true`.

The CLI `query inspect --show-source` returns the indexed symbol's full source range after checking the file digest. MCP source previews are bounded; see [limits and freshness](../runbooks/limits-and-freshness.md).

## Documentation and syntax search

```sh
contextunity-forge-mcp docs search 'dependency' --doc-type architecture
contextunity-forge-mcp docs get 'docs/architecture/indexing.md' --section 'Ownership and data flow'
contextunity-forge-mcp ast grep 'print($VALUE)' --lang python --path src
```

`docs` reads the index. `ast grep` reads admitted file paths from the index, narrows them with FTS terms, and parses matching source after checking its indexed digest. It requires a built index and a compiled language profile. `docs search` accepts `--component` and `--limit`; `ast grep` accepts `--limit`.

## Repository tasks

Use [repository tasks](tasks.md) for `task list`, `create`, `sync`, `inspect`,
`claim`, `submit`, `blackboard`, `extend-scope`, `delete`, `reset`, `cleanup`, and the
`migrate preview/apply/verify` administration commands. These operations use
`tasks_db` from `forge-mcp.yaml`; global `--db` selects only the code index.
`task submit --evidence` accepts a JSON object with claim-bound identity and
typed proof. The [ACDD workflow](acdd.md) completes a task with:

```sh
contextunity-forge-mcp task list [--repository NAME|all] [--milestone REF] [--status STATUS] [--stage STAGE] [--milestone-status active|planned|completed|all] [--planned|--completed|--all] [--full]
```

Task list defaults to tasks from active milestones and compact subtask
references/statuses. Use `--milestone-status planned|completed|all` or the
mutually exclusive `--planned`, `--completed`, and `--all` shorthands to select
other milestone sets. A targeted `--milestone REF` defaults to all milestone
statuses. Use `--full` to include subtask titles and verification evidence.

```sh
contextunity-forge-mcp task claim TASK_ID --stage deliver --worker REVIEWER --worktree PATH [--bundle]
contextunity-forge-mcp task submit TASK_ID --stage deliver --action pass --evidence '<JSON_OBJECT>'
contextunity-forge-mcp task context TASK_ID
```

`task claim` includes context by default. Context follows the active gate:
contract/build receive symbols and test seams; review receives the candidate
snapshot; delivery receives snapshot and milestone references; completed tasks
include the receipt. See
[task context](tasks.md#unified-task-context-bundle) for fields and limits.

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

With scope and keys omitted, blackboard commands select the unique in-progress
task; multiple in-progress tasks fail closed. If none is in progress, they fall
back to the unique active milestone and fail closed if it is missing or
ambiguous. Explicit milestone scope includes only milestone-level messages.

Manage iterative subtasks within an admitted task:

```sh
contextunity-forge-mcp task subtask add TASK_ID SUBTASK_REF "Description of subtask" [--workspace WS]
contextunity-forge-mcp task subtask update TASK_ID SUBTASK_REF --status in_progress|completed|pending [--evidence "Test notes"] [--workspace WS]
contextunity-forge-mcp task subtask list TASK_ID [--workspace WS]
contextunity-forge-mcp task reset TASK_ID
contextunity-forge-mcp task reopen TASK_ID
contextunity-forge-mcp task context TASK_ID
```

Subtasks allow tracking fine-grained discoveries, checklists, and verification steps
without altering the parent task contract digest or inflating the milestone queue.
Resetting or reopening a completed task clears terminal receipt state from both
SQLite and milestone Markdown, resets the gate to `contract/v1`, and retains existing subtask
history so new work and audits can proceed.

## Repository milestones

```sh
contextunity-forge-mcp milestone list [--archive] [--status planned|active|completed|cancelled|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone init [--num 011] [--slug short-name] [--title "Title"] [--plan docs/plans/proposal.md] [--dir docs/milestones] [--desc "Purpose"] [--depends-on m-prior] [--active]
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <full-sha>] --verification-command "cargo test --all-targets" --tests-passed <count> --tests-failed 0
```

`milestone list` returns a table and structured rows containing ID, title,
status, `started_at`, task states, and the SQLite completion ratio. The default
list reads current milestone files; `--archive` includes archived files, and
`--status completed|cancelled` includes matching archived files. Status values
are `planned`, `active`, `completed`, `cancelled`, and `all`. `milestone show`
resolves a full ID or numeric file prefix and returns frontmatter, outcomes, and task metadata. `--full` includes
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

`milestone handoff` requires every milestone task in SQLite to be completed and
the verification command to have zero failed tests. It writes `status:
completed` and a `handoff` receipt with `completed_at`, `duration`, full Git
commit, and language-neutral verification fields. It moves the file into an
`archive/` directory beside the selected milestone and updates SQLite task references. An omitted
`--commit` uses the current Git `HEAD`. See [repository tasks](tasks.md) for
task claims, receipts, and activation timing. Milestone commands are CLI-only;
the MCP task interface contains five flat tools.

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

`serve` uses stdin and stdout for MCP messages; the [MCP setup guide](mcp-setup.md) covers client configuration and automatic indexing. On Unix, `SIGHUP` replaces the server process with the executable currently installed at its path while preserving its PID and standard streams. On Linux, `reload` signals running `serve` processes owned by the current user and launched from the same executable path; it reports the signaled PIDs or fails when none are found. Start `serve` with this version once before using `reload`, then run `reload` after installing subsequent replacement binaries. An in-flight MCP request can be interrupted during replacement. The replacement restores that process's negotiated client handshake, so the client retries the interrupted request on the same connection. `guide` accepts `init`, `adapter`, `docs`, `acdd`, `query`, `ast`, and `validate`. `guide ast` prints the registered profile capability matrix and wildcard conventions. `guide init` creates `forge-mcp.yaml` and requires `--force` to replace it. Checkpoint content must be valid JSON; entries live in `.forge/checkpoints.json`.
