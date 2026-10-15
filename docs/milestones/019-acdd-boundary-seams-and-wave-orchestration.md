---
id: m-acdd-boundary-seams-and-wave-orchestration
title: ACDD receipt canonicalization, tiered role profile (contract/build/review+deliver), blackboard handoff, seam guards, and wave dispatch
doc_type: contract
status: active
repository: forge-mcp
project: forge-mcp
depends_on:
- m-authority-gap-diagnostics-and-milestone-header-protection
owners:
- .forge/acdd/profile.yaml
- AGENTS.md
- TESTS.md
- src/core/tasks/
- src/db/tasks_store/
- src/engine/tasks/
- src/engine/milestones/
- src/mcp/
- src/cli/
- skills/
- docs/reference/acdd.md
- docs/reference/profiles.md
- docs/reference/tasks.md
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- tests/acdd/
invariants:
- 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
- 'INV-CANONICAL-RECEIPT-SHA: API responses preserve full 40-character Git SHAs in receipt.commit and candidate_snapshot.commit; gates normalize 7-to-40 char SHA prefixes so source and accepted receipts never diverge.'
- 'INV-BLACKBOARD-INSPECT-TOLERANCE: task_blackboard(action=inspect) tolerates optional scope/task_id/milestone_ref fields when message_id is supplied and falls back to scoped read when message_id is omitted.'
- 'INV-TIERED-SUBAGENT-PROFILE: Default ACDD profile defines three tiered subagent roles—contract_author (sol-6.1:high for contract seam design), builder (gpt6-luna:xhigh for build code implementation connected via gate-targeted task_blackboard notes), and independent_reviewer (sol-6.1:xhigh governing review and deliver gates in a single reviewer subagent session)—while forbidding preflight subagents.'
- 'INV-PROFILE-CANONICAL-COMMANDS: Build verification strictly uses commands registered in GateProfile.commands (combining fast static checks like clippy/ruff/pyright with test execution so suffix test paths work via existing prefix validation), and context_bundle synthesizes workflow_guidance.scoped_commands from profile.commands + contract_seam_test/covering_tests.'
- 'INV-SEAM-REACHABILITY: A contract submission with seam_test_ref must reference an indexed test symbol or file with a 1-to-2 hop graph edge into allowed_scope when .forge/code-map.sqlite is present.'
- 'INV-SIGNATURE-BLAST-RADIUS: Modifying the public signature of an indexed symbol inside allowed_scope whose 1-hop external callers lie outside allowed_scope fails closed until extend_scope admits those caller paths.'
- 'INV-WAVE-DISJOINTNESS: Task sync automatically records top-down task_dependencies edges (i < j) for tasks in the same milestone with overlapping scope paths, preventing mutual TASK_SCOPE_CONFLICT deadlocks among ready tasks.'
- 'INV-WORKTREE-AFFINITY: A task deferred during extend_scope due to an active claim conflict retains its builder worker and worktree binding and is resumed by that worker once the blocking task completes.'
- 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
related_plans: []
started_at: 2026-10-14T15:41:30+00:00
---

# ACDD receipt canonicalization, tiered role profile (contract/build/review+deliver), blackboard handoff, seam guards, and wave dispatch

## Outcome and purpose

### Reopened after boundary audit (2026-10-15)

The user reopened this milestone after auditing landed revision `90fab6b`.
The previous handoff (`e10c06d`, 692 passing tests) missed two reproducible
violations of task 2's existing invariants:

- M019-A1, pre-existing-missed: public Rust trait methods were omitted from
  signature boundary checks because their signatures have no explicit `pub`.
- M019-A2, pre-existing-missed: stale indexed test edges could admit a contract
  after the current test stopped reaching production code.

Both findings remain unresolved until production-seam regressions, repair,
independent review, and a new milestone handoff pass. Task 2 is reopened;
the other two task receipts remain historical delivery evidence. The installed
CLI/MCP runtime must also be checked against the repaired source before closure.

Based on production rollout telemetry (`rollout-2026-10-14T08-02-19`) and direct code inspection of `src/core/tasks/mod.rs`, `src/core/tasks/profile.rs`, `src/core/tasks/gates.rs`, `src/engine/tasks/gates.rs`, `src/engine/tasks/blackboard.rs`, `src/engine/tasks/context.rs`, and `src/engine/tasks/workspaces.rs`, this milestone resolves receipt synchronization loops, profile command/linter bottlenecks, and subagent orchestration overhead using Forge's existing canonical mechanisms:

1. **Fix `receipt.commit` 40-char vs 7-char Mismatch, Manual YAML Edits & Blackboard `inspect` Errors:**
   - **Root Cause (`src/core/tasks/mod.rs` lines 32–49)**: `shorten_task_response_commits` recursively rewrites every `"commit"` field in JSON responses (including `receipt.commit` and `candidate_snapshot.commit`) to 7 chars (`"e8b893f"`), while the milestone file on disk stores the full 40-char SHA (`"e8b893f104a7a269aa268bc0c1d3ba86720c4fc0"`). Agents see `source != accepted` and edit the markdown file by hand, breaking YAML indentation and triggering `AUTHORITY_GAP`.
   - **Root Cause (`src/engine/tasks/blackboard.rs` lines 360–374)**: `validate_blackboard_request` rejects `BlackboardAction::Inspect` with `TASK_BLACKBOARD_INVALID: inspect accepts only message_id` whenever an agent passes `task_id` or `scope` alongside `message_id`.
   - **Solution**: Keep full 40-char SHAs in `receipt.commit`, `candidate_snapshot.commit`, and `latest_snapshot.commit` (shortening only human display strings like `inspect_cmd`), normalize 7-to-40 char commit prefixes in `gates.rs`, add `NEVER_EDIT_RECEIPTS` to `workflow_guidance.rules`, and make `blackboard(action="inspect")` ignore redundant context fields or fall back to `read` when `message_id` is omitted.

