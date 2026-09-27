# CLI Reference

`contextunity-forge-mcp` provides a full-featured CLI surface alongside its stdio MCP server.

## Global Options

- `--root <PATH>`: Workspace root path (defaults to current working directory or `FORGE_WORKSPACE_ROOT`).
- `--db <PATH>`: Database path (defaults to `<root>/.forge/code-map.sqlite` or `FORGE_DB`).
- `--verbose`: Enable verbose logging.
- `-h, --help`: Print help.
- `-V, --version`: Print version.

## Commands

### `serve`
Start the stdio Model Context Protocol (MCP) server. Standard input/output are reserved for JSON-RPC MCP frames.
```sh
contextunity-forge-mcp serve
contextunity-forge-mcp --root /path/to/repo serve
```

### `build`
Perform a full cold build of the code graph database from scratch.
```sh
contextunity-forge-mcp build . --verbose
contextunity-forge-mcp build /path/to/repo --output /path/to/custom.sqlite
```

### `scan`
Scan admitted files and report file count, byte totals, and excluded paths without writing to database.
```sh
contextunity-forge-mcp scan . --pretty
```

### `delta`
Incrementally update the index for a list of modified, added, or deleted files.
```sh
contextunity-forge-mcp delta . src/main.rs src/utils.rs
```

### `query`
Execute graph inspection and analysis queries against the indexed SQLite database.

```sh
# Workspace overview
contextunity-forge-mcp query overview

# Symbol inspection
contextunity-forge-mcp query inspect 'src/scanner.rs:scan'
contextunity-forge-mcp query inspect 'Scanner' --show-doc --show-source

# Symbol search
contextunity-forge-mcp query search 'Token*' --kind function --limit 30

# Incoming dependency impact (blast radius)
contextunity-forge-mcp query impact 'src/scanner.rs:scan' --depth 2

# Symbol context and relationships
contextunity-forge-mcp query explain 'src/scanner.rs:scan'

# Test relationships
contextunity-forge-mcp query tests 'src/scanner.rs:scan' --direction inbound
contextunity-forge-mcp query tests 'tests/test_scanner.rs' --direction outbound

# Removal safety analysis
contextunity-forge-mcp query remove 'src/legacy.rs:deprecated_fn'

# Subgraph slice or Cypher query
contextunity-forge-mcp query run slice 'src/scanner.rs:scan' --depth 2 --limit 50
contextunity-forge-mcp query run unwired --limit 50
contextunity-forge-mcp query run cypher 'MATCH (n:function) RETURN n' --limit 50

# Diagnostics & SQL analysis
contextunity-forge-mcp query analyze '' --include-cycles
contextunity-forge-mcp query analyze src/scanner.rs
contextunity-forge-mcp query analyze 'SELECT kind, count(*) FROM nodes GROUP BY kind'
```

### `docs`
Search and retrieve indexed Markdown documentation, contracts, and ADRs.
```sh
# Full-text documentation search
contextunity-forge-mcp docs search 'Fail-Closed invariant' --doc-type architecture

# Read specific document section
contextunity-forge-mcp docs get 'docs/architecture/code-quality.md' --section 'Fail-Closed'
```

### `ast`
Search Tree-sitter AST syntax patterns across code without executing source.
```sh
contextunity-forge-mcp ast grep 'def $NAME($$$ARGS):' --lang python --path src/
```

### `checkpoint`
Manage local intermediate session checkpoints in `.forge/checkpoints.json`.
```sh
contextunity-forge-mcp checkpoint list
contextunity-forge-mcp checkpoint save --name 'task-step' --content '{"step": 3, "status": "verified"}'
contextunity-forge-mcp checkpoint get --name 'task-step'
contextunity-forge-mcp checkpoint delete --name 'task-step'
```

### `guide`
Display interactive usage guidance, adapter configuration templates, and validation reports.
```sh
contextunity-forge-mcp guide init
contextunity-forge-mcp guide adapter
contextunity-forge-mcp guide docs
contextunity-forge-mcp guide validate
contextunity-forge-mcp guide query
```
