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
docs:
  - docs
milestones:
  - docs/milestones
plans:
  - docs/plans
ignore:
  - target
  - node_modules
linked_workspaces:
  - name: shared-library
    path: ../shared-library
    enabled: true
    roots:
      - src
    docs:
      - docs
```

`roots` and `docs` are literal, relative directories or paths inside the workspace. `milestones` and `plans` default to `docs/milestones` and `docs/plans` when omitted. Their `*` segments match one directory component (for example, `extensions/*/docs/milestones`). Files below these directories are excluded from the code and documentation search index. Milestone commands read contracts from the configured milestone directories. `ignore` matches file or directory basenames. The scanner admits compiled source extensions and Markdown/MDX within these roots; scan roots do not define import or package roots. Unavailable linked workspaces are skipped. A linked entry with `enabled: false` is excluded; omission of `enabled` means true.

The default basename exclusions cover `.git`, `.forge`, `target`, `node_modules`, `.venv`, `__pycache__`, `build`, `dist`, `coverage`, `.cache`, `.next`, `.nuxt`, `.svelte-kit`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, and `.gradle`. The scanner also honors `.gitignore` and skips symbolic links. Add project-specific generated directories with `ignore`; authored source under an excluded basename can be scoped explicitly with a different directory name.

The index records linked paths under their configured workspace names. Changing source scope or linked-workspace settings causes the MCP server to rebuild or update its owned index on the next database read. CLI readers use the index already on disk, so run `build` or `delta` after source changes when using only the CLI.

## Repository documentation admission

This repository lists current documentation files and directories explicitly in
`docs`: governance, navigation, roadmap, ADRs, architecture, reference,
runbooks, and testing. Milestones and plans are declared separately in `milestones`
and `plans`, keeping them out of the general code search index.

## Source-only linked libraries

Set `ignore` inside each linked entry to exclude that library's vendor directories and build outputs. A linked entry does not inherit the target library's `forge-mcp.yaml` settings. Keep the same roots and exclusions in the library's own adapter and its consumers.

```yaml
linked_workspaces:
  - name: shared-library
    path: ../shared-library
    roots: [src, frontend, tests]
    docs: [docs]
    ignore: [vendor, vendors, dist, build]
```

Ignore names are literal basenames, not glob patterns. Add a directory such as `static` only when the library builds all of its contents from sources retained elsewhere. Keep authored host assets and source-level type contracts. Excluded bundles neither enter the index nor trigger source refresh when rebuilt.

## Task storage

Task operations read `forge-mcp.yaml`; an omitted `tasks_db` defaults to
`.forge/tasks.sqlite`. Relative paths resolve from the configuration directory.
For cross-worktree coordination, configure the same shared path locally;
tracked configuration uses a portable relative path.
`task_repository` and `task_project` select the
qualified task namespace. See [repository tasks](tasks.md) for identity,
concurrency, receipts, and administration. Code indexing works without task
storage configuration.

Set root `agents_guidance` to a path inside the root repository. It defaults to
`AGENTS.md`. Claim and inspect include that path and stage-specific
`workflow_guidance`. If the file is missing, they include inline steps, a
`TASK_GUIDANCE_MISSING` warning, and the [canonical ACDD reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md).

Linked entries opt into the primary task store with `tasks.enabled: true`.
`tasks.milestones_dir` defaults to `docs/milestones`, and `tasks.agents_guidance`
defaults to `AGENTS.md`, both relative to the linked repository root.
Omitted or disabled task entries are excluded. See [linked repository tasks](tasks.md#linked-repository-tasks)
for synchronization, namespace selection, and scope confinement.

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

The root `forge-mcp.yaml` and the file selected with CLI `--adapter` are configuration rather than indexed sources. Nested files with that name and linked-workspace adapters remain ordinary YAML sources when admitted by their configured roots. A linked workspace's own adapter does not control the parent index.

See [MCP tools](mcp-tools.md) for continuation arguments and [limits and freshness](../runbooks/limits-and-freshness.md) for computation budgets.
