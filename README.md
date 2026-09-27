# ContextUnity Forge MCP

ContextUnity Forge MCP is a local code graph and documentation server for AI coding agents. One Rust executable indexes supported source files and Markdown into SQLite, then exposes 15 tools over the Model Context Protocol (MCP). The graph is static evidence: dynamic calls and code outside the indexed roots can remain unresolved.

## Install and connect

From this checkout, install the binary with a Rust toolchain and a C compiler:

```sh
cargo install --path . --locked
command -v contextunity-forge-mcp
```

The default build covers Python, Rust, TypeScript/JavaScript, Vue, Protocol Buffers, and Markdown. To include the [optional language profiles](docs/reference/languages.md), install with `cargo install --path . --locked --features all-languages`.

Register the installed binary as a **stdio MCP server**. For VS Code or GitHub Copilot, save this portable configuration as `.mcp.json` in the repository you want to index; other MCP clients use their own configuration location. Replace both absolute paths:

```json
{
  "mcpServers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "/absolute/path/to/contextunity-forge-mcp",
      "args": ["--root", "/absolute/path/to/repository", "serve"]
    }
  }
}
```

Use the path printed by `command -v contextunity-forge-mcp` for `command`. Restart or reconnect the MCP client after changing its configuration. The server communicates through standard input and output; it needs no separate database service. [VS Code's MCP configuration reference](https://code.visualstudio.com/docs/agents/reference/mcp-configuration) documents the portable format and client-specific alternatives.

**The first database-backed MCP tool call builds the index automatically.** No `build` command or adapter initialization is needed for MCP use. The index lives at `<repository>/.forge/code-map.sqlite`. On later reads, the server checks admitted files and indexing settings, then applies a delta or rebuilds when needed. Start with `code_map_overview` to confirm the workspace, then call `code_map_search` with `{"pattern":"parse*"}` and `code_map_inspect` with a returned symbol ID. Use `code_map_impact` with `{"selector":"path/to/file.py:parse","depth":1}` for direct incoming dependencies. In agent chat, try “Find the parser entry point and show its callers” or “Which tests cover this function?”; the agent can combine search, inspection, impact, and test mapping.

## Workspace adapter

The optional `<repository>/forge-mcp.yaml` adapter controls what is indexed and how much MCP output is returned. Without it, Forge scans the workspace root, honors Git ignore rules, and excludes common build and cache directories. For example:

```yaml
roots:
  - src
doc_roots:
  - docs
ignore:
  - target
  - node_modules
linked_workspaces:
  - name: shared-library
    path: ../shared-library
    enabled: true
    roots:
      - src
response:
  page_size: 30
  max_output_bytes: 65536
```

`roots` and `doc_roots` are paths within the repository; `ignore` matches exact file or directory names. Enabled linked workspaces contribute to the same graph. Changing indexing scope triggers a rebuild on the next MCP database read. Response settings change presentation without rebuilding. `contextunity-forge-mcp --root /absolute/path/to/repository guide init` can create a starter adapter; it is optional and will not replace an existing file unless passed `--force`. See [configuration](docs/reference/configuration.md) for the full contract.

## Command-line use

The CLI reads the index already on disk. Build it explicitly when using the CLI without MCP, and rebuild or run `delta` after source changes:

```sh
cd /absolute/path/to/repository
contextunity-forge-mcp scan .
contextunity-forge-mcp build .
contextunity-forge-mcp query overview
contextunity-forge-mcp query search 'parse*'
contextunity-forge-mcp query inspect 'src/module.py:parse'
```

See the [CLI reference](docs/reference/cli.md) for all arguments.

## MCP tool map

| Tool | What it does |
| --- | --- |
| `code_map_overview` | Shows indexed components, languages, counts, and coverage. |
| `code_map_search` | Finds code symbols by full-text term or prefix pattern. |
| `code_map_inspect` | Resolves one selector and shows its definition and immediate evidence. |
| `get_code_snippet` | Reads a bounded, digest-checked source preview around a symbol. |
| `code_map_explain` | Shows direct inbound and outbound relationships for a symbol. |
| `code_map_impact` | Traverses incoming dependencies to estimate change impact. |
| `code_map_tests` | Finds tests of a target or dependencies used by a test. |
| `code_map_prove_removal` | Checks indexed callers and unresolved coverage before removal; static evidence only. |
| `code_map_query` | Routes graph operations such as `slice`, `unwired`, and a small Cypher subset. |
| `code_map_analyze` | Shows diagnostics and optional cycles, or runs bounded read-only SQL. |
| `ast_grep_search` | Matches a syntax pattern in admitted source for a selected language. |
| `search_docs` | Searches indexed Markdown sections with document-type filters. |
| `get_doc` | Retrieves an indexed document or an exact heading section. |
| `session_checkpoint` | Saves, lists, reads, or deletes local JSON session checkpoints. |
| `forge_guide` | Returns built-in guidance on queries, configuration, and recovery. |

Use a symbol ID returned by search, or a selector such as `src/module.py:parse` or `src/module.py#L12`. Ambiguous bare names return an error. Paged results default to 30 items and are bounded to 64 KiB; continue with the returned offset and generation. See the [MCP tool reference](docs/reference/mcp-tools.md) for exact fields and limits.

## Performance and comparison

One local comparison used a repository with 79 files and 1,342 indexed nodes:

| Measured operation | Forge | `codebase-memory-mcp` | Ratio in this run |
| --- | ---: | ---: | ---: |
| Cold indexing | 557 ms | 7,635 ms (fast mode) | 13.7× |
| One-shot CLI search | 6 ms | 3,968–5,965 ms | 661–994× |
| Recorded distribution size | 13.5 MB binary | 280 MB bundle | — |

The CLI search figures include process startup and do not describe warm MCP tool-call latency. Binary and bundle sizes are not identical packaging units. Hardware and the competitor build version were not recorded, so these figures are a local snapshot rather than a cross-version performance guarantee.

Forge uses a repository-local SQLite index, direct stdio MCP serving, compile-time language profiles, and bounded responses. [`codebase-memory-mcp`](https://github.com/DeusData/codebase-memory-mcp) currently uses a shared coordination daemon for MCP sessions and offers a browser UI and broader language and type-resolution features; its ordinary one-shot CLI runs locally without that daemon. Both products use SQLite. The supplied 10,815 MB figure for the competitor is a configured memory budget, so it is not compared with measured RAM consumption.

## Documentation

The [documentation index](docs/README.md) groups the full reference, indexing architecture, language support, and operating limits.
