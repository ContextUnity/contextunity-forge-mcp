# ContextUnity Forge MCP

A high-performance native code graph, documentation engine, and Model Context Protocol (MCP) server written in pure Rust.

One standalone binary scans your codebase using Tree-sitter, links symbols and architectural invariants into an embedded SQLite database, and exposes 15 specialized MCP tools for AI coding agents and developers.

> [!NOTE]
> **Alpha Version (v0.2.0-alpha)**: ContextUnity Forge MCP is currently in active alpha development. There is no pre-packaged installer yet. Build directly from source using the Rust toolchain.

---

## Highlights

- **Pure Rust & Native Performance**: No Python runtime, external database server, or system SQLite required. Sub-second cold indexing and millisecond-level incremental updates.
- **15 Specialized MCP Tools**: High-level semantic tools for repository overview, symbol search, AST previews, incoming blast-radius analysis, test mapping, and removal proofs.
- **Resilient & Collision-Free**: Prevents semantic hijacking (bare symbol names are never confused with modules), supports `path:symbol` and line coordinates (`path:line`), and filters Markdown documentation to dedicated tools.
- **Fail-Closed Security**: Strict 2.0-second SQLite timeouts, 64 KiB output ceilings, and automatic blocking of SQL write injections and path traversal attempts.
- **Multi-Workspace & Worktree Linking**: Index multiple repositories or sibling worktrees into a single unified code graph via `forge-mcp.yaml`.

---

## Building from Source

### Prerequisites
- Rust 1.80+ (`cargo`, `rustc`)
- C compiler (`gcc` or `clang` for bundled SQLite and Tree-sitter)
- Linux / Unix environment

### Build & Install
```sh
git clone https://github.com/ContextUnity/contextunity-forge-mcp.git
cd contextunity-forge-mcp
cargo build --release

# Copy or install to your PATH:
install -m 755 target/release/contextunity-forge-mcp ~/.local/bin/contextunity-forge-mcp
```

