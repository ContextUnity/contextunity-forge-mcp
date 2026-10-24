---
title: "Future Specifications Index & Task Promotion Router"
doc_type: index
---

# Future Specifications Index & Task Promotion Router

This directory contains the **target architectural specifications, profiles, and authoring guidelines** being developed in milestone `AC-001` (Hierarchical ACDD Gates, Modular Catalogs, and Milestone Orchestration).

To maintain absolute separation between **current runtime behavior (As-Is)** and **planned platform design (To-Be)**:
- The active repository (`docs/`, `skills/`, `src/`) strictly describes and executes against the currently working code and schema.
- All future designs, target profile examples, v2 authoring rules, and target ADRs reside here in `docs/future/`.
- Each artifact in this directory is owned by an explicit task in `AC-001`, which implements its engine support and promotes it to its permanent location upon delivery or milestone handoff.

---

## 1. Inventory of Future Artifacts & Task Ownership

| Target Artifact in `docs/future/` | Permanent Target Location | Owning Task in `AC-001` | Promotion Trigger / Condition |
| :--- | :--- | :--- | :--- |
| **`adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md`** | `docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md` | Task 6 (`scope-extension-manifest-sync-and-review-audit`) | Promoted upon milestone handoff commit when all 7 tasks pass verification. |
| **`references/acdd_profile.v2.yaml.example`** | `skills/contextunity-forge/references/acdd_profile.yaml.example` | Task 1 (`universal-gate-ast-and-modular-catalog-spec`) | Copied into `references/` once the polymorphic gate AST and catalog serde pass seam tests. |
| **`milestones/AGENTS.md`** | `docs/milestones/AGENTS.md` | Task 6 (`scope-extension-manifest-sync-and-review-audit`) | Promoted during handoff verification after the updated binary accepts `symbols:` in `TaskSpec`. |

---

## 2. Implementing Tasks & Promotion Instructions

### Task 1: `universal-gate-ast-and-modular-catalog-spec`
- **Source Artifact**: `docs/future/references/acdd_profile.v2.yaml.example`
- **Responsibility**:
  1. Implement serde data structures for polymorphic `GateDef`, `GateProfile`, and catalog schemas in `src/core/tasks/gates.rs` and `src/core/tasks/catalog.rs`.
  2. Implement `TaskSpec.symbols` schema and validator.
  3. Validate parsing of `docs/future/references/acdd_profile.v2.yaml.example` against the new AST parser in `tests/acdd/gates_ast.rs`.
  4. Copy `docs/future/references/acdd_profile.v2.yaml.example` to `skills/contextunity-forge/references/acdd_profile.yaml.example`.

### Task 2: `sqlite-gate-transitions-and-trace-persistence`
- **Source Specification**: `docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md` (Pillars 3, 4, 11)
- **Responsibility**:
  1. Implement SQLite tables `gate_transitions`, `gate_execution_traces`, `gate_checkpoints`, and `milestone_contract_matrix`.
  2. Implement `recipient_task_id` addressing in `task_blackboard`.
  3. Verify migrations and persistence APIs in `tests/acdd/persistence.rs`.

### Task 3: `zero-arg-transition-engine-and-command-runner`
- **Source Specification**: `docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md` (Pillars 1, 4, 11)
- **Responsibility**:
  1. Implement zero-argument proof compilation from gate execution traces.
  2. Implement autonomous command gate runner and `by_pattern` dispatch.
  3. Implement automated telemetry passport commit note generator (2 KiB limit).

### Task 4: `upfront-contract-synthesis-and-shared-helper-extraction`
- **Source Specification**: `docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md` (Pillar 8)
- **Responsibility**:
  1. Implement contract synthesis matrix generator and helper extraction.
  2. Implement preflight milestone init validation and worktree scaffolding.

### Task 5: `container-gate-and-claim-engine`
- **Source Specification**: `docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md` (Pillars 1, 3, 9)
- **Responsibility**:
  1. Implement state-driven task claim with compound breadcrumb injection.
  2. Implement in-place execution runner (`--in-place`).
  3. Implement static seam reachability verifier with stale-index diagnostics.

### Task 6: `scope-extension-manifest-sync-and-review-audit`
- **Source Artifacts**:
  - `docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md`
  - `docs/future/milestones/AGENTS.md`
- **Responsibility**:
  1. Implement atomic `task extend-scope` manifest synchronization and review audit gate.
  2. Implement milestone verification container and handoff trace/checkpoint purging.
  3. **Preparation Step**: Prepare target copies of `docs/adr/0020-*.md`, register in `docs/adr/README.md`, and `docs/milestones/AGENTS.md`.
  4. **Handoff Execution Step**: Land promotions and prune `docs/future/` staging directory during terminal milestone handoff when all tasks pass verification.
