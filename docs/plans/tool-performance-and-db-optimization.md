---
title: "Forge MCP: Tool Performance Hardening and SQLite Database Optimization"
repository: forge-mcp
area: forge
priority: high
status: active
kind: plan
source_path: docs/plans/tool-performance-and-db-optimization.md
created_at: 2026-09-29
author: Antigravity
evidence_base: /tmp/forge-codebase-audit/REPORT.md
doc_type: guide
related_milestones:
  - docs/milestones/010-repository-task-lifecycle.md
---

# Forge MCP: Tool Performance Hardening and SQLite Database Optimization

## 1. Executive Summary & Audit Baseline

On September 29, 2026, an exhaustive comparative audit between **Forge MCP** (generation `dad69e61...`, pure Rust) and **Codebase Memory (CBM v0.10.8)** was executed across 5 representative codebase scenarios (`Django bootstrap`, `PIM catalogue queryset`, `Horoshop provider publish`, `ChannelSyncEntry ledger`, `GridViewSpec Django host`).

### Key Audit Findings
1. **Functional Accuracy vs Noise:**
   - Forge provides superior symbol precision (1–25 candidates vs CBM's 200–900 candidates), accurate resolution evidence (`resolved`, `unresolved`, `ambiguous`, `external`), and structural AST/documentation search.
   - CBM provides better natural language BM25 ranking and test discovery, though 22.1% of its `CALLS` edges are heuristic (`confidence < 0.5`) and some test associations lack graph connectivity (degree = 0).
2. **Performance Gaps:**
   - Forge search median: **68.3 ms** vs CBM **40.1 ms**.
   - `code_map_tests` tail latency: median 206.8 ms, **p95 1,657.6 ms**.
   - `ast_grep_search` tail latency: median 503.8 ms, **p95 1,960.9 ms**.
   - Every Forge MCP tool invocation pays a **28–59 ms** baseline overhead due to full filesystem `source_inventory` re-validation (3,897 paths).
3. **Critical Functional Defects:**
   - **`code_map_prove_removal` is globally blocked:** Checks global workspace counts of `unresolved` (103,083) and parser `errors` (68). As a result, safe local symbol removal is impossible anywhere in the codebase due to unrelated HTML/JS parse errors or global unlinked variables.
   - **`code_map_tests` recall blind spot:** S4 (`ChannelSyncEntry`) returned 0 tests because the test created the entry through dynamic/helper paths without a direct static graph edge.
4. **Storage Overhead:**
   - Forge database size is **838.94 MiB** (1 combined database) vs CBM's **153.94 MiB** (3 project databases) — a **5.45x** disparity.
   - 80% of storage is consumed by 6 uncompressed or redundant tables (`local_facts`, `edge_occurrences`, `edges`, `reverse_dependencies`, `shared_owners`, `resolution_coverage`).

---

## 2. Target Architecture & Objectives

1. **Sub-30ms Tool Response Time:** Eliminate the MCP admission inventory scan overhead and bound graph traversal depth.
2. **Zero-Overhead AST Search (<30ms):** Filter candidate files using the indexed database and FTS token index instead of scanning disk files.
3. **Context-Sensitive Removal Safety:** Localize `prove_removal` to target candidate incoming dependencies and consuming scopes.
4. **Storage Compaction (<200 MiB):** Compress raw JSON facts with Zstandard, eliminate duplicate tables (`reverse_dependencies`), and optimize indexes with `WITHOUT ROWID` or integer foreign keys.
5. **Unresolved Pruning (103k -> <25k):** Differentiate known third-party/stdlib libraries as `external` and resolve package re-export aliases.

---

## 3. Work Breakdown & Execution Plan

### Phase 1: Removal Proof & Logic Hardening (`prove_removal`)
- [ ] **Target-Scoped Safety Evaluation:**
  - In `src/db/traversal.rs` (`prove_removal`), replace global `SELECT count(*) FROM resolution_coverage` and `SELECT count(*) FROM errors` with target-scoped evaluation.
  - Check `unresolved` only where `expression == target_name` or within files that statically depend on the target symbol's module.
  - Isolate workspace health: return `target_safe_to_remove: bool` independently of `workspace_unresolved_count: usize` and `workspace_errors_count: usize`.
- [ ] **Verification:** Re-evaluate S1..S5 removal targets; isolated and unreferenced symbols must report `safe_to_remove: true`.

### Phase 2: Tool Speed, Ranking & Read Liveness
- [ ] **Hybrid Search with SQLite FTS5 BM25 (`code_map_search`):**
  - Update `node_search` virtual table schema in `src/core/schema.rs`: change `detail='none'` to `detail='column'` (or standard FTS5) to enable SQLite's native C-level `bm25(node_search)` scoring. (Storage impact is negligible: ~2–3 MiB).
  - Implement two-tier hybrid ranking in `src/db/symbols.rs`:
    1. Highest priority (+1000 score): exact symbol match (`name == pattern`) and exact prefix matches (`name LIKE pattern || '%'`).
    2. High priority (+500 score): qualified name match (`qualname`).
    3. Natural language relevance: native SQLite FTS5 `bm25(node_search)` scoring, replacing rigid multi-term `AND` constraints with ranked multi-term matching.
    4. Graph boost: boost score (+50) for nodes with high degree / public exports.
  - Exclude documentation sections from code symbol search by default (`include_docs: false`) to avoid mixing thousands of doc matches into code queries (as seen in S5).
  - Formalize `group_by_file: bool` in MCP response schemas to minimize LLM token usage.
- [ ] **MCP Inventory Scan Debouncing:**
  - In `src/mcp/server.rs` (`admit()`), introduce a short liveness TTL (e.g. 2–3s) or top-level directory mtime watch before re-executing `scan_reusing` and reading 3,897 `source_inventory` rows.
  - Eliminate 28–59 ms floor latency from read-only MCP tool calls.

### Phase 3: Test Discovery & AST Grep Acceleration
- [ ] **Bounded Traversal & Lexical Fallback (`code_map_tests`):**
  - In `src/db/symbols.rs` (`tests`), add recursion depth control (`max_depth = 4`) to `WITH RECURSIVE seeds ... walk ...` to prevent graph traversal runaway (reducing p95 from 1,657ms to <50ms).
  - Add lexical fallback: if graph-connected test edges return 0 (e.g. S4 `ChannelSyncEntry`), run a bounded FTS query on files with `is_test = 1` for `pattern` or `test_<symbol_name>`.
- [ ] **Index-Assisted AST Search (`ast_grep_search`):**
  - In `src/cli/ast.rs`, remove disk-wide filesystem walk (`scanner::scan_with_adapter`).
  - Read candidate file paths matching `language` directly from SQLite `files` table.
  - Parse AST query pattern for literal identifiers; if present, pre-filter candidate files via `node_search` (FTS5) before invoking Tree-sitter.
  - Target: Drop AST search p95 from 1,960ms to <30ms.

### Phase 4: SQLite Database Compaction (839 MiB -> <200 MiB)
- [ ] **Zstandard Compression for `local_facts`:**
  - In `src/core/schema.rs` and `src/db/writer.rs`, replace `facts_json TEXT` with `facts_blob BLOB` compressed using `zstd`.
  - Decompress in-memory on demand during delta hydration (`GB/s` throughput in Rust).
  - Estimated savings: **~95 MiB** (114.7 MiB -> ~18 MiB).
- [ ] **Eliminate Redundant `reverse_dependencies` Table:**
  - Drop table `reverse_dependencies` (24.1 MiB table + 20.2 MiB indexes = 44.3 MiB).
  - Add index `idx_dependencies_target ON dependencies(target, kind)`.
  - Route reverse dependency lookups to `dependencies WHERE target = ?`.
  - Estimated savings: **~44.3 MiB** with zero data loss.
- [ ] **Schema Compaction & `WITHOUT ROWID`:**
  - Benchmark `WITHOUT ROWID` on composite primary key tables: `edges (src_public_id, dst_public_id, kind)`, `edge_occurrences (owner, ordinal)`, `shared_owners (kind, key, owner, ordinal)`.
  - Normalize repeated file path strings across `edge_occurrences` and `resolution_coverage` using integer `file_id`.
  - Estimated savings: **~120–160 MiB**.
- [ ] **Dictionary Normalization for `resolution_coverage`:**
  - Deduplicate repeated static evidence strings (top 3 reasons account for 76,932 rows).
  - Estimated savings: **~25–35 MiB**.

### Phase 5: Python Unresolved Reference Reduction (103k -> <25k)
- [ ] **External Library Triage:**
  - Collect declared dependencies from workspace package manifests through language profiles. Attribute nonlocal absolute imports and their calls as `external`, retaining verified import provenance and separate standard-library recognition.
- [ ] **Package Re-export Chains:**
  - Extend re-export resolution for internal package hubs: `contextunity.core.types`, `contextunity.commerce.catalogue.models`, and `contextunity.core.cell_edges`.

### Phase 6: Verification & Benchmark Gates
- [ ] Run benchmark validation against `/tmp/forge-codebase-audit/` test suites.
- [ ] Verify `PRAGMA quick_check` and `PRAGMA foreign_key_check` on compacted database.
- [ ] Validate cold start indexing time and delta update time on 1 and 10 files against performance budgets.
- [ ] Ensure all existing MCP tools maintain strict backwards compatibility.


### Phase 7: Cold and Incremental Build Allocation Repair

**Authority and owner:** The October 1, 2026 user amendment adds allocation repair to this active Forge MCP plan. Product changes, tests, and performance evidence belong to `projects/contextunity-forge-mcp`. This phase preserves public Rust model and linker interfaces, MCP/CLI JSON, SQLite semantics, and deterministic commitment encoding.

| Work item | Production boundary | Required proof |
| --- | --- | --- |
| Typed value-flow storage | Language profiles and AST extraction → private typed facts envelope → linker, incremental identity, writer and commitments | Public extraction adapters retain the existing details JSON shape. Internal producers and consumers share typed facts. Legacy malformed facts remain fail-closed; property order, island rebasing, scope limits and sparse encoding remain equivalent. |
| Compact graph tags | Internal linker graph → writer; public Graph adapters retain String fields | Known kind/confidence/status values use allocation-free tags; arbitrary future values remain representable. Public graph JSON, lexical ordering and durable hashes match the existing representation. |
| Dependency-driven inference | ValueFlowIndex return, binding and field reads → ordered dirty epochs | First pass, same-pass propagation order, stopping condition and bounded iteration count remain unchanged. Missing-field reads and newly discovered dependencies trigger reevaluation. Public resolver signatures retain their original bounds. |
| Shared writer keys and reusable search buffers | SharedKeyCache and both persist_files paths | Both maps share key ownership, collision checks remain equivalent, and batch buffers are recycled after successful flush. Search text and cold/delta commitments retain identical bytes. |

- [x] Connect typed facts through Python, Rust, TS/JS, HTML and Vue producers, legacy hydration, alias resolution, contract projection and incremental reference keys.
- [x] Use compact tags on the internal build path while preserving public model defaults and linker adapters.
- [x] Introduce dependency-tracked dirty epochs and verify chained factory, binding and field propagation through existing public semantic suites.
- [x] Verify shared key ownership and buffer reuse through public database roundtrip and commitment integrity suites.
- [x] Run `cargo test --all-targets`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --test commitment_integrity` after integration.
- [ ] Build release and compare immutable binaries on the same frozen Commerce source inventory without concurrent compilation. Record elapsed, link, persist_graph, persist_files, seal, peak memory and warm MCP latency.
- [ ] Run the required live Commerce cold build and publish status counts. Acceptance requires elapsed <=12s, persist_graph <1.2s, no paired performance regression and unresolved <40,000.

**Current evidence:** The frozen 3,920-file inventory has digest `5c5e6400895a6fb813ff442dc96a65f940eb4da5b49bdc26e2cddff247ad1116`. Baseline unresolved is 68,161; the current expanded semantic graph has 63,980, including newly indexed HTML references. The original user figure 85,172 belongs to a different baseline and is not a paired comparison. The latest pre-amendment candidate cold build is 14.810s, with link 1.850s, persist_files 2.099s, persist_graph 1.159s and seal 2.930s. These results leave performance and coverage acceptance open. Existing pre-amendment verification records 365 passing tests, three pre-existing ignored tests, strict all-feature Clippy and ten passing commitment tests; new changes require fresh evidence.

**Evidence locations:** `/tmp/forge-benchmark-snapshot-valueflow-parallel-b1.json`, `/tmp/forge-benchmark-snapshot-valueflow-parallel-b1.db`, `/tmp/forge-valueflow-perf-all-targets.log`. Temporary benchmark evidence is retained outside the product repository. No new artifact directory or benchmark test binary belongs to this phase.

**Measured repair findings:** The first allocation candidate passes 366 tests, strict all-feature Clippy and ten commitment tests. On the frozen inventory, its complete coverage, edge and occurrence row digests match the preceding expanded semantic graph. Paired ABBA elapsed means are 16.637s before and 16.890s after; peak memory decreases approximately 11%, while CPU time decreases 0.9%. Elapsed <=12s remains an open gate. Interleaved warm exact-search medians are 3.308159ms and 3.308295ms; the preceding sequential difference does not reproduce under interleaving. Sub-millisecond RPC latency remains unmet.

- [x] Replace canonical coverage sealing's dictionary JOIN/sort path with preloaded dictionaries and borrowed rows; preserve exact generic fallback for noncanonical storage, arbitrary statuses and orphan behavior. Partial-owner loads use selected dictionaries so incremental updates retain bounded work.
- [x] Restore classic HTML's legacy empty-section JSON slots and property order through streaming typed serialization. Canonical classic null sections decode consistently across cold, public and cached paths; all other scopes retain strict decoding. Prove cold/public/delta equivalence and compare all durable node/facts bytes against the paired binary.
- [ ] Repeat complete verification and cold/delta/interleaved warm measurements after both repairs. Keep failed or unmet performance gates open.

**Verified Rust baseline:** A fresh build of this Forge checkout with release SHA `328fdfafe0210b7eb1ed93bf935549cc98c292f8e92be9d87f754c9d4e0eaf78` indexes 136 files in 1.063s and reports Rust unresolved 15,650, resolved 8,664, external 3,677 and ambiguous 73. Inventory digest is `6e39d859043b1def86276d92d370cfaf261ccb5720b4dfbde0d91e6b7f07d5e0`, with zero source readback mismatches. The earlier serving-index count 17,387 is not this current-binary baseline. Subsequent source edits require a refreshed measurement. Evidence: `/tmp/forge-valueflow-final-status.json` and `/tmp/bench-forge-rust-current.db`.




**Verified allocation repair (`1307d247`):** All-targets passes 369 tests (three existing ignored), strict all-feature Clippy passes, commitment integrity passes 12 tests, and release build passes. The immutable release SHA is `1307d24709ce5d1c1a398794620775f8a54c89d4da376f3f645e04b384b7644d`. Paired frozen ABBA cold means improve from 12.192s to 11.535s (5.39%); both candidate runs are <=12s. Mean persist_graph improves from 1.122s to 1.032s, seal from 2.374s to 2.162s, and peak memory from approximately 1,217MiB to 1,079MiB. All durable node details, compressed local facts, search text and commitment digest bytes match the preceding expanded semantic binary, including classic HTML empty slots.

**Open acceptance:** Required live Commerce cold build is 12.201s, persist_graph 1.100s, seal 2.224s, on 3,940 files with zero source readback mismatches. Unresolved remains 64,910; allocation repair intentionally preserves graph semantics. Interleaved warm exact/inspect/impact medians are 2.385/2.710/2.641ms; sub-millisecond RPC latency remains unmet. Initial one-file delta improves 2.256s to 2.162s, but the bounded repeated comparison has means 2.260s to 2.468s under increasing host load; incremental no-regression is not established. Repeated seal means are 354.15/354.54ms, so the preceding single-run seal difference does not establish a regression. Delta coverage, edges, occurrences and inventory match paired old/new and cold semantic rows. Do not claim all performance gates closed.

**Refreshed Rust baseline:** This release indexes 138 files with Rust resolved 8,743, external 3,728, ambiguous 81 and unresolved 15,818. The preceding 15,650 count used 136 files; these inventories are not a regression comparison. Phase 8 structural callee extraction is admitted as the next bounded implementation slice; its coverage gain and performance remain unmeasured. Evidence and binary/inventory provenance: `/tmp/forge-valueflow-final-performance-evidence.json`, `/tmp/forge-valueflow-seal-pair-wire-digests.json`, `/tmp/forge-valueflow-delta-repeat-result.json`.

**Remaining cold persistence owner-cluster:** The user authorizes continued latency and allocation repair. Source audit identifies fresh JSON Strings for 60,869 nodes, a pending-batch String duplicate of each Arc-backed shared key, and 10,520 single-row source-inventory/local-facts/document inserts in the current Commerce database. Canonical owner is src/db/writer.rs; origin is pre-existing incomplete batching/allocation repair. Cold build passes through populate/persist_files; delta shares the cache/batch helpers but retains its current incremental statements. Public models, schema, encoding, row ordering and graph semantics remain fixed.

- [x] Share one Arc-backed key among pending SQL values and both key-cache maps; retain collision detection, successful-push ordering and final key/owner flush ordering.
- [x] Recycle bounded cold node-details byte buffers through unchanged streaming DetailsView serialization. Bind successful serializer output directly as SQL TEXT through private OwnedJsonText; reclaim only after successful insertion, without unsafe conversion or per-node UTF-8 rescanning.
- [x] Batch remaining cold source_inventory/local_facts/doc_sections rows with SQLite parameter limits, borrowed preencoded blobs, owned fallback blobs, exact storage types and final remainder flushes before bulk FTS.
- [ ] Prove full/remainder batches through existing public domain suites, exact durable JSON/blob/document/shared-key/domain commitment bytes, existing cold/delta parity and matched-inventory timing/RSS. No new root test binary or public API solely to expose private cold fallback.

**Current cold persistence proof (17ea20dc):** All-targets passes 378 tests (three existing ignored), strict all-feature Clippy is clean, and commitment integrity passes 12/12. The public full/remainder contract exercises 252 files and 251 document sections, exact JSON/blob/TEXT/REAL storage and shared keys. Omitting the final document batch flush deliberately fails the same contract at 250 versus 251 sections; restoring the source passes. All 29 durable tables match the preceding binary on frozen Commerce and Forge inventories. An earlier version also proves exact isolated one-file delta parity; the final serializer repair is cold-only. Evidence: /tmp/forge-persistence-jsontext-*.log and /tmp/forge-jsontext-performance-evidence.json.

**Current performance acceptance remains open:** Same-input Commerce ABBA file persistence means improve 2138 to 1826ms; graph persistence 1337 to 1320ms, sealing 2766 to 2884ms, elapsed 15.407 to 14.674s. A bounded reverse pair confirms file-stage improvement, but host effects and mixed stages do not establish whole-build non-regression. Latest live cold is 14.259s (3960 files, inventory 3966b1700abff8195ff9c0b5dc758e37dad1211e0a284d22e35cb00089a13c20), graph 1366ms, files 1798ms and seal 2454ms. Statuses are resolved 166220, external 54347, ambiguous 53, unresolved 64910. The predecessor's 11.648s receipt belongs to a3729643 and is not this release's acceptance. Absolute <=12s, graph <1.2s, unresolved <40000, hot RPC sub-millisecond and stable whole/delta non-regression remain open.

**Rust fast paths (0f6a1b86):** Simple pattern identifiers avoid cursor setup and lifetime-shape checking follows annotation classification. Same-input facts, graph and domain hashes remain byte-identical; all-targets 377, strict Clippy and commitment 12/12 pass. Bounded Rust extraction improves 253 to 155ms and elapsed 853 to 676ms, while linking varies 66.9 to 74.4ms. Overall stage non-regression remains unestablished. Evidence: /tmp/forge-valueflow-fastpath-performance-evidence.json.

**Bounded parallelism measurement:** Read-only host diagnosis finds 16 available CPUs, no detected cgroup CPU quota, and no RAYON_NUM_THREADS/RAYON_RS_NUM_CPUS cap. During the busy JS comparison Forge averages 2.7-2.9 cores while aggregate host busy is 60-75%; present verification samples cannot identify the historical contention source or explain every regression. Admit a bounded same-binary, immutable-input 8-versus-16 Rayon thread experiment after Cargo finishes. Record host busy, CPU pressure, extraction/link/persistence/sealing and elapsed; change no production default unless repeated evidence supports the cap without semantic or incremental regression. Evidence: /tmp/forge-host-diagnosis.json.

**Rust standard contract prerequisite:** Current extraction does not retain no_std/no_implicit_prelude or exact Cargo target context. Admit finite associated standard operations only after provider/local shadow checks and conservative prelude availability. String::with_capacity is valid; Option::with_capacity is an invalid cross-pair example. Qualified dependency/prelude context stays separately owned and cannot be guessed from the Cargo dependency list.

### Phase 8: Rust Resolution Coverage in the Forge Repository

**Scope:** Rust extraction and linking in `projects/contextunity-forge-mcp`. The validated release `447e922e` indexes 138 files (128 Rust) and reports 15,827 unresolved Rust records: computed/dynamic 5,823, no lexical candidate 5,161, shadowed local/parameter 4,648 and unverified inferred member 147. These records include types, imports and fields as well as calls. The serving Forge MCP currently reports 17,797 under different indexer semantics; source-inventory matching does not establish equal engine semantics. Use the validated release database for acceptance, not the superseded serving-index counts. Evidence: `/tmp/forge-rust-unresolved-priorities.json`.

| Order | Fix and owner | Contract and positive proof | Performance guard |
| --- | --- | --- | --- |
| 1 | Reproducible current-binary baseline; benchmark and index metadata seams | Freeze this repository's source inventory and classify reasons using the validated release binary. Distinguish missing provider, unsupported syntax, missing receiver, external origin, ambiguity and dynamic dispatch. | Group bounded coverage queries; avoid complete node JSON hydration or concurrent benchmarking and compilation. |
| 2 | Structural Rust callee extraction; `languages/rust.rs`, `rust/value_flow.rs`, AST receiver hints | Admit `generic_function` bases and receiver/member AST fields independently of source formatting. Equivalent multiline and turbofish fixtures resolve to the same actual local target or verified external origin. | Borrow AST text; preserve generic arguments once; no regex or expression reparsing on the linker path. |
| 3 | Local provider and re-export admission; Rust imports, member index and semantic context | Follow `use super::*`, `pub use`, crate aliases and type aliases through indexed providers, retaining visibility, workspace boundaries, cycles and ambiguity. Positive nested-module proofs identify actual declarations. | Precompute shared provider/export maps once; use bounded in-memory candidate sets. |
| 4 | Nominal receiver propagation; Rust typed extractor and shared reducer | Record method-call initializer/return evidence and tuple-field ordinal types. Proven local call/field chains retain source order and mutation barriers. Public fixtures cover tuple wrappers and nominal methods. | Reuse typed flow storage and member indexes; avoid JSON roundtrips and cloned per-node facts. |
| 5 | Explicit container projection; semantic schema, Rust extractor and resolver | Preserve applied arguments and project through proven `Result<T,E>?`, `unwrap` or `Option<T>` operations. Direct containers retain container identity; async results retain their future boundary. | Bound type arguments/depth; no global monomorphization or unverified method-name inference. |
| 6 | Local closures and constrained generic implementations; Rust symbols, scopes and members | Link exact closure bindings to their callable scopes; admit generic members only when canonical receiver and visible bounds establish a unique declaration. Unconstrained callbacks and trait-object dispatch retain uncertainty. | Bound candidate sets and capture facts; no per-call workspace implementor scans. |
| 7 | Macro and external boundaries; Rust macro extraction and coverage evidence | Link local macro identities and canonical standard-library associated operations. Expansion-dependent procedural macros retain explicit uncertainty. | Keep cargo/rustc macro expansion outside ordinary indexing; use manifest provenance rather than third-party API lists. |

- [x] Establish the current release baseline before implementation admission.
- [ ] Implement structural callee and provider admission fixes with existing public Rust/receiver suites.
- [ ] Implement nominal fields and explicit projections with mutation, scope and boundary proofs.
- [ ] Evaluate closures, constrained generics and macro identity after measured earlier phases.
- [ ] Run full tests, strict all-feature Clippy, commitment integrity and paired cold/delta benchmarks after each admitted slice.

**Observed examples:** The inspected index contains turbofish `row.get::<_, String>` and multiline receiver expressions; 160 `self.0` occurrences require tuple-field classification. Indexed `_` and lifetime records contradict current extractor exclusions, so they are evidence of a measurement mismatch until reproduced with the current binary. No coverage percentage or speed gain is established by this investigation.


**Implemented Rust structural slice (`447e922e`):** Rust call references and initializer call facts share bounded AST callee extraction for ordinary paths, field chains, turbofish and multiline syntax. Dense names are borrowed; normalization uses one buffer with eight-segment/512-byte bounds. Computed receivers, qualified trait dispatch, generic receiver paths and macros retain their existing boundary. Initializer method calls use existing nominal receiver/return admission. The semantic context excludes synthetic Rust `impl` nodes from lexical name providers, preserving their member scopes; this repairs imported type shadowing during factory-return inference. An import-edge test follows the existing module-target contract rather than inventing an import-to-struct edge.

**Current proof:** Focused Rust value-flow tests pass 13/13, all-targets passes 374 tests with three existing ignored, strict all-feature Clippy passes, commitment integrity passes 12 tests, and release passes. SHA is `447e922eec73cba2497ec0f6bf4421cc2177c72dd82ebb5f8f1c7f0aa329a172`; logs are `/tmp/forge-rust-slice-{focused-final,all-targets,clippy,commitment,release}.log`. Public fixtures cover actual cross-file targets and positions, external import provenance, nominal return propagation, opaque dispatch and exact resource bounds. Test modules remain <=800 lines.

**Paired Rust coverage:** Both immutable binaries index the same current 138-file inventory `628015c0078c029ec42c66c7bf4f307180ea87e604ff5f80db29a4a2fdb61fe0`. Previous `1307d247` reports unresolved 15,913, resolved 8,788, external 3,746 and ambiguous 81; new `447e922e` reports unresolved 15,827, resolved 8,834, external 3,793 and ambiguous 81. Unresolved decreases by 86. The dynamic bucket decreases 6,740 to 5,823, but shadowed/no-lexical buckets increase: syntax reclassification is not equivalent to successful resolution. Seven newly admitted local edges have source-backed targets, including generic batching calls. Evidence: `/tmp/forge-valueflow-rust-slice-status.json`. This comparison supersedes unrelated source-inventory totals for measuring this slice. Commerce cold and live performance measurements remain to be attached; the full plan stays open.


**Rust slice performance follow-up:** Same-inventory Rust ABBA elapsed means are 1.192s before and 1.093s after; CPU 3.725s to 3.455s, extraction 349ms to 318ms, graph persistence 117ms to 107ms, sealing 179ms to 165ms. Linking is effectively unchanged at 121.53ms to 122.09ms. The earlier single-run +32ms elapsed difference does not repeat. Host load varies, especially in the last baseline run; no stable percentage gain is claimed. Evidence: `/tmp/forge-valueflow-rust-repeat-evidence.json`.

Commerce ABBA for this slice has elapsed means 18.159s to 17.937s and CPU 60.805s to 60.325s at approximately 55–57% host busy time. Full graph, node details, compressed facts and search text match the preceding `1307d247` binary exactly. Stage means vary (graph 1.720s to 1.795s, seal 3.137s to 3.310s); these timing receipts do not establish individual-stage non-regression. Required live Commerce is 19.262s on the same 3,940-file inventory, with zero readback mismatches. Absolute <=12s, <40,000 unresolved, sub-millisecond RPC and stable incremental performance acceptance remain open. Evidence: `/tmp/forge-valueflow-rust-slice-performance-evidence.json`. Earlier low-load 11.535s frozen allocation results are not substituted for this latest live receipt.

- [x] Implement and prove bounded structural Rust callee extraction and imported type admission around synthetic impl blocks.
- [ ] Continue provider/re-export admission, tuple-field propagation, explicit container projection and constrained closure/generic contracts under the existing Phase 8 proof and performance guards.


**Next bounded Rust field contract:** Canonical production owner is languages/rust/value_flow.rs, with proofs in existing receivers/rust_value_flow.rs and receivers/persistence.rs. Named struct fields and shared dotted-field consumption already exist. Seed plain nominal parameter annotations into existing binding facts and extract tuple-field ordinal annotations from type children only. Preserve exact positions, declaration order, lifetime-erased nominal types and Unknown generic-dependent fields; primitive parameter types and explicit container projection remain outside this slice. Tighten actual generic-parameter identifier/root checks where necessary to prevent T::Associated from becoming a concrete provider. Reuse generic-owner information when practical, with no public model/schema change or new linker scans.

**Field proof matrix:** Existing public AST/linker proofs cover String and Vec named fields, tuple attributes/visibility and actual local method targets, plus a concrete field alongside unknown generic fields. The existing alias/flow proof retains all four initializer bindings and adds its nominal client parameter as the fifth ordered binding. A provider Holder(Client) to Holder(Other) change must relink an unchanged holder.0.execute consumer, match a cold rebuild and verify commitments. Run new public contracts on the pre-fix source, then full gates and matched-input coverage/cold/delta measurements. The older bbe inventory's 1424 deep dotted records and 345 numeric projections include non-calls and are opportunity samples, not promised recovery counts.

- [x] Produce tuple-field ordinal types and plain nominal parameter flow through the existing Rust typed facts, retaining primitive/generic barriers and declaration positions. Reuse one lazy generic-name set per owner. Named fields retain their existing source ordering; no public model/schema or shared resolver change.

**Rust field proof (a906b404):** Three public contracts fail on the pre-fix source, then pass: nominal/tuple actual member targets, generic-root Unknown boundaries, and field-provider changes relinking an unchanged consumer with cold/delta parity. Rust receiver 21/21, persistence 9/9, all-targets 382 tests (three existing ignored), strict all-feature Clippy and commitments 12/12 pass. Logs: /tmp/forge-rust-fields-*.log. Release SHA a906b404a52ce8489be1b867f7dc72a5c9e454a8802d86a46b5640557ec14823.

**Matched Rust outcome:** Frozen 139-file inventory 0435eb73 reports unresolved 15786 to 15706, external 4057 to 4200, resolved 9064 and ambiguous 81 unchanged. Actual std PathBuf tuple-field calls gain external provenance. Nodes 2975 and unique local edges 7209 are unchanged; local member targets are proved by fixtures, not claimed as a corpus gain. All 29692 durable raw references match byte-for-byte. The 63 additional persisted coverage rows are resolution/evidence separation: calls and references already exist, but their formerly identical unresolved evidence collapsed into one row. They are not newly extracted calls. Evidence: /tmp/forge-rust-fields-coverage-diff.json, /tmp/forge-rust-fields-raw-reference-proof.json, /tmp/forge-rust-fields-performance-evidence.json.

**Rust field performance remains bounded evidence:** Rust ABBA means elapsed 720 to 693ms, CPU 2.925 to 2.410s, seal 92.1 to 94.0ms. Commerce all 29 durable tables match; elapsed 12.383 to 12.434s, CPU 51.875 to 53.165s, files 1506 to 1540ms, graph 1113 to 1202ms, seal 2344 to 2352ms, with candidate host busy 40-46% versus baseline 39-41%. Whole/stage non-regression remains open. Latest live cold is 11.940s on stable 3960-file inventory 90845b12, resolved 166890, external 54347, ambiguous 53, unresolved 64245. This individual elapsed gate passes; <40000 unresolved and hot RPC sub-millisecond remain unmet.

**Thread-count experiment:** Same a906 binary and immutable input, ABBA 8-versus-16 threads, all four output roots equal. Elapsed 12.002 versus 11.851s; CPU 36.400 versus 51.665s (8 threads saves 29.5% CPU), graph 1071 versus 1155ms, seal 1973 versus 2259ms, extraction 3016 versus 2927ms, link 1597 versus 1528ms. Eight threads trades about 1.3% wall latency for less CPU. No production default or configuration changes are admitted from this tradeoff under the zero-latency-regression constraint. Evidence: /tmp/forge-threadcap-performance-evidence.json; PSI is recorded only for the final pair.

### Phase 9: Measured Commerce Resolution Priorities

**Baseline and authority:** The user requests the next steps for reducing Commerce unresolved. Read-only analysis of the validated release database `/tmp/bench-cru-cold.db` reports 3,940 files and 64,910 unresolved: Python 42,512, JavaScript 11,806, TypeScript 5,576, HTML 3,528, Vue 1,486, Proto 2. Among the five primary languages, unknown parameter/local callable identity accounts for 29,887 records, no lexical candidate for 19,174, and computed/dynamic callee for 9,897. These buckets overlap in potential remedies and are not promised recovery counts. To reach fewer than 40,000 unresolved requires at least 24,911 successful admissions. Evidence: `/tmp/forge-commerce-unresolved-priorities.json` and its read-only script.

| Order | Owner and concrete contract | Positive proof and guard |
| --- | --- | --- |
| 1 | TypeScript/JavaScript profile: admit finite ECMAScript callable conversion globals String, Number and Boolean through existing shadow checks. The callable builtin table omits these while builtin_type recognizes them. | Prove actual call coverage and lexical override boundaries. Observed String opportunity is 214 JS plus 151 TS occurrences; no guaranteed gain is asserted. Static matching adds no per-reference allocation. |
| 2 | Shared AST binding admission, TypeScript CommonJS extraction, main linker and semantic context: distinguish an imported declaration from an actual overwrite with source provenance and position. | Existing production assert import is external, but assert.equal is blocked by declaration/rebinding metadata before alias lookup. Prove local imported call edges and node:assert call provenance, actual reassignment, shadowed require and parameter scopes. Observed assert.equal/deepEqual counts are 825/223. Do not globally prefer aliases or subtract imported names. Shared binding collection currently omits TS assignment/update AST kinds; repair these boundaries together. Existing CJS tests proving only import edges do not prove call resolution. |
| 3 | Language profiles and browser/Vue extraction context: admit finite platform globals only with the appropriate platform/provider contract. | Prove document/Element/HTMLElement calls and types in TS and embedded JS, preserving lexical overrides and nullable query results. Observed TS createElement/Element/HTMLElement counts are 108/119/115 and JS querySelector 229. Framework setup macros and auto-imports require actual framework/build evidence; a manifest dependency alone does not establish a global. |
| 4 | Shared semantic provider admission and local return contracts: align wildcard/re-export providers with the ordinary linker; propagate declared/inferred local returns, typed fields and proven stdlib/framework factories. | Ordinary import linking expands wildcards while semantic import context skips '*'. Prove actual receiver declarations rather than assigning arbitrary methods by name. Python logger.info 480, row.get 569 and db.execute 475 are classification samples, not all one defect. Django create/get call admission exists; model return contracts require a separate proven provider rule. |
| 5 | Shared typed expression reduction plus language AST profiles: model deeper call chains with bounded durable typed evidence and distinct reference identity. | One-hop factory().member already works. Current callee strings cannot represent deeper chain structure. Cache keys using only owner/start position can collide between nested calls; use span/ordinal or borrowed expression discrimination. Public APIs remain stable and cold/public/cache/delta paths retain the evidence. Use a per-file arena and bounded depth rather than per-segment JSON trees or boxed/cloned expression DTOs. |
| 6 | Shared flow and callback scopes: propagate proven element/parameter types through callbacks, destructuring and branch guards. | Direct local TS arrow factories already work. New callback identities come from indexed signatures or known container element contracts, not guesses. Preserve nullable, reassignment, control-flow and unconstrained callback boundaries. Mixed object data and nested/default destructuring require explicit contracts. |

- [x] Implement conversion globals and prove observable calls through existing consolidated public test suites.
- [x] Repair lexical builtin suppression using actual containing scopes in shared linker admission. A nested sibling String declaration currently suppresses global String calls throughout a file. Origin is a pre-existing coverage gap found during JS conversion fixture audit. Prove sibling global calls, visible local declaration targets, unknown parameters and captured bindings through the public TS/JS linker seam; preserve conservative ambiguity and shared consumer parity. The finite conversion-global slice uses separate fixture files and does not claim this repair.
- [ ] Repair CommonJS declaration/reassignment admission across both linker consumers; prove actual calls, not only imports.
- [ ] Admit browser and Vue platform contracts with provider evidence and scope boundaries.
- [ ] Repair shared provider parity and bounded return/receiver contracts using representative Commerce samples.
- [ ] Design durable typed chains and callback evidence before admitting broader syntax.
- [ ] For every slice, compare same-input status counts, new source-backed edges, cold and incremental timing, strict Clippy, full tests and deterministic commitments. Keep the <40,000 target open until measured.


**Conversion globals proof (e0b98789):** Finite callable matches String/Number/Boolean reuse existing lexical/import/shadow guards for JS, TS and embedded scripts. The public test proves builtin calls, actual local String declaration targets and unknown Boolean parameter boundaries in separate files. The separately admitted sibling-scope builtin suppression gap remains open. All-targets passes 379 tests (three existing ignored), strict all-feature Clippy is clean, commitment integrity passes 12/12, release SHA e0b9878941bed9a5b3446d2bdc3718736dfde1c679744cb7ce99dbac059b5e3b; logs /tmp/forge-js-conversions-*.log.

**Same-input semantic result:** The immutable 3960-file Commerce inventory 156331d83206aba117543f1e4170dfc6a3c61750be48e58ff602399a036734d4 retains all 285530 coverage records and changes exactly 666 unresolved to resolved: String 427, Number 149, Boolean 90. Unresolved 64910 to 64244, resolved 166220 to 166886; external 54347 and ambiguous 53 are unchanged. JS contributes 369, TS 168, embedded HTML 120 and Vue 9. Nodes/details/local facts/search/edge targets/occurrences/documents match; only coverage evidence, dependency support and their commitments change. Evidence: /tmp/forge-js-conversions-coverage-diff.json and /tmp/forge-js-conversions-performance-evidence.json.

**Performance gate remains open:** Frozen ABBA elapsed means 24.537 to 22.887s, CPU 67.285 to 66.525s, files 3488 to 3197ms, graph 2297 to 2341ms, seal 4243 to 4118ms; host busy 60-75%. Higher graph-stage mean and host variation do not establish zero regression or absolute <=12s. Latest live is 20.270s with changed inventory 90845b12 (three changed files), resolved 166890, external 54347, ambiguous 53, unresolved 64245. Do not substitute the matched 64244 for live counts or compare different inventories as a regression. The first failed-adapter pair is excluded; immutable candidate /tmp/forge-valueflow-candidate-js-conversions-bin is preserved.

### Phase 10: Verified Cross-language and Rust Recommendation Amendments

**Authority:** The user requests verification of universal, Python, JS/TS, Vue, HTML and five additional Rust recommendations while the main plan remains active. This phase records accepted contracts, corrected diagnoses and rejected heuristics. It does not mark implementation complete or promise recovery of the observed counts. The recommendation audit is read-only. Its first two admitted Rust corrections are implemented and verified as a separately measured production slice; other amendments remain pending.

**Measurement correction:** Commerce remains 64,910 unresolved (Python 42,512; JS 11,806; TS 5,576; HTML 3,528; Vue 1,486; Proto 2). The supplied Python 49,543, JS/TS 18,245 and Vue 1,414 belong to other measurements. HTML has five missing-template rows, not five total unresolved. Rust baseline is 15,827, not the older serving-index 17,797. Evidence: `/tmp/forge-commerce-unresolved-priorities.json`, `/tmp/forge-rust-unresolved-priorities.json`.

**Rust pattern/lifetime slice measurement:** Both release `447e922e` and `bbe56c4e` use the same current 139-file source inventory (`56d175d9bfc3656d50025ead8d036c140773e6b7431910022b837303bb4fae0c`). Unresolved changes from 15,919 to 15,577 (-342), resolved from 8,861 to 8,995 (+134), external from 3,824 to 4,032 (+208), with ambiguous remaining 81 and zero readback mismatches. This is distinct from the earlier 138-file baseline. Rust ABBA elapsed means are 1,241ms versus 1,055ms, but the first baseline run has extra idle time; extraction increases 348ms to 387ms and linking 91.79ms to 100.54ms. CPU means change 3.955s to 3.860s. Stage-level non-regression remains open pending repeated matched measurements; reduced unresolved is not permission to inflate semantic certainty. Verification logs: `/tmp/forge-rust-pattern-lifetime-{focused,alltargets,clippy,commitment,release}.log`. Final evidence: `/tmp/forge-valueflow-rust-pattern-performance-evidence.json` and `/tmp/forge-valueflow-rust-pattern-status.json`. Bounded Rust repeat elapsed means increase 865.10ms to 912.48ms (+5.5%), CPU 2.965s to 3.240s (+9.3%), extraction 247.25ms to 268.38ms and linking 81.04ms to 91.04ms. Host busy ranges 34–47%; new lifetime support also adds 81 flow bindings and 27,887 details bytes with the same 2,967 nodes. These explain additional admitted work but do not establish zero regression. Commerce frozen ABBA increases 12.5% elapsed under different host load; full wire and graph hashes remain identical. Required live Commerce is 14.749s (persist_graph 1,250ms, persist_files 1,850ms, seal 2,674ms, link 2,001ms, extraction 3,527ms), 3,940 matching files and zero readback mismatches; unresolved remains 64,910. The <=12s, persist_graph<1.2s, unresolved<40,000 and stage non-regression acceptance gates stay open.

**Admitted performance investigation:** Profile the shared pattern traversal's simple-identifier cursor setup and duplicated reference unwrapping during lifetime parameter classification before making further changes. A bounded leaf fast path and delayed lifetime-shape check are concrete opportunities; the previous pattern path allocated a singleton vector, so source inspection alone does not prove a net regression. Preserve the new binding/type contracts and compare equal inventories, host load and admitted fact counts. No new SQL, JSON roundtrip or per-reference source scan was introduced by this Rust slice.

#### Shared admission and extraction contracts

| Recommendation | Disposition and canonical owner | Required proof and failure boundary |
| --- | --- | --- |
| Standard container methods | Extend existing LanguageProfile builtin_type/builtin_member/builtin_generic hooks and shared typed receiver reduction. Python-specific receivers.rs does not become the universal registry. Admit only AST-known literals, unshadowed constructors, annotations or proven returns. | A .get/.map/.push name cannot establish a receiver type. JS {} is Object, not Map; Rust {} is a block, not a dictionary. Prove language-correct literal/container members and actual custom/local declarations. Retain generic arguments and mutation boundaries. |
| External origin propagation | Retain verified receiver/import origins; add return/member contracts for proven external types and factories. Distinguish dependency provenance from callee provider identity. | A factory imported from another package can return a local callback, primitive or unknown object. Do not mark every descendant external. Existing external_factory_requires_a_return_contract_before_receiver_inference already proves this boundary. Keep manifest availability separate from type/signature evidence. |
| Apparent literal callees | Preserve legitimate literal method calls and actual invalid literal-call diagnostics. Correct false route facts at their producer. | Rust has 260 quoted-literal receiver calls: into 196, to_owned 42, repeat 9, to_string 6, contains 5, len 2; no bare empty-string/array/object unresolved call rows. Commerce samples 0, empty string and array are handles facts from generic get/put route detection, not call() argument extraction. |
| Route registrar provenance | Own ast/routes.rs plus the language route hooks and semantic provider admission. Require actual router/decorator/factory evidence before interpreting get/put as route registration. | request.GET.get(key, default) and PageState.put(namespace, arrayData) are data APIs. Prove their real calls/data relations and prove a canonical router still emits route/handler relations, including supported middleware arrays. Do not globally delete literal references. |
| Typed scope/cache ownership | Keep AST profiles as producers, typed facts as the shared contract, and semantic context/value flow as consumers. Deep chains require durable private evidence and distinct reference identity. | No new per-reference SQL, regex, JSON reconstruction or cloned expression trees. Cache reload and delta must consume the same evidence as cold extraction. Preserve public exhaustive Rust models and the bounded worklist. |

#### Next Rust slices, in implementation order

| Priority | Accepted change and owner | Verified evidence | Public proof and guards |
| --- | --- | --- | --- |
| 1 | Correct Rust binding-pattern traversal in both ast/mod.rs::scope_bindings and languages/rust/value_flow.rs. Skip constructor/type paths and field labels; bind actual payloads and shorthand fields. | Some is already builtin: 434 resolved / 96 shadowed; Ok 322 / 12; Err 49 resolved. All-child tuple_struct_pattern traversal adds Some as a false binding. | Prove constructor calls, payload variable identity and genuine local replacements. Cover tuple-struct and struct patterns. Do not simply remove Some/Ok/Err names globally. |
| 2 | Normalize lifetime-only generic type syntax to its nominal base in Rust type_expr. Preserve actual type/const arguments and unknown generic bounds. | Node<'_> currently loses all lifetime args and then becomes Unknown because args is empty. node.kind has 105 unresolved; node.child_by_field_name has 113. Imported Connection methods already partly external. | Prove actual tree_sitter Node<'_> parameter/method import provenance and local nominal lifetime types. Use structural AST normalization, not raw string stripping. Manifest declares dependencies; it does not supply type definitions. |
| 3 | Add finite per-type associated standard-operation and return contracts through Rust profile/shared admission. | Vec::new has 131 unresolved; instance Vec members already exist. Main linker normalizes :: to dot. | Prove unshadowed canonical Vec::new/with_capacity and applicable String/Box operations. Reject the type-by-operation cross product: Option::with_capacity is invalid. PathBuf needs explicit canonical provider; from/default need the appropriate trait contract. |
| 4 | Admit qualified std/core/alloc namespaces through private implicit-provider provenance, after local declaration/module lookup and crate context validation. | std::env::temp_dir has 40 unresolved; std::time::Instant::now has 10. Explicit imports already carry verified external evidence. | Prove implicit namespace calls/types and real local module identity. Account for edition, no_std, no_implicit_prelude and alloc availability. Use evidence such as Rust standard-library namespace; never fabricate a use/import line. Keep public API compatibility. |
| 5 | Align wildcard/re-export providers between ordinary linker and semantic context. | Ordinary linker expands wildcard aliases; semantic context skips alias '*'. Representative text, Facts and Syntax uses require provider tracing. | Prove super::* and public re-export calls plus declared return receivers, visibility, collisions and module boundaries. Precompute provider maps once. |
| 6 | Project declared named and tuple fields and AST-known literal/container receivers. | self.0 160 includes field access; self.0.join 63, facts.nodes.iter 62, node.id.as_str 37 are representative gaps. Rust literal hints are absent. | Prove exact declared field types and actual literal members. into/to_owned are trait-related: respect visible local traits and ambiguity rather than admitting every spelling. Preserve Deref, nullable/container and generic boundaries. |
| 7 | Compose deeper proven returns and explicit Result/Option projections, then closures and uniquely constrained traits. | Existing one-hop and declared local returns work; longer chains and ? remain incomplete. | Use bounded private expression evidence, durable hydration and distinct reference identity. Preserve async/future boundaries and unverified external returns. No cargo/rustc expansion or whole-workspace implementor scan on the hot path. |

**Already handled Rust cleanup:** Current release coverage has zero exact _, a, static, 'a and 'static expressions. type_references already skips lifetime/lifetime_parameter and _. self.0 is not to be dropped as a call: many records are legitimate field references. Keep positive extraction contracts and implement tuple projection instead of repeating stale cleanup claims.

#### Python, browser, Vue and template follow-ups

| Area | Accepted contract and evidence | Rejected shortcut / retained boundary |
| --- | --- | --- |
| Python logging / DB | Add canonical stdlib and indexed/stub-backed factory returns, including logging.getLogger and sqlite3.connect where the factory argument permits it. Follow local wrapper returns to actual classes. | logger can be a local ContextUnitLoggerAdapter; db/conn/cursor names establish no DBAPI identity. sqlite3.connect(factory=...) can return a local subclass. loguru requires provider evidence rather than arbitrary logger naming. |
| Django command fields | Existing proven BaseCommand ancestry already admits stdout.write/stderr.write. Add verified OutputWrapper field/alias facts and missing finite style contracts where justified. | Sample unresolved stdout.write entries are member references alongside already-external calls. Preserve local overrides and unknown-base barriers; do not advertise them all as unresolved calls. |
| JS/TS builtins | Array/Object, JSON.parse/stringify and selected Math members already exist. Complete missing callable/type/member contracts for standard globals, with separate browser context. | Boolean/Number type recognition does not recognize their calls. document alone does not admit document.querySelector because matching is currently expression-based. Node files do not gain arbitrary browser globals. |
| Alpine / JS identifier syntax | Structurally admit valid JavaScript identifier/member syntax including $, then consult an actual Alpine provider. | window.Alpine.$data is not a browser standard; all 120 observed rows must not become builtin solely by name. |
| Playwright | Follow verified imports, fixture contracts or Page/Locator factory signatures through actual browser/context/newPage/locator calls. | Test filename and variables page/editor do not prove Playwright types. Unknown external factory results stay unknown without contracts. |
| Vue macros | Record script-setup provenance and admit defineProps/defineEmits/defineExpose as scoped compiler intrinsics. | ref/computed/reactive/watch/onMounted/nextTick are runtime Vue APIs, requiring imports or a verified auto-import mapping. They are not universal compiler macros. |
| Nuxt / i18n / Pinia | Workspace Nuxt config enables i18n/Pinia modules; preload scoped mappings from actual config/generated declarations. Resolve local exported store factories before external fallback. Follow destructured t from useI18n through its return contract. | Merely installing unplugin-auto-import, Pinia or vue-i18n does not prove a global. Source exports useAdminStore/useMessagesStore, but these callable provider symbols are absent from the current index; fix factory/export admission. Do not mark arbitrary t/store names external. |
| Django templates | Five misses reference four shipped template candidates. Dependencies/settings prove Django admin/apps and APP_DIRS; Django/Jazzmin template resources exist outside indexed corpus. Build an exact cold-loaded provider inventory/config contract after indexed local providers. | Retain local template overrides and ambiguous provider evidence. admin/*, django/forms/* and especially registration/* prefixes alone do not establish external resources. These five fixes leave 3,523 other HTML unresolved records. |

**Reference contracts:** Vue compiler macro scope is documented at https://vuejs.org/api/sfc-script-setup.html; Nuxt auto-import behavior at https://nuxt.com/docs/3.x/guide/concepts/auto-imports. Installed dependencies do not substitute for the workspace's enabled mappings.

- [x] Implement Rust pattern-binding and lifetime-only nominal normalization first, in the existing Rust receiver/domain suites. Release `bbe56c4e` passes 377 all-target tests (3 ignored), strict all-feature Clippy and 12 commitment tests. The shared structural traversal preserves payload/shorthand bindings; only lifetime-only generic types become nominal types. Actual type/const arguments keep their uncertainty.
- [ ] Implement finite standard associated operations and implicit-provider provenance with local/provider priority.
- [ ] Repair proven false route registration facts and CommonJS binding/reassignment parity through existing public suites.
- [ ] Add standard container, canonical factory, platform/framework and resource contracts incrementally.
- [ ] Prove actual calls/types/fields plus supported boundaries, cold/cache/delta equivalence and deterministic commitments. Keep tests in existing domain suites <=800 lines.
- [ ] Run full all-targets tests, strict all-feature Clippy, commitment integrity and matched-inventory release cold/incremental benchmarks after every implemented slice. Observed counts remain opportunities until those measurements establish recovery.

---

## 4. Dependencies & Ownership

- **Owning Repository:** `projects/contextunity-forge-mcp` (Rust codebase)
- **Primary Agents:** Luna 6 xhigh (Builder), SOL 6 high (Reviewer)
- **Authority Docs:**
  - `docs/architecture/indexing.md`
  - `/tmp/forge-codebase-audit/REPORT.md` (Baseline evidence)


### Parallel language repair batch (October 1, 2026)

**Validated shared scope receipt:** Release 62438e9e passes 383 all-target tests (three existing ignored), strict all-feature Clippy and 12 commitment tests. Same frozen Commerce inventory changes 12 coverage entries (nine references, three calls), unresolved 64244 to 64232. Frozen Rust gains two builtin write! calls. Latest live Commerce is 15.553s / 64233 unresolved, so stable cold latency and the <40000 coverage target remain open. Evidence: /tmp/forge-shared-builtins-performance-evidence.json and /tmp/forge-shared-builtins-commerce-coverage-diff.json.

The user requests non-overlapping Python, TS/JS, HTML and Rust agents. Language producers and domain tests have separate owners; root integrates shared linker admission. Verification uses one Cargo lane and a combined release/benchmark batch.

| Slice | Admitted production contract | Proof and limits |
| --- | --- | --- |
| TS/JS | Verified module-level const CommonJS namespace imports survive their own binding barrier in both linker consumers. | Completed initializer, captured timing, actual writes/member replacement/namespace escapes, shadowed require and nearer bindings remain guarded. Missing/malformed provenance stays unknown. The 1048 assert.equal/deepEqual unresolved records are an opportunity, not a promised gain. |
| Python | Canonical logging.getLogger return origin only after verified external import admission. | Imported member identity, local logging.py, aliases, temporal/scope shadows and factory mutations must preserve actual providers or uncertainty. No logger-name heuristics. |
| HTML | Four exact Django framework template targets become external only when unindexed and declared in the same project. | Local template providers win. Nearest language manifest scopes and linked-workspace boundaries are collected in the existing manifest walk; a global Django dependency does not establish another project's framework context. Five current records are the bounded opportunity. |
| Rust | Primitive str receiver type and finite inherent core::str members. | Proven &str annotations, local candidate precedence and generic uncertainty are preserved. This remains valid with no_std/no_implicit_prelude; implicit prelude and allocating trait methods are not inferred. |

- [x] Complete production and positive public proofs for the four slices.
- [x] Close bounded review findings, then run all targets, all-feature strict Clippy and commitment integrity.
- [ ] Measure same-input Commerce and Rust coverage and cold/delta timing with immutable releases. Report actual gains and unmet criteria.


**Validated Python collection follow-up:** The language-owned value-flow producer currently returns Unknown for dictionary/list/set/tuple literals, while finite dict.get/list.append member admission already exists. A future compact, validated literal-kind fact must resolve directly to the builtin type, preserving positions, reassignments and conditional joins. Reusing an ordinary name-based dict/list annotation is unsound because local declarations may shadow those names while literal types remain builtin. Keep public model compatibility and avoid per-literal JSON roundtrips; admit the representation and durable/cold/delta contract together before implementation. The observed 1311 unresolved row.get/raw.get/data.get entries are an expression bucket, not a recovery claim: exact source samples instead require json.loads plus TypeGuard narrowing, TypedDict map semantics, or ORM values/iteration return inference. Prioritize source-backed TypedDict/narrowing and callback/container flow after the current parallel batch measurement; do not broaden resolution from method names alone.

**Final parallel language batch receipt:**

Metrics and evidence below are copied from the received summary JSON.

```json
{
  "release_sha": "c8b2e664f4e7a4cc543d570ac41bf1864cc77553ba3ee91583b55e9b89c11c9d",
  "tests": {
    "passed": 390,
    "ignored": 3,
    "failed": 0
  },
  "clippy_clean": true,
  "commitments_passed": 12,
  "review_findings_closed": true,
  "evidence_files": [
    "/tmp/forge-parallel-batch-performance-evidence.json",
    "/tmp/forge-parallel-batch-final-summary.json",
    "/tmp/forge-parallel-batch-raw-proof.json",
    "/tmp/forge-parallel-batch-raw-proof-summary.json",
    "/tmp/forge-parallel-batch-commerce-coverage-diff.json",
    "/tmp/forge-parallel-batch-forge-coverage-diff.json",
    "/tmp/forge-parallel-batch-commerce-status.json",
    "/tmp/forge-parallel-batch-forge-status.json",
    "/tmp/forge-parallel-batch-live-status.json",
    "/tmp/forge-parallel-batch-alltargets.log",
    "/tmp/forge-parallel-batch-clippy.log",
    "/tmp/forge-parallel-batch-commitment.log",
    "/tmp/forge-parallel-batch-release.log",
    "/tmp/forge-parallel-batch-focused-cjs.log",
    "/tmp/forge-parallel-batch-focused-html.log",
    "/tmp/forge-parallel-batch-focused-python-1.log",
    "/tmp/forge-parallel-batch-focused-python-2.log",
    "/tmp/forge-parallel-batch-focused-rust.log"
  ],
  "commerce": {
    "before_unresolved": 64232,
    "after_unresolved": 62770,
    "before_elapsed_ms": 20573.812434,
    "after_elapsed_ms": 18306.471086,
    "inventory": "156331d83206aba117543f1e4170dfc6a3c61750be48e58ff602399a036734d4",
    "live": {
      "unresolved": 62771,
      "elapsed_ms": 18697.016628
    }
  },
  "rust": {
    "before_unresolved": 15908,
    "after_unresolved": 15761,
    "inventory": "a6b1cd6812f8328602e0d36216a442a36b5527a8d840958fe1820cf0e94e7128"
  },
  "acceptance": {
    "coverage_met": false,
    "latency_met": false,
    "nonregression_proven": false
  },
  "attribution_summary": "Commerce 1,462 changed rows = 1,282 JS calls + 175 Python calls + 5 HTML templates. Rust 265 changed rows = 147 status gains + 118 still-unresolved evidence changes; 273 raw matching call occurrences are not new calls. Raw references and edges are identical.",
  "attribution": {
    "commerce": {
      "changed_rows": 1462,
      "javascript_call_rows": 1282,
      "python_call_rows": 175,
      "html_template_rows": 5,
      "raw_matching_call_occurrences": 1457,
      "raw_matching_template_occurrences": 5,
      "raw_references_identical": true,
      "raw_edges_identical": true
    },
    "rust": {
      "changed_rows": 265,
      "status_gains": 147,
      "still_unresolved_evidence_changes": 118,
      "raw_matching_call_occurrences": 273,
      "raw_matching_occurrences_are_new_calls": false,
      "raw_references_identical": true,
      "raw_edges_identical": true
    }
  }
}
```
