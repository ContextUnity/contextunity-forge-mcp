---
doc_type: guide
title: Documentation
---

# Documentation

Use the [repository README](../README.md) for installation and a first query.
Read [documentation instructions](AGENTS.md) before editing these pages.

## Reference

- [MCP setup](reference/mcp-setup.md): install, connect a client, select the workspace, and verify the first index.
- [CLI](reference/cli.md): commands, arguments, and examples.
- [MCP tools](reference/mcp-tools.md): all 15 tools, selectors, paging, and a discovery workflow.
- [Configuration](reference/configuration.md): indexing roots, linked workspaces, and response settings.
- [Languages](reference/languages.md): compiled profiles, extensions, and static-analysis limits.

## Architecture

- [Indexing and resolution](architecture/indexing.md): source inventory, language profiles, linking, persistence, and query boundaries.
- [Architectural decisions](adr/README.md): accepted decision ownership.

## Operations and verification

- [Limits and freshness](runbooks/limits-and-freshness.md): budgets, recovery steps, source verification, and failure behavior.
- [Testing](testing/README.md): test authority and verification commands.

## Direction and work

- [Roadmap](roadmap.md): strategic context.
- [Plans](plans/README.md): ongoing designs, research, and proposals.
- [Milestones](milestones/README.md): admitted commitment ownership.
- [Historical archive](archive/README.md): retained historical material.

Current contracts and milestones are indexed through explicit doc_roots in
forge-mcp.yaml. Plans and general historical archives are read as files;
completed milestone receipts remain within the indexed milestone root.

For a code change, start with the workspace overview, select an exact symbol, inspect direct relationships, and verify the relevant source. Missing edges are bounded by the indexed languages, roots, and unresolved-reference coverage.
