---
title: "ADR 0015: ACDD Lifecycle Snapshots and Proactive Defect Resolution"
doc_type: adr
status: accepted
date: 2026-10-07
---

# ADR 0015: ACDD Lifecycle Snapshots and Proactive Defect Resolution

## Status

Accepted. Handoff content-evidence amendment admitted in milestone 049 on 2026-10-20.

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
   The `commit` field in `evidence` is optional on `contract` and `build` (auto-populated with the captured snapshot SHA). On `review` and `deliver`, omitted `commit` inherits the accepted build candidate SHA. Worktree divergence is verified by tree comparison against the candidate; snapshot refs are never shifted on review or delivery, preserving build snapshot references from garbage collection.
3. **Reviewer Inspection via `inspect_cmd` and 5 Review Contours**:
   Independent reviewers inspect candidate diffs using `inspect_cmd` (`git show <snapshot_commit>`), evaluating changes against the contract across five explicit contours: `paths` (scope fidelity), `claims` (contract requirements without invented additions), `concurrency`, `project_isolation`, and `administration`.
4. **Task Delivery vs Milestone Handoff**:
   - `deliver` validates accepted build and review proofs, records `status: completed` with a typed receipt (retaining the build candidate snapshot SHA) in the milestone document, and clears temporary blackboard state.
   - Immediately after delivery, one clean atomic Git commit is created on the branch containing strictly the scoped files, tests, and milestone document (without `git add -A` and without `--amend`).
   - After all tasks are delivered, commit the final implementation, run the instruction-level documentation scan, and obtain independent integration review and repository verification against that same content and an explicit target base. `milestone handoff-context` supplies the canonical content and accepted-authority binding; `milestone handoff --evidence-file` validates the external record and archives it with the milestone. The reviewer differs from every accepted task builder. Forge launches no agents and runs no verification commands.
   - Accepted task contracts, gate proofs, candidate snapshots, reviewer decisions, and identities remain immutable. Branch commit SHAs are optional navigation metadata. Rebase, cherry-pick, merge, amend, and multi-task squash do not invalidate acceptance solely because SHA, ancestry, or task-to-commit cardinality changes. Handoff does not reconstruct first-parent task landings or require one commit per task. Refresh uniquely resolvable pointers; omit unresolved pointers and honor commit-recording settings.
   - Final evidence binds repository/project/milestone identity, current contracts and accepted proofs, final implementation paths/content/modes, and selected target-base content. Only generated receipt/status/handoff metadata is projected out. Changed implementation, base, or authority needs fresh final review and verification; accepted tasks remain closed. Final review accounts for combined changes, upstream integration and conflict resolutions within admitted authority.
   - Retain all accepted proof objects under content-addressed `refs/forge/evidence/` independently of branch history and reflog expiry. Pin available historical task proofs when preparing handoff context; transient task snapshot refs may then be pruned after archive publication. Missing immutable proof objects fail closed. Commit the archive afterward; its own SHA remains unrecorded.
5. **Conflict-Aware Scope Extension & Out-of-Scope Defect Handling**:
   - **Sibling task scope**: If an adjacent defect falls within the admitted scope of another task in the milestone (hierarchical check covering exact match, subdirectory enclosure, or ancestor enclosure across both frozen and unfrozen paths), `extend-scope` rejects the collision with `TASK_SCOPE_CONFLICT`, requiring `task reopen <owner_id>`.
   - **Unowned defect paths**: Admit paths through `extend_scope` before editing, following the [scope contract](../reference/tasks.md#five-flat-mcp-tools). The reviewer verifies the `paths` contour. Record deferred findings in milestone `deferred_defects` or a linked successor contract before delivery clears temporary blackboard messages.
6. **Zero-Shot Context Bundle by Default**:
   `task claim` includes context by default. [ADR 0016](0016-lean-stage-tailored-task-context-bundles.md) defines the current stage-specific fields and bounds.

## Consequences

- Task delivery remains an atomic scoped commit under the configured profile; later branch integration may rewrite or combine commits without rewriting accepted evidence.
- Snapshot commit SHAs are fully deterministic and immune to sibling task branch commits.
- Accepted proof objects remain reachable after handoff and branch-history rewrites. External final review replaces commit-history archaeology, while task gate immutability and delivery rollback stay unchanged.
- Sibling task scopes are protected from hijacking, while unowned adjacent defects are either legitimately repaired under reviewer governance or recorded as deferred findings.
