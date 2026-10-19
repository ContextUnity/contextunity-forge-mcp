---
id: m-admission-integrity-database-guards-and-lifecycle-hardening
title: Admission integrity, database collision protection, and MCP lifecycle hardening
doc_type: contract
status: active
depends_on:
- m-declarative-framework-packs-and-cold-build-pipeline
owners:
- src/engine/scanner/
- src/engine/languages/
- src/engine/tasks/
- src/engine/milestones/
- src/engine/pipeline.rs
- src/db/
- src/mcp/
- src/cli/
- src/core/
invariants:
- 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
- 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
- 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
- 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
- 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
- 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-19T14:47:39+00:00
---

# Admission integrity, database collision protection, and MCP lifecycle hardening

## Outcome and purpose

Harden ContextUnity Forge MCP against runtime admission thrashing, cross-domain database overwrites, phantom workspace duplicates, and daemon process leaks:

1. **Manifest Discovery Symmetry & Admission Loop Elimination:**
   Unify dependency manifest collection across cold build (`writer::build`), incremental delta (`writer::delta`), and MCP server admission (`Server::admit`). Retain scanner-discovered manifests in `ScanReport` and eliminate divergent full-disk walks, ensuring identical manifest digests and preserving warm admission performance without triggering repeated rebuild bail-outs.

2. **Cross-Domain Database Collision & Overwrite Guard:**
   Enforce strict isolation between task tracking (`tasks.sqlite`) and code-map index (`code-map.sqlite`) databases. Validate targets across CLI (`build`, `serve`, `open_or_rebuild`), `atomic_build`, and `TasksStore::open_project` using filename patterns, configured `tasks_db` paths, and SQLite schema table introspection to prevent fatal data loss.

3. **Subproject Workspace Deduplication:**
   Align milestone directory scanning in `Registry::load` with declared milestone frontmatter (`project: <name>`) and configured `owners`/`aliases` from `forge-mcp.yaml`. Prevent simultaneous path-inferred and declared-frontmatter registration from spawning duplicate phantom workspaces (e.g. `contextunity/extensions.base` alongside `contextunity/base`).

4. **MCP Parent Lifecycle & Heap Compaction:**
   Preserve connected clients across inactivity and SIGHUP reload. Remove idle-based shutdown and its configuration entirely. On Unix, monitor the launching parent during initialization and connected operation; shut down when that parent disappears. Retain every allocator trim from milestone 031 and avoid duplicate trimming within one MCP dispatch.

5. **Greenfield Seam Scaffolding Guidance:**
    Enhance `validate_seam_reachability` to detect uncreated scope files on greenfield features and return actionable diagnostics directing the contract author to scaffold minimal stubs and refresh the index. Proof policy selection never bypasses seam reachability.

---

## Tasks in this milestone

### task: manifest-discovery-symmetry-and-admission-loop-elimination

```yaml
task_ref: manifest-discovery-symmetry-and-admission-loop-elimination
target: Unify dependency manifest collection between cold build and MCP admission to eliminate digest divergence and prevent repeated rebuild bail-outs
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/engine/scanner/mod.rs
- src/engine/languages/dependency_registry.rs
- src/mcp/server.rs
- src/engine/pipeline.rs
- tests/mcp/freshness/freshness_and_admission.rs
status: completed
subtasks:
- subtask_ref: scan-report-manifest-retention
  title: Retain dependency_manifests vector in ScanReport from collect_candidates in scan_reusing without discarding
  status: completed
  evidence: null
- subtask_ref: registry-scanner-manifest-unification
  title: Reuse scanner::dependency_manifests_for_build for manifest-only discovery; preserve streaming cold-build collection
  status: completed
  evidence: null
- subtask_ref: server-admission-and-delta-manifest-wiring
  title: Pass scan.dependency_manifests to try_collect_with_paths in Server::admit and delta, ensuring identical digest calculation with writer::build
  status: completed
  evidence: null
receipt:
  contract_revision: 4
  passed_at: 2026-10-19T15:10:10.443822227+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test mcp
      exit_code: 0
      tests_passed: 171
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All 5 changed tracked files match admitted scope and build snapshot 235a414; git diff --check passed. Parent-owned untracked milestone and two planning reports are excluded from task source delivery. No reviewer source edits. Detailed inventory and hashes: /tmp/forge-049-implementation-20261019/task1-independent-review.md and task1-review-source-sha256.txt.'
        claims:
          applicable: true
          evidence: 'Cold build keeps scanner manifest discovery and bounded streaming; scan report retains paths; admission and delta reuse them through fallible try_collect_with_paths. Direct root and linked-root manifest rules, typed framework errors, missing digest rebuild and unchanged external manifest behavior verified. Independent cargo clippy --all-targets --all-features -- -D warnings exit 0; cargo test --lib exit 0, 85 passed/0 failed; cargo test --test mcp exit 0, 86 passed/0 failed. Exact logs and commands: /tmp/forge-049-implementation-20261019/task1-review-gates.json. Milestone paired performance acceptance remains pending; this is task 1 only.'
        concurrency:
          applicable: true
          evidence: Source inspection confirms reader slot is cleared before build/delta; delta RAII locks drop before SourceSnapshotMismatch reaches bounded retry/backoff. No generation lock is retained during sleep. Database/adapter identity fencing, transaction cleanup, full rebuild lock release, and terminal fail-closed exhaustion remain. Complete MCP target passes. See /tmp/forge-049-implementation-20261019/task1-independent-review.md.
        project_isolation:
          applicable: true
          evidence: Explicit-root Forge graph matches assigned worktree (generation c65435f8, 312 files, 7007 nodes, 0 parse errors). Scanner root, symlink, ignored-name, same-filesystem, checked_child and linked-prefix boundaries remain shared across paths; no task/worktree resolution changes. Tests use isolated real temporary workspaces and an isolated skill directory.
        administration:
          applicable: true
          evidence: No schema, migration, task profile, settings or privileged configuration changes. Candidate is bound to base 9f358ecc and scoped snapshot 235a414. Reviewer will use Forge automatic delivery with generated receipt only; no manual task/main commit or receipt edit. Existing streaming and allocator trim code is preserved; full milestone performance and later-task acceptance remain outstanding.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
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

### task: database-collision-and-overwrite-guard

```yaml
task_ref: database-collision-and-overwrite-guard
target: Protect tasks.sqlite and code-map.sqlite from mutual overwrite and corruption across CLI commands, MCP server startup, and cold build execution
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/db/writer.rs
- src/engine/pipeline.rs
- src/core/fs.rs
- src/engine/tasks/mod.rs
- src/db/tasks_store/mod.rs
- src/cli/mod.rs
- tests/core/database_guard.rs
- tests/core.rs
- docs/reference/configuration.md
status: completed
subtasks:
- subtask_ref: validate-code-map-target-guard
  title: Reject code-map build/serve output targets matching tasks.sqlite, matching configured tasks_db, or containing task tracking schema tables
  status: completed
  evidence: null
