---
id: m-source-modularity-and-god-file-decomposition
title: Source code modularity, god-file decomposition, and canonical helper extraction
doc_type: contract
status: active
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
---

# Source code modularity, god-file decomposition, and canonical helper extraction

## Outcome and purpose

Follow [ADR 0018](../adr/0018-layer-dependency-boundaries-and-core-leaf-isolation.md). The task titles are the contract.

Task 1 and task 2 share no files. Task 3 follows task 2. Tasks 4 and 5 follow task 1.

## Tasks in this milestone

### task: core-leaf-purification-and-canonical-primitives

```yaml
task_ref: core-leaf-purification-and-canonical-primitives
target: "Make src/core a leaf by moving submit and delivery-tree checks to engine::tasks, keeping pure receipt checks in core::tasks, and adding core::fs, core::git, and core::sys"
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
status: planned
subtasks:
  - subtask_ref: split-gates-operational-execution
    title: "Move TasksStore::submit, submit_with_pre_persist, accepted, validate_delivery_commit, delivery_tree_entries, ProvisionalReceiptRollback, and the receipt-side commit_tree_and_parent into src/engine/tasks/gates.rs. Keep REVIEW_CONTOURS, Evidence, validate_durable_receipt, validate_durable_receipt_with_profile, and their proof checkers in src/core/tasks/gates.rs"
    status: pending
  - subtask_ref: extract-canonical-git-primitives
    title: "Implement crate::core::git (git_command, git_stdout, git_status, git_commit_exists) and migrate the 4 ordinary Git calls that move with delivery verification"
    status: pending
  - subtask_ref: unify-atomic-fs-primitives
    title: "Move canonical_root, checked_child, and safe_relative into core::fs. Add atomic_replace beside atomic_write through commit_atomic_temp. Point ProvisionalReceiptRollback at core::fs::atomic_replace and leave milestones::atomic_replace for task 5"
    status: pending
  - subtask_ref: extract-system-memory-and-cgroup-detection
    title: "Move cgroup v1/v2 parsing, available_memory_bytes, and memory_budget_bytes from engine::scanner into core::sys, and pub use those public functions from engine::scanner"
    status: pending
  - subtask_ref: verify-core-leaf-invariants
    title: "Verify src/core imports no engine, db, mcp, or cli module, commitments still use rusqlite, and cargo test --test core passes"
    status: pending
```

### task: deduplicate-proven-patterns-and-typescript-seams

```yaml
task_ref: deduplicate-proven-patterns-and-typescript-seams
target: "Deduplicate the matching stub filter and the TypeScript callback-parameter walk, then split typescript.rs along orthogonal subsystems"
proof_policy: seam-test-first
contract_revision: 5
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
status: planned
subtasks:
  - subtask_ref: deduplicate-linker-candidate-filters
    title: "Add filter_stubs_and_overloads in linker/traits.rs and call it from linker.rs lines 940-953 and 2736-2749, python/linker.rs lines 482-495, and semantic_context.rs lines 719-740. Leave semantic_context.rs lines 897-919 and db/reader/selectors.rs unchanged"
    status: pending
  - subtask_ref: deduplicate-typescript-callback-param-ast
    title: "Extract first_callback_parameter for the identical walks at typescript.rs lines 1049-1060 and 2078-2089"
    status: pending
  - subtask_ref: update-build-rs-profile-discovery
    title: "Teach build.rs generate_profiles to register languages/<name>.rs and languages/<name>/mod.rs, keeping the generated #[path] attribute"
    status: pending
  - subtask_ref: decompose-typescript-orthogonal-subsystems
    title: "Split typescript.rs into dom.rs, prototypes.rs, frameworks.rs, types.rs, and mod.rs. Leave typescript_bindings declared from languages/mod.rs"
    status: pending
  - subtask_ref: verify-typescript-and-linker-suites
    title: "Verify cargo test --test languages and cargo test --test linker pass"
    status: pending
```

### task: linker-pipeline-decomposition-along-existing-marks