By default, the binary compiles with support for **Python, Rust, TypeScript, JavaScript, Vue, Protocol Buffers, and Markdown**.
To enable optional languages (Go, Java, C#, C, C++, PHP, Ruby):
```sh
cargo build --release --features all-languages
```

---

## Quickstart (CLI)

```sh
cd /path/to/your/project

# 1. Initialize configuration and inspect file inventory
contextunity-forge-mcp guide init
contextunity-forge-mcp scan .

# 2. Build the code graph database (.forge/code-map.sqlite)
contextunity-forge-mcp build . --verbose

# 3. Query the index
contextunity-forge-mcp query overview
contextunity-forge-mcp query search 'Token*' --kind function
contextunity-forge-mcp query inspect 'src/auth.py:login'
contextunity-forge-mcp query impact 'src/auth.py:login' --depth 1

# 4. Start the stdio MCP server
contextunity-forge-mcp serve
```

For complete CLI options and subcommands, see [docs/cli-reference.md](docs/cli-reference.md).

---

## Repository Adapter (`forge-mcp.yaml`)

The `forge-mcp.yaml` adapter file defines indexing boundaries, exclusions, and linked sibling repositories. Place it in your project root:

```yaml
# Source directories to index
roots:
  - src
  - packages

# Documentation directories (Markdown / MDX)
doc_roots:
  - docs

# Basename exclusions
ignore:
  - target
  - node_modules
  - .venv
  - dist

# Index sibling repositories or worktrees into a single unified code graph
linked_workspaces:
  - name: shared-contracts
    path: "../shared-contracts"
    enabled: true     # Set to false to disable without deleting the configuration
    roots:
      - src
    doc_roots:
      - docs

  - name: extra-library
    path: "../extra-library"
    enabled: false    # Quickly toggle off when not needed
    roots:
      - lib
```

- **`enabled: true/false`**: Toggle linked workspaces on or off with a single line. Modifying this automatically refreshes the code graph on the next query.
- **Fail-Open Resilience**: If a linked worktree is temporarily deleted or unmounted, Forge skips it gracefully without crashing or corrupting the local database.

---

## MCP Client Configuration

To connect Forge MCP to your AI agent or IDE, add it to your client's MCP configuration:

### Claude Desktop
Add to `~/.config/Claude/claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "contextunity-forge": {
      "command": "contextunity-forge-mcp",
      "args": ["--root", "/path/to/project", "serve"]
    }
  }
}
```

### Antigravity (Google DeepMind)
Tool definitions and instructions are installed in:
`~/.gemini/antigravity-cli/mcp/contextunity-forge/`

In `~/.gemini/antigravity-cli/mcp/contextunity-forge/config.json`:
```json
{
  "mcpServers": {
    "contextunity-forge": {
      "command": "contextunity-forge-mcp",
      "args": ["--root", "/path/to/project", "serve"]
    }
  }
}
```

### Cursor
Add to `.cursor/mcp.json` or `~/.cursor/mcp.json`:
```json
{
  "mcpServers": {
    "contextunity-forge": {
      "command": "contextunity-forge-mcp",
      "args": ["--root", "/path/to/project", "serve"]
    }
  }
}
```

### VS Code
Add to `.vscode/mcp.json`:
```json
{
  "servers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "contextunity-forge-mcp",
      "args": ["--root", "/path/to/project", "serve"]
    }
  }
}
```

### Zed
Add to Zed's `settings.json`:
```json
{
  "context_servers": {
    "contextunity-forge": {
      "command": "contextunity-forge-mcp",
      "args": ["--root", "/path/to/project", "serve"]
    }
  }
}
```

---

## 15 Specialized MCP Tools

| Category | Tool | Purpose |
|---|---|---|
| **Discovery & Search** | `code_map_overview` | Workspace hierarchy, component list, indexing statistics, and unresolved imports. |
| | `code_map_search` | High-speed FTS5 and prefix symbol search (`pattern="Token*"`). |
| | `ast_grep_search` | Structural Tree-sitter AST syntax matching with `$NAME` and `$$$ARGS` captures. |
| | `search_docs` | Full-text search across Markdown documentation, contracts, and ADRs. |
| | `get_doc` | Retrieve exact Markdown documents or header sections (`path_or_id`, `section`). |
| **Inspection & Context** | `code_map_inspect` | Detailed symbol definition, signature, docstring, and architectural invariants. |
| | `get_code_snippet` | Bounded AST preview (5 leading + 35 body lines) without reading entire files. |
| | `code_map_explain` | Direct call hierarchy: symbol callers (inbound) and dependencies (outbound). |
| | `code_map_impact` | Recursive incoming dependency tracing (blast radius). Always start with `depth=1`. |
| | `code_map_tests` | Discover tests exercising a symbol (`inbound`) or dependencies of a test (`outbound`). |
| **Safety & Verification** | `code_map_prove_removal` | Verify whether a symbol or module can be safely deleted without broken references. |
| | `code_map_analyze` | Diagnostics summary (`target=''`), per-file issues, cycles check, or read-only SQL. |
| | `code_map_query` | Universal query router: `slice`, `unwired`, or `cypher` (`MATCH (n) RETURN n`). |
| | `session_checkpoint` | Save, inspect, and restore agent working memory in `.forge/checkpoints.json`. |
| | `forge_guide` | Interactive guidance for query routing, adapter config, and budget recovery. |

---

## Resilient Selector Syntax

Forge resolves selectors intelligently across all query tools:
- **Canonical ID**: `function:src/scanner.rs:42:scan` (exact single-node lookup).
- **File Path**: `src/scanner.rs` (automatically resolves to the module node; strips `file://`, `file:`, `./`).
- **Path + Symbol**: `src/scanner.rs:scan` or `src/scanner.rs::scan` (resolves the inner symbol, never colliding with the module).
- **Path + Line**: `src/scanner.rs:42` or `src/scanner.rs#L42` (resolves to the innermost AST function/method enclosing that line).
- **Bare Names**: `scan` (searches for symbol; if ambiguous between a module and function, prompts with exact candidate IDs).
- **Markdown Handling**: Passing a `.md` path to code tools directs callers to `get_doc` or `search_docs`.

---

## Session Checkpoints

The `session_checkpoint` tool provides local, non-destructive memory storage for AI coding agents:
- Checkpoints are saved in `.forge/checkpoints.json` inside the workspace root.
- They allow agents to record intermediate decisions, task state, or discovery checkpoints across turns without creating Git commits or polluting repository files.
- Keys are sanitized with strict path-traversal guards (`../../etc/passwd` is rejected).

---

## Documentation & Skills

- **[docs/cli-reference.md](docs/cli-reference.md)**: Full command-line interface guide.
- **[docs/operational-boundaries.md](docs/operational-boundaries.md)**: Timeouts, memory ceilings, and performance targets.
- **[docs/language-profiles.md](docs/language-profiles.md)**: Language extraction boundaries and AST profiles.
- **[docs/language-support.md](docs/language-support.md)**: Supported syntax features and extraction limits.
- **[docs/context-protection-plan.md](docs/context-protection-plan.md)**: LLM context protection and token management.
- **Agent Skill**: Available at [`.agents/skills/contextunity-forge/SKILL.md`](.agents/skills/contextunity-forge/SKILL.md) and [`skills/contextunity-forge/SKILL.md`](skills/contextunity-forge/SKILL.md).

---

## License

Apache-2.0 or MIT.
