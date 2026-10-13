---
id: m-authority-gap-diagnostics-and-milestone-header-protection
title: Authority gap diagnostic ergonomics, milestone header protection, and footgun elimination
doc_type: contract
status: active
repository: forge-mcp
project: forge-mcp
depends_on:
- m-acdd-verification-invariants-and-handoff-hardening
owners:
- src/core/tasks/
- src/db/tasks_store/
- src/engine/tasks/
- src/mcp/
- tests/acdd/
invariants:
- 'INV-AUTHORITY-DIAGNOSTICS: Rejection of mismatched task specifications in sync and validate_authority must identify the task ID, file vs database contract revisions, and attribute divergence to milestone frontmatter fields and/or task YAML fields with distinct remediation actions.'
- 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}", "{CODE}: action ''{action}''. {facts}. Do: {action}" for action-scoped errors, or "{CODE}: milestone ''{milestone_ref}''. {facts}. Do: {action}" for milestone-scoped errors. Facts name divergent tree paths, wrong commits, unsatisfied dependencies, claim holders with timestamps, or revision deltas. Each imperative states one action.'
- 'INV-CLAIM-GUIDANCE-PROTECTION: Task claim workflow guidance embeds the constant PROTECTED rule and three standing operational constraints (worktree isolation, snapshot immutability, independent reviewer) directly in guidance without tool schema bloat.'
related_plans: []
started_at: 2026-10-13T18:23:32+00:00
---

# Authority gap diagnostic ergonomics, milestone header protection, and footgun elimination

## Outcome and purpose

Harden ContextUnity Forge MCP against opaque error codes and accidental footguns that cause autonomous agents to lose context, hallucinate non-existent operations, or loop destructively:

1. **Structured `AUTHORITY_GAP` and `TASK_AUTHORITY_MISSING` Attribution:**
   Digest calculation binds `repository`, `project`, `id`, `invariants`, `owners`, `depends_on`, optional `acdd_profile`, and the task body (excluding `receipt`, `status`, `subtasks`; `task.contract_revision` stays inside the digest and stays out of the attributed field list). Decompose digest divergence into two distinct sets of qualified fields: milestone frontmatter (`milestone.*`) vs task specification YAML (`task.*`). Format errors in both `task sync` and `validate_authority` (claim/submit) with exact task ID, revision comparisons (`file revision X <= stored Y` or `file revision X > stored Y (unsynced)`), and one remediation command. When `file revision > stored` and frontmatter fields diverged, the command reverts those fields and runs task sync, because the higher revision still changes the digest after the revert. Pre-detect milestone ID divergence in both `sync` and `validate_authority` before task comparison.

2. **Single-Line Fact/Action Error Diagnostics:**
   Standardize all task lifecycle failure modes across `tasks_store`, `engine/tasks`, and `mcp` into the deterministic format:
   `{CODE}: {task_id}. {facts}. Do: {imperative}.`
   Eliminate bare codes (`TASK_CANDIDATE_MISMATCH`, `TASK_NOT_READY`, `TASK_ALREADY_CLAIMED`, `TASK_STALE_SUBMISSION`, `TASK_REVIEW_NOT_INDEPENDENT`, `TASK_SELECTOR_INVALID`, `TASK_STALE_CLAIM`, `TASK_NOT_FOUND`, `TASK_EVIDENCE_INVALID`, `TASK_STAGE_INVALID`). Propagate rich error strings through MCP without generic suppression.

3. **In-Context Guidance Protection:**
   Inject the constant `PROTECTED` rule and three concise operational constraints directly into `workflow_guidance` on `task claim` so executing agents receive immediate runtime guardrails without relying on external documentation or schema bloat.

---

## Tasks in this milestone

### task: authority-gap-diagnostics

