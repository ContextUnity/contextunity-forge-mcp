# ContextUnity Forge MCP

A native Rust code map and documentation engine with a command line interface and a Model Context Protocol (MCP) server. One executable scans repositories, extracts syntax and documentation, links symbols, and queries bundled SQLite. Deployment requires no Python interpreter, virtual environment, database service, or separately installed SQLite.

## Build and start

Build on Linux with the Rust toolchain and a C compiler for bundled SQLite and Tree-sitter. Database identity checks currently require Unix file metadata. Build from this repository:

```sh
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

The default binary includes Python, Rust, TypeScript/JavaScript, Vue and Proto.
Other grammars are optional:

```sh
cargo build --release --locked --features lang-go
cargo build --release --locked --features all-languages
cargo build --release --locked --no-default-features --features lang-rust
```

`--no-default-features` alone builds the Markdown/document engine. See the
[profile guide](docs/language-profiles.md) for feature selection and adding a
provider. No package installation or runtime plugin loader is required.

From the repository you want to index:

```sh
contextunity-forge-mcp guide init
contextunity-forge-mcp guide validate
contextunity-forge-mcp build . --output .code-map.sqlite --verbose
contextunity-forge-mcp query overview --db .code-map.sqlite
contextunity-forge-mcp --db .code-map.sqlite serve
```

Invoking the executable without a subcommand also starts the stdio MCP server. Keep standard output reserved for protocol messages when running `serve`.

## Architecture

```text
CLI commands                         MCP stdio requests (rmcp)
     └───────────────────┬───────────────────┘
                    shared core
                         │
           adapter / workspace configuration
                         │
                scanner and content hashes
                         │
       Tree-sitter syntax + Markdown sections
                         │
              symbol and document linker
                         │
       SQLite writer / SHA-256 commitments
                         │
       read queries / recursive graph traversal
```

The CLI owns argument parsing and output. The MCP layer owns protocol transport and tool schemas. Both call the same domain operations. The scanner determines admitted files; language extractors produce definitions, imports, and calls; the linker resolves references and preserves unresolved evidence. Markdown sections supply searchable documentation and code references. SQLite stores graph rows, extraction contributions, document sections, full text search indexes, and commitments.

The database is a repository snapshot. The CLI requires an explicit build or
delta after source changes. MCP checks the source inventory before reads and
updates its owned index when source changes. Query results reflect indexed
evidence; dynamic execution, external libraries, and unindexed files require
separate inspection.

## Source coverage

| Input | Extraction boundary |
| --- | --- |
| Python (`.py`, `.pyi`) | Definitions, classes, imports, calls, attached documentation |
| TypeScript / JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`) | Functions, classes, methods, imports, calls |
| Vue (`.vue`) | JavaScript or TypeScript `<script>` blocks and basic template bindings |
| Rust (`.rs`) | Functions, types, traits, implementations, imports, calls |
| Go (`.go`) | Types, functions, methods, imports, calls |
| Markdown (`.md`, `.mdx`) | Sections, metadata, invariants, code references |
| Protocol Buffers (`.proto`) | Enums, messages, services, RPCs, imports, type references |
| Java (`.java`) | Declarations, local calls and inheritance; classpath resolution remains conservative |
| C# (`.cs`) | Declarations, local calls and base relations; assembly resolution remains conservative |
| Kotlin (`.kt`, `.kts`) | Declarations, local calls and base relations; inferred receiver types remain conservative |
| PHP (`.php`) | Declarations and supported calls/relations; autoload and dynamic dispatch remain unresolved |
| Ruby (`.rb`) | Declarations and supported calls; bare-call and dynamic forms retain unresolved evidence |
| C (`.c`, `.h`) | Declarations, local calls and include facts; preprocessing and indirect calls remain conservative |
| C++ (`.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`) | Declarations, local calls and supported bases; templates and implicit calls remain conservative |

Profiles are discovered at build time from `src/engine/languages/*.rs` and
selected by `lang-<provider>` Cargo features. A provider exports a `PROFILES`
slice; no manual Rust registry edit is required. The compiled profile
fingerprint includes enabled providers and invalidates incompatible indexes.
The table lists all available modes; optional modes require their feature.
See [language profiles and extension guide](docs/language-profiles.md) for the
contract and limits. `.h` uses the C profile when enabled.

