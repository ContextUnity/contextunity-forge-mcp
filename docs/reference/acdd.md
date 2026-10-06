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

Each task moves through `contract/v1`, `build/v1`, `review/v1`, and
`deliver/v1`. A worker claims the current gate before submitting evidence for
that claim. Contract records a failing test through a public seam for greenfield
tasks, or proves existing seams directly via `proof_policy: direct-proof` (with exit code 0)
without synthetic breakage. Build records passing tests. Review checks the candidate against
the contract and five review contours. The accepted reviewer and delivery worker must have a
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

## Task taxonomy and test proof policies

Tasks in ACDD fall into two structural categories:
- **Feature Task**: Delivers a single coherent architectural capability. Requires exactly **one** root seam test at `contract/v1`. Subtasks represent iterative implementation steps and are strictly prohibited from authoring separate micro-unit test binaries or standalone test functions.
- **Scope Task**: Operates across an entire architectural domain or subsystem (such as Language Semantics in Milestone 020). Replaces ad-hoc test function sprawl with a single unified, parameterized table-driven harness (`cases: [...]`), where each subtask contributes a test case row to prove red-to-green resolution.

### Subtask contracts and universal Definition of Done (DoD)

Subtasks deepen a parent task without inflating the root milestone task queue. Every subtask operates under these binding invariants:
1. **Concrete Syntax-Targeted Mandate (No Abstract Formulations)**: Subtasks must never be formulated as vague goals (e.g. "improve resolution", "fix edge cases"). Every subtask contract must explicitly specify: (1) the concrete syntax, grammar construct, or API contract targeted; (2) the exact expected resolution or state transition status; (3) the verifiable production-path metric or acceptance delta.
2. **Production-Seam Evidence (Anti-Toy-Fixture Gate)**: Passing an isolated, synthetic micro-unit test in a vacuum does not satisfy subtask completion. Verification evidence must demonstrate real production-path fulfillment on the reference corpus. A subtask cannot be marked completed if the targeted universal construct still fails on unshadowed code in the reference workload.
3. **No Review-Repair Micro-Looping (Anti-Looping Invariant)**: Reviewers are strictly prohibited from rejecting builds on uncontracted hypothetical edge cases. A review finding is only valid if it cites an admitted contract requirement, a regression against baseline suites, or a verifiable defect on reference corpus code. Speculative micro-findings belong to future tasks or the blackboard, not blocking the active delivery pipeline.
4. **Volume-First Prioritization**: When executing domain scope tasks, subtasks must be implemented in order of reference corpus impact volume. Never spend execution iterations on esoteric constructs (<10 occurrences) while high-volume categories (>100 occurrences) remain unhandled.
5. **No Partial / Incomplete Commits**: Never merge or commit partial implementations into milestone branches while known standard syntactic or contract constructs remain unhandled or fail closed as unknown.

Task contracts specify one of three proof policies:
- `seam-test-first`: Greenfield contract requiring a failing red seam test (`contract/v1`) with non-zero exit code, followed by a passing green test at `build/v1`.
- `direct-proof`: Admitted for pre-existing code reconciliation or refactoring where working production code is already in place. Validates existing seams directly, accepting exit code 0 without authoring synthetic red breakage.
- `deferred-final-test`: Milestone-level test review gate executed prior to milestone handoff. Governed by the `test-suite-refactor` skill to audit and strengthen coverage at the boundaries where new contracts interface with pre-existing contracts. It possesses explicit cross-scope authority to refactor, consolidate, or update pre-existing tests outside individual task scopes.

## Evidence and retained context

`task_submit` accepts a JSON `evidence` object directly. It binds `task_id`,
stage, claim and contract revisions, worker, worktree, commit, and non-null
`proof` to the active claim. Contract, build, and review use typed proof.
Contract proof names the public seam test and its nonzero failure exit code (or
exit code 0 when `proof_policy: direct-proof` or `proof_policy: deferred-final-test` is used);
build proof records the command, exit code, and test counts; review proof records a
decision and evidence for the five contours. The same object is retained in SQLite
`task_gates.evidence`. No evidence file is needed.

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