- subtask_ref: tasks-store-code-map-guard
  title: Reject tasks store project initialization on databases named code-map.sqlite or containing code map tables (nodes, edges, files)
  status: completed
  evidence: null
- subtask_ref: cli-command-boundary-validation
  title: Validate code-map output target in open_or_rebuild, serve, and build subcommands before acquiring locks or creating files
  status: completed
  evidence: null
receipt:
  contract_revision: 5
  passed_at: 2026-10-19T16:11:53.555040200+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test core
      exit_code: 0
      tests_passed: 126
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All 9 scope paths implement collision protection, its real proof or matching configuration documentation. Tracked files match candidate49e381; new database_guard.rs bytes also match via git show/cmp. git diff --check passed. Manifest changes are admitted authority; 2 unrelated untracked planning reports remain excluded. No reviewer source edits; source hashes retained in H/task2-review-source-sha256.txt, where H=/tmp/forge-049-implementation-20261019.
        claims:
          applicable: true
          evidence: Verified canonical target guard at public build/delta, atomic build, final locked publication and 3 CLI boundaries; reserved names/custom paths/task schema/invalid settings/locked SQLite/nonregular targets fail closed. Task opener rejects code-map names/schema. Recovered invalid-configuration and initialization race defects are closed. Independently ran strict all-target/all-feature Clippy exit0, cargo test --lib 85/0, cargo test --test core 41/0 +1 existing ignored, 0 filtered. Exact logs H/task2-review-gates.json. Full049 paired performance acceptance and tasks3–6 remain pending.
        concurrency:
          applicable: true
          evidence: Unknown-store initializer closes its old SQLite handle, takes canonical generation lock, reopens/rechecks, and holds lock through initialization. Publisher revalidates under same lock before sidecar removal/rename. Real separate child-process direct/alias race passes and preserves published index bytes; initialized-store opener still succeeds during writer transaction. Unix conditional leaves existing non-Unix opener available (source checked).
        project_isolation:
          applicable: true
          evidence: Actual MCP stdio server overview confirms assigned WT generation d34ad12f,313 files/7018 nodes/0 parse errors. Paths are normalized/canonicalized for configured DB separation and canonical initialization locking; code publication retains symlink/parent guards. MCP impact/seam plus validated CLI AST evidence retained H/task2-review-{guard-callers,opener-impact,seam,cli-ast}.json. Tests and skill copy are isolated.
        administration:
          applicable: true
          evidence: No migration/schema version/profile/privilege changes. Known task stores retain main connection-local open; config docs match runtime. Candidate49e381 binds to delivery base56eb0cd. Forge owns scoped automatic commit/receipt; reviewer makes no manual commit or receipt edit. Existing streaming remains; no milestone performance completion claim. Full contour rationale H/task2-independent-review.md.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
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

### task: subproject-workspace-deduplication

```yaml
task_ref: subproject-workspace-deduplication
target: Eliminate duplicate phantom workspaces generated by simultaneous path inferencing and declared frontmatter scanning across extension milestone directories
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/engine/tasks/workspaces.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/lifecycle.rs
- src/engine/tasks/blackboard.rs
- src/engine/tasks/gates.rs
- src/engine/milestones/contract.rs
- src/engine/milestones/mod.rs
- src/core/tasks/mod.rs
- src/db/tasks_store/mod.rs
- docs/reference/tasks.md
- docs/reference/configuration.md
- tests/acdd/milestones.rs
status: completed
subtasks:
- subtask_ref: settings-owners-and-aliases-wiring
  title: Add owners and aliases configuration mappings to TaskSettings parsed from forge-mcp.yaml
  status: completed
  evidence: null
- subtask_ref: subproject-milestones-precedence
  title: Prioritize declared_projects and configured owners/aliases over path-based inferencing in Registry::load, eliminating phantom duplicate workspaces
  status: completed
  evidence: null
- subtask_ref: ambiguous-milestone-regression-test
  title: Prove subproject milestone sync, list, claim, and delivery preserve one canonical project identity in tests/acdd/milestones.rs
  status: completed
  evidence: null
receipt:
  contract_revision: 5
  passed_at: 2026-10-19T17:01:02.592705271+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test acdd
      exit_code: 0
      tests_passed: 192
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All 12 admitted paths exactly match build candidate 02d571e against base a6e379e; git diff --check passed. 690 insertions/95 deletions implement identity resolution, real proof and matching docs. Untracked successor plan excluded. No reviewer source edits. Hashes H/task3-review-source-sha256.txt, with H=/tmp/forge-049-implementation-20261019.
        claims:
          applicable: true
          evidence: Verified declared project > longest component owner > path inference > raw default, then exactly one alias; raw default/maps reach authority and receipts. README/empty inference, default-alias drift and mixed-directory duplicate-location findings are closed with true runtime reds and full lifecycle/init/handoff proof. Independent strict all-target/all-feature Clippy exit 0; complete lib 85/0 and acdd 107/0, zero ignored/filtered, 192 actual passes. H/task3-review-gates.json and task3-independent-review.md. Full 049 paired performance remains unaccepted.
        concurrency:
          applicable: true
          evidence: Registry configuration maps remain immutable within an operation and propagate into receipt stores; no second config read in linked delivery. Snapshot batches load settings once; public root store keeps one settings load/direct opener. No new locks, schema, workers or index walks; existing transaction, digest, candidate and worktree checks remain. Complete ACDD concurrency/authority tests pass.
        project_isolation:
          applicable: true
          evidence: 'Actual worktree-rooted MCP generation ba8a47a8: 313 files/7038 nodes/0 parse errors. Shared snapshot production seam has direct test edge; source and runtime checks supplement known Rust graph gaps. Resolution dedups only identical canonical-root/relative-path pairs; distinct linked roots retain ambiguity. Complete existing linked no-config MCP/CLI lifecycle and scope isolation pass. H/task3-review-*-final.json.'
        administration:
          applicable: true
          evidence: No schema migration/profile/privilege/install change. in_progress remains an active milestone synonym and is covered end to end. Legacy migration consumer semantics remain unchanged/outside scope. Scoped snapshot 02d571e binds to base a6e379e; Forge will create one automatic task commit/receipt. No manual commit or receipt edit. Tasks 4–6 and full milestone performance/landing remain outstanding.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    architectural_notes:
    - 'decisions: Finding #407 is an introduced regression in the active task-3 receipt parser. Reachable scenario: primary configuration registers a linked directory without its own configuration; sync/claim/build/review succeed, delivery fails. Governing invariant: one canonical identity from registry through receipt, while preserving existing linked-workspace behavior. Closure map: Registry::load validates primary configuration and owner/alias maps -> Registry-owned store setup propagates those already-loaded maps to TasksStore -> shared parse_milestone_manifest resolves receipt identity -> task-id/digest checks remain mandatory -> receipt persistence. Invalid primary or linked configuration continues to fail during registry resolution. Do not add a missing-config catch-all or reread a different workspace configuration during delivery. Regression evidence: the existing complete linked MCP/CLI lifecycle test fails in task3-build-gate-2.log; companion owner/alias full lifecycle cases cover canonical receipt identity. Verify both equivalent routes (engine submit and public store_for_task), then run full lib/acdd and strict Clippy. No new persistence schema or global cache.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: mcp-idle-timeout-and-memory-compaction

```yaml
task_ref: mcp-idle-timeout-and-memory-compaction
target: Preserve idle MCP connections, terminate servers after parent loss, and retain indexing memory reclamation without duplicate MCP trimming
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/mcp/server.rs
- tests/common/mcp_client.rs
- tests/mcp/freshness/stdio_and_response.rs
- docs/reference/mcp-setup.md
- docs/architecture/performance.md
status: completed
subtasks:
- subtask_ref: glibc-malloc-trim-binding
  title: Preserve all ten main allocator trims at existing indexing batch boundaries without adding an unused wrapper
  status: completed
  evidence: null
