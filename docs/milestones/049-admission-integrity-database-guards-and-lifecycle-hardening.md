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
target: "Preserve idle MCP connections, terminate servers after parent loss, and retain indexing memory reclamation without duplicate MCP trimming"
proof_policy: seam-test-first
contract_revision: 4
scope:
  - src/core/sys.rs
  - src/mcp/server.rs
  - src/mcp/tools.rs
  - src/engine/pipeline.rs
  - tests/common/mcp_client.rs
  - tests/mcp/freshness/stdio_and_response.rs
  - docs/reference/mcp-setup.md
  - docs/architecture/performance.md
status: ready
subtasks:
  - subtask_ref: glibc-malloc-trim-binding
    title: "Implement trim_memory() wrapping malloc_trim(0) on Unix glibc to return unused heap arenas to the operating system"
    status: completed
  - subtask_ref: mcp-parent-lifecycle
    title: "Remove inactivity shutdown; prove connected server exit after parent loss with transport held open, and preserve SIGHUP session continuity"
    status: completed
  - subtask_ref: post-tool-memory-trimming
    title: "Invoke trim_memory() after tool dispatch responses in responding and after cold build / delta index passes"
    status: completed
```

### task: greenfield-seam-guidance-and-diagnostics

```yaml
task_ref: greenfield-seam-guidance-and-diagnostics
target: "Provide actionable error diagnostics and contract author workflow guidance for greenfield features where admitted scope files do not yet exist on disk"
proof_policy: seam-test-first
contract_revision: 4
scope:
  - src/engine/tasks/gates.rs
  - src/core/tasks/acdd.default.yaml
  - docs/reference/tasks.md
  - skills/contextunity-forge/SKILL.md
  - skills/contextunity-forge/references/acdd_profile.yaml.example
  - tests/acdd/tasks.rs
status: ready
subtasks:
  - subtask_ref: validate-seam-missing-scope-diagnostic
    title: "Detect uncreated scope files in validate_seam_reachability and output targeted remediation instructions for scaffolding minimal stubs and refreshing the index"
    status: completed
  - subtask_ref: acdd-profile-greenfield-guidance
    title: "Update contract_author role guidance and steps in acdd.default.yaml to mandate minimal file scaffolding for new modules in scope"
    status: completed
  - subtask_ref: reference-and-skill-alignment
    title: "Update docs/reference/tasks.md and skills/contextunity-forge/SKILL.md with greenfield seam reachability rules and proof policy alternatives"
    status: completed
```

### task: mcp-worktree-context-and-task-claim-resolution

```yaml
task_ref: mcp-worktree-context-and-task-claim-resolution
target: "Resolve explicit worktree paths, exact names or unique name suffixes; preserve a server-bound linked worktree and reject ambiguous automatic claim selection"
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
status: ready
subtasks:
  - subtask_ref: page-input-worktree-schema-and-selector-allowlist
    title: "Add worktree field to PageInput and allow worktree in rejected_selector_fields for all query and inspect tools"
    status: completed
  - subtask_ref: active-claim-worktree-discovery-and-resolution
    title: "Implement active_claim_worktree helper querying tasks.sqlite and resolve_target_root_and_db in server module"
    status: completed
  - subtask_ref: server-worktree-dispatch-and-auto-rebuild
    title: "Dispatch code navigation MCP tools with resolved worktree root and db in ServerHandler::call_tool with auto-indexing"
    status: completed
  - subtask_ref: worktree-context-integration-tests
    title: "Add integration tests in tests/mcp/context.rs proving automatic worktree discovery and explicit worktree query execution"
    status: completed
```

## Scope boundary and related ACDD work

Upstream delivery provenance at handoff, automatic verification accounting, and
reuse of verification receipts belong to a successor of milestones 017/019.
The [handoff plan](../plans/acdd-handoff-upstream-provenance-and-verification.md)
records the observed 031/052 overlap, safe acceptance requirements, scope hygiene,
and verification options. They do not add implementation tasks to 049.

Existing subtask completion labels describe the recovered WIP. They are not
accepted task deliveries; the milestone remains subject to its ACDD gates.

## Acceptance and performance proof

The builder implements all six tasks; an independent reviewer checks their
contracts, correctness, concurrency, isolation, administration, and production-seam
proof before delivery. Task ordering follows Forge's shared-scope dependencies.
The initial restored implementation is retained as a backup; each delivery
contains only its admitted behavior and subsequent tasks may refine shared files.

Use release binaries from the unchanged `main` base `9f358ec` and the candidate.
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

