---
id: m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance
title: Milestone handoff pruning, evidence cleanup, crash recovery, and list guidance
doc_type: contract
status: active
depends_on: []
owners:
- src/db/tasks_store/
- src/engine/tasks/
- src/engine/milestones/
- src/cli/milestone.rs
- src/cli/task.rs
- src/core/git.rs
- docs/
invariants:
- 'INV-SHARED-STORE-TOMBSTONE-GUARD: milestone_tombstones records archived milestone IDs, terminal completion status, and completed task IDs; directory sync silently ignores tombstoned active manifests while explicit sync fails closed.'
- 'INV-HANDOFF-SQLITE-PRUNING: Completed milestones are completely purged from active SQLite tasks, claims, revisions, blackboard messages, and traces upon handoff; the archive manifest is the canonical durable record.'
- 'INV-ARCHIVE-AUTHORITATIVE-DEPENDENCY: Dependencies on completed milestones and tasks are satisfied by milestone_tombstones and docs/milestones/archive/*.md, requiring zero bridge tasks in SQLite.'
- 'INV-IDEMPOTENT-HANDOFF-MATRIX: Handoff evaluates the 7-case state matrix prior to ready_milestone, ensuring clean crash recovery across archive writing, torn archive repair, SQLite pruning, cross-worktree unmerged manifests, and file deletion.'
- 'INV-EPHEMERAL-STATE-REFS: Legacy permanent refs/forge/evidence/<sha> refs are discontinued and pruned; in-flight tasks maintain a single moving keeper ref refs/forge/state/<milestone>/<task_slug> chaining parentless root snapshots, deleted upon milestone handoff.'
- 'INV-CRASH-DB-RECOVERABILITY: If tasks.sqlite is deleted or corrupted, milestone db-recover reconstructs active milestone and task state from active worktrees, manifest files, and refs/forge/state/ pointers, pre-seeding tombstones across all linked worktrees from archive manifests.'
- 'INV-MARKDOWN-FIRST-MILESTONE-LIST: Terminal milestone list renders structured Markdown blocks without pipe tables, supports --limit on archive listings, calculates task completion from task blocks, and provides actionable guidance.'
required_skills:
- contextunity-forge
- agent-instructions
related_plans:
- docs/plans/milestone-handoff-pruning-and-list-guidance.md
started_at: 2026-10-22T00:58:39+00:00
---

# Milestone handoff pruning, evidence cleanup, crash recovery, and list guidance

## Outcome and purpose

Deliver lifecycle cleanliness, zero-bloat persistence, and robust crash recovery across ContextUnity Forge MCP:

1. **SQLite Handoff State Pruning & Version Migration (v5 $\to$ v6)**: Fully prune completed milestone tasks, claims, subtasks, revisions, blackboard messages, and traces from SQLite `.forge/tasks.sqlite` upon `milestone handoff`. This is an engine-wide global project setting enabled by default across all workflows (configurable/opt-out in `forge-mcp.yaml`), not a per-workflow profile setting. The durable record lives in `docs/milestones/archive/<slug>.md`. Schema upgrades via a one-shot v5 $\to$ v6 migration updating `task_store_metadata.schema_version` while preserving lock-free opens for v6 databases.
2. **Resurrection Guard (`milestone_tombstones`)**: Prevent stale active manifests (e.g. on unmerged worktrees or branches) from resurrecting pruned milestones into SQLite during batch `task sync` in `manage_inner`, while explicit sync fails closed.
3. **Archive-Authoritative Dependency Satisfaction & Fast Cache**: Treat `docs/milestones/archive/*.md` and Git history as permanent ground truth (<100ms directory scan if DB unrecovered). Use `milestone_tombstones` as a fast SQLite cache without arbitrary timers, evicted only after verifying across linked worktrees that no active manifest remains and archive contains a verified handoff token.
4. **Idempotent 7-Case Handoff State Machine**: Handle process interruptions safely before, during, or after archive writing, torn archive post-prune repair, SQLite pruning, cross-worktree unmerged active manifests, and active file deletion, with `.bak` backup/restore compensation on write failure, eliminating dangerous reverse ref-swaps post-prune.
5. **Elimination of Git Ref Bloat (`refs/forge/evidence/*`)**: Discontinue permanent evidence refs (pruning milestone-scoped proof SHAs at handoff and legacy ~15 MB dead refs). Amend ADR 0015 and `docs/reference/tasks.md` to establish the markdown archive and branch history as permanent proof storage. Keep parentless snapshots pure while maintaining a single moving keeper pointer `refs/forge/state/<milestone>/<task_slug>` per active task (chaining snapshots as parents to protect from `git gc`), deleted in all handoff completion paths.
6. **Disaster Recovery (`milestone db-recover`)**: Enable zero-loss reconstruction of SQLite state from active worktree manifests and in-flight Git state pointers if `tasks.sqlite` is lost, discovering all linked worktrees via `git worktree list --porcelain` and pre-seeding tombstones from archive manifests to prevent active manifest resurrection from other worktrees.
7. **Markdown-First `milestone list` with Pagination**: Replace pipe tables with structured Markdown cards, add `--limit` (default 10, clamped 1..=100) for archive listings, calculate completion stats from markdown task blocks, and render clear agent guidance.

