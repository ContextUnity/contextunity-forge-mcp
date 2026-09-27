---
doc_type: api
title: Workspace configuration
---

# Workspace configuration

`forge-mcp.yaml` is optional. Without it, Forge scans the workspace root and excludes common build and cache directories. `guide init` writes a starter file. Put the adapter in the workspace root; `build` and `scan` can select another in-root file with `--adapter`.

## Indexing scope

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
    doc_roots:
      - docs
```

`roots` and `doc_roots` are literal, relative directories or paths inside the workspace. `ignore` matches file or directory basenames. The scanner admits compiled source extensions and Markdown/MDX within these roots; scan roots do not define import or package roots. Unavailable linked workspaces are skipped. A linked entry with `enabled: false` is excluded; omission of `enabled` means true.

The index records linked paths under their configured workspace names. Changing source scope or linked-workspace settings causes the MCP server to rebuild or update its owned index on the next database read. CLI readers use the index already on disk, so run `build` or `delta` after source changes when using only the CLI.

## MCP response policy

```yaml
response:
  detail: compact
  page_size: 30
  max_output_bytes: 65536
  source_context:
    enabled_by_default: false
    leading_lines: 5
    max_body_lines: 35
```

`page_size` accepts 1–100; `max_output_bytes` accepts 1,024–65,536. Source previews accept 0–20 leading lines and 1–100 body lines. `detail` is `compact` or `full`. These settings control presentation and do not change extracted facts or require an index rebuild. Unknown response fields and out-of-range values are rejected.

See [MCP tools](mcp-tools.md) for continuation arguments and [limits and freshness](../operations/limits-and-freshness.md) for computation budgets.
