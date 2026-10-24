---
title: "Milestone Instructions & Authoring Governance"
doc_type: guide
---

# Milestone Instructions & Authoring Governance

This document governs agent instructions for authoring, reviewing, executing, and archiving ACDD milestones in `docs/milestones/`.

---

## 1. Milestone Document Structure & Frontmatter

Every milestone document (`docs/milestones/<TAG>-<NNN>-<slug>.md`) is an immutable ACDD execution contract. When authoring a milestone:

```yaml
---
id: AC-001-hierarchical-acdd-gates-and-milestone-orchestration
title: Hierarchical ACDD gates, modular catalogs, and milestone orchestration
doc_type: contract
status: planned
depends_on:
  - m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance
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
1. **`id`**: Must match `<TAG>-<NNN>-<slug>` conforming to [Domain Taxonomy](README.md#milestone-taxonomy-and-domain-tagging) (`AC`, `LS`, `PF`, `AR`, `UX`, `IN`).
2. **`doc_type: contract`**: Milestones are legally binding execution contracts, not loose proposals.
3. **`status: planned`**: Unreleased milestones remain `planned` until the first task is claimed.
4. **`invariants:`**: Must be prefixed with `INV-<NAME>` in single quotes. Every invariant must be verifiable by automated tests.
5. **Zero Unreleased Schema Fields**: Do not declare frontmatter fields unsupported by the active runtime parser (e.g. `guidance:`, `symbols:`, or `wave:`). Target schemas are documented in `docs/future/milestones/AGENTS.md`.

---

## 2. Task Decomposition & Strict Symmetrical Boundaries

Tasks in a milestone must be decomposed to minimize deviation probability and eliminate builder improvisation:

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
status: planned
subtasks:
  - subtask_ref: concrete-subtask-1
    title: "Explicit, testable description of subtask 1"
  - subtask_ref: concrete-subtask-2
    title: "Explicit, testable description of subtask 2"
```

### Task Specification Rules:
- **`target`**: Must state what is built, what invariants are proved, and what tools/budgets are respected.
- **`proof_policy` (Contract Proof Admission)**:
  - **`seam-test-first` (Greenfield / TDD / Defect Fixes)**: Default policy for greenfield features and bug fixes. Requires authoring a sensitive failing test on the production seam at `contract` stage (`red_exit_code: 101` in Rust, runtime assertion/panic failure) proving the absence of the capability or presence of the defect, followed by a passing run at `build` stage (`exit_code: 0`).
  - **`direct-proof` (Refactoring / Decomposition / Optimization)**: Mandatory policy for god-file decomposition, architectural refactoring, performance optimization, and module restructuring where external contracts remain invariant. Accepts `red_exit_code: 0` at `contract` stage by binding to an existing characterization seam test (`seam_test_ref`), proving that 100% of baseline behavior is captured before code changes and preserved after `build`. Prohibits authoring artificial panic stubs solely to produce a red exit code.
  - **`deferred-final-test` (Scaffolding / Upfront Synthesis / Manifests)**: Admitted for contract synthesis, workspace scaffolding, and documentation sync tasks where isolated unit tests do not apply. Accepts `red_exit_code: 0` at task level; full verification is deferred to the repository-wide gate matrix prior to milestone handoff.
- **`depends_on:` (Topological Waves DAG)**:
  - Waves are strictly formed by topological dependencies in `depends_on: [...]`. Do not declare a bespoke `wave:` field inside YAML.
  - Tasks with empty `depends_on: []` run in Wave 0 (Bootstrap).
  - Tasks depending solely on Task 0 run in parallel in Wave 1.
  - Subsequent waves depend on the completion of upstream wave tasks.
- **`scope:` (Physical File Boundary)**:
  - Declare every physical source file the builder is permitted to edit.
  - **Disjoint Scopes**: Tasks in the same parallel wave MUST have disjoint `scope:` paths to prevent Git conflicts during parallel worktree runs.
  - **Co-Evolution of Documentation**: If a task modifies user-facing behavior, CLI/MCP tools, or diagnostics, include the corresponding `docs/` files directly in `scope:`.
  - Builders can extend files via `task extend-scope` (recording additions in SQLite for reviewer verification). Dual synchronization with the markdown document is delivered in milestone `AC-001`.
- **Target Interface Boundaries & Symbols Confinement**: In the current runtime (`v0.3.x`), interface fidelity is governed through contract seam tests and cognitive review. Formal declarative `symbols:` validation and confinement are target v2 capabilities specified in [`docs/future/milestones/AGENTS.md`](../future/milestones/AGENTS.md).

### 3. Test Harness Pre-Registration Invariant (Task 0 Pattern)

When milestone tasks allocate dedicated, disjoint seam test files:
- **Bootstrap Registration**: Task 0 (bootstrap task) creates the stub test files upfront and registers them in the shared test suite root or registry index (`<path/to/test_suite_index>`).
- **Zero Scope Collisions**: Concurrent wave tasks (Waves 1..N) own strictly their individual seam test files, never touching the shared parent test suite index. This ensures that parallel worktrees avoid merge conflicts on test registration and that type-checkers, compilers, and test runners execute cleanly from turn 0.
- **Bootstrap Template & Quality Checklist**: For the universal Wave 0 (Task 0) YAML contract template and the 5 universal bootstrap verification gates, see [Wave 0 Bootstrap Task Template in docs/future/milestones/AGENTS.md](../future/milestones/AGENTS.md#3-wave-0-bootstrap-task-template--contract-quality-checklist-task-0-pattern).

---

## 4. Task Verification & Review Governance

Milestone execution in the current engine (`v0.3.x`) progresses through the linear gate sequence: `contract` -> `build` -> `review` -> `deliver`.
- **Review Rejections**: When independent cognitive review uncovers implementation defects or scope discrepancies, the reviewer submits rejection with detailed findings to `task_blackboard`, routing execution back to the builder.
- **Architectural Escalation**: When unexpected interface shifts occur, agents escalate via structured A/B Decision Cards on `task_blackboard`:
```markdown
> [!IMPORTANT]
> **Architectural Escalation (Milestone <ID>)**
> **Task**: `<task_ref>`
> **Finding**: <Concise summary with code coordinates file:line>
> 
> **Architect Recommendation:**
> - **[A] (Recommended)** <Cleanest architectural path forward>
> - **[B]** <Conservative alternative avoiding interface mutation>
```
- **Target v2 Two-Loop ACDD Governance**: Target hierarchical gates, declarative `reject_routing: { scope: build, symbols: contract }`, and 4-tier HITL governance are specified in [`docs/future/milestones/AGENTS.md`](../future/milestones/AGENTS.md) and will be promoted upon landing milestone `AC-001`.

---

## 5. Milestone Verification & Hand-off

During the milestone handoff stage:
1. Execute the required milestone verification gate from [`TESTS.md`](../../TESTS.md#milestone-handoff-gate).
2. Run `contextunity-forge-mcp milestone handoff <id> ...` to archive the manifest and rewrite landed task receipts.
3. Stage the archived manifest in `archive/` and stage the removal of the active working manifest.
4. Ensure obsolete or untracked pre-archive working manifest files (`docs/milestones/<id>-...md`) are unlinked so that branch integration and fast-forward landing into `main` remain completely clean without untracked duplicates.