2. **Configure Tiered ACDD Roles (`contract_author`, `builder`, `independent_reviewer`), Blackboard `contract -> build` Handoff & Canonical `commands`:**
   - **Model Tiering by Cognitive Demand (`src/core/tasks/acdd.default.yaml` & `.forge/acdd/profile.yaml`)**:
     - Model tiering preserves architectural contract quality without overspending on mechanical implementation:
       - **`contract_author` (`sol-6.1:high`, `mode: subagent`)**: Explores the seam, authors the sensitive failing seam test, posts distilled implementation context to `task_blackboard` (`topic: "draft"`, `gate: "build"`), and submits `contract_proof`.
       - **`builder` (`gpt6-luna:xhigh`, `mode: subagent`)**: Starts with a clean context window, receives the `gate: "build"` blackboard note + `contract_seam_test` + `invariants_summary` in `context_bundle`, implements the fix inside `allowed_write_scope`, runs the registered profile command (static checks + scoped test), and submits `command_proof`.
       - **`independent_reviewer` (`sol-6.1:xhigh`, `mode: subagent`, `worker_id != builder`)**: Combines review and delivery into one role/model. Evaluates the 5 review contours, submits `review_proof`, and upon `pass` immediately claims and submits `deliver` in the same session without spawning a redundant delivery worker.
       - **Coordinator**: Never spawns throwaway 1–2 minute "preflight" subagents just to run `git status` or read 1 file (`NO_PREFLIGHT_SUBAGENTS`).
   - **Canonical `commands` Without Schema Changes (`src/core/tasks/gates.rs` lines 119–128)**:
     - `validate_command` already accepts any command that starts with a registered `commands.<name>.command` prefix followed by whitespace (`proof.command.strip_prefix(command)`).
     - By registering `command: "cargo clippy --all-targets --all-features -- -D warnings && cargo test"` (or `"ruff check && pyright && pytest -q --tb=line -o strict_markers=false"` in Python profiles), static checks run automatically before every `build` closure while agents still append the specific test target (`... && cargo test --test acdd` or `... && pytest -q --tb=line -o strict_markers=false tests/unit/test_seam.py`) at the end.
     - `context_bundle` (`src/engine/tasks/context.rs`) synthesizes ready-to-copy `workflow_guidance.scoped_commands` by appending `contract_seam_test` / `covering_tests` to the active profile's registered `commands`.

3. **Enforce 1-Hop Boundary Seam/Signature Guards & Conflict-Free Wave Dispatch:**
   - Add `external_callers` and 2-hop `covering_tests` to `context_bundle`, enforce `TASK_SEAM_DISCONNECTED` and `TASK_SCOPE_INCOMPLETE` in `src/engine/tasks/gates.rs`, auto-infer top-down `task_dependencies` ($i < j$) and `wave_index` during `TasksStore::sync`, and add `ManageAction::Next` (`task next`) with worktree-preserving `extend_scope` parking.

---

## Tasks in this milestone

### task: receipt-sha-canonicalization-and-blackboard-ergonomics

