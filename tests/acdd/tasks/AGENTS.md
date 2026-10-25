# Task Test Sub-Suite — Domain Partitioning and Rules

This document defines the module boundaries and ownership rules for task tests under `tests/acdd/tasks/`.

## Domain Partitioning

| Module | Focus & Responsibility | Key Production Seams |
| :--- | :--- | :--- |
| [lifecycle.rs](lifecycle.rs) | Task state machine, claim, submit, reject, reset, revisions, transitions. | `src/engine/tasks/claim.rs`, `src/engine/tasks/gates.rs` |
| [seams.rs](seams.rs) | Static seam reachability, call hops, import/re-export resolution, stale index. | `src/engine/tasks/context.rs`, `src/db/reader.rs` |
| [boundaries.rs](boundaries.rs) | Public signature and commit-scope boundaries, trait methods, language callables (TS, Python, JS). | `src/engine/tasks/gates.rs` |
| [workspaces.rs](workspaces.rs) | Linked worktrees, multi-repo perimeters, scope confinement. | `src/engine/tasks/workspaces.rs` |
| [guidance.rs](guidance.rs) | Agent metadata, skill checks (`required_skills`), claim context, and RFC blackboard injection. | `src/engine/tasks/context.rs` |
| [retirement.rs](retirement.rs) | File retirement, build gate caller safety, legacy baseline revalidation. | `src/engine/tasks/gates.rs` |

## Authoring Rules

1. **Strict Sub-Module Placement**: Place new task tests in their corresponding domain file above based on the subsystem under test. Do not dump tests into `mod.rs`.
2. **Domain Separation**: Keep test concerns cleanly isolated. If a test verifies reachability resolution, it belongs in `seams.rs`; if it tests state transitions, it belongs in `lifecycle.rs`.
3. **Seam Verification**: Every test must prove a real production path via public or composition seams, never mock internal logic.
4. **Regression Preservation**: When refactoring or moving tests between files, ensure the total unique test count and coverage dimensions remain intact.

## File Size Ceiling

Keep each domain test module at or below 4,000 lines. When a module approaches the ceiling, split it along a distinct behavior boundary and preserve the collected test count.
