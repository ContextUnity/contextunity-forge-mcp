---
doc_type: guide
title: Documentation
---

# Documentation

The [repository README](../README.md) explains how the code map, the documentation index, and the task queue work together, and where an agent receives a bounded slice of context. These pages are the contracts behind that explanation. Read [documentation instructions](AGENTS.md) before editing them.

If you are connecting a client, start with [MCP setup](reference/mcp-setup.md): install the binary, point it at the workspace, and check the first index. The adapter fields behind that setup — roots, linked repositories, response size — are in [configuration](reference/configuration.md).

If you are planning work, read [ACDD](reference/acdd.md) for the loop, [task operations](reference/tasks.md) for claims, evidence, and receipts, and [ACDD profiles](reference/profiles.md) for gates, roles, and pins on a milestone or a task. Admitted work lives in [milestones](milestones/README.md). Proposals that are not yet admitted live in [plans](plans/README.md). [Roadmap](roadmap.md) is the short strategic note above both.

If you are writing pages Forge should retrieve, follow [documentation instructions](AGENTS.md). Headings become searchable sections. A backticked symbol becomes an edge to code, and inspect returns that section with the symbol.

If you are reading or extending the graph, the [MCP tools](reference/mcp-tools.md) reference and the [CLI](reference/cli.md) reference name every call. [Languages](reference/languages.md) lists compiled profiles and the static limits of each one. [Framework manifests](reference/framework-manifests.md) explains project rule files and what it takes to add a language. [Indexing](architecture/indexing.md) and [engine modularity](architecture/modularity.md) describe how extraction, linking, and storage fit together.

If you are installing skills, the [skills index](../skills/README.md) lists `contextunity-forge` and the companion skills, and what to put in `AGENTS.md`.

Operational ceilings, freshness, and what a failed query looks like are in [limits and freshness](runbooks/limits-and-freshness.md). Verification commands are in [TESTS.md](../TESTS.md) and the [testing guide](testing/README.md).

## Reference

- [MCP setup](reference/mcp-setup.md)
- [CLI](reference/cli.md)
- [MCP tools](reference/mcp-tools.md)
- [Configuration](reference/configuration.md)
- [ACDD](reference/acdd.md)
- [ACDD profiles](reference/profiles.md)
- [Repository tasks](reference/tasks.md)
- [Languages](reference/languages.md)
- [Framework manifests](reference/framework-manifests.md)

## Architecture

- [Architecture index](architecture/README.md)
- [Indexing and resolution](architecture/indexing.md)
- [SQLite concurrency](architecture/concurrency.md)
- [Engine modularity](architecture/modularity.md)
- [Performance guidance](architecture/performance.md)
- [Architectural decisions](adr/README.md)

## Direction

- [Roadmap](roadmap.md)
- [Plans](plans/README.md)
- [Milestones](milestones/README.md)
- [Historical archive](archive/README.md)

Current milestone and plan directories are the `milestones` and `plans` entries in `forge-mcp.yaml`. Individual plan and milestone contracts are excluded from documentation search. Their README pages stay indexed.

For a code change, start with the workspace overview, select an exact symbol, inspect direct relationships, and verify the relevant source. Missing edges are bounded by the indexed languages, roots, and unresolved-reference coverage.
