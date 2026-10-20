---
id: m-admission-integrity-database-guards-and-lifecycle-hardening
title: Admission integrity, database collision protection, and MCP lifecycle hardening
doc_type: contract
status: completed
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
handoff:
  completed_at: 2026-10-20T11:43:54.221958149+00:00
  duration: 20h 56m
  commit: ff4226d40233e6ad8af7acc505462457f87a577c
  target_base: 1dc907d8c3184f75e9a735fd9f2a4ede36f38f45
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-admission-integrity-database-guards-and-lifecycle-hardening
      authority_digest: b0fda0621a7e3396a7e19fc119084075ceebee97c8107ffaaf4c4a3d90aa9306
      implementation_digest: 8338a2e0d6998d208b650968bb74834cf5b3acc2953615758acf71ce35df9902
      target_base_digest: 68cfc8dab3b48dac607ee6c230581cd8d095b30d8c82951118c80b409edb16db
      builders:
      - codex-049-builder
    target_base: 1dc907d8c3184f75e9a735fd9f2a4ede36f38f45
    review:
      worker_id: codex-049-whole-branch-reviewer
      decision: pass
      summary: 'Independent integration review of the complete settled milestone delta from main1dc907d8c3184f75e9a735fd9f2a4ede36f38f45 to ff4226d40233e6ad8af7acc505462457f87a577c: all70 changed files map to the13 admitted completed task contracts or authorized milestone metadata, including the separately authorized exact-five companion skill sync. Reconciled original whole-branch review503, closure512/514 and final feature review523/524 against current source and accepted task authority; WR1–4, nested discovery, canonical project identity and commitless receipt binding findings are closed. All11 original task blocks remain byte-identical. Manifest admission, database fencing, workspace/worktree routing, parent liveness, greenfield/retirement seams, lifecycle statuses, Cargo identity, documentation instructions and final handoff content evidence align with current contracts and canonical docs; ADR0015 amendment is explicitly authorized and other accepted ADR boundaries remain intact. Indexing/MCP optimizations are preserved, with exactly10 original allocator trims at the same five owners and no MCP dispatch trim; no new benchmark claim. Reused attributable complete target/mutation evidence and independently verified the final Git HEAD/base, clean worktree, namespace/binding and reviewer independence from all accepted builders. No unresolved scope or contract findings. This review covers the combined branch delta; fresh final repository verification is supplied separately by the coordinator.'
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 810
      tests_failed: 0
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 810
    tests_failed: 0
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
  commit: 56eb0cdca04b3033b2bd5135991c2b35fa81361d
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
  commit: a6e379eb64676b4c52686133dfc143861e2b6d56
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
  commit: 22e64ad8922b072bb28ae0ae5f18bdd40efd8833
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
  commit: f6cfe1533bf4bd654410afe4f6a6dddb5a7da6e0
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
  commit: fef1545e1b5217eccb269b5b8dfb8cba8c31fffd
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
  commit: 472fb65538e2d9c4e79153b3402b82a2f55069da
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
  commit: 9ec8e696c89bcd580c090b07c656262798952054
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
  commit: c80b6239e4309ca90c8d1cb030c1c094847bab65
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

### task: audit-invariant-remediation

```yaml
task_ref: audit-invariant-remediation
target: Close the four independently verified post-delivery audit findings without changing indexing semantics, allocator trims, or public interfaces
proof_policy: seam-test-first
scope:
- src/engine/pipeline.rs
- src/engine/tasks/gates.rs
- src/engine/tasks/mod.rs
- src/db/tasks_store/mod.rs
- tests/core/database_guard.rs
- tests/acdd/tasks.rs
- tests/mcp/context.rs
- docs/reference/configuration.md
- docs/reference/tasks.md
- docs/reference/mcp-tools.md
invariants:
- 'INV-DATABASE-SEPARATION: An unreadable existing code-map target is rejected before replacement and its bytes are preserved.'
- 'INV-RETIRED-SYMBOL-CALLER-SAFETY: Retirement requires accepted baseline caller evidence or an indexed baseline target; a refreshed index cannot turn missing baseline evidence into proof of no callers, at either contract or build.'
- 'INV-ROUTING-REGULAR-DATABASE: Active-claim routing rejects nonregular task database targets before SQLite open while preserving explicit worktree selection.'
- 'INV-FROZEN-LAYERS: Task-store tests introduce no db-to-engine dependency; the real initializer/publication race remains covered through integration seams.'
subtasks:
- subtask_ref: unreadable-target-preservation
  title: Fail closed on target read errors through build and CLI without removing corrupt-index recovery
  status: completed
  evidence: repair-closure-ledger.md; full gate 804 passed, 0 failed, 1 existing ignored; strict all-target/all-feature Clippy exit0; isolated documented global-skill fixture. Independent confirmation pending.
- subtask_ref: retirement-baseline-evidence
  title: Preserve caller safety before and after contract acceptance with staged and unstaged deletions and stale or refreshed indexes
  status: completed
  evidence: repair-closure-ledger.md; full gate 804 passed, 0 failed, 1 existing ignored; strict all-target/all-feature Clippy exit0; isolated documented global-skill fixture. Independent confirmation pending.
- subtask_ref: routing-special-files
  title: Reject FIFO and directory task database targets through actual MCP requests without blocking subsequent explicit navigation
  status: completed
  evidence: repair-closure-ledger.md; full gate 804 passed, 0 failed, 1 existing ignored; strict all-target/all-feature Clippy exit0; isolated documented global-skill fixture. Independent confirmation pending.
- subtask_ref: storage-test-layering
  title: Move the real initializer race composition to the existing core integration suite and retain the writer-independent store-open unit proof
  status: completed
  evidence: repair-closure-ledger.md; full gate 804 passed, 0 failed, 1 existing ignored; strict all-target/all-feature Clippy exit0; isolated documented global-skill fixture. Independent confirmation pending.
status: completed
receipt:
  commit: fe98f78606a0e83759a1b95b789055ef9225cb2e
  contract_revision: 1
  passed_at: 2026-10-19T22:17:14.415467268+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 805
      tests_failed: 0
      log: Full logs /tmp/forge-049-implementation-20261019/repair-v2-{commitment,clippy,all-targets}.log; isolated global skill fixture; original host drift log preserved.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Both independent reviewers PASS; all10paths equal b2762f2; forbiddenedges removed.
        claims:
          applicable: true
          evidence: F2 legacy persistedJSON restart/build replay and captured/live union verified.
        concurrency:
          applicable: true
          evidence: Real process lock/race and initialized reader retained; review targets247/0/1ignored.
        project_isolation:
          applicable: true
          evidence: Explicit049binding, T3identity ownership and confinement preserved.
        administration:
          applicable: true
          evidence: Full805/0/1ignored commitment16/0 strictClippy0; hostskill drift disclosed. Main optimizations preserved. External status task excluded.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-DATABASE-SEPARATION: An unreadable existing code-map target is rejected before replacement and its bytes are preserved.'
    - 'INV-RETIRED-SYMBOL-CALLER-SAFETY: Retirement requires accepted baseline caller evidence or an indexed baseline target; a refreshed index cannot turn missing baseline evidence into proof of no callers, at either contract or build.'
    - 'INV-ROUTING-REGULAR-DATABASE: Active-claim routing rejects nonregular task database targets before SQLite open while preserving explicit worktree selection.'
    - 'INV-FROZEN-LAYERS: Task-store tests introduce no db-to-engine dependency; the real initializer/publication race remains covered through integration seams.'
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

This user-directed remediation closes audit findings F1–F4 recorded on milestone
blackboard message 459. The eight earlier delivery receipts remain intact. The
remediation adds no capability, profile, schema, migration, or allocator policy.
Task gate proof uses the existing core, ACDD, and MCP targets and strict Clippy;
final verification uses the complete gate in `TESTS.md`. Existing user decisions
about measured performance remain in force.

### task: unified-task-and-subtask-status-lifecycle

```yaml
task_ref: unified-task-and-subtask-status-lifecycle
target: Unify task and subtask status lifecycle into a symmetric taxonomy (ready, in_progress, completed, cancelled, blocked), retain parked as task-only for worktree affinity, and support pending as a backward-compatible alias
proof_policy: seam-test-first
contract_revision: 2
invariants:
- 'INV-STATUS-TAXONOMY-SYMMETRY: Task and subtask lifecycle statuses share symmetric core states: ready (canonical initial status replacing subtask pending), in_progress, completed, cancelled (abandoned/obsoleted without manifest deletion), and blocked.'
- 'INV-TASK-WORKTREE-PARKED-AFFINITY: The parked status remains exclusively valid for top-level tasks to preserve worker ID and worktree routing on scope or concurrency conflicts; subtasks cannot be parked.'
- 'INV-STATUS-BACKWARD-COMPATIBILITY: Legacy subtask status pending is parsed and accepted as a direct alias for ready across SQLite storage, CLI serialization, and manifest parsing.'
- 'INV-CANCELLED-SCHEDULER-EXCLUSION: Cancelled tasks and subtasks are excluded from task next, claim candidacy, and dependency unblocking, but remain preserved in history and manifest queries.'
scope:
- src/core/tasks/mod.rs
- src/engine/tasks/mod.rs
- src/cli/task.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/subtasks.rs
- src/db/tasks_store/lifecycle.rs
- tests/acdd/tasks.rs
- tests/acdd/subtasks.rs
- tests/acdd/mcp.rs
- docs/reference/tasks.md
- docs/reference/cli.md
- docs/reference/mcp-tools.md
- skills/contextunity-forge/SKILL.md
status: completed
subtasks:
- subtask_ref: symmetric-status-enum-and-subtask-ready
  title: Unify SubtaskStatus to support ready, in_progress, completed, cancelled, and blocked, accepting legacy pending as alias for ready
  status: completed
  evidence: 'Full affected targets: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test acdd --test mcp; exit0, 297 passed,0 failed. status-build-targets-final.log. Includes cancellation cached dependency same-digest/higher-revision/fresh-import red→green regression; independent final review pending.'
