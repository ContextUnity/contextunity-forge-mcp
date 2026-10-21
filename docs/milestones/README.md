---
title: "Milestones"
doc_type: guide
---

# Milestones

Milestone commitment files in this directory (`0XX-*.md`) define the repository's execution queue and lifecycle commitments.

## Ownership and Truth

- **Single Source of Truth:** Each milestone file (`docs/milestones/0XX-*.md`) owns its status (`active`, `planned`, `completed`, or `cancelled`), invariants, task breakdown, and acceptance outcomes. A cancelled milestone records a non-empty `closure.reason`.
- **Queue Order:** Numeric filename prefixes (`010-*.md`, `020-*.md`, ...) supply the queue execution order. Supported milestone filenames also admit tagged prefixes such as `P-001-*.md` or `EC-001-*.md`.
- **Archive:** Completed and cancelled milestones are retained in [archive/](archive/README.md) with their durable receipts and rationale. Sync prunes operational task state for cancelled milestones.

Use `contextunity-forge-mcp milestone list` to inspect queue order and dependencies.
Continue an active commitment; admit the next planned commitment when its
dependencies are satisfied. Its first accepted task claim activates it.

Milestone contracts are searchable documentation when admitted by the configured documentation roots. Search visibility does not admit a milestone or task, make a proposal an execution commitment, or change the manifest as the source of lifecycle truth. Use the task operations to inspect and change operational state. See [ADR 0014](../adr/0014-milestone-task-subtask-hierarchy-and-visibility.md) for lifecycle, task visibility, and coordination invariants.

## Milestone Taxonomy and Domain Tagging

Milestones use a 2-letter domain prefix and a 3-digit sequence number (`TAG-NNN-slug.md`), providing up to 1000 milestone slots (`001`–`999`) per domain:

| Domain Prefix | Domain Name | Scope & Responsibilities | Range |
| :--- | :--- | :--- | :--- |
| **`AC`** | **ACDD & Task Lifecycle** | Hierarchical gates, DAG orchestration, workflow governance, task store | `AC-001` .. `AC-999` |
| **`LS`** | **Language Semantics** | Tree-sitter parsers, AST extraction, declarative framework packs, linker | `LS-001` .. `LS-999` |
| **`PF`** | **Performance & Storage** | Cold-build throughput, SQLite compaction, latency budgets, index memory | `PF-001` .. `PF-999` |
| **`AR`** | **Architecture & Modularity** | Engine architecture, traits (`LanguageLinker`), god-file decomposition | `AR-001` .. `AR-999` |
| **`UX`** | **Agent UX & Ergonomics** | MCP response compaction, token efficiency, Markdown-first dual-surface | `UX-001` .. `UX-999` |
| **`IN`** | **Installer & Onboarding** | Multi-flavor distribution, `install.sh`, packaging, repo onboarding | `IN-001` .. `IN-999` |

### Legacy Phase 0 Series (`010`–`056`)

Archived milestones with 3-digit pure numbers (`010`–`056`) represent the foundational Phase 0 implementation. They map to the new domain taxonomy as follows:
- `010`–`019` $\to$ Phase 0 predecessors of **`AC`** (`ACDD & Lifecycle`).
- `020`–`023` $\to$ Phase 0 predecessors of **`LS`** (`Language Semantics`).
- `030`–`031` $\to$ Phase 0 predecessors of **`PF`** (`Performance`).
- `040`–`048` $\to$ Phase 0 predecessors of **`AR`** (`Architecture`).
- `050`–`056` $\to$ Phase 0 predecessors of **`UX`** (`Agent Ergonomics & MCP`).

Active commitments migrate to the `TAG-NNN` standard; archived milestone files and receipts remain untouched in [archive/](archive/README.md) to preserve historical commit and receipt integrity.
