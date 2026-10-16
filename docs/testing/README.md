---
title: "Testing and verification"
doc_type: guide
---

# Testing and verification

## Test ownership

Read [tests/AGENTS.md](../../tests/AGENTS.md) before placing, moving, or changing
tests. It owns domain grouping, public seams, and benchmark placement.
[Root instructions](../../AGENTS.md) own engineering requirements;
[`TESTS.md`](../../TESTS.md) owns the required test and lint commands.

## Rust verification

Run from the repository root with the current Cargo manifest. Match the command
to the proof boundary. [`TESTS.md`](../../TESTS.md) owns the exact required
commands and the milestone handoff gate.

| Stage | Verification |
| --- | --- |
| Contract and build | Run the named test through its existing domain executable, for example `cargo test --test acdd tasks::TEST_NAME`. `seam-test-first` requires red then green. `direct-proof` and `deferred-final-test` accept exit code 0 at contract. All policies require passing build proof. |
| Task build and review | Run the affected domain executable, for example `cargo test --test acdd`, plus strict Clippy. Rerun focused checks after candidate changes. |
| Milestone deferred final test | Follow [`TESTS.md`](../../TESTS.md#milestone-handoff-gate) once after all task changes and the end-to-end test are in place. |

Confirm a filtered test runs at least one test; an unmatched filter can exit 0
without proving the contract. Verify the intended failure on the red run and
the actual pass count on the green run.

Use the appropriate domain suite for a task outside `acdd` (`core`, `languages`,
`linker`, `incremental`, `mcp`). Run `cargo test --test commitment_integrity`
when changing commitment or storage invariants. When test targets or suites
change, update the affected instructions during `build` per the
[ACDD contract](../reference/acdd.md#documentation-and-instructions-co-evolution).

Performance-sensitive changes also use the benchmark procedure in root
instructions. Record the exact binary, workspace inventory, commands, and results
with the owning work. Documentation changes use metadata, link, scan-admission,
and retrieval checks as their focused proof.

## Semantic resolution evidence

Exercise receiver and candidate contracts through the linker and, when they
affect persisted or public results, through indexing and query readers. Keep
exact call edges distinct from possible-target evidence. Assert both positive
resolution and negative cases for unknown alternatives, unsupported syntax,
incomplete joins, and candidate overflow; do not use name-only fixtures as proof
of a unique target. Query tests cover paged possible calls and analysis gaps;
impact tests cover candidate callers and removal safety. The Milestone 046 admission baseline is recorded in
[ADR 0008](../adr/0008-bounded-static-value-flow.md).

For language resolution and cross-language framework composition, run the full
`cargo test --test linker` and `cargo test --test languages` targets. The
`incremental` target checks that cached and cold graph output remain equivalent.
