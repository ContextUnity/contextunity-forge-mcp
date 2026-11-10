---
title: "ADR 0021: Workflow, Plan, and Milestone Lifecycle Hierarchy"
doc_type: adr
status: accepted
date: 2026-11-03
decision_makers:
  - Oleksii (Architecture Owner)
  - Antigravity (ACDD Coordinator)
consulted:
  - Forge MCP Core Team
informed:
  - Autonomous Agent Coordinators and Builders
---

# ADR 0021: Workflow, Plan, and Milestone Lifecycle Hierarchy

> [!IMPORTANT]
> This hierarchy includes the core task DAG and receipts, milestone verification, and the headless runtime with native `workflow run` and `plan run` commands. See the [ACDD execution reference](../reference/acdd.md#headless-task-milestone-plan-and-workflow-execution) for the implemented boundaries. Schema validation alone does not prove end-to-end execution.

## Context and decision

[ADR 0020](0020-hierarchical-acdd-gates-and-milestone-orchestration.md) defines the shared gate model. A workflow, plan, and milestone have distinct execution subjects and persistence needs. Their graphs must not introduce synthetic task identities or mix root milestone gates into task execution.

### Workflow

A Workflow is a pure root `gates:` graph with no `tasks:` container, task IDs, task rows, or task claims. The native runner executes it in memory with `save: none`, including configured role runners. Autonomous run logs are not implemented; a valid profile is not evidence that it has run.

### Plan

A Plan uses only a `tasks:` container and executes its steps in the active working tree. It uses snapshots and does not automatically commit. It has no milestone synthesis or handoff gates. Native `plan run` executes in place; completed plans may be archived separately.

### Milestone

A Milestone combines root contract synthesis, a task DAG, engine verification, independent integration review, a delivery commit, and handoff context binding and archival. The headless runner executes the root `gates:` graph and its declared transitions, and the existing handoff validator checks content-bound verification and review.

```yaml
gates:
  - id: contract_synthesis
    role: architect
  - tasks:
      gates:
        - id: build
          role: builder
        - id: review
          role: independent_reviewer
          fail: build
        - id: deliver
          receipt: true
          need: [review]
          save: commit
  - id: verify
    command: check_full_repo
    pass: milestone_review
    fail: polish
  - id: polish
    role: builder
    pass: verify
  - id: milestone_review
    role: independent_reviewer
    pass: handoff
    fail: polish
  - id: handoff
    receipt: true
    absorb: true
    save: commit
```

## Persistence and audit log

The architectural persistence levels are `none`, `log`, `snapshot`, and `commit`. `none` keeps execution in memory. `log` records gate progression, timestamps, commands, results, output digests, durations, review decisions, and terminal status in an audit log. `snapshot` adds working-tree snapshots; `commit` adds delivery commits and bound receipts. Workflow defaults to `none`, Plan to `snapshot`, and Milestone delivery to `commit`. Autonomous Workflow run logs, including `.forge/runs/` and associated trace records, are not implemented. Existing task and milestone records retain their own evidence and lifecycle rules.

## Consequences and verification

Profiles distinguish the models by graph shape. Root milestone gates execute once at the milestone boundary; task gates execute for each admitted task. Verification must consume observed, content-bound engine evidence and independent review before handoff. Round 4 findings R4-01–R4-09, R4-15, R4-19, and R4-20 identify historical incomplete enforcement. End-to-end tests after AC-003 must exercise each model, persistence choice, failure route, recovery, and Git effects.