```yaml
task_ref: receipt-sha-canonicalization-and-blackboard-ergonomics
target: Preserve canonical 40-char commit SHAs in receipts and snapshots, accept short SHA prefixes in gate submissions, forbid manual receipt YAML edits, and make task_blackboard inspect tolerant of scope parameters
proof_policy: seam-test-first
contract_revision: 3
scope:
- src/core/tasks/mod.rs
- src/engine/tasks/gates.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/blackboard.rs
- src/engine/tasks/workspaces.rs
- tests/acdd/milestones.rs
- docs/reference/tasks.md
- docs/reference/mcp-tools.md
- tests/acdd/tasks.rs
- tests/acdd/blackboard.rs
- tests/acdd/mcp.rs
depends_on: []
invariants:
- 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
- 'INV-CANONICAL-RECEIPT-SHA: API responses preserve full 40-character Git SHAs in receipt.commit and candidate_snapshot.commit; gates normalize 7-to-40 char SHA prefixes so source and accepted receipts never diverge.'
- 'INV-BLACKBOARD-INSPECT-TOLERANCE: task_blackboard(action=inspect) tolerates optional scope/task_id/milestone_ref fields when message_id is supplied and falls back to scoped read when message_id is omitted.'
- 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
status: completed
subtasks:
- subtask_ref: preserve-full-sha-in-receipts-and-snapshots
  title: Preserve full receipt and snapshot SHAs in src/core/tasks/mod.rs and tests/acdd/milestones.rs, and normalize short SHA prefixes in src/engine/tasks/mod.rs and src/engine/tasks/gates.rs before gate comparison
  status: completed
  evidence: 'cargo test --test acdd deferred_final_task_context_survives_delivery_and_prunes_blackboard -- --nocapture: exit 0, 1 passed, 0 failed; cargo test --test acdd: exit 0, 85 passed, 0 failed; cargo clippy --all-targets --all-features -- -D warnings: exit 0. Full SHA receipt/snapshot/gate assertions updated.'
- subtask_ref: relax-blackboard-inspect-parameter-validation
  title: Update validate_blackboard_request in src/engine/tasks/blackboard.rs so action=inspect ignores redundant scope/task_id/milestone_ref parameters when message_id is present and routes to scoped read when message_id is absent
  status: completed
  evidence: 'Before fix: cargo test --test acdd linked_task_mcp_and_cli_resolve_repository_roots_and_guidance -- --nocapture exited 101, failing only isolated subtask_ref and combined context-selector cases through the production MCP path. After fix: focused seam passed 1/0; cargo test --test acdd passed 85/0; cargo clippy --all-targets --all-features -- -D warnings exited 0; fmt and diff checks passed. Inspect with message_id ignores scope, task_id, milestone_ref, and subtask_ref.'
- subtask_ref: inject-never-edit-receipts-guardrail
  title: 'Add NEVER_EDIT_RECEIPTS rule to workflow_guidance.rules in src/engine/tasks/workspaces.rs instructing agents never to hand-edit receipt: or status: YAML in docs/milestones/*.md'
  status: completed
  evidence: 'cargo test --test acdd: exit 0, 85 passed, 0 failed. MCP workflow-guidance test asserts the NEVER_EDIT_RECEIPTS rule.'
receipt:
  commit: 05bdbe7f97d63f01309bb7c2251575645749b660
  contract_revision: 3
  passed_at: 2026-10-14T16:18:51.135568048+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 85
      tests_failed: 0
      log: 'Review finding M019-T1-R1 regression: before fix, `cargo test --test acdd linked_task_mcp_and_cli_resolve_repository_roots_and_guidance -- --nocapture` exited 101, failing isolated and combined subtask_ref cases. After fix, same focused MCP seam passed 1/0. Full `cargo test --test acdd`: exit 0, 85 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings`: exit 0. `cargo fmt --all -- --check` and `git diff --check`: exit 0.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed full HEAD diff and dc8ab6a..e35f0b2 remediation delta. All 11 admitted candidate paths match the worktree byte-for-byte; git status has only admitted implementation/docs/tests plus coordinated milestone authority activation/amendment. git diff --check exits 0. No profiles/credentials/untracked changes.
        claims:
          applicable: true
          evidence: 'Canonical SHA response helper preserves all machine commit/evidence values; display inspect_cmd remains abbreviated. canonical_commit_from_prefix enforces 7..=40 ASCII hex prefixes against expected full SHA; public submit and transactional store normalize before comparison/persistence. Existing MCP lifecycle proves 7-char review submission and full snapshot/receipt/stored evidence; helper cases cover 7/12/39/40. Blackboard inspect ignores scope/task_id/milestone_ref/subtask_ref with message_id and otherwise uses resolved scoped Read and schema filters. Exact NEVER_EDIT_RECEIPTS rule is injected. M019-T1-R1 closed by removal of subtask_ref reject predicate, standalone/combined MCP cases, and updated docs. Independent cargo test --test acdd linked_task_mcp_and_cli_resolve_repository_roots_and_guidance -- --nocapture: exit 0, 1 passed, 0 failed; accepted builder verification records full affected suite 85/0 and strict Clippy exit 0.'
        concurrency:
          applicable: true
          evidence: Reviewed engine submit and TasksStore submit_with_pre_persist. Normalization occurs within existing revision/worker/worktree fencing and before accepted-evidence persistence. Prefix matching binds only the selected candidate, not arbitrary Git references. Baseline checks, candidate validation, gate ordering, independent reviewer enforcement and atomic receipt rollback remain intact. Blackboard tolerance modifies validation/read routing without changing transactions or lock order.
        project_isolation:
          applicable: true
          evidence: Inspect-by-message retains configured Registry workspace lookup; fallback reuses resolve_blackboard_context, owner/project checks and active profile schema. Existing linked MCP/CLI production lifecycle and traversal rejection pass. No root/symlink confinement, database namespace, task identifier or linked-profile resolution changes.
        administration:
          applicable: true
          evidence: No migrations/profile pins/protected configuration changed. Core helper imports no engine/db/mcp/cli. Candidate remains e35f0b2f3bce779420f32658a75d1e7793af7b92 on captured baseline cac453e3b62cccb5796a290a9a9ad2bd92563518. Auto delivery commit/hook path and canonical durable receipt writer remain existing authority. Milestone receipt/status were not manually edited during review.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-CANONICAL-RECEIPT-SHA: API responses preserve full 40-character Git SHAs in receipt.commit and candidate_snapshot.commit; gates normalize 7-to-40 char SHA prefixes so source and accepted receipts never diverge.'
    - 'INV-BLACKBOARD-INSPECT-TOLERANCE: task_blackboard(action=inspect) tolerates optional scope/task_id/milestone_ref fields when message_id is supplied and falls back to scoped read when message_id is omitted.'
    - 'INV-TIERED-SUBAGENT-PROFILE: Default ACDD profile defines three tiered subagent roles—contract_author (sol-6.1:high for contract seam design), builder (gpt6-luna:xhigh for build code implementation connected via gate-targeted task_blackboard notes), and independent_reviewer (sol-6.1:xhigh governing review and deliver gates in a single reviewer subagent session)—while forbidding preflight subagents.'
    - 'INV-PROFILE-CANONICAL-COMMANDS: Build verification strictly uses commands registered in GateProfile.commands (combining fast static checks like clippy/ruff/pyright with test execution so suffix test paths work via existing prefix validation), and context_bundle synthesizes workflow_guidance.scoped_commands from profile.commands + contract_seam_test/covering_tests.'
    - 'INV-SEAM-REACHABILITY: A contract submission with seam_test_ref must reference an indexed test symbol or file with a 1-to-2 hop graph edge into allowed_scope when .forge/code-map.sqlite is present.'
    - 'INV-SIGNATURE-BLAST-RADIUS: Modifying the public signature of an indexed symbol inside allowed_scope whose 1-hop external callers lie outside allowed_scope fails closed until extend_scope admits those caller paths.'
    - 'INV-WAVE-DISJOINTNESS: Task sync automatically records top-down task_dependencies edges (i < j) for tasks in the same milestone with overlapping scope paths, preventing mutual TASK_SCOPE_CONFLICT deadlocks among ready tasks.'
    - 'INV-WORKTREE-AFFINITY: A task deferred during extend_scope due to an active claim conflict retains its builder worker and worktree binding and is resumed by that worker once the blocking task completes.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-CANONICAL-RECEIPT-SHA: API responses preserve full 40-character Git SHAs in receipt.commit and candidate_snapshot.commit; gates normalize 7-to-40 char SHA prefixes so source and accepted receipts never diverge.'
    - 'INV-BLACKBOARD-INSPECT-TOLERANCE: task_blackboard(action=inspect) tolerates optional scope/task_id/milestone_ref fields when message_id is supplied and falls back to scoped read when message_id is omitted.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
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

1. In `src/core/tasks/mod.rs` (lines 32–49), update `shorten_task_response_commits` so `receipt.commit`, `candidate_snapshot.commit`, and `latest_snapshot.commit` retain their canonical 40-character Git SHA (matching the milestone Markdown document on disk), keeping 7-char formatting strictly inside `"inspect_cmd"` or a separate `"short_commit"` field.
   Update the affected existing assertion in `tests/acdd/milestones.rs` so API response checks expect the same canonical full SHA.
