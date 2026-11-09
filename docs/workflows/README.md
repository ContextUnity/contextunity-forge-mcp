---
title: "Workflows"
doc_type: guide
---

# Workflows

The `docs/workflows/` directory contains standalone, taskless gate graph definitions (`doc_type: workflow` or `.workflow.yaml`).

## Overview

> [!NOTE]
> `contextunity-forge-mcp workflow run` executes a taskless workflow in the active workspace. `plan run` executes structured plans in-place. See the [CLI reference](../reference/cli.md#standalone-workflows) for the execution and persistence limits.

Unlike **Milestones** (formal contractual releases with DAG waves and Level 0 preflight/post-verification) and **Plans** (in-place step-by-step operational manuals), a **Workflow** is a pure state machine of gates executed directly against repository state.

- **Zero Tasks**: The runner creates no task IDs or SQLite `tasks` rows.
- **Zero Worktree Branching**: The runner uses the active workspace.
- **Persistence**: Execution uses `save: none` (in memory); `save: log` and dedicated run logs are not implemented.
- **Use Cases**: Continuous quality checks, polish loops, PR review automation, scheduled health audits, and lifecycle test suites.

## Example & Test Profiles

- [code-quality-and-polish.workflow.yaml](code-quality-and-polish.workflow.yaml): Standalone `verify -> polish -> review` quality gate.
- [test-workflow-lifecycle.workflow.yaml](test-workflow-lifecycle.workflow.yaml): Test profile specifying the pure taskless Workflow model (`save: log`, `fail: polish`).
- [test-plan-lifecycle.workflow.yaml](test-plan-lifecycle.workflow.yaml): Test profile specifying the Plan in-place execution model (`save: snapshot`, strictly `tasks:` container).
- [test-milestone-lifecycle.workflow.yaml](test-milestone-lifecycle.workflow.yaml): Test profile specifying the full 3-tier Milestone lifecycle (Level 0 preflight $\to$ task DAG $\to$ Level 0 verification $\to$ handoff).
