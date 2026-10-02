# Tests — Invariants and Test Creation Boundaries

This document defines the rules for placing, creating, refactoring, and maintaining tests in `contextunity-forge-mcp`.

## 1. Positive Contract Specifications
- Tests prove observable positive contracts, formal specifications, valid boundaries, and real fail-closed error states.
- Verify defect fixes through existing domain contract seams. Structure tests around permanent specification requirements rather than transient historical incidents.

## 2. Compilation and Binary Budget (Coherent Domain Modules)
- In Cargo, every `.rs` file directly in `tests/` is compiled and linked as an independent test executable.
- Organize tests into coherent domain modules:
  - Language extraction and semantics belong in language suites.
  - Linker, receiver inference, and manifest resolution belong in linker suites.
  - Commitments, scanner limits, and core storage belong in core suites.
  - MCP protocol, tool contracts, and response policies belong in MCP/query suites.
- Group new task tests into existing domain test suites.
- Table-driven tests over function duplication: iterate over structured data tables (`for case in cases { ... }`) for permutations and error codes.
- Shared test harnesses over repeated boilerplate: common setups (temporary SQLite stores, worktree layouts) belong in shared test helpers (e.g. `tests/common/`).
- Domain cohesion: keep coherent test suites unified within their subsystem domain (`tests/core_basics/tasks.rs`, `tests/manifests.rs`). Split tests strictly along natural architectural sub-boundaries when distinct domains emerge.

## 3. Public Seams Over Internal Mocks
- Drive tests through the narrowest real public or composition seam (engine scanner, linker pipeline, MCP tool context, or DB reader).
- Use real collaborating components (live SQLite connections, engine pipelines, filesystem structures). Place protocol fakes strictly outside the boundary being proved.

## 4. Benchmarking Isolation
- Place latency benchmarks, traversal stress tests, and tool comparisons in `benchmarks/`, managed by `benchmarks/run_benchmarks.py`.
