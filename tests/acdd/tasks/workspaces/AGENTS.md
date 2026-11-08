# Workspace Test Routes

- Keep linked worktree and repository identity checks in [worktrees.rs](worktrees.rs).
- Keep scope and profile confinement checks in [confinement.rs](confinement.rs).
- Keep snapshot and delivery controls in [snapshots.rs](snapshots.rs).
- Keep the Git-heavy snapshot recovery workflow in [snapshot_recovery.rs](snapshot_recovery.rs).
- Run every snapshot, GC, crash, recovery, and handoff variant in the default lane; reuse physical seeds without dropping assertions.
- Keep real Git, SQLite, and child-process mechanics in the `git.rs`, `storage.rs`, and `process.rs` extensions of `TaskHarness`; keep scenario deltas and contract assertions in the tests.
