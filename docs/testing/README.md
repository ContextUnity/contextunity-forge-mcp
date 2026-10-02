---
title: "Testing and verification"
doc_type: guide
---

# Testing and verification

## Test ownership

Read [tests/AGENTS.md](../../tests/AGENTS.md) before placing, moving, or changing
tests. It owns domain grouping, public seams, module size, and benchmark placement.
[Root instructions](../../AGENTS.md) own engineering and verification requirements.

## Rust verification

Run from the repository root with the current Cargo manifest. Match the command
to the proof boundary:

| Stage | Verification |
| --- | --- |
| Contract red and build green | Run the named test with a filter through its existing domain executable, for example `cargo test --test core_basics tasks::TEST_NAME`. Verify the expected red failure, then a green result. |
| Task build and review | Run the affected domain executable, for example `cargo test --test core_basics`, plus strict Clippy. Rerun focused checks after candidate changes. |
| Milestone deferred final test | Run the complete suite once after all task changes and the end-to-end test are in place. Record actual passed and failed counts for `milestone handoff`. |

Confirm a filtered test runs at least one test; an unmatched filter can exit 0
without proving the contract. Verify the intended failure on the red run and
the actual pass count on the green run.

Use the appropriate suite for a task outside `core_basics`. Run
`cargo test --test commitment_integrity` when changing commitment or storage
invariants and again at the milestone gate. The full milestone commands are:

```sh
cargo test --test commitment_integrity
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Performance-sensitive changes also use the benchmark procedure in root
instructions. Record the exact binary, workspace inventory, commands, and results
with the owning work. Documentation changes use metadata, link, scan-admission,
and retrieval checks as their focused proof.
