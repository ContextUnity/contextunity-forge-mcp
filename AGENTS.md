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
- Forge code-graph workflow: [`contextunity-forge`](~/.agents/skills/contextunity-forge/SKILL.md).
- Test suite rules and boundaries: [`tests/AGENTS.md`](tests/AGENTS.md).

## Task Execution & Repository Milestones

This repository directly owns its execution queue and task commitments:
- **Macro direction**: [docs/roadmap.md](docs/roadmap.md).
- **Execution queue & milestones**: [`docs/milestones/`](docs/milestones/) ordered by numeric prefix (`010-*.md`, `020-*.md`).
- **Research & proposal drafts**: [`docs/plans/`](docs/plans/README.md).
- **Milestone discovery**: Run `contextunity-forge-mcp milestone list` to inspect active and planned milestones and task completion ratios. Run `contextunity-forge-mcp milestone show <id-or-prefix> --full` to read the selected contract and task descriptions.
- **Milestone worktree mandate**: Every milestone must be developed in its own dedicated worktree. Always create and enter `.worktrees/<milestone-prefix>-<slug>` before editing files or claiming tasks. Keep the root checkout on `main` untouched.
- **Milestone format integrity**: Resolve any legacy or mismatched receipt formats directly in the milestone markdown document inside the worktree to match the current typed receipt contract. Preserve engine and parser source in `src/` unchanged when importing milestone specifications.
- **Milestone creation**: Run `contextunity-forge-mcp milestone init --plan <path>` to scaffold the next numbered contract. Use `--active` to start its active development clock at creation.
- **Milestone closure**: Run `contextunity-forge-mcp milestone handoff <id-or-prefix> --verification-command <command> --tests-passed <count> --tests-failed 0` only after every SQLite task in the milestone is `completed` and the final verification passes. The command records one structured milestone receipt and moves the milestone into `docs/milestones/archive/`.
- **Post-merge binary install**: After merging a completed milestone into `main`, install the updated release binary with `cargo install --path . --root ~/.local --force`.
- **Milestone-free fixes & fast-forward branches**: Minor, self-contained fixes or maintenance improvements that do not belong to an active milestone may be committed directly to `main` or developed in a dedicated worktree branch and merged via fast-forward (`git merge --ff-only`), strictly upon explicit user instruction. All changes must satisfy clippy and test suites.
- **Pre-existing code reconciliation**: When admitting or verifying tasks whose implementation already exists (e.g. historical migration or post-refactor reconciliation), do not author synthetic failing tests solely to force a red state. Validate the existing seam directly via `proof_policy: direct-proof` or targeted green test evidence in `contract/v1`.
- **Subtasks over task proliferation & Quality Gates**: Keep milestone tasks scoped to clear architectural domains. When new discoveries, edge cases, or sub-components arise during implementation, deepen the active task using subtasks (`contextunity-forge-mcp task subtask add <task_id> <subtask_ref> <title>` or MCP `task_manage` with `action: "subtask_add"`). Never inflate the milestone queue by spawning lightweight sub-tasks as full root ACDD tasks.
  - **Universal Subtask Definition of Done (DoD) & Implementation Invariants**: A subtask must resolve an explicit, bounded slice of the parent task's contract in full alignment with the milestone's overall design, architectural constraints (ADRs), and invariants. A subtask is NOT completed merely by passing an isolated micro-unit test in a vacuum:
    - **Concrete Syntax-Targeted Mandate (No Abstract Formulations)**: Subtasks must NEVER be formulated as vague, aspirational goals (e.g. "improve resolution", "handle return flow", "fix edge cases"). Every subtask must explicitly specify: (1) the concrete syntax, grammar construct, API contract, or data flow pattern targeted; (2) the exact expected resolution, classification, or state transition status; (3) the verifiable production-path metric or acceptance delta.
    - **No Partial / Incomplete Commits**: Never merge or commit partial implementations into milestone branches while known standard syntactic or contract constructs remain unhandled or fail closed as unknown. Commits must be held until the full concrete contract slice is demonstrably implemented and passes corpus-level verification.
    - **Contract Slice Resolution**: The subtask must demonstrably fulfill its targeted slice of the contract, satisfying its stated acceptance criteria and invariants without breaking existing boundaries.
    - **Milestone & Architectural Alignment**: Implementation must adhere to the milestone's design and established ADRs, building upon existing seams rather than introducing ad-hoc mechanisms or conflicting patterns.
    - **Production-Seam Evidence (Anti-Toy-Fixture Gate)**: Verification evidence must demonstrate real production-path fulfillment (e.g. public integration seams, observable state/behavior transitions, verifiable performance/contract criteria), not synthetic stubs that mock away actual system complexity. A subtask CANNOT be marked completed if the targeted universal construct still fails on unshadowed code in the reference corpus.
    - **Systemic Non-Regression**: The change must introduce zero warnings/lints, maintain behavioral equivalence for untouched cases, and preserve system invariants (e.g. Merkle determinism, fail-closed boundaries).
  - **Architectural Grounding (ADR Alignment)**: Before modifying linkers, AST extractors, or schemas, consult existing architecture in `docs/adr/` and `docs/architecture/` via `search_docs` / `get_doc`. Build upon existing seams (`value_flow.rs`, `semantic_context.rs`, `normalized_imports`) instead of inventing ad-hoc isolated patches.
  - **Anti-Looping Invariant**: If an architectural fix fails to produce green tests or causes performance/Merkle regressions after 2 iterations, stop looping. Post an `architectural_blocker` entry to `task_blackboard`, retain fail-closed behavior, and escalate rather than spinning in speculative rewrites.
