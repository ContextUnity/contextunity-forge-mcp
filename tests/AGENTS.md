# Tests — Invariants and Test Creation Boundaries

This document defines the rules for placing, creating, refactoring, and maintaining tests in `contextunity-forge-mcp`.

## 1. Positive Contract Specifications
- Tests prove observable positive contracts, formal specifications, valid boundaries, and real fail-closed error states.
- Verify defect fixes through existing domain contract seams. Structure tests around permanent specification requirements rather than transient historical incidents.
- Prove admitted milestone requirements. Do not add a test that invents an uncontracted requirement in order to justify a new subsystem.
<!-- acdd-forge:test-invariants:start -->
- **Additive seam evolution**: When extending existing capabilities, extend existing domain seam tests by adding parameterized test cases (table rows in `for case in cases`) ONLY for new input variations of the same unit/capability.
- **Domain decomposition over monoliths**: NEVER append unrelated sub-scenarios, review remediation blocks, or multi-step integration workflows into an existing test function. Each distinct capability or task contract MUST be authored as a separate, descriptively-named `#[test]` function in the appropriate domain module. This preserves test independence, thread parallelization, and pinpoint failure diagnostics.
- **Fixture boundaries**: Keep unit and extractor tests fast and in-memory. Do not construct heavy filesystem fixtures, multi-crate workspaces (`Workspace::new()`), or thread pools inside syntax/AST extractor tests; isolate integration and workspace probes into dedicated integration modules or tests.
- **Subsume duplicate tests**: When a strengthened seam covers the full contract, remove or replace weaker/duplicate tests in the same test diff rather than leaving redundant assertions and duplicate setups.
- **Runtime-red contract proof**: Contract tests for extensions or greenfield must compile cleanly and fail at runtime on assertions (exit code 101). Never introduce compilation failures or unresolvable imports, as this breaks AST indexing and blocks gate submission.
<!-- acdd-forge:test-invariants:end -->
- For semantic receiver evidence, cover exact resolution separately from possible targets. Exercise complete and incomplete joins, unknown alternatives, overflow, and later unconditional writes through the linker and persisted query seam; possible targets must not appear as exact `calls` edges.

## 2. Compilation and Binary Budget (Coherent Domain Modules)
- In Cargo, every `.rs` file directly in `tests/` is compiled and linked as an independent test executable.
- Organize tests into coherent domain modules:
  - `tests/acdd.rs` (`tests/acdd/`): Task lifecycles, subtasks, blackboard coordination, and milestone operations (`INV-ACDD-SUITE-COHESION`). Read [tests/acdd/AGENTS.md](acdd/AGENTS.md) for ACDD sub-suite partitioning and authoring rules.
  - `tests/cli_roots.rs`: CLI entrypoints, root discovery, and path normalization across subcommands.
  - `tests/core.rs` (`tests/core/`): Storage engine, SQLite schema, adapters, scanner limits, and coverage diagnostics.
  - `tests/impact_context.rs`: Impact analysis, traversal reachability, and blast-radius evaluation context.
  - `tests/incremental.rs` (`tests/incremental/`): Delta indexing, resolution identity, module and directory scoping.
  - `tests/languages.rs` (`tests/languages/`): Language extraction, AST patterns, and language-specific semantics (`INV-CENTRAL-VS-LANGUAGE-ISOLATION`).
  - `tests/framework_manifests.rs` (`tests/framework_manifests/`): Framework manifest activation and operations, dependency discovery, CommonJS, and manifest deltas.
  - `tests/linked_workspaces.rs`: Multi-workspace links, cross-root dependencies, and workspace boundaries.
  - `tests/linker.rs` (`tests/linker/`, `tests/receivers/`): Cross-file symbol linking, receiver inference, and overload disambiguation.
  - `tests/mcp.rs` (`tests/mcp/`, `tests/mcp_context/`): MCP JSON-RPC protocol server, tool contracts, freshness, and response compaction (includes `tests/mcp_context/ast_search.rs` via `tests/mcp/context.rs`).
  - `tests/query_context.rs` (`tests/query_context/`): Query options, symbol search filtering, and query execution context.
  - `tests/commitment_integrity.rs`: Merkle trees, leaf digest stability, and deterministic cryptographic commitments.
  - `tests/common/`: Shared fixtures, RAII temp workspaces, and MCP test clients (`INV-SHARED-HARNESS`).