2. In `src/engine/tasks/mod.rs` and `src/engine/tasks/gates.rs`, when `evidence.commit` is a 7–39 hex-character prefix of the 40-character `build.commit` (or resolves via `git rev-parse --verify`), normalize it to the full 40-character SHA before candidate comparison and gate persistence.
3. In `src/engine/tasks/blackboard.rs` (lines 360–374), update `validate_blackboard_request` for `BlackboardAction::Inspect` so optional `scope`, `task_id`, `subtask_ref`, and `milestone_ref` parameters are ignored when `message_id` is supplied, and requests with `message_id == None` and a valid `task_id` or `milestone_ref` automatically dispatch to `BlackboardAction::Read`.
4. In `src/engine/tasks/workspaces.rs`, inject `NEVER_EDIT_RECEIPTS: Never manually edit receipt: or status: blocks in docs/milestones/*.md; task_submit(stage=deliver) writes and commits receipts automatically.` into `workflow_guidance.rules`.

---

### task: tiered-profile-and-boundary-seam-gates

```yaml
task_ref: tiered-profile-and-boundary-seam-gates
target: Configure tiered ACDD roles (contract_author with sol-6.1:high, builder with gpt6-luna:xhigh, independent_reviewer with sol-6.1:xhigh for chained review+deliver), configure canonical check+test commands, inline ADR invariant summaries, and enforce static seam/signature gates
proof_policy: seam-test-first
contract_revision: 1
scope:
- .forge/acdd/profile.yaml
- AGENTS.md
- TESTS.md
- src/core/tasks/acdd.default.yaml
- src/core/tasks/profile.rs
- src/core/tasks/gates.rs
- src/engine/tasks/context.rs
- src/engine/tasks/gates.rs
- src/engine/tasks/workspaces.rs
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/acdd_profile.yaml.example
- docs/reference/acdd.md
- docs/reference/profiles.md
- docs/reference/tasks.md
- tests/acdd/tasks.rs
- tests/acdd/milestones.rs
- tests/acdd/mcp.rs
depends_on:
- receipt-sha-canonicalization-and-blackboard-ergonomics
invariants:
- 'INV-TIERED-SUBAGENT-PROFILE: Default ACDD profile defines three tiered subagent roles—contract_author (sol-6.1:high for contract seam design), builder (gpt6-luna:xhigh for build code implementation connected via gate-targeted task_blackboard notes), and independent_reviewer (sol-6.1:xhigh governing review and deliver gates in a single reviewer subagent session)—while forbidding preflight subagents.'
- 'INV-PROFILE-CANONICAL-COMMANDS: Build verification strictly uses commands registered in GateProfile.commands (combining fast static checks like clippy/ruff/pyright with test execution so suffix test paths work via existing prefix validation), and context_bundle synthesizes workflow_guidance.scoped_commands from profile.commands + contract_seam_test/covering_tests.'
- 'INV-SEAM-REACHABILITY: A contract submission with seam_test_ref must reference an indexed test symbol or file with a 1-to-2 hop graph edge into allowed_scope when .forge/code-map.sqlite is present.'
- 'INV-SIGNATURE-BLAST-RADIUS: Modifying the public signature of an indexed symbol inside allowed_scope whose 1-hop external callers lie outside allowed_scope fails closed until extend_scope admits those caller paths.'
subtasks:
- subtask_ref: tiered-profile-and-canonical-commands
  title: Configure tiered roles in acdd.default.yaml, .forge/acdd/profile.yaml, and workspaces.rs (contract_author with sol-6.1:high, builder with gpt6-luna:xhigh, independent_reviewer with sol-6.1:xhigh for review and deliver), configure check+test commands, and synthesize workflow_guidance.scoped_commands in context.rs
  status: completed
  evidence: Configured the three default roles, canonical combined Rust command and compatibility alias; scoped guidance selects only rust-check-and-test. Focused production seam passed; final cargo test --test acdd passed (87/87); strict Clippy passed (exit 0); rustfmt and git diff --check passed.
- subtask_ref: blackboard-contract-handoff-and-inline-adr-invariants
  title: Update contract gate steps to post gate=build blackboard notes, chain review+deliver in independent_reviewer guidance, add NO_PREFLIGHT_SUBAGENTS to workflow_guidance.rules, and inline ADR invariants_summary + expected_evidence_template in context.rs
  status: completed
  evidence: Added contract-to-build blackboard handoff, independent reviewer continuity, NO_PREFLIGHT_SUBAGENTS, inline ADR invariant and decision summaries, and claim-bound evidence templates. Verified in final cargo test --test acdd (87/87); strict Clippy passed (exit 0).
- subtask_ref: external-callers-and-static-seam-signature-gates
  title: Add 1-hop external_callers and 2-hop covering_tests to context.rs and enforce TASK_SEAM_DISCONNECTED and TASK_SCOPE_INCOMPLETE in src/engine/tasks/gates.rs
  status: completed
  evidence: |-
    Prior A1/A2 evidence retained: A1 treats public Rust trait methods without explicit pub as exported signature boundaries; inline source-unit proof covers default and declaration-only methods, restricted visibility, and body/private controls. A2 revalidates each indexed seam hop against current claimed-worktree typed source facts and applicable import references, requires exact current node identity, and accepts harmless line shifts without whole-file digest or general value-flow freshness claims. Historical RED: /tmp/m019-a1-red.log, /tmp/m019-a2-red.log, /tmp/m019-inline-red.log. Historical GREEN: /tmp/m019-focused-a1-integration.log, /tmp/m019-focused-a2-integration.log, /tmp/m019-focused-a2-shared-primary.log, /tmp/m019-focused-a1-lib.log.

    M019-AR1 introduced-regression repair: removed only the per-edge edge.dst.is_test rejection from indexed_edge_is_current; current identity/reference/import checks remain at every hop, and non-test terminal guards remain in validate_seam_reachability. The regression asserts an exact persisted SQLite calls witness: src/seam.rs::test_connected (test) -> src/test_helpers.rs::helper (is_test=1) -> src/api.rs::route (is_test=0). Fresh submission passes; rewriting helper to return a constant without reindexing yields TASK_SEAM_DISCONNECTED and retains contract stage. Existing matrix retains terminal-test rejection and three-hop rejection. Pre-fix valid RED: /tmp/m019-ar1-red-validated.log (1 test, fresh exact witness rejected with TASK_SEAM_DISCONNECTED); fixture-only failures before the lower-level module fixture were not counted as RED. GREEN regression: /tmp/m019-ar1-green-focused.log (1/1).

    Focused matrix: /tmp/m019-focused-contract-seams.log (3/3, direct/two-hop/stale/three-hop/shared-index cases), /tmp/m019-focused-a1-traits.log (1/1). Registered build command `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd` exited 0; strict Clippy clean and complete ACDD 98/98 in /tmp/m019-build-clippy-acdd-ar1.log. Separate `cargo test --lib` passed 44/44 in /tmp/m019-build-lib-ar1.log. `cargo fmt --all --check` and `git diff --check` passed.

    Fresh local worktree map: /tmp/m019-code-map-ar1.log; generation/output root 800beec821e5b069f944121f37b479b9921f12f1e839bc81bf372ca5752612bc; 289 files, 5,423 nodes, 18,075 edges, 0 parse errors. Impact /tmp/m019-impact-ar1.log confirms indexed_edge_is_current -> validate_seam_reachability -> TasksStore.submit_with_pre_persist. Residual static-test query /tmp/m019-residual-tests-ar1.log reports 0 direct test relationships and 1 unresolved reference; the focused and full ACDD tests exercise the real submission path. Workspace analyze /tmp/m019-analyze-ar1.log reports 0 parse errors and 34,108 unresolved references across the full graph.

    Unsupported experiments remain non-evidence: prior receiver-type fixture had no fresh indexed edge; earlier alias/order scratch attempt was not executed. Neither is represented as a closed finding or a general flow guarantee.
status: completed
receipt:
  contract_revision: 1
  passed_at: 2026-10-15T01:25:57.092028445+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 98
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed complete four-path repair diff at 549ed219 and bounded closure delta to frozen b1915bd282bc757b16a8217899155e0575cdc672, baseline 4962c83e3478e135c08c7f2be28b7fbb3cb1e02a. All 17 admitted-scope blobs match the accepted candidate; only gates.rs, acdd/tasks.rs and the two admitted task/profile references are modified. Protected milestone id/invariants/owners/dependencies/profile are unchanged. No nonignored untracked files or changes outside accepted scope. Independent cargo fmt --check and git diff --check exit 0, /tmp/m019-closure-review-fmt-diff.log.
        claims:
          applicable: true
          evidence: 'M019-A1 public Rust trait default/declaration methods inherit visible-trait boundary classification, retaining private/body-only controls and baseline external callers across reindex. M019-A2 validates each supported indexed 1/2-hop witness against current claimed-worktree source identity, outgoing typed references and applicable imports; missing/stale nodes/references reject, terminal remains non-test, 3-hop rejects, harmless line shifts pass, shared primary cannot attest a different claimed tree. M019-AR1 closed: only early per-edge dst.is_test rejection was removed; terminal guards remain. New real task-submit regression asserts exact SQLite calls test_connected -> test-marked src/test_helpers.rs helper -> non-test src/api.rs route, accepts fresh, rejects disconnected helper without reindex and retains contract stage. Genuine RED /tmp/m019-ar1-red-validated.log ran 1 failed after exact witness precondition; GREEN /tmp/m019-ar1-green-focused.log ran 1 passed. Independent strict Clippy followed by complete cargo test --test acdd exited 0, 98 passed/0 failed (/tmp/m019-closure-review-clippy-acdd.log); independent cargo test --lib exited 0, 44 passed/0 failed (/tmp/m019-closure-review-lib.log). No general typed value-flow freshness claim; unsupported receiver/alias experiments are excluded.'
        concurrency:
          applicable: true
          evidence: Validation remains within existing claim/profile/revision and candidate fencing before persistence inside the immediate task transaction. No task DB writes or transaction/rollback/receipt changes were added by the currentness algorithm or AR1 repair. Read-only map connections and confined current source reads preserve existing lock order and task namespaces. Complete ACDD independently passes competing claims, stale submissions, atomic delivery and rollback checks with 98/0.
        project_isolation:
          applicable: true
          evidence: Current source is parsed from evidence.worktree through confined_path. Seam validation prefers that worktree's local map and otherwise checks current claimed-tree facts against shared read-only map; real shared-primary regression covers stale test/helper in alternate claimed worktree. No workspace/profile resolution, namespace, symlink, traversal or linked-root policy changes. Current worktree map generation 800beec821e5b069f944121f37b479b9921f12f1e839bc81bf372ca5752612bc has 289 files, 5423 nodes, 18075 edges and 0 parse errors; impact reaches validate_seam_reachability and submit_with_pre_persist. Static direct-test relationship gaps are recorded; real task-submit runs provide production-path proof.
        administration:
          applicable: true
          evidence: No schema, DB/core dependency edge, migration, profile pin, receipt writer, Git delivery or privileged-operation changes. Cohesive validation algorithm stays in existing engine/tasks canonical owner under ADR0018, preserving core leaf. Earlier historical receipts are unchanged. Ledger159 retains A1/A2 dispositions and ledger160 adds AR1 origin, exact RED/GREEN, current map and excluded experiments. Same independent reviewer m019-repair-review differs from builder m019-repair-builder and continues review+delivery in this session after consolidated support approval. No manual task commit or receipt edit.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-CANONICAL-RECEIPT-SHA: API responses preserve full 40-character Git SHAs in receipt.commit and candidate_snapshot.commit; gates normalize 7-to-40 char SHA prefixes so source and accepted receipts never diverge.'
    - 'INV-BLACKBOARD-INSPECT-TOLERANCE: task_blackboard(action=inspect) tolerates optional scope/task_id/milestone_ref fields when message_id is supplied and falls back to scoped read when message_id is omitted.'
    - 'INV-TIERED-SUBAGENT-PROFILE: Default ACDD profile defines three tiered subagent roles—contract_author (sol-6.1:high for contract seam design), builder (gpt6-luna:xhigh for build code implementation connected via gate-targeted task_blackboard notes), and independent_reviewer (sol-6.1:xhigh governing review and deliver gates in a single reviewer subagent session)—while forbidding preflight subagents.'
    - 'INV-PROFILE-CANONICAL-COMMANDS: Build verification strictly uses commands registered in GateProfile.commands (combining fast static checks like clippy/ruff/pyright with test execution so suffix test paths work via existing prefix validation), and context_bundle synthesizes workflow_guidance.scoped_commands from profile.commands + contract_seam_test/covering_tests.'
    - 'INV-SEAM-REACHABILITY: A contract submission with seam_test_ref must reference an indexed test symbol or file with a 1-to-2 hop graph edge into allowed_scope when .forge/code-map.sqlite is present.'
    - 'INV-SIGNATURE-BLAST-RADIUS: Modifying the public signature of an indexed symbol inside allowed_scope whose 1-hop external callers lie outside allowed_scope fails closed until extend_scope admits those caller paths.'
    - 'INV-WAVE-DISJOINTNESS: Task sync automatically records top-down task_dependencies edges (i < j) for tasks in the same milestone with overlapping scope paths, preventing mutual TASK_SCOPE_CONFLICT deadlocks among ready tasks.'
    - 'INV-WORKTREE-AFFINITY: A task deferred during extend_scope due to an active claim conflict retains its builder worker and worktree binding and is resumed by that worker once the blocking task completes.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
    - 'INV-TIERED-SUBAGENT-PROFILE: Default ACDD profile defines three tiered subagent roles—contract_author (sol-6.1:high for contract seam design), builder (gpt6-luna:xhigh for build code implementation connected via gate-targeted task_blackboard notes), and independent_reviewer (sol-6.1:xhigh governing review and deliver gates in a single reviewer subagent session)—while forbidding preflight subagents.'
    - 'INV-PROFILE-CANONICAL-COMMANDS: Build verification strictly uses commands registered in GateProfile.commands (combining fast static checks like clippy/ruff/pyright with test execution so suffix test paths work via existing prefix validation), and context_bundle synthesizes workflow_guidance.scoped_commands from profile.commands + contract_seam_test/covering_tests.'
    - 'INV-SEAM-REACHABILITY: A contract submission with seam_test_ref must reference an indexed test symbol or file with a 1-to-2 hop graph edge into allowed_scope when .forge/code-map.sqlite is present.'
    - 'INV-SIGNATURE-BLAST-RADIUS: Modifying the public signature of an indexed symbol inside allowed_scope whose 1-hop external callers lie outside allowed_scope fails closed until extend_scope admits those caller paths.'
    architectural_notes:
    - 'decisions: {"closure_note":"Use currentness claims narrowly: current indexed identities, current outgoing typed references and applicable imports, plus non-test terminal status. Do not claim whole-file or general typed value-flow freshness.","receiver_fixture":"The receiver-type retarget experiment failed its exact fresh-index edge precondition and was removed; record as not proof, not as a dismissed finding.","review_guidance":"Independent review should focus on every-hop source/witness association, exact import/applicable binding ownership, shared-primary claimed-worktree behavior, implicit-public trait method classification, and fail-closed stale diagnostics. Static graph coverage residual is recorded above."}'
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

1. **Tiered Subagent Roles (`contract_author`, `builder`, `independent_reviewer`) & Blackboard Handoff (`src/core/tasks/acdd.default.yaml`, `.forge/acdd/profile.yaml`, `src/engine/tasks/workspaces.rs`)**:
   - In `src/core/tasks/acdd.default.yaml`, `.forge/acdd/profile.yaml`, and `src/engine/tasks/workspaces.rs` (`default_subagent_role`), configure 3 tiered roles:
     - `contract_author` (`mode: subagent`, `models: ['sol-6.1:high']`, `reuse_on_reject: true`): Assigned to `contract`.
       - Spawn a dedicated `contract_author` subagent with `sol-6.1:high` reasoning to author the sensitive failing seam test, post distilled target/seam notes to `task_blackboard(topic="draft", gate="build")`, and submit `contract_proof`.
     - `builder` (`mode: subagent`, `models: ['gpt6-luna:xhigh']`, `reuse_on_reject: true`): Assigned to `build`.
       - Spawn a fresh `builder` subagent with high-throughput coding model `gpt6-luna:xhigh` that reads the `gate="build"` blackboard note from `context_bundle`, implements the contract inside `allowed_write_scope`, runs the registered profile command, and submits `command_proof`.
     - `independent_reviewer` (`mode: subagent`, `models: ['sol-6.1:xhigh']`, `reuse_on_reject: true`): Assigned to `review` and `deliver` (combining review + delivery into one role/model).
       - Spawn one `independent_reviewer` subagent (`worker_id != builder`) to evaluate all review contours with `sol-6.1:xhigh`, submit `review_proof`, and upon `pass` immediately claim and submit `deliver` in the same session without spawning a redundant delivery worker.
   - In `src/engine/tasks/workspaces.rs`, add `NO_PREFLIGHT_SUBAGENTS: Coordinator executes task_list, task_manage, and worktree setup directly; never spawn throwaway preflight subagents just to run git status or read a single file.` to `workflow_guidance.rules`.
2. **Canonical Profile `commands` & Self-Contained Context (`src/core/tasks/acdd.default.yaml`, `.forge/acdd/profile.yaml`, `src/engine/tasks/context.rs`)**:
   - Use the existing `CommandDef { command, description }` schema (no new fields in `CommandDef`): configure `commands` in `.forge/acdd/profile.yaml` and `acdd_profile.yaml.example` so static checks precede the test runner (`"cargo clippy --all-targets --all-features -- -D warnings && cargo test"` for Rust; `"ruff check && pyright && pytest -q --tb=line -o strict_markers=false"` for Python), leveraging `validate_command`'s existing `strip_prefix` check to accept appended test targets.
   - In `src/engine/tasks/context.rs`:
     - Synthesize `workflow_guidance.scoped_commands` by pairing `profile.commands` with `contract_seam_test` / `covering_tests`.
     - Extract `invariants_summary: Vec<String>` (up to 5 invariant/decision lines per matched ADR) in `scan_adrs` and inject `expected_evidence_template` into `workflow_guidance`.
3. **1-Hop Boundary Context & Static Seam/Signature Gates (`src/engine/tasks/context.rs`, `src/engine/tasks/gates.rs`)**:
   - `query_external_callers` queries `.forge/code-map.sqlite` for up to 25 non-test symbols outside `allowed_scope` with 1-hop `calls`, `possible_calls`, `implements`, or `overrides` edges into `allowed_scope`.
   - On `ProofKind::Contract` submission with `seam_test_ref`, verify via `ast::extract_typed` and `.forge/code-map.sqlite` that the referenced test file/symbol has a 1–2 hop edge into `allowed_scope`, or reject with `TASK_SEAM_DISCONNECTED`.
   - On `contract` or `build` submission, if a modified symbol in `allowed_scope` changed its `details["signature"]` while having 1-hop `external_callers` outside `allowed_scope`, reject with `TASK_SCOPE_INCOMPLETE`.

---

### task: top-down-wave-sync-and-priority-task-next

```yaml
task_ref: top-down-wave-sync-and-priority-task-next
target: Auto-infer top-down task_dependencies and wave_index during task sync for overlapping scopes, add atomic task next dispatcher, and support worktree-preserving parking on extend_scope conflicts
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/core/tasks/mod.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/lifecycle.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/cli/task.rs
- src/cli/milestone.rs
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- docs/reference/tasks.md
- tests/acdd/tasks.rs
- tests/acdd/milestones.rs
- tests/acdd/mcp.rs
depends_on:
- tiered-profile-and-boundary-seam-gates
invariants:
- 'INV-WAVE-DISJOINTNESS: Task sync automatically records top-down task_dependencies edges (i < j) for tasks in the same milestone with overlapping scope paths, preventing mutual TASK_SCOPE_CONFLICT deadlocks among ready tasks.'
- 'INV-WORKTREE-AFFINITY: A task deferred during extend_scope due to an active claim conflict retains its builder worker and worktree binding and is resumed by that worker once the blocking task completes.'
- 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
status: completed
subtasks:
- subtask_ref: auto-infer-topdown-scope-dependencies
  title: During TasksStore::sync in src/db/tasks_store/mod.rs, add top-down task_dependencies edges (i < j) whenever two tasks in the same milestone share overlapping scope paths and compute wave_index
  status: completed
  evidence: 'Production seam tests/acdd/mcp.rs::wave_sync_and_atomic_next_preserve_parked_worktrees: focused cargo test --test acdd wave_sync_and_atomic_next_preserve_parked_worktrees -- --nocapture exit 0, 1 passed/0 failed; complete cargo test --test acdd exit 0, 94 passed/0 failed; cargo clippy --all-targets --all-features -- -D warnings exit 0. Atomic SQLite, declaration-order waves, gate priority/independence, original worktree affinity/bytes, and canonical MCP error propagation verified.'
