---
id: m-acdd-lifecycle-snapshots-and-proactive-defect-resolution
title: ACDD lifecycle snapshots, hierarchical scope conflict governance, and proactive defect resolution
doc_type: contract
status: completed
depends_on:
- m-unified-agent-context-and-task-lifecycle:completed
owners:
- src/core/tasks/
- src/db/tasks_store.rs
- src/engine/tasks.rs
- src/engine/tasks/
- src/engine/milestones.rs
- src/mcp/
- docs/
- tests/
invariants:
- 'INV-ROOT-SNAPSHOT-DETERMINISM: Pre-delivery candidate code states are captured as deterministic root tree Git snapshots (without -p HEAD) under refs/forge/snapshots/, ensuring cryptographic determinism independent of sibling branch commits in shared worktrees.'
- 'INV-SNAPSHOT-REF-PRESERVATION: Candidate snapshot refs remain pinned and immutable across review and deliver gates; verification compares working tree states without shifting refs prior to gate acceptance.'
- 'INV-HIERARCHICAL-SCOPE-CONFLICT: Scope expansion via extend_scope enforces hierarchical conflict detection against all tasks in the milestone, rejecting exact matches, subdirectory enclosures, and ancestor collisions across both frozen and unfrozen paths with TASK_SCOPE_CONFLICT.'
- 'INV-TYPED-DEFERRED-DEFECTS: Milestone contracts maintain a typed block under tasks for out-of-scope defects and deferred review findings, ingested into milestone metadata and preserved across receipt writes.'
- 'INV-UNIFIED-CONTEXT-DEFAULT: Task claim provides the full context bundle by default (ADRs, symbols, tests, blackboard), eliminating procedural instruction overhead.'
started_at: 2026-10-07T07:35:00+00:00
handoff:
  completed_at: 2026-10-07T08:47:45.825346295+00:00
  duration: 1h 12m
  commit: fb5776a332fa2e54ce9beb0d2c984fc17b3ab224
  verification:
    command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
    status: passed
    tests_passed: 600
    tests_failed: 0
---

# ACDD lifecycle snapshots, hierarchical scope conflict governance, and proactive defect resolution

## Outcome and purpose

Decouple intermediate gate verification from repository branch history using deterministic root Git snapshots, preserve snapshot refs from premature displacement or garbage collection, govern concurrent multi-agent task scopes with hierarchical conflict detection, provide a typed home for deferred out-of-scope defects directly in milestone specifications, and make unified zero-shot context bundling the default behavior on task claims.

## Tasks in this milestone

### task: acdd-root-snapshots-and-candidate-preservation

