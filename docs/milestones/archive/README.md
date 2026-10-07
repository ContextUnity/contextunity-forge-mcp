---
title: "Archived milestones"
doc_type: guide
---

# Archived milestones

Retain completed and cancelled milestone files, stable IDs, task receipts,
verification summaries, and source-plan links here. Completed manifests carry
their handoff verification; cancelled manifests carry a non-empty
`closure.reason` that preserves why the commitment stopped. The archived
manifest is the durable lifecycle record even when its operational task state
is no longer present in SQLite.

When `task sync` reads a cancelled milestone, it prunes that milestone's tasks,
their outgoing dependencies, and milestone-level blackboard messages. It
preserves unsatisfied incoming dependency blocks on tasks that remain. The
archive therefore retains the cancellation rationale while the live task queue
contains only operational state still eligible for work. The configured
milestone root includes this archive. Apply the completion procedure in the
[task reference](../../reference/tasks.md) before moving completed task-backed
commitments here; record cancellation rationale in the manifest before
archiving a cancelled commitment.

See [ADR 0014](../../adr/0014-milestone-task-subtask-hierarchy-and-visibility.md)
for the lifecycle and pruning invariants.
