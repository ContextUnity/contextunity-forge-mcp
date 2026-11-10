---
title: "Milestone Handoff, Recovery, and List Guidance"
doc_type: plan
status: completed
date: 2026-10-23
---

# Milestone Handoff, Recovery, and List Guidance

AC-002 delivers four bounded capabilities. The current runtime contracts live in
[task operations](../reference/tasks.md), [CLI commands](../reference/cli.md),
and [ADR 0015](../adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md).

## Terminal State and Dependencies

Milestone handoff prunes terminal task-store state by default. Project-level
`handoff_pruning: false` retains history without changing workflow profiles.
Schema 6 tombstones use canonical repository/project identities. Directory sync
ignores tombstoned manifests; explicit attempts to resurrect them fail closed
inside the writer transaction.

Completed dependencies resolve from tombstones and configured archive documents,
including wildcard roots. Cancelled tasks do not receive completion credit.
The seven-state handoff retry matrix covers durable archive publication,
pruning, relocation, and cleanup without requiring retained SQLite history.

## Proof Retention and Recovery

SQLite COMMIT owns the logical transition. Complete task-owned checkpoints are
published before managed success or observation. Their namespaced keepers retain
actual parentless candidate and prepared-base objects. Bounded provisional
protection covers snapshot construction until durable keeper publication.
There is no global Git-first state journal or new permanent per-proof pin.

`milestone db-recover` restores acknowledged active provenance from verified
checkpoints, current authorities, and linked-worktree archives. Healthy legacy
state is enrolled before recoverability is promised. Pending publication is
retried without replaying claims or gates; missing uncheckpointed state,
ambiguous intents, incomplete cohorts, and invalid gate provenance fail closed.

Archive telemetry preserves exact final verification and integration-review
provenance. Historical SHA identities do not promise permanent Git-object
reachability. Cleanup respects other active proof owners and exact present
`proof_shas` lists, including authoritative empty lists.

## Milestone Listing

Terminal output uses Markdown sections, literal single-line manifest text,
clickable encoded file paths, task counts, and actionable next commands.
The `milestones` JSON row shape is unchanged.

Archive listing defaults to 10 documents and clamps `--limit` to 1..=100 without
limiting active/planned documents. Archive task statuses and completion come
from task blocks without querying SQLite; cancelled tasks remain in the total.
Current milestone progress continues to use the task store.

## Verification and Boundaries

Each task has accepted contract, build, independent review, and delivery proof.
Use [TESTS.md](../../TESTS.md) for task and final milestone gates. These contracts
make no measured latency or benchmark claim.

AC-001 profile-v2 runtime and task-level telemetry remain separate planned work.
The current runnable profile and future v2 proposal are explicitly distinguished
in the [profile reference](../reference/profiles.md#reference-examples).