- subtask_ref: post-tool-memory-trimming
  title: Exclude redundant per-request and duplicate indexing-end trims; prove main trimming sites are preserved and require final paired performance acceptance
  status: completed
  evidence: null
- subtask_ref: mcp-parent-lifecycle
  title: Remove inactivity shutdown; prove connected server exit after parent loss with transport held open, and preserve SIGHUP session continuity
  status: completed
  evidence: null
receipt:
  contract_revision: 5
  passed_at: 2026-10-19T17:39:30.564757828+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test mcp
      exit_code: 0
      tests_passed: 174
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All five admitted files byte-match frozen snapshot 398f724 against base 22e64ad; git diff --check passed. Only additional tracked path is the admitted task-4 contract revision/scope reduction in the milestone manifest; untracked successor plan excluded. No reviewer source edits. H/task4-review-binding-final.json and task4-review-source-sha256.txt, H=/tmp/forge-049-implementation-20261019.
        claims:
          applicable: true
          evidence: 'T4-R1 repaired and independently reverified: original parent identity survives SIGHUP exec; both held-open-input control and replacement/reparent race cases exit in final external reproduction, original red retained. Complete independent strict Clippy exit 0; lib 86/0 and mcp 88/0, zero ignored/filtered, 174 actual passes. Real pending/connected parent-loss, reload continuity and inactivity cases pass. H/task4-review-gates.json, task4-review-parent-reload-final.json and task4-independent-review.md. Full049 performance remains unaccepted.'
        concurrency:
          applicable: true
          evidence: One bounded two-second asynchronous monitor surrounds initialization and connected serve_session, with original PID retained across exec. Private reload value is consumed/removed before session creation and set only on exec Command; failed exec cannot mutate live environment with it. Process exit on parent loss avoids Tokio blocking-stdin shutdown hang. No new DB locks/workers/index walks. All ten allocator trims retained; five containing files byte-identical to task3 base, no per-request or duplicate-end trims.
        project_isolation:
          applicable: true
          evidence: Actual W-rooted MCP generation 24745d04, 313 files/7051 nodes/zero parse errors; direct contract-test edge to production parent helper. Reproduction uses disposable H roots and real binary/process/stdio, sources unchanged. Independent tests use isolated skill and Cargo target. No root routing, admission, shared DB or linked-workspace semantics changed.
        administration:
          applicable: true
          evidence: Reviewer codex-049-reviewer distinct from accepted builder. No public setting, CLI/MCP protocol, schema, privilege, installation or task policy change; non-Unix path remains free of Unix monitor APIs. Forge automatic scoped delivery/receipt only; no manual commit or receipt edit. Original failed review/repro retained and current repair bound to candidate. Tasks5–6, full performance acceptance, checkpoint integration and milestone landing remain outside this disposition.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    architectural_notes:
    - 'decisions: Task4 contract refinement before activation: preserve all ten existing main allocator trims; omit recovered extra per-tool trim and duplicate cold/delta end trims. Main already reclaims heap at indexing batch boundaries, satisfying INV-DAEMON-RESOURCE-BOUND without a new unused trim_memory wrapper. Initial full-recovery paired evidence remains blocked (milestone blackboard405): overview median+13.926%, delta+3.277%; supplementary no-trim probe did not establish causality and does not prove regression closed. User requires no performance regressions. Amend only the ready task4 specification/revision via manifest and Forge sync, preserving statuses/receipts. Implementation owner: src/mcp/server.rs parent lifecycle; consumer: CLI serve -> whole session including pending initialization and connected/SIGHUP phases. Use one parent-monitor future outside session future to avoid duplicate select loops. No per-request allocator work or idle timer/configuration. Unit seam proves parent identity; actual stdio launcher-loss test keeps input pipe open in pending-initialization and connected phases; existing SIGHUP and obsolete timeout tests prove continuity. Final full MCP/strict Clippy and paired release acceptance remain mandatory. No public helper API, cache framework, or changes to main trimming sites needed. Proposed reduced scope: server.rs, common/mcp_client.rs, freshness/stdio_and_response.rs, reference/mcp-setup.md, architecture/performance.md. Task3 remains in build; no task4 source edits or claims yet.'
    - 'decisions: Task-4 finding418 repaired and BUILD ACCEPTED: strict all-target/all-feature Clippy exit0; complete lib86/0 and MCP88/0, zero ignored/filtered,174 actual passes. Both pending-initialization and connected real servers exit on launcher loss while test keeps input open; idle survives obsolete timeout environment; SIGHUP same-PID continuity passes. Unix parent loss explicitly exits process because Tokio stdin''s blocking read prevents ordinary runtime shutdown; no per-request work or idle timeout. Controlled getppid fault on identical binaries/source produces real regression timeout (exit101,0/1); source hashes unchanged; same test without shim passes (exit0,1/0). Raw evidence H/task4-parent-fault.{json,log}, task4-parent-restored-green.log and task4-build-gate-{0,1,2}.log; H=/tmp/forge-049-implementation-20261019. Existing SIGHUP fixture ETXTBSY race repaired only in shared test client''s bounded one-second spawn retry for that exact error. All ten main allocator trims retained at original batch boundaries; no unused wrapper, redundant end trim or MCP per-request trim. Optimization preservation checker passes. Current implementation carried into cumulative future stage4/5/6 copies without touching other future changes. Independent reviewer now owns review/delivery. Full 049 performance acceptance remains OPEN; initial failed paired measurements are retained and not superseded by functional tests.'
    - 'decisions: T4-R1 closure: original launcher PID survives SIGHUP exec through private reload-only Command.env; serve consumes/removes it before service initialization. The current process and exec-failure path never acquire the variable. Public serve signature, idle survival and all ten batch trims are unchanged. The existing real-process lifecycle seam now covers pending initialization, connected parent loss and Linux connected SIGSTOP/atomic replacement/launcher kill/queued SIGHUP/SIGCONT with held-open stdin and actual exit checks. Reviewer pre-fix evidence is preserved in H/task4-review-parent-reload.{json,py}. Repaired complete gates: strict Clippy exit0, lib86/0, MCP88/0, zero ignored/filtered, H/task4-build-gates.json. No performance conclusion: final paired acceptance remains open. H=/tmp/forge-049-implementation-20261019.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: greenfield-seam-guidance-and-diagnostics

