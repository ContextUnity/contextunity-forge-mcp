# ContextUnity Forge MCP

ContextUnity Forge MCP is a local code graph and documentation server for AI coding agents. It indexes supported source files and Markdown into a repository-local SQLite database and exposes **15 MCP tools** for finding symbols, tracing dependencies, reading source, and searching docs. The graph records static evidence; dynamic calls and files outside the indexed roots can remain unresolved.

## What it provides

- **Code discovery:** search symbols, inspect definitions, read bounded source previews, and match syntax patterns.
- **Dependency analysis:** inspect direct links, trace incoming impact, find related tests, and assess removal using indexed evidence.
- **Documentation search:** index Markdown and MDX headings alongside code and retrieve exact sections.
- **Local operation:** a Rust executable serves MCP over standard input and output; no separate database service or shared coordination daemon is required.
- **Bounded responses:** pages default to 30 items, MCP output is capped at 64 KiB, and graph and SQL operations have protective limits.

The default build covers Python, Rust, TypeScript/JavaScript, Vue, Protocol Buffers, HTML, YAML, TOML, and Markdown. Other language profiles are available as compile-time features; see [language support](docs/reference/languages.md).

## Install

From this checkout, install with a Rust toolchain and a C compiler:

```sh
cargo install --path . --locked
command -v contextunity-forge-mcp
```

To include every optional language profile, use `cargo install --path . --locked --features all-languages`. The installed executable is both the CLI and the MCP server.

## Connect an MCP client

**No manual indexing or configuration initialization is needed.** The first database-backed MCP request builds `.forge/code-map.sqlite` in the repository. Later reads check the file inventory and adapter settings, then apply a delta or rebuild if needed.

Register the executable as a stdio server. For VS Code or GitHub Copilot, save this portable example as `.mcp.json` at the repository root:

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

Forge uses the MCP process's current working directory as its workspace. The example works when the client starts servers in the repository directory. If the client uses another directory or cannot find the binary on `PATH`, follow the [MCP setup guide](docs/reference/mcp-setup.md) for `cwd`, absolute paths, and `--root`. Running `contextunity-forge-mcp` without a subcommand starts the stdio server; the MCP client manages its lifetime.

Reconnect the client, then ask an agent to use `code_map_overview`. That first database-backed call creates the index and reports its workspace and coverage. Typical follow-ups:

- “Find the function that parses configuration and show its source.” → `code_map_search`, `code_map_inspect`, `get_code_snippet`.
- “What calls this function and which tests cover it?” → `code_map_impact` with depth 1, then `code_map_tests`.
- “Find the documentation for this component.” → `search_docs`, then `get_doc`.

## CLI quick start

The CLI reads the existing index; build it explicitly if you use the CLI without MCP:

```sh
cd /path/to/repository
contextunity-forge-mcp scan .
contextunity-forge-mcp build .
contextunity-forge-mcp query overview
contextunity-forge-mcp query search 'parse*'
contextunity-forge-mcp query inspect 'src/module.py:parse'
```

`scan` reports the admitted files without writing an index. `build` creates a full index. After source edits, run `build` or `delta` before relying on CLI query results. See the [CLI reference](docs/reference/cli.md) for the remaining commands and options.

## Workspace adapter

The optional `forge-mcp.yaml` file belongs in the repository root. It defines source and documentation roots, exclusions, linked workspaces, and MCP response sizes. Without it, Forge scans the workspace root, respects Git ignore rules, and excludes common build and cache directories.

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

`roots` and `doc_roots` admit paths inside the workspace; `ignore` matches exact basenames. Enabled linked workspaces contribute to the same index. Changes to indexing scope trigger a rebuild on the next MCP database read; response-only changes do not. `contextunity-forge-mcp guide init` creates a starter adapter when you need one and does not replace an existing file without `--force`. The [configuration reference](docs/reference/configuration.md) covers every setting.

## How the graph is built

```text
admitted source + Markdown → language/heading extraction → linker → SQLite
                                                              ↓
                                                   CLI and bounded MCP reads
```