```yaml
task_ref: linker-pipeline-decomposition-along-existing-marks
target: "Split link_compact_impl along its existing mark checkpoints, with no line-count cap"
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
status: planned
subtasks:
  - subtask_ref: extract-route-matching-and-normalization
    title: "Move RouteEntry, normalize_route_segments, and normalize_client_url into src/engine/linker/routes.rs"
    status: pending
  - subtask_ref: extract-indexing-stages
    title: "Move namespace and re-export index construction through the mark at line 458, and inheritance indexing through the mark at line 638, into src/engine/linker/indexing.rs"
    status: pending
  - subtask_ref: extract-reference-linking-stage
    title: "Move HTML registry construction at lines 668-687 and the par_iter reference loop from line 688 through the mark at line 3083 into src/engine/linker/resolution.rs, keeping parameters all, public_all, owners, root, dependency_registry, and flows"
    status: pending
  - subtask_ref: assemble-linker-pipeline-orchestrator
    title: "Keep link_compact_impl as the coordinator of indexing, semantic admission, value flow, resolution, and graph merge"
    status: pending
  - subtask_ref: verify-linker-suite
    title: "Verify cargo test --test linker and cargo test --test receivers pass"
    status: pending
```

### task: task-engine-and-store-responsibility-split

```yaml
task_ref: task-engine-and-store-responsibility-split
target: "Separate Git index snapshots, blackboard persistence, and subtask storage from task claim and submit, and migrate the ordinary Git calls in those files"
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
status: planned
subtasks:
  - subtask_ref: isolate-git-index-and-snapshot-engine
    title: "Move TemporaryGitIndex, GitHeadRollback, GitIndexRollback, and candidate snapshot capture into git_snapshot.rs. Migrate 12 ordinary Git calls in tasks.rs and 1 in tasks_store.rs to core::git. Switch the atomic_replace calls in tasks.rs to core::fs::atomic_replace. Keep stdin, GIT_INDEX_FILE, and spawn local"
    status: pending
  - subtask_ref: decompose-blackboard-engine-and-persistence
    title: "Move blackboard request validation into engine/tasks/blackboard.rs, and blackboard_post, blackboard_post_scoped, blackboard_post_scoped_with_gate, blackboard_page, blackboard_page_for_gate, blackboard_inspect, blackboard_read, and blackboard_clear into db/tasks_store/blackboard.rs"
    status: pending
  - subtask_ref: decompose-subtask-and-lifecycle-store
    title: "Move subtask CRUD into db/tasks_store/subtasks.rs and claim, reset, reopen, extend_scope, delete, and cleanup into db/tasks_store/lifecycle.rs. Leave TasksStore::submit in engine/tasks/gates.rs"
    status: pending
  - subtask_ref: assemble-task-engine-and-store-facades
    title: "Keep tasks/mod.rs and tasks_store/mod.rs as facades, including the gates module added by task 1 and the existing context and workspaces modules"
    status: pending
  - subtask_ref: verify-acdd-task-suite
    title: "Verify cargo test --test acdd passes"
    status: pending
```

### task: milestone-engine-and-scanner-modularization

```yaml
task_ref: milestone-engine-and-scanner-modularization
target: "Split milestone contract parsing from Git handoff, migrate the 8 milestone Git calls, remove milestones::atomic_replace, and turn the post-cgroup scanner into scanner/mod.rs"
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
status: planned
subtasks:
  - subtask_ref: extract-milestone-contract-parsing
    title: "Move markdown contract snapshots and YAML frontmatter parsing into src/engine/milestones/contract.rs"
    status: pending
  - subtask_ref: extract-milestone-git-handoff
    title: "Move ancestor checks, changed-path checks, and landed-commit attribution into src/engine/milestones/handoff.rs. Migrate the 8 Git calls to core::git and delete milestones::atomic_replace in favor of core::fs::atomic_replace"
    status: pending
  - subtask_ref: finalize-scanner-engine-modularity
    title: "Turn the scanner that task 1 already stripped of cgroup parsing into scanner/mod.rs for traversal, delta inventory, and adapter execution. Keep scanner/adapter.rs"
    status: pending
  - subtask_ref: verify-milestone-and-core-suites
    title: "Verify cargo test --test acdd and cargo test --test core pass"
    status: pending
```

## Verification gates and proof requirements

Before delivering each task and closing this milestone:

1. Run the focused test seam for the admitted change.
2. Run `cargo test --test <affected-target>` with no filter.
3. Run `cargo clippy --all-targets --all-features -- -D warnings` with zero warnings.
4. Before handoff, run the command from [TESTS.md](../../TESTS.md): `cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`.
5. Confirm `src/core` imports no module from `src/engine`, `src/db`, `src/mcp`, or `src/cli`, and `src/core/commitments` still uses `rusqlite`.
