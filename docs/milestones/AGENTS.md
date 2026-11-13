---
title: "Milestone Instructions & Authoring Governance"
doc_type: guide
---

# Milestone Instructions & Authoring Governance

This document defines the milestone authoring format and workflow supported by the Universal Hierarchical Gate Platform in [ADR 0020](../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md).

---

## 1. Milestone Document Structure & Frontmatter

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

## 2. Task Decomposition & Strict Symmetrical Boundaries

`TaskSpec` accepts physical file boundaries (`scope:`) and logical interface boundaries (`symbols:`):

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
  - **Structural Module Registrations (Non-Exclusive Wiring)**: Declaring `pub mod <file_in_scope>;` inside parent module files (e.g. `mod.rs`, `lib.rs`) is permitted as non-exclusive structural wiring without demanding whole-file exclusive write locks, eliminating artificial scope blockages.
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

## 3. Wave 0 Bootstrap Task Template & Contract Quality Checklist (Task 0 Pattern)

To eliminate cross-task scope drift, merge conflicts on shared test harnesses, and semantic ambiguities across parallel builder agents, every non-trivial milestone should declare an initial **Wave 0 Bootstrap Task** (`Task 0`).

Task 0 serves as the architectural foundation: it synthesizes the cross-task contract matrix, establishes deterministic topological execution waves, standardizes shared data models and error taxonomies, pre-registers seam test stubs in the shared test suite index, and freezes disjoint file scopes before downstream tasks begin.

### Canonical Wave 0 (Task 0) Template

```yaml
### task: upfront-contract-synthesis-and-dag-prioritization

```yaml
task_ref: upfront-contract-synthesis-and-dag-prioritization
target: "Synthesize cross-task contract matrix, establish deterministic topological execution waves, standardize shared data models & error variants, pre-register seam test harness modules in <test_suite_index>, and freeze disjoint scopes for all milestone tasks"
proof_policy: seam-test-first
contract_revision: 1
depends_on: []
scope:
  - docs/milestones/<TAG>-<NNN>-<slug>.md
  - docs/adr/<NNNN>-<slug>.md
  - <path/to/shared_core_definitions>
  - <path/to/test_suite_index>
  # Pre-registered seam test stubs for all milestone tasks:
  - <path/to/task_1_seam_test>
  - <path/to/task_2_seam_test>
  - <path/to/task_n_seam_test>
symbols:
  - enum:<SharedErrorEnum>
  - struct:<SharedDataMatrix>
status: ready
subtasks:
  - subtask_ref: cross-task-data-contract-matrix
    title: "Define canonical cross-task contract matrix and shared data structures"
  - subtask_ref: deterministic-priority-waves-dag
    title: "Establish deterministic topological execution waves and explicit DAG dependencies"
  - subtask_ref: seam-test-harness-pre-registration
    title: "Pre-register seam test stub files in test suite index and freeze test module boundaries"
  - subtask_ref: disjoint-scope-boundaries-freeze
    title: "Freeze disjoint file write scopes and seam test allocations across all tasks"
  - subtask_ref: shared-error-taxonomy-and-diagnostic-codes
    title: "Standardize shared error variants and diagnostic error codes in ADR and reference profile"
```
```

### Contract Quality Checklist (Universal Bootstrap Verification Gates)

Before opening downstream parallel waves, the contract author and architect must verify that the Wave 0 contract satisfies these universal quality gates:

1. **Zero-Collision Seam Test Pre-Registration**:
   - Every seam test file designated for subsequent tasks (`<path/to/task_seam_test>`) must be pre-created as a minimal valid stub and declared in the test suite root or registry index (`<path/to/test_suite_index>`) within Task 0.
   - Downstream wave tasks (Waves 1..N) own strictly their individual seam test files and are **strictly forbidden from modifying the shared test suite index**. This guarantees conflict-free worktrees and reproducible test runner execution across parallel agents.
2. **Disjoint File Scope Boundaries**:
   - All tasks within the same topological execution wave must have strictly disjoint `scope:` declarations.
   - Zero shared production source files and zero shared test files across concurrent tasks in the same wave to prevent VCS merge collisions.
3. **Symbol-to-Scope Closure & Preflight Alignment (`milestone init`)**:
   - **Symbol-to-Container Closure**: Every symbol declared in `symbols: [...]` must have its declaring container file included in `scope:`. If a symbol resides in a file not listed in `scope:`, `milestone init` (or preflight manifest validation) fails closed (`SCOPE_CLOSURE_UNRESOLVED`) before builder dispatch.
   - **Parent Module Entry Points**: For every greenfield source file introduced in a task's `scope:`, its parent module entry point (e.g. `mod.rs`, `lib.rs`) must either be pre-scaffolded in Task 0 or declared as a non-exclusive structural registration (`pub mod <slug>;`), preventing mid-flight module declaration lockouts.
   - **Cross-Subtask Semantic Domain Alignment**: Subtasks that manipulate distinct subsystems (e.g. claim guidance in `context.rs` vs. submit engine in `submit.rs` vs. CLI flags in `cli/task.rs`) must not be bundled into a task that lacks authority over those files. Each subtask must be strictly mapped to the task owning that subsystem.
   - **Storage Transaction Atomicity**: If a task updates multiple database tables or state stores (e.g. tasks, contract matrix, and blackboard), the task scope must contain the transaction coordinator (`src/db/tasks_store/mod.rs`), or the operation must be encapsulated in a single atomic storage method declared in the contract.
   - **Preflight Enforcement**: This closure validation is executed automatically during `contextunity-forge-mcp milestone init` (as part of `INV-PREFLIGHT-MILESTONE-INIT`), validating that symbols, scopes, parent modules, and subtasks form a closed graph before any agent worktree is created.
4. **Unified Error Taxonomy & Diagnostics**:
   - Shared domain error enumerations, exit codes, and diagnostic identifiers must be declared upfront in shared core interface definitions or architectural specifications.
   - Downstream builders must not introduce competing ad-hoc error structures.
5. **Published Cross-Task Contract Matrix**:
   - Producer-consumer boundaries for shared data types, interfaces, and schemas must be published to `task_blackboard` (or recorded in the milestone document) before Wave 1 begins.
   - Each shared data structure must have an unambiguous producer task and designated consumer tasks.
6. **Clean Seam Verification & Expected Red Proof**:
   - All pre-registered test files and shared definitions must compile or validate cleanly under the repository's strict type-checker, compiler, or linter gates (zero build errors).
   - Under `INV-SEAM-ENGINE-RED-GREEN`, the contract gate execution must produce an actionable runtime assertion failure matching the configured `expected_failure.exit_codes` (e.g. exit 101 in Rust, exit 1 in Python/pytest or Go/Node), confirming the presence of a sensitive, unfulfilled contract seam prior to implementation.

---

## 4. Two-Loop ACDD Governance & Loop Routing

Milestone execution follows the Two-Loop Model ([ADR 0020 Pillar 13](../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md)):

```
Fast Inner Loop:  builder (build) -> review -> (scope/test defect) -> builder (build)
Long Outer Loop:  reviewer -> (symbols violation) -> contract -> seam test -> builder
```

### Loop Routing:
```yaml
loop:
  scope: build         # Implementation bugs or unadmitted files -> builder reverts or extends scope
  symbols: contract    # Unadmitted public interface mutation or architectural gap -> contract_author / architect
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