- subtask_ref: atomic-task-next-dispatcher
  title: Implement ManageAction::Next (CLI task next and MCP task_manage action=next) selecting deliver -> review (builder != worker) -> unblocked parked task -> lowest wave_index ready task
  status: completed
  evidence: 'Production seam tests/acdd/mcp.rs::wave_sync_and_atomic_next_preserve_parked_worktrees: focused cargo test --test acdd wave_sync_and_atomic_next_preserve_parked_worktrees -- --nocapture exit 0, 1 passed/0 failed; complete cargo test --test acdd exit 0, 94 passed/0 failed; cargo clippy --all-targets --all-features -- -D warnings exit 0. Atomic SQLite, declaration-order waves, gate priority/independence, original worktree affinity/bytes, and canonical MCP error propagation verified.'
- subtask_ref: worktree-preserving-extend-scope-parking
  title: When extend_scope hits a path locked by an in_progress sibling task, add the holder to task_dependencies, mark parked_worker_id/parked_worktree, post a blockers blackboard note, and release the active claim
  status: completed
  evidence: 'Production seam tests/acdd/mcp.rs::wave_sync_and_atomic_next_preserve_parked_worktrees: focused cargo test --test acdd wave_sync_and_atomic_next_preserve_parked_worktrees -- --nocapture exit 0, 1 passed/0 failed; complete cargo test --test acdd exit 0, 94 passed/0 failed; cargo clippy --all-targets --all-features -- -D warnings exit 0. Atomic SQLite, declaration-order waves, gate priority/independence, original worktree affinity/bytes, and canonical MCP error propagation verified.'
