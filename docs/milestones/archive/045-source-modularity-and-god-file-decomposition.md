---
id: m-source-modularity-and-god-file-decomposition
title: Source code modularity, god-file decomposition, and canonical helper extraction
doc_type: contract
status: completed
depends_on:
- m-installer-distribution-and-acdd-onboarding
owners:
- src/core/
- src/db/
- src/engine/
- src/engine/languages/
- src/engine/linker/
- src/engine/tasks/
invariants:
- 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli. Direct rusqlite use in src/core/commitments stays.'
- 'INV-FROZEN-ENGINE-DB-CYCLE: The legacy mutual import cycle between src/engine and src/db accepts no new edge. Ports that would break it belong to a later milestone.'
- 'INV-BALANCED-MODULARITY: Split only along mixed responsibilities. Cohesive multi-pass pipeline functions have no line-count cap.'
- 'INV-DEDUPLICATE-PROVEN-PATTERNS: Collapse a repeated block only where the control flow matches. Distinct rules stay separate.'
- 'INV-CANONICAL-HELPERS: Ordinary Git queries, atomic file writes, and cgroup memory detection have one owner in src/core.'
- 'INV-COMPAT-REEXPORTS: Moved scanner helpers keep pub use re-exports at engine::scanner. Pure receipt checks stay at core::tasks::gates. core does not re-export engine.'
- 'INV-ZERO-SEMANTIC-REGRESSION: Language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results.'
related_plans: []
started_at: 2026-10-10T21:27:11+00:00
handoff:
  completed_at: 2026-10-11T00:37:00.463497322+00:00
  duration: 3h 9m
  commit: fb424febdff544b74d3bf134dd01fdf6e312e877
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 655
    tests_failed: 0
---

# Source code modularity, god-file decomposition, and canonical helper extraction

## Outcome and purpose

Follow [ADR 0018](../adr/0018-layer-dependency-boundaries-and-core-leaf-isolation.md). The task titles are the contract.

Task 1 and task 2 share no files. Task 3 follows task 2. Tasks 4 and 5 follow task 1.

## Tasks in this milestone

### task: core-leaf-purification-and-canonical-primitives

