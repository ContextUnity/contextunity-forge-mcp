# Milestone Test Domains

- Keep initialization and discovery cases in `discovery.rs`, lifecycle cases in `lifecycle.rs`, and verification and evidence cases in `verification.rs`.
- Keep handoff lifecycle in `handoff.rs`, journal recovery in `recovery.rs`, history rewrites in `history.rs`, receipts in `receipts.rs`, and delivery rollup in `delivery.rs`.
- Drive all milestone domain tests through `MilestoneHarness` (`tests/acdd/milestones/harness.rs` or `harness/`). Never invoke Git CLI subprocesses or execute raw SQLite SQL directly in test functions.
- Reuse canonical templates in `tests/acdd/fixtures/milestones/` or `MilestoneFixtureBuilder`. Never inline multi-line markdown heredocs.
- Run `cargo test --test acdd milestones` for the milestone suite and all L1 matrix cases.
- Split a module only on a real domain boundary after deleting duplicates. Do not chase line-count ceilings or grow `MilestoneHarness` with one-caller helpers.
