# ContextUnity Forge MCP — Agent Router

Read [documentation instructions](docs/AGENTS.md) before documentation changes.
Read [repository plans](docs/plans/README.md) for proposed and ongoing work.
Read [roadmap](docs/roadmap.md) for strategic context and
[milestones](docs/milestones/README.md) for admitted commitments.

## Routes

- Runtime behavior and setup: [README.md](README.md) and [`docs/`](docs/).
- Architecture: [indexing](docs/architecture/indexing.md).
- Reference: [configuration and tools](docs/reference/README.md).
- Operations: [runbooks](docs/runbooks/README.md).
- Verification: [testing](docs/testing/README.md).
- Forge code-graph workflow: [`contextunity-forge`](.agents/skills/contextunity-forge/SKILL.md).
- Test suite rules and boundaries: [`tests/AGENTS.md`](tests/AGENTS.md).

## Codebase Architecture

```text
src/
├── cli/       # CLI commands: build, scan, delta, query, docs, ast, guide, checkpoint
├── mcp/       # MCP JSON-RPC protocol server, tool router (tools.rs), and response limits
├── core/      # Models (models.rs), SQLite schema (schema.rs), Merkle tree (commitments.rs),
│              # response policy (response.rs), debug logger (debug_log.rs)
├── db/        # Storage engine:
│              #   writer.rs   -> Cold build & incremental delta indexing pipeline
│              #   reader.rs   -> Selectors, disambiguation, traversal, diagnostics
│              #   symbols.rs  -> Direct indexed exact queries and FTS5 search
│              #   traversal.rs-> Graph reachability, impact, slices, cycle analysis
└── engine/    # Analysis pipeline:
               #   scanner.rs  -> File walk, gitignore filtering, forge-mcp.yaml adapter
               #   ast/        -> AST extraction, relations.rs (calls, imports, mutates)
               #   languages/  -> Language profiles (Rust, Python, TS, Go, Java, etc.),
               #                  builtins, manifests (manifests.rs, build_manifest.rs)
               #   linker.rs   -> Cross-file symbol resolution, receiver inference,
               #                  external status classification, edge creation
```

## Benchmarks & Performance Profiling

All performance measurements, tool comparisons, and quality benchmarks live in `benchmarks/`:

- **Runner**: `python3 benchmarks/run_benchmarks.py --profile benchmarks/profiles/commerce-release-update.json`
- **MCP Quality Benchmark**: `python3 benchmarks/mcp_tool_quality_benchmark.py` (assesses answer completeness and agent usability against Codebase Memory)
- **MCP Latency & Cold Build Benchmark**: `python3 benchmarks/mcp_tool_comparison_benchmark.py`
- **Reference Workspace**: `/home/oleksii/ContextUnity/worktrees/commerce-release-update`
- **Policy**: Never commit manual profiling harnesses or `#[ignore]` benchmark tests into `tests/`. Use `benchmarks/` scripts.

## Test Rules & Boundaries

Read [`tests/AGENTS.md`](tests/AGENTS.md) before authoring, moving, or editing tests:

1. **No Absence / Negative Bug Probes**:
   - Do NOT write tests that merely assert the absence of an agent's historical hallucination or bug.
   - Tests must prove observable positive contracts, formal specifications, valid boundaries, or real fail-closed error states.
2. **No Micro-Spike Test Binaries**:
   - Every file directly in `tests/*.rs` is compiled and linked by Cargo as an independent executable.
   - Do NOT create a new `tests/*.rs` file for a single task, PR, or review round.
   - Group tests into existing domain test suites (`tests/languages/`, `tests/manifests.rs`, `tests/core_basics.rs`, `tests/python_semantics.rs`, `tests/typescript_semantics.rs`).
   - Keep test files <= 800 lines.
3. **Public Seams**:
   - Drive tests through public interfaces (CLI, MCP tool router, reader, or linker pipeline); do not construct tests around unexported private internals.

## Performance & Optimization Invariants

Every change touching scanner, AST extractors, linker, writer, or commitments
must respect these performance laws:

1. **Batching over N-queries**:
   - Never execute single-row `stmt.execute(params![...])` in loops over high-cardinality collections (coverage, dependencies, shared owners, edges, facts).
   - Use multi-value batches (`insert_multi_value_batch`) with SQLite parameter limits (`multi_value_batch_rows::<N>(tx)`).
   - Flush remaining batch buffers immediately after loops terminate.

2. **In-memory dictionary lookup over SQL JOINs in commitments**:
   - Leaf digest and Merkle root calculation over hundreds of thousands of rows must resolve paths, nodes, and keys using in-memory pre-loaded maps (`path_map`, `node_map`, `key_map`, `file_map`).
   - Do NOT issue 3-way or 4-way SQL `LEFT JOIN` queries against raw database tables during commitment calculation.

3. **Zero-allocation streaming hashing**:
   - Stream row bytes directly into incremental cryptographic hashers (`hasher.update(...)`).
   - Prohibit intermediary heap allocations (e.g. `Vec<u8>`, cloned DTO batches) per row across large record sets.

4. **Measured parallelism**:
   - Use Rayon (`into_par_iter()`) for CPU-bound sorting, hashing, or AST extraction only when the collection size warrants thread synchronization overhead (e.g. `>= 8192` records).

5. **Cold build latency regression gate**:
   - Verify performance-sensitive edits against the reference benchmark repository:
     `/home/oleksii/ContextUnity/worktrees/commerce-release-update`.
   - Compile release binary: `cargo build --release`.
   - Run cold build:
     `rm -f /tmp/bench-cru-cold.db* && ./target/release/contextunity-forge-mcp --root /home/oleksii/ContextUnity/worktrees/commerce-release-update --db /tmp/bench-cru-cold.db build --verbose`.
   - Compare `seal_ms`, `persist_graph_ms`, and `elapsed_ms`. Any unverified latency regression is a blocker.

## Semantic & Code Extraction Quality

1. **Precise AST symbol categorization**:
   - Language extractors must assign exact kinds (`function`, `method`, `class`, `type`, `interface`).
   - Top-level type aliases (e.g., Python PEP 695 `type_alias_statement`, `TypeAlias`, `TypeVar`, TypeScript `type_alias_declaration`, Rust `type_item`) must be indexed as `kind = "type"` with valid `qualname` so import linkers resolve them.

2. **Cross-language method semantics**:
   - Functions within classes, structs, or `impl` blocks receive `kind = "method"` with accurate receiver metadata (`is_method: true`, `is_static: bool`, `receiver_name`).

3. **Evidence & resolution fidelity**:
   - Distinguish `resolved`, `external`, `ambiguous`, and `unresolved`.
   - External dependencies and imports must be attributed via `external_origin` with import line verification instead of leaking into raw `unresolved`.

## Verification Guards

- Run all commands from this repository root following `Cargo.toml`.
- **Merkle tree determinism**: `cargo test --test commitment_integrity` must pass at all times.
- **Strict linting**: `cargo clippy --all-targets --all-features -- -D warnings` must produce 0 warnings.
- **Comprehensive test suite**: `cargo test --all-targets` must pass without regressions.
- **Git safety**: Obtain explicit user approval before `git commit`, `git push`, `git checkout <file>`, or `git restore`.