```yaml
task_ref: authority-gap-diagnostics
target: Decompose digest attribution between milestone frontmatter and task spec with qualified field names, report structured AUTHORITY_GAP in sync and validate_authority, and enrich TASK_AUTHORITY_MISSING
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/core/tasks/mod.rs
- src/db/tasks_store/mod.rs
- src/engine/tasks/mod.rs
- tests/acdd/tasks.rs
status: completed
receipt:
  contract_revision: 2
  passed_at: 2026-10-13T18:52:39.116314731+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 81
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Compared the full candidate snapshot 5bf6efd with the task worktree; the candidate changes only src/core/tasks/mod.rs, src/db/tasks_store/mod.rs, src/engine/tasks/mod.rs, and tests/acdd/tasks.rs, all admitted paths. The milestone authority document is the task's milestone reference.
        claims:
          applicable: true
          evidence: Authority mismatch diagnostics remain implemented in core and used by both sync and validate_authority. The added linked-project regression drives real TasksStore::sync against a shared SQLite database and confirms a foreign project at the same manifest path does not suppress rename detection. cargo test --test acdd passed 81 tests, 0 failed.
        concurrency:
          applicable: true
          evidence: sync_selected now begins an IMMEDIATE SQLite transaction before stored_task_id_for_milestone and keeps the check in the same transaction as admission; the cancelled path drops the transaction before prune_cancelled. Strict Clippy exited 0.
        project_isolation:
          applicable: true
          evidence: stored_task_id_for_milestone filters current descriptors by authority_snapshot.repository and project, and applies project/namespace task-id prefixes to legacy descriptors. The regression seeds project aaa before zzz at the same milestone_ref and verifies zzz still detects its own ID change.
        administration:
          applicable: true
          evidence: No profile, schema, or migration files changed. authority_snapshot remains optional/defaulted for prior task descriptors. Candidate snapshot 5bf6efd matches the current scoped source/test files; strict cargo clippy --all-targets --all-features -- -D warnings exited 0.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-AUTHORITY-DIAGNOSTICS: Rejection of mismatched task specifications in sync and validate_authority must identify the task ID, file vs database contract revisions, and attribute divergence to milestone frontmatter fields and/or task YAML fields with distinct remediation actions.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}", "{CODE}: action ''{action}''. {facts}. Do: {action}" for action-scoped errors, or "{CODE}: milestone ''{milestone_ref}''. {facts}. Do: {action}" for milestone-scoped errors. Facts name divergent tree paths, wrong commits, unsatisfied dependencies, claim holders with timestamps, or revision deltas. Each imperative states one action.'
    - 'INV-CLAIM-GUIDANCE-PROTECTION: Task claim workflow guidance embeds the constant PROTECTED rule and three standing operational constraints (worktree isolation, snapshot immutability, independent reviewer) directly in guidance without tool schema bloat.'
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

#### Contract and acceptance criteria

- Maintain byte-for-byte immutability for existing `Milestone::digest(&self, task: &TaskSpec) -> Result<String>` serialization format and output. Implement separate diagnostic attribution helpers in `src/core/tasks/mod.rs` to compute qualified frontmatter fields (`milestone.invariants`, `milestone.owners`, `milestone.depends_on`, `milestone.acdd_profile`, `milestone.id`) and qualified task spec fields (`task.target`, `task.proof_policy`, `task.scope`, `task.scope_roots`, `task.invariants`, `task.depends_on`, `task.acdd_profile`, and the remaining `TaskSpec` content fields). Omit `task.contract_revision` from `{spec_fields}`: it stays inside `Milestone::digest`, and the revision sentence already states it.
- Milestone ID change detection:
  - In `src/db/tasks_store/mod.rs` (`sync`), before the task-by-task loop, check if any task is already stored for `milestone_ref`. If stored tasks exist, parse `stored_id` from their `task_id` (`{project}/{stored_id}:{task_ref}`). If `milestone.id != stored_id`:
    `AUTHORITY_GAP: milestone '{milestone_ref}'. milestone id changed from '{stored_id}' to '{file_id}'. Do: revert milestone id in {milestone_ref}. Task ids are {project}/{id}:{task_ref}; sync would admit them as new tasks.`
  - In `src/engine/tasks/mod.rs` (`validate_authority` on claim/submit), extract `stored_id` from `task.task_id` before task lookup. If `milestone.id != stored_id`:
    `AUTHORITY_GAP: milestone '{milestone_ref}'. milestone id changed from '{stored_id}' to '{file_id}'. Do: revert milestone id in {milestone_ref}. Task ids are {project}/{id}:{task_ref}; task cannot be resolved.`
- In `src/db/tasks_store/mod.rs` (`sync`), when `spec.contract_revision <= existing.contract_revision` and digest diverged:
  - Frontmatter fields mutated only:
    - If `milestone.invariants` is mutated:
      `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; digest fields [{frontmatter_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}. Declare task invariants inside task.invariants.`
    - If `milestone.invariants` is not mutated (only `milestone.owners`, `milestone.depends_on`, `milestone.acdd_profile`):
      `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; digest fields [{frontmatter_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}.`
  - Task YAML fields mutated only:
    `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; digest fields [{spec_fields}]. Do: set contract_revision above {stored_rev} and run contextunity-forge-mcp task sync.`
  - Both frontmatter and task YAML fields mutated:
    - If `milestone.invariants` is mutated:
      `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; frontmatter fields [{frontmatter_fields}], task fields [{spec_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}, declare task invariants inside task.invariants, set contract_revision above {stored_rev}, and run contextunity-forge-mcp task sync.`
    - If `milestone.invariants` is not mutated:
      `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; frontmatter fields [{frontmatter_fields}], task fields [{spec_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}, set contract_revision above {stored_rev}, and run contextunity-forge-mcp task sync.`
- In `src/engine/tasks/mod.rs` (`validate_authority` on claim/submit):
  - When `milestone.digest(spec)? != task.digest`:
    - If `spec.contract_revision <= task.contract_revision`:
      - Frontmatter mutated only (with `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; digest fields [{frontmatter_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}. Declare task invariants inside task.invariants.`
      - Frontmatter mutated only (without `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; digest fields [{frontmatter_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}.`
      - Task YAML mutated only:
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; digest fields [{spec_fields}]. Do: set contract_revision above {stored_rev} and run contextunity-forge-mcp task sync.`
      - Both mutated (with `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; frontmatter fields [{frontmatter_fields}], task fields [{spec_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}, declare task invariants inside task.invariants, set contract_revision above {stored_rev}, and run contextunity-forge-mcp task sync.`
      - Both mutated (without `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} <= stored {stored_rev}; frontmatter fields [{frontmatter_fields}], task fields [{spec_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}, set contract_revision above {stored_rev}, and run contextunity-forge-mcp task sync.`
    - If `spec.contract_revision > task.contract_revision` (file revised, sync pending):
      - Frontmatter mutated only (with `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} > stored {stored_rev} (unsynced); digest fields [{frontmatter_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}, declare task invariants inside task.invariants, and run contextunity-forge-mcp task sync.`
      - Frontmatter mutated only (without `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} > stored {stored_rev} (unsynced); digest fields [{frontmatter_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref} and run contextunity-forge-mcp task sync.`
      - Task YAML mutated only:
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} > stored {stored_rev} (unsynced); digest fields [{spec_fields}]. Do: run contextunity-forge-mcp task sync.`
      - Both mutated (with `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} > stored {stored_rev} (unsynced); frontmatter fields [{frontmatter_fields}], task fields [{spec_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref}, declare task invariants inside task.invariants, and run contextunity-forge-mcp task sync.`
      - Both mutated (without `milestone.invariants`):
        `AUTHORITY_GAP: task '{id}'. file revision {file_rev} > stored {stored_rev} (unsynced); frontmatter fields [{frontmatter_fields}], task fields [{spec_fields}]. Do: revert [{frontmatter_fields}] in {milestone_ref} and run contextunity-forge-mcp task sync.`
  - Enrich `TASK_AUTHORITY_MISSING`:
    - When milestone file is missing: `TASK_AUTHORITY_MISSING: milestone file '{path}' is missing. Do: restore milestone file '{path}'.`
    - When task is missing from milestone: `TASK_AUTHORITY_MISSING: task '{id}' not found in milestone document '{path}'. Do: restore task '{id}' in '{path}'.`
- Add/update integration tests in `tests/acdd/tasks.rs`:
  1. Specification change with equal or lower contract revision producing structured diagnostic with task ID, revision numbers, and qualified field attribution.
  2. Frontmatter invariant mutation vs owner mutation triggering milestone-level header attribution with tailored remediation (with or without invariant declaration advice).
  3. Simultaneous frontmatter and task spec mutation reporting both field sets and combined remediation.
  4. Milestone ID change detected prior to task ingestion in both sync and validate_authority.
  5. Missing milestone file vs missing task ID producing distinct `TASK_AUTHORITY_MISSING` diagnostics on claim/submit.
  6. An unsynced higher `contract_revision` with a frontmatter-only change reports qualified frontmatter fields, omits `task.contract_revision`, and tells the caller to revert those fields and run task sync.

---

### task: diagnostic-ergonomics

```yaml
task_ref: diagnostic-ergonomics
target: Replace bare uninformative task lifecycle errors with single-line fact/action diagnostics and propagate them through MCP response
proof_policy: seam-test-first
contract_revision: 2
depends_on:
- authority-gap-diagnostics
scope:
- src/db/tasks_store/mod.rs
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/lifecycle.rs
- src/engine/tasks/gates.rs
- src/engine/tasks/mod.rs
- src/mcp/response.rs
- tests/acdd/tasks.rs
- tests/acdd/mcp.rs
status: completed
receipt:
  contract_revision: 2
  passed_at: 2026-10-13T19:45:08.307487333+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 84
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Compared the full candidate snapshot 5919993 against the worktree: the only changed files are the eight admitted paths; no staged or untracked files are present, git diff --check is clean, and all admitted source/test files match snapshot 5919993. The only delta from prior candidate de75d6c is the raw Git runner replacement in src/engine/tasks/mod.rs.'
        claims:
          applicable: true
          evidence: 'Traced each enumerated diagnostic through TasksStore, engine claim/submit/manage flows, and MCP response propagation. The exact-message tests use real stores and engine/MCP paths, including candidate mismatch, dependency/status precedence, stale claim/submission, selector validation, and MCP claim-holder text. Fresh cargo test --test acdd exited 0: 84 passed, 0 failed. The prior runner finding is fixed: candidate path inspection uses crate::core::git::git_command at src/engine/tasks/mod.rs:551 and :564.'
        concurrency:
          applicable: true
          evidence: Claim and submission mutations remain fenced by IMMEDIATE SQLite transactions (src/db/tasks_store/lifecycle.rs:48-50; src/engine/tasks/gates.rs:203-205). Submission compares active stage/profile, claim revision, and the ordered evidence identity fields before persistence; the existing delivery flow retains candidate-baseline and rollback checks. Strict cargo clippy --all-targets --all-features -- -D warnings exited 0.
        project_isolation:
          applicable: true
          evidence: Candidate inspection uses the validated claimed worktree as cwd through core::git::git_command and passes task scope only after '--' (src/engine/tasks/mod.rs:548-551,561-564). Task and dependency queries are keyed by full task_id; existing linked-workspace and nested-repository ACDD cases passed in the fresh 84-test run.
        administration:
          applicable: true
          evidence: No schema, migration, profile, repository configuration, credentials, or generated files changed. Existing profile pins, review independence, delivery checks, and receipt handling remain intact; the auto_commit=false candidate-commit regression passed. Strict Clippy exited 0.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-AUTHORITY-DIAGNOSTICS: Rejection of mismatched task specifications in sync and validate_authority must identify the task ID, file vs database contract revisions, and attribute divergence to milestone frontmatter fields and/or task YAML fields with distinct remediation actions.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}", "{CODE}: action ''{action}''. {facts}. Do: {action}" for action-scoped errors, or "{CODE}: milestone ''{milestone_ref}''. {facts}. Do: {action}" for milestone-scoped errors. Facts name divergent tree paths, wrong commits, unsatisfied dependencies, claim holders with timestamps, or revision deltas. Each imperative states one action.'
    - 'INV-CLAIM-GUIDANCE-PROTECTION: Task claim workflow guidance embeds the constant PROTECTED rule and three standing operational constraints (worktree isolation, snapshot immutability, independent reviewer) directly in guidance without tool schema bloat.'
    architectural_notes:
    - 'deferred: Milestone 018 changes observable task lifecycle diagnostics, but the task scopes do not include docs/reference/tasks.md. An attempted Forge extend_scope for that path was rejected with TASK_SCOPE_INVALID, so no documentation file was changed. Defer the reference documentation update to a follow-up task after milestone delivery.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Contract and acceptance criteria

- Standardize error format across all failure modes. Task errors use `{CODE}: {task_id}. {facts}. Do: {imperative}.` Action selectors use `{CODE}: action '{action}'. {facts}. Do: {imperative}.` Milestone id changes use `{CODE}: milestone '{milestone_ref}'. {facts}. Do: {imperative}.` Each imperative states one action.
- `TASK_CANDIDATE_MISMATCH`:
  - Worktree tree diverged (`engine/tasks/mod.rs:609, 634`):
    `TASK_CANDIDATE_MISMATCH: task '{id}' diverged from snapshot {short}. {paths_with_status}. Do: restore those paths to snapshot {short}.`
  - Scoped snapshot commit mismatch (`engine/tasks/mod.rs:569`):
    `TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != scoped snapshot {snapshot}. Do: resubmit with commit {snapshot}.`
  - Completed delivery commit differs from receipt (`engine/tasks/mod.rs:591`):
    `TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != accepted receipt commit {delivered}. Do: resubmit with commit {delivered}.`
  - Review evidence commit != candidate snapshot (`gates.rs:283`):
    `TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != candidate snapshot {snapshot}. Do: resubmit with commit {snapshot}.`
  - Delivery commit != candidate snapshot (`gates.rs:304`):
    `TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != candidate snapshot {snapshot}. Do: resubmit with commit {snapshot}.`
  - Explicit commit != candidate snapshot (`engine/tasks/mod.rs:618`):
    `TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != candidate snapshot {snapshot}. Do: resubmit with commit {snapshot}.`
  - Review source mismatch (`gates.rs:310`):
    `TASK_CANDIDATE_MISMATCH: task '{id}'. review source '{source}' commit != candidate snapshot {snapshot}. Do: resubmit review evidence with commit {snapshot}.`
- `TASK_NOT_READY` (`tasks_store/lifecycle.rs:57`). Evaluate status first. Emit the dependency message only when status is `ready`:
  - Status != ready:
    `TASK_NOT_READY: task '{id}'. status is '{status}' (active stage '{stage}'). Do: claim only ready tasks at their active stage.`
  - Unsatisfied dependencies:
    `TASK_NOT_READY: task '{id}' has unsatisfied dependencies: [{dep_ids}]. Do: complete dependencies before claiming stage '{stage}'.`
- `TASK_ALREADY_CLAIMED` (`tasks_store/mod.rs:198`, `tasks_store/lifecycle.rs:54`):
  - Include worker, worktree, revision, and claimed_at in `TaskAlreadyClaimed` struct fields:
    `TASK_ALREADY_CLAIMED: task '{id}' is claimed by worker '{worker}' in worktree '{worktree}' (revision {rev}, claimed at {claimed_at}). Do: submit the active claim from worker '{worker}' before opening another.`
  - In `src/mcp/response.rs:74-76`, propagate the rich `TaskAlreadyClaimed` display text verbatim into the MCP error response instead of generic replacement.
- `TASK_STALE_SUBMISSION`:
  - Stage or profile_pin changed concurrently (`engine/tasks/mod.rs:529`, `gates.rs:208`):
    `TASK_STALE_SUBMISSION: task '{id}'. stage or profile_pin changed concurrently. Do: retry submission.`
  - Claim revision already submitted with different evidence (`gates.rs:229`):
    `TASK_STALE_SUBMISSION: task '{id}'. claim_revision {rev} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {rev}.`
  - Status is not `in_progress` (`gates.rs:255`, checked before the other predicates in that condition):
    `TASK_STALE_SUBMISSION: task '{id}'. status is '{status}'. Do: claim the task before submitting.`
  - Evidence fields mismatch on `claim_revision`, `contract_revision`, `task_id`, `worker_id`, or `worktree` (`gates.rs:255`). Report the first failing field:
    `TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {evidence_val} != active {active_val}. Do: resubmit with active {field} {active_val}.`
- `TASK_STAGE_INVALID`:
  - Evidence stage != submitted stage (`gates.rs:196`):
    `TASK_STAGE_INVALID: task '{id}'. evidence.stage '{evidence_stage}' != submitted stage '{stage}'. Do: set evidence.stage to '{stage}'.`
  - Submitted stage != active stage (`tasks_store/mod.rs:1148`):
    `TASK_STAGE_INVALID: task '{id}'. submitted stage '{stage}' does not match active stage '{active_stage}'. Do: submit for active stage '{active_stage}'.`
  - Reject target missing from profile (`gates.rs:459`):
    `TASK_STAGE_INVALID: task '{id}'. reject target stage '{target}' not defined in profile. Do: set reject_to to a stage that exists in the profile.`
- `TASK_EVIDENCE_INVALID`:
  - Evidence not JSON object (`engine/tasks/mod.rs:532`):
    `TASK_EVIDENCE_INVALID: task '{id}'. evidence is not a valid JSON object. Do: pass evidence as a JSON object.`
  - Evidence object fails to parse (`engine/tasks/mod.rs:535`):
    `TASK_EVIDENCE_INVALID: task '{id}'. evidence JSON does not match the evidence object. Do: pass the evidence fields required by stage '{stage}'.`
  - Null proof (`gates.rs:257`):
    `TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. proof is null. Do: provide non-empty proof for stage '{stage}'.`
  - Empty commit (`gates.rs:261`):
    `TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. commit field is empty. Do: set commit to candidate snapshot {snapshot}.`
  - Delivery without commit (`gates.rs:296`):
    `TASK_EVIDENCE_INVALID: task '{id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {snapshot}.`
- `TASK_REVIEW_NOT_INDEPENDENT` (`gates.rs:279`):
  - `TASK_REVIEW_NOT_INDEPENDENT: task '{id}'. reviewer worker '{reviewer}' matches builder '{builder}'. Do: submit review with a worker_id other than '{builder}'.`
- `TASK_SELECTOR_INVALID` (`engine/tasks/lifecycle.rs:168`):
  - Error occurs before task ID resolution:
    `TASK_SELECTOR_INVALID: action '{action}'. missing or invalid fields: [{fields}]. Do: pass required fields for '{action}'.`
- `TASK_STALE_CLAIM` (`tasks_store/lifecycle.rs`):
  - Status is not `in_progress` (`lifecycle.rs:12`):
    `TASK_STALE_CLAIM: task '{id}'. status is '{status}'. Do: leave the claim unchanged.`
  - Requested revision differs from the active revision (`lifecycle.rs:12`):
    `TASK_STALE_CLAIM: task '{id}'. active claim_revision {active_rev} != requested {rev}. Do: release claim_revision {active_rev}.`
  - Open claim row is missing (`lifecycle.rs:19`):
    `TASK_STALE_CLAIM: task '{id}'. claim_revision {rev} has no open claim row. Do: claim the task again before modifying it.`
- `TASK_NOT_FOUND` (`tasks_store/mod.rs:469`, `engine/tasks/lifecycle.rs:212`, and the other `TASK_NOT_FOUND` sites in this task's scope):
  - `TASK_NOT_FOUND: task '{id}' not found in tasks database. Do: verify task id and sync milestone.`
- Update all existing exact-match assertions in `tests/acdd/tasks.rs` and `tests/acdd/mcp.rs`.

---

### task: claim-guidance

```yaml
task_ref: claim-guidance
target: Inject constant PROTECTED constraint and concise three-rule operational guidance into task claim workflow guidance without tool schema bloat
proof_policy: seam-test-first
contract_revision: 2
depends_on:
- diagnostic-ergonomics
scope:
- src/engine/tasks/workspaces.rs
- tests/acdd/milestones.rs
- tests/acdd/mcp.rs
status: completed
receipt:
  contract_revision: 2
  passed_at: 2026-10-13T19:59:15.084798016+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 85
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate 2834e1c changes only src/engine/tasks/workspaces.rs, tests/acdd/milestones.rs, and tests/acdd/mcp.rs, all admitted paths. No staged or untracked files exist; git diff --check is clean, and current scoped files match the snapshot.
        claims:
          applicable: true
          evidence: 'src/engine/tasks/workspaces.rs:541-558 emits the exact PROTECTED constant and the ISOLATION, IMMUTABILITY, and INDEPENDENCE rules in workflow_guidance. The CLI seam test asserts all four in the actual task claim response; the MCP seam test asserts them for both default/full and bundle=false claim responses. Fresh cargo test --test acdd exited 0: 85 passed, 0 failed.'
        concurrency:
          applicable: true
          evidence: The change only constructs claim-response guidance; it adds no task state, database writes, or transaction changes. Existing concurrent claim/subtask coverage ran in the fresh ACDD suite; the claim guidance test also proves both claim bundle modes return the same operational rules.
        project_isolation:
          applicable: true
          evidence: ISOLATION text is populated from the worktree already attached to the claim response (workspaces.rs:543-546); CLI and MCP tests verify the exact active workspace path appears. No workspace registry, path validation, task identity, or profile resolution code changed.
        administration:
          applicable: true
          evidence: No profile, schema, configuration, migration, or generated-state files changed. The MCP tools/list test verifies the catalog is <=65,536 bytes and does not duplicate the protected or operational rule text in schema metadata (tests/acdd/mcp.rs:525-536). Strict cargo clippy --all-targets --all-features -- -D warnings exited 0.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-AUTHORITY-DIAGNOSTICS: Rejection of mismatched task specifications in sync and validate_authority must identify the task ID, file vs database contract revisions, and attribute divergence to milestone frontmatter fields and/or task YAML fields with distinct remediation actions.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}", "{CODE}: action ''{action}''. {facts}. Do: {action}" for action-scoped errors, or "{CODE}: milestone ''{milestone_ref}''. {facts}. Do: {action}" for milestone-scoped errors. Facts name divergent tree paths, wrong commits, unsatisfied dependencies, claim holders with timestamps, or revision deltas. Each imperative states one action.'
    - 'INV-CLAIM-GUIDANCE-PROTECTION: Task claim workflow guidance embeds the constant PROTECTED rule and three standing operational constraints (worktree isolation, snapshot immutability, independent reviewer) directly in guidance without tool schema bloat.'
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

#### Contract and acceptance criteria

- In `src/engine/tasks/workspaces.rs` (`workflow_guidance`), inject exact constant string:
  `"PROTECTED: milestone id, invariants, owners, depends_on, and acdd_profile are immutable while tasks are active. Declare task invariants inside task.invariants."`
- Inject three concise operational rules into `workflow_guidance["rules"]` on claim:
  1. `ISOLATION: Develop exclusively inside dedicated worktree '{worktree}'.`
  2. `IMMUTABILITY: Keep admitted scope unchanged after review pass and during deliver.`
  3. `INDEPENDENCE: Review stage requires a worker_id distinct from the accepted builder.`
- Preserve MCP response budgets: keep tool schema metadata concise without duplicating documentation into schema descriptions.
- Add integration tests in `tests/acdd/milestones.rs` and `tests/acdd/mcp.rs` asserting presence of `PROTECTED` text and three-rule guidance in claim responses.
