---
title: "ACDD Lifecycle Metadata Cleanup, Archive Commands, and Verification"
doc_type: plan
purpose: >-
  Deliver lifecycle metadata cleanup and archive commands (clear and archive
  for milestones, plans, and workflows) deferred during AC-003 delivery.
depends_on: []
---

# ACDD Lifecycle Metadata Cleanup, Archive Commands, and Verification

Deliver lifecycle metadata cleanup and archive commands (`clear` and `archive`
for milestones, plans, and workflows) deferred during AC-003 delivery.

## 1. Milestone Clear Implementation

- **Target**: Purge milestone tasks, blackboard messages, gate execution traces, checkpoint home metadata, and Git checkpoint references.
- **Scope**: crates/forge-tasks/src/milestones/mod.rs, crates/forge-tasks/src/store/mod.rs, src/cli/milestone.rs
- **Status**: completed
- **Subtasks**:
  - [x] Add TasksStore::clear_milestone method to delete tasks, blackboard, traces, dependencies, and metadata
  - [x] Add milestones::clear_milestone to prune Git checkpoint refs and invoke store cleanup
  - [x] Wire MilestoneCommand::Clear CLI subcommand and format human-readable output

## 2. Plan Clear and Archive Implementation

- **Target**: Add plan clear to purge snapshots and SQLite state, and plan archive to relocate completed plan manifest to archive.
- **Scope**: src/cli/plan.rs, crates/forge-tasks/src/engine/orchestrator/plan.rs
- **Status**: completed
- **Depends On**: [Milestone Clear Implementation]
- **Subtasks**:
  - [x] Implement plan clear to delete snapshot refs and plan tasks/traces from SQLite
  - [x] Implement plan archive to move manifest to docs/plans/archive/ and clean state
  - [x] Wire PlanCommand::Clear and PlanCommand::Archive CLI subcommands

## 3. Workflow Clear Implementation

- **Target**: Add workflow clear to purge execution run logs in .forge/runs/ and SQLite traces.
- **Scope**: src/cli/workflow.rs, crates/forge-tasks/src/engine/orchestrator/workflow.rs
- **Status**: completed
- **Depends On**: [Milestone Clear Implementation]
- **Subtasks**:
  - [x] Implement workflow clear to remove .forge/runs/<name>* and trace records
  - [x] Wire WorkflowCommand::Clear CLI subcommand

## 4. Universal Clear Command Dispatch

- **Target**: Provide top-level unified clear command contextunity-forge-mcp clear for milestones, plans, and workflows.
- **Scope**: src/cli/mod.rs
- **Status**: completed
- **Depends On**: [Plan Clear and Archive Implementation, Workflow Clear Implementation]
- **Subtasks**:
  - [x] Add Command::Clear with subject and identifier routing
  - [x] Support unified argument parsing and dispatch to milestone, plan, and workflow cleaners

## 5. Integration and Seam Verification

- **Target**: Domain tests proving authentic cleanup of SQLite records, Git refs, and files across all three lifecycle models.
- **Scope**: tests/acdd/tasks/cleanup.rs, tests/acdd/milestones/mod.rs
- **Status**: completed
- **Depends On**: [Universal Clear Command Dispatch]
- **Subtasks**:
  - [x] Author test proving milestone clear prunes DB rows, checkpoint_home metadata, and Git checkpoint refs
  - [x] Author test proving plan clear and archive prune snapshots and relocate document
  - [x] Author test proving workflow clear deletes run logs and traces
  - [x] Verify strict clippy and full test suite passes
