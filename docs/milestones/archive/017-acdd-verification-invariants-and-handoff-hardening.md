---
id: m-acdd-verification-invariants-and-handoff-hardening
title: ACDD verification invariants, safe handoff ancestry, and profile governance hardening
doc_type: contract
status: completed
depends_on:
- m-acdd-declarative-profiles-and-workflow-governance
owners:
- src/core/tasks/
- src/engine/milestones/
- src/engine/tasks/
- src/db/
- docs/
- skills/contextunity-forge/
- tests/
invariants:
- 'INV-REPACK-TREE-VERIFICATION: Repacked task commits in ACDD are verified against the accepted delivery snapshot using scoped tree diffs; any commit diverging in scoped content from the accepted completion is rejected with TASK_COMMIT_TREE_MISMATCH in sync, migrate verify, and milestone handoff.'
- 'INV-HANDOFF-ANCESTRY-SAFETY: Milestone handoff strictly validates that delivery commits descend from the task baseline and are genuine ancestors of the handoff commit, preventing obsolete or foreign commits from being resolved as landed task SHAs.'
- 'INV-FAIL-CLOSED-BASELINE: Retrieval of candidate snapshot baselines from task_gates evidence fails closed on corrupted records or missing baseline fields without swallowing errors in synchronization or migration verification.'
- 'INV-STRICT-PROFILE-SPECIFICATION: ModelSpec and RoleDef enforce strict field parsing and deny unknown attributes; role guidance text dynamically adapts for inline execution modes without stale subagent delegation text.'
- 'INV-ACDD-NOMENCLATURE-ALIGNMENT: Documentation and agent guidance uniformly use Augmented Context-Driven Development nomenclature, reflecting active runbooks and standing Git permission distinctions between automated and manual deliveries.'
related_plans: []
started_at: 2026-10-11T11:53:26+00:00
handoff:
  completed_at: 2026-10-11T12:01:27.199463438+00:00
  duration: 0h 8m
  commit: a92fc18952dd3517039dc517945481082be7d823
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 660
    tests_failed: 0
---

# ACDD verification invariants, safe handoff ancestry, and profile governance hardening

## Outcome and purpose

Harden the ACDD execution engine and milestone handoff mechanics in ContextUnity Forge MCP against commit tampering, stale ancestry substitution, and profile specification drifts:
1. Prevent silent code substitution during task repacking by comparing scoped trees against accepted delivery snapshots (`TASK_COMMIT_TREE_MISMATCH`) and enforcing proper full-tree comparisons when scopes are empty.
2. Secure milestone handoff by validating commit ancestry, rejecting foreign or obsolete delivery commits, and resolving true landed commits in the milestone branch.
3. Eliminate error-swallowing when retrieving candidate baselines from `task_gates`, ensuring fail-closed verification across `sync` and `migrate verify`.
4. Enforce strict profile validation: reject unknown fields on `ModelSpec` and `RoleDef`, parse compound `model:reasoning` identifiers cleanly, and dynamically adapt guidance text for inline role modes.
5. Standardize repository documentation and agent skills around **Augmented Context-Driven Development**, purging dead links and clarifying fast-forward Git landing permissions.

---

## Tasks in this milestone

### task: repack-tree-and-ancestry-verification