```yaml
task_ref: core-leaf-purification-and-canonical-primitives
target: Make src/core a leaf by moving submit and delivery-tree checks to engine::tasks, keeping pure receipt checks in core::tasks, and adding core::fs, core::git, and core::sys
proof_policy: seam-test-first
contract_revision: 7
scope:
- src/core/fs.rs
- src/core/git.rs
- src/core/sys.rs
- src/core/adapter.rs
- src/core/mod.rs
- src/core/tasks/gates.rs
- src/core/tasks/mod.rs
- src/engine/tasks.rs
- src/engine/tasks/gates.rs
- src/engine/scanner.rs
- tests/core/scanner_guard_limits.rs
subtasks:
- subtask_ref: split-gates-operational-execution
  title: Move TasksStore::submit, submit_with_pre_persist, accepted, validate_delivery_commit, delivery_tree_entries, ProvisionalReceiptRollback, and the receipt-side commit_tree_and_parent into src/engine/tasks/gates.rs. Keep REVIEW_CONTOURS, Evidence, validate_durable_receipt, validate_durable_receipt_with_profile, and their proof checkers in src/core/tasks/gates.rs
  status: completed
  evidence: Moved submit, receipt transaction handling, delivery tree validation, and rollback into `src/engine/tasks/gates.rs`; `src/core/tasks/gates.rs` retains `Evidence`, gate proof validation, and durable receipt checks. Verified with `cargo test --manifest-path /tmp/contextunity-045-core-leaf-purification-and-canonical-primitives/Cargo.toml --test core` (31 passed, 0 failed, 1 ignored) and strict all-target Clippy.
- subtask_ref: extract-canonical-git-primitives
  title: Implement crate::core::git (git_command, git_stdout, git_status, git_commit_exists) and migrate the 4 ordinary Git calls that move with delivery verification
  status: completed
  evidence: Added `core::git::{git_command, git_stdout, git_status, git_commit_exists}` and routed the four delivery verification Git commands through `git_command`. Verified with the core integration suite and `cargo clippy --manifest-path /tmp/contextunity-045-core-leaf-purification-and-canonical-primitives/Cargo.toml --all-targets --all-features -- -D warnings`.
- subtask_ref: unify-atomic-fs-primitives
  title: Move canonical_root, checked_child, and safe_relative into core::fs. Add atomic_replace beside atomic_write through commit_atomic_temp. Point ProvisionalReceiptRollback at core::fs::atomic_replace and leave milestones::atomic_replace for task 5
  status: completed
  evidence: Moved canonical root, safe relative path, and checked child helpers to `core::fs`; added `atomic_replace` using shared `commit_atomic_temp`; moved receipt rollback to `core::fs::atomic_replace`; kept `engine::scanner` helper re-exports. Verified with the core integration suite and strict all-target Clippy.
- subtask_ref: extract-system-memory-and-cgroup-detection
  title: Move cgroup v1/v2 parsing, available_memory_bytes, and memory_budget_bytes from engine::scanner into core::sys, and pub use those public functions from engine::scanner
  status: completed
  evidence: Moved host/cgroup memory parsing and memory budget calculation to `core::sys`, re-exported public memory helpers from `engine::scanner`, and retained cgroup unit tests. Verified with `cargo test --manifest-path /tmp/contextunity-045-core-leaf-purification-and-canonical-primitives/Cargo.toml --lib` (37 passed) and the core integration suite.
- subtask_ref: verify-core-leaf-invariants
  title: Verify src/core imports no engine, db, mcp, or cli module, commitments still use rusqlite, and cargo test --test core passes
  status: completed
  evidence: Confirmed `src/core` has no `crate::engine`, `crate::db`, `crate::mcp`, or `crate::cli` references; core commitments still directly use rusqlite. `cargo test --manifest-path /tmp/contextunity-045-core-leaf-purification-and-canonical-primitives/Cargo.toml --test core` passed 31 tests (1 ignored), `cargo test --manifest-path /tmp/contextunity-045-core-leaf-purification-and-canonical-primitives/Cargo.toml --lib` passed 37 tests, and strict all-target Clippy passed.
status: completed
receipt:
  commit: 36b009a3bbb22bbf657ed1fe850aebd8b4e05e4c
  contract_revision: 7
  passed_at: 2026-10-11T00:02:57.417046638+00:00
  evidence:
    command_proof:
      command: cargo test --test core
      exit_code: 0
      tests_passed: 31
      tests_failed: 0
      log: 'Focused seam: cargo test --test core scanner_memory_helpers_reexport_the_core_system_contract passed (1 passed). Full core suite passed 31, failed 0, ignored 1. cargo clippy --all-targets --all-features -- -D warnings passed. Mutation check: replacing the direct scanner re-export with a forwarding wrapper made the seam fail (exit 101); the source was restored.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed pinned candidate 8e068fb against baseline 1cd4768d973454b79c0c6d79cb1398d572d7a122. The scoped patch changes only tests/core/scanner_guard_limits.rs (13 insertions, 10 deletions); the snapshot's included task artifacts are within the admitted path set. No profile/configuration, credential, generated-state, or neighboring-project paths are changed.
        claims:
          applicable: true
          evidence: The seam now compares function items with std::ptr::fn_addr_eq, then samples available memory once and budget once for positivity and floor/cap assertions, avoiding equality checks across volatile host-memory reads. The accepted baseline's src/engine/scanner/mod.rs still directly re-exports core::sys::{available_memory_bytes, memory_budget_bytes}; this patch does not alter that production file, so the final facade remains restored. Build evidence records the focused/core tests and strict all-target Clippy passing, plus a forwarding-wrapper mutation failing the seam (exit 101).
        concurrency:
          applicable: true
          evidence: The patch changes only the test and does not touch transaction, claim, receipt, Git, or filesystem publication logic. The replacement removes two time-separated dynamic memory samples from the re-export identity assertion; the remaining value checks each use one sample, so concurrent host/cgroup memory changes cannot cause the old cross-call equality failure.
        project_isolation:
          applicable: true
          evidence: The test exercises the public core and scanner function items in the same crate and introduces no workspace paths, database access, profile loading, or Git operations. The production module re-export is unchanged from the captured baseline.
        administration:
          applicable: true
          evidence: No schema, migration, profile, gate, repository configuration, or delivery behavior changes. Review identity differs from builder codex-build-memory-reexport-stability-20261011-r1. The accepted build evidence reports the focused seam, full core suite, strict Clippy, and wrapper-mutation failure; I did not rerun tests.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli. Direct rusqlite use in src/core/commitments stays.'
    - 'INV-FROZEN-ENGINE-DB-CYCLE: The legacy mutual import cycle between src/engine and src/db accepts no new edge. Ports that would break it belong to a later milestone.'
    - 'INV-BALANCED-MODULARITY: Split only along mixed responsibilities. Cohesive multi-pass pipeline functions have no line-count cap.'
    - 'INV-DEDUPLICATE-PROVEN-PATTERNS: Collapse a repeated block only where the control flow matches. Distinct rules stay separate.'
    - 'INV-CANONICAL-HELPERS: Ordinary Git queries, atomic file writes, and cgroup memory detection have one owner in src/core.'
    - 'INV-COMPAT-REEXPORTS: Moved scanner helpers keep pub use re-exports at engine::scanner. Pure receipt checks stay at core::tasks::gates. core does not re-export engine.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results.'
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

### task: deduplicate-proven-patterns-and-typescript-seams

```yaml
task_ref: deduplicate-proven-patterns-and-typescript-seams
target: Deduplicate the matching stub filter and the TypeScript callback-parameter walk, then split typescript.rs along orthogonal subsystems
proof_policy: seam-test-first
contract_revision: 6
scope:
- build.rs
- src/engine/linker.rs
- src/engine/languages/python/linker.rs
- src/engine/linker/traits.rs
- src/engine/linker/semantic_context.rs
- src/engine/languages/mod.rs
- src/engine/languages/typescript.rs
- src/engine/languages/typescript/
- src/engine/languages/typescript/mod.rs
- src/engine/languages/typescript/dom.rs
- src/engine/languages/typescript/prototypes.rs
- src/engine/languages/typescript/frameworks.rs
- src/engine/languages/typescript/types.rs
- src/engine/languages/typescript/linker_bindings.rs
- tests/languages.rs
- tests/linker.rs
status: completed
subtasks:
- subtask_ref: deduplicate-linker-candidate-filters
  title: Add filter_stubs_and_overloads in linker/traits.rs and call it from linker.rs lines 940-953 and 2736-2749, python/linker.rs lines 482-495, and semantic_context.rs lines 719-740. Leave semantic_context.rs lines 897-919 and db/reader/selectors.rs unchanged
  status: completed
  evidence: Added one shared candidate filter and replaced the four matching blocks; `cargo test --test linker` passed (93 passed, 0 failed), including the concrete-vs-stub and ambiguous-stub unit cases.
