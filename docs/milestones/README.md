---
title: "Milestones"
doc_type: guide
---

# Milestones

Milestone commitment files in this directory (`0XX-*.md`, `TAG-NNN-*.md`) define the repository's execution queue and lifecycle commitments.

## Ownership and Truth

- **Single Source of Truth:** Each milestone file (`docs/milestones/0XX-*.md`, `docs/milestones/TAG-NNN-*.md`) owns its status (`active`, `planned`, `completed`, or `cancelled`), invariants, task breakdown, and acceptance outcomes. A cancelled milestone records a non-empty `closure.reason`.
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

Active commitments migrate to the `TAG-NNN` standard; archived milestone files retain their historical commitments and receipts in [archive/](archive/README.md).

## Milestone Authoring and Contract Governance

Every milestone document in `docs/milestones/` is an immutable ACDD execution contract. When authoring or revising milestones, follow [`docs/milestones/AGENTS.md`](AGENTS.md) for the active format and canonical Wave 0 bootstrap template.

### 1. Frontmatter Format

```yaml
---
id: AC-001-hierarchical-acdd-gates-and-milestone-orchestration
title: Hierarchical ACDD gates, modular catalogs, and milestone orchestration
doc_type: contract
status: planned
depends_on: []
owners:
  - src/core/tasks/
  - src/engine/tasks/
invariants:
  - 'INV-...: Explicit invariant statements'
required_skills:
  - contextunity-forge
---
```

Frontmatter must strictly declare only fields supported by the active engine parser (`id`, `title`, `doc_type`, `status`, `depends_on`, `guidance`, `owners`, `invariants`, `required_skills`, `related_plans`). Task blocks may declare `scope`, `extended_scope`, `symbols`, dependencies, and subtasks as documented in [`docs/milestones/AGENTS.md`](AGENTS.md). Unknown fields fail manifest validation.

### 2. Task Blocks and Scope Boundaries

Every YAML task block in `## Tasks in this milestone` must explicitly define physical file boundaries and dependencies forming topological waves:

```yaml
### task: sqlite-gate-transitions-and-trace-persistence

```yaml
task_ref: sqlite-gate-transitions-and-trace-persistence
target: "Implement SQLite schema migrations for gate_transitions and gate_execution_traces"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - upfront-contract-synthesis-and-dag-prioritization
scope:
  - src/db/tasks_store/schema.rs
  - src/db/tasks_store/trace.rs
status: planned
subtasks:
  - subtask_ref: gate-transitions-table-migration
    title: "Implement SQLite migration and store APIs for durable gate_transitions audit table"
  - subtask_ref: gate-execution-trace-table-migration
    title: "Implement SQLite migration for gate_execution_traces with 2 KiB output snippets"
```
```

- **`depends_on:` (Topological Waves)**: Waves are purely determined by the dependency DAG `depends_on: [...]`. Tasks without dependencies run in Wave 0; tasks depending solely on Task 0 run concurrently in Wave 1.
- **`scope:` and `extended_scope:` (Files)**: `scope` is the admitted baseline. Builders add files through `task extend-scope`, which synchronizes the task store and milestone manifest; review proof must approve the exact extensions before delivery.
- **`symbols:` (Interfaces)**: Declare public symbol additions or signature changes with the universal `kind:identifier` syntax. The builder keeps public changes within this list; private helpers remain governed by file scope.

### 3. Review & Verification Governance

- **Gate progression**: The active profile defines task gates and milestone containers. Task dependencies determine execution waves, and review findings route through the configured `reject_routing` policy.
- **Review and escalation**: Reviewers evaluate every configured contour and explicitly approve non-empty `extended_scope`. Escalate unadmitted public-interface changes through the milestone's architect and HITL guidance.
