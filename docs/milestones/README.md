---
title: "Milestones"
doc_type: guide
---

# Milestones

Milestone commitment files in this directory (`0XX-*.md`) define the repository's execution queue and lifecycle commitments.

## Ownership and Truth

- **Single Source of Truth:** Each milestone file (`docs/milestones/0XX-*.md`) owns its status (`active`, `planned`, `completed`, or `cancelled`), invariants, task breakdown, and acceptance outcomes. A cancelled milestone records a non-empty `closure.reason`.
- **Queue Order:** Numeric filename prefixes (`010-*.md`, `020-*.md`, ...) supply the queue execution order.
- **Archive:** Completed and cancelled milestones are retained in [archive/](archive/README.md) with their durable receipts and rationale. Sync prunes operational task state for cancelled milestones.

To find the active commitment, discover the lowest-numbered file with `status: active` in this directory or run `contextunity-forge-mcp milestone list`. A `planned` milestone whose `depends_on` predecessor is still active stays blocked.

Only this overview README is admitted for documentation indexing; individual milestone contracts remain separate from `doc_search`. See [ADR 0014](../adr/0014-milestone-task-subtask-hierarchy-and-visibility.md) for lifecycle, task visibility, and coordination invariants.

## Milestone Taxonomy and Naming Groups

Admitted milestones use decade prefixes `01x`–`05x`:
- `0X0` is the foundational contract for that domain.
- `0X1`–`0X9` are later slices in the same domain.
- Milestone files own lifecycle state. This page records domain boundaries and does not name the active milestone.

### Domains

- `01x` — Task lifecycle, ACDD gates, worktree automation, and agent coordination.
- `02x` — Language semantics, Tree-sitter parsers, AST extractors, and linker resolution.
- `03x` — Performance hardening, cold-build throughput, SQLite compaction, and latency budgets.
- `04x` — Engine architecture, modularity traits (`LanguageLinker`), framework manifests, and universal AST search.
- `05x` — Agent ergonomics, MCP response compaction, token efficiency, and optical context hypotheses.
