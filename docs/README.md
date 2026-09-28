---
doc_type: guide
title: Documentation
---

# Documentation

Use the [repository README](../README.md) to build the binary and run a first query. The pages below describe the current implementation for readers and coding agents.

## Reference

- [MCP setup](reference/mcp-setup.md): install, connect a client, select the workspace, and verify the first index.
- [CLI](reference/cli.md): commands, arguments, and examples.
- [MCP tools](reference/mcp-tools.md): all 15 tools, selectors, paging, and a discovery workflow.
- [Configuration](reference/configuration.md): indexing roots, linked workspaces, and response settings.
- [Languages](reference/languages.md): compiled profiles, extensions, and static-analysis limits.

## Architecture

- [Indexing and resolution](architecture/indexing.md): source inventory, language profiles, linking, persistence, and query boundaries.

## Operations

- [Limits and freshness](operations/limits-and-freshness.md): budgets, recovery steps, source verification, and failure behavior.

For a code change, start with the workspace overview, select an exact symbol, inspect direct relationships, and verify the relevant source. Missing edges are bounded by the indexed languages, roots, and unresolved-reference coverage.
