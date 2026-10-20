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
`build` proof. This focused contract/build proof uses the narrowest real
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
The embedded ACDD profile registers the combined command
`cargo clippy --all-targets --all-features -- -D warnings && cargo test`.
Claim context supplies `workflow_guidance.scoped_commands` with the contract
seam and indexed covering-test targets appended as `--test <target>`; use the
command matching the affected proof and record its exact string.

## Milestone handoff gate

Before `milestone handoff`, run each command from the repository root:

1. `cargo test --test commitment_integrity`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all-targets`

Record the complete command in the final integration evidence's
`verification.command`:
`cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`.
Set `verification.exit_code` to `0`, `tests_passed` to the sum of passed counts
from `cargo test --all-targets`, and `tests_failed` to `0`; count the standalone
commitment run only once and exclude Clippy from test counts.

Prepare the binding with `milestone handoff-context <id> --target-base main` and
obtain independent integration review against that exact final content and base.
Follow the documentation-agent scan in [AGENTS.md](AGENTS.md#documentation-scan-before-handoff)
before final verification. Supply the completed review and verification record to
`milestone handoff <id> --evidence-file /tmp/handoff-evidence.json`; see the
[record fields](docs/reference/tasks.md#milestone-cli-lifecycle). Forge validates
and records supplied evidence without running verification. Changed content or
authority requires fresh final review and verification.

Use `test-suite-refactor` only when the admitted change splits, replaces,
deduplicates, or removes a typed test suite. It is not a general final-verification
step.