```yaml
task_ref: acdd-root-snapshots-and-candidate-preservation
target: Implement deterministic root tree Git snapshots without parent coupling, make evidence commit optional on contract and build, and preserve candidate snapshot refs across review and deliver gates
agent_type: worker
proof_policy: direct-proof
contract_revision: 1
scope:
- src/core/tasks/gates.rs
- src/engine/tasks.rs
- src/engine/tasks/workspaces.rs
- tests/core_basics/tasks.rs
status: completed
receipt:
  commit: 8902c53a6e9c0d95f10d23867d85b15608d31755
  contract_revision: 1
  passed_at: 2026-10-07T08:16:58.395058683+00:00
  evidence:
    test_proof:
      command: cargo test --test core_basics && cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 49
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Candidate root snapshot 8902c53 contains only the four declared paths: src/core/tasks/gates.rs, src/engine/tasks.rs, src/engine/tasks/workspaces.rs, and tests/core_basics/tasks.rs.'
        claims:
          applicable: true
          evidence: The focused production seam proves sibling commits do not affect the root SHA, untracked scoped files are captured, failed pin leaves contract/v1 unpersisted, retry pins, and refs survive review/deliver until handoff cleanup.
        concurrency:
          applicable: true
          evidence: Snapshot index files use create_new with a process-local AtomicU64 sequence; the Git ref pin runs under the SQLite immediate gate transaction and failures roll back gate state.
        project_isolation:
          applicable: true
          evidence: Registry ownership/worktree scope checks remain before submission; snapshot refs are keyed by repository/project/milestone/task identity.
        administration:
          applicable: true
          evidence: Evidence.commit is optional and auto-captured; candidate objects and update-ref exit status are checked, with TASK_SNAPSHOT_REF_FAILED on failure. Full core_basics 49/49 and strict Clippy passed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ROOT-SNAPSHOT-DETERMINISM: Pre-delivery candidate code states are captured as deterministic root tree Git snapshots (without -p HEAD) under refs/forge/snapshots/, ensuring cryptographic determinism independent of sibling branch commits in shared worktrees.'
    - 'INV-SNAPSHOT-REF-PRESERVATION: Candidate snapshot refs remain pinned and immutable across review and deliver gates; verification compares working tree states without shifting refs prior to gate acceptance.'
    - 'INV-HIERARCHICAL-SCOPE-CONFLICT: Scope expansion via extend_scope enforces hierarchical conflict detection against all tasks in the milestone, rejecting exact matches, subdirectory enclosures, and ancestor collisions across both frozen and unfrozen paths with TASK_SCOPE_CONFLICT.'
    - 'INV-TYPED-DEFERRED-DEFECTS: Milestone contracts maintain a typed block under tasks for out-of-scope defects and deferred review findings, ingested into milestone metadata and preserved across receipt writes.'
    - 'INV-UNIFIED-CONTEXT-DEFAULT: Task claim provides the full context bundle by default (ADRs, symbols, tests, blackboard), eliminating procedural instruction overhead.'
    architectural_notes:
    - Candidate snapshots use an empty temporary Git index and deterministic root commits; temp index files are exclusively created with a process-local sequence. For contract/build, the engine calls a Git pin hook only after typed proof validation while the SQLite immediate transaction is held; missing objects and update-ref failures abort gate persistence. Review/deliver reuse the accepted build SHA without moving the ref. The focused production seam covers sibling commits, untracked scope files, no parent, pin failure rollback/retry, preservation, and handoff cleanup.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
subtasks:
- subtask_ref: exclude-sibling-head-content-from-root-snapshots
  title: When contract/build snapshots start from a committed task tree, exclude unrelated HEAD-only paths so identical scoped source produces the same candidate SHA. Verify through tests/core_basics/tasks.rs::scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff by committing README.md outside scope between contract/build and asserting equal snapshot SHAs, no README in git show, and no parent.
  status: completed
  evidence: 'cargo test --test core_basics tasks::scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff: exit 0, 1 passed; the pre-fix run exited 101 because unrelated README.md changed the contract/build SHA. Post-fix assertions pass for equal scoped SHA, omission of README.md, no parent, and included untracked source. cargo test --test core_basics: exit 0, 49 passed. cargo clippy --all-targets --all-features -- -D warnings: exit 0.'
- subtask_ref: pin_candidate_refs_before_gate_persistence
  title: On contract/build pass submissions with a Git candidate, validate the candidate object and update refs/forge/snapshots only after typed proof validation but before persisting the SQLite gate; a failed object/ref update must leave the same claim at the same gate. Verify through tests/core_basics/tasks.rs::scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff by forcing a nested ref-path collision, asserting contract/v1 remains in_progress, then removing the collision and asserting a retry passes with the pinned candidate.
  status: completed
  evidence: 'tests/core_basics/tasks.rs::scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff passed 1/1 with the forced refs/forge/snapshots path collision: TASK_SNAPSHOT_REF_FAILED left status=in_progress and gate=contract/v1; retry after removal passed and pinned the candidate. cargo test --test core_basics passed 49/49. cargo clippy --all-targets --all-features -- -D warnings passed. cargo build --bin contextunity-forge-mcp and git diff --check passed.'
```

Decouple candidate snapshots from `HEAD` branch commits by capturing root tree commits under `refs/forge/snapshots/`. Make `evidence.commit` optional on `contract/v1` and `build/v1`, auto-populating from the captured snapshot SHA. Preserve candidate refs on `review/v1` and `deliver/v1` without updating refs before proof validation.

### task: hierarchical-scope-conflict-detection

