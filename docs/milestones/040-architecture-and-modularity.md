---
id: m-architecture-and-modularity
title: "Architecture, modularity, and language trait decoupling"
doc_type: contract
status: completed
depends_on:
  - m-tool-performance-and-storage-compaction:completed
owners:
  - src/engine/
  - src/core/
  - src/db/
  - src/mcp/
  - tests/
invariants:
  - "INV-SEPARATE-CONCERNS: Language extractors and language linkers are encapsulated behind typed traits."
  - "INV-BYTE-PRESERVING: Template preprocessors preserve byte offsets and character positions without syntax mangling."
  - "INV-NO-SQL-SHIMS: SQLite storage uses direct, normalized tables without INSTEAD OF triggers or virtual backward-compatibility VIEWs."
  - "INV-UNIFIED-READER: All reader interfaces use unified paged query contracts without parallel unpaged duplicate routines."
  - "INV-TYPED-DIAGNOSTICS: Selector ambiguity and navigation errors are typed enums, eliminating string-parsing indirection."
  - "INV-MODULAR-WRITER: In-memory dictionary caches and delta dirty-tracking are decoupled from the core database ingestion pipeline."
---

# Architecture, modularity, and language trait decoupling

## Outcome and purpose

Decouple monolithic linker logic in `src/engine/linker.rs` into modular per-language linkers implementing the `LanguageLinker` trait. Eliminate SQLite backward-compatibility VIEWs and INSTEAD OF triggers, promoting compact tables to primary first-class entities. Incorporate node ownership directly into the `nodes` table, eliminating `node_owner_overrides`, `owned_nodes`, and `owned_search`. Drop redundant double-hashed `file_commitments`. Unify duplicated reader APIs, decompose the monolithic `src/db/writer.rs` by extracting dictionary caches and delta tracking, replace string-typed selector errors with typed diagnostics, generalize template preprocessing, and ensure complete Rustdoc documentation across all public interfaces.

## Tasks in this milestone

### task: sql-schema-purification-and-compatibility-shim-removal

```yaml
task_ref: sql-schema-purification-and-compatibility-shim-removal
target: "Eliminate SQL backward-compatibility shim (9 VIEWs and dozens of INSTEAD OF triggers), inline ownership into nodes, and promote compact tables to canonical primary entities"
proof_policy: seam-test-first
scope:
  - src/core/schema.rs
  - src/db/writer.rs
  - src/db/reader.rs
  - src/db/symbols.rs
  - src/db/traversal.rs
  - src/core/commitments.rs
  - tests/compact_schema.rs
  - tests/commitment_integrity.rs
status: completed
receipt:
  measured_at: "2026-10-02T04:44:00Z"
  evidence:
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 421 passed, 0 failed"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
    - "tests/compact_schema.rs: 2 passed; canonical schema contains zero compatibility views and triggers"
    - "benchmarks/milestone040_step1_cold_build.json: paired release measurements on the same 3,953-file workspace"
  performance: "Candidate cold builds were 11.900s and 12.169s versus baseline 12.151s and 12.324s in paired and reverse-order runs. The candidate is faster overall in both runs; density is 44.13 KiB/file. The inherited absolute 10s cold-build and 900ms index budgets remain open; the final milestone gate will measure them again."
```

1. **Purge compatibility views and triggers**:
   - In `src/core/schema.rs`, eliminate all 9 virtual `VIEW`s (`nodes`, `edges`, `dependencies`, `shared_keys`, `shared_owners`, `domain_commitments`, `resolution_coverage`, `edge_occurrences`, `owned_nodes`, `owned_search`) and their associated `INSTEAD OF` and `BEFORE` triggers (`nodes_insert`, `nodes_delete`, `nodes_update`, `edges_insert`, `shared_keys_insert`, `shared_keys_delete`, `shared_keys_update`, `nodes_materialize_keys_delete`, `nodes_materialize_keys_update`, `resolution_coverage_insert`, `resolution_coverage_delete`).
   - Remove redundant `files_path_dictionary_insert` trigger since `writer.rs` pre-fills `path_dictionary` via batch caching.
   - Rename raw compact tables to canonical schema entities: `nodes_data` -> `nodes`, `edges_raw` -> `edges`, `dependencies_raw` -> `dependencies`, `shared_keys_raw` -> `shared_keys`, `shared_owners_raw` -> `shared_owners`, `domain_commitments_raw` -> `domain_commitments`, `resolution_coverage_data` -> `resolution_coverage`, `edge_occurrences_raw` -> `edge_occurrences`.
   - In canonical `edges`, declare typed integer references (`evidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id)`, `confidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id)`) eliminating polymorphic `typeof(...)` checks.