```yaml
task_ref: greenfield-seam-guidance-and-diagnostics
target: Provide actionable error diagnostics and contract author workflow guidance for greenfield features where admitted scope files do not yet exist on disk
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/engine/tasks/gates.rs
- src/core/tasks/acdd.default.yaml
- docs/reference/tasks.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/acdd_profile.yaml.example
- tests/acdd/tasks.rs
status: completed
subtasks:
- subtask_ref: validate-seam-missing-scope-diagnostic
  title: Detect uncreated scope files in validate_seam_reachability and output targeted remediation instructions for scaffolding minimal stubs and refreshing the index
  status: completed
  evidence: null
- subtask_ref: acdd-profile-greenfield-guidance
  title: Update contract_author role guidance and steps in acdd.default.yaml to mandate minimal file scaffolding for new modules in scope
  status: completed
  evidence: null
- subtask_ref: reference-and-skill-alignment
  title: Update docs/reference/tasks.md and skills/contextunity-forge/SKILL.md with greenfield seam reachability rules and proof policy alternatives
  status: completed
  evidence: null
receipt:
  contract_revision: 4
  passed_at: 2026-10-19T17:53:17.825293952+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test acdd
      exit_code: 0
      tests_passed: 195
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Exactly six admitted paths byte-match candidate84b2a5e against basef6cfe153; git diff --check passed, no extra tracked changes. Untracked successor plan excluded; no reviewer source edits. H/task5-review-binding.json and task5-review-source-sha256.txt; H=/tmp/forge-049-implementation-20261019.
        claims:
          applicable: true
          evidence: Real TasksStore::submit regression proves missing production file actionable error, unindexed real stub rejection, unchanged stage/claim after both rejections, then actual writer rebuild and same proof advancing to build. Original diagnostic red H/task5-contract-red.log and direct helper red H/task5-unit-contract-red.log retained. Independent strict Clippy exit0; complete lib87/0 + acdd108/0, zero ignored/filtered,195 actual passes. Profile/reference/skill/example all require compiling stubs, module/import declarations and current index; proof policies never waive reachability. H/task5-review-gates.json and task5-independent-review.md.
        concurrency:
          applicable: true
          evidence: Only existing final disconnected error branch calls missing_scope_files; ordered metadata reads have no mutation, locks or new state. Successful paths and existing reachability/current-source/evidence/policy checks are unchanged. Real-store failure keeps claim and contract stage intact. No MCP dispatch/index work, extra allocator trims, workers or public signatures.
        project_isolation:
          applicable: true
          evidence: Actual W-rooted MCP generation5d0290a0, 313 files/7054 nodes/zero parse errors; direct helper contract edge. Cross-module store-call graph gap remains documented, with actual store integration providing behavioral proof. Independent tests use disposable fixtures, isolated repository skill copy and Cargo target. Missing scope reads use the claimed worktree; existing confinement/authority is unchanged.
        administration:
          applicable: true
          evidence: Independent codex-049-reviewer differs from builder; same session continues delivery. Six-path scope and contract4 unchanged; instruction-only YAML strings preserve gates/policies. No schema, privilege, install, profile acceptance bypass, manual source/manifest/receipt edit or commit. Task4 source hash retained. Full049 performance acceptance, task6, main checkpoint integration and milestone landing remain open/outside this disposition.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    architectural_notes:
    - 'decisions: Task5 build closure: only final disconnected-contract error gains deterministic missing-scope paths, compiling stub/module/import guidance and index refresh command; existing reachability/current-index/proof-policy gates stay unchanged. Small missing_scope_files helper is called only in that error branch. Real TasksStore::submit test reuses existing indexed-boundary fixture and covers missing file rejection, unindexed stub rejection with unchanged stage/claim, then actual index refresh and accepted contract to build. Original implementation fails this exact error assertion (H/task5-contract-red.log,101,0/1); direct indexed missing_scope_files_contract also fails empty scaffold (H/task5-unit-contract-red.log,101,0/1), and both pass in complete green gates. Cross-module TasksStore::submit graph calls remain unresolved (including equivalent UFCS, reverted); no synthetic edges, graph fix, scope expansion or validation bypass added. Direct unit metadata witness plus real-store integration prove complementary boundaries. Strict all-target/all-feature Clippy0, complete lib87/0 + acdd108/0 =195, no ignored/filtered; H/task5-build-gates.json. Documentation, packaged skill, embedded role guidance and profile example agree on compiling production stubs and index refresh; proof-policy alternatives never waive reachability. Future stage5/6 carry retains all accepted tasks1–4 and T4-R1. H=/tmp/forge-049-implementation-20261019. Final milestone paired performance acceptance remains open.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: mcp-worktree-context-and-task-claim-resolution

```yaml
task_ref: mcp-worktree-context-and-task-claim-resolution
target: Resolve explicit worktree paths, exact names or unique name suffixes; preserve a server-bound linked worktree and reject ambiguous automatic claim selection
proof_policy: seam-test-first
contract_revision: 4
invariants:
- 'INV-WORKTREE-AWARENESS: Explicit worktree paths, exact basenames and unique hyphen-delimited suffixes take priority. Without an explicit value, retain the server-bound linked worktree, otherwise select one unique active claimed worktree or the primary root when none exists. Multiple active worktrees or matching names fail with WORKTREE_AMBIGUOUS; never select the latest unrelated claim.'
scope:
- src/mcp/tools/inputs.rs
- src/mcp/tools.rs
- src/mcp/server.rs
- src/engine/tasks/mod.rs
- docs/reference/tasks.md
- docs/reference/mcp-tools.md
- docs/reference/mcp-setup.md
- skills/contextunity-forge/SKILL.md
- tests/mcp/context.rs
status: completed
subtasks:
- subtask_ref: page-input-worktree-schema-and-selector-allowlist
  title: Add worktree field to PageInput and allow worktree in rejected_selector_fields for all query and inspect tools
  status: completed
  evidence: null