receipt:
  commit: 1a226240873ba3310c32a69fb0d4fee668aad86f
  contract_revision: 1
  passed_at: 2026-10-14T19:55:27.605222363+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 94
      tests_failed: 0
      log: 'M019-T3-R1 scoped repair: Status::Parked and name mapping admit parked in serde/MCP schema/Clap; updated public task-list help and reference docs. Real production MCP/CLI regression checks exact parked and all legacy filter selections after actual parking; default remains ready; schema catalog includes parked. Pre-fix shipped CLI task list --status parked failed exit 2. Focused: cargo test --test acdd wave_sync_and_atomic_next_preserve_parked_worktrees; exit 0; 1 passed, 0 failed, 93 filtered (1.25s). Required full affected target: cargo test --test acdd; exit 0; 94 passed, 0 failed, 0 filtered (7.61s). Required static check: cargo clippy --all-targets --all-features -- -D warnings; exit 0 (10.11s), no test count. Native rustfmt and git diff --check passed. Scope unchanged; repair trace/mutation matrix blackboard 153 and current closure evidence in review blackboard. Ready for stable independent review confirmation; no additional findings addressed.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate ed68b63 changes exactly 12 files, all in the admitted 15-path task scope; status shows no staged or untracked files. The seven-file repair delta from b23315c is confined to the two status surfaces, the MCP/CLI/docs descriptions, and the ACDD MCP regression test. `git diff --check` is clean and the worktree matches the candidate snapshot across all changed paths.
        claims:
          applicable: true
          evidence: 'The parked status is admitted by engine `Status::Parked` and `Status::name`, which drive both serde/Schemars MCP input and Clap CLI parsing. MCP task_list description, CLI help, and task/CLI/MCP reference docs list parked semantics. The production-seam regression creates an actual parked task through extend_scope, checks exact parked/ready/in_progress/blocked/completed/all selections through MCP stdio and the shipped CLI, and verifies default ready behavior excludes parked tasks. `cargo test --test acdd` exited 0: 94 passed, 0 failed, 0 ignored, 0 measured, 0 filtered; 8.11s test phase.'
        concurrency:
          applicable: true
          evidence: Selection and claim occur inside one SQLite IMMEDIATE transaction; claim revision is advanced and persisted with the open claim row before commit. The ACDD seam test races two MCP server processes at a barrier and verifies exactly one receives the task and exactly one open claim is persisted. Parking writes status, owner dependency, retained worker/worktree, claim closure, and blocker note in one transaction; resume remains revision-fenced.
        project_isolation:
          applicable: true
          evidence: Dispatch selects from the active workspace store, filters candidate task IDs by the owning repository/project, and validates the chosen worktree, task authority, and scope through the workspace registry before claiming. Parked resumption uses the stored canonical worktree and same worker; direct claim enforces both. No workspace, project, profile, or path configuration is changed by this candidate.
        administration:
          applicable: true
          evidence: No profile, schema migration, repository configuration, credential, or protected milestone file is changed. The store schema stays at version 5; the implementation uses the existing task tables and transactional receipt/delivery path. Candidate snapshot remains unchanged after the review test; no delivery or commit has yet been performed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-CANONICAL-RECEIPT-SHA: API responses preserve full 40-character Git SHAs in receipt.commit and candidate_snapshot.commit; gates normalize 7-to-40 char SHA prefixes so source and accepted receipts never diverge.'
    - 'INV-BLACKBOARD-INSPECT-TOLERANCE: task_blackboard(action=inspect) tolerates optional scope/task_id/milestone_ref fields when message_id is supplied and falls back to scoped read when message_id is omitted.'
    - 'INV-TIERED-SUBAGENT-PROFILE: Default ACDD profile defines three tiered subagent roles—contract_author (sol-6.1:high for contract seam design), builder (gpt6-luna:xhigh for build code implementation connected via gate-targeted task_blackboard notes), and independent_reviewer (sol-6.1:xhigh governing review and deliver gates in a single reviewer subagent session)—while forbidding preflight subagents.'
    - 'INV-PROFILE-CANONICAL-COMMANDS: Build verification strictly uses commands registered in GateProfile.commands (combining fast static checks like clippy/ruff/pyright with test execution so suffix test paths work via existing prefix validation), and context_bundle synthesizes workflow_guidance.scoped_commands from profile.commands + contract_seam_test/covering_tests.'
    - 'INV-SEAM-REACHABILITY: A contract submission with seam_test_ref must reference an indexed test symbol or file with a 1-to-2 hop graph edge into allowed_scope when .forge/code-map.sqlite is present.'
    - 'INV-SIGNATURE-BLAST-RADIUS: Modifying the public signature of an indexed symbol inside allowed_scope whose 1-hop external callers lie outside allowed_scope fails closed until extend_scope admits those caller paths.'
    - 'INV-WAVE-DISJOINTNESS: Task sync automatically records top-down task_dependencies edges (i < j) for tasks in the same milestone with overlapping scope paths, preventing mutual TASK_SCOPE_CONFLICT deadlocks among ready tasks.'
    - 'INV-WORKTREE-AFFINITY: A task deferred during extend_scope due to an active claim conflict retains its builder worker and worktree binding and is resumed by that worker once the blocking task completes.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
    - 'INV-WAVE-DISJOINTNESS: Task sync automatically records top-down task_dependencies edges (i < j) for tasks in the same milestone with overlapping scope paths, preventing mutual TASK_SCOPE_CONFLICT deadlocks among ready tasks.'
    - 'INV-WORKTREE-AFFINITY: A task deferred during extend_scope due to an active claim conflict retains its builder worker and worktree binding and is resumed by that worker once the blocking task completes.'
    - 'INV-DIAGNOSTIC-ERGONOMICS: Every task lifecycle error uses one single-line format and propagates that text verbatim to MCP: "{CODE}: {task_id}. {facts}. Do: {action}".'
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