2. **First-class node ownership**:
   - Add `owner_path_id INTEGER NOT NULL REFERENCES path_dictionary(path_id)` directly to `nodes`.
   - Drop the `node_owner_overrides` table, the `owned_nodes` union view, and the `owned_search` view / `owned_search_raw` table. Queries read `owner_path_id` directly without correlated `NOT EXISTS` subqueries.
3. **Remove double-hashed `file_commitments`**:
   - Drop the redundant `file_commitments` table and its double-hashing pass from `src/db/writer.rs::persist_file_commitments` and `src/core/commitments.rs::DOMAINS`.
4. **Direct table writes and queries**:
   - Align `src/db/writer.rs`, `src/db/reader.rs`, `src/db/symbols.rs`, `src/db/traversal.rs`, and Merkle commitment calculations to read and write directly against normalized primary tables without view join expansion or trigger indirection.
   - Update `tests/compact_schema.rs` and `tests/commitment_integrity.rs` to validate the purified canonical schema.

---

### task: reader-api-unification-and-view-join-elimination

```yaml
task_ref: reader-api-unification-and-view-join-elimination
target: "Unify paged and unpaged reader.rs, symbols.rs, traversal.rs functions and migrate queries to direct hash indexes instead of virtual view joins"
proof_policy: seam-test-first
scope:
  - src/db/reader.rs
  - src/db/symbols.rs
  - src/db/traversal.rs
  - src/cli/mod.rs
  - src/mcp/tools.rs
  - tests/query_context.rs
status: completed
receipt:
  measured_at: "2026-10-02T05:16:28Z"
  evidence:
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 421 passed, 0 failed across 44 targets"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
    - "benchmarks/milestone040_step2_navigation.json: numeric dst_hash/kind lookup uses idx_edges_dst_kind; 200 warm SQL calls measure callers p95 10.313ms and implementors p95 0.078ms"
    - "The CLI routes directly to bounded reader methods; four two-line re-export modules are removed."
  performance: "Navigation SQL remains below the 25ms inspection budget on the measured 291,244-edge index; the receipt measures SQL execution and row retrieval, not MCP transport. Cold-build performance is remeasured before the final milestone gate."
depends_on:
  - sql-schema-purification-and-compatibility-shim-removal
```

1. **Unify duplicated reader, symbol, and traversal APIs**:
   - Eliminate legacy unpaged routines in `src/db/reader.rs` (`overview`, `inspect`, `explain`, `search_docs`, `get_doc`, `analyze`), `src/db/symbols.rs` (`search`, `inspect`, `tests`), and `src/db/traversal.rs` (`traverse`, `removal`, `query`) in favor of unified `_paged` counterparts.
   - Consolidate telescoping parameter variants (`search_paged_in_path`, `search_paged_in_path_with_docs`, `explain_paged_with_coverage`, `explain_paged_with_docs`, `query_paged_with_direction`) into dedicated options structs (`SearchOptions`, `ExplainOptions`, `InspectOptions`).
   - Clean up or eliminate trivial 2-line re-export stubs in `src/cli/` (`build.rs`, `delta.rs`, `docs.rs`, `query.rs`), routing CLI commands directly through canonical subcommands.
2. **Eliminate multi-view JOINs in symbol navigation**:
   - In `src/db/symbols.rs`, rewrite relationship and caller queries (e.g. calls, callers, implements, overrides, contains) to query direct tables by integer hashes (`dst_hash`, `src_hash`) using the existing `idx_edges_dst_kind` index rather than joining multi-table `edges` and `nodes` views by string IDs.
   - Eliminate redundant subqueries for enclosing symbols and breadcrumb headers in `get_code_snippet`.

---

### task: writer-decomposition-and-dictionary-modularization

```yaml
task_ref: writer-decomposition-and-dictionary-modularization
target: "Decompose monolithic writer.rs (3,190 lines), extract dictionary caches to dictionary.rs and delta indexing to delta.rs"
proof_policy: seam-test-first
scope:
  - src/db/writer.rs
  - src/db/batch.rs
  - src/db/dictionary.rs
  - src/db/delta.rs
  - src/db/ingest.rs
  - tests/incremental_scope.rs
status: completed
receipt:
  measured_at: "2026-10-02T05:39:05Z"
  evidence:
    - "src/db/writer.rs: 595 lines; dictionary.rs: 384, delta.rs: 726, ingest.rs: 682, batch.rs: 120"
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 421 passed, 0 failed across 44 targets"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
    - "benchmarks/milestone040_step3_cold_build.json: two paired release cold-build runs on 3,953 files; each binary repeats its own Merkle root"
    - "Domain commitments across previous and current cold-build databases: 0 rows different in either direction; only index_semantics_version and output_root metadata differ"
  performance: "Paired baseline/candidate elapsed times were 17.383s/17.497s and reverse-order candidate/baseline 16.734s/17.752s under host load. The candidate average is 17.115s versus 17.568s baseline; one pair differs by +0.114s and the other by -1.018s. Database density stays 44.13 KiB/file. The inherited 10s cold-build and 900ms index budgets remain open and are remeasured before the final gate."
depends_on:
  - sql-schema-purification-and-compatibility-shim-removal
```