- subtask_ref: active-claim-worktree-discovery-and-resolution
  title: Implement active_claim_worktree helper querying tasks.sqlite and resolve_target_root_and_db in server module
  status: completed
  evidence: null
- subtask_ref: server-worktree-dispatch-and-auto-rebuild
  title: Dispatch code navigation MCP tools with resolved worktree root and db in ServerHandler::call_tool with auto-indexing
  status: completed
  evidence: null
- subtask_ref: worktree-context-integration-tests
  title: Add integration tests in tests/mcp/context.rs proving automatic worktree discovery and explicit worktree query execution
  status: completed
  evidence: null
receipt:
  contract_revision: 4
  passed_at: 2026-10-19T20:15:29.833930474+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test mcp
      exit_code: 0
      tests_passed: 178
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All nine admitted source/doc/skill paths byte-match frozen candidate99f899 and existing independent Arc binding; current scope hashes H/task6-final-review-binding.json. No source/manifest edits by reviewer. Other task sections including T7/T8 captured before automatic delivery for preservation check.
        claims:
          applicable: true
          evidence: 'Functional claims verified: explicit path/exact name/unique hyphen suffix, physical dedup, bound linked-root precedence, fresh unique active claim or primary, ambiguity failure, own custom DB, server-bound task operations. Independent strict all-target/all-feature Clippy0, full lib88/0+MCP90/0=178, zero ignored/filtered, unchanged source. Direct indexed worktree_name_contract→resolve_explicit_worktree generationa4cb143c; contract red retained. T6-R1 supported DB symlink repair and live retarget cases green. T6-R2 performance differences accepted explicitly by user decision447; this pass does not assert zero measured regression.'
        concurrency:
          applicable: true
          evidence: One shared immutable Arc<WorktreeContext> caches only canonical roots/repository/configured DB path keyed by source root, digest, and config presence. Mutex drops before fresh readonly SQLite query; no claim TTL/cache. Config changes and configured DB alias retarget remain next-request visible. Arc repair does not alter lock boundary, presence key, fresh alias canonicalization, parent lifecycle, streaming, or allocator trim behavior.
        project_isolation:
          applicable: true
          evidence: Real stdio coverage includes primary/linked/external registered worktrees, custom DB, live duplicate/ended/ambiguous claims, alternate task DB and symlink retarget, missing/empty/malformed/restored config, explicit override. Target adapter and DB are rooted correctly; task tools remain bound to server. Graph/domain commitments equal in retained paired receipts; no performance equality inferred.
        administration:
          applicable: true
          evidence: Independent codex-049-reviewer distinct from codex-049-builder, review claim13/contract4. User explicitly superseded performance stop in decision447; prior blocker records and paired-1/2/3 raw data preserved. No further benchmark or repeated tests after exact source/evidence binding. Review accepts observed performance differences as an authorized exception, not zero regression. T7/T8 definitions preserved; Forge alone may write receipt/automatic task commit. No manual commit, main integration, install, push, or milestone handoff.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-WORKTREE-AWARENESS: Explicit worktree paths, exact basenames and unique hyphen-delimited suffixes take priority. Without an explicit value, retain the server-bound linked worktree, otherwise select one unique active claimed worktree or the primary root when none exists. Multiple active worktrees or matching names fail with WORKTREE_AMBIGUOUS; never select the latest unrelated claim.'
    architectural_notes:
    - 'decisions: Task6 functional build closure, revision4/claim6, before performance acceptance: strict all-target/all-feature Clippy exit0, full lib88/0 + MCP90/0=178, zero ignored/filtered (H/task6-build-gates.json). Explicit paths/exact names/unique hyphen suffixes, physical dedup, linked-server binding, fresh unique active claims/ambiguity, custom server DB and task-tool binding covered through real stdio. Direct indexed worktree_name_contract calls production resolver; original compiling scaffold genuinely fails path assertion101 (H/task6-contract-red.log). One private server context entry caches only canonical repository/configured task-DB path by source root, adapter digest and config presence; mutex drops before fresh readonly SQLite query. No claim cache/TTL, public Adapter schema change, duplicate task YAML parsing per request, extra trim, or graph/policy bypass. T6-B1 (#426) repaired: adapter_path represented explicit override only; default config now recognized. Original wrong-predicate implementation fails real newly-created-DB/claim assertion; final same test passes plus alternate DB, malformed config, explicit override and missing->empty->missing->restored configuration transitions (empty and missing share digest). Additional hypothetical digest/presence/claim-cache mutants were not executed; original wrong-predicate red and full production-path transitions are actual evidence. Source diff only9 admitted paths plus nonprotected milestone acceptance paragraph; main checkpoint8473951 and upstream test/version retained; accepted Tasks1–5 unchanged except admitted shared Task6 paths. Source-preservation checker retains main cold/streaming paths, 5-second warm inventory TTL and all10 main batch trims. H=/tmp/forge-049-implementation-20261019. Release pair preparation running; build submission/review/delivery and full milestone performance acceptance remain pending.'
    - 'decisions: T6-R1 repair route in same builder session/claim8 after formal independent rejection (#428). Reachable invariant INV-WORKTREE-AWARENESS: task operations accept configured symlink DB aliases; implicit code routing must read the same supported store and observe alias retargeting on the next request. Canonical owner active_claim_worktree consumes cached configured PathBuf -> fresh filesystem canonicalization -> readonly SQLite with NOFOLLOW -> distinct physical claim roots -> call_tool target -> real stdio overview. Preserve missing DB fallback, ambiguity failures, readonly/no migration/no schema changes/no claim cache. Existing actual-process red H/task6-review-symlink-db.json shows direct real DB success, alias error14, task list alias exit0, explicit target success; raw reviewed candidate/source hashes bound in H/task6-review-reject-binding.json. Minimum fix canonicalizes existing configured database immediately before opening, leaving NOFOLLOW on resolved path; no canonical DB target cached. Same stdio regression now configures a symlink and retargets unchanged alias to alternate claim DB, default claim DB, then missing DB, asserting next-request other/selected/primary. Existing config/claim/linked/custom DB cases retained. Original NOFOLLOW-on-alias mutation is killed by independent actual-process red; retarget assertions protect against canonical-target caching. Strict/full lib/MCP rerunning; finding remains pending green/re-review. Release preparation separately caught shared Cargo target root-package freshness collision (identical baseline/candidate SHA; candidate build0.08s). No measurements executed with that pair. Invalid preparation records retained; candidate now requires an isolated target with freshly compiled artifact bound to W source path and distinct SHA before any benchmark. H=/tmp/forge-049-implementation-20261019.'
    - 'decisions: T6-R1 builder green after bounded repair under buildclaim8: strict Clippy0, full lib88/0 + MCP90/0=178, no ignored/filtered. Same actual stdio test now verifies symlink-configured DB plus same-path retargeting to alternate/default/missing stores without restart; all prior selection/config/claim cases pass. Original independent production red remains H/task6-review-symlink-db.json, source-bound to rejected candidate dbe1f0645b7e95c8d8a70ad468a4afe2bb0a47d8. Only repair beyond prior reviewed candidate: fresh existing-DB canonicalization before readonly NOFOLLOW open, and expanded existing stdio seam; no public API, schema, ownership, cached claim/canonical DB target or Task2 guard changes. Required independent re-review and paired performance are still pending. H/task6-build-gates.json and logs are current green, earlier results retained under H/verification-history. Isolated candidate release compilation is in progress; no benchmark has run with invalid shared-target preparation.'
    - 'decisions: T6-R2 bounded repair under buildclaim10/contract4: the existing one-entry context cache now owns an Arc<WorktreeContext>; each request shares its immutable snapshot instead of cloning five path/string allocations, including configured DB. Cache invalidation remains source_root+adapter digest+config presence; the mutex still drops before a fresh active_claim_worktree lookup. Existing database existence/canonicalization, readonly NOFOLLOW connection, fresh claims, ambiguity behavior and config/alias-retarget tests are unchanged. Only admitted server.rs changes; no TTL, claim cache, configuration schema or public adapter modifications. Scoped strict Clippy/lib/MCP verification is running. Paired-2 original binary, patch, raw samples and all failed metrics are retained. Independent re-review and a new explicitly authorized five-pair measurement after this actual code repair are required; no performance acceptance is claimed.'
    - 'decisions: User explicitly accepted the measured performance differences and instructed: ''який нахер блокер? 3 мс ? не відволікайся, працюй''. This supersedes the prior request to halt on T6-R2 / blockers 445 and 446. Retain all measured results without claiming zero measured differences. Close the performance-only acceptance objection by user decision; no further benchmarks. Continue functional independent review and delivery, then T7/T8. No source change is justified merely to chase the accepted small timing differences.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: upstream-aware-handoff-provenance-and-safe-diagnostics