Unsupported extensions are outside the scanner's inventory. Vue extracts component names, event handlers, bound props and interpolations from templates; styles and complex template expressions remain outside syntax extraction. MDX is treated as Markdown. The scanner bounds a file to 5 MiB, the admitted corpus to 100,000 files, and total admitted bytes to 500 MiB. Exceeding a bound is an error.

## Command line reference

Global `--root` defaults to the current directory (`FORGE_WORKSPACE_ROOT` overrides it). Global `--db` defaults to `<root>/.forge/code-map.sqlite` (`FORGE_DB` overrides it). An explicit `--db` is useful when a build uses a custom `--output`. Build, scan, and delta also accept their own positional workspace root. CLI results are JSON.

Use `contextunity-forge-mcp --help` and `<command> --help` for exact arguments and defaults. Quote selectors, text queries, and structural patterns so the shell passes them unchanged.

### Indexing

```sh
# Inspect the admitted file inventory.
contextunity-forge-mcp scan . --pretty

# Build a complete database.
contextunity-forge-mcp build . --output .code-map.sqlite --verbose

# Update explicitly changed or deleted repository-relative paths.
contextunity-forge-mcp delta . --db .code-map.sqlite src/main.rs
```

Delta reparses the listed changed files, gathers affected references from persisted dependencies, and replaces the affected files' graph contributions in one SQLite write-ahead log (WAL) transaction. Unchanged file digests are reused only when file identity and timestamps still match. Include every admitted addition, change, and deletion in the command; an unlisted source change is rejected. Changes during indexing also reject the update. Rebuild when changing adapter scope.

### Code map

```sh
contextunity-forge-mcp query overview --db .code-map.sqlite
contextunity-forge-mcp query inspect 'build' --show-doc --db .code-map.sqlite
contextunity-forge-mcp query inspect 'build' --show-source --db .code-map.sqlite
contextunity-forge-mcp query search '*Revert*' --kind function --limit 100
contextunity-forge-mcp query tests 'build'
contextunity-forge-mcp query tests 'test_build' --direction outbound
contextunity-forge-mcp query impact 'build' --depth 3 --db .code-map.sqlite
contextunity-forge-mcp query explain 'build' --db .code-map.sqlite
contextunity-forge-mcp query remove 'legacy_helper' --db .code-map.sqlite
```

Start with an overview and use returned selectors to inspect individual symbols. Inspection includes linked documentation by default. `--show-doc` enables it explicitly; `--show-doc false` disables it. Impact follows graph relationships to a bounded depth. Removal analysis reports evidence for a proposed retirement; interpret the result within index coverage.

`--show-source` adds the exact indexed line range, preserving line endings, from a local or linked workspace. Source is omitted by default. A changed, missing, or symlinked source file returns an error; update or rebuild the index before requesting source again.

Symbol search uses FTS5 for terms and simple prefixes such as `Order*`. Patterns with `*` elsewhere, such as `*Revert*`, match substrings in names and qualified names. Other characters are literal; regular expressions and typo correction are not supported. Results have a configurable limit and a `truncated` flag.

Test mapping follows static dependencies transitively with cycle detection. The default inbound direction returns test symbols that depend on the selected production symbol; outbound returns production dependencies of a selected test. Selecting a class includes its members. Containment does not connect unrelated sibling tests. Test files and Rust `#[test]`, `#[tokio::test]`, and `#[async_std::test]` functions supply test markers. Unresolved references can hide dependencies; an empty result is not proof that no tests exist.

The graph includes `inherits` from a derived class to its base, `implements` from a type to an interface or trait, `decorates` from a decorator to its decorated symbol, and `mutates` from an assignment owner to a field. These relationships participate in impact and slice queries. Python class field assignments include ORM declarations without requiring an ORM dependency.

Resolution coverage marks known external imports separately from unresolved local dependencies. Rust `std::`, `core::` and `alloc::` imports and JavaScript/TypeScript `node:` imports receive `external` only when no indexed provider matches. Unknown package names remain `unresolved`; an absolute import alone does not prove that a dependency exists outside the index. Overview and analysis report `external_imports` separately from unresolved references. Calls through external imports can remain unresolved because the external source is not indexed.

