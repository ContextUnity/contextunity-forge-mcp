---
title: "Source Code Architecture — Modularity and Invariants"
doc_type: architecture
---

# Source Code Architecture — Modularity and Invariants

This document defines the architectural layering, import boundaries, modularity invariants, and canonical helper ownership for all code in `src/`.

## 1. Architectural Layers and Import Boundaries

The codebase operates on a 5-layer hierarchy:

```text
       ┌───────────┐       ┌───────────┐
       │  src/cli  │       │  src/mcp  │
       └─────┬─────┘       └─────┬─────┘
             │                   │
             └─────────┬─────────┘
                       ▼
               ┌───────────────┐
               │  src/engine   │◄────┐
               └───────┬───────┘     │ (legacy cycle frozen)
                       │             │
                       ▼             │
               ┌───────────────┐     │
               │    src/db     ├─────┘
               └───────┬───────┘
                       ▼
               ┌───────────────┐
               │   src/core    │ (strict leaf)
               └───────────────┘
```

> [!IMPORTANT]
> Invariant: `src/core` is a strict leaf. It never imports `src/engine`, `src/db`, `src/mcp`, or `src/cli`. Direct `rusqlite` use in `src/core/commitments` stays. Pure receipt checks (`validate_durable_receipt`, `validate_durable_receipt_with_profile`, `REVIEW_CONTOURS`) stay in `core::tasks::gates`. Shell verification, milestone receipt writes, delivery-tree Git checks, and `tasks_store` coordination live in `src/engine/tasks/gates.rs`. See [ADR 0018](../docs/adr/0018-layer-dependency-boundaries-and-core-leaf-isolation.md).

> [!IMPORTANT]
> Invariant: The legacy mutual import cycle between `src/engine` and `src/db` is frozen against any new import edges. Breaking this cycle requires abstract storage and graph-builder ports in `src/core`, which is deferred to a dedicated architectural milestone.

- **`src/core`**: Foundational domain models, cryptographic commitments, response pagination budgets, and OS/filesystem primitives. Strict leaf.
- **`src/db`**: SQLite persistence, schema migrations, reader snapshot isolation, FTS5 symbol indexing, and graph traversal caches. Depends on `src/core`. Legacy imports from `src/engine` are frozen.
- **`src/engine`**: AST fact extraction, language profiles, cross-file symbol linking, filesystem scanner, and ACDD task/milestone execution policies. Depends on `src/core` and `src/db`.
- **`src/mcp`**: JSON-RPC Model Context Protocol server, tool dispatch, and schema generation. Depends on `src/engine`, `src/db`, and `src/core`.
- **`src/cli`**: Command-line entry points and subcommand routing. Depends on `src/engine`, `src/db`, and `src/core`.

## 2. Balanced Modularity and Code Deduplication

> [!IMPORTANT]
> Invariant: Modules are split strictly along mixed architectural responsibilities (e.g. process execution vs lifecycle state, cgroup parsing vs directory walking, contract parsing vs Git handoff), never by arbitrary line-count thresholds. Do not enforce line limits on multi-pass pipeline functions that form a single cohesive algorithm.

> [!IMPORTANT]
> Invariant: Proven duplicates (identical logic repeated across files or passes) must be extracted into focused shared helpers. Domain algorithms representing distinct rules (e.g. language-specific AST profiles, linker passes, snapshot verification) must not be prematurely compressed.

### When to Keep Together (DO NOT Split)
- **High Domain Cohesion**: Closely related data structures, types, and their direct transformation logic belonging to the same semantic unit.
- **Tightly Coupled Pipelines**: Complex static analysis algorithms (`src/engine/linker/value_flow.rs`, `src/engine/linker/semantic_context.rs`, and the core reference linking loop in `src/engine/linker.rs`) where splitting would require exposing private internal state, inventing synthetic glue DTOs, or creating circular imports.
- **Linear Readability**: Keeping a cohesive multi-pass algorithm readable top-to-bottom in a single file is preferred over scattering logic across shallow delegating helpers.

### When to Split (DO Split)
- **Mixed Layer Responsibilities**: A module mixing protocol ingress, business logic, persistence queries, or external process execution.
- **Orthogonal Subsystems**: Code that changes for fundamentally different reasons or at different cadences (e.g., Git index manipulation inside task lifecycles, DOM globals inside TypeScript extraction profiles, or cgroup memory parsing inside directory walkers).
- **Proven Duplicates**: Extract repeated code blocks into single helpers (e.g. stub/overload candidate filters, arrow function parameter AST extraction, Git command wrappers, shared atomic file renaming).

## 3. Canonical Helper Ownership

> [!IMPORTANT]
> Invariant: Shared infrastructure operations have exactly one canonical owner in `src/core`. Do not invent local variants or duplicate raw subprocess calls in downstream engine or milestone files.

> [!IMPORTANT]
> Invariant: When moving shared utility functions into `src/core`, existing locations must retain `pub use` re-exports to preserve compilation and interface compatibility across untargeted callers.

1. **Git Process Execution (`crate::core::git`)**:
   - `core::git` owns the low-level subprocess runner: working directory (`cwd`) binding, exit status checks, and stdout capture (`git_command`, `git_stdout`, `git_status`, `git_commit_exists`).
   - High-level Git workflows (temporary index manipulation, rollback guards, and tree candidate snapshotting) remain domain logic in `src/engine/tasks/git_snapshot.rs`.
   - Special-case Git invocations (piped stdin, custom `GIT_INDEX_FILE` environment variables, or background child spawning) retain their specialized contracts.
2. **Filesystem and Atomic Writes (`crate::core::fs`)**:
   - `core::fs` owns workspace path validation (`canonical_root`, `checked_child`, `safe_relative`).
   - `core::fs::atomic_write` handles secure file creation with mode setting (`0o600`/`0o755`), directory creation, and symlink validation.
   - `core::fs::atomic_replace` handles same-directory atomic replacement preserving existing file mode and umask for milestone markdown contracts.
   - Both operations share internal atomic temp-file write and rename routines (`commit_atomic_temp`) to eliminate boilerplate duplication while preserving distinct permission semantics.
3. **System Memory and Budgets (`crate::core::sys`)**:
   - `core::sys` is the sole owner of Linux cgroup v1/v2 memory parsing, headroom computation, and memory budget calculations.

## 4. Module Layout and `build.rs` Conventions

> [!IMPORTANT]
> Invariant: `#[path = "..."]` attributes are permitted only when required by `build.rs` code generation and `OUT_DIR` language profile registration. Internal submodules must use standard Rust module hierarchy once the root path resolves to `mod.rs`.

- When decomposing an oversized module, convert it into an idiomatic directory module (`<name>/mod.rs` with sibling submodules `<name>/<submodule>.rs`).
- `build.rs` generates language profile registrations into `OUT_DIR/language_profiles.rs`. Modifying language profile file discovery requires updating `build.rs` in the same scope.

## 5. Verification Gates

Before delivering any source code change, verify the required gates from [TESTS.md](../TESTS.md):

1. **Task-Level Proof**: Run the focused test seam proving the admitted change.
2. **Target Gate**: Run the complete affected test target:
   ```bash
   cargo test --test <affected-target>
   ```
3. **Strict Linter**:
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```
4. **Milestone Handoff Gate**: Run the single compound verification command:
   ```bash
   cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
   ```
