# ContextUnity Forge MCP — Agent Router

Read [documentation instructions](docs/AGENTS.md) before documentation changes.
Read [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md) for structural constraints.
Read [roadmap](docs/roadmap.md) for strategic context and
[milestones](docs/milestones/README.md) for admitted commitments.

## Routes

- Runtime behavior and setup: [README.md](README.md) and [`docs/`](docs/).
- Architecture: [indexing](docs/architecture/indexing.md).
- Reference: [configuration and tools](docs/reference/README.md), [task operations](docs/reference/tasks.md), [ACDD](docs/reference/acdd.md), and [CLI commands](docs/reference/cli.md).
- Operations: [runbooks](docs/runbooks/README.md) and [ACDD execution runbook](docs/runbooks/acdd.md).
- Verification: [testing](docs/testing/README.md).
- Planning and execution queue: [roadmap](docs/roadmap.md), [milestones](docs/milestones/README.md), and [plans](docs/plans/README.md).
- Architecture & Decisions: [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md).
- Forge code-graph workflow: the shared `contextunity-forge` skill.
- Test suite rules and boundaries: [`tests/AGENTS.md`](tests/AGENTS.md).

## Task execution and ownership

- Use the shared `contextunity-forge` skill for tool navigation and follow the [execution runbook](docs/runbooks/acdd.md). Read returned workflow guidance for the active task gate.
- Read the [ACDD contract](docs/reference/acdd.md) for gates, task taxonomy, and proof policies; use the [task reference](docs/reference/tasks.md) for schemas and state transitions.
- Discover admitted commitments through `contextunity-forge-mcp milestone list` and `milestone show <id> --full`.
- Use a dedicated milestone worktree and a separate child worktree for every parallel writer. The runbook owns creation, claim, integration, and cleanup order.
- Verify imported task specifications against the current receipt schema; preserve runtime source while reconciling documents.
- Route adjacent defects through their owning task or admitted scope. Record deferred findings in milestone `deferred_defects` or a linked successor contract before delivery.
- Use Forge for graph discovery, AST tools for code patterns, and `lean-ctx` for source reads and shell commands. Use task MCP/CLI operations for execution state.
- Apply the [test instructions](tests/AGENTS.md) for test placement and the runbook for verification cadence.
- Follow explicit user authorization for maintenance outside milestones and all Git commits, merges, publication, and cleanup. Use the runbook's post-merge installation command for Forge releases.

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

## Architectural Discovery & Prior Art Inspection

Before designing or introducing new tables, indices, extractors, or pipeline stages:
1. **Inspect Existing Code & Symbols**:
   - Use `code_map_explain` with `show_doc: true` or `code_map_inspect` with `show_doc: true` to discover the purpose and contracts of existing subsystems.
   - Use `code_map_search` with exact/pattern selectors to verify whether a symbol or helper already exists before writing duplicate functionality.
2. **Read Architectural Documentation First**:
   - Use `get_doc` or `search_docs` to read the relevant ADRs (`docs/adr/`) and architecture guides (`docs/architecture/`).
   - Check `src/core/schema.rs` and existing virtual FTS tables (`node_search`, `doc_search`) before proposing any new index or schema modification.
3. **Prohibition on Redundant Reinvention**:
   - Never create duplicate parallel mechanisms (e.g. creating a new FTS table for files when `node_search` already indexes symbol tokens and `files` indexes file paths). Always build upon established architectural seams.

## Benchmarks & Performance Profiling

All performance measurements, tool comparisons, and quality benchmarks live in `benchmarks/`:

- **Runner**: `python3 benchmarks/run_benchmarks.py --profile benchmarks/profiles/commerce-release-update.json`
- **MCP Quality Benchmark**: `python3 benchmarks/mcp_tool_quality_benchmark.py` (assesses answer completeness and agent usability against Codebase Memory)
- **MCP Latency & Cold Build Benchmark**: `python3 benchmarks/mcp_tool_comparison_benchmark.py`
- **Reference Workspace**: `../../worktrees/commerce-release-update` (relative to Forge repository root)
- **Policy**: Never run benchmarks autonomously without explicit user approval. Never commit manual profiling harnesses or `#[ignore]` benchmark tests into `tests/`. See [`benchmarks/AGENTS.md`](benchmarks/AGENTS.md).