- Group new task tests into existing domain test suites.
- **Domain modularity over monolithic files**: Organize test files strictly along cohesive architectural and domain boundaries. Do not accumulate unrelated test concerns into single monolithic files; when distinct behavioral domains emerge (e.g. lifecycle, reachability, boundaries, workspaces), split them into domain sub-directories with their own `mod.rs` and local `AGENTS.md`.
- Keep framework-manifest tests in `tests/framework_manifests/`; keep HTML template and relation tests in `tests/languages/html/`.
- Keep TypeScript framework tests under `tests/languages/typescript/frameworks/`, with parser-selection coverage in `grammar.rs`.
- Keep receiver tests under `tests/receivers/{rust,python}/`; Rust module-path cases stay under `rust/module_paths.rs`.
- Each test subtree maintains its own local `AGENTS.md` specifying module scope, shared helpers, and verification rules:
  - [`tests/acdd/AGENTS.md`](acdd/AGENTS.md): ACDD gate AST, runners, and task lifecycle domain partitioning.
  - [`tests/acdd/tasks/AGENTS.md`](acdd/tasks/AGENTS.md): Decomposed ACDD task suite modules (`boundaries.rs`, `guidance.rs`, `lifecycle.rs`, `seams.rs`, `workspaces.rs`).
  - [`tests/framework_manifests/AGENTS.md`](framework_manifests/AGENTS.md): Declarative framework packs and TOML manifests.
  - [`tests/languages/html/AGENTS.md`](languages/html/AGENTS.md): HTML templates, Django/Jinja syntax, and HTMX/Alpine relations.
  - [`tests/languages/typescript/frameworks/AGENTS.md`](languages/typescript/frameworks/AGENTS.md): NestJS, Nuxt, and Vue runtime test suites.
  - [`tests/receivers/AGENTS.md`](receivers/AGENTS.md): Value flow receiver tests routing and structure.
  - [`tests/receivers/rust/AGENTS.md`](receivers/rust/AGENTS.md): Rust field chains, trait dispatch, deref pointers, and module paths.
  - [`tests/receivers/python/AGENTS.md`](receivers/python/AGENTS.md): Python C3 MRO, mixins, and decorator flow.
- Fast submodule test execution commands:
  ```bash
  cargo test --test framework_manifests manifests::<module>::
  cargo test --test languages html::<module>::
  cargo test --test languages typescript::frameworks::<module>::
  cargo test --test linker typed_receivers::rust::<module>::
  cargo test --test linker typed_receivers::python::<module>::
  cargo test --test linker value_flow::
  ```
- Table-driven tests over function duplication: iterate over structured data tables (`for case in cases { ... }`) for permutations and error codes.
- Shared test harnesses over repeated boilerplate: common setups (temporary SQLite stores, worktree layouts) belong in shared test helpers (`tests/common/`, `tests/acdd/support.rs`).
- Domain cohesion: keep coherent test suites unified within their subsystem domain. Split tests strictly along natural architectural sub-boundaries when distinct domains emerge.

## 3. Suite moves
- When moving, renaming, splitting, or removing a test suite or test file, update `docs/testing/README.md` and this file in the same `build`.
- Keep public value-flow reducer cases in `tests/linker/value_flow.rs`; keep private reducer implementation checks beside their source.

## 4. Public Seams Over Internal Mocks

Drive tests through public production seams, reuse shared fixtures/factories, and verify permutations through structured test case tables.
- Drive tests through the narrowest real public or composition seam (engine scanner, linker pipeline, MCP tool context, or DB reader).
- Use real collaborating components (live SQLite connections, engine pipelines, filesystem structures). Place protocol fakes strictly outside the boundary being proved.

## 5. Benchmarking Isolation
- Place latency benchmarks, traversal stress tests, and tool comparisons in `benchmarks/`, managed by `benchmarks/run_benchmarks.py`.
