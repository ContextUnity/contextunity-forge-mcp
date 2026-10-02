---
doc_type: guide
title: MCP setup
---

# MCP setup

Forge runs as a local stdio MCP server. The executable also provides CLI commands. With no subcommand, `contextunity-forge-mcp` starts the MCP server; `serve` names that mode explicitly. The server uses its process working directory as the workspace unless `--root` or `FORGE_WORKSPACE_ROOT` selects another root.

## Install the binary

From the Forge checkout:

```sh
cargo install --path . --locked
command -v contextunity-forge-mcp
```

A Rust toolchain and C compiler are required to build the bundled SQLite and language grammars. Use `--features all-languages` during installation for optional language profiles. The binary must be on the MCP client's `PATH` or named by its absolute path in the client configuration.

## Configure a client

For VS Code or GitHub Copilot's portable format, put `.mcp.json` at the root of the repository to index:

```json
{
  "mcpServers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "contextunity-forge-mcp"
    }
  }
}
```

This form uses the client's workspace directory. If the client launches servers elsewhere, set the working directory explicitly when it supports `cwd`:

```json
{
  "mcpServers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "/absolute/path/to/contextunity-forge-mcp",
      "cwd": "/absolute/path/to/repository"
    }
  }
}
```

For clients without a `cwd` setting, pass the repository root to Forge:

```json
{
  "mcpServers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "/absolute/path/to/contextunity-forge-mcp",
      "args": ["--root", "/absolute/path/to/repository"]
    }
  }
}
```

`.mcp.json` uses `mcpServers`. VS Code's `.vscode/mcp.json` uses a top-level `servers` object instead. Other clients choose their own file locations and may use a different wrapper; keep the executable and workspace settings the same. See [VS Code's MCP configuration reference](https://code.visualstudio.com/docs/agents/reference/mcp-configuration) for the portable and VS Code formats.

## First request and subsequent reads

Reconnect the client and call `code_map_overview`. The first database-backed call creates `<root>/.forge/code-map.sqlite` automatically. The result reports the indexed workspace, generation, counts, and coverage. No `guide init` or manual `build` is required for MCP use.

Before later database reads, Forge checks the adapter and admitted source inventory. Changed files trigger an incremental update or full rebuild as needed. A changed response policy affects output without reindexing. `ast_grep_search` scans admitted source directly, so use `code_map_overview` when you want to initialize or verify the SQLite index.

For a first exploration, use `code_map_search` with `{"pattern":"parse*"}`, inspect an ID returned by search, then ask `code_map_impact` with `depth=1`. The [tool reference](mcp-tools.md) lists all 20 tools and their fields.

## If the server does not appear

- **Executable not found:** run `command -v contextunity-forge-mcp` in a terminal and put that absolute path in `command`.
- **Wrong repository:** compare `code_map_overview`'s workspace with the intended root. Set `cwd` or `--root` explicitly.
- **Indexing error:** inspect the MCP client's server log and validate `forge-mcp.yaml` with `contextunity-forge-mcp guide validate` from the repository.
- **CLI results are stale:** CLI readers use the index on disk. Run `contextunity-forge-mcp build .` or `delta` after source changes; MCP reads refresh automatically.

The [adapter reference](configuration.md) covers admission rules and linked workspaces; [limits and freshness](../runbooks/limits-and-freshness.md) covers scan and response budgets.
