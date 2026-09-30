# ContextUnity Forge MCP — Agent Router

Planning and portfolio-state requests load global `planner`, then
[`.agents/skills/planner/ADDON.md`](.agents/skills/planner/ADDON.md).

## Routes

- Runtime behavior and setup: [README.md](README.md) and [`docs/`](docs/).
- Architecture and reference: [`docs/reference/`](docs/reference/).
- Forge code-graph workflow: [`contextunity-forge`](.agents/skills/contextunity-forge/SKILL.md).

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