- subtask_ref: deduplicate-typescript-callback-param-ast
  title: Extract first_callback_parameter for the identical walks at typescript.rs lines 1049-1060 and 2078-2089
  status: completed
  evidence: Extracted the shared TypeScript callback parameter walk into `typescript/dom.rs`; `cargo test --lib first_callback_parameter` passed (1 passed, 39 filtered), and `cargo test --test languages` passed (252 passed).
- subtask_ref: update-build-rs-profile-discovery
  title: 'Teach build.rs generate_profiles to register languages/<name>.rs and languages/<name>/mod.rs, keeping the generated #[path] attribute'
  status: completed
  evidence: Profile discovery accepts flat `<name>.rs` files and only directory modules backed by an exact declared Cargo feature `lang-<name>`; helpers such as support/registries are excluded. Regression `helper_modules_are_not_advertised_as_language_profiles` failed before the filter and passed after it. Full `cargo test --test languages` passed (253 passed).
- subtask_ref: decompose-typescript-orthogonal-subsystems
  title: Split typescript.rs into dom.rs, prototypes.rs, frameworks.rs, types.rs, and mod.rs. Leave typescript_bindings declared from languages/mod.rs
  status: completed
  evidence: Moved DOM handling, frameworks, prototype helpers, and type inference to their own modules; `typescript_bindings` remains declared from `languages/mod.rs`. Corrected candidate passed `cargo test --test languages` (253 passed) and targeted rustfmt checks.
- subtask_ref: verify-typescript-and-linker-suites
  title: Verify cargo test --test languages and cargo test --test linker pass
  status: completed
  evidence: On the corrected candidate, `cargo test --test languages` passed (253 passed); `cargo test --test linker` passed (93 passed); `cargo clippy --all-targets --all-features -- -D warnings` passed; targeted rustfmt and `git diff --check` passed.
