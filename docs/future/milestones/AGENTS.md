---
title: "Milestone Instructions & Authoring Governance (v2 Target)"
doc_type: guide
status: future_target
owning_task: AC-001/task-6
promotion_target: docs/milestones/AGENTS.md
---

# Milestone Instructions & Authoring Governance (v2 Target)

> [!NOTE] Target Specification (Milestone AC-001)
> This document specifies the v2 milestone authoring format enabled by the Universal Hierarchical Gate Platform delivered in milestone `AC-001` (governed by [ADR 0020](../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md)).
> **Promotion Trigger**: Promoted to `docs/milestones/AGENTS.md` during Task 6 / Milestone Handoff after the upgraded binary passes full verification.

---

## 1. Milestone Document Structure & Frontmatter (v2)

```yaml
---
id: <TAG>-<NNN>-<slug>
title: "Human-readable milestone title"
doc_type: contract
status: planned
depends_on:
  - <upstream-milestone-id>
guidance:
  hitl: pro                  # Guidance slot binding: careless | curious | pro | paranoic
owners:
  - src/core/tasks/
  - src/engine/tasks/
invariants:
  - 'INV-SLOT-NAME: Explicit, testable invariant statement.'
required_skills:
  - contextunity-forge
  - agent-instructions
related_plans: []
---
```

### Frontmatter Invariants:
1. **`id`**: Must match `<TAG>-<NNN>-<slug>` conforming to Domain Taxonomy (`AC`, `LS`, `PF`, `AR`, `UX`, `IN`).
2. **`doc_type: contract`**: Milestones are legally binding execution contracts.
3. **`status: planned`**: Unreleased milestones remain `planned` until the first task is claimed.
4. **`guidance: { slot: tier }`**: Binds profile guidance slots. For `hitl`, specify `careless | curious | pro | paranoic`. The engine dynamically resolves `format!("{}.{}", slot, tier)` (`hitl.pro`).
5. **`invariants:`**: Must be prefixed with `INV-<NAME>` in single quotes. Every invariant must be verifiable by automated tests.

---

## 2. Task Decomposition & Strict Symmetrical Boundaries (v2)

In the v2 engine, `TaskSpec` natively accepts both physical file boundaries (`scope:`) and logical interface boundaries (`symbols:`):

```yaml
### task: <task-slug>

```yaml
task_ref: <task-slug>
target: "Clear, single-sentence imperative outcome detailing exact deliverables"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - <upstream-task-slug>
scope:
  - src/core/module.rs
  - src/engine/module.rs
  - docs/reference/module.md
symbols:
  - struct:GateTransition          # Admitted new symbol
  - fn:TasksStore::extend_scope    # Admitted signature modification
  - table:gate_transitions         # Admitted database table
status: planned
subtasks:
  - subtask_ref: concrete-subtask-1
    title: "Explicit, testable description of subtask 1"
  - subtask_ref: concrete-subtask-2
    title: "Explicit, testable description of subtask 2"
```
```

### Task Specification Rules:
- **`target`**: Must state what is built, what invariants are proved, and what tools/budgets are respected.
- **`depends_on:` (Topological Waves DAG)**:
  - Waves are strictly formed by topological dependencies in `depends_on: [...]`.
  - Tasks with empty `depends_on: []` run in Wave 0 (Bootstrap).
  - Tasks depending solely on Task 0 run in parallel in Wave 1.
  - Subsequent waves depend on the completion of upstream wave tasks.
- **`scope:` (Physical File Boundary)**:
  - Declare every physical source file the builder is permitted to edit.
  - **Disjoint Scopes**: Tasks in the same parallel wave MUST have disjoint `scope:` paths to prevent Git conflicts during parallel worktree runs.
  - **Co-Evolution of Documentation**: If a task modifies user-facing behavior, CLI/MCP tools, or diagnostics, include the corresponding `docs/` files directly in `scope:`.
  - Builders can autonomously extend files via `task extend-scope`, which atomically updates SQLite and the milestone document and requires review sign-off.
- **`symbols:` (Public Interface Boundary & Universal Syntax)**:
  - Closed list of public symbols the task is authorized to **create (new)** or **modify (public signature changes)**.
  - Uses universal `kind:identifier` syntax:
    - Code: `struct:`, `fn:`, `trait:`, `type:`, `class:`, `interface:`
    - Storage: `table:`, `column:`, `index:`
    - API: `route:`, `schema:`
    - CLI: `cmd:`, `flag:`
    - Catalogs: `catalog:`, `slot:`
  - Builder agents are strictly forbidden from creating unadmitted public symbols or modifying unlisted public signatures (`UNADMITTED_SYMBOL_EXPANSION`). Private helpers inside scoped files remain unconstrained.

---

## 3. Test Harness Pre-Registration Invariant (Task 0 Pattern)

When milestone tasks allocate dedicated, disjoint seam test files (e.g. `tests/<subsystem>/<task_seam>.rs`):
- **Bootstrap Registration**: Task 0 (bootstrap task) creates the stub test files upfront and registers them in the subsystem's parent `mod.rs` (e.g. `tests/<subsystem>/mod.rs`).
- **Zero Scope Collisions**: Concurrent wave tasks (Waves 1..N) own strictly their individual seam test files, never touching the shared parent `mod.rs`. This ensures that parallel worktrees avoid merge conflicts on test registration and that `cargo check` / `cargo test` compiles cleanly from turn 0.

---

## 4. Two-Loop ACDD Governance & Reject Routing

Milestone execution follows the Two-Loop Model ([ADR 0020 Pillar 13](../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md)):

```
Fast Inner Loop:  builder (build) -> review -> (scope/test defect) -> builder (build)
Long Outer Loop:  reviewer -> (symbols violation) -> architect -> seam test -> builder
```

### Reject Routing:
```yaml
reject_routing:
  scope: build         # Implementation bugs or unadmitted files -> builder reverts or extends scope
  symbols: contract    # Unadmitted public interface mutation or architectural gap -> architect
```

### HITL Governance Tiers (`guidance.hitl`):
- **`careless`**: Architect autonomously approves all symbols; escalates to human only on DAG deadlocks or fatal environment crashes.
- **`curious`**: Architect approves routine changes; surfaces interface evolutions to `task_blackboard`; escalates on breaking public APIs, destructive DB migrations, or new external dependencies.
- **`pro`** (*default*): Professional engineering balance. Architect autonomously approves internal helpers/types; strictly escalates to human on public library API changes, CLI modifications, and DB migrations via an A/B Decision Card.
- **`paranoic`**: Zero autonomous symbol expansion. Every interface deviation requires human sign-off via an A/B Decision Card.

---

## 5. Milestone Verification & Hand-off

During the milestone handoff stage:
1. Execute the required milestone verification gate from `TESTS.md`.
2. Run `contextunity-forge-mcp milestone handoff <id> ...` to archive the manifest and rewrite landed task receipts.
3. Stage the archived manifest in `archive/` and stage the removal of the active working manifest.
4. Ensure obsolete or untracked pre-archive working manifest files (`docs/milestones/<id>-...md`) are unlinked so that branch integration and fast-forward landing into `main` remain completely clean.