- subtask_ref: task-cancelled-status-and-scheduler-omission
  title: Add cancelled status to TaskStatus and exclude cancelled tasks/subtasks from candidate selection, task next, and dependency chains
  status: completed
  evidence: 'Full affected targets: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test acdd --test mcp; exit0, 297 passed,0 failed. status-build-targets-final.log. Includes cancellation cached dependency same-digest/higher-revision/fresh-import red→green regression; independent final review pending.'
- subtask_ref: store-and-manifest-status-normalization
  title: Normalize status parsing and serialization across SQLite tasks_store, milestone manifest readers, and MCP task tools
  status: completed
  evidence: 'Full affected targets: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test acdd --test mcp; exit0, 297 passed,0 failed. status-build-targets-final.log. Includes cancellation cached dependency same-digest/higher-revision/fresh-import red→green regression; independent final review pending.'
- subtask_ref: status-lifecycle-seam-tests
  title: Add integration tests verifying symmetric lifecycle transitions, cancelled task retention, and pending backward compatibility
  status: completed
  evidence: 'Full affected targets: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test acdd --test mcp; exit0, 297 passed,0 failed. status-build-targets-final.log. Includes cancellation cached dependency same-digest/higher-revision/fresh-import red→green regression; independent final review pending.'
receipt:
  commit: 5914910e0a15c637cfae501bd1079eceeaf6399b
  contract_revision: 2
  passed_at: 2026-10-19T22:58:43.749842907+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test acdd --test mcp
      exit_code: 0
      tests_passed: 297
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS. Compared actual unstaged/staged/untracked status and full scoped candidate diff against claim authority: exactly13 changed admitted paths, permitted src/engine/tasks/lifecycle.rs extension unchanged; only authorized manifest contract/subtask metadata plus pre-existing external successor definition. No profile/config/credentials/generated source changes. All14 allowed paths equal immutable candidate33a248c and pre/post independent gate hashes (status-review-final-binding.json);10 other manifest task blocks preserved bytewise. Each change implements status normalization, cancellation, real seams, or admitted docs/skill reconciliation.'
        claims:
          applicable: true
          evidence: 'PASS. Traced all4 task invariants: pure core SubtaskStatus accepts pending and emits ready, manifest/descriptors/storage/CLI/MCP use shared normalization; parked remains task-only and prior affinity selection unchanged; cancelled task sync revokes claims/revisions, retains history, excludes ready/next/claim/dependency completion and freezes child mutation. P2 lineage blackboard476-478 plus raw status-cached-dependency-red.log confirms pre-fix real failure. Shared transaction invalidation now covers same digest, higher revision and fresh reimport, preserving durable completion for absent deleted history. Real lib/SQLite/engine/stdio/CLI tests exercised. Independently ran cargo clippy --all-targets --all-features -- -D warnings (exit0) and complete cargo test --lib --test acdd --test mcp (exit0):92+114+91=297 passed,0failed,0ignored,0filtered. Artifacts status-review-gates.json,status-review-clippy.log,status-review-targets.log. ADR0001/0014/0018 boundaries retained; no invented TaskStatus API. Existing performance waiver remains explicit, no benchmark/zero-regression claim.'
        concurrency:
          applicable: true
          evidence: PASS. Reviewed sync_selected existing Immediate transaction, claim/next/reset/child update transactions and end_claim/next_revision/save fencing. Cache clear, descriptor cancellation and claim revocation commit together; malformed receipt/completed cancellation errors roll back all writes, preserving historical completion. No new lock/connection/read query/schema/cache subsystem; ordinary reader and dependency query paths unchanged. Canonical cancelled filter uses existing shared status enum; same-digest cancellation idempotence and reset/re-admission covered. Existing concurrent/rollback/parked tests pass in complete targets.
        project_isolation:
          applicable: true
          evidence: PASS. Current review explicitly rooted in assigned049 worktree; snapshot baseline4339366 and worker identity bound to claim5/contract2. New writes use existing fully-qualified task ID and assert_id boundaries; dependency invalidation matches exact dependency_id only. Registry/owner/alias/profile/path resolution, traversal/symlink guards and task database separation unchanged. Real isolated SQLite workspaces plus CLI/MCP parity passed. Core enum imports no db/engine; db only uses shared core normalizer. No neighboring project or global skill writes.
        administration:
          applicable: true
          evidence: 'PASS. Reviewed protected manifest headers/profile pins/gate proof/receipt logic: unchanged, no migration or policy weakening. Repo skill reconciles valid global CLI/search guidance while preserving049 routing; global installation untouched, test fixture isolated via CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=H/repair-global-skill (host installation drift not claimed fixed). Independent worker codex-049-status-reviewer differs from builder codex-049-builder. Registered MCP old-parser claim failed before mutation; blackboard479 records exact incompatibility and parent-authorized fresh explicit-W CLI fallback. Automatic delivery only, hooks/atomic receipt/candidate parent checks remain in existing Forge; landed SHA and neighbor preservation to be checked after delivery.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-STATUS-TAXONOMY-SYMMETRY: Task and subtask lifecycle statuses share symmetric core states: ready (canonical initial status replacing subtask pending), in_progress, completed, cancelled (abandoned/obsoleted without manifest deletion), and blocked.'
    - 'INV-TASK-WORKTREE-PARKED-AFFINITY: The parked status remains exclusively valid for top-level tasks to preserve worker ID and worktree routing on scope or concurrency conflicts; subtasks cannot be parked.'
    - 'INV-STATUS-BACKWARD-COMPATIBILITY: Legacy subtask status pending is parsed and accepted as a direct alias for ready across SQLite storage, CLI serialization, and manifest parsing.'
    - 'INV-CANCELLED-SCHEDULER-EXCLUSION: Cancelled tasks and subtasks are excluded from task next, claim candidacy, and dependency unblocking, but remain preserved in history and manifest queries.'
    architectural_notes:
    - 'decisions: Admitted task contract revision 2 before implementation; synchronized explicit 049 manifest and claimed contract via MCP (claim revision 3). Scope now includes docs/reference/{tasks,cli,mcp-tools}.md, skills/contextunity-forge/SKILL.md, CLI status descriptions, tasks_store sync owner, and existing ACDD domain suites. Instruction changes are part of reviewed task delivery; external installed skill remains outside task writes. Reconcile existing valid CLI/search guidance without dropping 049 worktree/greenfield instructions.'
    - 'decisions: Build design: preserve existing public String status fields and use core SubtaskStatus for canonical parsing/serialization; pending remains an accepted alias on all existing subtask boundaries. Cancelled top-level task specifications enter through existing manifest sync, without adding a new command/API. Existing ready/parked-only dispatch and completed-only dependency rules already exclude cancellation; sync revokes claims and increments revision, retains history, and rejects cancelling accepted completion. Reset/re-admission restores eligibility explicitly. SQLite schema/open behavior and code indexing/allocator paths remain unchanged. Repo skill reconciles valid global CLI/search descriptions with 049 worktree/greenfield guidance. All 14 changed paths are in task scope or the task manifest; git diff --check passes. Strict Clippy passed; current complete lib/ACDD/MCP rerun pending.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: cargo-root-crate-import-resolution-and-seam-connectivity