```yaml
task_ref: repack-tree-and-ancestry-verification
target: Enforce scoped tree equality and fail-closed ancestry verification for repacked task commits in sync and milestone handoff
proof_policy: seam-test-first
contract_revision: 3
scope:
- src/core/git.rs
- src/core/tasks/mod.rs
- src/db/tasks_store/mod.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/mod.rs
- src/engine/tasks/gates.rs
- src/cli/migrate.rs
- tests/acdd/milestones.rs
- tests/acdd/support.rs
- tests/acdd/mcp.rs
- tests/acdd/tasks.rs
status: completed
subtasks:
- subtask_ref: scoped-tree-equality-check
  title: Implement git_scoped_tree_changed and reject divergent repacked commits with TASK_COMMIT_TREE_MISMATCH
  status: completed
  evidence: null
- subtask_ref: fail-closed-baseline-propagation
  title: Propagate candidate baseline resolution errors in task sync and migrate verify instead of swallowing them
  status: completed
  evidence: null
- subtask_ref: handoff-ancestry-and-landed-commit-resolution
  title: Enforce ancestor validation and exact landed commit resolution in milestone handoff
  status: completed
  evidence: null
receipt:
  commit: d530c7adc9936d43061f0911dd713ecf9565fb26
  contract_revision: 3
  passed_at: 2026-10-11T11:57:23.297489038+00:00
  evidence:
    command_proof:
      command: cargo test
      exit_code: 0
      tests_passed: 73
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Changes strictly confined to repack, tree check, and ancestry verification files in task scope.
        claims:
          applicable: true
          evidence: Scoped tree equality, fail-closed baseline propagation, and handoff ancestry invariants proved.
        concurrency:
          applicable: true
          evidence: SQLite transactional gates, rollback protections, and generation tracking verified.
        project_isolation:
          applicable: true
          evidence: Paths remain confined within repository workspace root.
        administration:
          applicable: true
          evidence: Task delivery ancestry checks and repack verification enforce fail-closed integrity.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPACK-TREE-VERIFICATION: Repacked task commits in ACDD are verified against the accepted delivery snapshot using scoped tree diffs; any commit diverging in scoped content from the accepted completion is rejected with TASK_COMMIT_TREE_MISMATCH in sync, migrate verify, and milestone handoff.'
    - 'INV-HANDOFF-ANCESTRY-SAFETY: Milestone handoff strictly validates that delivery commits descend from the task baseline and are genuine ancestors of the handoff commit, preventing obsolete or foreign commits from being resolved as landed task SHAs.'
    - 'INV-FAIL-CLOSED-BASELINE: Retrieval of candidate snapshot baselines from task_gates evidence fails closed on corrupted records or missing baseline fields without swallowing errors in synchronization or migration verification.'
    - 'INV-STRICT-PROFILE-SPECIFICATION: ModelSpec and RoleDef enforce strict field parsing and deny unknown attributes; role guidance text dynamically adapts for inline execution modes without stale subagent delegation text.'
    - 'INV-ACDD-NOMENCLATURE-ALIGNMENT: Documentation and agent guidance uniformly use Augmented Context-Driven Development nomenclature, reflecting active runbooks and standing Git permission distinctions between automated and manual deliveries.'
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

### task: profile-validation-and-role-adaptation

```yaml
task_ref: profile-validation-and-role-adaptation
target: Harden profile parsing with strict model reasoning specs, deny unknown fields, and adapt inline role guidance
proof_policy: seam-test-first
contract_revision: 3
scope:
- .forge/acdd/profile.yaml
- src/core/tasks/profile.rs
- src/core/tasks/acdd.default.yaml
- src/engine/tasks/workspaces.rs
- src/cli/guide.rs
- tests/acdd/tasks.rs
- skills/contextunity-forge/references/acdd_profile.yaml.example
status: completed
subtasks:
- subtask_ref: strict-model-spec-parsing
  title: Parse model specs with explicit reasoning tokens and enforce deny_unknown_fields on ModelSpec and RoleDef
  status: completed
  evidence: null
- subtask_ref: inline-role-guidance-adaptation
  title: Adapt workflow guidance text dynamically when role execution mode is inline
  status: completed
  evidence: null
- subtask_ref: configuration-examples-hardening
  title: Update reference profile examples to valid guidance keys and active schema
  status: completed
  evidence: null
