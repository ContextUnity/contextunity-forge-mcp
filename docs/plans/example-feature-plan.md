---
title: "Example In-Place Feature Plan"
doc_type: plan
purpose: >-
  Demonstrate an operational in-place plan executed step-by-step under the tasks: container
  without Level 0 milestone gates (no contract_synthesis, no handoff passport).
depends_on: []
---

# Example In-Place Feature Plan

This plan demonstrates the **Plan Model** defined in [ADR 0021](../adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md).

Native `plan run` execution and autonomous run logs are planned for AC-003; this example does not describe an available command.

## Execution Characteristics

- **In-Place**: Planned execution via `contextunity-forge-mcp plan run docs/plans/example-feature-plan.md`.
- **Zero Level 0 Gates**: Skips `contract_synthesis` and `handoff`; executes steps directly through `build -> review -> deliver`.
- **Persistence**: Uses `save: snapshot` (never auto-commits to Git).
- **Archiving**: Completed plans can be moved to `docs/plans/archive/`.

---

## 1. Step 1: Core Type Extraction

- **Target**: Extract and decouple shared value structures.
- **Scope**: `crates/forge-core/src/types.rs`
- **Subtasks**:
  - [x] Define clean DTO model
  - [x] Wire serde serialization
- **Status**: completed

## 2. Step 2: Engine Integration

- **Target**: Wire the extracted types into the pipeline.
- **Scope**: `crates/forge-tasks/src/engine/`
- **Subtasks**:
  - [ ] Replace legacy references
  - [ ] Verify unit tests pass
- **Status**: ready
