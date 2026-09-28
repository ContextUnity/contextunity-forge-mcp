---
doc_type: api
title: CLI reference
---

# CLI reference

`contextunity-forge-mcp` provides a command-line interface and a stdio MCP server. Run `contextunity-forge-mcp --help` or append `--help` to any command for accepted arguments. Commands other than `serve` print JSON results.

## Global options

| Option | Behavior |
| --- | --- |
| `--root <PATH>` | Workspace for `serve`, `query`, `docs`, `ast`, `guide`, and `checkpoint`. Defaults to `.`; `FORGE_WORKSPACE_ROOT` can set it. |
| `--db <PATH>` | Index path for commands that read or write the database. Defaults to `<root>/.forge/code-map.sqlite`; `FORGE_DB` can set it. |

`build`, `scan`, and `delta` also accept a positional workspace path. Their path, when supplied, determines the workspace they operate on.

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
contextunity-forge-mcp query analyze src/module.py
contextunity-forge-mcp query run slice 'src/module.py:parse' --depth 1 --limit 30
contextunity-forge-mcp query run cypher 'MATCH (n:function) RETURN n' --limit 30
```

`query tests` also accepts `--direction outbound` for dependencies of a test. `query run` accepts `overview`, `inspect`, `explain`, `impact`, `slice`, `unwired`, and `cypher`; `raw_cypher`, `doctor`, and `search` are accepted aliases. Pass a Cypher page limit with `--limit`, outside the query string. `query analyze` accepts an indexed path, an empty target for the workspace, or one read-only `SELECT`/`WITH` statement. Its diagnostic form computes cycles; the MCP form requires `include_cycles: true`.

The CLI `query inspect --show-source` returns the indexed symbol's full source range after checking the file digest. MCP source previews are bounded; see [limits and freshness](../operations/limits-and-freshness.md).

## Documentation and syntax search

```sh
contextunity-forge-mcp docs search 'dependency' --doc-type architecture
contextunity-forge-mcp docs get 'docs/architecture/indexing.md' --section 'Ownership and data flow'
contextunity-forge-mcp ast grep 'print($VALUE)' --lang python --path src
```

`docs` reads the index. `ast grep` scans admitted source directly and requires a compiled language profile. `docs search` accepts `--component` and `--limit`; `ast grep` accepts `--limit`.

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