Static route patterns include HTTP decorators, `path('...', handler)`, `app.get('...', handler)` and similar HTTP methods, and `{path: '...', component: handler}` objects. Route nodes have names such as `GET /items` and a `handles` edge when the handler resolves. Registrations without an explicit HTTP method use `ANY`; Python `route(..., methods=[...])` creates one node per method. Dynamic paths and unresolved handlers require source inspection. These are syntax patterns, not runtime framework validation.

Route registrations retain all explicit handlers, including inline functions and parenthesized callbacks. Vue script blocks use absolute source coordinates. Proto RPC type references participate in dependency traversal and removal checks.

Rebuild existing indexes to populate the relationships, route nodes, test markers, and composite edge indexes. Readers check a separate index semantics version; MCP rebuilds an incompatible owned index after validating workspace ownership. Unreadable or foreign databases are rejected without replacement.

See [language support and current limits](docs/language-support.md) for verified boundaries.

### General queries and analysis

```sh
contextunity-forge-mcp query run slice 'build' --depth 2 --limit 100
contextunity-forge-mcp query run unwired --limit 100
contextunity-forge-mcp query run raw_cypher 'MATCH (n) RETURN n' --limit 100
contextunity-forge-mcp query analyze 'SELECT kind, COUNT(*) FROM nodes GROUP BY kind'
contextunity-forge-mcp query analyze src/
```

`raw_cypher` accepts a restricted set of graph shapes, not arbitrary Cypher. See operational boundaries below. SQL analysis accepts one read-only `SELECT` or `WITH` statement without a semicolon. SQL text is limited to 64 KiB, SQLite scalar values and serialized results to 8 MiB, and execution has a two-second progress budget. Queries that exceed these limits return an error. SQL analysis returns at most 1,000 rows; public `--limit` values range from 1 to 10,000. Impact and slice traversal accept depths up to 16 and return at most 1,000 nodes with a truncation indicator.

Path analysis treats path characters literally and reports total diagnostic counts and truncation. Cycle detection retains cross-file cycles, uses integer node keys internally, and rejects graphs exceeding its 500,000-edge computation budget.

### Checkpoints

```sh
contextunity-forge-mcp checkpoint save --name orientation --content '{"focus":"indexer"}'
contextunity-forge-mcp checkpoint list
contextunity-forge-mcp checkpoint get --name orientation
contextunity-forge-mcp checkpoint delete --name orientation
```

Checkpoints persist in `<root>/.forge/checkpoints.json`. Content is JSON; names contain 1–200 characters and each saved value is limited to 1 MiB.

### Documentation

```sh
contextunity-forge-mcp docs search 'Code Quality invariant' --doc-type architecture --db .code-map.sqlite
contextunity-forge-mcp docs get 'docs/architecture/code-quality.md' --section 'Fail-Closed' --db .code-map.sqlite
```

Markdown YAML frontmatter supplies document metadata. Headings split documents into sections. Inline code spans identify symbol references; supported GitHub alert blocks identify architectural invariants:

```markdown
---
title: Request handling
doc_type: architecture
---

# Request validation

> [!IMPORTANT] Invariant:
> `validate_request` runs before request dispatch.
```

### Structural search

```sh
contextunity-forge-mcp ast grep 'def $NAME($$$ARGS):' --lang python --path src/
```

Use single quotes around patterns containing `$` to prevent shell expansion. Structural matches identify syntax locations; they do not execute source code.

### Guidance

```sh
contextunity-forge-mcp guide init
contextunity-forge-mcp guide adapter
contextunity-forge-mcp guide docs
contextunity-forge-mcp guide validate
```

## Repository adapter

`forge-mcp.yaml` defines repository scope; `contextunity-forge-mcp.yaml` is also recognized. With no adapter, scanning starts at the workspace root and applies Git ignore rules and built-in exclusions. `ignore` entries match exact file or directory basenames. Root paths stay inside the workspace; parent traversal and symbolic links are rejected. When both filenames exist, `forge-mcp.yaml` takes precedence. Build and scan accept `--adapter` for an explicit workspace-local file. The guide commands print the supported template and validate configuration. Start with repository-relative source and documentation roots:

