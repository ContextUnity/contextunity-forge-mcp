---
id: m-acdd-declarative-profiles-and-workflow-governance
title: ACDD declarative profiles, workflow governance, and standalone onboarding
doc_type: contract
status: active
depends_on:
- m-acdd-lifecycle-snapshots-and-proactive-defect-resolution
owners:
- src/core/tasks/
- src/engine/tasks/
- src/db/
- src/cli/
- src/mcp/
- skills/contextunity-forge/
- docs/
invariants:
- 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
- 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
- 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
- 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
- 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources) plus the milestone receipt, with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
- 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 5 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v4 to v5 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v5 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
- 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
related_plans: []
started_at: 2026-10-10T05:51:02+00:00
---

# ACDD declarative profiles, workflow governance, and standalone onboarding

## Outcome and purpose

Solidify the ACDD execution framework and ContextUnity Forge MCP governance so that any repository (internal or external) can seamlessly adopt, configure, and execute augmented contracts:
1. Establish a declarative YAML profile engine (`acdd_profile`) as the single source of truth for arbitrary workflow gates, strictly separating arbitrary gate `id` from closed `proof` taxonomy (`contract`, `command`, `review`, `delivery`, `none`, and `scheme` with recursive structural validation under `proof: { scheme: ... }` driving bundles, independence, and receipts), gate fields (explicit `sha_snapshot: bool` per gate in defaults), `commands` registry, hierarchical `policies`, and nested `contours` dictionary (`IndexMap` preserving order) with operational criteria. Fail-closed compilation on merged profiles enforces non-empty gates, unique IDs, strictly prior `reject_to` and `independent_from`, strictly prior `review_sources` (all verifying the identical candidate SHA), exactly one terminal `delivery`, and existing roles/contours/commands. Profiles support multi-tier resolution: `task.spec.acdd_profile` > `milestone.acdd_profile` > `forge-mcp.yaml` link > `.forge/acdd/profile.yaml` > embedded defaults. Single-line profile pinning (`acdd_profile: "<path>:<sha256_prefix>"`, minimum 7 hex characters) in contract specifications is recorded in SQLite task metadata during `task sync`, and Forge verifies file hash integrity on claim and execution, failing closed with `TASK_PROFILE_TAMPERED` on mismatch. Task operations validating gate identifiers fail closed with `TASK_STAGE_UNKNOWN` if a task stage is missing from active profile gates. Runtime code enforces zero backward-compatibility shims across SQLite runtime data and active milestones (`command_proof`, clean gate tokens, 6 canonical blackboard topics); historical receipts in `docs/milestones/archive/` are exempt from active profile validation. Configuration paths in `.forge/acdd/**` are protected from `extend_scope` (`TASK_SCOPE_PROTECTED`) and uncontracted modifications fail with `TASK_SCOPE_VIOLATION` unless explicitly admitted in the active task's initial planning-time `task.spec.scope`. Profiles in linked workspaces are strictly ignored (only the active workspace root defines the profile). Empty `gates` in override inherits base; non-empty replaces whole sequence; roles merge field-wise (repository profile specifying `sol-6.1 high` advisory reviewer). Delivery gates (`proof: delivery`) support `auto_commit: bool` (default true) automating Git commits from the candidate snapshot tree of the nearest predecessor snapshot gate (e.g. `build`) plus milestone receipt with parent HEAD (verifying `git rev-parse HEAD` matches `candidate_baseline_head`), hooks enforced, and atomic rollback on failure (task remains uncompleted at `deliver`). The created delivery commit SHA is saved in the task's delivery receipt in SQLite, and milestone handoff rewrites receipt.commit for each task with its landed commit SHA in the milestone branch before archiving. Delivery rolls up reviews across `review_sources`.
2. Transition the `contextunity-forge` skill into a general MCP tool reference, adding runnable examples (`references/acdd_profile.yaml.example`, `references/forge-mcp.yaml.example`, and canonical `references/gitignore.example` tracking `.forge/acdd/**` including `.forge/acdd/profile.yaml`, and framework extensions in `.forge/frameworks/**`) and delegating specific gate rules to dynamic `workflow_guidance`, explicitly documenting that gate sequences are profile-driven and default gate IDs are baseline examples. Explicitly codify standing Git permissions in `references/agents.md.example` and `AGENTS.md`: when `auto_commit: true`, Forge commits upon deliver; agents do not create duplicate task commits, and agents commit only when `auto_commit: false` or upon milestone archive after handoff.
3. Standardize nomenclature across the entire active codebase and documentation to **Augmented Contract-Driven Development** (retiring "admitted") and simplify default gate identifiers by removing the `/v1` suffix (`contract`, `build`, `review`, `deliver`) while verifying that `task list --stage` and MCP stage arguments validate dynamically against active profile gates without altering historical archive milestones or ADRs.
4. Release task scope locks upon task completion in `TasksStore` strictly during claim and `extend_scope` conflict checks, allowing sequential sibling tasks to evolve documentation and shared files without artificial `TASK_SCOPE_CONFLICT` errors while preserving historical path records.
5. Improve task guidance validation to accurately recognize repository skill references (including `contextunity-forge-mcp skill`) using semantic word-boundary and contextual parsing, and validate repository `.gitignore` against canonical rules to eliminate blanket `.forge/` directory ignores that inadvertently exclude `.forge/acdd/**` (including `.forge/acdd/profile.yaml`) or framework extensions in `.forge/frameworks/**`.
6. Codify repository-wide test gates in `TESTS.md`, clarify CLI-only milestone lifecycle semantics (`milestone init/list/show/handoff`), and update milestone handoff to reconcile and rewrite receipt.commit in each task block and SQLite store with the task's landed commit SHA in the milestone branch before archiving.
7. Upgrade the task-store schema from version 4 to version 5 for blackboard gate targeting, canonical single-word topics (`draft`, `notes`, `findings`, `blockers`, `decisions`, `deferred`), single legacy topic migration (`contract_draft` -> `draft`, `contract_findings` -> `findings`, `build_proof` -> `notes`, `architectural_notes` -> `decisions`; canonical topics preserved unchanged; unknown -> `notes`), rich schema hints, and receipt rollup for decisions and deferred topics.

---

## Tasks in this milestone

### task: completed-task-scope-release-and-co-evolution-planning