```yaml
task_ref: cargo-root-crate-import-resolution-and-seam-connectivity
target: Resolve root Cargo package name imports to src/ in Rust import normalization, enabling integration test files in tests/ to establish static seam edges to local library symbols
proof_policy: seam-test-first
contract_revision: 2
invariants:
- 'INV-RUST-ROOT-CRATE-IMPORT-NORMALIZATION: When Rust code (e.g. integration tests in tests/) imports the workspace crate by its Cargo package name (or normalized identifier with underscores replacing hyphens), Rust::normalize_import maps the root crate identifier directly to src/, resolving public module and symbol paths identically to internal crate:: imports.'
- 'INV-INTEGRATION-TEST-SEAM-CONNECTIVITY: Integration test functions importing library symbols via use <pkg_name>::... produce directed graph call edges to the indexed library symbols, satisfying ACDD contract gate seam connectivity without failing TASK_SEAM_DISCONNECTED.'
scope:
- src/engine/languages/rust.rs
- src/engine/languages/dependency_registry.rs
- src/engine/linker/indexing.rs
- tests/languages/profiles.rs
- tests/acdd/tasks.rs
- docs/reference/tasks.md
status: completed
subtasks:
- subtask_ref: cargo-package-name-crate-resolution
  title: Detect root package name from Cargo.toml or directory identity and map pkg_name identifiers to src in Rust::normalize_import
  status: completed
  evidence: Cargo identity from existing one-pass admitted manifest parse. Full lib92/languages263/acdd115/mcp91 PASS; root normalization matrix covers underscore/hyphen, lib.name, supported/default explicit lib layout, missing/malformed/nested/binary-only/custom-path/autolib guards; independent real CLI layout probe7/7 at /tmp/forge-049-implementation-20261019/cargo-final-layout-probe.json.
- subtask_ref: integration-test-graph-edge-emission
  title: Verify directed edge emission from tests/ call sites to src/ symbols when using crate package imports
  status: completed
  evidence: Persisted directed integration test -> library calls pass ordinary/alias/module/glob/root-symbol cases; cold/delta parity after source edits and Cargo rename invalidation pass. Complete languages263 PASS, lib92 PASS.
- subtask_ref: seam-connectivity-contract-gate-tests
  title: Add seam tests proving integration-test contract gate passage without TASK_SEAM_DISCONNECTED
  status: completed
  evidence: Real production ACDD contract seam test passes via Cargo package import without TASK_SEAM_DISCONNECTED. Complete acdd115 PASS; existing boundary target test now requires exact newly connected external covering target while retaining strict canonical commands/caller admission. Strict Clippy + total561 tests PASS0failed0ignored; H/cargo-import-build-{clippy,targets}.log.
receipt:
  commit: 8971a164c6c43799396d25f0da0ac72c2bc4b3bf
  contract_revision: 2
  passed_at: 2026-10-19T23:32:57.033742001+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test languages --test acdd --test mcp
      exit_code: 0
      tests_passed: 561
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS. Reviewed full base5914910 scoped diff and all staged/unstaged/untracked status: six admitted files plus authorized current task manifest revision/scope correction only. All six files match frozen64628f7 before/after independent gates; no profile/config/generated-source/credential changes. Ten neighboring task blocks remain bytewise unchanged. Every change implements root Cargo identity, import normalization, real seam proof or its admitted docs. Artifacts H=/tmp/forge-049-implementation-20261019: cargo-review-binding.json and cargo-review-inspect.json.'
        claims:
          applicable: true
          evidence: 'PASS. Traced both task invariants through one existing TOML parse -> workspace-root registry identity -> cached normalization -> workspace-relative module/symbol resolution. Package hyphen normalization and lib.name override work; wrong/missing/malformed/nested/binary-only metadata decline identity. Real persisted calls cover ordinary/alias/module/glob/root-symbol imports, source cold/delta parity and Cargo rename invalidation. Real TasksStore contract submission reaches build. F1 closed: fixture uses existing engine::pipeline::build and preexisting db::reader edge, preserving ADR0018. F2 red cargo-preflight-layout-probe.json retained; current real CLI cargo-final-layout-probe.json passes7/7, rejecting custom library and disabled implicit layout while retaining explicit standard/empty bare/single/double-quoted lib tables. Own strict Clippy exit0; complete cargo test --lib --test languages --test acdd --test mcp exit0,92+115+263+91=561 passed,0failed/ignored/filtered. cargo-review-gates.json and cargo-review-{clippy,targets}.log. No remaining findings.'
        concurrency:
          applicable: true
          evidence: PASS. New registry map is built once from the admitted manifest snapshot and read immutably during normalization; normalized imports remain cached by file/module before parallel resolution. No new transaction, claim mutation, lock, connection, receipt write or publication path. Existing manifest digest and semantic fingerprint fence stale Cargo identities; cold/delta and rename cases pass complete affected targets. ADR0002/0003/0010 publication and rollback behavior are unchanged. No new per-request filesystem/SQLite work; autolib check walks existing AST only. No benchmark or zero-measured-regression claim; existing user performance disposition449 remains unchanged.
        project_isolation:
          applicable: true
          evidence: PASS. Review claim5/contract2 is rooted in assigned049 worktree and distinct from accepted builder. Exact-W fresh CLI overview confirms worktree metadata, generation96e23a27,313 files,7108 nodes,zero parse errors. Root identities are keyed by primary/linked workspace; nearest Cargo manifest must equal that workspace root. New normalized imports are relative, so existing owner-workspace filter prevents cross-root resolution. Nested package cannot borrow root identity; malformed/missing/unrelated names decline mapping. Scanner root/symlink guards, profiles, task namespaces and database paths are unchanged. cargo-review-seam-graph.json shows actual direct registry collection calls; unresolved references are retained honestly.
        administration:
          applicable: true
          evidence: PASS. No schema migration, profile pin, gate proof/receipt policy, CLI/MCP dispatch, Git helper, global installation or protected milestone header changed. Full10 neighboring task definitions preserved; current manifest amendment is the admitted revision2 scope correction. ADR0009 classification owner and ADR0018 frozen edges preserved. Actual installed Forge skill was used by complete MCP suite without override. Independent worker codex-049-cargo-reviewer differs from codex-049-builder; MCP review claim succeeded. Existing Forge auto-commit/hook/candidate-parent/atomic receipt checks remain authoritative; delivery will use same reviewer with no manual commit or source edits, followed by landed receipt/HEAD/clean-status verification.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-RUST-ROOT-CRATE-IMPORT-NORMALIZATION: When Rust code (e.g. integration tests in tests/) imports the workspace crate by its Cargo package name (or normalized identifier with underscores replacing hyphens), Rust::normalize_import maps the root crate identifier directly to src/, resolving public module and symbol paths identically to internal crate:: imports.'
    - 'INV-INTEGRATION-TEST-SEAM-CONNECTIVITY: Integration test functions importing library symbols via use <pkg_name>::... produce directed graph call edges to the indexed library symbols, satisfying ACDD contract gate seam connectivity without failing TASK_SEAM_DISCONNECTED.'
    architectural_notes:
    - 'decisions: Contract accepted via MCP snapshot58e4518 (red registry unit exit101, direct indexed production collect calls); build claim4/revision2 active. Implementation uses one existing Cargo TOML visitor pass for dependencies and crate identity, stores borrowed-read root names per workspace, and normalizes Rust imports at existing NamespaceIndexes cache construction. Preserve actual Cargo library identity ([lib].name override), root library presence and nearest-manifest/workspace boundary; no arbitrary directory-name guess for missing/virtual/malformed metadata. Planned mutation matrix: missing normalized name => positive direct/alias/module/glob/root call edges fail; permissive prefix/no library => unrelated/prefix/binary-only negatives fail; stale snapshot => Cargo rename cold/delta parity fails; absolute-root-based identity => existing two-worktree regression fails. No per-import source/manifest read, no new global cache/schema/connections, no allocator/index-publication changes.'
    - 'decisions: Independent review closure at frozen64628f7/base5914910, claim5/contract2: all five contours PASS; strict all-target/all-feature Clippy0 plus complete lib92/acdd115/languages263/mcp91=561pass0fail0ignored0filtered. Proof H/cargo-review-evidence.json; gates/binding/logs H/cargo-review-*. F1 frozen-layer edge closed using existing pipeline/reader seams. F2 original red H/cargo-preflight-layout-probe.json retained; final real CLI H/cargo-final-layout-probe.json7/7 includes unsupported/disabled negatives and explicit standard/empty quoted-table positives. Six scoped bytes equal candidate and ten neighbor task blocks unchanged. No new benchmark; accepted user performance decision449 retained without zero-regression claim. Proceed same-reviewer automatic delivery, no source edits/manual duplicate commit.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: whole-branch-review-closure-and-documentation-instructions

```yaml
task_ref: whole-branch-review-closure-and-documentation-instructions
target: Close whole-branch findings 049-WR-1 through 049-WR-4, commit the authorized companion skill sync, and require a documentation agent through pre-handoff instructions
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- cargo-root-crate-import-resolution-and-seam-connectivity
scope:
- src/engine/tasks/workspaces.rs
- tests/acdd/milestones.rs
- AGENTS.md
- docs/reference/configuration.md
- docs/reference/languages.md
- docs/reference/mcp-tools.md
- docs/reference/tasks.md
- skills/README.md
- skills/contextunity-forge/SKILL.md
- skills/agent-instructions/
- skills/close-review-findings/
- skills/lean-ctx/
- skills/test-contract-seam/
- skills/test-suite-refactor/
invariants:
- 'INV-MANIFEST-FAIL-CLOSED: Configured custom milestone directories retain malformed numbered candidates for authoritative sync rejection; missing frontmatter, invalid YAML, and missing string IDs never become a successful empty bulk sync. Preserve valid declared-project deduplication and README/empty-directory exclusions.'
- 'INV-DOCUMENTATION-TRUTH: Manifest admission and implicit worktree routing documentation describe the existing runtime boundaries; provide a concrete external review-authority prerequisite installation route without adding a sixth bundled companion.'
- 'INV-PREHANDOFF-DOCUMENTATION-INSTRUCTIONS: Before handoff, the coordinator launches a documentation agent using agent-instructions against the final milestone diff; the agent updates admitted documentation, instructions, skill examples and invariant descriptions, records its scan and remaining decision conflicts on the milestone blackboard, and returns edits for independent review before final verification. Enforce this through instructions only.'
- 'INV-BOUNDED-REVIEW-CLOSURE: Preserve all accepted task receipts, lifecycle gates, configuration-presence routing guards, cold/delta/MCP hot paths, allocator trims, and the exact five authorized companion trees. No automatic agent launcher, milestone gates, runtime documentation receipt requirement, or optional ADR redesign.'
subtasks:
- subtask_ref: malformed-custom-directory-sync
  title: Retain malformed numbered candidates for authoritative parsing and prove bulk/direct rejection plus valid and README controls through real lifecycle and CLI
  status: ready
  evidence: null
