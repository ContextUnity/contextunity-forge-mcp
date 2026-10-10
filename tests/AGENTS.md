# Tests — Invariants and Test Creation Boundaries

This document defines the rules for placing, creating, refactoring, and maintaining tests in `contextunity-forge-mcp`.

## 1. Positive Contract Specifications
- Tests prove observable positive contracts, formal specifications, valid boundaries, and real fail-closed error states.
- Verify defect fixes through existing domain contract seams. Structure tests around permanent specification requirements rather than transient historical incidents.
- Prove admitted milestone requirements. Do not add a test that invents an uncontracted requirement in order to justify a new subsystem.

## 2. Compilation and Binary Budget (Coherent Domain Modules)
- In Cargo, every `.rs` file directly in `tests/` is compiled and linked as an independent test executable.
- Organize tests into coherent domain modules:
  - `tests/acdd.rs` (`tests/acdd/`): Task lifecycles, subtasks, blackboard coordination, and milestone operations (`INV-ACDD-SUITE-COHESION`).
  - `tests/cli_roots.rs`: CLI entrypoints, root discovery, and path normalization across subcommands.
  - `tests/core.rs` (`tests/core/`): Storage engine, SQLite schema, adapters, scanner limits, and coverage diagnostics.
  - `tests/impact_context.rs`: Impact analysis, traversal reachability, and blast-radius evaluation context.
  - `tests/incremental.rs` (`tests/incremental/`): Delta indexing, resolution identity, module and directory scoping.
  - `tests/languages.rs` (`tests/languages/`): Language extraction, AST patterns, framework manifests, and language-specific semantics (`INV-CENTRAL-VS-LANGUAGE-ISOLATION`).
  - `tests/linked_workspaces.rs`: Multi-workspace links, cross-root dependencies, and workspace boundaries.
  - `tests/linker.rs` (`tests/linker/`, `tests/receivers/`): Cross-file symbol linking, receiver inference, and overload disambiguation.
  - `tests/mcp.rs` (`tests/mcp/`, `tests/mcp_context/`): MCP JSON-RPC protocol server, tool contracts, freshness, and response compaction (includes `tests/mcp_context/ast_search.rs` via `tests/mcp/context.rs`).
  - `tests/query_context.rs` (`tests/query_context/`): Query options, symbol search filtering, and query execution context.
  - `tests/commitment_integrity.rs`: Merkle trees, leaf digest stability, and deterministic cryptographic commitments.
  - `tests/common/`: Shared fixtures, RAII temp workspaces, and MCP test clients (`INV-SHARED-HARNESS`).
- Group new task tests into existing domain test suites.
- Table-driven tests over function duplication: iterate over structured data tables (`for case in cases { ... }`) for permutations and error codes.
- Shared test harnesses over repeated boilerplate: common setups (temporary SQLite stores, worktree layouts) belong in shared test helpers (`tests/common/`).
- Domain cohesion: keep coherent test suites unified within their subsystem domain (`tests/acdd/tasks.rs`, `tests/languages/manifests.rs`). Split tests strictly along natural architectural sub-boundaries when distinct domains emerge.

## 3. Suite moves
- When moving, renaming, splitting, or removing a test suite or test file, update `docs/runbooks/acdd.md`, `docs/testing/README.md`, and this file in the same `build`.

## 4. Public Seams Over Internal Mocks

Drive tests through public production seams, reuse shared fixtures/factories, and verify permutations through structured test case tables.
- Drive tests through the narrowest real public or composition seam (engine scanner, linker pipeline, MCP tool context, or DB reader).
- Use real collaborating components (live SQLite connections, engine pipelines, filesystem structures). Place protocol fakes strictly outside the boundary being proved.

## 5. Benchmarking Isolation
- Place latency benchmarks, traversal stress tests, and tool comparisons in `benchmarks/`, managed by `benchmarks/run_benchmarks.py`.