```yaml
task_ref: upstream-aware-handoff-provenance-and-safe-diagnostics
target: Explain later scope changes using reviewed upstream deliveries reachable through Git ancestry, and avoid destructive checkout guidance
proof_policy: seam-test-first
contract_revision: 1
invariants:
- 'INV-UPSTREAM-HANDOFF-PROVENANCE: In handoff verification, validate_later_scope_changes must explain changed files either by local reviewed deliveries or reviewed upstream deliveries reachable through merge history; a reproducible three-way combination (git merge-file) of reviewed local and upstream inputs is accepted without failing handoff.'
- 'INV-SAFE-REMEDIATION-GUIDANCE: Handoff diagnostics must never suggest unconditional git checkout or history destruction when differences are explained by valid integration branches.'
scope:
- src/engine/milestones/handoff.rs
- src/core/git.rs
- tests/acdd/milestones.rs
- docs/reference/tasks.md
status: completed
subtasks:
- subtask_ref: upstream-delivery-provenance-resolution
  title: Traverse merge ancestry in validate_later_scope_changes to discover reviewed upstream deliveries explaining post-task file changes
  status: completed
  evidence: 'Strict Clippy all targets/all features; cargo test --lib:89 passed; cargo test --test acdd:109 passed. Real-Git integration table covers reviewed upstream-only, clean local+upstream combination, archived upstream, corrupted review, unreviewed/unreachable/dirty/post-merge/conflicting changes; native diagnostic seam verifies user files unchanged and safe reconciliation guidance. Logs: /tmp/forge-049-implementation-20261019/task7-build-gate-{0,1,2}.log.'
- subtask_ref: three-way-clean-combination-acceptance
  title: Verify clean three-way combinations of local and upstream reviewed changes using reproducible merge-base reconstruction
  status: completed
  evidence: 'Strict Clippy all targets/all features; cargo test --lib:89 passed; cargo test --test acdd:109 passed. Real-Git integration table covers reviewed upstream-only, clean local+upstream combination, archived upstream, corrupted review, unreviewed/unreachable/dirty/post-merge/conflicting changes; native diagnostic seam verifies user files unchanged and safe reconciliation guidance. Logs: /tmp/forge-049-implementation-20261019/task7-build-gate-{0,1,2}.log.'
- subtask_ref: non-destructive-handoff-diagnostics
  title: Replace unconditional git checkout suggestions with actionable branch-reconciliation diagnostics
  status: completed
  evidence: 'Strict Clippy all targets/all features; cargo test --lib:89 passed; cargo test --test acdd:109 passed. Real-Git integration table covers reviewed upstream-only, clean local+upstream combination, archived upstream, corrupted review, unreviewed/unreachable/dirty/post-merge/conflicting changes; native diagnostic seam verifies user files unchanged and safe reconciliation guidance. Logs: /tmp/forge-049-implementation-20261019/task7-build-gate-{0,1,2}.log.'
receipt:
  contract_revision: 1
  passed_at: 2026-10-19T20:35:15.997262311+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test acdd
      exit_code: 0
      tests_passed: 198
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Read all four admitted diffs: handoff.rs, core/git.rs, tests/acdd/milestones.rs, docs/reference/tasks.md. Exact byte comparison to frozen bf5d669 passed for all four; H/task7-review-binding.json. No other source modifications or reviewer source/docs/manifest edits. Other task definitions, including T8, hashed for post-delivery preservation.'
        claims:
          applicable: true
          evidence: Review confirms fallback accepts reviewed incoming ancestry only when receipt pointer is newly introduced through the incoming parent and file equals that accepted delivery. Existing durable receipt/profile/accepted gate provenance and independent review validation are reused. Local input equals a landed reviewed file or earlier verified merge. A unique merge base and exact clean git merge-file bytes/mode must reproduce committed result; unsupported/conflicted cases fail closed. Native safe diagnostic seam has indexed direct edge to task_landed_commit generationeb8d36b2. Full public-handoff integration table covers upstream-only, combination, archive accept; corrupt review, unreviewed, dirty incoming, unreachable receipt, post-merge edit and conflict rejection. Strict Clippy0, full lib89/0+ACDD109/0=198, zero ignored/filtered; current logs and command proof retained.
        concurrency:
          applicable: true
          evidence: Git provenance compares immutable commit objects. Accepted upstream provenance cache exists only within one handoff, not across requests. New merge reconstruction reads blobs and writes only exclusively created private temporary files (Unix0600) with RAII cleanup; no checkout/index mutation. Rejection occurs before existing archive/SQLite publication; negative real-Git scenarios preserve manifest, task descriptors, Git status and archive absence. No MCP/indexing hot-path changes.
        project_isolation:
          applicable: true
          evidence: Core git helper uses explicit worktree and literal file pathspec, reads exact committed file entries including modes, and compares result bytes. Upstream candidates come from same configured project store outside current milestone; ownership scope must cover the path, commit must be reachable from incoming and not local parent, accepted review/candidate/baseline/pointer validation all remain required. Tests create isolated real repositories/stores and exercise archived upstream identity. Unreachable identical bytes and unreviewed incoming edits reject.
        administration:
          applicable: true
          evidence: Same independent codex-049-reviewer differs from codex-049-builder, reviewclaim4/contract1. Required strict Clippy and unfiltered full affected lib/ACDD targets independently run,198 passes. No manual commit/receipt edits, benchmarks, main integration or handoff invocation outside test fixtures. Safe remediation text removes unconditional checkout and requires preservation/reconciliation. User-accepted performance disposition447 is unchanged. Automatic Forge delivery only; T8 definition preserved.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-UPSTREAM-HANDOFF-PROVENANCE: In handoff verification, validate_later_scope_changes must explain changed files either by local reviewed deliveries or reviewed upstream deliveries reachable through merge history; a reproducible three-way combination (git merge-file) of reviewed local and upstream inputs is accepted without failing handoff.'
    - 'INV-SAFE-REMEDIATION-GUIDANCE: Handoff diagnostics must never suggest unconditional git checkout or history destruction when differences are explained by valid integration branches.'
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

### task: build-gate-deletion-scope-and-file-retirement

```yaml
task_ref: build-gate-deletion-scope-and-file-retirement
target: Allow intentional file deletions in task scope during BUILD gate validation, verifying that deleted public symbols have no remaining live callers outside scope
proof_policy: seam-test-first
contract_revision: 1
invariants:
- 'INV-INTENTIONAL-SCOPE-DELETION: When a scoped file is intentionally deleted (deleted in working tree or staged deletion), BUILD gate validation must treat the file as removed rather than failing with TASK_SCOPE_INCOMPLETE read errors.'
- 'INV-RETIRED-SYMBOL-CALLER-SAFETY: Public symbols removed by file deletion must satisfy existing caller safety: validation passes if all callers are within admitted scope or removed; external unadmitted callers fail with actionable diagnostics.'
scope:
- src/engine/tasks/gates.rs
- tests/acdd/tasks.rs
- docs/reference/tasks.md
status: completed
subtasks:
- subtask_ref: scope-file-existence-and-status-tolerance
  title: Update current_scope_files and validate_public_signature_scope to detect deleted files without aborting on fs::read_to_string
  status: completed
  evidence: Strict Clippy all targets/all features passed; cargo test --lib 90 passed; cargo test --test acdd 110 passed, no failures/ignored/filtered. Native deleted-source seam red101 then green; real task build table covers 20 combinations of staged/unstaged, stale/refreshed index, no callers, live external caller rejection, admitted caller update/deletion acceptance, unadmitted deleted baseline caller rejection. Production only handles read NotFound; baseline caller checks unchanged and no new successful-read operations. Logs /tmp/forge-049-implementation-20261019/task8-build-gate-{0,1,2}.log.