1. In `src/db/tasks_store/mod.rs` (`sync`), for every pair of tasks $(T_i, T_j)$ ($i < j$) in `parsed.tasks` whose `scope` entries overlap via `crate::core::tasks::path_matches_scope_entry`, insert $T_i \to T_j$ into `task_dependencies` (without mutating `TaskSpec.depends_on` in the authority digest) and compute `wave_index` (`0` for unblocked tasks, `max(dep.wave_index) + 1` otherwise).
2. `ManageAction::Next` (`contextunity-forge-mcp task next --worker <id> --worktree <path>` and MCP `task_manage` with `action: "next"`) atomically claims and returns the `context_bundle` for the highest-priority eligible `ready` task:
   - Priority 1: `stage == "deliver"` where `builder != worker_id`.
   - Priority 2: `stage == "review"` where `builder != worker_id`.
   - Priority 3: `ready` task previously parked by `worker_id` (`parked_worker_id == worker_id`) whose blocking dependency is now `completed`.
   - Priority 4: `ready` task at `build` or `contract` ordered by `wave_index ASC, task_id ASC` with no active claim scope overlap.
3. In `TasksStore::extend_scope`, if a requested path conflicts with an `in_progress` task `holder_id`, `extend_scope` appends the path to `task_scope_paths`, inserts `holder_id -> task_id` into `task_dependencies`, stores `parked_worker_id` and `parked_worktree` in the descriptor, posts a `"blockers"` note to `task_blackboard`, ends the open claim row, and returns `{ "status": "parked", "blocked_by": holder_id }`.