- **Tool Protocol & Prioritization**: In any workspace where ContextUnity Forge is available, agents must strictly route through Forge tools:
  - Code & symbol discovery: use `code_map_*` tools.
  - AST structural patterns: use `ast_grep_search` / `ast-grep`.
  - Source reads & shell execution: use `lean-ctx` (`ctx_read`, `ctx_shell`).
  - Task and subtask management: use `task_*` MCP tools or `contextunity-forge-mcp task` CLI.
  - Ad-hoc scripts (raw `sqlite3` on `.forge/*.sqlite`, raw bash `grep` / `python` scrapers) are strictly discouraged when specialized Forge tools exist.
- **Task Blackboard Protocol & Lifecycle Rules**: The blackboard (`task_blackboard` MCP tool or `contextunity-forge-mcp task blackboard`) is the mandatory ephemeral coordination channel across agent turns and subtasks:
  - **When to read**:
    - *Task claim or turn entry*: Agent MUST read blackboard messages (`action: "read"`) upon claiming a task or starting a turn to restore active context, baseline numbers, and documented blockers.
    - *Before starting a subtask*: Review recent entries to ensure alignment with recorded architectural seams.
  - **When to post (Lifecycle Topics)**:
    - `hypothesis`: Post immediately after initial analysis and before writing code, capturing baseline metrics and identified root causes.
    - `architectural_seam`: Post before code modifications, documenting ADR alignment, planned pipeline changes, and targeted files.
    - `measured_delta`: Post upon verifying a subtask, recording the production-path delta (metric before/after, tests passed) BEFORE marking the subtask completed.
    - `blockers`: Post immediately if an approach fails after 2 iterations or hits fundamental parser/system limits, then halt for escalation.
- **Parallel task isolation**: Tasks in a milestone may proceed concurrently. When delegating heavy tasks to subagents, create dedicated child worktrees (`.worktrees/<milestone-prefix>-<task-slug>`) branched from the milestone. When working in a shared worktree, run targeted tests (`cargo test --test <name>`), stage and review only scoped files, and do not block a completed task on unfinished sibling tasks.
- **Proactive Scope & Adjacent Defect Resolution (No Bystander Inaction)**: When adjacent defects or missing helpers are uncovered: if the defect is in a module owned by another task in the milestone, reopen that task via `task reopen <task_id>` (extending into another task's scope fails with `TASK_SCOPE_CONFLICT`). If unowned, either extend scope via `task extend-scope <task_id> <path>` with reviewer approval on the `paths` contour, or record it in `## Deferred and out-of-scope defects` under the milestone tasks (via typed `deferred_defects: [...]` block) or on `task_blackboard` for subsequent milestones. Reviewers must accept legitimate defect fixes within the verified scope while rejecting uncontracted scope creep.

## Development Worktrees

Every milestone, isolated subagent task, or parallel spike operates within a dedicated worktree:
- Create worktrees under `.worktrees/<branch-name>`:
  `git worktree add .worktrees/<branch-name> -b <branch-name>`
- Switch into that directory for all development, task operations, and verification.
- `.worktrees/` is gitignored at repository root to keep untracked workspaces clean.
- On merge into `main`, verify the milestone state, prune the worktree with `git worktree remove .worktrees/<branch-name>`, and delete the merged feature branch.

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