- subtask_ref: retired-public-symbol-caller-verification
  title: Verify that symbols from deleted files in baseline are cleanly retired or fail closed when unadmitted external callers remain
  status: completed
  evidence: Strict Clippy all targets/all features passed; cargo test --lib 90 passed; cargo test --test acdd 110 passed, no failures/ignored/filtered. Native deleted-source seam red101 then green; real task build table covers 20 combinations of staged/unstaged, stale/refreshed index, no callers, live external caller rejection, admitted caller update/deletion acceptance, unadmitted deleted baseline caller rejection. Production only handles read NotFound; baseline caller checks unchanged and no new successful-read operations. Logs /tmp/forge-049-implementation-20261019/task8-build-gate-{0,1,2}.log.
- subtask_ref: deletion-scope-seam-tests
  title: Add unit and integration tests proving that intentional file deletion in task scope passes build validation cleanly
  status: completed
  evidence: Strict Clippy all targets/all features passed; cargo test --lib 90 passed; cargo test --test acdd 110 passed, no failures/ignored/filtered. Native deleted-source seam red101 then green; real task build table covers 20 combinations of staged/unstaged, stale/refreshed index, no callers, live external caller rejection, admitted caller update/deletion acceptance, unadmitted deleted baseline caller rejection. Production only handles read NotFound; baseline caller checks unchanged and no new successful-read operations. Logs /tmp/forge-049-implementation-20261019/task8-build-gate-{0,1,2}.log.
