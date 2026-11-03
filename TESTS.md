---
title: "Tests and Verification"
doc_type: guide
---

# Tests and Verification

Read this file for repository-wide test gates. Read [tests/AGENTS.md](tests/AGENTS.md)
for test placement, authoring boundaries, public-seam requirements, and benchmarking
isolation.

## Task-level proof

Run the focused test command required by the admitted task contract (e.g. `cargo test --test <target> <seam_filter>` or scoped test module). Record the exact command, exit code, passing count, and failing count in the active `build` and `review` evidence. This focused test proof uses the narrowest real production seam that proves the task without forcing slow, unrelated suite execution during task iteration.

For Rust task builds and reviews, run strict Clippy alongside the focused test:
`cargo clippy --all-targets --all-features -- -D warnings`. Record its exact command and exit code with the build evidence; Clippy contributes no test count.
The embedded ACDD profile registers the scoped verification command:
`cargo clippy --all-targets --all-features -- -D warnings && cargo test`.
Claim context supplies `workflow_guidance.scoped_commands` with the contract seam and indexed covering-test targets; use the scoped command matching the task proof (with test filter) and record its exact string. Full unfiltered suite execution (`cargo test --all-targets`) is deferred to the milestone handoff gate.

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
[record fields](docs/reference/tasks.md#milestone-cli-lifecycle). Handoff context
commits admitted working-tree polish before producing the review binding. Run the
required commands above on that content and obtain fresh independent review.
For source polish, select its registered project verification command through
`verification.project_command`; handoff executes it again and records the observation.
Select the registered lint command explicitly through `verification.lint_command`;
only its actual successful execution reports clean lint. These observations do not
replace the mandatory commands or their count rules. Changed content or authority
requires fresh final review and verification.

Use `test-suite-refactor` only when the admitted change splits, replaces,
deduplicates, or removes a typed test suite. It is not a general final-verification
step.