```yaml
task_ref: hierarchical-scope-conflict-detection
target: Implement hierarchical scope conflict detection in extend_scope and govern unowned path admission
agent_type: worker
proof_policy: direct-proof
contract_revision: 1
scope:
- src/db/tasks_store.rs
- tests/core_basics/tasks.rs
status: completed
receipt:
  commit: b57555d9ee83977b8dafc99f1553117d01366cd8
  contract_revision: 1
  passed_at: 2026-10-07T08:19:01.312231581+00:00
  evidence:
    test_proof:
      command: cargo test --test core_basics && cargo clippy --all-targets --all-features -- -D warnings && git diff --check
      exit_code: 0
      tests_passed: 49
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate snapshot b57555d contains only src/db/tasks_store.rs and tests/core_basics/tasks.rs, matching the declared scope.
        claims:
          applicable: true
          evidence: The public manage seam covers exact/child/parent conflicts against frozen and unfrozen owner paths, identifies the owner, and verifies unowned src/tests additions remain frozen=0.
        concurrency:
          applicable: true
          evidence: extend_scope performs sibling-scope read/check/insert under BEGIN IMMEDIATE, serializing competing scope expansions before ownership changes.
        project_isolation:
          applicable: true
          evidence: assert_id enforces the store namespace; conflict lookup joins tasks by the same milestone_ref and excludes only the current task.
        administration:
          applicable: true
          evidence: Unowned extensions remain constrained to src/ or tests/, symlink confinement still applies, and all failure paths report TASK_SCOPE_CONFLICT. Core suite 49/49 and strict Clippy passed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ROOT-SNAPSHOT-DETERMINISM: Pre-delivery candidate code states are captured as deterministic root tree Git snapshots (without -p HEAD) under refs/forge/snapshots/, ensuring cryptographic determinism independent of sibling branch commits in shared worktrees.'
    - 'INV-SNAPSHOT-REF-PRESERVATION: Candidate snapshot refs remain pinned and immutable across review and deliver gates; verification compares working tree states without shifting refs prior to gate acceptance.'
    - 'INV-HIERARCHICAL-SCOPE-CONFLICT: Scope expansion via extend_scope enforces hierarchical conflict detection against all tasks in the milestone, rejecting exact matches, subdirectory enclosures, and ancestor collisions across both frozen and unfrozen paths with TASK_SCOPE_CONFLICT.'
    - 'INV-TYPED-DEFERRED-DEFECTS: Milestone contracts maintain a typed block under tasks for out-of-scope defects and deferred review findings, ingested into milestone metadata and preserved across receipt writes.'
    - 'INV-UNIFIED-CONTEXT-DEFAULT: Task claim provides the full context bundle by default (ADRs, symbols, tests, blackboard), eliminating procedural instruction overhead.'
    architectural_notes:
    - Scope conflicts are evaluated from all sibling task_scope_paths rows for the same milestone inside a SQLite IMMEDIATE transaction, independent of frozen status. The symmetric PathBuf prefix check rejects exact, ancestor, and descendant collisions; additions outside frozen roots are accepted only below src/ or tests/ and inserted as frozen=0. The public seam now records the complete frozen/dynamic path-shape matrix.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

Prevent scope collisions across sibling tasks in the milestone by checking exact matches, child directory enclosure, and parent directory enclosure across both frozen and unfrozen paths, returning `TASK_SCOPE_CONFLICT`. Permit governed extension to unowned paths under `src/` or `tests/` with `frozen = 0` subject to reviewer approval on the `paths` contour.

### task: typed-deferred-defects-and-milestone-scaffolding

```yaml
task_ref: typed-deferred-defects-and-milestone-scaffolding
target: Define typed DeferredDefect schema, parse deferred_defects in milestone specifications, and scaffold the section in milestone init
agent_type: worker
proof_policy: direct-proof
contract_revision: 1
scope:
- src/core/tasks/mod.rs
- src/engine/milestones.rs
- tests/core_basics/tasks.rs
status: completed
receipt:
  commit: eb262e4f7d8b54a4ced7b79bdf3b8d115a9db5fb
  contract_revision: 1
  passed_at: 2026-10-07T08:22:47.594610811+00:00
  evidence:
    test_proof:
      command: cargo test --test core_basics
      exit_code: 0
      tests_passed: 49
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Only tests/core_basics/tasks.rs was changed for this task, within the declared scope; no production files needed modification.
        claims:
          applicable: true
          evidence: The tests cover frontmatter and body YAML aggregation, scaffolded empty typed metadata, and preservation through the real TasksStore delivery receipt path.
        concurrency:
          applicable: false
          evidence: No concurrent implementation or synchronization behavior changed; the added assertions exercise a single task delivery transaction.
        project_isolation:
          applicable: true
          evidence: All parser, CLI, and receipt checks use isolated temporary workspaces and task stores.
        administration:
          applicable: true
          evidence: The generated milestone CLI document includes the documented section and parses back to the expected typed empty defect list.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ROOT-SNAPSHOT-DETERMINISM: Pre-delivery candidate code states are captured as deterministic root tree Git snapshots (without -p HEAD) under refs/forge/snapshots/, ensuring cryptographic determinism independent of sibling branch commits in shared worktrees.'
    - 'INV-SNAPSHOT-REF-PRESERVATION: Candidate snapshot refs remain pinned and immutable across review and deliver gates; verification compares working tree states without shifting refs prior to gate acceptance.'
    - 'INV-HIERARCHICAL-SCOPE-CONFLICT: Scope expansion via extend_scope enforces hierarchical conflict detection against all tasks in the milestone, rejecting exact matches, subdirectory enclosures, and ancestor collisions across both frozen and unfrozen paths with TASK_SCOPE_CONFLICT.'
    - 'INV-TYPED-DEFERRED-DEFECTS: Milestone contracts maintain a typed block under tasks for out-of-scope defects and deferred review findings, ingested into milestone metadata and preserved across receipt writes.'
    - 'INV-UNIFIED-CONTEXT-DEFAULT: Task claim provides the full context bundle by default (ADRs, symbols, tests, blackboard), eliminating procedural instruction overhead.'
    architectural_notes:
    - Deferred defects stay as typed milestone metadata under the existing frontmatter/body parser and are preserved when task receipts rewrite task YAML blocks. Production schema and scaffold implementation already met contract; this delivery adds regression coverage at parse, CLI init, and real TasksStore receipt seams.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