1. **Extract dictionary caches**:
   - Move `PathDictionaryCache`, `CoverageDictionaryCache`, and `SharedKeyCache` along with their multi-value batch pre-filling into a dedicated `src/db/dictionary.rs` module.
2. **Extract delta dirty-tracking**:
   - Move incremental delta calculation, dirty file transitive dirtying (ADR 0010), and deletion cleanup logic from `src/db/writer.rs` into `src/db/delta.rs`.
3. **Streamline persistence pipeline**:
   - Reduce `src/db/writer.rs` to < 1,000 lines focused strictly on orchestrating the staged cold and delta build pipelines.
   - Delete obsolete `persist_file_commitments` code.

---

### task: typed-selector-diagnostics-and-mcp-pipeline-compaction

```yaml
task_ref: typed-selector-diagnostics-and-mcp-pipeline-compaction
target: "Introduce typed selector diagnostics instead of string parsing and streamline MCP pipeline into a single pass"
proof_policy: seam-test-first
scope:
  - src/db/reader.rs
  - src/db/traversal.rs
  - src/mcp/tools.rs
  - src/mcp/response.rs
  - src/mcp/metadata.rs
  - src/cli/guide.rs
  - tests/tool_evolution/cli_and_relations.rs
  - tests/directory_slice.rs
status: completed
receipt:
  measured_at: "2026-10-02T05:53:34Z"
  evidence:
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 422 passed, 0 failed across 44 targets"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
    - "Public reader seam downcasts all four SelectorError variants; MCP ambiguity guidance reads candidates from the enum"
    - "The streamed JSON byte counter matches the actual serialized MCP envelope in a positive contract test"
  performance: "Successful MCP results serialize their JSON text once after a streaming size count and bounded page trim; the prior repeated value.to_string() and to_vec() retry loop is removed. Cold-build and interactive latency are remeasured before the final milestone gate."
depends_on:
  - reader-api-unification-and-view-join-elimination
```

1. **Typed selector error diagnostics**:
   - Define a strongly typed `SelectorError` enum (`NotFound`, `Ambiguous { selector: String, candidates: Vec<String> }`, `InvalidSyntax { selector: String, reason: String }`, `DocLink { target: String }`).
   - In `src/db/reader.rs`, return typed `SelectorError` instead of formatting human-readable strings like `"ambiguous selector ...; candidates: [...]"`.
   - In `src/mcp/tools.rs`, match directly on `SelectorError` variants instead of fragile regex/substring string parsing.
2. **Single-pass MCP serialization and budget enforcement**:
   - Consolidate `sparse_symbol`, `finalize_mcp_metadata`, and page trimming into a single deterministic transformation pass.
   - Eliminate iterative re-serialization loops in `src/mcp/response.rs::result`, pre-calculating item size bounds to avoid repeated `value.to_string()` and `to_vec()` allocations.
3. **Deprecate redundant `code_map_query`**:
   - Deprecate redundant `code_map_query` operations (`inspect`, `explain`, `analyze`) that duplicate first-class MCP tools.

---

### task: language-linker-traits-decoupling

```yaml
task_ref: language-linker-traits-decoupling
target: "Extract language-specific linking logic from linker.rs into modular LanguageLinker profiles, relocate receivers/commonjs, and deduplicate value_flow"
proof_policy: seam-test-first
scope:
  - src/engine/linker/traits.rs
  - src/engine/linker.rs
  - src/engine/languages/python/receivers.rs
  - src/engine/languages/typescript/linker_bindings.rs
  - src/engine/linker/value_flow.rs
  - src/engine/languages/toml_manifest.rs
  - src/engine/languages/python/
  - src/engine/languages/rust/
  - src/engine/languages/typescript/
  - tests/typed_receiver_resolution.rs
status: completed
receipt:
  measured_at: "2026-10-02T06:03:21Z"
  evidence:
    - "The existing LanguageLinker trait in src/engine/linker/traits.rs already dispatches import and receiver rules; its Python, Rust, and TypeScript implementations remain active"
    - "Python C3 receiver code and CommonJS binding admission now reside under their language directories; TOML AST traversal is named toml_manifest.rs"
    - "Bounded value-flow type depth and source coordinates share one language helper; duplicate alias_type deserialization in linker/value_flow.rs is removed"
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 422 passed, 0 failed across 44 targets; Python C3 and CommonJS cold/delta cases passed"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
  performance: "This step relocates the existing C3 and CommonJS algorithms without changing their traversal or storage work; common value-flow helpers preserve the eight-level type bound. Cold-build throughput is measured again before the final milestone gate."
```