```yaml
roots:
  - src
  - tests
doc_roots:
  - docs
ignore:
  - target
  - node_modules
  - .venv
```

`eligible_roots` is an alternative to `roots`; `roots` takes precedence. `doc_roots` extends the selected roots. `excluded_directory_names` and `excluded_file_names` extend the same basename exclusion set as `ignore`. Use plain paths for roots; document root glob patterns are not expanded. Use `guide init` for the implementation's current complete template. Keep generated artifacts and dependency trees outside indexing scope. Rebuild after changing scope so the stored index and adapter describe the same repository.

### Linked workspaces and sibling worktrees

To index related workspaces (e.g. parent repository, child worktree, or shared libraries) into a single unified code map, configure `linked_workspaces`:

```yaml
roots:
  - src
doc_roots:
  - docs

linked_workspaces:
  - name: commerce
    path: /home/user/ContextUnity/worktrees/commerce-horoshop-api
    roots:
      - extensions
    doc_roots:
      - docs
  - name: gridviewspec
    path: /home/user/ContextUnity/projects/gridviewspec
    roots:
      - src
    ignore:
      - dist
```

- **Unified Graph in Local Database**: The SQLite index is stored in the local worktree (`<local-root>/.forge/code-map.sqlite`).
- **Path Namespacing**: Nodes and files from linked workspaces are prefixed as `[<workspace_name>]/<relative_path>`.
- **Cross-workspace Search**: Symbols and doc sections from all linked workspaces are searchable and inspectable.
- **Import identity**: Exact dependencies require a complete module namespace, compatible language and justified workspace. Matching a name suffix does not prove a dependency; ambiguous providers remain visible in resolution coverage. Scan roots do not define runtime import roots.
- **Fail-Open Resilience**: If a linked worktree/workspace becomes unavailable (e.g. branch worktree removed or unmounted), the indexer skips it and automatically rebuilds the database without it, eliminating crashes and stale data.

## Database verification and updates

Readers check the database schema, engine, workspace binding, and SHA-256 commitments before returning indexed data. An incompatible database requires a rebuild; the database belongs to the canonical workspace path used during indexing.

A successful build or delta can write an optional private `<database>.verified.json` verification record. It binds the verified generation to the workspace, database identity, schema, and commitment algorithm. If the record is missing, malformed, stale, or not private, readers perform full verification. A nonempty WAL also requires full verification. The record contains no source index and can be removed when diagnosing verification behavior.

The MCP server checks inventory metadata before admitting each read. Changed
file identities or timestamps trigger content hashing; changed content,
additions and deletions update the index through delta. An incompatible owned
index is rebuilt. The read and source admission share the connection lock, and
each request ends its SQLite read snapshot so subsequent requests can observe
external deltas. Unchanged reads do not rewrite the index. A failed admission
returns an error instead of presenting old metadata as current.

## MCP tools

| Tool | Purpose |
| --- | --- |
| `code_map_overview` | Repository and module overview |
| `code_map_inspect` | Symbol inspection with optional documentation |
| `get_code_snippet` | Bounded source preview with continuation for a selected symbol |
| `code_map_search` | FTS5 and wildcard symbol search with a kind filter |
| `code_map_tests` | Transitive test dependents or production dependencies |
| `code_map_impact` | Bounded dependency impact |
| `code_map_explain` | Symbol context and incoming/outgoing relationships |
| `code_map_query` | General graph query operations |
| `code_map_analyze` | Analysis entry point |
| `code_map_prove_removal` | Indexed evidence for symbol or file removal |
| `ast_grep_search` | Structural syntax search |
| `search_docs` | Documentation full text search |
| `get_doc` | Document or section retrieval |
| `forge_guide` | Onboarding, adapter guidance, query routing, and validation |
| `session_checkpoint` | Workspace checkpoint management |

MCP responses have a 64 KiB hard ceiling. Lists use compact projections and
pages of 30 items by default (maximum 100). Continue with the returned
`next_offset` and `generation`; an index change requires restarting the query.
`detail: "full"` is explicit and remains bounded. Source previews default to
five leading and 35 body lines, with omission metadata and a separate source
offset. Global analysis summarizes diagnostics; select an exact file for
detailed rows. See [context protection](docs/context-protection-plan.md) for
response examples, adapter settings and the distinction from CLI output.
`forge_guide` with `topic: "query"` provides a compact path from overview to
symbol, dependency, test, diagnostic, and document queries, with recovery hints.