Introduce `DeferredDefect` with `id`, `source`, `title`, `path`, `disposition`, and `notes`. Parse `deferred_defects: [...]` blocks from both frontmatter and markdown body YAML blocks in `Milestone::parse_specification`. Update `milestone init` to scaffold `## Deferred and out-of-scope defects` by default.

### task: unified-context-bundle-default-and-guidance-sync

```yaml
task_ref: unified-context-bundle-default-and-guidance-sync
target: Enable unified context bundling by default on task claim and synchronize guidance and schemas across CLI, MCP, and documentation
agent_type: worker
proof_policy: direct-proof
contract_revision: 2
scope:
- src/engine/tasks/context.rs
- src/mcp/tools.rs
- docs/reference/tasks.md
- docs/reference/acdd.md
- docs/runbooks/acdd.md
- AGENTS.md
- tests/mcp_context/tasks.rs
status: completed
receipt:
  commit: d7048ddd601c202a2eaf580e34d040ad5f92d7b5
  contract_revision: 2
  passed_at: 2026-10-07T08:46:53.839714824+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets
      exit_code: 0
      tests_passed: 600
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: in_scope_test_only
        claims:
          applicable: true
          evidence: default_bundle_and_typed_metadata_verified_across_four_gates
        concurrency:
          applicable: false
          evidence: not_applicable_test_only
        project_isolation:
          applicable: true
          evidence: isolated_temp_workspaces_real_stdio_store
        administration:
          applicable: true
          evidence: all_deferred_defect_fields_retained_reference_parity_green
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ROOT-SNAPSHOT-DETERMINISM: Pre-delivery candidate code states are captured as deterministic root tree Git snapshots (without -p HEAD) under refs/forge/snapshots/, ensuring cryptographic determinism independent of sibling branch commits in shared worktrees.'
    - 'INV-SNAPSHOT-REF-PRESERVATION: Candidate snapshot refs remain pinned and immutable across review and deliver gates; verification compares working tree states without shifting refs prior to gate acceptance.'
    - 'INV-HIERARCHICAL-SCOPE-CONFLICT: Scope expansion via extend_scope enforces hierarchical conflict detection against all tasks in the milestone, rejecting exact matches, subdirectory enclosures, and ancestor collisions across both frozen and unfrozen paths with TASK_SCOPE_CONFLICT.'
    - 'INV-TYPED-DEFERRED-DEFECTS: Milestone contracts maintain a typed block under tasks for out-of-scope defects and deferred review findings, ingested into milestone metadata and preserved across receipt writes.'
    - 'INV-UNIFIED-CONTEXT-DEFAULT: Task claim provides the full context bundle by default (ADRs, symbols, tests, blackboard), eliminating procedural instruction overhead.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
subtasks:
- subtask_ref: milestone-lifecycle-e2e-retention
  title: 'In tests/mcp_context/tasks.rs::task_stdio_lifecycle_submits_inline_evidence_in_independent_worktrees, exercise omitted-option MCP task_claim plus contract/build/review/deliver submissions; assert context_bundle is present and Milestone::parse after deliver preserves every typed deferred_defects field. Acceptance: focused test passes, owner suites remain core_basics 49 and mcp_context 30, no new unique test is added.'
  status: completed
  evidence: cargo_test_mcp_context_tasks_lifecycle_1_passed_cargo_test_core_basics_and_mcp_context_79_passed_cargo_test_all_targets_600_passed_0_failed_3_ignored_no_new_unique_test
```

Default `bundle: true` on `task claim` in CLI and MCP tool router. Deliver zero-shot orientation (ADRs, symbols, tests, blackboard) upon claim. Clarify evidence commit omission and receipt snapshot preservation in workflow steps and documentation.

## Deferred and out-of-scope defects

```yaml
deferred_defects: []
```
