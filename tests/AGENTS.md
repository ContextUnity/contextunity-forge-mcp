# Tests — Invariants and Test Creation Boundaries

This document defines the rules for placing, creating, refactoring, and maintaining tests in `contextunity-forge-mcp`.

## 1. No Absence / Negative Bug Probes (Заборона "анти-баг" тестів)
- **Do NOT write tests merely asserting the absence of an ephemeral agent bug or hallucination.**
- Tests must assert observable positive contracts, formal specifications, valid boundaries, or real fail-closed error states.
- If a bug occurred due to flawed logic, fix the production logic and verify it through the public contract seam. Do NOT create dedicated single-bug regression probes (e.g., `audit_regressions.rs`, `profile_review_regressions.rs`, or tests asserting that "symbol X does not produce historical bug Y").

## 2. Compilation and Binary Budget (Заборона мікро-файлів тестів)
- In Cargo, every `.rs` file directly in `tests/` is compiled and linked as an independent test executable.
- **Do NOT create a new `tests/*.rs` file for individual PRs, review rounds, or prompt sub-tasks.**
- Tests must be organized into coherent domain modules rather than scattered across dozens of 30-line files:
  - Language extraction and semantics belong in language suites.
  - Linker, receiver inference, and manifest resolution belong in linker suites.
  - Commitments, scanner limits, and core storage belong in core suites.
  - MCP protocol, tool contracts, and response policies belong in MCP/query suites.
- Keep test modules under 800 lines. Split only when a domain module exceeds the size cap, along clean architectural sub-boundaries.

## 3. Public Seams Over Internal Mocks
- Drive tests through the narrowest real public or composition seam (engine scanner, linker pipeline, MCP tool context, or DB reader).
- Fake only external system boundaries; do not mock internal engine components.

## 4. No Benchmarks in Integration Test Suites
- Latency benchmarks, traversal stress tests, and tool comparisons belong in `benchmarks/`, managed by `benchmarks/run_benchmarks.py`.
- Do NOT add `#[ignore]`-tagged manual benchmarking harnesses to `tests/`.