## Building from Source

```sh
cargo build --release
```

The resulting standalone executable is located at `target/release/contextunity-forge-mcp`.

## MCP client configuration

Use absolute executable, workspace, and database paths for clients launched outside the repository. Replace paths below with your own. Merge the server entry into existing configuration.

### Claude Desktop

Add a server to `claude_desktop_config.json`; the Linux installer uses `~/.config/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "contextunity-forge": {
      "command": "/home/you/.local/bin/contextunity-forge-mcp",
      "args": ["--root", "/path/to/repository", "--db", "/path/to/repository/.forge/code-map.sqlite", "serve"]
    }
  }
}
```

### Cursor

Use the same `mcpServers` shape in `.cursor/mcp.json` for a project or `~/.cursor/mcp.json` for a user configuration. See [Cursor MCP configuration](https://prod.cursor.com/help/customization/mcp).

### Antigravity

The installer provisions `~/.gemini/antigravity-cli/mcp/contextunity-forge/config.json` using the `mcpServers` shape above. Confirm this file is loaded by the client distribution you use.

### VS Code

Place this in `.vscode/mcp.json`. The dedicated MCP file uses `servers` and the `stdio` type:

```json
{
  "servers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "/home/you/.local/bin/contextunity-forge-mcp",
      "args": ["--root", "/path/to/repository", "serve"]
    }
  }
}
```

See [VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration).

### Zed

Merge this into Zed's `settings.json`:

```json
{
  "context_servers": {
    "contextunity-forge": {
      "command": "/home/you/.local/bin/contextunity-forge-mcp",
      "args": ["--root", "/path/to/repository", "serve"],
      "env": {}
    }
  }
}
```

See [Zed MCP configuration](https://zed.dev/docs/ai/mcp).

## Performance targets

These are acceptance targets, not measured results. Compare release builds on a documented machine with a fixed repository snapshot and adapter. Report corpus size, node and edge counts, cold/warm state, sample count, and measurement boundaries alongside results.

| Operation | Target |
| --- | --- |
| Cold indexing, approximately 3,000 files | < 800 ms |
| Incremental update | < 20 ms |
| MCP query latency, 95th percentile | < 5 ms |
| Native process startup | < 5 ms |
| Active stdio resident memory | < 15 MB |

A shell command includes process startup; an already running MCP request measures a different boundary. Small fixture timings do not establish the 3,000-file target.

## Operational boundaries

- The index captures syntax and resolvable relationships. Dynamic dispatch, receiver typing, and external source can remain unresolved. Empty caller results alone do not establish that removal is safe.
- Delta updates affected graph contributions in a WAL transaction. Its runtime still includes repository inventory checks, dependency resolution, commitment verification, and persistence.
- Go calls through receiver variables, external source-bearing re-exports, and modules with identical language-neutral stems (for example `util.py` and `util.ts`) can remain unresolved or ambiguous. Inspect resolution diagnostics before relying on their graph edges.
- Python receiver reassignment and static methods currently have a known confidence limitation: some calls can receive an incorrect exact target. Confirm those call sites in source before using impact or removal results.
- Protocol Buffer syntax extractor captures messages, enums, services, and RPC signatures along with import edges.
- `raw_cypher` supports `MATCH (n) RETURN n`, `MATCH (n:kind) RETURN n`, and `MATCH (a)-[e]->(b) RETURN a,e,b`. It is not a general Cypher runtime.
- `code_map_query` takes `limit` as a tool argument, not a Cypher clause. Deep slices with more than 1,000 immediate links are rejected before recursion; use `depth: 1` and page direct neighbors, or select a narrower module. A smaller page limit does not reduce the recursive count. `code_map_analyze` takes `target: ""` for workspace diagnostics; cycles require `include_cycles: true`.
- Documentation search operates on indexed sections; update the index after editing Markdown.
- The standalone database format and tool surface require compatibility verification before replacing another Forge deployment.
- Successful client configuration writes do not establish that a particular client discovers or launches the server. Zed configuration is manual.
