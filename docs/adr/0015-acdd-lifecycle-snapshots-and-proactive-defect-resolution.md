---
title: "ADR 0015: ACDD Lifecycle Snapshots and Proactive Defect Resolution"
doc_type: adr
status: accepted
date: 2026-10-07
---

# ADR 0015: ACDD Lifecycle Snapshots and Proactive Defect Resolution

## Status

Accepted.

## Context

Task lifecycle governance in ACDD required architectural decoupling between gate verification and repository branch history:
1. **Premature Branch Commits**: Creating Git commits on feature branches during intermediate build gates polluted branch history and produced fragile dependencies between unreviewed working trees and milestone receipts.
2. **Parent-Coupled Snapshot Invalidation**: Using `-p HEAD` for tree snapshots caused sibling task commits in shared worktrees to artificially alter the snapshot SHA even when scoped source files were identical. Furthermore, shifting snapshot refs prior to gate validation caused dangling commits and `TASK_CANDIDATE_MISMATCH`.
3. **Rigid Hash Validation**: Enforcing 40- or 64-character hex strings blocked standard short Git hashes and lightweight tree snapshots.
4. **Bystander Inaction vs Scope Clashes**: Strict path boundary checks caused agents to ignore adjacent defects, claiming they were "out of scope", while unconstrained path expansion risked hijacking files owned by concurrent sibling tasks.
5. **Shared Worktree Concurrency**: In shared worktrees where multiple tasks may proceed concurrently, `git add -A` captured unreviewed changes from sibling tasks.

## Decision

1. **Root Scoped Git Snapshots over Branch Commits**:
   Before task delivery, code state is captured via deterministic Git snapshots under `refs/forge/snapshots/{repository}/{project}/{milestone_id}/{task_ref}` using an isolated index (`GIT_INDEX_FILE`), fixed identity, and timestamp. Snapshots are created as root tree commits (without `-p HEAD`), ensuring cryptographic determinism based strictly on scoped source content and immunity to concurrent branch commits.
2. **Safe Candidate Consistency & Ref Preservation**:
   The `commit` field in `evidence` is optional on `contract/v1` and `build/v1` (auto-populated with the captured snapshot SHA). On `review/v1` and `deliver/v1`, omitted `commit` inherits the accepted build candidate SHA. Worktree divergence is verified by tree comparison against the candidate; snapshot refs are never shifted on review or delivery, preserving build snapshot references from garbage collection.
3. **Reviewer Inspection via `inspect_cmd` and 5 Review Contours**:
   Independent reviewers inspect candidate diffs using `inspect_cmd` (`git show <snapshot_commit>`), evaluating changes against the contract across five explicit contours: `paths` (scope fidelity), `claims` (contract requirements without invented additions), `concurrency`, `project_isolation`, and `administration`.
4. **Task Delivery vs Milestone Handoff**:
   - `deliver/v1` validates accepted build and review proofs, records `status: completed` with a typed receipt (retaining the build candidate snapshot SHA) in the milestone document, and clears temporary blackboard state.
   - Immediately after delivery, one clean atomic Git commit is created on the branch containing strictly the scoped files, tests, and milestone document (without `git add -A` and without `--amend`).
   - After all tasks are delivered and committed, run final verification and record task commit SHAs in the milestone. `milestone handoff` records supplied verification, archives the milestone, and prunes snapshot refs. Commit the archive as the handoff commit; its own SHA remains unrecorded.
5. **Conflict-Aware Scope Extension & Out-of-Scope Defect Handling**:
   - **Sibling task scope**: If an adjacent defect falls within the admitted scope of another task in the milestone (hierarchical check covering exact match, subdirectory enclosure, or ancestor enclosure across both frozen and unfrozen paths), `extend-scope` rejects the collision with `TASK_SCOPE_CONFLICT`, requiring `task reopen <owner_id>`.
   - **Unowned defect paths**: Admit paths through `extend_scope` before editing, following the [scope contract](../reference/tasks.md#five-flat-mcp-tools). The reviewer verifies the `paths` contour. Record deferred findings in milestone `deferred_defects` or a linked successor contract before delivery clears temporary blackboard messages.
6. **Zero-Shot Context Bundle by Default**:
   `task claim` includes context by default. [ADR 0016](0016-lean-stage-tailored-task-context-bundles.md) defines the current stage-specific fields and bounds.

## Consequences

- Feature branch history remains clean and linear, eliminating intermediate WIP commits before review.
- Snapshot commit SHAs are fully deterministic and immune to sibling task branch commits.
- Snapshot refs remain pinned and protected until final milestone handoff.
- Sibling task scopes are protected from hijacking, while unowned adjacent defects are either legitimately repaired under reviewer governance or recorded as deferred findings.