---

## Tasks in this milestone

### task: sqlite-handoff-pruning-and-tombstone-migration

```yaml
task_ref: sqlite-handoff-pruning-and-tombstone-migration
target: Implement version-gated v5 to v6 milestone_tombstones migration, directory scan resurrection guards in manage_inner, and cascade SQLite handoff pruning with distinct milestone_ref and milestone_scope_ref keys
proof_policy: seam-test-first
contract_revision: 10
scope:
- src/db/tasks_store/mod.rs
- src/engine/tasks/lifecycle.rs
- src/engine/tasks/mod.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/mod.rs
- tests/acdd/tasks.rs
- tests/acdd/milestones.rs
- tests/acdd/mcp.rs
- docs/reference/configuration.md
- docs/reference/tasks.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/acdd_profile.current.yaml.example
status: completed
subtasks:
- subtask_ref: version-gated-v6-tombstone-migration
  title: Implement one-shot v5 to v6 schema migration updating schema_version for milestone_tombstones and preserving lock-free opens on v6
  status: ready
  evidence: null
- subtask_ref: directory-scan-resurrection-guard
  title: Implement silent continue in batch sync loop (lifecycle.rs manage_inner) for tombstoned or archived milestones and fail-closed validation for explicit sync
  status: ready
  evidence: null
- subtask_ref: handoff-sqlite-pruning-transaction
  title: Purge completed milestone tasks, subtasks, claims, task_revisions, task_blackboard (by distinct milestone_scope_ref), and gate traces from SQLite inside BEGIN IMMEDIATE transaction
  status: ready
  evidence: null
receipt:
  contract_revision: 10
  passed_at: 2026-10-22T22:51:31.901395393+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 116
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Effective stored spec.scope/allowed_write_scope has13 paths including runtime-admitted tests/acdd/subtasks.rs; initial scope12 and manifest remain unchanged until generated delivery. Candidate tree contains exactly those13 regular100644 non-executable blobs; independent verifier confirmed every on-disk byte and SHA256 against final packet before/after required review run. HEAD remains95d4e0525cf36257b4f438409f98154193893050. Staged binary diff remains21b0be3ef8df0b3baa36d6f7cb18a016127af6bf9ef1eb351b7ef72252338db1; foreign staged plan/v2 and root Cargo.toml modification preserved. Only Forge may generate receipt and scoped commit.
        claims:
          applicable: true
          evidence: PASS. Reviewed complete Task1 Contract10 and cumulative ledger1045 superseding1043, prior findings1039, handoff1046. R1 and R2 introduced-regression/confirmed-fixed. Fresh5->6/fresh6/lock-free6, transaction validation/dependency propagation/cascade/revisions/namespaced milestone blackboard, cancelled terminal no completed-task credit, global default pruning and opt-out retained-history behavior remain implemented by existing owners and production seams. tests/acdd/subtasks.rs retains every prior assertion with explicit opt-out and adds durable receipt equivalence/location/no-tombstone; parser/profile test assertions retained and v2 unchanged. Review cargo test --test acdd exit0:116passed0failed0filtered, archive89a36ee9d8b05bb2. Build16 strictClippy0 warnings plus acdd116/0 accepted on same13-path binding.
        concurrency:
          applicable: true
          evidence: PASS; R1 confirmed-fixed. src/db/tasks_store/mod.rs831-839 derives canonical identity then acquires BEGIN IMMEDIATE before querying tombstone, prior to task/revision writes. Both public sync/create call this canonical owner. manage_inner575-589 skips only typed MilestoneArchived in batch; explicit STORE_SYNC_FAILED and unrelated errors propagate. Existing child-process SQLite busy-handler rendezvous interleaves prune while both admission modes wait; current complete review target passes. Original runtime boundary-red archives1271f4348de4824e/6ac722f088effbff inspected; five owner mutation receipts468eb2e926502c07 remain identical and match restored owner SHA256 hashes; missing-guard/prelock mutations fail runtime101. Existing public handoff and rollback trigger prove atomic prune and no tombstone on failed delete. GDB engine batch/explicit late-admission results also inspected (prune fixture, not full handoff).
        project_isolation:
          applicable: true
          evidence: 'PASS; R2 confirmed-fixed. Canonical milestone_identity1486 uses existing repository/project namespace for handoff producer1473, public lookup1491, transaction lookup837. Public shared-store seam exercises same bare milestone ID/path in aaa/zzz: foreign sync/create and engine batch/explicit remain allowed, both public store handoffs succeed independently, cancelled task credit staysfalse while terminal sentinel true and completed IDs[]; no new representation/schema migration. Forge graph363files9170nodes0parseerrors matched; AST confirms2 sync_selected wrappers,3 identity calls,1 engine public lookup,2 handoff engine calls. Source inventory across src/tests/docs/skills shows no residual runtime bare-ID reader/producer. Graph receiver limits supplemented by AST/live source; future Task2/3 consumers remain deferred.'
        administration:
          applicable: true
          evidence: 'PASS for current admitted behavior. One-shot version5 migration remains short transactional CREATE+marker6; known6 opens no DDL; schema and profile pins/hook behavior preserved. Current example equals baseline runnable example; v2 is user-owned staged proposal. Six runtime mutation101 receipts inspected: missing guard, prelock guard, bare producer, bare reader, mask-all errors, fixture opt-out removal. Original five owner/test source hashes match final ledger and receipt objects unchanged; fixture sixth receipt794faecd0399262a verified. Git delivery path uses task.spec.scope13, isolated index, hooks enabled, baseline-parent validation and scoped index updates. Three advisory subtasks inspected ready/null: not fabricated or updated; gate acceptance supplied by accepted task proofs. One pre-existing staged-v2 EOF whitespace warning is out of scope and untouched; unstaged task diff --check0. No authority changes or new deferred task defects.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SHARED-STORE-TOMBSTONE-GUARD: milestone_tombstones records archived milestone IDs, terminal completion status, and completed task IDs; directory sync silently ignores tombstoned active manifests while explicit sync fails closed.'
    - 'INV-HANDOFF-SQLITE-PRUNING: Completed milestones are completely purged from active SQLite tasks, claims, revisions, blackboard messages, and traces upon handoff; the archive manifest is the canonical durable record.'
    - 'INV-ARCHIVE-AUTHORITATIVE-DEPENDENCY: Dependencies on completed milestones and tasks are satisfied by milestone_tombstones and docs/milestones/archive/*.md, requiring zero bridge tasks in SQLite.'
    - 'INV-IDEMPOTENT-HANDOFF-MATRIX: Handoff evaluates the 7-case state matrix prior to ready_milestone, ensuring clean crash recovery across archive writing, torn archive repair, SQLite pruning, cross-worktree unmerged manifests, and file deletion.'
    - 'INV-EPHEMERAL-STATE-REFS: Legacy permanent refs/forge/evidence/<sha> refs are discontinued and pruned; in-flight tasks maintain a single moving keeper ref refs/forge/state/<milestone>/<task_slug> chaining parentless root snapshots, deleted upon milestone handoff.'
    - 'INV-CRASH-DB-RECOVERABILITY: If tasks.sqlite is deleted or corrupted, milestone db-recover reconstructs active milestone and task state from active worktrees, manifest files, and refs/forge/state/ pointers, pre-seeding tombstones across all linked worktrees from archive manifests.'
    - 'INV-MARKDOWN-FIRST-MILESTONE-LIST: Terminal milestone list renders structured Markdown blocks without pipe tables, supports --limit on archive listings, calculates task completion from task blocks, and provides actionable guidance.'
    architectural_notes:
    - 'decisions: Contract10/claim17 exact candidate a6d66cfb3301d2fbad25172fe8d7b242542eb2b7: independent closure confirms R1/R2 fixed, all five contours pass and review acdd116/0. Cumulative ledger1045 and original1039 remain lineage. Required mutations all runtime101; owner/test hashes match receipts. No authority or design changes. Three advisory subtasks remain ready/null and were not altered. Task2 owns archive-authoritative dependency discovery and retry matrix; Task3 owns keeper/recovery; Task4 owns list metrics. Current user authorizes one same-reviewer Forge auto delivery after accepted review. Preserve foreign staged plan/v2 and Cargo modification; do not clean or handoff milestone.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Schema version bumps to `"6"` via `task_store_metadata.schema_version`. `TasksStore::open_project` detects version `"5"`, executes the one-shot migration inside a short `BEGIN IMMEDIATE` creating `milestone_tombstones` and updating `schema_version` to `"6"`. Version `"6"` opens proceed lock-free without running DDL. Uninitialized stores create `milestone_tombstones` and insert `"6"` directly. Both version checks in `open_project` accept `"6"`.
2. `milestone_tombstones` schema has columns `milestone_id TEXT PRIMARY KEY`, `archived_at INTEGER`, `all_tasks_terminal INTEGER`, and `completed_task_ids TEXT` (JSON array).
3. The batch directory sync loop in `manage_inner` (`src/engine/tasks/lifecycle.rs`) silently skips (`continue`) active manifests whose milestone ID is in `milestone_tombstones` or whose path matches `is_archived_path`. `workspace.manifests()` remains purely filesystem-based without filtering.
4. Explicit `task sync <tombstoned_id>` returns `STORE_SYNC_FAILED` with an informative error stating the milestone is archived.
5. In `handoff_milestone`, dependency satisfaction is propagated to edges in other milestones first (`UPDATE task_dependencies SET satisfied = 1 WHERE dependency_id = ?`), followed by explicit deletion of `task_revisions` and `tasks` using relative manifest paths (`?1` and `?2`), explicit deletion of milestone-scoped `task_blackboard` rows (`task_id IS NULL`) using canonical computed `milestone_scope_ref` keys (`?3` and `?4`), and inserting the tombstone record within the same transaction.

---

### task: archive-authoritative-dependencies-and-retry-matrix

```yaml
task_ref: archive-authoritative-dependencies-and-retry-matrix
target: "Implement archive and tombstone dependency satisfaction and the pre-ready_milestone 7-case handoff state matrix"
proof_policy: seam-test-first
contract_revision: 8
scope:
  - src/db/tasks_store/mod.rs
  - src/db/tasks_store/lifecycle.rs
  - src/engine/tasks/lifecycle.rs
  - src/engine/milestones/handoff.rs
  - src/cli/milestone.rs
  - tests/acdd/milestones.rs
  - tests/acdd/tasks.rs