receipt:
  commit: 93a59633fb4822a3d5847903fd9b7ed054709cbb
  contract_revision: 6
  passed_at: 2026-10-10T22:44:20.989126009+00:00
  evidence:
    command_proof:
      command: cargo test --test languages
      exit_code: 0
      tests_passed: 253
      tests_failed: 0
      log: 'The new helper-module regression failed before the build.rs change and passes now. Also passed: cargo test --test linker (93); cargo clippy --all-targets --all-features -- -D warnings; rustfmt checks; git diff --check.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed pinned snapshot f7c7c3f. It contains only the task's admitted build.rs, linker/language sources, src/engine/languages/typescript/**, and tests/languages.rs/tests/linker.rs. The round-2 delta from accfb0e is limited to build.rs and the in-scope production regression test; no protected configuration, generated state, credentials, or neighboring project paths changed.
        claims:
          applicable: true
          evidence: The repeated stub/overload filter preserves the prior rule at all four intended call sites, and the TypeScript callback parameter walk and subsystem split remain as previously reviewed. The round-2 build.rs directory branch now requires both mod.rs and a matching declared lang-<name> feature in Cargo.toml. The production test helper_modules_are_not_advertised_as_language_profiles verifies support/registries are absent from AVAILABLE_LANGUAGE_FEATURES and languages::require() returns an unknown-language error. Accepted build evidence records this regression failing before the fix and passing after, plus 253 language tests, 93 linker tests, and strict all-target Clippy.
        concurrency:
          applicable: true
          evidence: The snapshot changes build-time profile discovery, local linker candidate filtering, TypeScript modules, and tests. It does not alter task-store transactions, revision/claim fencing, receipts, locks, Git mutation, or shared task namespaces; no concurrent task state is introduced.
        project_isolation:
          applicable: true
          evidence: Directory discovery enumerates the repository-relative src/engine/languages tree, reads the repository Cargo.toml, and canonicalizes discovered module paths. The round-2 filter excludes helper directories without changing task IDs, workspace resolution, database rows, profile path routing, or Git operations.
        administration:
          applicable: true
          evidence: No ACDD profile/configuration, schema migration, task database, hook, receipt, or delivery code is changed. Cargo.toml remains outside the candidate changes; existing profile digest inputs and generated output placement remain intact. The discovered-feature filter only prevents helper module names from being advertised as language features.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli. Direct rusqlite use in src/core/commitments stays.'
    - 'INV-FROZEN-ENGINE-DB-CYCLE: The legacy mutual import cycle between src/engine and src/db accepts no new edge. Ports that would break it belong to a later milestone.'
    - 'INV-BALANCED-MODULARITY: Split only along mixed responsibilities. Cohesive multi-pass pipeline functions have no line-count cap.'
    - 'INV-DEDUPLICATE-PROVEN-PATTERNS: Collapse a repeated block only where the control flow matches. Distinct rules stay separate.'
    - 'INV-CANONICAL-HELPERS: Ordinary Git queries, atomic file writes, and cgroup memory detection have one owner in src/core.'
    - 'INV-COMPAT-REEXPORTS: Moved scanner helpers keep pub use re-exports at engine::scanner. Pure receipt checks stay at core::tasks::gates. core does not re-export engine.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results.'
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

### task: linker-pipeline-decomposition-along-existing-marks

```yaml
task_ref: linker-pipeline-decomposition-along-existing-marks
target: Split link_compact_impl along its existing mark checkpoints, with no line-count cap
proof_policy: seam-test-first
contract_revision: 6
depends_on:
- deduplicate-proven-patterns-and-typescript-seams
scope:
- src/engine/linker.rs
- src/engine/linker/mod.rs
- src/engine/linker/routes.rs
- src/engine/linker/indexing.rs
- src/engine/linker/resolution.rs
status: completed
subtasks:
- subtask_ref: extract-route-matching-and-normalization
  title: Move RouteEntry, normalize_route_segments, and normalize_client_url into src/engine/linker/routes.rs
  status: completed
  evidence: Moved RouteEntry, URL normalization, and route matching into linker/routes.rs; `cargo test --lib route_normalization_preserves_dynamic_segment_rules` passed (1 passed).
- subtask_ref: extract-indexing-stages
  title: Move namespace and re-export index construction through the mark at line 458, and inheritance indexing through the mark at line 638, into src/engine/linker/indexing.rs
  status: completed
  evidence: Moved profile/import/namespace/package-export construction and Python/Rust inheritance indexes into linker/indexing.rs; `cargo test --test linker` passed (93 tests).
- subtask_ref: extract-reference-linking-stage
  title: Move HTML registry construction at lines 668-687 and the par_iter reference loop from line 688 through the mark at line 3083 into src/engine/linker/resolution.rs, keeping parameters all, public_all, owners, root, dependency_registry, and flows
  status: completed
  evidence: Moved HTML registry setup and the parallel reference-resolution pass into linker/resolution.rs with the admitted context inputs; linker suite passed (93 tests), including receiver fixtures.
- subtask_ref: assemble-linker-pipeline-orchestrator
  title: Keep link_compact_impl as the coordinator of indexing, semantic admission, value flow, resolution, and graph merge
  status: completed
  evidence: Kept link_compact_impl coordinating route and indexes, semantic admission, value flow, resolution, profiling marks, and graph merge.
- subtask_ref: verify-linker-suite
  title: Verify cargo test --test linker and cargo test --test receivers pass
  status: completed
  evidence: '`cargo test --test linker` passed (93); `cargo test --test linker receiver` passed (86, 7 filtered); focused route unit passed (1); `cargo clippy --all-targets --all-features -- -D warnings` and targeted rustfmt/git diff --check passed. `cargo test --test receivers` is not a Cargo target; receiver modules are nested under tests/linker and ran as part of the linker suite.'
receipt:
  commit: 7e34f9da8a5857653ea70ee6666f89e500f357db
  contract_revision: 6
  passed_at: 2026-10-10T23:12:01.742590004+00:00
  evidence:
    command_proof:
      command: cargo test --test linker
      exit_code: 0
      tests_passed: 93
      tests_failed: 0
      log: 'Fixed the duplicate helper by calling parent super::qualified; AST confirms no local qualified() definition remains. Also passed: cargo test --lib route_normalization_preserves_dynamic_segment_rules (1 passed); cargo clippy --all-targets --all-features -- -D warnings; targeted rustfmt and git diff --check.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Inspected pinned candidate snapshot 6ee235c against baseline 1b1df31. The snapshot payload contains only src/engine/linker.rs and the new src/engine/linker/{indexing,resolution,routes}.rs; each is inside the admitted scope, while src/engine/linker/mod.rs and all configuration, generated state, credentials, and neighboring project files are unchanged in the task patch.
        claims:
          applicable: true
          evidence: 'The prior duplicate is fixed: indexing.rs now calls super::qualified and has no local qualified() definition; the sole helper remains in linker.rs. The extraction preserves route matching/normalization and both consumers use RouteIndex; namespace/package-export/inheritance indexes retain their existing logic; link_compact_impl remains the coordinator; the resolution pass retains the existing per-file graph computation and merge. Accepted build proof records cargo test --test linker (93 passed), the focused route-normalization seam test (1 passed), strict all-target Clippy, targeted rustfmt, and git diff --check.'
        concurrency:
          applicable: true
          evidence: The extracted resolver retains the baseline immutable per-file Rayon par_iter and returns per-file Graph values to the existing coordinator merge/sort. No shared mutable task state, transaction, claim revision, receipt, lock, or database behavior changed.
        project_isolation:
          applicable: true
          evidence: The resolver continues to receive and apply the existing owners filter over the same all.par_iter() entries. The extracted modules introduce no workspace resolution, filesystem traversal, task identity, project path, or Git mutation behavior.
        administration:
          applicable: true
          evidence: No ACDD profile, repository configuration, migration, hook, receipt, or delivery implementation changed. Forge build evidence pins candidate 6ee235c to baseline 1b1df31; no privileged administration path is touched.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli. Direct rusqlite use in src/core/commitments stays.'
    - 'INV-FROZEN-ENGINE-DB-CYCLE: The legacy mutual import cycle between src/engine and src/db accepts no new edge. Ports that would break it belong to a later milestone.'
    - 'INV-BALANCED-MODULARITY: Split only along mixed responsibilities. Cohesive multi-pass pipeline functions have no line-count cap.'
    - 'INV-DEDUPLICATE-PROVEN-PATTERNS: Collapse a repeated block only where the control flow matches. Distinct rules stay separate.'
    - 'INV-CANONICAL-HELPERS: Ordinary Git queries, atomic file writes, and cgroup memory detection have one owner in src/core.'
    - 'INV-COMPAT-REEXPORTS: Moved scanner helpers keep pub use re-exports at engine::scanner. Pure receipt checks stay at core::tasks::gates. core does not re-export engine.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results.'
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

### task: task-engine-and-store-responsibility-split

```yaml
task_ref: task-engine-and-store-responsibility-split
target: Separate Git index snapshots, blackboard persistence, and subtask storage from task claim and submit, and migrate the ordinary Git calls in those files
proof_policy: seam-test-first
contract_revision: 6
depends_on:
- core-leaf-purification-and-canonical-primitives
scope:
- src/engine/tasks.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/git_snapshot.rs
- src/engine/tasks/blackboard.rs
- src/engine/tasks/lifecycle.rs
- src/db/tasks_store.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/blackboard.rs
- src/db/tasks_store/subtasks.rs
- src/db/tasks_store/lifecycle.rs
status: completed
subtasks:
- subtask_ref: isolate-git-index-and-snapshot-engine
  title: Move TemporaryGitIndex, GitHeadRollback, GitIndexRollback, and candidate snapshot capture into git_snapshot.rs. Migrate 12 ordinary Git calls in tasks.rs and 1 in tasks_store.rs to core::git. Switch the atomic_replace calls in tasks.rs to core::fs::atomic_replace. Keep stdin, GIT_INDEX_FILE, and spawn local
  status: completed
  evidence: Moved snapshot/index/rollback machinery into `engine/tasks/git_snapshot.rs`; routed 12 ordinary task-engine Git operations and 1 store Git operation through `core::git`, with stdin, spawn, and GIT_INDEX_FILE flows kept local. Atomic replacement uses `core::fs`. Verified by seam test (1 passed), ACDD suite (68 passed), strict all-target Clippy, and all-target/all-features check.
- subtask_ref: decompose-blackboard-engine-and-persistence
  title: Move blackboard request validation into engine/tasks/blackboard.rs, and blackboard_post, blackboard_post_scoped, blackboard_post_scoped_with_gate, blackboard_page, blackboard_page_for_gate, blackboard_inspect, blackboard_read, and blackboard_clear into db/tasks_store/blackboard.rs
  status: completed
  evidence: Moved engine request types/validation/dispatch into `engine/tasks/blackboard.rs` and TasksStore blackboard persistence into `db/tasks_store/blackboard.rs`, with facade re-exports preserving public names. Verified by ACDD suite (68 passed) and strict all-target Clippy.
- subtask_ref: decompose-subtask-and-lifecycle-store
  title: Move subtask CRUD into db/tasks_store/subtasks.rs and claim, reset, reopen, extend_scope, delete, and cleanup into db/tasks_store/lifecycle.rs. Leave TasksStore::submit in engine/tasks/gates.rs
  status: completed
  evidence: Moved subtask CRUD to `db/tasks_store/subtasks.rs` and claim/reset/reopen/scope/delete/cleanup operations to `db/tasks_store/lifecycle.rs`. Verified by ACDD suite (68 passed), all-target/all-features check, and strict all-target Clippy.
- subtask_ref: assemble-task-engine-and-store-facades
  title: Keep tasks/mod.rs and tasks_store/mod.rs as facades, including the gates module added by task 1 and the existing context and workspaces modules
  status: completed
  evidence: Turned both former god files into `mod.rs` facades and retained existing modules and compatibility exports. Forge code-map reports zero parse errors in both module trees; `cargo check --all-targets --all-features` and strict Clippy pass.
- subtask_ref: verify-acdd-task-suite
  title: Verify cargo test --test acdd passes
  status: completed
  evidence: '`cargo test --test acdd` passed: 68 passed, 0 failed, 0 ignored. `cargo test --lib temporary_git_index_is_removed_when_dropped` passed: 1 passed. Strict `cargo clippy --all-targets --all-features -- -D warnings` passed.'
receipt:
  commit: 3b9aaa03c20d360c9ad2e402153b110f8278149d
  contract_revision: 6
  passed_at: 2026-10-10T23:44:36.297123965+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 68
      tests_failed: 0
      log: Also passed cargo test --lib temporary_git_index_is_removed_when_dropped (1 passed), cargo check --all-targets --all-features, cargo clippy --all-targets --all-features -- -D warnings, targeted rustfmt, and git diff --check.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Inspected pinned candidate 5e423e9 against baseline 086b61deb0d9fbfb33ae87e66116470ff0aaa650. The scoped patch replaces src/engine/tasks.rs with src/engine/tasks/mod.rs and adds git_snapshot.rs, blackboard.rs, lifecycle.rs; it renames src/db/tasks_store.rs to src/db/tasks_store/mod.rs and adds db blackboard, subtasks, and lifecycle modules. Every changed or added path is in the initial scope, including the in-scope temporary-index regression test; no profile, configuration, credentials, generated state, or neighboring project paths changed.
        claims:
          applicable: true
          evidence: 'Facade/API compatibility is retained: tasks/mod.rs keeps the public request/parameter types and re-exports blackboard, BlackboardAction/Request/Scope plus claim/manage/reopen/reset; tasks_store/mod.rs keeps the data types and re-exports clear_blackboard and is_canonical_blackboard_topic, while moved TasksStore methods remain public inherent methods. Engine blackboard validation/dispatch, DB blackboard persistence, DB subtask CRUD, and DB lifecycle operations are split into the contracted modules. Ordinary Git queries now use core::git; raw Command::new("git") remains only in git_snapshot.rs for piped-stdin/spawn index operations or commands isolated with GIT_INDEX_FILE. Both milestone receipt replacements use core::fs::atomic_replace. TemporaryGitIndex removes its file in Drop and the added unit test checks that cleanup. Build evidence records cargo test --test acdd (68 passed), the temporary-index cleanup test (1 passed), cargo check --all-targets --all-features, and strict all-target Clippy.'
        concurrency:
          applicable: true
          evidence: The moved TasksStore claim/reset/scope/delete and subtask add/update paths retain immediate SQLite transactions; claim/revision and task status checks remain in place. Blackboard task posts retain conditional INSERT ... SELECT guards against task completion/subtask deletion races. The engine delivery path retains the receipt lock, baseline/HEAD fencing, and GitHeadRollback/GitIndexRollback restoration after hook or persistence failure. No new lock ordering or cross-store sharing is introduced.
        project_isolation:
          applicable: true
          evidence: Workspace owner selection, worktree validation, frozen scope checks, confined milestone paths, and namespace-qualified task/milestone guards remain on the production paths. Snapshot capture starts from an empty temporary index and adds only admitted scope paths; delivery still requires the receipt path to be under the worktree and applies candidate files through the isolated delivery index.
        administration:
          applicable: true
          evidence: No profile, schema migration, repository configuration, hook, or archived receipt format changed. Submission still derives the accepted snapshot from scoped capture, validates candidate/baseline identity and tree/parent before commit, preserves hook failure rollback, and persists the receipt through the existing pre-persist delivery path. Ordinary Git helper ownership is consolidated under core::git, with only stdin/spawn or GIT_INDEX_FILE operations remaining local.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli. Direct rusqlite use in src/core/commitments stays.'
    - 'INV-FROZEN-ENGINE-DB-CYCLE: The legacy mutual import cycle between src/engine and src/db accepts no new edge. Ports that would break it belong to a later milestone.'
    - 'INV-BALANCED-MODULARITY: Split only along mixed responsibilities. Cohesive multi-pass pipeline functions have no line-count cap.'
    - 'INV-DEDUPLICATE-PROVEN-PATTERNS: Collapse a repeated block only where the control flow matches. Distinct rules stay separate.'
    - 'INV-CANONICAL-HELPERS: Ordinary Git queries, atomic file writes, and cgroup memory detection have one owner in src/core.'
    - 'INV-COMPAT-REEXPORTS: Moved scanner helpers keep pub use re-exports at engine::scanner. Pure receipt checks stay at core::tasks::gates. core does not re-export engine.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results.'
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

### task: milestone-engine-and-scanner-modularization

```yaml
task_ref: milestone-engine-and-scanner-modularization
target: Split milestone contract parsing from Git handoff, migrate the 8 milestone Git calls, remove milestones::atomic_replace, and turn the post-cgroup scanner into scanner/mod.rs
proof_policy: seam-test-first
contract_revision: 6
depends_on:
- core-leaf-purification-and-canonical-primitives
scope:
- src/engine/milestones.rs
- src/engine/milestones/mod.rs
- src/engine/milestones/contract.rs
- src/engine/milestones/handoff.rs
- src/engine/scanner.rs
- src/engine/scanner/mod.rs
status: completed
subtasks:
- subtask_ref: extract-milestone-contract-parsing
  title: Move markdown contract snapshots and YAML frontmatter parsing into src/engine/milestones/contract.rs
  status: completed
  evidence: Moved the Snapshot type and markdown/YAML frontmatter/task-block parser to `engine/milestones/contract.rs`; the new production-seam test now passes (1 passed).
- subtask_ref: extract-milestone-git-handoff
  title: Move ancestor checks, changed-path checks, and landed-commit attribution into src/engine/milestones/handoff.rs. Migrate the 8 Git calls to core::git and delete milestones::atomic_replace in favor of core::fs::atomic_replace
  status: completed
  evidence: Moved ancestry, changed-path, landed-commit attribution, handoff, and snapshot-ref cleanup into `engine/milestones/handoff.rs`; all eight ordinary Git calls use `core::git`, and activation uses `core::fs::atomic_replace`. AST search finds zero raw `Command::new("git")` in milestone modules; ACDD handoff tests pass.
- subtask_ref: finalize-scanner-engine-modularity
  title: Turn the scanner that task 1 already stripped of cgroup parsing into scanner/mod.rs for traversal, delta inventory, and adapter execution. Keep scanner/adapter.rs
  status: completed
  evidence: Moved `engine/scanner.rs` to `engine/scanner/mod.rs`, kept `scanner/adapter.rs`, corrected its relative module path, and preserved scanner exports. `cargo test --test core` passes scanner helper compatibility tests.
- subtask_ref: verify-milestone-and-core-suites
  title: Verify cargo test --test acdd and cargo test --test core pass
  status: completed
  evidence: '`cargo test --test acdd`: 68 passed, 0 failed. `cargo test --test core`: 31 passed, 0 failed, 1 ignored. Strict all-target/all-features Clippy passes.'
receipt:
  commit: 754650ce2f8d093d4ae52acf904dbf1b62d9b10b
  contract_revision: 6
  passed_at: 2026-10-10T23:52:59.702911660+00:00
  evidence:
    command_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 68
      tests_failed: 0
      log: Passed. Also passed the contract seam test (1), cargo test --test core (31 passed, 1 ignored), cargo check --all-targets --all-features, cargo clippy --all-targets --all-features -- -D warnings, rustfmt on scoped modules, AST check with zero direct Git Command calls in milestones, and git diff --check.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Compared pinned candidate 7871f8b with baseline 0aef65ffe0c4917a3869048fe647633a533ff8d1. The scoped patch contains only the additions milestones/contract.rs and milestones/handoff.rs and the admitted moves milestones.rs to milestones/mod.rs and scanner.rs to scanner/mod.rs. No configuration, profile, credential, generated-state, or neighboring-project path is changed. scanner/adapter.rs remains at the directory-relative location, and scanner/mod.rs declares #[path = "adapter.rs"].'
        claims:
          applicable: true
          evidence: Snapshot and snapshot parsing moved from the facade with parsing behavior preserved; the new snapshot_parses_frontmatter_identity_and_task_blocks test calls the real filesystem-reading parser. The facade retains its operations and publicly re-exports handoff. All eight milestone Git operations route through core::git (merge-base, diff, both rev-list forms, rev-parse, cat-file, for-each-ref, update-ref); special 0/1 exit-status handling for ancestry and diff remains explicit. activate_on_claim calls core::fs::atomic_replace and the local implementation is removed. Scanner adapter and helper exports remain available from engine::scanner. The accepted build evidence records the seam, ACDD/core suites, and strict Clippy passing; I did not rerun tests as instructed.
        concurrency:
          applicable: true
          evidence: 'The extracted handoff preserves operation order: create the archive with create_new, transactionally update task rows/receipts through handoff_milestone, then remove the active milestone; on removal failure it restores original receipts/path in SQLite and deletes the archive. The unchanged database method uses an IMMEDIATE transaction and checks task state plus equality of discovered, expected, and receipt ID sets before commit. Git calls remain synchronous and use the same workspace root; extraction adds no concurrent mutation path.'
        project_isolation:
          applicable: true
          evidence: Milestone discovery resolves from the supplied root; snapshot requires the document path to be beneath that root and validates repository/project identities. Handoff opens TasksStore for the milestone's repository/project namespace; Git operations use the same root, and landed-commit comparisons retain the task's declared scope. No linked-workspace profile or path is introduced.
        administration:
          applicable: true
          evidence: The patch changes no schema, profile, gate, configuration, or delivery policy. Handoff continues validating durable receipts against each task's loaded profile, resolves and verifies the handoff commit in the owning repository, and records task-specific landed SHAs in archived receipts. No direct Git Command invocation remains in the milestone handoff module; all eight operations use canonical helpers.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli. Direct rusqlite use in src/core/commitments stays.'
    - 'INV-FROZEN-ENGINE-DB-CYCLE: The legacy mutual import cycle between src/engine and src/db accepts no new edge. Ports that would break it belong to a later milestone.'
    - 'INV-BALANCED-MODULARITY: Split only along mixed responsibilities. Cohesive multi-pass pipeline functions have no line-count cap.'
    - 'INV-DEDUPLICATE-PROVEN-PATTERNS: Collapse a repeated block only where the control flow matches. Distinct rules stay separate.'
    - 'INV-CANONICAL-HELPERS: Ordinary Git queries, atomic file writes, and cgroup memory detection have one owner in src/core.'
    - 'INV-COMPAT-REEXPORTS: Moved scanner helpers keep pub use re-exports at engine::scanner. Pure receipt checks stay at core::tasks::gates. core does not re-export engine.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results.'
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

## Verification gates and proof requirements

Before delivering each task and closing this milestone:

1. Run the focused test seam for the admitted change.
2. Run `cargo test --test <affected-target>` with no filter.
3. Run `cargo clippy --all-targets --all-features -- -D warnings` with zero warnings.
4. Before handoff, run the command from [TESTS.md](../../TESTS.md): `cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`.
5. Confirm `src/core` imports no module from `src/engine`, `src/db`, `src/mcp`, or `src/cli`, and `src/core/commitments` still uses `rusqlite`.
