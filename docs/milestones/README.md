---
title: "Milestones"
doc_type: guide
---

# Milestones

Milestone commitment files in this directory (`0XX-*.md`) define the repository's execution queue and lifecycle commitments.

## Ownership and Truth

- **Single Source of Truth:** Each milestone file (`docs/milestones/0XX-*.md`) owns its own status (`active`, `planned`, `deferred`, `completed`), invariants, task breakdown, and acceptance outcomes.
- **Queue Order:** Numeric filename prefixes (`010-*.md`, `020-*.md`, ...) supply the queue execution order.
- **Archive:** Completed milestones are moved to [archive/](archive/README.md) along with their proof receipts.

To find the active commitment, discover the lowest-numbered file with `status: active` in this directory or run `contextunity-forge-mcp milestone list`. A `planned` milestone whose `depends_on` predecessor is still active stays blocked.

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