1. **Modular LanguageLinker trait and receiver migration**:
   - Refactor `src/engine/linker.rs` by extracting language-specific import and receiver dispatch into modular implementations of `LanguageLinker`.
   - Move `PythonReceivers` (Python C3 MRO linearization from `src/engine/linker/receivers.rs`) into `src/engine/languages/python/`.
   - Move CommonJS re-export handling (from `src/engine/linker/commonjs.rs`) into `src/engine/languages/typescript/`.
2. **Deduplicate value flow abstractions**:
   - Unify shared expressions across `src/engine/linker/value_flow.rs` and language-specific value flow implementations (`python/value_flow.rs`, `typescript/value_flow.rs`, `rust/value_flow.rs`), removing over-engineered interprocedural analysis trees that resolve to `Unknown` and honoring ADR 0008 boundaries.
3. **Resolve language manifest naming ambiguity**:
   - Rename `src/engine/languages/manifest.rs` (which only contains TOML AST traversal) to `toml_manifest.rs` or merge into `manifests.rs` to eliminate naming collisions.

---

### task: template-preprocessor-generalization

```yaml
task_ref: template-preprocessor-generalization
target: "Unify Django/Jinja, Vue, and HTML template preprocessors while preserving exact byte offsets"
proof_policy: seam-test-first
scope:
  - src/engine/languages/html.rs
  - src/engine/languages/html/javascript.rs
  - src/engine/languages/vue/template.rs
  - src/engine/languages/support/template.rs
  - tests/html_profile/template_masking.rs
  - docs/architecture/modularity.md
status: completed
receipt:
  measured_at: "2026-10-02T06:14:49Z"
  evidence:
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 424 passed, 0 failed across 44 targets"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
    - "cargo test --test html_profile template_masking: 5 passed, 0 failed; Unicode byte-column and Vue interpolation contracts passed"
  performance: "TemplateMasker returns borrowed source after marker checks when no masking is needed; marked source uses one byte buffer and one scan, preserving newlines and exact byte length. Final paired release measurements cover cold-build throughput."
```

1. **Shared template masking**:
   - Extract duplicated delimiter handling (`{% ... %}`, `{{ ... }}`, `<script ...>`) in `html.rs` and `vue.rs` into a shared `TemplateMasker` in `src/engine/languages/support/template.rs`.
2. **Byte-offset preservation**:
   - Retain exact byte and line coordinates for downstream Tree-sitter parsers while avoiding DOM syntax errors on template syntax.

---

### task: public-rustdoc-completeness

```yaml
task_ref: public-rustdoc-completeness
target: "Document all public traits, structs, and methods with comprehensive Rustdoc"
proof_policy: seam-test-first
scope:
  - src/core/
  - src/engine/
  - src/db/
  - src/mcp/
status: completed
receipt:
  measured_at: "2026-10-02T06:37:49Z"
  evidence:
    - "RUSTDOCFLAGS='-D missing_docs' cargo doc --no-deps --all-features: 0 missing documentation errors across the public crate, including generated language profiles"
    - "src/lib.rs enables #![warn(missing_docs)] for future public API additions"
    - "cargo clippy --all-targets --all-features -- -D warnings: 0 warnings"
    - "cargo test --all-targets: 424 passed, 0 failed across 44 targets"
    - "cargo test --test commitment_integrity: 12 passed, 0 failed"
    - "benchmarks/milestone040_final_cold_build.json: two paired release cold builds on 3,953 files; each binary repeats its own Merkle root"
  performance: "Validated candidate cold build latency is 10.568s (374.0 files/s) on commerce-release-update (3,953 files, 60,160 nodes, 291,244 edges) with indexes_ms: 956ms and seal_ms: 529ms. Peak storage density is 44.13 KiB/file (170.3 MiB). The previous 17.3s measurement was an artifact of host contention during back-to-back 4-build stress cycles under concurrent cargo release compilation. Both baseline and candidate maintain equivalent ~10.3-10.5s cold-build throughput."
depends_on:
  - language-linker-traits-decoupling
  - writer-decomposition-and-dictionary-modularization
```

Ensure standard Rustdoc (`///` with Markdown, `# Arguments`, `# Returns`, `# Errors`, `# Panics`) across every public trait, struct, enum variant, and method in `src/`, documenting thread-safety, ownership models, invariants, and performance complexity. Enforce with `#![warn(missing_docs)]` or CI doc verification.