receipt:
  commit: 13962e132bd37784d3a4112d3fb82ca9d67b242d
  contract_revision: 3
  passed_at: 2026-10-11T11:54:43.356548559+00:00
  evidence:
    command_proof:
      command: cargo test
      exit_code: 0
      tests_passed: 73
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All changed files are strictly confined to task initial scope.
        claims:
          applicable: true
          evidence: ModelSpec and RoleDef parsing invariants proved by tests/acdd/tasks.rs.
        concurrency:
          applicable: true
          evidence: SQLite transactions and claim revision fencing verified.
        project_isolation:
          applicable: true
          evidence: Workspace confinement checks maintained without boundary escapes.
        administration:
          applicable: true
          evidence: Profile validation and deny_unknown_fields enforcement strictly adhere to administrative invariant.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPACK-TREE-VERIFICATION: Repacked task commits in ACDD are verified against the accepted delivery snapshot using scoped tree diffs; any commit diverging in scoped content from the accepted completion is rejected with TASK_COMMIT_TREE_MISMATCH in sync, migrate verify, and milestone handoff.'
    - 'INV-HANDOFF-ANCESTRY-SAFETY: Milestone handoff strictly validates that delivery commits descend from the task baseline and are genuine ancestors of the handoff commit, preventing obsolete or foreign commits from being resolved as landed task SHAs.'
    - 'INV-FAIL-CLOSED-BASELINE: Retrieval of candidate snapshot baselines from task_gates evidence fails closed on corrupted records or missing baseline fields without swallowing errors in synchronization or migration verification.'
    - 'INV-STRICT-PROFILE-SPECIFICATION: ModelSpec and RoleDef enforce strict field parsing and deny unknown attributes; role guidance text dynamically adapts for inline execution modes without stale subagent delegation text.'
    - 'INV-ACDD-NOMENCLATURE-ALIGNMENT: Documentation and agent guidance uniformly use Augmented Context-Driven Development nomenclature, reflecting active runbooks and standing Git permission distinctions between automated and manual deliveries.'
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

### task: documentation-and-workflow-alignment

```yaml
task_ref: documentation-and-workflow-alignment
target: Align repository documentation, agent guidance, and skills with Augmented Context-Driven Development nomenclature and accurate Git permissions
proof_policy: direct-proof
contract_revision: 2
scope:
- docs/reference/acdd.md
- docs/reference/tasks.md
- docs/reference/configuration.md
- docs/runbooks/README.md
- docs/runbooks/acdd.md
- docs/AGENTS.md
- docs/README.md
- AGENTS.md
- README.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/agents.md.example
- tests/AGENTS.md
status: completed
subtasks:
- subtask_ref: acdd-nomenclature-standardization
  title: Standardize documentation headings and references to Augmented Context-Driven Development
  status: completed
  evidence: null
- subtask_ref: purge-dead-runbook-links
  title: Remove references to deleted runbooks and point agents to current reference docs
  status: completed
  evidence: null
- subtask_ref: standing-git-permissions-clarification
  title: Clarify auto_commit permissions and fast-forward milestone landing rules in agent guidance
  status: completed
  evidence: null
receipt:
  commit: a92fc18952dd3517039dc517945481082be7d823
  contract_revision: 2
  passed_at: 2026-10-11T11:58:31.718640952+00:00
  evidence:
    command_proof:
      command: cargo test
      exit_code: 0
      tests_passed: 73
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Documentation, agent instructions, and skill references strictly confined to task scope.
        claims:
          applicable: true
          evidence: Augmented Context-Driven Development nomenclature and accurate Git permissions verified.
        concurrency:
          applicable: true
          evidence: No concurrent store state conflicts; documents verified.
        project_isolation:
          applicable: true
          evidence: All documentation confined to workspace repository paths.
        administration:
          applicable: true
          evidence: Purged dead runbook references and aligned standing permissions.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPACK-TREE-VERIFICATION: Repacked task commits in ACDD are verified against the accepted delivery snapshot using scoped tree diffs; any commit diverging in scoped content from the accepted completion is rejected with TASK_COMMIT_TREE_MISMATCH in sync, migrate verify, and milestone handoff.'
    - 'INV-HANDOFF-ANCESTRY-SAFETY: Milestone handoff strictly validates that delivery commits descend from the task baseline and are genuine ancestors of the handoff commit, preventing obsolete or foreign commits from being resolved as landed task SHAs.'
    - 'INV-FAIL-CLOSED-BASELINE: Retrieval of candidate snapshot baselines from task_gates evidence fails closed on corrupted records or missing baseline fields without swallowing errors in synchronization or migration verification.'
    - 'INV-STRICT-PROFILE-SPECIFICATION: ModelSpec and RoleDef enforce strict field parsing and deny unknown attributes; role guidance text dynamically adapts for inline execution modes without stale subagent delegation text.'
    - 'INV-ACDD-NOMENCLATURE-ALIGNMENT: Documentation and agent guidance uniformly use Augmented Context-Driven Development nomenclature, reflecting active runbooks and standing Git permission distinctions between automated and manual deliveries.'
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
