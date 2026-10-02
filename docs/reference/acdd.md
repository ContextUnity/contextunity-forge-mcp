---
title: Admitted-contract-driven development
doc_type: api
---

# Admitted-contract-driven development

Admitted-contract-driven development (ACDD) connects a Git milestone contract
to an executable task queue. The milestone document owns target, scope,
invariants, dependencies, and durable receipts. SQLite owns claims, gate
transitions, proof, and temporary collaboration messages. A code-index rebuild
does not reset task state.

The [task operations reference](tasks.md) defines API and proof shapes. The
[execution runbook](../runbooks/acdd.md) gives the delivery sequence, agent
handover protocol, and verification cadence.

## Completion and authority

Task **delivery** uses `deliver/v1` to complete one task and write its receipt
into the milestone. `milestone handoff` is a separate CLI operation after every
task is complete and the deferred final test has passed; it archives the
milestone.

The admitted milestone specification defines what the task must prove. A
reviewer checks the stable candidate against that contract and verified
runtime behavior. Findings outside the task's scope go to the owning task or
milestone decision process; they do not silently expand the frozen contract.

## Task lifecycle

Each task moves through `design/v1`, `contract/v1`, `build/v1`, `review/v1`, and
`deliver/v1`. A worker claims the current gate before submitting evidence for
that claim. Contract records a failing test through a public seam. Build
records passing tests. Review checks the candidate against the contract and
five review contours. The accepted reviewer and delivery worker must have a
different `worker_id` from the accepted builder; review and delivery use the
accepted build commit.

`agent_type` in a task specification identifies the requested agent
specialization. Claim and inspect return `workflow_guidance` for the active
stage: role, steps, instruction path, and, for review and delivery, the builder
identity to avoid. Configure `agents_guidance` in the root `forge-mcp.yaml`, or
`linked_workspaces[].tasks.agents_guidance` for a linked repository. Both paths
default to `AGENTS.md` in their own repository. A missing file produces
`TASK_GUIDANCE_MISSING`, inline steps, and the
[canonical ACDD reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md).

## Evidence and retained context

`task_submit` accepts a JSON `evidence` object directly. It binds `task_id`,
stage, claim and contract revisions, worker, worktree, commit, and non-null
`proof` to the active claim. Contract, build, and review use typed proof.
Contract proof names the red seam test and its
nonzero exit code; build proof records the command, exit code, and test counts;
review proof records a decision and evidence for the five contours. The same
object is retained in SQLite `task_gates.evidence`. No evidence file is needed.

Build proof accepts a passing, focused domain test. Run the full
`cargo test --all-targets` suite for the deferred final milestone test. The
[testing guide](../testing/README.md) defines when to run each check.

`task_blackboard` keeps task-scoped messages in SQLite. Use `post` for a topic
and payload, and `read` for chronological messages, optionally filtered by
topic or limited in count. An omitted author uses the active claim worker, or
the transport name when unclaimed. Messages are useful for contract findings,
build proof, and `architectural_notes`; they do not replace gate evidence.

At successful task delivery, Forge verifies the accepted build and review
proof and the current milestone specification. It writes the task receipt into
the milestone document, carrying the commit, verified invariants, review
summary, and `architectural_notes`. It then clears that task's blackboard.
The milestone document retains the durable outcome; the blackboard remains a
temporary coordination surface.

After all tasks are completed, run the milestone's deferred final test and
`milestone handoff`. That command records the milestone verification result
and archives the completed document. See
[milestone CLI lifecycle](tasks.md#milestone-cli-lifecycle).
