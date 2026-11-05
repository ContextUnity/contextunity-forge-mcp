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
> This hierarchy is implemented across AC-001 (core task DAG and receipts), AC-002 (milestone graph and verification), and [AC-003](../milestones/AC-003-headless-orchestrator-and-role-runners.md) (planned headless orchestrator runtime and native `workflow run` and `plan run` commands). Root verify and handoff operations exist, but Round 4 found gaps in graph execution and verification authority; schema validation alone does not prove end-to-end execution.

## Context and decision

[ADR 0020](0020-hierarchical-acdd-gates-and-milestone-orchestration.md) defines the shared gate model. A workflow, plan, and milestone have distinct execution subjects and persistence needs. Their graphs must not introduce synthetic task identities or mix root milestone gates into task execution.

### Workflow

A Workflow is a pure root `gates:` graph with no `tasks:` container, task IDs, task rows, or task claims. Execution is ephemeral and in memory by default (`save: none`). It can opt into an audit log. Native taskless dispatch, role execution, and autonomous run logs are planned for AC-003; a valid profile is not evidence that it has run.

### Plan

A Plan uses only a `tasks:` container and executes its steps in the active working tree. It uses snapshots and does not automatically commit. It has no milestone synthesis or handoff gates. Native in-place `plan run` is planned for AC-003; completed plans may be archived separately.

### Milestone

A Milestone combines root contract synthesis, a task DAG, engine verification, independent integration review, a delivery commit, and handoff context binding and archival. Root verification and handoff operations exist today. The accepted design uses a single root `gates:` graph and the declared transitions throughout; the Round 4 audit found that the existing root operations do not yet enforce that graph and that mandatory observed verification can be bypassed. Those defects remain under remediation.

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

The architectural persistence levels are `none`, `log`, `snapshot`, and `commit`. `none` keeps execution in memory. `log` records gate progression, timestamps, commands, results, output digests, durations, review decisions, and terminal status in an audit log. `snapshot` adds working-tree snapshots; `commit` adds delivery commits and bound receipts. Workflow defaults to `none`, Plan to `snapshot`, and Milestone delivery to `commit`. Autonomous Workflow run logs, including `.forge/runs/` and associated trace records, are planned for AC-003 and are not evidence of current taskless execution. Existing task and milestone records retain their own evidence and lifecycle rules.

## Consequences and verification

Profiles distinguish the models by graph shape. Root milestone gates execute once at the milestone boundary; task gates execute for each admitted task. Verification must consume observed, content-bound engine evidence and independent review before handoff. Round 4 findings R4-01–R4-09, R4-15, R4-19, and R4-20 identify incomplete enforcement; the [audit plan](../plans/gate-engine-bugs-and-coverage-audit.md) tracks the reopened boundaries. End-to-end tests after AC-003 must exercise each model, persistence choice, failure route, recovery, and Git effects.
