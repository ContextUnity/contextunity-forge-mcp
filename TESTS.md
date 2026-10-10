---
title: "Tests and Verification"
doc_type: guide
---

# Tests and Verification

Read this file for repository-wide test gates. Read [tests/AGENTS.md](tests/AGENTS.md)
for test placement, authoring boundaries, public-seam requirements, and benchmarking
isolation.

## Task-level proof

Run the focused test command required by the admitted task contract. Record the
exact command, exit code, passing count, and failing count in the active
`build/v1` proof. This focused contract/build proof uses the narrowest real
production seam that proves the task; it is separate from the affected-target
gate below.

At both task build and review, run the complete affected test target without a
filter: `cargo test --test <affected-target>`. Record the exact target command,
exit code, passing count, and failing count in the build/review evidence. Do not
use a focused test command in place of this target gate. If a target cannot pass
because of an external environment issue, record the failing test and its cause
in the evidence without changing or omitting the required command.

For Rust task builds, also run
`cargo clippy --all-targets --all-features -- -D warnings`. Record its exact
command and exit code with the build evidence; Clippy contributes no test count.

## Milestone handoff gate

Before `milestone handoff`, run each command from the repository root:

1. `cargo test --test commitment_integrity`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all-targets`

Pass the complete gate as the `--verification-command` string:
`cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`.
Set tests-passed to the sum of the passed counts reported by each test target in
`cargo test --all-targets`.
Set tests-failed to 0. Clippy contributes no test count. Record the exact command
and these counts in the handoff receipt.
Run the verification yourself before invoking `milestone handoff`; that command
records the supplied result and does not execute the verification command.

Use `test-suite-refactor` only when the admitted change splits, replaces,
deduplicates, or removes a typed test suite. It is not a general final-verification
step.