- subtask_ref: reference-and-skill-prerequisite-reconciliation
  title: Close WR2-WR4 with an external prerequisite route and accurate manifest and worktree selection boundaries
  status: ready
  evidence: null
- subtask_ref: documentation-agent-instructions-and-companion-sync
  title: Add the instruction-level pre-handoff documentation scan and include the five authorized companion skill updates and active support resources
  status: ready
  evidence: null
status: completed
receipt:
  commit: 3fa51ce9ffc41f795a4952e21891513f5696ae96
  contract_revision: 1
  passed_at: 2026-10-20T01:42:57.171357649+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 811
      tests_failed: 0
      log: H/wr-closure-final-{clippy,all-targets}.log and H/wr1-nested-{red,green}.log; H=/tmp/forge-049-implementation-20261019. CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=H/closure-global-skill isolates package validation; actual installed global skill not synchronized in this task.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS. Frozen build aa6781dcf536d2611560207ae4fde343f1f43cc0/tree3b7b91a56308c9f95450fe6c40aac8ce06de9f6f: all25 scoped snapshot files match live bytes and executable modes after independent tests. Stored contract1 scope includes lifecycle.rs and tasks/mod.rs via admitted extend_scope. Manifest diff only appends the two authorized task contracts; protected header and all11 accepted task blocks are byte-identical to HEAD8971a164. Exact-five companion14-file binding8c68052a8b6e3851df5643a5a365f3041cd3fb192e700615b8928b007ac17590 preserved. No uncontracted source or optional ADR implementation.'
        claims:
          applicable: true
          evidence: 'PASS. #503 WR1 and supplemental#512 close through one shared recursive numbered_manifests owner used by classification and bulk enumeration; missing/invalid headers remain undeclared until authoritative parser rejects with file path. Actual engine+CLI regression covers direct/archive/nested x4 invalid headers with valid controls and unchanged persisted details. Initial and nested red101 -> green0 retained. Independent cargo test --test acdd:116pass0fail0ignored0filtered, exit0; H/wr-closure-independent-acdd.json/log SHA cf23348212817884c725635ce96f988183e5e1fe1bde06aa80f5878906059d79. Accepted builder strictClippy0 + all-targets811pass0fail1existingignored. WR2 external prerequisite route matches observed origin/main path; WR3 canonical configuration/languages discovery boundary and WR4 config-presence routing match live enforcement/tests. AGENTS owns instruction-only documentation agent, packaged Forge skill routes to it; independent edit review precedes final verification.'
        concurrency:
          applicable: true
          evidence: PASS. Shared traversal preserves file-type/no-symlink iteration and sorted candidate enumeration; no eager full milestone parsing, cache, SQL, schema, transaction, locking or claim-routing change. Invalid file parsing precedes that file's task mutation, with persisted-details preservation exercised in actual engine/CLI tests. No new runtime agent launcher, milestone gate or documentation-receipt enforcement. Cold/delta/warm-MCP paths and all allocator trims untouched; existing user performance acceptance449 unchanged, no benchmark or measured-zero-regression claim.
        project_isolation:
          applicable: true
          evidence: PASS. Explicit-root Forge discovery confirmed W, source inspection and real isolated Git/SQLite/CLI tests cover Registry -> candidate retention -> authoritative sync. Graph coverage gaps for qualified/manage reexports were not treated as absence proof; actual red/green regression validates the production route. Shared walker skips symlink entries and numbered-file filter preserves README/empty exclusions and existing declared-project/physical-file dedup tests. Independent tests ran serially with CARGO_TARGET_DIR=H/cargo-target and CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=H/closure-global-skill. This proves the packaged skill fixture; actual installed global Forge skill was not synchronized.
        administration:
          applicable: true
          evidence: PASS. Stable independent worker codex-049-whole-branch-reviewer differs from builder codex-049-builder; reviewclaim4/contract1. All25 snapshot bytes+modes bound; source/docs/receipts not edited by reviewer. ADR0014 fail-closed authority restored, ADR0009/0010 unchanged, optional prior ADR proposals deferred; only future admitted handoff task authorizes ADR0015 replacement and has no implementation here. Exactfive companions retained with all active support files, no sixth bundled skill; prerequisite install path confirmed in shared repo origin/main (review-authority blob b115858a5837739444ae42fb4e93fe4942a5a6d4). Global installation, remote publication, handoff and next task implementation not performed. Forge automatic delivery/receipt/commit only.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-MANIFEST-FAIL-CLOSED: Configured custom milestone directories retain malformed numbered candidates for authoritative sync rejection; missing frontmatter, invalid YAML, and missing string IDs never become a successful empty bulk sync. Preserve valid declared-project deduplication and README/empty-directory exclusions.'
    - 'INV-DOCUMENTATION-TRUTH: Manifest admission and implicit worktree routing documentation describe the existing runtime boundaries; provide a concrete external review-authority prerequisite installation route without adding a sixth bundled companion.'
    - 'INV-PREHANDOFF-DOCUMENTATION-INSTRUCTIONS: Before handoff, the coordinator launches a documentation agent using agent-instructions against the final milestone diff; the agent updates admitted documentation, instructions, skill examples and invariant descriptions, records its scan and remaining decision conflicts on the milestone blackboard, and returns edits for independent review before final verification. Enforce this through instructions only.'
    - 'INV-BOUNDED-REVIEW-CLOSURE: Preserve all accepted task receipts, lifecycle gates, configuration-presence routing guards, cold/delta/MCP hot paths, allocator trims, and the exact five authorized companion trees. No automatic agent launcher, milestone gates, runtime documentation receipt requirement, or optional ADR redesign.'
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

