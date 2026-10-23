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
3. **Archive-Authoritative Dependency Satisfaction**: Resolve completed dependencies from canonical `milestone_tombstones` or verified configured archive documents, including wildcard roots, without retained bridge tasks or a measured latency guarantee. Cancelled tasks do not receive completion credit.
4. **Idempotent 7-Case Handoff State Machine**: Handle process interruptions safely before, during, or after archive writing, torn archive post-prune repair, SQLite pruning, cross-worktree unmerged active manifests, and active file deletion, with `.bak` backup/restore compensation on write failure, eliminating dangerous reverse ref-swaps post-prune.
5. **Elimination of Git Ref Bloat (`refs/forge/evidence/*`)**: Discontinue permanent evidence refs. ADR 0015 and `docs/reference/tasks.md` establish the Markdown archive and branch history as permanent records, without promising perpetual snapshot-object reachability. Pure parentless snapshots are retained by moving `refs/forge/state/<repository>/<project>/<milestone>/<task_slug>` keepers until handoff cleanup, which preserves active foreign ownership.
6. **Acknowledged-State Recovery (`milestone db-recover`)**: Restore checkpointed active provenance after SQLite loss, subject to the Task3-local acknowledged-state contract below. Discover linked worktrees and preseed archive tombstones. SQLite COMMIT remains authoritative; pending publication retries without replaying mutations. Missing uncheckpointed legacy state, unresolved intents, and incomplete publication cohorts fail closed rather than inventing claims or guaranteeing recovery of unacknowledged lost state.
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
target: Implement archive and tombstone dependency satisfaction and the pre-ready_milestone 7-case handoff state matrix
proof_policy: seam-test-first
contract_revision: 11
scope:
- src/core/tasks/mod.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/lifecycle.rs
- src/engine/tasks/workspaces.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/mod.rs
- src/engine/milestones/integration.rs
- src/cli/milestone.rs
- tests/acdd/milestones.rs
- tests/acdd/tasks.rs
- docs/reference/tasks.md
- docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md
- skills/contextunity-forge/SKILL.md
- TESTS.md
status: completed
subtasks:
- subtask_ref: tombstone-and-archive-dependency-satisfaction
  title: Evaluate dependency satisfaction against milestone_tombstones (satisfying sentinel when all_tasks_terminal == 1) and docs/milestones/archive/*.md
  status: completed
  evidence: '{"contract_revision":11,"build_claim_revision":15,"binding_sha256":"1ef7b9c276c29fab001198dd9dbc9868b0097b9c486f52066fe3129c923384c7","baseline":"543635f2321d4cbc337428ab189a88078239e146","stage":"builder-verification-complete-independent-review-pending","checks":[{"label":"focused-1","command":"cargo test --test acdd acdd::milestones::milestone_handoff_requires_completed_tasks_and_archives_typed_receipt -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-1.log","seconds":26.39,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"1fd2788f895b23e1d51c997e089fd68c07c4f1ec4b4a8be4d494783700b4d5f8"},{"label":"focused-2","command":"cargo test --test acdd acdd::tasks::cross_milestone_dependency_diagnostic_gates_readiness_and_claim -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-2.log","seconds":2.28,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"596750eea59cd1334aa3650d22c33a2c5a8089c40b0017b604dcc75f028b6ef9"},{"label":"focused-3","command":"cargo test --test acdd acdd::milestones::default_project_alias_is_applied_once_across_task_lifecycle -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-3.log","seconds":0.89,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"28e51b8cb63e64e0f7a105bb454aea178c8a546424d1cf6e9c36b23c79c3f419"},{"label":"focused-4","command":"cargo test --test acdd acdd::milestones::subproject_milestones_handles_readme_owners_and_non_trivial_aliases -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-4.log","seconds":2.6,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"507c1ffd2d8807dc4bdbf2c06bf26993547223fbb41870f9e1b9a830acc21840"},{"label":"required-clippy","command":"cargo clippy --all-targets --all-features -- -D warnings","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-required-clippy.log","seconds":8.46,"test_counts":[],"warning_lines":0,"sha256":"04a43242b9ffa9f84167297e49237a49741b9fe3d017121fb0bd11b15c84d63a"},{"label":"required-acdd","command":"cargo test --test acdd","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-required-acdd.log","seconds":38.06,"test_counts":[{"passed":116,"failed":0,"ignored":0,"measured":0,"filtered":0}],"warning_lines":0,"sha256":"cdba1fce2eefba1a369cf288716164291c454fdaf20096fc9b66739344400cda"}],"focused_matrix":"51 dependency cases;21 recovery/terminal rows;linked foreign prepared historical target/task proof/keeper;alias-owner normal+torn+repeat","mutations":"14 runtime kills, restored; see /tmp/ac002-t2-resume11-mutations-results.json","ledger":"/tmp/ac002-t2-repair11-closure-ledger.json","packet":"/tmp/ac002-t2-repair11-review-packet.json","scope":["src/core/tasks/mod.rs","src/db/tasks_store/mod.rs","src/db/tasks_store/lifecycle.rs","src/engine/tasks/lifecycle.rs","src/engine/tasks/workspaces.rs","src/engine/milestones/handoff.rs","src/engine/milestones/mod.rs","src/engine/milestones/integration.rs","src/cli/milestone.rs","tests/acdd/milestones.rs","tests/acdd/tasks.rs","docs/reference/tasks.md","docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md","skills/contextunity-forge/SKILL.md","TESTS.md"]}'
- subtask_ref: pre-ready-handoff-state-matrix
  title: Implement 7-case handoff recovery matrix evaluated before ready_milestone with .bak backup/restore compensation on write failure, torn archive repair, and no reverse ref-swaps
  status: completed
  evidence: '{"contract_revision":11,"build_claim_revision":15,"binding_sha256":"1ef7b9c276c29fab001198dd9dbc9868b0097b9c486f52066fe3129c923384c7","baseline":"543635f2321d4cbc337428ab189a88078239e146","stage":"builder-verification-complete-independent-review-pending","checks":[{"label":"focused-1","command":"cargo test --test acdd acdd::milestones::milestone_handoff_requires_completed_tasks_and_archives_typed_receipt -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-1.log","seconds":26.39,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"1fd2788f895b23e1d51c997e089fd68c07c4f1ec4b4a8be4d494783700b4d5f8"},{"label":"focused-2","command":"cargo test --test acdd acdd::tasks::cross_milestone_dependency_diagnostic_gates_readiness_and_claim -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-2.log","seconds":2.28,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"596750eea59cd1334aa3650d22c33a2c5a8089c40b0017b604dcc75f028b6ef9"},{"label":"focused-3","command":"cargo test --test acdd acdd::milestones::default_project_alias_is_applied_once_across_task_lifecycle -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-3.log","seconds":0.89,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"28e51b8cb63e64e0f7a105bb454aea178c8a546424d1cf6e9c36b23c79c3f419"},{"label":"focused-4","command":"cargo test --test acdd acdd::milestones::subproject_milestones_handles_readme_owners_and_non_trivial_aliases -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-4.log","seconds":2.6,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"507c1ffd2d8807dc4bdbf2c06bf26993547223fbb41870f9e1b9a830acc21840"},{"label":"required-clippy","command":"cargo clippy --all-targets --all-features -- -D warnings","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-required-clippy.log","seconds":8.46,"test_counts":[],"warning_lines":0,"sha256":"04a43242b9ffa9f84167297e49237a49741b9fe3d017121fb0bd11b15c84d63a"},{"label":"required-acdd","command":"cargo test --test acdd","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-required-acdd.log","seconds":38.06,"test_counts":[{"passed":116,"failed":0,"ignored":0,"measured":0,"filtered":0}],"warning_lines":0,"sha256":"cdba1fce2eefba1a369cf288716164291c454fdaf20096fc9b66739344400cda"}],"focused_matrix":"51 dependency cases;21 recovery/terminal rows;linked foreign prepared historical target/task proof/keeper;alias-owner normal+torn+repeat","mutations":"14 runtime kills, restored; see /tmp/ac002-t2-resume11-mutations-results.json","ledger":"/tmp/ac002-t2-repair11-closure-ledger.json","packet":"/tmp/ac002-t2-repair11-review-packet.json","scope":["src/core/tasks/mod.rs","src/db/tasks_store/mod.rs","src/db/tasks_store/lifecycle.rs","src/engine/tasks/lifecycle.rs","src/engine/tasks/workspaces.rs","src/engine/milestones/handoff.rs","src/engine/milestones/mod.rs","src/engine/milestones/integration.rs","src/cli/milestone.rs","tests/acdd/milestones.rs","tests/acdd/tasks.rs","docs/reference/tasks.md","docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md","skills/contextunity-forge/SKILL.md","TESTS.md"]}'
- subtask_ref: cross-worktree-tombstone-landing-guard
  title: Detect cross-worktree unmerged active manifests when milestone is tombstoned and return MILESTONE_ALREADY_HANDED_OFF without failing ready_milestone
  status: completed
  evidence: '{"contract_revision":11,"build_claim_revision":15,"binding_sha256":"1ef7b9c276c29fab001198dd9dbc9868b0097b9c486f52066fe3129c923384c7","baseline":"543635f2321d4cbc337428ab189a88078239e146","stage":"builder-verification-complete-independent-review-pending","checks":[{"label":"focused-1","command":"cargo test --test acdd acdd::milestones::milestone_handoff_requires_completed_tasks_and_archives_typed_receipt -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-1.log","seconds":26.39,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"1fd2788f895b23e1d51c997e089fd68c07c4f1ec4b4a8be4d494783700b4d5f8"},{"label":"focused-2","command":"cargo test --test acdd acdd::tasks::cross_milestone_dependency_diagnostic_gates_readiness_and_claim -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-2.log","seconds":2.28,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"596750eea59cd1334aa3650d22c33a2c5a8089c40b0017b604dcc75f028b6ef9"},{"label":"focused-3","command":"cargo test --test acdd acdd::milestones::default_project_alias_is_applied_once_across_task_lifecycle -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-3.log","seconds":0.89,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"28e51b8cb63e64e0f7a105bb454aea178c8a546424d1cf6e9c36b23c79c3f419"},{"label":"focused-4","command":"cargo test --test acdd acdd::milestones::subproject_milestones_handles_readme_owners_and_non_trivial_aliases -- --exact","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-focused-4.log","seconds":2.6,"test_counts":[{"passed":1,"failed":0,"ignored":0,"measured":0,"filtered":115}],"warning_lines":0,"sha256":"507c1ffd2d8807dc4bdbf2c06bf26993547223fbb41870f9e1b9a830acc21840"},{"label":"required-clippy","command":"cargo clippy --all-targets --all-features -- -D warnings","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-required-clippy.log","seconds":8.46,"test_counts":[],"warning_lines":0,"sha256":"04a43242b9ffa9f84167297e49237a49741b9fe3d017121fb0bd11b15c84d63a"},{"label":"required-acdd","command":"cargo test --test acdd","exit_code":0,"log":"/tmp/ac002-t2-resume11-final-required-acdd.log","seconds":38.06,"test_counts":[{"passed":116,"failed":0,"ignored":0,"measured":0,"filtered":0}],"warning_lines":0,"sha256":"cdba1fce2eefba1a369cf288716164291c454fdaf20096fc9b66739344400cda"}],"focused_matrix":"51 dependency cases;21 recovery/terminal rows;linked foreign prepared historical target/task proof/keeper;alias-owner normal+torn+repeat","mutations":"14 runtime kills, restored; see /tmp/ac002-t2-resume11-mutations-results.json","ledger":"/tmp/ac002-t2-repair11-closure-ledger.json","packet":"/tmp/ac002-t2-repair11-review-packet.json","scope":["src/core/tasks/mod.rs","src/db/tasks_store/mod.rs","src/db/tasks_store/lifecycle.rs","src/engine/tasks/lifecycle.rs","src/engine/tasks/workspaces.rs","src/engine/milestones/handoff.rs","src/engine/milestones/mod.rs","src/engine/milestones/integration.rs","src/cli/milestone.rs","tests/acdd/milestones.rs","tests/acdd/tasks.rs","docs/reference/tasks.md","docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md","skills/contextunity-forge/SKILL.md","TESTS.md"]}'
receipt:
  contract_revision: 11
  passed_at: 2026-10-23T01:52:46.244205851+00:00
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
          evidence: 'PASS. Live contract11 scope15 matches accepted BUILD17 candidate7b8a611884b05b930d5af771920f1d9408f7df5c: all15 disk postimages/hash/mode100644/non-executable files verified. Baseline HEAD543635f unchanged. From review16 only handoff.rs topology closure and existing milestones.rs alias seam change; other13 scope paths unchanged. Production prefix72fe25f336bf5aba3def7140eb2683dca33931f3d3eaea3df2800c14a74365f2/suffix8998beb36dc55cdaa814af8278ee929ddb7f4f29f2dbc5e4c3d7aaaa189834f7 match reviewed f4 candidate. Cargo/manifest/foreign staged plan/v2 blobs preserved; staged diff9a767f2ddb9c5e54be672e7a2b0750d7fdfdbb85425c03efb5ad948331f214ac unchanged. No source/docs/tests/scope edits by reviewer.'
        claims:
          applicable: true
          evidence: PASS bounded R4 closure with prior complete review16 retained. AC5 actual topology producer1183-1194 aliases explicit raw projects once, leaves canonical item.project/parsed.project unchanged, shares branch between upstream1199/downstream1223, and classifies unresolved by difference against canonical known set1250. Existing alias seam6131-6322 adds six public task-sync/context/handoff CLI modes, asserting known+genuine missing upstream and downstream identically in response/archive/note. No prior assertion removed. Valid pre-fix red101 and four mutation runtime101 receipts verified, restoration bound current source6b8a8558. Required review cargo test --test acdd exit0:116passed0failed0filtered. Accepted exact-bound BUILD17 Clippy0warnings reused, not counted as tests. R1-R3 confirmed-fixed retained through unchanged owners/proof linkage, not reopened.
        concurrency:
          applicable: true
          evidence: PASS preserved review16 contour. R4 changes only a local pure input branch, adds no shared mutation, SQL, state or public API. Byte-identical prefix/suffix and other13 paths preserve R1 predecision foreign task/prepared handoff ownership capture at all5 cleanup callsites, actual selected target, CAS notes, exclusive cleanup/shared protection/legacy refusal. R3 terminal reconciliation before atomic repair, no-SQL postdecision recovery, backup compensation, no reverse undo and final HandoffBinding revalidation/persistence/pruning boundaries unchanged. Prior14 mutation records identical and prior independent confirmation1061 retained; not rerun.
        project_isolation:
          applicable: true
          evidence: PASS. R4 raw qualified owner takes intentional single-hop core canonical_project; unqualified dependencies use already-canonical project unchanged. Six modes cover no/single aliases, default chain, configured-owner chain, explicit old and explicit base; known/missing identities and incoming edges checked in three persisted/public representations. Core helper remains byte-identical, no transitive alias rewrite. R1 foreign project/worktree proofs and R2 wildcard/concrete discovery/DB authority remain byte-identical with review16 closure retained. Forge graph current363files9240nodes0parseerrors; AST two closure consumers; impact5nodes depth2, one public handoff caller. Macro assertions read live and graph limitations acknowledged. No whole-system alias coverage claim.
        administration:
          applicable: true
          evidence: PASS. Raw1055/1061 origins preserved; R4 classified pre-existing-missed relative first Task2 repair review, introduced relative completed Task1 baseline, not latest repair regression or incomplete R1-R3 recurrence. New closure inherits existing archive/note binding+checksum persistence exactly; R1-R3 strict terminal map, ownership note schema, generic command observations, unknown lint handling, optout, profilepins and Task1 receipt unchanged. Docs/ADR/skill/TESTS already promise canonical topology and remain byte-identical. All3 advisory subtasks completed with revision11/build15 domain evidence, retained because those production owners are unchanged; no fabricated status or receipt edits. Foreign staged proposal blank EOF line is pre-existing and outside scope; scoped diff check passes. Auto_commit true in embedded deliver gate; same reviewer continues with exact candidate, preserving foreign staged blobs.
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
    - 'decisions: {"schema": "ac002-task2-accepted-closure-v1", "task_id": "forge-mcp/m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance:archive-authoritative-dependencies-and-retry-matrix", "contract_revision": 11, "claim_revision": 18, "candidate": "7b8a611884b05b930d5af771920f1d9408f7df5c", "baseline": "543635f2321d4cbc337428ab189a88078239e146", "cumulative_dispositions": [{"id": "AC002-T2-R1", "severity": "P1", "origin": "introduced-regression", "disposition": "confirmed-fixed", "original_message": 1055, "evidence": "Current handoff_context708-725 records actual selected target; HandoffProofOwner integration213-269 is strict and bounded, stable identity/worktree replacement. active_proof_owners888-976 gathers active manifest/prepared handoff/task descriptor+gate snapshot ownership before decisions1296/1526. All5 cleanup callers1342,1346,1420,1502,1704 pass capture; cleanup979-1040 skips shared SHA/keeper refs, deletes exclusive refs and clears only outgoing ownership. Existing public CLI linked-worktree case1278-1457,1638-1710,1913-1942 includes historical selected target distinct from both HEADs, active foreign contract snapshot, same-ID keeper, unknown legacy refusal, outgoing record cleanup and later shared release. Current review116/0 executes it; six runtime101 mutation kills verified, restoration hashes match current candidate."}, {"id": "AC002-T2-R2", "severity": "P2", "origin": "introduced-regression", "disposition": "confirmed-fixed", "original_message": 1055, "evidence": "core configured_milestone_dirs1159-1232 exactly moves shipped confined expansion; engine wrapper47-49 and DB consumer1811 use same owner. ArchiveSettings1721 canonical parser plus dependency admission1173/1735 aliases explicit projects once; persisted IDs never re-aliased by reader1780. Existing cross_milestone seam8612-8744 adds17 rows across3layouts=51 through real SQLite sync/list/claim, preserving original assertions; one consumer/no bridge rows. Current review116/0 and five runtime101 mutation kills cover literal wildcard, alias admission, double alias, invalid archive and cancelled credit. R4 is a different newly detected passport-topology consumer; no claim of whole-system alias correctness."}, {"id": "AC002-T2-R3", "severity": "P1", "origin": "introduced-regression", "disposition": "confirmed-fixed", "original_message": 1055, "evidence": "TerminalTask23 requires explicit source_receipt/landed_receipt keys. Producer terminal_tasks37-72 and passport1617-1632 bind exact canonical completed/cancelled IDs/receipts; reconcile74-123 checks exact identities, status, source-or-landed receipt equivalence, landed map and duplicate/missing passport outcomes. Torn consumer1433-1493 validates note/binding/projection, parses with raw owner/default inputs, reconciles before atomic replacement and unlink, with no SQLite after1414. Current116/0 executes21 recovery rows including same-count terminal swaps and recomputed checksums plus chained alias/owner normal+torn+repeat seams. Three runtime101 mutation kills verified. No cancelled completion credit; valid repair retains exact landed receipt."}, {"id": "AC002-T2-R4", "severity": "P2", "origin": "pre-existing-missed", "origin_comparison": "Introduced by Task2 relative completed Task1 baseline543635f, present before R1-R3 repair; not introduced by latest R4 repair and not incomplete recurrence of R1-R3. Original raw1061 remains unchanged.", "disposition": "confirmed-fixed", "canonical_owner": "handoff.rs::telemetry_passport local canonical closure1183-1194", "authority": "Contract11 Task2 AC5 truthful resolved upstream/downstream topology; current user preserves intentional single-hop alias semantics.", "post_fix_invariant": "Qualified raw projects alias once; unqualified dependencies retain canonical owner. Existing canonical upstreams are known, missing ones unresolved, and incoming edges agree in response/archive/note.", "traceability": "task sync -> handoff-context exact binding -> public handoff1519 -> telemetry_passport1080 local closure -> upstream1199/downstream1223 -> canonical known difference1250 -> verified passport -> namespace Git note+archive before prune", "seam": "tests/acdd/milestones.rs::default_project_alias_is_applied_once_across_task_lifecycle, six parameter rows6131-6322 (18 representation comparisons), prior assertions retained", "regression": {"path": "/tmp/ac002-t2-r4-red2.log", "sha256": "7301593ce3d749830af1b30a505ddc172e57c1064f4980cb41c509d1684b82e4", "exit_code": 101, "passed": 0, "failed": 1}, "mutations": [{"name": "double-alias-default", "exit_code": 101, "sha256": "ac071781d411df0b3565288bd36fc42129c7960604cacb37faa44174f00fe7fc", "assertion": "panicked at tests/acdd/milestones.rs:6316:5:", "restoration_matches": true}, {"name": "skip-explicit-alias", "exit_code": 101, "sha256": "d3e88c26a0fbfe04de8e0746d04625892a31c7d0c32db118d666228f71fc1b82", "assertion": "panicked at tests/acdd/milestones.rs:6316:5:", "restoration_matches": true}, {"name": "missing-configured-incoming", "exit_code": 101, "sha256": "3f2813fab93fbf5777dec91dba3c59a4153dfdb6703059e3f711ff96660ad8c3", "assertion": "panicked at tests/acdd/milestones.rs:6316:5:", "restoration_matches": true}, {"name": "known-marked-unresolved", "exit_code": 101, "sha256": "9b5740d8d29d85f871223d65e533fceb05d00a366294d8017045fb764aacde5f", "assertion": "panicked at tests/acdd/milestones.rs:6316:5:", "restoration_matches": true}], "owner_proof": {"compared_candidate": "f4f378ab23359f2706998d62439166b3440722bc", "source_change_only_topology_closure": true, "prefix_sha256": "72fe25f336bf5aba3def7140eb2683dca33931f3d3eaea3df2800c14a74365f2", "suffix_sha256": "8998beb36dc55cdaa814af8278ee929ddb7f4f29f2dbc5e4c3d7aaaa189834f7", "old_closure_sha256": "21984dbb7de466b0be8d2c236cab9e69153dc64f74c68b1652da17d0d171ac03", "new_closure_sha256": "bc70b7e875ca3fb1e06dd4ff5fa8f958f81c819d2934726a869e2d6e43e9fb3d", "all_other13_scope_files_unchanged": true, "changed_paths_this_wave": ["src/engine/milestones/handoff.rs", "tests/acdd/milestones.rs"], "r1_r2_r3": "All production bytes outside local topology closure identical, including cleanup/ownership, core wildcard expansion, DB archive authority, terminal reconciliation and passport persistence. Prior14 independently confirmed mutation receipts reused, not rerun.", "protected": {"files": {"Cargo.toml": "6ff4851c8907d9f0ed900bd6d66917eba643d1fc781228b46606aa39a2376921", "docs/milestones/AC-002-milestone-handoff-pruning-evidence-cleanup-and-list-guidance.md": "aacf6b9eedbc805e260fe30ae3c8569945e238ff9e819696163b1c55afb36189", "docs/plans/milestone-handoff-pruning-and-list-guidance.md": "9f90aabdb8daa30cc83f177279e226e53bd7e34d92c2db4e501e2b1743ee73a8", "skills/contextunity-forge/references/acdd_profile.yaml.example": "c77e4728c4300c1ea15923757f6a9a77970ff5c6d9535e9932392990353ef4c6"}, "staged_diff_sha256": "9a767f2ddb9c5e54be672e7a2b0750d7fdfdbb85425c03efb5ad948331f214ac"}}, "remaining_scope": "Claim bounded to this Task2 passport producer and exercised modes; no whole-system alias, Task3/Task4, AC001/v2 or recovery expansion."}], "review_contours": {"paths": {"applicable": true, "evidence": "PASS. Live contract11 scope15 matches accepted BUILD17 candidate7b8a611884b05b930d5af771920f1d9408f7df5c: all15 disk postimages/hash/mode100644/non-executable files verified. Baseline HEAD543635f unchanged. From review16 only handoff.rs topology closure and existing milestones.rs alias seam change; other13 scope paths unchanged. Production prefix72fe25f336bf5aba3def7140eb2683dca33931f3d3eaea3df2800c14a74365f2/suffix8998beb36dc55cdaa814af8278ee929ddb7f4f29f2dbc5e4c3d7aaaa189834f7 match reviewed f4 candidate. Cargo/manifest/foreign staged plan/v2 blobs preserved; staged diff9a767f2ddb9c5e54be672e7a2b0750d7fdfdbb85425c03efb5ad948331f214ac unchanged. No source/docs/tests/scope edits by reviewer."}, "claims": {"applicable": true, "evidence": "PASS bounded R4 closure with prior complete review16 retained. AC5 actual topology producer1183-1194 aliases explicit raw projects once, leaves canonical item.project/parsed.project unchanged, shares branch between upstream1199/downstream1223, and classifies unresolved by difference against canonical known set1250. Existing alias seam6131-6322 adds six public task-sync/context/handoff CLI modes, asserting known+genuine missing upstream and downstream identically in response/archive/note. No prior assertion removed. Valid pre-fix red101 and four mutation runtime101 receipts verified, restoration bound current source6b8a8558. Required review cargo test --test acdd exit0:116passed0failed0filtered. Accepted exact-bound BUILD17 Clippy0warnings reused, not counted as tests. R1-R3 confirmed-fixed retained through unchanged owners/proof linkage, not reopened."}, "concurrency": {"applicable": true, "evidence": "PASS preserved review16 contour. R4 changes only a local pure input branch, adds no shared mutation, SQL, state or public API. Byte-identical prefix/suffix and other13 paths preserve R1 predecision foreign task/prepared handoff ownership capture at all5 cleanup callsites, actual selected target, CAS notes, exclusive cleanup/shared protection/legacy refusal. R3 terminal reconciliation before atomic repair, no-SQL postdecision recovery, backup compensation, no reverse undo and final HandoffBinding revalidation/persistence/pruning boundaries unchanged. Prior14 mutation records identical and prior independent confirmation1061 retained; not rerun."}, "project_isolation": {"applicable": true, "evidence": "PASS. R4 raw qualified owner takes intentional single-hop core canonical_project; unqualified dependencies use already-canonical project unchanged. Six modes cover no/single aliases, default chain, configured-owner chain, explicit old and explicit base; known/missing identities and incoming edges checked in three persisted/public representations. Core helper remains byte-identical, no transitive alias rewrite. R1 foreign project/worktree proofs and R2 wildcard/concrete discovery/DB authority remain byte-identical with review16 closure retained. Forge graph current363files9240nodes0parseerrors; AST two closure consumers; impact5nodes depth2, one public handoff caller. Macro assertions read live and graph limitations acknowledged. No whole-system alias coverage claim."}, "administration": {"applicable": true, "evidence": "PASS. Raw1055/1061 origins preserved; R4 classified pre-existing-missed relative first Task2 repair review, introduced relative completed Task1 baseline, not latest repair regression or incomplete R1-R3 recurrence. New closure inherits existing archive/note binding+checksum persistence exactly; R1-R3 strict terminal map, ownership note schema, generic command observations, unknown lint handling, optout, profilepins and Task1 receipt unchanged. Docs/ADR/skill/TESTS already promise canonical topology and remain byte-identical. All3 advisory subtasks completed with revision11/build15 domain evidence, retained because those production owners are unchanged; no fabricated status or receipt edits. Foreign staged proposal blank EOF line is pre-existing and outside scope; scoped diff check passes. Auto_commit true in embedded deliver gate; same reviewer continues with exact candidate, preserving foreign staged blobs."}}, "checks": {"command": "cargo test --test acdd", "exit_code": 0, "passed": 116, "failed": 0, "filtered": 0, "log": "/tmp/ac002-t2-r4-reviewer-acdd.log", "sha256": "5a45b754bc3b6189f10bb647b4210f6aaa94b3558263c03cf736c9913b41155c"}, "accepted_lint": "BUILD17 strictClippy0warnings exact-bound", "authority": "Existing Task2 AC1-5 only; raw1055/1061 preserved; R1-R3 confirmed-fixed at16 retained; R4 pre-existing-missed confirmed-fixed at18."}'
    - 'deferred: {"task_id": "forge-mcp/m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance:archive-authoritative-dependencies-and-retry-matrix", "contract_revision": 11, "candidate": "7b8a611884b05b930d5af771920f1d9408f7df5c", "owner": "coordinator and admitted successor task owners", "destination": "Task3 keeper/database recovery; Task4 Markdown lists/formatting; AC001/future-v2 remain separate", "status": "not implemented or accepted by Task2", "learning": "Raw configured project input and canonical namespace are distinct; R4 matrix proves only this passport owner. No new rule or scope amendment."}'
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
target: Amend ADR 0015 and tasks.md, retire fail-closed evidence-ref check in handoff.rs, prune milestone proof refs and unpinned evidence, establish keeper refs, and implement multi-worktree milestone db-recover
proof_policy: seam-test-first
contract_revision: 10
invariants:
- 'INV-KEEPER-SQLITE-COMMIT: SQLite remains the logical commit owner; task keepers checkpoint committed task state and never authorize an uncommitted gate. No store-wide ref, synthetic task owner, or Git-first decision journal is introduced.'
- 'INV-KEEPER-ACKNOWLEDGED-RECOVERY: Recover acknowledged active task, claim, revision and gate provenance. Publish complete task checkpoints before managed success or observation of new state; refuse pending reads without taking a writer lock. If SQL is lost during an unresolved checkpoint intent, fail closed without reviving an old claim or fabricating the unacknowledged outcome.'
- 'INV-KEEPER-TASK-FENCE: Genuine task-owned intent and revision reservations may precede SQL commit solely to fence stale claims and detect ambiguity. They are not accepted gate evidence. Preserve maximum allocated revisions and reject incomplete publication cohorts.'
- 'INV-KEEPER-LEGACY-ENROLLMENT: Enroll healthy existing task state before retiring legacy pins or promising recoverability. Already-lost uncheckpointed runtime state is explicitly unrecoverable; manifests do not recreate claims or proof history.'
- 'INV-KEEPER-AUTOMATIC-ORPHAN-RECOVERY: Plain milestone db-recover uses healthy SQLite authority to rewind orphaned precommit refs to acknowledged ref@{1}, end interrupted submit claims and advance revisions without accepting provisional proofs. Obsolete archives without handoff.proof_shas and their stale active copies supply no recovery authority; missing SQL with an unresolved intent remains fail-closed.'
scope:
- src/core/git.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/lifecycle.rs
- src/db/tasks_store/subtasks.rs
- src/db/tasks_store/recovery.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/integration.rs
- src/engine/milestones/mod.rs
- src/engine/milestones/recovery.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/lifecycle.rs
- src/engine/tasks/gates.rs
- src/engine/tasks/workspaces.rs
- src/engine/tasks/git_snapshot.rs
- src/cli/milestone.rs
- src/cli/task.rs
- tests/acdd/milestones.rs
- tests/acdd/tasks.rs
- tests/acdd/subtasks.rs
- skills/contextunity-forge/SKILL.md
- docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md
- docs/reference/tasks.md
- docs/reference/cli.md
subtasks:
- subtask_ref: adr-0015-and-tasks-doc-proof-retention-amendment
  title: Amend ADR 0015, docs/reference/tasks.md, and SKILL.md to define archive manifests and branch commits as permanent proof storage while deprecating refs/forge/evidence
  status: completed
  evidence: 'T3-CR9-B18-docs: scope23 tasks reference reconciles strict historical proof lists, stage-specific checkpoint authority, bounded provisional kept-pack retention and Pending publication. Required strict Clippy0 and ACDD116/0 on unchanged content; new review findings pending independent confirmation.'
- subtask_ref: moving-keeper-ref-lifecycle
  title: Keep candidate snapshots parentless, point moving keeper ref refs/forge/state/<milestone>/<task_slug> at candidate parents to prevent git gc, and delete in all handoff completion paths
  status: completed
  evidence: 'T3-CR9-B18-keeper: actual parentless construction under GC exclusion transfers to genuine task-owned kept pack before unchanged forced-GC commit-tree wrapper; real GC exits0 and object survives. Post-SQL Pending retains pack across process exit and publication-only db-recover removes it after actual keeper parents. Three R3 runtime mutation kills; prior foreign-owner confirmations retained.'
- subtask_ref: retire-evidence-check-and-prune-legacy-refs
  title: Retire fail-closed evidence-ref check in handoff.rs, prune milestone-scoped proof refs (from task_gates or archive manifest), and clean unpinned evidence refs via db-recover
  status: completed
  evidence: 'T3-CR9-B18-legacy: shared strict proof_shas across handoff/recovery; 8 parser cases and permissive-parser mutation kill. Absent-only fallback, present empty exact list, malformed metadata rejects before promotion/deletion, historical missing objects allowed. Prior Task2 foreign ownership independently confirmed; full ACDD116/0.'
- subtask_ref: milestone-db-recover-command
  title: Implement milestone db-recover CLI command discovering worktrees via git worktree list --porcelain, pre-seeding tombstones from archive manifests across all worktrees, and reconstructing SQLite task store from active manifests and state refs
  status: completed
  evidence: 'T3-CR9-B18-recovery: public db-recover checkpoint tamper matrix 12 variants x passed/rejected gates (24 refusals), valid recovery/inspect/idempotent retry, pure pinned GateDef proof validation and exact claim/submission transitions. Two R1 mutation kills. Prior owner isolation/1073 independent confirmations retained; full ACDD116/0; current findings pending independent review.'
status: completed
receipt:
  contract_revision: 10
  passed_at: 2026-10-23T15:04:02.369252851+00:00
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
          evidence: All 23 admitted Task3 postimages exactly match candidate 092efd88e6107bea92b89318c5ee5fc1f593997c. Scoped diff SHA256 versus baseline HEAD3939e5de is 598d0e55568a941bd512595788886600ed897c87ef379e0ebd6159ad2f17752d. Task5 five-path candidate remains unchanged. No reviewer edits, Git mutations, delivery, repack, or handoff.
        claims:
          applicable: true
          evidence: 'Raw1110 R1/R2 are closed by current source and public seam evidence: exact acknowledged/SQL preimage plus bounded reservation/live claim/token/cohort validation prevents arbitrary claim revocation; transient canonical obsolete-owner classification excludes typed/corrupt keeper payloads before reads, mutation, enrollment and restoration without granting dependency credit. Present current archive proof_shas remains strict. Independent cargo test --test acdd passed116/0 in705.87s; accepted BUILD strictClippy0 and focused mutation/replay evidence reviewed. Task blackboard1115.'
        concurrency:
          applicable: true
          evidence: SQLite remains logical owner. Genuine orphan recovery keeps intent fencing through SQL end_claim/revision commit, then CAS-rewinds the exact observed tip to exact ref@{1}. Exact already-released SQL recognition makes retry after COMMIT/Git failure CAS-only with the same revision and no double release. Conflicting tokens, invalid bounds, changed preimages and incomplete prior cohorts fail before SQL/ref mutation.
        project_isolation:
          applicable: true
          evidence: Owner selection uses canonical effective repository/default-project/owners/aliases and configured milestone roots across equivalent Git worktrees; no linked profile compilation or obsolete TaskSpec parsing occurs. Prefixes include complete repository/project/milestone identities. Headerless/unidentified historical garbage receives no namespace, while recognized obsolete owners are ignored unchanged in healthy recovery, composed reads and missing/corrupt restoration.
        administration:
          applicable: true
          evidence: No manual recovery flag/API, schema, global journal, DB-to-engine dependency, rollback state machine, profile-v2 runtime, receipt acceptance, or unrelated IN001 change was introduced. Missing/corrupt SQL with pending intent remains fail-closed. Affected documentation scan against main95d4e052 is milestone blackboard1114; no documentation edits needed.
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
    - 'INV-KEEPER-SQLITE-COMMIT: SQLite remains the logical commit owner; task keepers checkpoint committed task state and never authorize an uncommitted gate. No store-wide ref, synthetic task owner, or Git-first decision journal is introduced.'
    - 'INV-KEEPER-ACKNOWLEDGED-RECOVERY: Recover acknowledged active task, claim, revision and gate provenance. Publish complete task checkpoints before managed success or observation of new state; refuse pending reads without taking a writer lock. If SQL is lost during an unresolved checkpoint intent, fail closed without reviving an old claim or fabricating the unacknowledged outcome.'
    - 'INV-KEEPER-TASK-FENCE: Genuine task-owned intent and revision reservations may precede SQL commit solely to fence stale claims and detect ambiguity. They are not accepted gate evidence. Preserve maximum allocated revisions and reject incomplete publication cohorts.'
    - 'INV-KEEPER-LEGACY-ENROLLMENT: Enroll healthy existing task state before retiring legacy pins or promising recoverability. Already-lost uncheckpointed runtime state is explicitly unrecoverable; manifests do not recreate claims or proof history.'
    - 'INV-KEEPER-AUTOMATIC-ORPHAN-RECOVERY: Plain milestone db-recover uses healthy SQLite authority to rewind orphaned precommit refs to acknowledged ref@{1}, end interrupted submit claims and advance revisions without accepting provisional proofs. Obsolete archives without handoff.proof_shas and their stale active copies supply no recovery authority; missing SQL with an unresolved intent remains fail-closed.'
    architectural_notes: []
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

1. ADR 0015, `docs/reference/tasks.md`, and `skills/contextunity-forge/SKILL.md` are amended to record that durable proofs reside in the markdown archive and branch history; creation of permanent `refs/forge/evidence/<sha>` Git references is removed.
2. In `src/engine/milestones/handoff.rs`, the fail-closed expectation of `refs/forge/evidence/` after handoff is retired. Handoff renders proof SHAs (gate `commit`, `candidate_baseline_head`, receipt `commit`, `handoff.commit`, `target_base`) into `handoff.proof_shas` before deleting SQLite rows, and in all completion and recovery paths executes `git update-ref -d refs/forge/evidence/<sha>` idempotently using the single fallback rule (read `handoff.proof_shas`; if absent, take `commit`, `candidate_baseline_head`, and 40-char hex SHAs; missing refs succeed), unpinning proof objects.
3. Candidate snapshot commits remain parentless (created via `git commit-tree` without `-p`). A moving keeper ref `refs/forge/state/<milestone>/<task_slug>` points to a lightweight commit listing the in-flight candidate snapshots as parents (`-p`), keeping them reachable from `git gc` during active execution without mutating candidate commit SHAs.
4. All `milestone handoff` completion and post-prune recovery paths delete all `refs/forge/state/<milestone>/*` refs for that milestone via `git update-ref -d` idempotently (missing refs succeed).
5. Command `contextunity-forge-mcp milestone db-recover` executes in strict deterministic order: (1) runs `git worktree list --porcelain` to discover all linked worktree paths; (2) inspects `.forge/tasks.sqlite`: if corrupt or unreadable, renames to `.forge/tasks.sqlite.corrupt.<timestamp>`, then opens or initializes empty schema v6 SQLite database via `TasksStore::open_project` before tombstone pre-seeding; (3) scans `docs/milestones/archive/*.md` across every discovered worktree root to pre-seed `milestone_tombstones` (parsing frontmatter: `status: completed` sets `all_tasks_terminal = 1`, `status: cancelled` sets `all_tasks_terminal = 0`, and extracting completed task IDs from task blocks) FIRST; (4) imports active manifests across discovered worktrees only after tombstones are seeded, strictly skipping any milestone whose ID exists in `milestone_tombstones` or whose path matches `is_archived_path`; (5) reconstructs active tasks, claims, and revisions from active manifests and in-flight `refs/forge/state/` pointers; (6) idempotently deletes legacy `refs/forge/evidence/<sha>` refs using the single fallback rule across archive manifests (read `handoff.proof_shas`; if absent, take `commit`, `candidate_baseline_head`, and 40-char hex SHAs; missing refs succeed) and orphan keeper refs `refs/forge/state/<milestone>/*` for tombstoned milestones (missing refs succeed).
6. With healthy SQL, automatically resolve orphaned precommit submit intents without a CLI abandonment flag: rewind each keeper to its acknowledged `ref@{1}`, preserve consumed reservations, call `end_claim`, clear worker/worktree ownership, advance the interrupted claim revision, and select `ready` or `parked`. Commit the claim release before ref rollback so retries cannot release it twice. Skip obsolete archives lacking `handoff.proof_shas` before task parsing, validation and tombstone seeding; skip their stale active copies and keeper namespaces during restoration. Present malformed/null proof lists remain errors. Never fabricate a gate result or infer an unacknowledged outcome after SQL loss.

---

### task: markdown-milestone-list-and-archive-limit

```yaml
task_ref: markdown-milestone-list-and-archive-limit
target: Refactor milestone list to Markdown-first format, add archive pagination limit, and provide actionable agent guidance
proof_policy: seam-test-first
contract_revision: 9
scope:
- src/cli/milestone.rs
- src/cli/mod.rs
- src/core/format/mod.rs
- src/engine/milestones/mod.rs
- src/mcp/tools.rs
- tests/acdd/milestones.rs
- docs/reference/cli.md
status: completed
subtasks:
- subtask_ref: markdown-card-formatting
  title: Render active and planned milestones as GitHub-Flavored Markdown blocks with status indicators, clickable paths, and no pipe tables
  status: completed
  evidence: 'CR9 BUILD12: existing public CLI seam runtime red101 for escaped generic table, green GFM Active/Planned/Completed sections, empty sections, status/start/completion and real encoded clickable file URI. JSON10-key rows unchanged; formatter pure and CLI identity separate. Full ACDD116/0; strict Clippy0.'
- subtask_ref: archive-limit-and-task-block-stats
  title: Add --limit <N> (default 10, clamped 1..=100) for archive listing and calculate completion directly from fenced YAML task blocks in archive markdown
  status: completed
  evidence: 'CR9 BUILD12: same public seam102 isolated archives; default10,0->1,1,100,101->100; active/planned never limited. Archived statuses/completion derive fenced task YAML:1completed+1cancelled+1planned=>1/3 despite fake frontmatter99/99 and unreadable SQLite sentinel unchanged. Statuscompleted implicit archive and Markdown1of102 counts. Two runtime101 mutation kills: limit minimum10 and restored archiveSQL read; mutations restored. Full ACDD116/0 Clippy0.'
- subtask_ref: actionable-cli-guidance-footer
  title: Append guidance notes explaining how to inspect archived milestones, view active tasks, or filter results, preserving --format json milestones array
  status: completed
  evidence: CR9 BUILD12 same supported CLI seam asserts milestone show/task list/archive/status command guidance; cli.md coevolved with limit semantics, task denominator, stable path order and JSON top-level aligned documents/count metadata. No MCP milestone tool added, no change to task-list API. Full ACDD116/0 Clippy0; no benchmarks.
receipt:
  contract_revision: 9
  passed_at: 2026-10-23T09:51:06.889330481+00:00
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
          evidence: All seven scoped candidate postimages exactly match snapshot 1bc8261c6ad4366adb883976ee66324a94452e37; six files differ from baseline, src/mcp/tools.rs is unchanged, and diff-check is clean. Repair delta is confined to src/core/format/mod.rs and the existing tests/acdd/milestones.rs seam, with only nonsemantic canonical style2021 formatting elsewhere in that admitted test file.
        claims:
          applicable: true
          evidence: 'R1 raw1093 is closed: renderer locally normalizes manifest display text to one line, escapes structural Markdown in headings and link labels, preserves raw JSON/path bytes and canonical URI encoding, and hostile public CLI coverage proves literal GFM, clickable encoded original path, no injected headings, and unchanged JSON. Complete cargo test --test acdd passed 116/0 in 386.02s on the frozen candidate; accepted final-binding strict Clippy evidence is exit0 with zero warnings.'
        concurrency:
          applicable: false
          evidence: Milestone list remains read-only under the existing checkpoint fence; no transaction, claim, rollback, lock-order, concurrent mutation, or namespace behavior changed.
        project_isolation:
          applicable: true
          evidence: Renderer consumes aligned row/document data only, preserves workspace-relative JSON paths and original absolute path bytes for canonical file URI encoding, and introduces no cross-workspace lookup or mutable shared-store access.
        administration:
          applicable: false
          evidence: No schema, migration, profile pin, hook, receipt, snapshot-parent, delivery, privileged configuration, Git mutation, or administrative recovery behavior changed.
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
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `milestone list` (CLI terminal output) emits clean GitHub-Flavored Markdown blocks with section headers (`## Active Milestones`, `## Planned Milestones`) and bullet details instead of ASCII pipe tables.
2. Machine-readable JSON output (`--format json`) preserves the exact `milestones` array structure for programmatic consumers.
3. `milestone list --archive` accepts `--limit <N>` (default 10, clamped between 1 and 100) and displays total archived count vs. displayed count.
4. Archive milestone completion and task metrics are derived directly from the fenced `task_ref:` YAML blocks within archive markdown files (completed / total manifest tasks including cancelled), without querying SQLite or reading document frontmatter.
5. Terminal output includes clear next-step guidance for agents (e.g. command syntax for viewing details or querying archive history).

### task: reconcile-delivery-configuration-and-documentation

```yaml
task_ref: reconcile-delivery-configuration-and-documentation
target: Admit and verify the existing test-profile adjustment, delivered plan, and current-versus-proposed profile documentation without implementing profile-v2 runtime
proof_policy: direct-proof
contract_revision: 2
scope:
- Cargo.toml
- src/core/tasks/profile.rs
- docs/plans/milestone-handoff-pruning-and-list-guidance.md
- docs/reference/profiles.md
- skills/contextunity-forge/references/acdd_profile.yaml.example
depends_on: []
status: completed
receipt:
  contract_revision: 2
  passed_at: 2026-10-23T14:45:11.901390704+00:00
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
          evidence: All five admitted postimages exactly match candidate 95525a42ba9b5c5bf261e4ecc36f6945bc1da8f7 and current baseline HEAD3939e5de. No profile-v2 runtime, neighboring configuration, manifest receipt, source, or review edits were introduced.
        claims:
          applicable: true
          evidence: Cargo test profile adjustment, current profile parser owner, completed plan, profile reference, and future-v2 proposal are consistently separated. docs/reference/profiles links the executable current example and explicitly labels acdd_profile.yaml.example unsupported future v2. Independent cargo test --test acdd passed 116/0 in 394.06s; accepted strict Clippy0 and focused profile seam1/0 evidence reused.
        concurrency:
          applicable: false
          evidence: Supplemental admission changes no runtime transaction, claim, checkpoint, receipt, or concurrent mutation behavior; src/core/tasks/profile.rs postimage is existing unchanged runtime dependency evidence only.
        project_isolation:
          applicable: true
          evidence: Profile documentation preserves active-root loading precedence, linked-workspace override exclusion, path pinning, SHA tamper checks, and protected .forge/acdd scope. Candidate contains no external or sibling workspace changes.
        administration:
          applicable: true
          evidence: Current executable schema remains distinct from the unimplemented v2 proposal; profile pins, gate references, terminal delivery requirements, auto_commit, hook and receipt behavior are documented without implementing v2. Independent branch-wide documentation scan is milestone blackboard1111.
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
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Preserve the existing explicit test-profile `opt-level = 0` adjustment and verify supported task-profile resolution with existing production-seam tests; make no benchmark or latency claim.
   `src/core/tasks/profile.rs` is the unchanged production dependency connecting the profile seam; it admits verification, not profile-v2 implementation.
2. Keep the runnable current example separate from the preserved future-v2 proposal, explicitly label the proposal unsupported by the current parser, and point executable-profile guidance at the runnable example. Do not implement any profile-v2 runtime capability.
3. Reconcile the delivered plan with accepted task proofs and acknowledged-state recovery. Preserve all four completed task contracts and receipts, and include this bounded supplemental adjustment in independent review and final integration verification.

#### Recovery Verification

The supported plain `milestone db-recover` resolves the interrupted supplemental
submit using healthy SQL and acknowledged keeper history. Its claim returns to
`ready` / `contract`, revision 5, with no accepted gate, without manual SQLite,
keeper, receipt or archive edits. The same operation succeeds twice on the copied
28-worktree topology; missing/corrupt SQL with that unresolved intent refuses
`TASK_RECOVERY_AMBIGUOUS`. The focused crash seam, strict Clippy and full ACDD
target pass before live recovery. Task3 contract 10 owns this bounded repair;
the supplemental task owns the existing configuration/documentation adjustment.