```yaml
task_ref: completed-task-scope-release-and-co-evolution-planning
target: Release exclusive task scope locks for completed tasks in conflict checks during claim and extend_scope, and codify in-build documentation co-evolution scope planning
proof_policy: direct-proof
contract_revision: 20
depends_on: []
invariants:
- 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
scope:
- src/db/tasks_store.rs
- docs/reference/tasks.md
- tests/acdd/tasks.rs
- tests/acdd/blackboard.rs
- tests/acdd/mcp.rs
- tests/acdd/subtasks.rs
status: completed
subtasks:
- subtask_ref: filter-completed-scope-locks
  title: Filter out completed tasks (status != 'completed') in conflict check queries during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks
  status: completed
  evidence: Updated claim and extend_scope conflict queries to exclude completed sibling rows while retaining their scope history; added same-project namespace filtering and claimability checks. Verified by the completed-owner and active-owner SQLite seam scenarios in tests/acdd/tasks.rs. `cargo test --test acdd` passed 63/63.
- subtask_ref: document-co-evolution-scope-rules
  title: Codify in-build documentation co-evolution scope planning and conflict release rules in docs/reference/tasks.md
  status: completed
  evidence: Updated docs/reference/tasks.md with planning-time documentation co-evolution scope, claim/extension conflict rules, completed-scope release, and linked project namespace isolation.
receipt:
  commit: 96c3f9d96eab284c4587b6965ed4be3d894a9e63
  contract_revision: 20
  passed_at: 2026-10-10T06:18:38.859453723+00:00
  evidence:
    test_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 63
      tests_failed: 0
      log: 'cargo clippy --all-targets --all-features -- -D warnings: exit 0'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: GPT-6.1 Sol independently compared baseline 2b8a466 to candidate 96c3f9d. Exact, ancestor, and descendant path overlaps use component-aware PathBuf::starts_with in claim; extension preserves normalization, admitted-root and symlink confinement. Scope seam covers frozen and dynamic overlap shapes.
        claims:
          applicable: true
          evidence: Both claim and extend_scope conflict queries exclude completed siblings. Claim conflict selection also applies milestone/project namespace and dependency readiness; documentation describes sequencing, release, namespace boundaries, and retained history.
        concurrency:
          applicable: true
          evidence: Conflict selection/acquisition is inside an immediate SQLite transaction; rejection occurs before claim insertion or extension persistence. Existing simultaneous claim test still requires exactly one winner; active-owner rejection is exercised after acquiring the owner.
        project_isolation:
          applicable: true
          evidence: Both queries bind milestone identity and exact repository/project namespace prefix. Reviewer verified trailing slash namespace construction, separator validation, workspace resolution in public callers, and linked-workspace overlap coverage.
        administration:
          applicable: true
          evidence: Completed status releases conflict selection without deleting stored scope rows; seam asserts historical path remains. Delivery updates completion/blackboard state, reset restores ready while retaining scope. Fixture changes retain blackboard, minimal-routing and proof-policy assertions with distinct valid paths.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources), with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 3 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v2 to v3 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v3 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
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

### task: agent-guidance-skill-validation-and-mcp-semantics

```yaml
task_ref: agent-guidance-skill-validation-and-mcp-semantics
target: Enforce accurate forge skill reference detection with word-boundary and contextual parsing, author canonical gitignore.example in skill references supporting .forge/acdd/** (including .forge/acdd/profile.yaml) and framework extensions (.forge/frameworks/), and validate workspace .gitignore for exact compliance rejecting directory-level .forge/ ignore in task guidance
proof_policy: seam-test-first
contract_revision: 17
depends_on:
- completed-task-scope-release-and-co-evolution-planning
invariants:
- 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
scope:
- src/engine/tasks/workspaces.rs
- tests/acdd/tasks.rs
- docs/reference/configuration.md
- skills/contextunity-forge/references/gitignore.example
- .gitignore
status: completed
subtasks:
- subtask_ref: implement-semantic-skill-mention-detection
  title: 'Implement mentions_forge_skill with exact token matching and contextual skill-word detection for -mcp references (seam test: tests/acdd/tasks.rs::task_guidance_requires_named_and_installed_forge_skill, breaking mutation: reject -mcp references)'
  status: pending
  evidence: null
- subtask_ref: author-canonical-gitignore-example
  title: 'Author canonical skills/contextunity-forge/references/gitignore.example specifying precise rules: ignoring .forge/*.sqlite*, .forge/*.lock, .forge/*.log, .forge/*.jsonl, .forge/tasks/, .forge/checkpoints.json, while tracking .forge/acdd/** (including .forge/acdd/profile.yaml) and framework extensions in .forge/frameworks/**'
  status: pending
  evidence: null
- subtask_ref: validate-gitignore-against-canonical-template
  title: Implement workspace .gitignore validation in src/engine/tasks/workspaces.rs ensuring repository .gitignore matches canonical rules (rejecting directory pattern .forge/ and ensuring .forge/frameworks/ and .forge/acdd/** remain tracked), reporting actionable guidance gaps
  status: pending
  evidence: null
- subtask_ref: add-mcp-skill-and-gitignore-validation-tests
  title: Add tests in tests/acdd/tasks.rs verifying both contextunity-forge and contextunity-forge-mcp skill references, as well as canonical .gitignore compliance and blanket .forge/ ignore detection
  status: pending
  evidence: null
- subtask_ref: align-configuration-guidance-docs
  title: Update docs/reference/configuration.md codifying canonical .gitignore configuration tracking .forge/acdd/** (including .forge/acdd/profile.yaml), framework extensions in .forge/frameworks/, and guidance validation
  status: pending
  evidence: null
receipt:
  commit: f8f4b2569001487c77b32760b130b708bbb2cc48
  contract_revision: 17
  passed_at: 2026-10-10T06:59:48.534792408+00:00
  evidence:
    test_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 63
      tests_failed: 0
      log: 'Final focused seam `cargo test --test acdd task_guidance_requires_named_and_installed_forge_skill`: exit 0, 1 passed, 0 failed. Final full ACDD `cargo test --test acdd`: exit 0, 63 passed, 0 failed. Final `cargo clippy --all-targets --all-features -- -D warnings`: exit 0. Final `git diff --check`: exit 0. R1 mutation evidence: temporarily exempting an appended `*.md` ignore rule from the exact-suffix check made the focused seam fail because validation returned no warning; restored the strict guard and reran all final green gates.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent Sol review confirms candidate f8f4b25 contains exactly the five admitted paths and current file hashes match the snapshot.
        claims:
          applicable: true
          evidence: Independent Sol review confirms exact final active-rule suffix guards against later wildcard/negation overrides; Markdown block boundaries preserve skill-name recognition; warning gives the complete .gitignore repair block. Earlier R1-R3 are closed.
        concurrency:
          applicable: false
          evidence: No new shared mutable state or concurrency behavior; parser and matcher remain local.
        project_isolation:
          applicable: true
          evidence: Independent candidate-helper probes confirm deep .forge/acdd/** and .forge/frameworks/** remain trackable and representatives of all six runtime classes remain ignored; workspace root is selected correctly.
        administration:
          applicable: true
          evidence: Reviewer worker codex-016-guidance-review-sol61-high-r3-20261010 is distinct from builder codex-016-guidance-builder-luna-max-repair2-20261010; contract revision 17 and candidate f8f4b25 verified.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources), with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 3 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v2 to v3 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v3 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
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

### task: acdd-skill-single-source-of-truth-and-spec-consolidation

```yaml
task_ref: acdd-skill-single-source-of-truth-and-spec-consolidation
target: Consolidate ACDD tool usage, onboarding reference examples, and standing Git permissions into shared skill as generic reference and redirect redundant reference spec
proof_policy: direct-proof
contract_revision: 17
depends_on:
- agent-guidance-skill-validation-and-mcp-semantics
invariants:
- 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
scope:
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/agents.md.example
- docs/runbooks/acdd.md
- docs/reference/acdd.md
- tests/mcp/skill_sync.rs
- tests/mcp.rs
- AGENTS.md
status: completed
subtasks:
- subtask_ref: elevate-skill-tool-guidance-and-standing-permissions
  title: 'Codify generic MCP tool usage, protocols, standing Git permissions, and delivery auto-commit semantics in skills/contextunity-forge/SKILL.md and AGENTS.md, documenting that delivery automatically commits candidate tree and milestone receipt when auto_commit: true (default) eliminating duplicate agent commits, that agents only commit task deliverables when auto_commit: false or upon milestone archive after handoff, and that workflow gate sequences are sourced dynamically from the active profile'
  status: pending
  evidence: null
- subtask_ref: maintain-onboarding-agents-md-example
  title: 'Maintain canonical skills/contextunity-forge/references/agents.md.example providing clean template for repository AGENTS.md routing and standing Git permissions (Forge commits on deliver when auto_commit: true; agents commit when auto_commit: false or milestone archive after handoff)'
  status: pending
  evidence: null
- subtask_ref: replace-redundant-reference-acdd-doc-with-redirect
  title: Replace deprecated docs/reference/acdd.md with clean cross-references pointing to SKILL.md and runbooks
  status: pending
  evidence: null
- subtask_ref: track-skill-drift-verification-test
  title: Update tests/mcp/skill_sync.rs to actively assert synchronization between repo skill and installed global skill (~/.agents/skills/contextunity-forge), failing when drift is detected
  status: pending
  evidence: null
receipt:
  commit: 98b7e831ff7c66b95871376c1292ce64faa5d2a6
  contract_revision: 17
  passed_at: 2026-10-10T07:27:10.811617997+00:00
  evidence:
    test_proof:
      command: CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/forge-skill-sync-016-r1-0t2dmnj6/matching cargo test --test mcp
      exit_code: 0
      tests_passed: 78
      tests_failed: 0
      log: 'Focused skill_sync seam with isolated matching skill copy: cargo test --test mcp skill_sync — exit 0, 2 passed, 0 failed (complete file-tree/content equality plus portable Markdown-link resolution from both user-level and repository-local install layouts). Full MCP suite with the same /tmp override: exit 0, 78 passed, 0 failed. Clippy: cargo clippy --all-targets --all-features -- -D warnings — exit 0. git diff --check — exit 0. Repository-relative Markdown link validation — 5 changed files passed. Regression evidence: before the SKILL.md fix, the link-resolution seam failed with missing task-reference targets in both isolated supported layout copies for ../../docs/reference/tasks.md; after replacing it with https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/tasks.md, both copies pass. Global installation was not modified; its pre-existing missing references/gitignore.example drift remains separately known.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Same stable reviewer confirms candidate 98b7e831ff7c66b95871376c1292ce64faa5d2a6, unchanged contract revision 17/digest 02bc69af0662989b2a19741d152beb43c15abd712d14c2c5f1e7acc2f45fd0f3 and same seven admitted paths. Compared original baseline 4c980a5, rejected candidate 2ede787, and remediation 98b7e83. Remediation changes only skills/contextunity-forge/SKILL.md and tests/mcp/skill_sync.rs. Original scoped diff remains the six authorized changed files; tests/mcp.rs remains registered and unchanged. All seven working file byte contents equal candidate before and after proof. Forge overview/claim confirm exact active absolute workspace. No repository/global edits by reviewer.
        claims:
          applicable: true
          evidence: 'PASS; sole cumulative raw finding R1 from review claim 4 is confirmed-fixed. Introduced-regression origin remains attributed to 2ede787 versus 4c980a5. 98b7e83 replaces escaping ../../docs/reference/tasks.md with canonical HTTPS Forge task-reference URL, preserving schema ownership. New real installed-layout boundary test copies the complete skill and resolves Markdown destinations in both documented install layouts. Reviewer independently compiled the candidate''s exact validate_installed_skill_links implementation under /tmp: old href fails in both global-like and repo-local layouts; existing package-contained target passes; canonical URL passes without network. Current cargo test --test mcp skill_sync with isolated matching copy passes 2/2. Full five-contour first review is retained for unchanged instruction/docs behavior: generic tool reference and profile delegation, reference redirect, Git auto_commit true/false and final archive rules remain consistent. No other prior findings existed; no residual R1 remains.'
        concurrency:
          applicable: false
          evidence: No production concurrency, SQLite, task-claim locking, or transaction behavior changes. Original sync test remains read-only with subprocess-local override. New installation-link test uses a uniquely created temporary directory incorporating process ID/time, copies immutable source bytes, collects both layout failures, and removes its owned directory on Drop. Reviewer proofs use independent /tmp trees and preserve stable scoped source. Concurrency control of task runtime is outside this candidate.
        project_isolation:
          applicable: true
          evidence: 'R1 installed-path isolation is restored: task-reference navigation no longer depends on consumer .agents/docs or Forge checkout location. The validator canonicalizes relative destinations and rejects targets outside the copied installed package; no user global skill is changed. Whole-tree sync still roots local skill at CARGO_MANIFEST_DIR and default global skill at HOME/.agents/skills/contextunity-forge. Fresh reviewer controls show missing global root, missing relative file, changed bytes, and extra file all exit 101. Supported global-like and repository-local installs both pass current boundary proof. Existing real-global installation drift from first review remains separately reported, not converted into a source regression or concealed by edits.'
        administration:
          applicable: true
          evidence: 'Read cumulative admission ledger /tmp/016-skill-sot-evidence-map.md and compared sole R1 plus previous rejected gate evidence. Current accepted build claim 5 records isolated focused 2/2, full MCP 78/78, Clippy all targets/features -D warnings exit0, diff check and five-file repository link validation pass. Reviewer fresh focused 2/2 plus exact current link-validator mutation matrix passed, proof artifacts /tmp/016-r1-review-xrub1qy8. Fresh repository-relative file/anchor validation passes for all five changed instruction/docs files; git diff --check 4c980a5 98b7e83 passes. No network success claim is made: canonical HTTPS link portability is validated structurally; remote content availability is not part of R1. Global installed drift remains an explicit environment limitation. R1 disposition confirmed-fixed by the same independent reviewer on this exact candidate.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources), with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 3 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v2 to v3 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v3 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
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

### task: milestone-handoff-contract-and-repo-verification-gates

```yaml
task_ref: milestone-handoff-contract-and-repo-verification-gates
target: Codify TESTS.md verification gates, clarify CLI-only milestone handoff recording and task landed commit rewriting, and align tool guidance across CLI and MCP
proof_policy: direct-proof
contract_revision: 17
depends_on:
- acdd-skill-single-source-of-truth-and-spec-consolidation
invariants:
- 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
scope:
- TESTS.md
- README.md
- AGENTS.md
- docs/AGENTS.md
- docs/README.md
- docs/reference/cli.md
- docs/reference/mcp-tools.md
- docs/testing/README.md
- src/cli/guide.rs
- src/mcp/tools.rs
status: completed
subtasks:
- subtask_ref: author-repo-tests-specification
  title: Author canonical TESTS.md defining task-level and milestone handoff verification gates
  status: completed
  evidence: TESTS.md now has YAML metadata and canonical task-level and milestone gates. It defines the strict Clippy command, the exact chained handoff command, sums per-target passed counts from cargo test --all-targets, excludes Clippy from test counts, and states that the caller runs verification before handoff. Verified with git diff --check and the local Markdown link-target check (81 links).
- subtask_ref: clarify-milestone-cli-only-and-handoff-semantics
  title: Clarify across README.md, CLI guides, and docs that milestone lifecycle commands are CLI-only, record verified test results, and that handoff rewrites receipt.commit for each task with its landed commit SHA in the milestone branch before archiving
  status: completed
  evidence: README.md and docs/reference/cli.md state that milestone lifecycle commands are CLI-only and verification results are caller-run and recorded, not executed. The handoff contract specifies landed task receipt.commit values, handoff.commit as branch HEAD at handoff, and the later archive commit as unrecorded. Runtime implementation is linked as a deferred prerequisite in task_blackboard message 125 to forge-mcp/forge-mcp/m-acdd-declarative-profiles-and-workflow-governance:acdd-configurable-profiles-and-workflow-engine / declarative-sha-snapshots-and-receipt-contour-validation. Verified with git diff --check and the local Markdown link-target check.
- subtask_ref: repo-neutral-lint-guidance-in-guide
  title: Replace Rust-specific Clippy mention in src/cli/guide.rs with repo-neutral lint check result
  status: completed
  evidence: src/cli/guide.rs describes build_proof as a focused test result plus a repo-neutral lint check result. Verified by Forge code_map_inspect and git diff --check.
- subtask_ref: clean-code-map-analyze-description
  title: Remove obsolete SQL references from code_map_analyze description in src/mcp/tools.rs
  status: completed
  evidence: src/mcp/tools.rs describes workspace/path diagnostics, stored syntax diagnostics, and cycles without advertising SQL on code_map_analyze. Focused MCP seam passed 1/1; Clippy passed. Full `cargo test --test mcp` was 77 passed/1 failed on the out-of-scope installed global skill drift, recorded separately in build evidence.
receipt:
  commit: 23199b91480980b4f0eb92f31c075bdbd357ffd2
  contract_revision: 17
  passed_at: 2026-10-10T07:46:34.341231101+00:00
  evidence:
    test_proof:
      command: cargo test --test mcp mcp_analyze_router_checkpoint_and_guide_boundaries
      exit_code: 0
      tests_passed: 1
      tests_failed: 0
      log: 'Focused production-seam proof: 1 passed, 0 failed, 77 filtered out. Required unfiltered affected target cargo test --test mcp: exit 101, 77 passed, 1 failed; failure was skill_sync::test_global_skill_matches_local_skill_when_present because the external global ContextUnity Forge skill install differs from the worktree-local skill (missing references/gitignore.example; changed SKILL.md and references/agents.md.example). cargo clippy --all-targets --all-features -- -D warnings: exit 0. git diff --check: exit 0 for tracked changes; TESTS.md no-index whitespace diagnostics: 0. Changed-document local-link check: 81 checked, 0 broken. TESTS.md now separates focused proof from the required full affected-target task gate while retaining milestone commands/count rules. Handoff receipt rewrite semantics remain a contract only and are deferred to forge-mcp/forge-mcp/m-acdd-declarative-profiles-and-workflow-governance:acdd-configurable-profiles-and-workflow-engine, subtask declarative-sha-snapshots-and-receipt-contour-validation.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Exact candidate 23199b91480980b4f0eb92f31c075bdbd357ffd2 contains exactly the ten admitted paths. Compared git ls-tree blob hashes with git hash-object for all ten current files: exact match. Prior reviewed candidate 62e5966 to current diff changes only TESTS.md. All unrelated dirty state preserved.'
        claims:
          applicable: true
          evidence: 'R1 confirmed-fixed: TESTS.md explicitly separates focused contract/build proof from the unfiltered cargo test --test <affected-target> at build and review and requires command, exit code, passed and failed counts. This satisfies AGENTS.md:19 and docs/testing/README.md:23 and canonical task-level gate authority. Milestone commands, summed all-targets passed counts, Clippy exclusion and caller-run verification remain unchanged. Previously inspected CLI/MCP code descriptions and handlers are identical blob hashes. Milestone lifecycle is documented CLI-only; landed task receipt.commit, handoff.commit at HEAD and later archive commit remain correctly distinguished as the admitted contract. Independently inspected deferred blackboard message 125: runtime rewrite is explicitly owned by successor task/subtask, not claimed completed. Accepted current build evidence: focused MCP command cargo test --test mcp mcp_analyze_router_checkpoint_and_guide_boundaries exit 0, 1 passed/0 failed; unfiltered cargo test --test mcp exit 101, 77 passed/1 failed on skill_sync::test_global_skill_matches_local_skill_when_present due external global skill drift (missing references/gitignore.example and differing installed SKILL.md/agents example), explicitly recorded without global edits. Strict Clippy exit 0, tracked diff check exit 0, TESTS whitespace zero diagnostics, 81 links checked/0 broken. Reused current builder target evidence per coordinator instruction; no redundant suite rerun. No remaining task-owned defect.'
        concurrency:
          applicable: false
          evidence: Entire candidate changes documentation and static CLI/MCP descriptions only. Repair changes only TESTS.md; no locks, transactions, scheduling, shared-state or task claim implementation changes.
        project_isolation:
          applicable: true
          evidence: Forge claim confirms exact requested 016 worktree root. All ten current blobs match candidate. No linked workspace/global installed skill writes or unrelated-state edits; external skill drift stays explicitly recorded as an environment limitation.
        administration:
          applicable: true
          evidence: Candidate retains accurate CLI-only milestone administration and caller-supplied verification recording. Standing ACDD authority for scoped task/archive work and approval boundaries for unrelated commits/push/publication preserved. Runtime receipt rewriting remains recorded as linked deferred work in message 125. No configuration/storage administration code changes, edits or review commits.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources), with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 3 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v2 to v3 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v3 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
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

### task: acdd-configurable-profiles-and-workflow-engine

```yaml
task_ref: acdd-configurable-profiles-and-workflow-engine
target: 'Implement fully arbitrary declarative workflow gates with closed proof taxonomy (contract, command, review, delivery, none, and scheme with recursive schema validation under proof: { scheme: ... }) driving bundles/receipts, persistent task.stage string ID storage, fail-closed validation on merged profiles, commands registry, hierarchical policies, nested contours dictionary (IndexMap) with criteria, multi-tier profile resolution (task.spec.acdd_profile, milestone.acdd_profile, forge-mcp.yaml, .forge/acdd/profile.yaml), explicit single-line acdd_profile: <path>:<sha256_prefix> pinning in contract specifications and SQLite metadata with TASK_PROFILE_TAMPERED validation, fail-closed TASK_STAGE_UNKNOWN diagnostics, scope protection for .forge/acdd/** with initial scope exception for dirty system files, field-wise role merging with role execution modes (mode: subagent | inline), reuse_on_reject policy returning to the same builder upon review rejection, ordered model specifications (models: Vec<ModelSpec>), rejection routing in workflow_guidance with targeted return-to-builder step, explicit sha_snapshot per gate in defaults (true for contract/build, false for review/deliver), zero runtime backward compatibility with one-time SQLite migration of legacy records (stages, evidence command_proof) exempting historical archive receipts, ignoring linked workspace profiles, review_sources rollup, delivery auto_commit saving created commit SHA in task delivery receipt from candidate snapshot tree with parent HEAD verifying candidate_baseline_head with hooks enforced and atomic rollback, milestone handoff rewriting receipt.commit of each task with its landed commit SHA in the milestone branch before archiving, and skill examples'
proof_policy: seam-test-first
contract_revision: 20
depends_on:
- milestone-handoff-contract-and-repo-verification-gates
invariants:
- 'INV-DECLARATIVE-ACDD-PROFILE-ROLE-MODEL-CYCLE: RoleDef contains mode: Option<RoleExecutionMode> with subagent and inline YAML values and effective default Subagent, reuse_on_reject: Option<bool> with effective default true, and ordered models: Vec<ModelSpec> defaulting empty, where models[0] is the primary default and ModelSpec owns model plus optional reasoning. RoleDef has no top-level model or reasoning fields. merge_profiles inherits mode and reuse_on_reject when overrides are None and inherits models when the override list is empty; a non-empty models override replaces the ordered list. Role recommendation continues to merge field-wise.'
- 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources), with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Roles define execution mode (mode: "subagent" | "inline"), rejection retention policy (reuse_on_reject: bool returning to the same builder upon review rejection), ordered model specifications (models: Vec<ModelSpec> preserving model and optional reasoning where the first model is the primary default), and field-wise role merging. When reject_to rewinds to a prior stage, workflow_guidance inspects task_gates and task_findings to deliver rejection: { rejected_from, worker_id, findings } with a targeted return-to-builder step ("Return task to builder ''{worker_id}'' with review rejection findings to repair defects.") when reuse_on_reject is enabled, returning null rejection on clean forward runs. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
scope:
- src/core/tasks/profile.rs
- src/core/tasks/acdd.default.yaml
- src/core/tasks/gates.rs
- src/core/tasks/mod.rs
- src/db/tasks_store.rs
- src/engine/tasks.rs
- src/engine/tasks/context.rs
- src/engine/tasks/workspaces.rs
- src/engine/milestones.rs
- src/cli/task.rs
- Cargo.toml
- Cargo.lock
- forge-mcp.yaml
- .forge/acdd/profile.yaml
- skills/contextunity-forge/references/acdd_profile.yaml.example
- skills/contextunity-forge/references/forge-mcp.yaml.example
- tests/acdd/tasks.rs
- tests/acdd/support.rs
- tests/acdd/mcp.rs
- tests/acdd/milestones.rs
- tests/acdd/blackboard.rs
- tests/acdd/subtasks.rs
- docs/reference/tasks.md
- docs/reference/configuration.md
subtasks:
- subtask_ref: author-profile-compiler-seam-tests
  title: 'Author seam tests in tests/acdd/tasks.rs verifying embedded profile defaults, override parsing, reject_to rewinds, arbitrary gate sequences, sha_snapshot behavior, fail-closed validation errors on merged profiles, proof.scheme recursive validation, multi-tier profile resolution (task/milestone overrides), delivery auto_commit and rollback with candidate_baseline_head consistency, milestone handoff task landed commit rewriting, acdd_profile: <path>:<sha256_prefix> pinning and tamper detection, archive receipt exemption, initial scope dirty system file exception, and ignoring linked workspace profiles'
  status: in_progress
  evidence: null
- subtask_ref: author-skill-profile-examples
  title: Author references/acdd_profile.yaml.example and references/forge-mcp.yaml.example demonstrating standalone profile configuration with sol-6.1 high reasoning reviewer role
  status: pending
  evidence: null
- subtask_ref: declarative-sha-snapshots-and-receipt-contour-validation
  title: Drive git commit snapshots strictly by gate_def.sha_snapshot, validate review contours in durable receipts against referenced profile.contours set, roll up reviews across review_sources, and execute delivery auto_commit directly from candidate snapshot tree of the nearest predecessor snapshot gate (e.g. build) with parent HEAD (verifying git rev-parse HEAD matches candidate_baseline_head), saving created commit SHA in task receipt, and executing milestone handoff to reconcile and rewrite receipt.commit of each task with its landed commit SHA in the milestone branch before archiving, with enforced hooks and atomic fail-closed rollback (leaving task uncompleted at deliver on failure)
  status: in_progress
  evidence: null
- subtask_ref: define-acdd-profile-yaml-schema-and-compiler
  title: 'Define GateProfile schema separating arbitrary gate id from closed proof taxonomy (contract, command, review, delivery, none, scheme); commands registry; hierarchical policies; explicit sha_snapshot per gate in acdd.default.yaml (true for contract/build, false for review/deliver); nested contours dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving order); proof: { scheme: ... } recursive structural schema validation for nested objects and typed arrays with exact path error reporting; multi-tier profile resolution (task.spec.acdd_profile > milestone.acdd_profile > forge-mcp.yaml > .forge/acdd/profile.yaml > default); explicit single-line acdd_profile: <path>:<sha256_prefix> pinning persisted in SQLite metadata with TASK_PROFILE_TAMPERED validation; fail-closed TASK_STAGE_UNKNOWN error on unknown task stage with actionable recommendation; exempting archive receipts (docs/milestones/archive/) from active validation; review_sources rollup; auto_commit option on proof: delivery; fail-closed validation on merged profile (unique IDs, strictly prior reject_to/independent_from, strictly prior review_sources with matching candidate SHA, existing roles/contours/commands, non-empty gates, exactly one terminal delivery); override inheritance (empty gates inherits, non-empty replaces whole sequence; field-wise role merge with mode, reuse_on_reject, and models: Vec<ModelSpec>); strictly ignoring linked workspace profiles'
  status: in_progress
  evidence: null
- subtask_ref: one-time-sqlite-storage-and-schema-migration
  title: Perform one-time SQLite data migration on schema v2 converting legacy gate stages (build/v1 -> build, deliver/v1 -> deliver, contract/v1 -> contract, review/v1 -> review) and evidence proof keys (test_proof -> command_proof) with zero runtime backward-compatibility shims, strictly without modifying blackboard topics
  status: pending
  evidence: null
- subtask_ref: proof-driven-context-bundles-and-workflow-guidance
  title: Refactor workspaces.rs and context.rs to key context bundles (ADRs, symbols, seam tests), reviewer independence, and workflow guidance off gate proof taxonomy (contract, command, review, delivery, none, scheme) rather than hardcoded stage names, delivering contour criteria to reviewers and rejection routing with targeted return-to-builder step upon reject_to rewind
  status: pending
  evidence: null
- subtask_ref: role-execution-modes-and-rejection-routing
  title: 'Define RoleDef mode: Option<RoleExecutionMode> with subagent and inline YAML values and effective default Subagent; reuse_on_reject: Option<bool> with effective default true; ordered models: Vec<ModelSpec> with empty-default inheritance, models[0] as primary, and model/reasoning stored only in ModelSpec; remove top-level model and reasoning; merge mode/reuse_on_reject from base when override is None, models from base when override is empty, and replace models with a non-empty override. Preserve field-wise recommendation merging and rejection routing in workflow_guidance. Prove defaults and merge behavior in tests/acdd/tasks.rs::declarative_profile_compiles_defaults_and_respects_role_overrides.'
  status: pending
  evidence: null
- subtask_ref: task-stage-string-persistence-and-cli-filter
  title: Refactor TasksStore and src/cli/task.rs so Task persists gate id string (task.stage) with runtime position resolution, halts claim on unknown gate IDs, and updates task list --stage to accept any active profile gate id
  status: in_progress
  evidence: null
- subtask_ref: transfer-contours-and-verify-detail-fidelity
  title: Convert contours in acdd.default.yaml into a nested dictionary under standard (contours.standard) with verbatim preservation of all 5 review contours (paths, claims, concurrency, project_isolation, administration) with complete operational criteria and descriptions
  status: pending
  evidence: null
- subtask_ref: update-architecture-and-runbook-documentation
  title: 'Update docs/reference/tasks.md and docs/reference/configuration.md to document the declarative profile schema, closed proof taxonomy including scheme validation, arbitrary gates lifecycle, sha_snapshot rules, delivery auto_commit, milestone handoff landed commit rewriting in task receipts, multi-tier resolution with acdd_profile: <path>:<sha256_prefix> pinning, scope protection for .forge/acdd/**, zero runtime backward compatibility, and configuration precedence'
  status: pending
  evidence: null
status: completed
receipt:
  contract_revision: 20
  passed_at: 2026-10-10T18:02:14.219433744+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 73
      tests_failed: 0
      log: 'cargo test --test acdd: exit 0; 73 passed, 0 failed. cargo clippy --all-targets --all-features -- -D warnings: exit 0.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Bounded repair delta from baseline 2a2583b to accepted candidate 9f31da2 changes only admitted src/engine/milestones.rs and tests/acdd/milestones.rs. Both current source blobs exactly equal snapshot blobs (SHA256 f108f403d781985bb5307caa727d2b6580e6c81361afb0b5278b2881491a2291 and ec5cefb621f45d4e30572b3bb97299d0de3c13fa7771706da0eb5afd766909fe). No hardcoded commit SHA or unrelated scope in repair. git diff --check scoped to both files is clean.
        claims:
          applicable: true
          evidence: 'INV-HANDOFF-RECORDING-CLARITY verified at milestones.rs:818-898: legacy fallback applies only when baseline absent and accepted candidate parentless, walks oldest-first first-parent history, requires exact git diff scoped tree equality and the landing itself to change scope relative to parent_fields[1]. Existing recorded-baseline branch retained; >=2 parents permits merges. Table-driven seam milestones.rs:863-1072 exercises direct and merge landings through actual engine completion, public TasksStore sync via tasks::manage Sync, verifies imported build_snapshot_candidate None, invokes actual CLI handoff, and checks archived landed receipt. Red artifact db71421b6f166ab5 contains 0 passed/1 failed/72 filtered and TASK_LANDED_COMMIT_NOT_FOUND; contract gate28 records exit101. Build gate29 records cargo test --test acdd exit0,73 passed,0 failed and strict Clippy exit0.'
        concurrency:
          applicable: true
          evidence: Repair changes read-only Git landing resolution, leaving existing handoff receipt validation, database operations, rollback and claim ownership boundaries intact. Lookup reads commit objects in owning repository and yields no partial receipt state; all tasks resolve before receipt persistence. Review claim30 is independent of builder-handoff-parentless-landing-20261010-luna and binds accepted candidate9f31da2.
        project_isolation:
          applicable: true
          evidence: All added Git commands use supplied owning workspace root via current_dir(root) and declared task scope pathspecs. Repair creates no new path resolution, symlink, traversal, linked profile or task namespace policy. Public seam uses ScopedWorkspace and independent SQLite database imported through production Sync; no external workspace references.
        administration:
          applicable: true
          evidence: 'Parentless legacy candidate lookup is generic and does not alter receipt schema, migration, profile pins, hook execution or delivery baseline fencing. Existing recorded-baseline path remains authoritative when evidence exists. Merge lookup compares first parent rather than excluding merge commits; no-baseline lookup fails closed with TASK_LANDED_COMMIT_NOT_FOUND if exact scope equality plus scoped landing change has no match. Reviewed formatting archives: repo-wide differences are existing; targeted output predates normalization of new conditions/archive expression, which current candidate already matches. Remaining targeted formatter differences are outside changed hunks. No reviewer source edits.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources) plus the milestone receipt, with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 5 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v4 to v5 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v5 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
    - 'INV-DECLARATIVE-ACDD-PROFILE-ROLE-MODEL-CYCLE: RoleDef contains mode: Option<RoleExecutionMode> with subagent and inline YAML values and effective default Subagent, reuse_on_reject: Option<bool> with effective default true, and ordered models: Vec<ModelSpec> defaulting empty, where models[0] is the primary default and ModelSpec owns model plus optional reasoning. RoleDef has no top-level model or reasoning fields. merge_profiles inherits mode and reuse_on_reject when overrides are None and inherits models when the override list is empty; a non-empty models override replaces the ordered list. Role recommendation continues to merge field-wise.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources), with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Roles define execution mode (mode: "subagent" | "inline"), rejection retention policy (reuse_on_reject: bool returning to the same builder upon review rejection), ordered model specifications (models: Vec<ModelSpec> preserving model and optional reasoning where the first model is the primary default), and field-wise role merging. When reject_to rewinds to a prior stage, workflow_guidance inspects task_gates and task_findings to deliver rejection: { rejected_from, worker_id, findings } with a targeted return-to-builder step ("Return task to builder ''{worker_id}'' with review rejection findings to repair defects.") when reuse_on_reject is enabled, returning null rejection on clean forward runs. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
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

### task: task-blackboard-targeted-routing-and-topics
```yaml
task_ref: task-blackboard-targeted-routing-and-topics
target: Enhance task_blackboard with optional gate targeting, task-store schema version 5 migration, legacy topic mapping, canonical single-word topics, compact blackboard_info summary, actionable schema hints, and durable receipt rollup
proof_policy: seam-test-first
contract_revision: 19
depends_on:
- acdd-configurable-profiles-and-workflow-engine
invariants:
- 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in task-store schema version 5 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v4 to v5 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v5 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
scope:
- src/db/tasks_store.rs
- src/engine/tasks.rs
- src/engine/tasks/context.rs
- src/core/tasks/gates.rs
- src/cli/guide.rs
- src/mcp/tools.rs
- skills/contextunity-forge/SKILL.md
- tests/acdd/blackboard.rs
- tests/acdd/tasks.rs
- docs/reference/tasks.md
status: completed
subtasks:
- subtask_ref: migrate-blackboard-schema-v3-and-gate-column
  title: 'Migrate task_blackboard from task-store schema version 4 to version 5 adding optional gate TEXT column with indexes, compound queries in TasksStore, and explicit one-time SQLite migration mapping legacy topics (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) to canonical topics with zero runtime backward-compatibility shims (seam test: tests/acdd/tasks.rs::task_blackboard_gate_targeting_and_schema_hint, breaking mutation: reject gate column query)'
  status: pending
  evidence: null
- subtask_ref: extend-blackboard-request-with-gate-targeting
  title: Support optional gate parameter in BlackboardRequest for post and read actions with validation against active profile gates
  status: pending
  evidence: null
- subtask_ref: adopt-canonical-single-word-topics-and-receipt-rollup
  title: Standardize the 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) and map decisions and deferred to durable receipt rollup in gates.rs
  status: pending
  evidence: null
- subtask_ref: provide-actionable-schema-hints-and-workflow-gates
  title: Return rich schema hints with canonical topic descriptions, active profile gates, and example JSON when blackboard validation fails
  status: pending
  evidence: null
- subtask_ref: auto-inject-targeted-messages-in-claim-and-guidance
  title: Automatically deliver targeted gate messages in workflow_guidance.blackboard_messages and blackboard_info during gate claim and context retrieval
  status: pending
  evidence: null
- subtask_ref: align-skill-and-guide-blackboard-spec
  title: Update skills/contextunity-forge/SKILL.md, guide.rs, and docs/reference/tasks.md with canonical topics, gate targeting, and examples
  status: pending
  evidence: null
- subtask_ref: add-blackboard-routing-and-validation-seam-tests
  title: Author comprehensive seam tests in tests/acdd/tasks.rs verifying gate targeting, canonical topics, legacy topic migration, error hints, and durable receipt rollup
  status: pending
  evidence: null
receipt:
  contract_revision: 19
  passed_at: 2026-10-10T15:37:42.433807623+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 72
      tests_failed: 0
      log: '72 passed, 0 failed. Focused: cargo test --test acdd task_blackboard_gate_targeting_and_schema_hint -- --nocapture (1 passed); v1 migration valid + rollback (2 passed); milestone profile pin routing/tampering (1 passed). cargo clippy --all-targets --all-features -- -D warnings: exit 0. git diff --check: exit 0. Targeted rustfmt --edition 2021 --check --config skip_children=true on 12 Task6 Rust paths: exit 1 with 122 pre-existing formatter diffs in dirty files; newly added migration/profile blocks are formatted and tests/acdd/blackboard.rs passes rustfmt --check. No known functional failures.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All 14 admitted source paths match candidate ae55bac; 13 changed versus baseline. Repair delta is limited to tasks_store.rs, engine/tasks.rs, and blackboard tests. Task5 receipt remains byte-identical. Scope evidence: /tmp/task6-r2-review-source-hashes.json.'
        claims:
          applicable: true
          evidence: Fresh independent review claim 8, contract 19, worker task6-review-sol61. Required ACDD gate passed 72/72. R1 and R2 independently confirmed fixed using public CLI seams; closure ledger /tmp/task6-r2-review-closure-ledger.json. Canonical-only runtime topics, gate targeting, claim/context injection, actionable hints, and labeled decisions/deferred in existing architectural_notes are verified.
        concurrency:
          applicable: true
          evidence: Immediate migration transactions preserve rollback for malformed, orphan, absolute, and traversal rows. Existing posting/completion fences and delivery transaction boundaries remain intact; concurrency and delivery rollback coverage passes in the 72-test ACDD target.
        project_isolation:
          applicable: true
          evidence: 'Schema-v1 migration qualifies each preserved row using its owning task namespace; shared-project public reads and inspect preserve metadata. Milestone effective pinned profile resolves in its owning confined root, task overrides prevail, default/nonmember gates reject, tampered hashes and external symlinks fail closed. Public CLI proofs: /tmp/task6-r2-review-v1-public-proof.json and /tmp/task6-r2-review-pin-public-proof.json.'
        administration:
          applicable: true
          evidence: Full migration chain reaches schema 5 with one-time legacy-topic mapping and no runtime compatibility fallback. Existing ReceiptRollup representation and historical Task5 receipt remain compatible without new fields/defaults. Documentation, skill, CLI guidance, and tests align. Builder strict Clippy and diff check passed; 122 scoped rustfmt differences are pre-existing. No new blocking findings.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-COMPLETED-SCOPE-RELEASE: Completed milestone tasks release their exclusive file locks in SQLite: the status != "completed" filter applies strictly to lock conflict detection during task claim and extend_scope, preserving stored scope path history while freeing file claims for successor tasks to extend into and evolve shared documentation and code paths without false scope conflicts.'
    - 'INV-REPO-AGNOSTIC-GUIDANCE: Task guidance gap detection verifies skill presence, repository instruction compliance, and exact canonical .gitignore compliance (tracking .forge/acdd/** including .forge/acdd/profile.yaml and .forge/frameworks/** extensions while ignoring runtime SQLite databases, locks, and logs) without imposing tool-name restrictions or hardcoded internal URLs on external consumer repositories.'
    - 'INV-SKILL-TOOL-REFERENCE-DELEGATION: The contextunity-forge skill serves as the generic tool reference and standing Git permission baseline, delegating workflow gate governance, review contours, and role policies to the active task profile. Git standing permissions explicitly distinguish auto_commit: true (Forge creates atomic commit on deliver) from auto_commit: false (agent commits after deliver) and final milestone archive commits.'
    - 'INV-HANDOFF-RECORDING-CLARITY: Milestone handoff is CLI-only and records caller-verified test results into durable receipts without conflating execution with persistence: upon verifying that all milestone tasks are completed and verification tests pass, handoff resolves the landed commit for each task in the milestone branch (the commit that landed the task''s work into the milestone branch, whether via fast-forward or merge commit), rewrites receipt.commit of each task in the milestone document and SQLite store to that landed commit SHA, records the overall milestone handoff.commit (HEAD at handoff) in frontmatter, and archives the document; the subsequent milestone archive commit occurs after handoff and is excluded from receipt.commit. Exact verification commands and counts are codified in TESTS.md.'
    - 'INV-DECLARATIVE-ACDD-PROFILE: The declarative YAML profile engine (GateProfile with embedded acdd.default.yaml via include_str!) strictly separates arbitrary gate id (workspace tokens defaulting to contract, build, review, deliver), closed proof taxonomy (contract, command, review, delivery, none, or scheme with recursive data schema validation driving context bundles, independent review, and durable receipts), gate fields (explicit sha_snapshot: bool per gate in defaults, reject_to, independent_from, review_sources, role, steps, tools, contours set reference, and proof: { scheme: ... } structural schema definition for scheme proof), command registry (commands mapping to executable shell commands), and hierarchical policies. Default 4 gates in acdd.default.yaml map strictly to proof: contract, command, review, and delivery; sha_snapshot is explicitly true for contract/build and false for review/deliver. contours is a nested dictionary (IndexMap<String, IndexMap<String, ContourDef>> preserving definition order) where "standard" preserves the canonical 5 contours (paths, claims, concurrency, project_isolation, administration) with operational criteria and descriptions; gates reference contour sets via gate.contours (defaulting to "standard"). Delivery gates (proof: delivery) support auto_commit: bool (default true), creating an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with sha_snapshot: true (which is build in the default profile, whose candidate tree SHA is verified across all review_sources) plus the milestone receipt, with parent HEAD, verifying git rev-parse HEAD matches candidate_baseline_head (the exact commit SHA of HEAD captured when the candidate snapshot was created), standard commit messages derived from task targets and completed subtasks, and enforced hooks; the created commit SHA is persisted into the task''s delivery receipt in SQLite. If git commit fails, SQLite receipt recording and task completion roll back atomically, leaving the task at deliver with an explicit error ready for reject rewind or retry. Upon milestone handoff, Forge resolves each task''s landed commit in the milestone branch and rewrites receipt.commit to that landed SHA before archiving the document (the archive commit is created after handoff and is excluded from receipt.commit). Delivery rolls up reviews across review_sources (strictly prior gates with proof: review verifying the identical candidate SHA). Profile resolution precedence: task.spec.acdd_profile over milestone.acdd_profile over forge-mcp.yaml path link over .forge/acdd/profile.yaml over embedded defaults. Single-line profile pinning format in milestone frontmatter or task spec (acdd_profile: "<path>:<sha256_prefix>" with at least 7 hex characters) is persisted into SQLite task metadata during task sync; Forge verifies the file hash prefix matches, failing closed with TASK_PROFILE_TAMPERED on mismatch. Task operations validating gate identifiers fail closed with TASK_STAGE_UNKNOWN if a task stage is missing from the active profile gates, providing actionable recommendations. Zero runtime backward-compatibility is maintained strictly across SQLite runtime data and active milestones: historical archive receipts in docs/milestones/archive/ are exempt from active profile validation. Protected system paths in .forge/acdd/** cannot be added via extend_scope (TASK_SCOPE_PROTECTED) and dirty system files fail closed with TASK_SCOPE_VIOLATION unless explicitly admitted in the active task initial planning-time task.spec.scope. Linked workspace profiles are strictly ignored (only the active workspace root defines the ACDD profile). Profile compilation evaluates the merged profile and fails closed if gates is empty, delivery is not exactly one or not terminal, IDs repeat, reject_to or independent_from do not point to strictly prior gates, review_sources do not point to strictly prior review gates, or roles/contours/commands are undefined. Task state persists task.stage string ID where position is computed at runtime and unknown IDs stop claim. task list --stage accepts any active profile gate id.'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 5 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v4 to v5 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v5 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
    - 'INV-BLACKBOARD-GATE-ROUTING: task_blackboard in task-store schema version 5 includes an optional gate TEXT column with query indexes, supports 6 canonical single-word topics (draft, notes, findings, blockers, decisions, deferred) with schema v4 to v5 migration and explicit legacy topic mapping (contract_draft -> draft, contract_findings -> findings, build_proof -> notes, architectural_notes -> decisions; canonical topics preserved unchanged; unknown -> notes) occurring strictly in the schema v5 migration, returns compact blackboard_info summaries and actionable schema hints on error, delivers targeted messages on claim, and rolls up decisions and deferred into durable receipts.'
    architectural_notes:
    - |-
      decisions: Contract boundary check for Task 6 (contract revision 18; local claim revision 2):

      Verified:
      - The live store is already schema v4: src/db/tasks_store.rs:391-427 migrates v3 to v4 for digest_version; open_project accepts v4 without a follow-on migration at lines 450-466, fresh stores start at v4 at line 477, and the task_blackboard table at line 486 has no gate column. Task 6 must therefore add the gate column and topic mapping in v4 to v5.
      - Task 5, acdd-configurable-profiles-and-workflow-engine, is completed at contract revision 20. Its receipt records the v3-to-v4 digest_version marker migration (line 503) and its durable verified_invariants include the earlier blackboard text (line 512). Preserve that completed receipt byte-for-byte.
      - Task 6 already owns src/db/tasks_store.rs and the blackboard engine, context, gates, CLI/MCP, tests, skill, and task reference paths. No scope change is needed.
      - Task 7, acdd-nomenclature-rename-augmented-contract-driven-development, depends on Task 6 but has an independent nomenclature target and revision 18. Leave its contract and dependency unchanged.

      Proposed minimal diff to docs/milestones/016-acdd-declarative-profiles-and-workflow-governance.md:

      ```diff
      @@ frontmatter invariants
      - INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 3 ... schema v2 to v3 migration ... occurring strictly in the schema v3 migration ...
      + INV-BLACKBOARD-GATE-ROUTING: task_blackboard in SQLite schema version 5 ... schema v4 to v5 migration ... occurring strictly in the schema v5 migration ...
      @@ outcome bullet 7
      - Upgrade the task-store schema from version 3 to version 4 for blackboard gate targeting, ...
      + Upgrade the task-store schema from version 4 to version 5 for blackboard gate targeting, ...
      @@ Task 6
      - target: "... task-store schema version 4 migration ..."
      + target: "... task-store schema version 5 migration ..."
      - contract_revision: 18
      + contract_revision: 19
      @@ Task 6 invariant
      - task_blackboard in task-store schema version 4 ... schema v3 to v4 migration ... schema v4 migration ...
      + task_blackboard in task-store schema version 5 ... schema v4 to v5 migration ... schema v5 migration ...
      @@ first Task 6 subtask title
      - Migrate task_blackboard from task-store schema version 3 to version 4 adding optional gate TEXT ...
      + Migrate task_blackboard from task-store schema version 4 to version 5 adding optional gate TEXT ...
      ```

      Keep the first subtask_ref stable to retain operational identity; preserve its named seam tests/acdd/tasks.rs::task_blackboard_gate_targeting_and_schema_hint and breaking mutation: reject gate column query. Also leave all completed receipts, including Task 5 at lines 475-525, and the entire Task 7 block at lines 573-613 unchanged. The active milestone objective bullet 7 and frontmatter invariant are the additional authority text that must change along with Task 6 target, revision, invariant, and first subtask title. No code, test, scope, or migration mechanism is proposed here.
    - |-
      decisions: Task 6 contract amendment proposal v2 — contract-only, no repository mutation

      Proposed milestone diff:
      - Frontmatter blackboard invariant: change the active blackboard schema target from v3 with migration v2→v3 to schema v5 with migration v4→v5.
      - Active milestone outcome #7: change task-store schema v3→v4 to v4→v5.
      - Task 6 target: v4→v5.
      - Task 6 contract_revision: 18→19.
      - Task 6 INV-BLACKBOARD-GATE-ROUTING: schema v5 with v4→v5 migration.
      - First Task 6 subtask title: schema v3→v4 becomes v4→v5. Keep its existing subtask_ref, named seam test tests/acdd/tasks.rs::task_blackboard_gate_targeting_and_schema_hint, and breaking mutation “reject gate column query” unchanged.
      - Task 7 contract_revision: 18→19 solely to acknowledge the changed inherited milestone invariant. Keep its target, scope, proof, dependency, and subtasks unchanged.

      Authority basis: Milestone::digest serializes self.invariants into each digest form in src/core/tasks/mod.rs. sync_selected accepts an unchanged digest, but when the digest changes it rejects completed specs or a contract_revision that is not greater than the stored revision with AUTHORITY_GAP. Therefore, changing the parent invariant changes Task 7 digest too; leaving Task 7 at revision 18 would block its bounded sync/claim. Task 5 is completed, so preserve its historical receipt and do not re-sync it after the parent invariant changes.

      After the milestone amendment is approved, sync only Task 6 and Task 7 by bounded task-specific operations; do not run broad milestone sync. Preserve Task 5 receipt exactly. Task 6 schema stays version 5 because schema v4 already exists for digest_version. No migration history or new mechanisms are proposed. No source or test edits have been made.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: acdd-nomenclature-rename-augmented-contract-driven-development

```yaml
task_ref: acdd-nomenclature-rename-augmented-contract-driven-development
target: "Consistently rename all active occurrences of 'admitted contract' to 'augmented contract' and verify clean default gate identifiers without /v1 suffixes where stage arguments validate against active profile across active codebase, documentation, tests, CLI, and skills without altering archive milestones or historical ADRs"
proof_policy: direct-proof
contract_revision: 20
depends_on:
  - task-blackboard-targeted-routing-and-topics
invariants:
  - 'INV-AUGMENTED-NOMENCLATURE: Nomenclature consistently uses "augmented contract" and "augmented contract-driven development", and standard gate identifiers are clean tokens without version suffixes (contract, build, review, deliver).'
scope:
  - docs/runbooks/
  - docs/reference/
  - skills/
  - AGENTS.md
  - README.md
scope_roots:
  - src/
  - docs/runbooks/
  - docs/reference/
  - skills/
  - tests/
  - AGENTS.md
  - README.md
status: ready
subtasks:
  - subtask_ref: rename-nomenclature-in-documentation-and-agents
    title: "Update AGENTS.md, README.md, docs/runbooks/, docs/reference/, and skills/ terminology from admitted contract to augmented contract without modifying historical ADRs or archived milestones"
    status: pending
  - subtask_ref: verify-clean-tokens-across-models-and-cli
    title: "Verify default clean gate identifiers (contract, build, review, deliver) without /v1 suffix across core models, queries, and verify CLI and MCP stage arguments validate dynamically against active profile"
    status: pending
  - subtask_ref: rename-nomenclature-in-source-code-and-comments
    title: "Audit active source models, CLI messages, and doc comments for legacy admitted-contract terminology and /v1 gate tokens; use bounded scope extensions only for verified matches, then rename terms and validate clean dynamic gate IDs"
    status: pending
  - subtask_ref: update-test-identifiers-and-assertions
    title: "Align test suites and error assertion texts with augmented contract nomenclature and clean gate tokens"
    status: pending
```