### Benchmarking & Performance Gate Lifecycle
1. **Staged Gate Order**:
   Execute verification strictly in this order:
   1) Compile release binary (`cargo build --release`).
   2) Allow host to settle to idle baseline (verify background compilation has terminated).
   3) Measure performance via a single controlled run on the reference workspace (`benchmarks/run_benchmarks.py`).
   4) Run the full test suite (`cargo test --all-targets`).
2. **Controlled Isolation**:
   Profile on a quiet system to obtain accurate, repeatable throughput and latency receipts.
3. **Contention Validation**:
   When measured latency spikes unexpectedly, check the baseline under identical conditions to confirm whether host contention caused the difference.

## Test rules and boundaries

Read [tests/AGENTS.md](tests/AGENTS.md) before changing tests and the
[ACDD contract](docs/reference/acdd.md) for admitted proof requirements.
Keep profiling in `benchmarks/`; cap profiling iterations at three per turn.
Record measured results and unresolved acceptance budgets in milestone evidence.

## Performance & Optimization Principles

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
   - Use Rayon (`into_par_iter()`) for CPU-bound sorting, hashing, or AST extraction only when measurements show that the collection size warrants thread synchronization overhead (around 8,192 records is a useful initial profiling point, not a universal cutoff).

5. **Measured build performance**:
   - Profile cold builds end to end and report extraction, linking, persistence, indexing, sealing, verification, and total wall time without double-counting nested phases.
   - Treat `rows_ms` according to its implementation scope; do not describe aggregate persistence time as node-only latency.
   - Use the Commerce reference repository for comparable measurements. The preferred cold-build aspiration is about 10 seconds there; this is guidance outside milestone 030, which owns the active acceptance gates.

6. **Lean node metadata & projection law**:
   - `nodes.details` is an index-projection surface, NOT an AST fact dump or compiler analysis heap.
   - Prohibit serializing large interprocedural analysis trees, complete value-flow AST graphs, or raw scope maps into `nodes.details`.
   - Keep node details lean; around 120 bytes per node is a recommended reference, not a repository-wide gate.

7. **Storage density & compressed fact storage**:
   - Measure overall SQLite storage density per indexed source file and node. Around 45 KiB/file, 3 KiB/node, and a 3.5:1 Zstandard ratio are recommended reference values; milestone 030 owns the active storage gates.
   - Keep `local_facts.facts_blob` compressed and preserve complete durable facts during encode/decode.

8. **Bulk SQLite ingestion pragmas**:
   - Cold database builds and batch rebuilds must execute under non-syncing bulk pragmas (`PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;`), executing an explicit WAL checkpoint only upon build finalization before Merkle seal.

9. **Interactive tool latency recommendations**:
   - For planning and comparison, recommended values are around 10ms for exact/prefix search, 30ms for full-text search, 25ms for inspection/explanation, 50ms for impact/test traversal, and 30ms for scoped removal proof. These are guidance outside the milestone that explicitly admits a gate.

10. **Target-scoped evaluation law (No global scans in localized tools)**:
    - Interactive tools must never issue unindexed table scans (`LIKE '%...'`), unconstrained workspace-wide counts (`SELECT count(*) FROM table`), or global diagnostics during symbol-level operations. Safety checks must evaluate strictly within the target's dependency subgraph.

11. **Prohibition against redundant disk re-reads and full-source DB duplication**:
    - The scanner and AST extractors read workspace files once during the extraction phase.
    - Persistence pipelines (`persist_files`, `persist_graph`, etc.) must NEVER re-read files from disk (`fs::read_to_string`).
    - Never duplicate raw, uncompressed source code files into SQLite tables or virtual FTS tables. Forge stores code structure, symbols, signatures, and relations, NOT an uncompressed mirror of the filesystem.

12. **Zero-allocation hot-path law in graph persistence**:
    - Loops iterating over high-cardinality collections (edges, occurrences, dependencies) must never allocate ad-hoc heap collections (e.g. `HashSet` of multi-field tuples) or compute complex multi-field hashes on hot per-record paths. Deduplication must be stream-oriented, batch-oriented, or handled via ordered sorting without CPU cache thrashing.

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