status: planned
subtasks:
  - subtask_ref: tombstone-and-archive-dependency-satisfaction
    title: "Evaluate dependency satisfaction against milestone_tombstones (satisfying sentinel when all_tasks_terminal == 1) and docs/milestones/archive/*.md"
    status: pending
  - subtask_ref: pre-ready-handoff-state-matrix
    title: "Implement 7-case handoff recovery matrix evaluated before ready_milestone with .bak backup/restore compensation on write failure, torn archive repair, and no reverse ref-swaps"
    status: pending
  - subtask_ref: cross-worktree-tombstone-landing-guard
    title: "Detect cross-worktree unmerged active manifests when milestone is tombstoned and return MILESTONE_ALREADY_HANDED_OFF without failing ready_milestone"
    status: pending
```

#### Acceptance

1. When checking task dependencies (`unsatisfied_dependencies` and `sync_selected`), missing predecessor tasks evaluate against `milestone_tombstones` and `docs/milestones/archive/` files; completed tasks or `{project}/{id}:__milestone__` sentinel (if `all_tasks_terminal == 1`) are treated as satisfied (`1`).
2. Active tasks with dependencies on archived milestones/tasks validate and unlock without requiring legacy bridge tasks in SQLite.
3. Archive manifest validity predicate strictly requires parseable YAML frontmatter containing `id`, `status: completed`, and a valid `handoff` block (`completed_at`, `commit`), and all tasks in terminal status (`completed` or `cancelled`).
4. Handoff checks the state of (Active Manifest, Local Archive Manifest, Tombstone in DB) before calling `ready_milestone`:
   - (Present, Absent, Absent): Normal initial handoff run. Calls `ready_milestone`, then collects proof SHAs (every gate `commit`, every `candidate_baseline_head`, and every receipt `commit` from SQLite, plus `handoff.commit` and `target_base`) into `handoff.proof_shas` in archive frontmatter, writes archive, prunes SQLite in `BEGIN IMMEDIATE`, unlinks active manifest (`fs::remove_file`), and idempotently deletes milestone evidence refs (reading `handoff.proof_shas`; if absent, taking `commit`, `candidate_baseline_head`, and every 40-char hex SHA in archive text; `git update-ref -d refs/forge/evidence/<sha>`, missing refs succeed) and keeper refs `refs/forge/state/<milestone>/*` (missing refs succeed).
   - (Present, Present [any], Absent): Crash post-write. Copies existing archive to `.bak`, calls `ready_milestone`, collects proof SHAs into `handoff.proof_shas`, rewrites archive, prunes SQLite in `BEGIN IMMEDIATE`, unlinks active manifest, and idempotently deletes milestone evidence refs (using the single fallback rule) and keeper refs (missing refs succeed). Restores `.bak` on transaction failure.
   - (Present, Valid, Present): Crash post-prune. Unlinks active manifest directly without calling `ready_milestone` and without touching SQLite. Idempotently deletes milestone evidence refs (reading `handoff.proof_shas`; if absent, taking `commit`, `candidate_baseline_head`, and every 40-char hex SHA in archive text; `git update-ref -d refs/forge/evidence/<sha>`, missing refs succeed) and keeper refs `refs/forge/state/<milestone>/*` (missing refs succeed). If unlink fails, does NOT attempt reverse ref-swap; leaves files for retry.
   - (Present, Invalid/Torn, Present): Crash during archive rewrite post-prune. Captures proof SHAs before rewrite from torn archive text (via single fallback: `handoff.proof_shas`, else `commit`, `candidate_baseline_head`, and 40-char hex SHAs), unioning with SHAs on disk active manifest and `handoff.commit` / `target_base` from re-invoked arguments. Re-applies render from active manifest and re-invoked arguments, stamping `status: completed`, `handoff` block (`completed_at`, `commit`, `target_base`, integration evidence), and unioned `proof_shas`, keeping landed receipts. This path strictly does NOT call `ready_milestone` and does NOT read or write SQLite (since `task_gates` is already pruned). Verifies `status: completed`, `handoff.completed_at`, `handoff.commit`, and validity predicate (matching `id`, all tasks terminal); if verification fails, leaves both files and returns error without swapping refs. Once verified, unlinks active manifest (`fs::remove_file`) and idempotently deletes milestone evidence refs (via the single fallback rule) and keeper refs `refs/forge/state/<milestone>/*` (missing refs succeed).
   - (Present, Absent, Present): Cross-worktree unmerged state. Bypasses `ready_milestone` and returns `MILESTONE_ALREADY_HANDED_OFF: Milestone '{id}' has already been handed off and pruned in SQLite by another worktree. Merge or pull the handoff commit into this branch.`
   - (Absent, Valid, Present): Completed; idempotent no-op success. Idempotently deletes milestone evidence refs (reading `handoff.proof_shas`; if absent, taking `commit`, `candidate_baseline_head`, and every 40-char hex SHA in archive text; `git update-ref -d refs/forge/evidence/<sha>`, missing refs succeed) and keeper refs `refs/forge/state/<milestone>/*` (missing refs succeed).
   - (Absent, Absent, Present): Post-unlink / unmerged state. Checks if archive blob is reachable in Git history (`HEAD:docs/milestones/archive/<slug>.md`); if reachable returns idempotent no-op success; parses `HEAD:docs/milestones/archive/<slug>.md` using the single fallback rule (reading `handoff.proof_shas`; if absent, taking `commit`, `candidate_baseline_head`, and every 40-char hex SHA in the Git blob text; `git update-ref -d refs/forge/evidence/<sha>`, missing refs succeed) and idempotently deletes keeper refs `refs/forge/state/<milestone>/*` for the known milestone ID (missing refs succeed). If not reachable: returns `MILESTONE_ALREADY_HANDED_OFF`.
5. Milestone handoff absorbs working tree polish (documentation, test refactorings, hotfixes) directly into the handoff integration commit, runs project verification if code was altered, and generates an automated Engineering Telemetry Passport (duration, tasks count, per-task execution time, review cycles, src/tests/docs diff breakdown, AST symbol deltas, test suite results, generic clean linter status, and resolved upstream/downstream topology) attached to `refs/notes/forge` and the archive receipt with verified handoff proof token.

---

### task: eliminate-legacy-evidence-refs-and-state-recovery-pointer

```yaml
task_ref: eliminate-legacy-evidence-refs-and-state-recovery-pointer
target: "Amend ADR 0015 and tasks.md, retire fail-closed evidence-ref check in handoff.rs, prune milestone proof refs and unpinned evidence, establish keeper refs, and implement multi-worktree milestone db-recover"
proof_policy: seam-test-first
contract_revision: 8
scope:
  - src/core/git.rs
  - src/engine/milestones/handoff.rs
  - src/engine/tasks/mod.rs
  - src/cli/milestone.rs
  - src/cli/task.rs
  - tests/acdd/milestones.rs
  - tests/acdd/tasks.rs
  - skills/contextunity-forge/SKILL.md
  - docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md
  - docs/reference/tasks.md
status: planned
subtasks:
  - subtask_ref: adr-0015-and-tasks-doc-proof-retention-amendment
    title: "Amend ADR 0015, docs/reference/tasks.md, and SKILL.md to define archive manifests and branch commits as permanent proof storage while deprecating refs/forge/evidence"
    status: pending
  - subtask_ref: moving-keeper-ref-lifecycle
    title: "Keep candidate snapshots parentless, point moving keeper ref refs/forge/state/<milestone>/<task_slug> at candidate parents to prevent git gc, and delete in all handoff completion paths"
    status: pending
  - subtask_ref: retire-evidence-check-and-prune-legacy-refs
    title: "Retire fail-closed evidence-ref check in handoff.rs, prune milestone-scoped proof refs (from task_gates or archive manifest), and clean unpinned evidence refs via db-recover"
    status: pending
  - subtask_ref: milestone-db-recover-command
    title: "Implement milestone db-recover CLI command discovering worktrees via git worktree list --porcelain, pre-seeding tombstones from archive manifests across all worktrees, and reconstructing SQLite task store from active manifests and state refs"
    status: pending
```

#### Acceptance

1. ADR 0015, `docs/reference/tasks.md`, and `skills/contextunity-forge/SKILL.md` are amended to record that durable proofs reside in the markdown archive and branch history; creation of permanent `refs/forge/evidence/<sha>` Git references is removed.
2. In `src/engine/milestones/handoff.rs`, the fail-closed expectation of `refs/forge/evidence/` after handoff is retired. Handoff renders proof SHAs (gate `commit`, `candidate_baseline_head`, receipt `commit`, `handoff.commit`, `target_base`) into `handoff.proof_shas` before deleting SQLite rows, and in all completion and recovery paths executes `git update-ref -d refs/forge/evidence/<sha>` idempotently using the single fallback rule (read `handoff.proof_shas`; if absent, take `commit`, `candidate_baseline_head`, and 40-char hex SHAs; missing refs succeed), unpinning proof objects.
3. Candidate snapshot commits remain parentless (created via `git commit-tree` without `-p`). A moving keeper ref `refs/forge/state/<milestone>/<task_slug>` points to a lightweight commit listing the in-flight candidate snapshots as parents (`-p`), keeping them reachable from `git gc` during active execution without mutating candidate commit SHAs.
4. All `milestone handoff` completion and post-prune recovery paths delete all `refs/forge/state/<milestone>/*` refs for that milestone via `git update-ref -d` idempotently (missing refs succeed).
5. Command `contextunity-forge-mcp milestone db-recover` executes in strict deterministic order: (1) runs `git worktree list --porcelain` to discover all linked worktree paths; (2) inspects `.forge/tasks.sqlite`: if corrupt or unreadable, renames to `.forge/tasks.sqlite.corrupt.<timestamp>`, then opens or initializes empty schema v6 SQLite database via `TasksStore::open_project` before tombstone pre-seeding; (3) scans `docs/milestones/archive/*.md` across every discovered worktree root to pre-seed `milestone_tombstones` (parsing frontmatter: `status: completed` sets `all_tasks_terminal = 1`, `status: cancelled` sets `all_tasks_terminal = 0`, and extracting completed task IDs from task blocks) FIRST; (4) imports active manifests across discovered worktrees only after tombstones are seeded, strictly skipping any milestone whose ID exists in `milestone_tombstones` or whose path matches `is_archived_path`; (5) reconstructs active tasks, claims, and revisions from active manifests and in-flight `refs/forge/state/` pointers; (6) idempotently deletes legacy `refs/forge/evidence/<sha>` refs using the single fallback rule across archive manifests (read `handoff.proof_shas`; if absent, take `commit`, `candidate_baseline_head`, and 40-char hex SHAs; missing refs succeed) and orphan keeper refs `refs/forge/state/<milestone>/*` for tombstoned milestones (missing refs succeed).

---

### task: markdown-milestone-list-and-archive-limit

```yaml
task_ref: markdown-milestone-list-and-archive-limit
target: "Refactor milestone list to Markdown-first format, add archive pagination limit, and provide actionable agent guidance"
proof_policy: seam-test-first
contract_revision: 8
scope:
  - src/cli/milestone.rs
  - src/engine/milestones/mod.rs
  - src/mcp/tools.rs
  - tests/acdd/milestones.rs
  - docs/reference/cli.md
status: planned
subtasks:
  - subtask_ref: markdown-card-formatting
    title: "Render active and planned milestones as GitHub-Flavored Markdown blocks with status indicators, clickable paths, and no pipe tables"
    status: pending
  - subtask_ref: archive-limit-and-task-block-stats
    title: "Add --limit <N> (default 10, clamped 1..=100) for archive listing and calculate completion directly from fenced YAML task blocks in archive markdown"
    status: pending
  - subtask_ref: actionable-cli-guidance-footer
    title: "Append guidance notes explaining how to inspect archived milestones, view active tasks, or filter results, preserving --format json milestones array"
    status: pending
```

#### Acceptance

1. `milestone list` (CLI terminal output) emits clean GitHub-Flavored Markdown blocks with section headers (`## Active Milestones`, `## Planned Milestones`) and bullet details instead of ASCII pipe tables.
2. Machine-readable JSON output (`--format json`) preserves the exact `milestones` array structure for programmatic consumers.
3. `milestone list --archive` accepts `--limit <N>` (default 10, clamped between 1 and 100) and displays total archived count vs. displayed count.
4. Archive milestone completion and task metrics are derived directly from the fenced `task_ref:` YAML blocks within archive markdown files (completed / total manifest tasks including cancelled), without querying SQLite or reading document frontmatter.
5. Terminal output includes clear next-step guidance for agents (e.g. command syntax for viewing details or querying archive history).
