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
- [MCP tools](reference/mcp-tools.md): all 20 tools, selectors, paging, and a discovery workflow.
- [Configuration](reference/configuration.md): indexing roots, linked workspaces, and response settings.
- [Repository tasks](reference/tasks.md): claims, direct JSON evidence, blackboard messages, and receipts.
- [ACDD](reference/acdd.md): admitted task gates, independent review, and context retention.
- [Languages](reference/languages.md): compiled profiles, extensions, and static-analysis limits.

## Architecture

- [Architecture index](architecture/README.md): system structure, contracts, and performance envelopes.
- [Indexing and resolution](architecture/indexing.md): source inventory, language profiles, linking, persistence, and query boundaries.
- [SQLite concurrency](architecture/concurrency.md): reader isolation, generation locking, and WAL pragmas.
- [Engine modularity](architecture/modularity.md): `LanguageProfile`, `TemplatePreprocessor`, and `LanguageLinker` traits.
- [Performance budgets](architecture/performance.md): latency targets, FTS5 BM25 search, and storage compaction.
- [Architectural decisions](adr/README.md): accepted decision records (ADRs).

## Operations and verification

- [Limits and freshness](runbooks/limits-and-freshness.md): budgets, recovery steps, source verification, and failure behavior.
- [ACDD execution runbook](runbooks/acdd.md): the task and milestone delivery sequence.
- [Testing](testing/README.md): test authority and verification commands.

## Direction and work

- [Roadmap](roadmap.md): strategic context and macro themes.
- [Plans](plans/README.md): active designs, proposals, and pre-implementation research.
- [Milestones](milestones/README.md): admitted commitments, task execution queue, and acceptance criteria.
- [Historical archive](archive/README.md): retained historical material and completed milestone receipts.

Current contracts and milestones are configured through `milestones` in
forge-mcp.yaml. Plans remain in `docs/plans/` (configured in `plans`) for research and proposal authoring
prior to milestone admission. Both are automatically excluded from the code/documentation search index.

For a code change, start with the workspace overview, select an exact symbol, inspect direct relationships, and verify the relevant source. Missing edges are bounded by the indexed languages, roots, and unresolved-reference coverage.
