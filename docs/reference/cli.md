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

`query tests` also accepts `--direction outbound` for dependencies of a test. `query run` accepts `overview`, `inspect`, `explain`, `impact`, `slice`, and `unwired`. `query analyze` accepts an indexed path, an empty target for the workspace, or one read-only `SELECT`/`WITH` statement. Its diagnostic form computes cycles; the MCP form requires `include_cycles: true`.

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
`claim`, `submit`, `extend-scope`, `delete`, `reset`, `cleanup`, and the
`migrate preview/apply/verify` administration commands. These operations use
`tasks_db` from `forge-mcp.yaml`; global `--db` selects only the code index.

## Repository milestones

```sh
contextunity-forge-mcp milestone list [--archive] [--status planned|active|completed|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone init [--num 011] [--slug short-name] [--title "Title"] [--plan docs/plans/proposal.md] [--desc "Purpose"] [--depends-on m-prior] [--active]
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <full-sha>] --verification-command "cargo test --all-targets" --tests-passed <count> --tests-failed 0
```

`milestone list` returns a table and structured rows containing ID, title,
status, `started_at`, task states, and the SQLite completion ratio. The default
list reads current milestone files; `--archive` and `--status all|completed`
include archived files. `milestone show` resolves a full ID or numeric file
prefix and returns frontmatter, outcomes, and task metadata. `--full` includes
the complete Markdown document.

`milestone init` creates a numbered file under `docs/milestones/`. An omitted
`--num` selects the largest current or archived milestone number plus ten, padded
to at least three digits. `--plan` imports plan metadata and notes. Piped stdin
supplies a description and task blocks. Planned milestones omit `started_at`;
`--active` records the current time. The command returns the file path and
task sync and claim guidance.

`milestone handoff` requires every milestone task in SQLite to be completed and
the verification command to have zero failed tests. It writes `status:
completed` and a `handoff` receipt with `completed_at`, `duration`, full Git
commit, and language-neutral verification fields. It moves the file into
`docs/milestones/archive/` and updates SQLite task references. An omitted
`--commit` uses the current Git `HEAD`. See [repository tasks](tasks.md) for
task claims, receipts, and activation timing. Milestone commands are CLI-only;
the MCP task interface contains four flat tools.

## Server, guide, and checkpoints

```sh
contextunity-forge-mcp --root /path/to/repository serve
contextunity-forge-mcp guide init
contextunity-forge-mcp guide query
contextunity-forge-mcp checkpoint save --name review --content '{"status":"in-progress"}'
contextunity-forge-mcp checkpoint get --name review
contextunity-forge-mcp checkpoint list
contextunity-forge-mcp checkpoint delete --name review
```

`serve` uses stdin and stdout for MCP messages; the [MCP setup guide](mcp-setup.md) covers client configuration and automatic indexing. `guide` accepts `init`, `adapter`, `docs`, `query`, and `validate`. `guide init` creates `forge-mcp.yaml` and requires `--force` to replace it. Checkpoint content must be valid JSON; entries live in `.forge/checkpoints.json`.