The scanner decides which files enter the index. Language profiles extract declarations, imports, calls, and other syntax facts; the linker connects references it can resolve and retains unresolved evidence. Markdown headings become searchable document sections. MCP checks freshness before database reads. The [indexing architecture](docs/architecture/indexing.md) explains the ownership and data flow.

Markdown and MDX files under the configured documentation roots are split at headings and stored as searchable sections with their document type and title. Code-style references in those sections are linked to matching symbols. `code_map_inspect` and `code_map_explain` include documentation linked to the selected symbol by default; set `show_doc=false` to omit it. Use `search_docs` to find relevant sections, then `get_doc` to read a document or an exact heading. Documentation is included when it is linked to the selected symbol; agents can search and retrieve other sections directly.

Graph results describe indexed, statically resolved relationships. A missing edge is not proof that a runtime call, generated file, or external caller does not exist. Check the reported path and source before making changes.

## Selectors

Use an exact symbol ID returned by `code_map_search` when available. The resolver also accepts these forms:

| Selector | Meaning |
| --- | --- |
| `src/module.py` | Module for a file path. |
| `src/module.py:parse` or `src/module.py::parse` | Symbol named `parse` inside that file. |
| `src/module.py:12` or `src/module.py#L12` | Narrowest indexed symbol covering line 12. |
| `parse` | Bare name; returns an ambiguity error when multiple targets match. |

`./`, `file:`, and `file://` path prefixes are accepted. Markdown paths belong in `get_doc` or `search_docs`. An ambiguous selector must be narrowed before impact or removal analysis.

## MCP tool map

| Group | Tool | What it does |
| --- | --- | --- |
| Discover | `code_map_overview` | Shows indexed components, languages, counts, and coverage. Start here to confirm the workspace. |
| Discover | `code_map_search` | Finds code symbols by full-text term or prefix pattern, returning IDs for exact selection. |
| Discover | `ast_grep_search` | Matches a syntax pattern in admitted source for a selected language. |
| Docs | `search_docs` | Searches indexed Markdown sections, with document-type and component filters. |
| Docs | `get_doc` | Retrieves an indexed document or an exact heading section. |
| Inspect | `code_map_inspect` | Resolves one selector and returns a compact signature/docstring, receiver-aware container, and caller/callee summary; set `include_coverage=true` for paged resolution evidence. |
| Inspect | `get_code_snippet` | Reads a bounded, digest-checked source preview around a selected symbol. |
| Relationships | `code_map_explain` | Shows a compact symbol summary and its direct inbound and outbound relationships. |
| Relationships | `code_map_impact` | Traverses inbound (default) or outbound dependencies to estimate change impact; start at depth 1. |
| Relationships | `code_map_tests` | Finds tests exercising a target or production dependencies used by a test. |
| Verify | `code_map_prove_removal` | Checks indexed callers and unresolved coverage before removal; the result covers static evidence only. |
| Query | `code_map_query` | Routes graph operations, including paged read-only SQL via `operation="sql"`. |
| Query | `code_map_analyze` | Shows diagnostics, stored syntax errors with `lint:true`, optional cycles, or bounded read-only SQL. |
| Session | `session_checkpoint` | Saves, lists, reads, or deletes local JSON checkpoints in `.forge/checkpoints.json`. |
| Help | `forge_guide` | Returns built-in guidance on queries, the adapter, and budget recovery. |

MCP pages default to 30 items and are bounded to 64 KiB. Continue a page with its returned offset and generation. Source previews are off by default; `get_code_snippet` requests a bounded preview explicitly. The [MCP tool reference](docs/reference/mcp-tools.md) lists exact request fields, direction rules, and limits.

## Documentation

- [MCP setup](docs/reference/mcp-setup.md): client configuration, working directory, first run, and troubleshooting.
- [CLI reference](docs/reference/cli.md) and [adapter configuration](docs/reference/configuration.md).
- [Language support](docs/reference/languages.md), [indexing architecture](docs/architecture/indexing.md), and [limits and freshness](docs/runbooks/limits-and-freshness.md).
- [Documentation index](docs/README.md): all pages grouped for readers and coding agents.
