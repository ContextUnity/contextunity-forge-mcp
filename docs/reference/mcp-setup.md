---
doc_type: guide
title: MCP setup
---

# MCP setup

Forge runs as a local stdio MCP server. The executable also provides CLI commands. With no subcommand, `contextunity-forge-mcp` starts the MCP server; `serve` names that mode explicitly. The server uses its process working directory as the workspace unless `--root` or `FORGE_WORKSPACE_ROOT` selects another root.

## Install the binary

Run the installer from the project directory whose `.mcp.json` should be configured:

```sh
curl -fsSL https://raw.githubusercontent.com/ContextUnity/contextunity-forge-mcp/main/install.sh | sh
```

The installer uses POSIX `sh`, ordinary shell utilities, `tar` with gzip support,
`curl` or `wget`, and `sha256sum` or `shasum`. It requires no compiler, Rust
toolchain, Python, Node, or `jq`. Supported release targets are Linux x86_64 and
aarch64, and macOS x86_64 and arm64. It downloads the latest stable release by
default and verifies the exact archive entry in `SHA256SUMS` before extracting
or changing the installation. Unsupported platforms and missing or mismatched
checksums stop installation.

Select a tagged release or nightly through its asset directory:

```sh
curl -fsSL https://raw.githubusercontent.com/ContextUnity/contextunity-forge-mcp/main/install.sh -o /tmp/forge-install.sh
FORGE_RELEASE_BASE_URL="https://github.com/ContextUnity/contextunity-forge-mcp/releases/download/<tag>" sh /tmp/forge-install.sh
# Or use .../releases/download/nightly for nightly assets.
```

`FORGE_RELEASE_BASE_URL` is an absolute release asset directory, containing
`SHA256SUMS` and `contextunity-forge-mcp-<rust-target>.tar.gz`. The archive has
the executable and companion `skills/` directory at its root. Verification
checks download integrity against that release manifest.

The binary is staged beside its destination and atomically renamed into
`~/.local/bin/contextunity-forge-mcp`. Add `~/.local/bin` to your shell `PATH`
when the installer prints the PATH notice. Client configuration uses the
absolute executable path. The bundled `skills/install.sh` refreshes
`~/.agents/skills` and links skills into detected Claude, Codex, Gemini and
Cursor homes; existing regular skill directories receive copies. Unrelated
skills remain present. The companion script also supports `--copy`,
`--dry-run`, selected skill names, and an `AGENTS_SKILLS_DIR` hub override.

The installer creates or updates the current project `.mcp.json`, setting only
the Forge server `command` and `type` fields. Other servers, root fields and
tailored Forge `args`, `env`, and `cwd` values survive. Formatting may change.
Malformed JSON, a nonobject root, nonobject `mcpServers` or Forge entry, and
symlink configuration files are rejected without overwriting configuration.

Run the same installer to update binary and companion skills. For an existing
installation, it invokes the installed binary `reload` command after atomic
replacement. On Linux and macOS, reload signals matching MCP server processes
for the same executable path and user. Both the default invocation without a
subcommand and explicit `serve` are recognized through the CLI grammar;
other CLI commands are excluded. Initialized MCP sessions retain their stdio
pipes. A notice reports when no matching server is running. The
installer preserves project adapters and does not promise a transaction across
binary, skills, and configuration changes.

To build from a Forge checkout instead:

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

All 20 MCP tools support the same `worktree` selector; it is optional except
for `task_claim` and `task_manage(action: "next")`, which require it for task
affinity. See [worktree selection](mcp-tools.md#worktree-selection) for
exact-name, prefix, zero-normalized number, suffix, substring, and path
precedence. Omitting the selector always retains the server root, regardless
of task claims. Selected worktree manifests, configuration, indexes, and
checkpoints stay local to that branch, while Git-linked worktrees use the
shared task database. Requests do not transmit the client's current directory.

The server keeps connected clients alive regardless of inactivity. On Unix it
exits when its launching parent disappears, including while awaiting MCP
initialization. SIGHUP reload preserves the established session and stdio pipes.
There is no inactivity shutdown setting.

Reconnect the client and call `code_map_overview`. The first database-backed call creates `<root>/.forge/code-map.sqlite` automatically. The result reports the indexed workspace, generation, counts, and coverage. No `guide init` or manual `build` is required for MCP use.

Before later database reads, Forge checks the adapter and admitted source inventory. Changed files trigger an incremental update or full rebuild as needed. A changed response policy affects output without reindexing. `ast_grep_search` scans admitted source directly, so use `code_map_overview` when you want to initialize or verify the SQLite index.

For a first exploration, use `code_map_search` with `{"pattern":"parse*"}`, inspect an ID returned by search, then ask `code_map_impact` with `depth=1`. The [tool reference](mcp-tools.md) lists all 20 tools and their fields.

## If the server does not appear

- **Executable not found:** run `command -v contextunity-forge-mcp` in a terminal and put that absolute path in `command`.
- **Wrong repository:** compare `code_map_overview`'s workspace with the intended root. Set `cwd` or `--root` explicitly.
- **Indexing error:** inspect the MCP client's server log and validate `forge-mcp.yaml` with `contextunity-forge-mcp guide validate` from the repository.
- **CLI results are stale:** CLI readers use the index on disk. Run `contextunity-forge-mcp build .` or `delta` after source changes; MCP reads refresh automatically.

The [adapter reference](configuration.md) covers admission rules and linked workspaces; [limits and freshness](../runbooks/limits-and-freshness.md) covers scan and response budgets.
