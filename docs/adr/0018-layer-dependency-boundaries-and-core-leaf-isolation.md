---
title: "ADR 0018: Layer Dependency Boundaries and Core Leaf Isolation"
doc_type: adr
status: accepted
date: 2026-10-10
---

# ADR 0018: Layer Dependency Boundaries and Core Leaf Isolation

## Status

Accepted.

## Decision

`src/core` is a leaf. It imports no module from `src/engine`, `src/db`, `src/mcp`, or `src/cli`. Merkle commitments in `src/core/commitments` keep their direct `rusqlite` use. That crate is not `src/db`.

`src/core/tasks` keeps profiles, immutable task DTOs, `REVIEW_CONTOURS`, and the pure receipt checks `validate_durable_receipt` and `validate_durable_receipt_with_profile`. Those functions take a receipt, a revision, and a gate profile. They do not open SQLite, spawn Git, or write milestone files. `src/db/tasks_store`, `src/cli/migrate`, and the ACDD tests call them at `core::tasks::gates`.

Shell verification, provisional milestone-file rollback, delivery-tree Git checks, and `tasks_store` coordination live in `src/engine/tasks/gates.rs`. `src/core` does not re-export that module. A re-export would point the leaf back at `engine`. Calling the moved module from `src/db` would add a new `db → engine` edge.

The existing `engine ↔ db` import cycle stays frozen. New edges between those two trees require storage and graph-builder ports in `src/core`, admitted by a later milestone.

Modules split where one file mixes independent responsibilities. Cohesive multi-pass algorithms keep their linear shape and have no line-count cap. Proven identical blocks collapse into one helper. Distinct control flow stays separate. Milestone contracts that move these boundaries follow this decision.

## Architectural invariants

### 1. Core leaf isolation

> [!IMPORTANT]
> Invariant: `src/core` never imports `src/engine`, `src/db`, `src/mcp`, or `src/cli`. Direct `rusqlite` use inside `src/core/commitments` remains.

- Domain models, Merkle commitments, response budgets, and the OS wrappers `core/fs`, `core/git`, and `core/sys` live in `src/core`.
- `canonical_root`, `checked_child`, and `safe_relative` originate in `core::fs`. `core/adapter.rs` re-exports them from there. `engine::scanner` keeps `pub use` re-exports for existing callers in `db`, `mcp`, and `cli`.
- Pure receipt and contour checks stay in `core::tasks::gates`.
- SQLite task coordination, shell gate commands, milestone receipt writes, and delivery-tree Git checks live in `engine::tasks::gates`.

### 2. Frozen engine-database cycle

> [!IMPORTANT]
> Invariant: The mutual import cycle between `src/engine` and `src/db` accepts no new edge. Ports in `src/core` are a later milestone.

- Current edges stay: `engine/tasks` uses `db/tasks_store`; `db/writer`, `db/reader`, `db/ingest`, `db/delta`, `db/symbols`, and `db/lint` use `engine` scanner, language, and linker types.
- `db/tasks_store` continues to call pure receipt validation through `core::tasks::gates`.

### 3. Balanced modularity and proven deduplication

> [!IMPORTANT]
> Invariant: Split a module only where responsibilities differ. Extract a helper only where the control flow is the same. Leave cohesive pipeline passes intact.

- Split Git index mechanics from task claim and submit, cgroup parsing from directory walking, and Markdown contract parsing from Git handoff.
- Keep value flow, semantic admission, and the reference-linking pass readable in place. HTML registry construction that precedes the reference loop stays with that pass's inputs.
- `filter_stubs_and_overloads` replaces the candidate list only where a narrower non-stub, non-overload set is kept when it is non-empty. That block is in `linker.rs`, `languages/python/linker.rs`, and the symbol-admission branch of `linker/semantic_context.rs`.
- The member lookup that returns one local member for exactly one non-stub and `Ambiguous` otherwise stays a separate rule.
- The selector disambiguation filter in `db/reader/selectors.rs` walks JSON values, not `&Node`, and stays local.
- `commit_tree_and_parent` in receipt checking and in snapshot capture may share the `rev-list --parents` and `rev-parse ^{tree}` queries. Receipt checks keep `TASK_RECEIPT_INVALID` and accept the existing commit string. Snapshot capture keeps `TASK_SNAPSHOT_INVALID` and the 40–64 hexadecimal object-id check.
- The first-parameter walk of an arrow function in the TypeScript profile is one helper. The surrounding DOM-receiver and parameter-type rules stay separate.

### 4. Canonical helper ownership

> [!IMPORTANT]
> Invariant: Each shared infrastructure operation has one owner in `src/core`. Callers outside the edit keep their current paths through `pub use`.

- `core::git` owns `git_command`, `git_stdout`, `git_status`, and `git_commit_exists` for ordinary cwd-bound queries. Piped stdin, `GIT_INDEX_FILE`, and `spawn` stay in `engine::tasks::git_snapshot` and may start from `git_command`.
- `core::fs::atomic_write` creates parent directories, sets mode `0o600` or `0o755`, and rejects symlink paths. `core::fs::atomic_replace` replaces a file in its own directory and preserves the existing mode and umask for milestone Markdown. Both share `commit_atomic_temp` for the temporary write, sync, and rename.
- `core::sys` owns Linux cgroup v1 and v2 memory parsing, headroom, and `memory_budget_bytes`. `engine::scanner` re-exports the budget function.

## Consequences

- `src/core` depends on no higher crate module. Merkle commitments still link `rusqlite`.
- `db` and `cli` keep calling pure receipt checks without a new import of `engine`.
- Path and memory helpers move once, and existing scanner paths continue to compile.
- Linker, TypeScript, and Git duplicates collapse only where the behavior matches. Resolution results stay the same.
- The `engine ↔ db` cycle gains no edge until a ports milestone admits one.

## Non-goals

- Removing `rusqlite` from `src/core/commitments`.
- Moving `validate_durable_receipt`, `validate_durable_receipt_with_profile`, or `REVIEW_CONTOURS` into `engine`.
- Breaking the current `engine ↔ db` cycle in this decision.
- A line-count cap on `link_compact_impl` or any other cohesive pass.
- Changing language extraction, static value flow, overload ambiguity, or selector disambiguation.
