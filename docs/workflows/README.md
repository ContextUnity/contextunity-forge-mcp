---
title: "Workflows"
doc_type: guide
---

# Workflows

The `docs/workflows/` directory contains standalone, taskless gate graph definitions (`doc_type: workflow` or `.workflow.yaml`).

## Overview

> [!NOTE]
> Standalone Workflow dispatch (`contextunity-forge-mcp workflow run`) and dedicated run logging into `.forge/runs/` are admitted architecture planned for delivery in Milestone `AC-003`. Prior to AC-003 delivery, these are profile and test definitions; validation does not execute a taskless workflow. Native `plan run` is also planned for AC-003.

Unlike **Milestones** (formal contractual releases with DAG waves and Level 0 preflight/post-verification) and **Plans** (in-place step-by-step operational manuals), a **Workflow** is a pure state machine of gates executed directly against repository state.

- **Zero Tasks**: The planned model creates no task IDs or SQLite `tasks` rows.
- **Zero Worktree Branching**: The planned runner uses the active workspace.
- **Persistence**: The model defaults to `save: none` (in memory). Opt-in `save: log` and autonomous run logs are planned for AC-003.
- **Use Cases**: Continuous quality checks, polish loops, PR review automation, scheduled health audits, and lifecycle test suites.

## Example & Test Profiles

- [code-quality-and-polish.workflow.yaml](code-quality-and-polish.workflow.yaml): Standalone `verify -> polish -> review` quality gate.
- [test-workflow-lifecycle.workflow.yaml](test-workflow-lifecycle.workflow.yaml): Test profile specifying the pure taskless Workflow model (`save: log`, `fail: polish`).
- [test-plan-lifecycle.workflow.yaml](test-plan-lifecycle.workflow.yaml): Test profile specifying the Plan in-place execution model (`save: snapshot`, strictly `tasks:` container).
- [test-milestone-lifecycle.workflow.yaml](test-milestone-lifecycle.workflow.yaml): Test profile specifying the full 3-tier Milestone lifecycle (Level 0 preflight $\to$ task DAG $\to$ Level 0 verification $\to$ handoff).
