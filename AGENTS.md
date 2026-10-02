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
- Forge code-graph workflow: [`contextunity-forge`](.agents/skills/contextunity-forge/SKILL.md).
- Test suite rules and boundaries: [`tests/AGENTS.md`](tests/AGENTS.md).

## Task Execution & Repository Milestones

This repository directly owns its execution queue and task commitments:
- **Macro direction**: [docs/roadmap.md](docs/roadmap.md).
- **Execution queue & milestones**: [`docs/milestones/`](docs/milestones/) ordered by numeric prefix (`010-*.md`, `020-*.md`).
- **Research & proposal drafts**: [`docs/plans/`](docs/plans/README.md).
- **Milestone discovery**: Run `contextunity-forge-mcp milestone list` to inspect active and planned milestones and task completion ratios. Run `contextunity-forge-mcp milestone show <id-or-prefix> --full` to read the selected contract and task descriptions.
- **Milestone creation**: Run `contextunity-forge-mcp milestone init --plan <path>` to scaffold the next numbered contract. Use `--active` to start its active development clock at creation.
- **Milestone closure**: Run `contextunity-forge-mcp milestone handoff <id-or-prefix> --verification-command <command> --tests-passed <count> --tests-failed 0` after every SQLite task reaches `completed`. The command records a structured receipt and moves the milestone into `docs/milestones/archive/`.
- **Operational task lifecycle**: Tasks are stored in `.forge/tasks.sqlite` (configured via `forge-mcp.yaml`). Query executable tasks using `task_list` or `task list` (defaults to `ready`). Claim and submit with direct JSON proof, and exchange temporary context with `task_blackboard` or `task blackboard`. Passing `deliver/v1` writes the durable task receipt into the milestone and clears task messages. Read [task operations](docs/reference/tasks.md) for gate and API details.

## Development Worktrees

For isolated subagent work, parallel branches, or spikes:
- Create worktrees under `.worktrees/<branch-name>`:
  `git worktree add .worktrees/<branch-name> <branch-name>`
- `.worktrees/` is gitignored at repository root to keep untracked workspaces clean.
- On merge, inspect the governing milestone with `milestone show`, reconcile task receipts, and run `milestone handoff` after all tasks complete per [docs/AGENTS.md](docs/AGENTS.md).
- Prune worktrees when finished: `git worktree remove .worktrees/<branch-name>`.

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
- **Reference Workspace**: `/home/oleksii/ContextUnity/worktrees/commerce-release-update`
- **Policy**: Never commit manual profiling harnesses or `#[ignore]` benchmark tests into `tests/`. Use `benchmarks/` scripts.

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

## Test Rules & Boundaries

Read [`tests/AGENTS.md`](tests/AGENTS.md) before authoring, moving, or editing tests:

1. **No Absence / Negative Bug Probes**:
   - Do NOT write tests that merely assert the absence of an agent's historical hallucination or bug.
   - Tests must prove observable positive contracts, formal specifications, valid boundaries, or real fail-closed error states.
2. **No Micro-Spike Test Binaries**:
   - Every file directly in `tests/*.rs` is compiled and linked by Cargo as an independent executable.
   - Do NOT create a new `tests/*.rs` file for a single task, PR, or review round.
   - Group tests into existing domain test suites (`tests/languages/`, `tests/manifests.rs`, `tests/core_basics.rs`, `tests/python_semantics.rs`, `tests/typescript_semantics.rs`).
   - Table-driven tests & shared harnesses: use parameterized data tables instead of copy-pasting functions; extract reusable fixtures into shared helpers; do NOT artificially split domain test files into part1/part2.
3. **Public Seams**:
   - Drive tests through public interfaces (CLI, MCP tool router, reader, or linker pipeline); do not construct tests around unexported private internals.
4. **No Self-Justifying Synthetic Feature Tests**:
   - Never author artificial tests that assert an invented, uncontracted requirement (such as matching raw string literals inside unindexed function bodies) solely to justify introducing heavy, redundant, or regressive subsystems. Tests must validate admitted contract specifications from active milestones.
5. **Bounded Profiling and Honest Receipts**:
   - Record measured metrics honestly in milestone receipts without spinning in recursive profiling loops (cap profiling iterations to <= 3 per turn).
   - If an acceptance budget remains open due to physical or external bottlenecks, document the measured finding transparently in the receipt and hand off rather than stalling execution.

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

5. **Universal cold build throughput budget**:
   - Cold build throughput must maintain `>= 400 files/sec` (`<= 2.5s per 1,000 files` end-to-end, including AST extraction, cross-file linking, SQLite persistence, and Merkle root sealing).
   - AST extraction throughput must maintain `>= 800 files/sec` (`<= 1.25s per 1,000 files`).
   - Node insertion latency (`rows_ms`) must remain `<= 1.5ms per 1,000 nodes`.
   - Merkle sealing throughput must maintain `>= 100,000 entities/sec` (`<= 10ms per 1,000 entities`).

6. **Lean node metadata & projection law**:
   - `nodes.details` is an index-projection surface, NOT an AST fact dump or compiler analysis heap.
   - Prohibit serializing large interprocedural analysis trees, complete value-flow AST graphs, or raw scope maps into `nodes.details`.
   - Average node details payload size must remain `<= 120 bytes per node`.

7. **Storage density budget & compressed fact storage**:
   - Overall SQLite database storage density must not exceed `<= 45 KiB per indexed source file` (or `<= 3.0 KiB per indexed node`).
   - Intermediate file AST facts (`local_facts.facts_blob`) must use Zstandard compression with compression ratio `>= 3.5:1`, capping fact storage at `<= 15 KiB per source file`.

8. **Bulk SQLite ingestion pragmas**:
   - Cold database builds and batch rebuilds must execute under non-syncing bulk pragmas (`PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;`), executing an explicit WAL checkpoint only upon build finalization before Merkle seal.

9. **Interactive tool query latency budgets**:
   - Exact/prefix symbol lookup (`code_map_search` with `exact=true`): `<= 10ms`.
   - Full-text & BM25 hybrid search (`code_map_search`): `<= 30ms`.
   - Structural symbol inspection (`code_map_inspect`, `code_map_explain`): `<= 25ms`.
   - Graph impact & test dependency traversal (`code_map_impact`, `code_map_tests`): `<= 50ms`.
   - Scoped removal safety proof (`code_map_prove_removal`): `<= 30ms`.

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