Review authority is milestone blackboard 503 and the user's explicit request to
repair the findings, commit current changes, and add the documentation agent at
instruction level. WR1 is a pre-existing missed regression in workspace directory
classification: bulk/direct sync -> Registry::load -> numbered header discovery
-> retained directory -> authoritative manifest parser -> rejection before that
file's task mutation. Retain malformed candidates as undeclared during discovery;
do not turn unrelated registry reads into eager full manifest validation. Real
bulk/direct regressions must cover missing frontmatter, malformed YAML, missing or
non-string ID, custom and archived paths, valid declared projects, and README-only
directories. Reverting the candidate-retention change must fail those negatives.
WR2 is a prerequisite distribution defect introduced by the companion sync;
WR3/WR4 are missed documentation reconciliation with already enforced runtime.
They need direct source/reference validation, not wording-only tests. The
documentation-agent scan supplements each task's existing documentation duty;
architecture decision changes still require owner approval and source defects
return to their owning task. Store closure evidence on the milestone blackboard.

### task: content-bound-handoff-and-rebase-independent-evidence

```yaml
task_ref: content-bound-handoff-and-rebase-independent-evidence
target: Accept milestone handoff from immutable task proofs and content-bound final integration evidence, without requiring preserved task commit identities or first-parent history
proof_policy: seam-test-first
contract_revision: 3
depends_on:
- whole-branch-review-closure-and-documentation-instructions
- manifest-discovery-symmetry-and-admission-loop-elimination
- database-collision-and-overwrite-guard
- subproject-workspace-deduplication
- mcp-idle-timeout-and-memory-compaction
- greenfield-seam-guidance-and-diagnostics
- mcp-worktree-context-and-task-claim-resolution
- upstream-aware-handoff-provenance-and-safe-diagnostics
- build-gate-deletion-scope-and-file-retirement
- audit-invariant-remediation
- unified-task-and-subtask-status-lifecycle
- cargo-root-crate-import-resolution-and-seam-connectivity
scope:
- src/engine/milestones/handoff.rs
- src/engine/milestones/integration.rs
- src/engine/milestones/mod.rs
- src/engine/tasks/git_snapshot.rs
- src/engine/tasks/mod.rs
- src/core/tasks/mod.rs
- src/core/git.rs
- src/db/tasks_store/mod.rs
- src/cli/milestone.rs
- tests/acdd/milestones.rs
- tests/acdd/tasks.rs
- tests/acdd/support.rs
- README.md
- AGENTS.md
- TESTS.md
- docs/adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md
- docs/reference/tasks.md
- docs/reference/cli.md
- docs/reference/configuration.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/agents.md.example
invariants:
- 'INV-HANDOFF-IMMUTABLE-PROOF: Accepted task contract revisions, candidate content, gate proofs, reviewer identities, and decisions remain immutable; updating a branch commit pointer never changes or recreates accepted evidence.'
- 'INV-HANDOFF-CONTENT-IDENTITY: Rebase, cherry-pick, merge, amend, and squash do not invalidate evidence solely because commit IDs, parents, timestamps, or task-to-commit cardinality change. Handoff requires neither the original task baseline on first-parent history nor one landed commit per task.'
- 'INV-HANDOFF-FINAL-INTEGRATION: Handoff requires a passing independent integration review and repository verification bound to the same final implementation content, selected target-base content, milestone identity, and admitted task authority. New content, changed authority, missing proof, foreign proof, or a failed review or verification rejects before archive or store mutation.'
- 'INV-HANDOFF-OPTIONAL-COMMIT-POINTER: Task branch commit SHAs are diagnostic navigation metadata. Resolve and refresh uniquely identifiable pointers automatically; absent, obsolete, unavailable, or many-to-one branch pointers do not invalidate otherwise valid content evidence. Respect record_task_commits and commit_tracking without weakening proof checks.'
- 'INV-HANDOFF-DURABLE-CONTENT: Content objects needed to verify accepted task and final integration evidence remain reachable independently of rewritten branch history and Git reflog expiry; snapshot cleanup must not discard the only durable proof.'
- 'INV-HANDOFF-BOUNDED-CHANGE: Keep task gate order, task candidate immutability, review independence, delivery hooks and rollback, database isolation, indexing optimizations, and all allocator trims. Add no milestone gate state machine, agent launcher, test runner, per-request I/O, or indexing work.'
subtasks:
- subtask_ref: immutable-content-and-optional-commit-pointers
  title: Separate accepted task content evidence from mutable branch pointers and preserve required evidence objects across rebases and handoff
  status: completed
  evidence: 'Verified candidate20e0b65: core/git.rs retain_proof_commit, engine/tasks/mod.rs proof pinning, milestones/handoff.rs accepted provenance and optional pointer mapping. Real CLI rewrite/GC/recording-mode matrix passed in complete cargo test --test acdd116/0; cargo test --lib91/0. Accepted receipt proofs remain immutable, pointers independent.'
- subtask_ref: final-integration-evidence-and-handoff-validation
  title: Validate a minimal externally supplied final integration review and verification record against current content and task authority instead of reconstructing each task landing
  status: completed
  evidence: 'Verified candidate20e0b65: typed IntegrationEvidence and HandoffBinding in integration.rs; handoff.rs binding validates final/base content, canonical authority and validated commitless receipts before publication. Real CLI stale/foreign/receipt-context/reviewer/verification/dirty/untracked negatives passed; content/receipt mutation failures retained in blackboard519. Complete ACDD116/0, lib91/0.'
- subtask_ref: real-git-rewrite-and-negative-proof
  title: Prove rebase and squash acceptance and stale or forged evidence rejection through the real CLI, Git repository, task store, and archive
  status: completed
  evidence: Verified candidate20e0b65 through complete independent cargo test --test acdd116/0 and cargo test --lib91/0, no filters/ignored. handoff_preserves_proofs_across_reviewed_history_rewrites drives real Git/CLI/SQLite including rebase, cherry-pick, merge, amend, squash, conflict resolution, reflog expiry/GC and pointer recording modes; negatives preserve manifest/task state. Logs H/handoff-independent-{acdd,lib}.log.
- subtask_ref: adr-instructions-and-reference-alignment
  title: Align ADR-0015, Git instructions, CLI and task references, verification instructions, and the packaged Forge skill with the accepted handoff model
  status: completed
  evidence: Independent source/doc review of candidate20e0b65 confirms authorized ADR0015 amendment, AGENTS instruction-only documentation scan, TESTS verification record, CLI/task/config references and packaged Forge skill/example alignment. Documentation scan520 binds final diff; no new runtime agent/gate/test runner or indexing/MCP hot-path work. All changed paths admitted in contract3 and extension.
status: completed
receipt:
  commit: ff4226d40233e6ad8af7acc505462457f87a577c
  contract_revision: 3
  passed_at: 2026-10-20T02:47:16.688251015+00:00
  evidence:
    command_proof:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 810
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: all changed/staged/untracked feature paths match contract3 and admitted subtasks.rs extension; milestone task metadata changes are authorized. All22 scoped snapshot blobs+modes match candidate20e0b65; HEAD3fa unchanged and tracked diff SHAa81086676e382677bfa8e6d44e4a2d61543f207fea764c9d4dae168d320f4183 matches documentation scan520. No protected profile/config or external installation edits.'
        claims:
          applicable: true
          evidence: 'PASS: CLI typed evidence reaches handoff current identity/content/base/authority checks, accepted claim/gate provenance and commitless receipt binding, optional pointer mapping and durable proof refs. Full independent cargo test --test acdd exit0 116passed0failed0ignored0filtered and cargo test --lib exit0 91passed0failed0ignored0filtered, serial. Real CLI rewrite+GC cases and stale/foreign/reviewer/verification/dirty negatives pass; content/receipt mutants fail as blackboard519 records. Canonical project alias regressions repaired without weakening positives. ADR0015 amendment and canonical docs align; no speculative expansion.'
        concurrency:
          applicable: true
          evidence: 'PASS: reviewed current immediate transaction/revision/claim fencing, candidate/baseline comparisons, prepublication clean-tree recheck, current receipt proof guard in store.handoff_milestone, and existing archive/store rollback. New proof-ref failures flow through existing delivery rollback. Complete ACDD target passes including lifecycle/delivery rollback cases; no lock ordering or schema changes introduced.'
        project_isolation:
          applicable: true
          evidence: 'PASS: explicit root CLI and fresh graph overview bind this milestone worktree (generation23dbf9961957a0ed392c5d604bbf7c101a64e267d52b4c63d804816f353abf4c;314files;0parseerrors). Canonical Snapshot.project is retained; namespaces and existing scope/path guards remain enforced. Existing explicit/default aliases and configured-owner lifecycle positives pass in full ACDD. Tests use isolated global skill fixture H/closure-global-skill; no installed-global sync.'
        administration:
          applicable: true
          evidence: 'PASS: contract3 preserves profile/gate order, reviewer independence, task auto_commit/hooks/rollback and historic receipts; no migration, milestone gate, agent launcher, verification runner or indexing/MCP hot-path work added. Documentation-agent instruction scan520 matches unchanged frozen content. Four completed subtask outcomes were evidence-recorded through Forge SQLite-only subtask_update after proof; no frozen file edit, tracked diff and22 blobs+modes unchanged. New proof refs preserve accepted objects independently of branch history; no benchmark or measured performance claim.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ADMISSION-DETERMINISM: Dependency manifest collection across cold build, incremental delta, and MCP source admission must yield identical path sets and digests for unchanged workspace state, eliminating false-positive rebuild loops.'
    - 'INV-DATABASE-SEPARATION: Task tracking databases (tasks.sqlite) and code map indices (code-map.sqlite) are mutually exclusive; attempts to target a task database for code-map output or open a code-map database as task store fail closed with TASK_DATABASE_COLLISION.'
    - 'INV-WORKSPACE-UNIQUENESS: Subproject milestone resolution honors declared_projects frontmatter and configured owner mappings, preventing duplicate phantom workspaces from conflicting on milestone references.'
    - 'INV-DAEMON-RESOURCE-BOUND: MCP servers preserve live connections regardless of inactivity, terminate on Unix when their launching parent disappears, and trim free heap pages after batch operations without removing the allocator trims delivered in milestone 031.'
    - 'INV-GREENFIELD-SEAM-ACTIONABILITY: Greenfield contract validation failure for uncreated scope files outputs deterministic remediation guidance with file paths and stub requirements.'
    - 'INV-PERFORMANCE-PRESERVATION: Preserve the indexing optimizations delivered on main; paired release measurements on an identical frozen corpus must show no cold-build, delta, or warm MCP latency regression before milestone delivery.'
    - 'INV-HANDOFF-IMMUTABLE-PROOF: Accepted task contract revisions, candidate content, gate proofs, reviewer identities, and decisions remain immutable; updating a branch commit pointer never changes or recreates accepted evidence.'
    - 'INV-HANDOFF-CONTENT-IDENTITY: Rebase, cherry-pick, merge, amend, and squash do not invalidate evidence solely because commit IDs, parents, timestamps, or task-to-commit cardinality change. Handoff requires neither the original task baseline on first-parent history nor one landed commit per task.'
    - 'INV-HANDOFF-FINAL-INTEGRATION: Handoff requires a passing independent integration review and repository verification bound to the same final implementation content, selected target-base content, milestone identity, and admitted task authority. New content, changed authority, missing proof, foreign proof, or a failed review or verification rejects before archive or store mutation.'
    - 'INV-HANDOFF-OPTIONAL-COMMIT-POINTER: Task branch commit SHAs are diagnostic navigation metadata. Resolve and refresh uniquely identifiable pointers automatically; absent, obsolete, unavailable, or many-to-one branch pointers do not invalidate otherwise valid content evidence. Respect record_task_commits and commit_tracking without weakening proof checks.'
    - 'INV-HANDOFF-DURABLE-CONTENT: Content objects needed to verify accepted task and final integration evidence remain reachable independently of rewritten branch history and Git reflog expiry; snapshot cleanup must not discard the only durable proof.'
    - 'INV-HANDOFF-BOUNDED-CHANGE: Keep task gate order, task candidate immutability, review independence, delivery hooks and rollback, database isolation, indexing optimizations, and all allocator trims. Add no milestone gate state machine, agent launcher, test runner, per-request I/O, or indexing work.'
    architectural_notes:
    - 'decisions: {"review": "independent task review PASS", "candidate": "20e0b65c158ef0ba650bab5ec75b27222b158cef", "baseline": "3fa51ce9ffc41f795a4952e21891513f5696ae96", "contract_revision": 3, "review_claim_revision": 8, "gates": "cargo test --test acdd116/0; cargo test --lib91/0; both exit0 no ignored/filters, serial", "binding": "22candidate blobs+modes match; tracked diffSHAa81086676e382677bfa8e6d44e4a2d61543f207fea764c9d4dae168d320f4183", "findings": "No unresolved findings; canonical project/fixture alias/commitless receipt binding repaired; content+receipt mutants fail (#519).", "subtasks": "All4 evidence-completed through operational Forge updates after independent proof; no file edits; frozen22 blobs+modes/diff unchanged.", "coverage": "All5 review contours; ADR0015 authorized amendment; CLI/task/config/TESTS/AGENTS/packaged skill aligned, docs scan520; no extra schema/gate/agent/test runner/indexing/warm request work.", "artifact": "/tmp/forge-049-implementation-20261019/handoff-independent-review-result.json"}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Implementation boundary and acceptance

The production path is CLI handoff input -> milestone handoff owner -> accepted
task evidence and final content checks -> existing task-store/archive publication.
The coordinator supplies the independent review and runs repository verification;
Forge validates their structured record and does not launch agents or run tests.
Use a small typed record supplied through a CLI evidence-file option, with its
accepted contents retained in the archived handoff receipt. Do not add a generic
workflow engine or a milestone claim/review/deliver lifecycle.

Bind final evidence to repository/project/milestone identity, the current task
contract and accepted-proof digests, an explicit target-base content identity,
and a deterministic final implementation content identity. Include reviewer and
builder identities, review decision and scope/contract findings, and the exact
verification command, exit status, and test counts. Require the reviewer to be
independent from the milestone's builders. Use repository-relative paths, file
contents and modes, additions, and deletions for content identity. Exclude only
generated receipt/status/handoff metadata through an explicit canonical
projection; retain manifest contracts, invariants, configuration, instructions,
tests, and documentation in the binding. Reject dirty implementation files and
untracked deliverables rather than silently verifying a different tree. Resolve
Git selectors at intake; mutable ref names and commit messages are not proof.

Final review accepts the combined delta against the selected target base,
including clean upstream integration and reviewed conflict resolutions. It must
account for admitted task outcomes and flag unadmitted changes. A conflict
resolution changes the content identity and needs a fresh final review and
verification; it does not reopen every completed task. Replace history-based
landing and later-scope reconstruction rather than adding a permissive bypass.
Preserve the earlier upstream task's accepted receipts as historical evidence;
this task changes the current acceptance mechanism under user authorization.

Keep accepted candidate snapshots/proofs distinct from optional branch pointers.
Automatically refresh pointers where a unique content-equivalent mapping exists;
otherwise omit unresolved navigation metadata without changing accepted proof.
Allow multiple tasks to share a squashed commit. Keep auto_commit behavior and
the normal per-task commit convention; remove any handoff requirement to repack
or preserve one commit per task. Keep historical archived receipts readable
without rewriting them, and preserve the existing archive/store rollback path.
This task's delivery must leave this active milestone usable with the new
handoff procedure without manual edits to its eleven accepted task receipts.

Use production CLI handoff cases in tests/acdd/milestones.rs, with real Git and
SQLite fixtures. Cover unchanged-content rewritten SHAs, rebase onto a changed
main touching the same file, cherry-pick, merge, multi-task squash, optional
commit-recording modes, and missing old branch objects while protected content
evidence remains available. Cover reviewed conflict resolutions and ensure a
post-review byte, mode, deletion, configuration, documentation, contract, or
target-base change rejects. Missing/corrupt/foreign proofs, a builder submitting
the integration review, failed verification, incomplete tasks, and out-of-scope
changes must reject without publishing an archive or mutating task proofs.
Restoring the old first-parent check must fail a positive rewrite case; disabling
content binding must fail a stale-evidence negative case. Verify proof retention
with reflogs expired and unreachable objects pruned in a disposable fixture.

Run strict Clippy and the complete affected lib/ACDD targets as required by
TESTS.md; run its full completion command before actual milestone handoff.
Limit production changes to the handoff/CLI/evidence path; add no work to cold
build, delta, or warm MCP requests and make no new benchmark acceptance claim.
Update documentation for this behavior in the same task. Follow the repository's
instruction-level documentation-agent scan before actual handoff. Runtime
documentation-agent launching, documentation-specific handoff enforcement, automatic
test execution/count collection, and benchmark tooling remain out of scope.

## Delivered task commits

| Task | Delivery commit |
| --- | --- |
| Manifest discovery symmetry | `56eb0cdca04b3033b2bd5135991c2b35fa81361d` |
| Database collision guard | `a6e379eb64676b4c52686133dfc143861e2b6d56` |
| Subproject workspace deduplication | `22e64ad8922b072bb28ae0ae5f18bdd40efd8833` |
| MCP lifecycle and memory reclamation | `f6cfe1533bf4bd654410afe4f6a6dddb5a7da6e0` |
| Greenfield seam guidance | `fef1545e1b5217eccb269b5b8dfb8cba8c31fffd` |
| MCP worktree context | `472fb65538e2d9c4e79153b3402b82a2f55069da` |
| Reviewed upstream handoff provenance | `9ec8e696c89bcd580c090b07c656262798952054` |
| Build-gate file retirement | `c80b6239e4309ca90c8d1cb030c1c094847bab65` |

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
using nearest rank over a fixed request count. Retain unsuccessful measurements
and host limitations alongside the successful runs. Functional equality and
source preservation checks support these measurements but do not substitute
for them.

The user accepts the measured timing differences and directs continued delivery
without further benchmarks; milestone blackboard decision `449` records this
acceptance and supersedes the earlier performance-only blockers. This acceptance
governs the performance requirement above and in the retained task receipts; it
does not assert zero observed timing differences. Existing upstream indexing
optimizations and allocator trims remain preserved.

The full handoff gate remains the exact command in `TESTS.md`; the milestone is
implemented and reviewed on its branch before any separate installation or
landing operation.

Final verification at delivery commit `c80b6239e4309ca90c8d1cb030c1c094847bab65`
passes commitment integrity (16 tests), strict Clippy, and all targets (798
passed, zero failed). The existing scanner filesystem stress test remains
ignored by default because it creates more than 100,000 files. The verification
receipt and complete logs are retained in
`/tmp/forge-049-implementation-20261019/handoff-verification-1792442850983785671/`.