receipt:
  contract_revision: 1
  passed_at: 2026-10-19T20:47:12.890335832+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test acdd
      exit_code: 0
      tests_passed: 200
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All three admitted diffs reviewed and byte-bound to frozen b7bdddb: gates.rs, tests/acdd/tasks.rs, docs/reference/tasks.md. Production delta only handles read_to_string NotFound after confinement; tests and documentation describe existing caller safety. H/task8-review-binding.json records scope hashes and other seven task sections for preservation. No reviewer source/docs/contract edits.'
        claims:
          applicable: true
          evidence: Missing Git-enumerated source contributes no current public symbols, so baseline symbols retire; only NotFound is skipped, other reads and parse failures remain rejecting. Existing captured accepted baseline caller evidence plus live indexed callers remain unioned, with out-of-scope callers rejected unchanged. Actual gate integration table20 cases spans staged/unstaged, stale/refreshed index, no caller/live external/admitted update/admitted deletion/unadmitted deleted caller. Last case rejects because unadmitted baseline caller remains outside scoped candidate. Native deletion seam directly calls production validator, generation859688b4. Independent strict all-target/all-feature Clippy0, unfiltered lib90/0+ACDD110/0=200, no ignored/filtered.
        concurrency:
          applicable: true
          evidence: Change only reads current source and recognizes NotFound, without deleting files, adding locks, changing shared state, or changing snapshot publication. Confined path validation precedes read; errors other than absence remain fail-closed. Accepted baseline-commit matching/caller capture and scope comparison are unchanged. Real gate rejection cases retain build stage and in_progress claim state; success advances review. No server/indexing hot path changes.
        project_isolation:
          applicable: true
          evidence: Git scope enumeration and baseline symbol extraction are unchanged and rooted in claimed worktree. Missing current source does not remove captured external caller ownership; refreshing index cannot erase accepted baseline callers. Admitted caller update/deletion succeeds, unadmitted live or deleted caller rejects with path and retired symbol. Isolated real Git/index/task-store fixtures exercise both staged and unstaged source changes.
        administration:
          applicable: true
          evidence: Same independent codex-049-reviewer differs from builder; reviewclaim4/contract1. Required strict Clippy and full lib/ACDD targets independently passed with200 actual passes. No extra policy/schema/scope, benchmark, manual commit, receipt edit, main integration, or milestone handoff. User-accepted performance decisions447/449 unchanged. Automatic Forge task delivery only, preserving other seven task definitions.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-INTENTIONAL-SCOPE-DELETION: When a scoped file is intentionally deleted (deleted in working tree or staged deletion), BUILD gate validation must treat the file as removed rather than failing with TASK_SCOPE_INCOMPLETE read errors.'
    - 'INV-RETIRED-SYMBOL-CALLER-SAFETY: Public symbols removed by file deletion must satisfy existing caller safety: validation passes if all callers are within admitted scope or removed; external unadmitted callers fail with actionable diagnostics.'
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

## Acceptance and performance proof

The builder implements all admitted tasks; an independent reviewer checks their
contracts, correctness, concurrency, isolation, administration, and production-seam
proof before delivery. Task ordering follows Forge's shared-scope dependencies.
The initial restored implementation is retained as a backup; each delivery
contains only its admitted behavior and subsequent tasks may refine shared files.

The initial comparison uses `main` base `9f358ec`; its failed measurements remain
recorded on the milestone blackboard. Final acceptance compares release binaries
from the integrated `main` checkpoint `1dc907d` and the final candidate. Merge
checkpoint `8473951` retains all accepted task commits and brings in upstream
version 0.3.1 and its linked-workspace regression test.
The user explicitly authorizes five sequential paired benchmark repetitions on
an identical isolated repository snapshot. Alternate baseline/candidate order;
keep toolchain, features, thread limits, extraction-cache state, and corpus fixed.
Measure fresh cold builds, one-file delta, and real stdio warm MCP requests after
initial admission and warmup. Record raw samples, commands, binary/source hashes,
corpus cardinality, host context, peak RSS, graph counts, and domain commitments.
Exclude compilation and initialization from warm request latency. Use median
wall time and median paired change for cold/delta; warm requests also report p95
using nearest rank over a fixed request count. A positive latency regression
blocks delivery; retain unsuccessful measurements and investigate rather than
discarding samples or relaxing this requirement. An unstable host prevents a
performance conclusion. Functional equality and source preservation checks
support these measurements but do not substitute for them.

The full handoff gate remains the exact command in `TESTS.md`; the milestone is
implemented and reviewed on its branch before any separate installation or
landing operation.

