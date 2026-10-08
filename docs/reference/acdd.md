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

## Completion levels: Delivery vs Handoff

- **Task delivery (`deliver/v1`)** completes a task and writes its snapshot-backed receipt.
- **Milestone handoff** records final verification and archives the completed milestone.

Commit each task after delivery. After all tasks complete, record their commit
SHAs in the milestone, then create the handoff commit. Its own SHA remains
unrecorded. Follow the [execution runbook](../runbooks/acdd.md#close-the-milestone).

The admitted milestone defines acceptance. Review findings cite that contract
or a verified regression. Follow the [review procedure](../runbooks/acdd.md#independent-review)
for scope admission, ownership, and durable deferred findings.

## Task lifecycle

Each task moves through `contract/v1`, `build/v1`, `review/v1`, and
`deliver/v1`. A worker claims the current gate before submitting evidence for
that claim. Omit `evidence.commit` to let Forge capture contract/build snapshots
and inherit the accepted build snapshot during review/delivery.
Contract records a failing test through a public seam for greenfield
tasks, or proves existing seams via `proof_policy: direct-proof` (exit code 0).
Build records passing tests. Review checks the candidate against
the contract and five review contours using `inspect_cmd` from the build gate.
The accepted reviewer and delivery worker must have a different `worker_id` from
the accepted builder; review and delivery use the accepted build snapshot SHA.

`agent_type` in a task specification identifies the requested agent
specialization. Claim and inspect return `workflow_guidance` for the active
stage: role, steps, instruction path, and, for review and delivery, the builder
identity to avoid. Configure `agents_guidance` in the root `forge-mcp.yaml`, or
`linked_workspaces[].tasks.agents_guidance` for a linked repository. Both paths
default to `AGENTS.md` in their own repository. A missing file produces
`TASK_GUIDANCE_MISSING`, inline steps, and the
[canonical ACDD reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md).

## Task taxonomy and test proof policies

Tasks in ACDD fall into two structural categories:
- **Feature Task**: Delivers a single coherent architectural capability. Requires exactly **one** root seam test at `contract/v1`. Subtasks represent iterative implementation steps and are strictly prohibited from authoring separate micro-unit test binaries or standalone test functions.
- **Scope Task**: Operates across an entire architectural domain or subsystem (such as Language Semantics in Milestone 020). Replaces ad-hoc test function sprawl with a single unified, parameterized table-driven harness (`cases: [...]`), where each subtask contributes a test case row to prove red-to-green resolution.

### Subtask acceptance

Each subtask names a concrete construct or API, expected result, and measurable
acceptance delta. Prove it through the production seam and reference workload.
Prioritize high-volume cases within the admitted contract. Deliver and commit
the complete admitted task. The [review procedure](../runbooks/acdd.md#independent-review)
owns finding admission and deferred work.

Task contracts specify one of three proof policies:
- `seam-test-first`: Greenfield contract requiring a failing red seam test (`contract/v1`) with non-zero exit code, followed by a passing green test at `build/v1`.
- `direct-proof`: Admitted for pre-existing code reconciliation or refactoring where working production code is already in place. Validates existing seams directly, accepting exit code 0 without authoring synthetic red breakage.
- `deferred-final-test`: Task proof policy for admitted final verification work.
  Contract proof accepts a nonnegative exit code; build still requires passing
  tests, followed by review and delivery. Declare test changes in the task scope.

All policies traverse the same four gates. Milestone closure runs final
verification through the [runbook](../runbooks/acdd.md#close-the-milestone).

## Evidence and retained context

`task_submit` accepts a JSON `evidence` object directly. It binds `task_id`,
stage, claim and contract revisions, worker, worktree, commit, and non-null
`proof` to the active claim. For `contract/v1` and `build/v1`, `commit` can be omitted
and Forge populates it with the captured scoped snapshot SHA; for `review/v1` and
`deliver/v1`, `commit` must match the accepted build snapshot SHA. Contract, build,
and review use typed proof.
Contract proof names the public seam test and its nonzero failure exit code (or
exit code 0 when `proof_policy: direct-proof` or `proof_policy: deferred-final-test` is used);
build proof records the command, exit code, and test counts; review proof records a
decision and evidence for the five contours. The same object is retained in SQLite
`task_gates.evidence`. No evidence file is needed.

Build proof accepts a passing, focused domain test. Run the full
`cargo test --all-targets` suite for the deferred final milestone test. The
[testing guide](../testing/README.md) defines when to run each check.

`task_blackboard` keeps milestone, task, and subtask messages in SQLite. Use
`post` for a topic and payload. `read` returns newest-first summary pages
(default 10, maximum 50); `inspect` retrieves a selected message by `message_id`
with its full payload. Use explicit context selectors when inference is ambiguous. An omitted author uses the active claim worker, or
the transport name when unclaimed. Messages are useful for contract findings,
build proof, and `architectural_notes`; they do not replace gate evidence.

At successful task delivery, Forge verifies the accepted build and review
proof and the current milestone specification. It writes the task receipt into
the milestone document, carrying the candidate snapshot commit, verified invariants,
review summary, and `architectural_notes`. It then clears that task's blackboard.
The milestone document retains the durable outcome; the blackboard remains a
temporary coordination surface.

The [milestone CLI reference](tasks.md#milestone-cli-lifecycle) defines archival
validation and verification fields.
