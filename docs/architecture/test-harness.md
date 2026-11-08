---
title: "Test Harness and Production Seam Architecture"
doc_type: architecture
---

# Test Harness and Production Seam Architecture

Integration and contract tests drive real production seams. Shared setup lives in
helpers; the `#[test]` owns the scenario mutation and every assertion.

Active hardening order and branch reality live in
`docs/plans/test-suite-diet-and-fixture-hardening.md`. Placement rules live in
`tests/AGENTS.md`. This page states only durable boundaries.

## Mental model

```text
#[test] scenario
  -> domain / shared harness (setup + verbs, >= 2 callers)
    -> production engine / store / MCP handler / extractor
```

> [!IMPORTANT]
> Invariant: Tests prove observable production behavior. Helpers must not replace
> the production pipeline with mockist doubles, and must not swallow contract
> assertions.
> - INV-TEST-SEAM-PRESERVING-DECOMPOSITION: Split scenarios without replacing
>   production collaborators with fakes inside the claimed boundary.
> - INV-TEST-HARNESS-BOUNDARY: The test states preconditions, triggers, and
>   assertions. Shared Git, SQLite, workspace, transport, and default evidence
>   construction live in harness or `tests/common` code with at least two callers.

## Layers

| Layer | Owns | Must not own |
| --- | --- | --- |
| Shared substrate (`tests/common`) | `Workspace` (tempdir + Git + build/delta helpers), `sqlite` query primitives, in-process `McpTestHarness` transport, shared in-process CLI runner | Domain assertions, one-off scenario mutations, suite-specific business logic |
| Domain harness (`*Harness`, local support) | Stateful domain verbs used by ≥2 tests in that suite (`claim_gate`, `advance_to`, `complete_task`, `handoff`, `seal`) | `assert!` on the contract under test; single-caller helpers; static functions unrelated to the harness state |
| Scenario (`#[test]`) | Case table or single mutation, direct composition of harness verbs, every assertion | Repeated low-level Git/SQL/tempdir plumbing, 100-line IIFE blocks returning one-off `*Observation` structs |

## Rules

1. **Harness returns values.** Prefer `Result`, receipts, and reusable domain snapshots.
   Put `assert!` / `assert_eq!` in the test.
2. **Two-caller rule without IIFE dumping.** One caller ⇒ express the scenario in the
   test by composing multi-caller primitives (`Workspace`, `sqlite::query_rows`,
   domain harness verbs). Never inline a 100-line procedural helper as a
   `(|| -> Result<_> { ... })()` IIFE block returning a single-use `*Observation`
   struct, and never leave zero-method `*Observation` struct shells in `harness.rs`
   or empty submodules.
3. **Cohesive harness methods (no static namespace dumping).** Every method on
   `<Domain>Harness` must construct or operate on that harness's domain state
   (`&self` / `&mut self`). Do not dump static Git commands, unrelated SQL queries,
   or file utilities onto `McpTestHarness` or `TaskHarness` merely to avoid raw calls
   in a test file; place shared workspace/Git helpers on `Workspace` and shared SQL
   helpers in `tests/common/sqlite.rs`.
4. **Canonical harness names (no type aliasing).** Never alias a harness type
   (`use McpTestHarness as Client`, `type Mcp = McpTestHarness`). Type aliases break
   static symbol/call-graph indexing and obscure harness ownership.
5. **Anti-mirror.** Name harness methods as domain operations (`advance_to`,
   `handoff`, `claim_gate`), never as `test_foo_helper`.
6. **Parameterize shared operations.** Prefer `complete_task(mode)` over N near-
   copies that differ by one flag.
7. **Shared seed, one mutation, bounded loop checks.** Rejection matrices build
   expensive state once, then change one field per row:
   - Never run full-database table dumps (`persisted_state`) or full cold index
     rebuilds inside N-iteration loops when lightweight invariants (`PRAGMA data_version`
     + `output_root`, bracketed by a full check before and after the loop) prove
     non-mutation.
   - Batch independent file-pair additions before running `delta` + cold parity
     checks instead of triggering $O(N^2)$ sequential cold rebuilds.
   - Clean up temporary `.forge/cold-*` databases and reuse extraction caches when
     testing linker/database parity rather than parser cold-cache misses.
8. **Real lifetime caching only.** Do not cache `Weak<Workspace>` when the caller
   drops the only `Arc<Workspace>` at the end of the test. Share read-only fixtures
   only when ≥2 tests in the binary reuse the exact same file set; otherwise build
   a local `Workspace` directly.
9. **Fail-loud fixture builders.** Happy-path fixture builders (`fixture_integration_evidence`,
   `observe_fixture_evidence`) must fail immediately on unexpected setup errors.
   Never silently fall back to `"unavailable"` digests or swallow non-contract command
   failures; provide separate explicit constructors for malformed-input tests.
10. **In-process default & in-memory when sufficient.** Call engine/task/MCP/CLI APIs
    in-process and use in-memory SQLite/AST fixtures unless filesystem or subprocess
    behavior is the explicit contract under test.
11. **No meta-test bloat.** Do not build multi-hundred-line test suites that only test
    test-only helpers (`SeedGitTemplate`, `FixtureSnapshot`, `McpTestHarness`). Keep
    production test budgets focused on production seams.
12. **Split by domain cohesion, not arbitrary line counts.** Decompose multi-subsystem
    monoliths (>1,500–2,000 lines mixing unrelated contracts such as `tests/acdd/mcp.rs`,
    `tests/mcp/context.rs`, `tests/mcp/tool_evolution/mcp_boundaries.rs`) into focused
    domain submodules. Never mechanically split a single cohesive domain file (e.g.,
    800–1,600 lines testing one tool or language surface) or hide test assertions in
    harness helpers just to hit an arbitrary 500/800-line ceiling.

## Existing harnesses

Use the harness already owned by the suite. Do not invent a parallel framework or
cross-mount another suite's private files via `#[path = "../..."]`.

| Harness | Suite | Role |
| --- | --- | --- |
| `TaskHarness` | `tests/acdd/tasks` | Task claim/submit/lifecycle |
| `MilestoneHarness` | `tests/acdd/milestones` | Milestone sync/handoff/fixtures |
| `McpTestHarness` | `tests/common/mcp_harness.rs` + `tests/mcp` | In-process MCP requests/tools (transport + session only) |
| `CommitmentHarness` | `tests/commitment_integrity` | In-memory/disk commitment sealing, verification, and savepoint tamper probes |
| Language / framework support | `tests/languages`, `tests/framework_manifests` | In-memory extract/index helpers and shared-cache cold parity checks |
| `PlanHarness` / `WorkflowHarness` | `tests/acdd/harness` | Markdown/profile extract fixtures only until AC-003 admits execution |

God-object growth, static utility dumping, and orphaned `*Observation` struct
modules in domain harnesses are defects. Prefer small, composable primitives on the
owning type over scenario-specific wrappers.
