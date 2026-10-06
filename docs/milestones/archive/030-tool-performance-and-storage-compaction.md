---
id: m-tool-performance-and-storage-compaction
title: Tool performance hardening and SQLite database compaction
doc_type: contract
status: completed
depends_on:
- m-language-semantics-and-resolution-coverage:completed
owners:
- src/mcp/
- src/db/
- src/core/
- src/cli/
- src/engine/
- benchmarks/
- tests/
invariants:
- 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
- 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
- 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
- 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
started_at: 2026-10-06T20:35:12+00:00
handoff:
  completed_at: 2026-10-07T07:27:43.220679492+00:00
  duration: 10h 52m
  commit: 52c5773c76d942ccac815f89f9ec450817c9629c
  verification:
    command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
    status: passed
    tests_passed: 609
    tests_failed: 0
---

# Tool performance hardening and SQLite database compaction

## Outcome and purpose

Deliver the admitted 030 performance gates on the Commerce reference workspace while preserving the targeted removal proof, native FTS5 BM25 ranking, bounded test discovery and complete AST results, compressed facts, and deterministic commitments. Keep approximately 10 seconds as a cold-build aspiration for `commerce-release-update`; report actual phase and total timings without imposing a hard wall-time gate. Index function-body tokens only after the density headroom and false-negative contracts below are satisfied.

Every task serves cold-build and warm-start optimization with correct architectural boundaries. Preserve integrated startup performance, separate code regressions from host contention, and keep profiling logs in the task blackboard.

## Tasks in this milestone

### task: prove-removal-target-scoping

```yaml
task_ref: prove-removal-target-scoping
target: Prove removal safety over the full selected symbol or file scope for canonical and supported path selectors, and page selected IDs without weakening target-scoped diagnostics. Report the warm Commerce p95 against 30 ms as a reference goal; treat it as advisory when correctness holds, latency remains within the general 50 ms traversal recommendation, and there is no material regression.
proof_policy: direct-proof
contract_revision: 8
scope:
- src/db/traversal.rs
- src/mcp/tools.rs
- tests/
- benchmarks/milestone030_benchmark.py
subtasks:
- subtask_ref: removal-query-reuse
  title: 'Remove redundant SQL passes in removal_paged for selected IDs and dependency totals; reuse the admitted generation and bounded diagnostic-ID set to build the selected-ID page, derive dependency_total from the paged dependency result, and preserve canonical plus ./, file:, and file:// selectors, exact continuation totals, target-scoped diagnostics, and the 10,000-node guard. Acceptance: public MCP removal tests preserve selected IDs, dependency rows/totals, verdict, and diagnostics; a 30-warm-sample Commerce receipt reports median/p95, stable generation, and unchanged database metadata/size. Treat p95 <=30 ms as a reference target, not a hard delivery gate; if missed, compare with the general ~50 ms traversal recommendation and baseline, and stop when the impact is non-critical.'
  status: completed
  evidence: SOL 6.1 PASS at contract revision 7 / review claim 24. Expanded public MCP continuation test covers canonical, ./, file:, and file://; full selected-ID/dependency sequence parity, totals and target diagnostics/verdict through final empty page. `cargo test --test impact_context --test query_context --test tool_evolution` passed 62/62; strict all-target/all-feature Clippy clean. Current Commerce p95 36.317ms vs 36.913ms baseline is advisory and below the general ~50ms traversal guidance; no further profiling needed.
status: completed
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 8
  passed_at: 2026-10-07T07:25:42.196029303+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 609
      tests_failed: 0
      log: 'cargo test --all-targets exit0: 44 suites, 609 passed, 0 failed, 3 ignored; strict cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test commitment_integrity 14 passed; git diff --check and scoped rustfmt --check clean. Completed removal-query-reuse receipt proves selector aliases, page continuation, diagnostics, verdict and <=50ms advisory baseline; test diagnostics_and_source::removal_safety_uses_complete_evidence_when_the_visible_page_is_empty passes in full suite.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: SOL 6.1 reviewed src/db/traversal.rs, src/mcp/tools.rs, and tests/tool_evolution/mcp_boundaries.rs at revision 8; no out-of-scope production changes.
        claims:
          applicable: true
          evidence: The public MCP contract covers canonical, ./, file:, and file:// selectors, full selected-ID and dependency paging/totals, target-scoped diagnostics, verdict, empty continuation, and the 10,000-node guard. The 36.317ms p95 is advisory, below the general ~50ms reference, with no material regression.
        concurrency:
          applicable: true
          evidence: Generation-bound pagination and admitted snapshot/identity checks remain intact; review found no stale-generation or concurrent-reader defect.
        project_isolation:
          applicable: true
          evidence: Selector resolution stays confined to the selected workspace file/symbol and its dependency evidence; no global diagnostics were added.
        administration:
          applicable: true
          evidence: SOL 6.1 adjudicated the task PASS at contract revision 8. Direct build evidence is 609 tests, strict Clippy, and commitment integrity; the 30ms figure is treated as advisory.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

Historical 2026-10-01 evidence: `impact_context` 3/3, `tool_evolution` 17/17, `commitment_integrity` 12/12, strict Clippy clean, full suite 400/0, and public removal-proof assertions. Current production-path proof also requires identical selected IDs, dependencies, and safety verdict for canonical paths and their supported `./`, `file:`, and `file://` aliases, plus response rows bounded by the requested page size.

### task: fts5-bm25-hybrid-ranking

```yaml
task_ref: fts5-bm25-hybrid-ranking
target: Preserve FTS5 BM25 hybrid ranking, bounded candidate scoring, filters, and exact totals on every page while the count horizon permits
proof_policy: direct-proof
contract_revision: 6
scope:
- src/core/schema.rs
- src/db/symbols.rs
- src/mcp/tools.rs
- tests/
- benchmarks/milestone030_benchmark.py
status: completed
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 6
  passed_at: 2026-10-07T07:25:42.307977904+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 609
      tests_failed: 0
      log: 'cargo test --all-targets exit0: 44 suites, 609 passed, 0 failed, 3 ignored; strict cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test commitment_integrity 14 passed; git diff --check and scoped rustfmt --check clean. Production ranked_search_page in src/db/symbols/search.rs preserves requested page and horizon-aware search_total; public tests search_and_paging::natural_symbol_queries_use_bm25_and_skip_markdown_by_default and ranking_and_coverage::search_ranking_applies_to_other_names_and_wildcard_pages pass within cargo test --all-targets.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: SOL 6.1 reviewed src/db/symbols/search.rs ranking and continuation/count paths plus public query/MCP tests.
        claims:
          applicable: true
          evidence: Native BM25 and hybrid boosts remain bounded as contracted; filters and exact totals persist through the existing count horizon, including empty continuation pages. Search and query-context regressions pass.
        concurrency:
          applicable: true
          evidence: The paged query retains generation/snapshot fencing and does not introduce a second connection or stale continuation state.
        project_isolation:
          applicable: true
          evidence: Candidate construction and totals remain scoped to the caller's filters/workspace; exact/token-empty paths preserve their distinct equality contracts.
        administration:
          applicable: true
          evidence: SOL 6.1 adjudicated rev6 PASS. Search p95 is recorded against baseline; 30ms is a recommendation and not a delivery blocker.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

Historical 2026-10-02 evidence: native BM25 with exact/prefix boosts and bounded graph boost; p95 23.44 ms in the recorded search scenario, text 24.25 ms and prefix 17.24 ms on the recorded database. Reconcile current search and paging behavior using public-seam evidence and report Commerce p95 against the retained baseline. Treat 30 ms as a recommended reference, not a delivery blocker; assess larger values against the general full-text recommendation, paired behavior, and material user impact. Keep an exact total on empty continuation pages when the bounded candidate-count probe established it; keep totals unavailable beyond the existing count horizon.

#### Detailed Execution Plan
1. **Preserve broad symbol query behavior and report its latency**:
   - In `src/db/symbols.rs`, broad symbol queries generate a CTE with `UNION` between structural matches and FTS BM25 candidates:
     `WITH fts AS MATERIALIZED (...), candidates AS (...)`.
   - The current full-text plan computes hybrid scores and ordering before applying a dynamic FTS candidate cap of `max(500, offset + limit + 1)`. Structural candidates are name/qualified-name matches and are ranked in their own CTE; exact-token queries use FTS5 plus case-insensitive equality filters, while token-empty queries use the direct binary-equality path. The 1,001-row probe bounds only whether the exact total is reported, and graph boosts are calculated for a separate top-50 candidate window. Profile these current bounds before changing them; do not claim that `LIMIT 500` avoids FTS scoring or bounds structural candidate materialization.
2. **Verification**:
   - `python3 benchmarks/milestone030_benchmark.py --root ... --db ...` records search-scenario p95 values and compares them with the retained baseline. Treat 30 ms as a recommendation and 50 ms for `code_map_tests` as general guidance; judge variance by paired behavior, correctness, and material user impact.
   - Exact and token-empty queries retain their existing case-insensitive and binary-equality behavior. Do not add duplicate NOCASE indexes without a measured plan need.

---

```yaml
task_ref: test-discovery-and-ast-grep-acceleration
target: Reconcile bounded code_map_tests traversal and preserve complete ast_grep_search results while using indexed file selection only where its coverage is sound
proof_policy: direct-proof
contract_revision: 6
scope:
- src/db/symbols.rs
- src/cli/ast.rs
- tests/
- benchmarks/milestone030_benchmark.py
status: completed
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 6
  passed_at: 2026-10-07T07:25:42.426970298+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 609
      tests_failed: 0
      log: 'cargo test --all-targets exit0: 44 suites, 609 passed, 0 failed, 3 ignored; strict cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test commitment_integrity 14 passed; git diff --check and scoped rustfmt --check clean. Production bounded search_paged retains full indexed-file fallback for general syntax, declaration candidates only for declaration shapes; tests mcp_boundaries::ast_grep_prefilter_preserves_matches_for_body_only_literals and ranking_and_coverage::test_mapping_uses_lexical_fallback_when_no_test_edges_exist pass in full suite.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: SOL 6.1 reviewed src/cli/ast.rs candidate selection, src/db/symbols.rs test discovery, benchmark code, and public production-seam regressions.
        claims:
          applicable: true
          evidence: Indexed declaration candidates are used only for syntax coverage they represent; body/literal or general structural patterns keep bounded indexed-file fallback, so pruning cannot manufacture an exact zero. Pagination, digest and truncation contracts remain intact.
        concurrency:
          applicable: true
          evidence: Admission uses one consistent indexed snapshot and continuation state; no mutable cross-request search cache was added.
        project_isolation:
          applicable: true
          evidence: AST candidate paths and test discovery remain within the configured indexed workspace and existing test scope.
        administration:
          applicable: true
          evidence: SOL 6.1 adjudicated rev6 PASS. Body-only literal fallback regression and full 609-test suite pass; strict Clippy and commitment integrity pass.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

Historical 2026-10-01 evidence: test-discovery p95 9.67 ms and workspace AST p95 17.93 ms. Reconcile bounded graph traversal with lexical fallback. AST declaration patterns may use indexed declaration candidates; general structural patterns must scan the bounded indexed-file set unless every matchable syntax location is represented in the candidate index. Never return an exact zero after pruning files whose function bodies or literals are absent from `node_search`. The function-body-token task is completed with its storage-density and false-negative checks; do not describe it as blocked.

### task: mcp-inventory-scan-debouncing

```yaml
task_ref: mcp-inventory-scan-debouncing
target: Load one request-consistent adapter for MCP response policy, database admission, and final response enforcement without changing index semantics or database snapshot fencing
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/mcp/tools.rs
- src/mcp/server.rs
- tests/
- benchmarks/
status: completed
subtasks:
- subtask_ref: public-adapter-load-proof
  title: For warm code_map_search and code_map_inspect stdio calls on a stable indexed workspace, reduce forge-mcp.yaml opens from three to one. Compare first, warm, and TTL-expired request latency and one controlled Commerce cold build against retained baseline evidence; preserve raw measurements in the blackboard.
  status: completed
  evidence: 'Public stdio inotify: one YAML open versus three baseline; 30-call warm p95 2.20-3.34 ms versus 4.03 ms baseline; TTL 110-130 ms versus 113 ms. Controlled Commerce cold 13.428 s before final error/lint-only MCP edits; current cold 14.562 s compiler-contended, not comparable. /tmp/030-research/executed/r1/{adapter-reviewed-opens,adapter-reviewed-pair,cold-final,cold-reviewed}.json.'
- subtask_ref: request-adapter-owner
  title: Share one immutable adapter across tool policy, Server::admit, and response::enforce for each public MCP request. Re-read configuration on the next request so response/DEBUG edits, root and linked-workspace availability, deletion, and invalid YAML retain their existing behavior; keep database/WAL identity checks and transaction fences on every query.
  status: completed
  evidence: One immutable Loaded/Failed request adapter is shared by policy, admission, final enforcement. Invalid YAML no same-request reopen; next request observes repair; forge_guide init remains available. SOL 6.1 independent rereview accepted; mcp_freshness transition test passed; cargo test --all-targets, commitment_integrity (14), and strict clippy passed.
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 5
  passed_at: 2026-10-07T05:41:58.269022893+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp_freshness stdio_response_only_adapter_transitions_preserve_facts_and_continuations
      exit_code: 0
      tests_passed: 15
      tests_failed: 0
      log: 'Full all-target suite exit 0; commitment_integrity 14 passed; mcp_freshness transition 1 passed on final source; strict clippy zero warnings. Public stdio inotify 1 YAML open vs 3 baseline; measured timing in blackboard #342.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Implementation stays in src/mcp/server.rs and src/mcp/tools.rs; public stdio probe is benchmarks/mcp_adapter_single_load.py. No engine/scanner producer source changed in final candidate.
        claims:
          applicable: true
          evidence: 'Linux inotify proves one YAML open versus three; paired 30-call warm timing, first and TTL timing, Commerce cold profile, graph EXCEPT parity, and full test/clippy receipts are in blackboard #342. Cold reviewed run had compiler overlap and is not a controlled regression claim.'
        concurrency:
          applicable: true
          evidence: Independent SOL 6.1 review verified request-local Loaded/Failed snapshot, no same-request YAML reload on error, shared connection mutex lifetime, unchanged DB/WAL identity and transaction fences; initial P2 fixed and rereview accepted.
        project_isolation:
          applicable: true
          evidence: All public requests use the fixed Commerce workspace root and disposable per-binary database; current test suite and inotify probe report no project leakage or DB mutation.
        administration:
          applicable: true
          evidence: Task revision 5 contract and both subtasks verified; build worker differs from independent SOL 6.1 reviewer; malformed YAML retains forge_guide init recovery and next-request reload.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes:
    - Research artifacts are in /tmp/030-research/adapter/README.md and public_adapter_latency.py (scratch only). The driver uses existing benchmark McpClient with explicit --repo/--binary/--root/--db/--output; default public target is code_map_inspect(Server::admit); it records startup plus first/warm/5.25s-delayed call. Proposed retained candidate invocation uses /tmp/030-storage-ab/candidate + /tmp/030-storage-ab/0-candidate.sqlite + Commerce root. Current source implies 2 load_adapter invocations and 4 serde_yaml deserializations per code_map_inspect request when config exists; this is static source-derived expectation, not actual per-request count/measurement. No builds, tests, benchmarks, binaries, DBs, or source trees were modified.
    - 'Updated scratch driver at /tmp/030-research/adapter/public_adapter_latency.py per review: --warm-repeats default 30; median and nearest-rank p95; hashes/sizes for binary and main DB before and after client close; raw JSON-RPC responses retained; separate semantic response strips only top-level freshness/generation keys and preserves errors/captures; MCP/transport errors are written to output before nonzero exit. Default code_map_inspect selector now matches Commerce profile: extensions/commerce/src/contextunity/commerce/modules/matcher/pipeline.py:run_matcher_pipeline. README explicitly calls this admission baseline, not adapter-cache speedup (no candidate exists), with refreshed invocation. No harness run performed.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: sqlite-storage-compaction-zstd

```yaml
task_ref: sqlite-storage-compaction-zstd
target: Ensure database storage density <=45 KiB per file (<3 KiB per node) through compressed facts and dictionary paths, preserve all public path selectors and deterministic commitments, and verify integrated cold/warm startup non-regression against the retained pre-storage candidate
proof_policy: direct-proof
contract_revision: 7
scope:
- src/core/schema.rs
- src/core/typed_facts.rs
- src/core/commitments/domain_leaves.rs
- src/db/
- src/cli/ast.rs
- src/engine/tasks/context.rs
- src/engine/scanner.rs
- docs/architecture/performance.md
- tests/
status: completed
subtasks:
- subtask_ref: body-token-density-headroom
  title: On a clean 3,901-file Commerce candidate before body-token indexing, report measured density and remaining headroom against the <=45 KiB/file and <=3 KiB/node storage gates. Keep body-token indexing blocked until its own contract proves the final density and reserve on the integrated candidate
  status: completed
  evidence: Commerce3901 files/65551 nodes:178651136 bytes,44.723KiB/file2.662KiB/node; remaining0.277KiB/file(~1.06MiB). Estimated+1.94KiB/file body tokens do not fit, dependent task must prove final reserve. Retained /tmp/030-storage-ab artifacts;607/0 full tests.
- subtask_ref: path-text-index-range-proof
  title: Compare path_dictionary range lookup joined through nodes.path_id with the existing indexed nodes.path lookup; prove exact file, directory, and descendant result parity, inspect both query plans, measure cold index-build time and database density, and remove the duplicate nodes.path text/index only when the measured saving is material
  status: completed
  evidence: 'Retained schema14/15 databases: full canonical node projection diff0 both ways; exact28,directory117,descendant16601 IDs diff0. EXPLAIN uses dictionary unique path index+idx_nodes_path vs old idx_nodes_path_text. Index phase candidate1195.639/1149.011ms vs1331.386 baseline; save8749056 bytes. Cold wall13.032/13.327 vs13.069s, +5s not reproduced; host variability recorded. Public path/removal and full suite607/0 pass.'
- subtask_ref: storage-component-breakdown
  title: On the integrated Commerce database, attribute dbstat bytes to compressed local_facts, node rows, node_search, path text/indexes, graph tables, and remaining indexes; identify the removable component, then verify <=45 KiB/file, <=3 KiB/node, and commitment integrity
  status: completed
  evidence: 'dbstat /tmp/030-storage-ab/summary.txt: nodes24739840,facts17006592,node_search5439488,path dictionary+node path-ID index1048576,graph45973504 bytes; no duplicated nodes.path/index. Overall178651136 bytes,44.723KiB/file2.662KiB/node. integrity_check ok, candidate repeat roots identical; commitment_integrity and full607/0 pass.'
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 7
  passed_at: 2026-10-07T04:42:14.838418644+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets --no-fail-fast && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check && git diff --check
      exit_code: 0
      tests_passed: 607
      tests_failed: 0
      log: 'Final source verification exit0 archive30cba55c3e5de4bb,607 tests/44 results, strict clippy0 warnings,fmt/diff clean. Existing removal test failed pre-fix with missing nodes.path, now public canonical/alias tests green. Schema15 dictionary migration full node projection diff0, exact/file/descendant plans indexed, integrity ok. Retained /tmp/030-storage-ab binaries/DBs/JSON: storage13.032s,pre13.069s,storage13.327s; no compiler overlap, variable host load, +5s not reproduced but prior17.7s cause unproven. Warm exact search13.6–18.8ms; no generalized p95 claim. Density44.723KiB/file2.662KiB/node,save8749056bytes; 0.277KiB/file reserve insufficient for body tokens. Benchmark binary precedes removal query repair; measured build pipeline unchanged by repair. Milestone purpose now shared startup non-regression. SOL6.1 source review passed after repair, formal review pending.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Independent SOL6.1 PASS: schema15 removes duplicated persisted nodes.path/index; DB/CLI/task context use dictionary. Public projection preserved via nodes_with_path; file removal SQL repaired, ID branch unchanged. Revision7 scopes cover migration.'
        claims:
          applicable: true
          evidence: Complete streaming Zstd facts preserved. Retained Commerce density44.723KiB/file2.662KiB/node saves8749056bytes. integrity ok; canonical node projection diff0; exact/directory/descendant28/117/16601 IDs diff0 and indexed plans. Cold13.032/13.327 vs pre13.069s; +5s not reproduced, prior17.7s cause unknown. Warm exactsearch13.6–18.8ms only; no general p95 claim.
        concurrency:
          applicable: true
          evidence: Transaction/publication lifecycle unchanged, schema15 mismatch requires rebuild. Dictionary values preserve canonical commitments; repeated candidate roots equal. commitment_integrity14/14 and cold/delta/freshness tests green.
        project_isolation:
          applicable: true
          evidence: Same absolute Commerce root/adapter; independent retained databases and SHA-identified binaries in /tmp/030-storage-ab. Textual bounds and workspace/owner boundaries preserved. Compiler overlap0, host not fully idle, so residual -37..+258ms not definitively attributed.
        administration:
          applicable: true
          evidence: Independent SOL6.1 verifies revision7/buildclaim12,3 completed subtasks. Full suite607/0 across44 results, strict Clippy no warnings; formatter and diff checks exit0 archive30cba55c3e5de4bb. Remaining0.277KiB/file cannot fit estimated1.94 body tokens; dependent task owns reserve. Benchmark predates removal-only query repair; measured build pipeline unchanged. No delivery blockers.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Detailed Execution Plan
1. **Zstandard Compression in `src/core/typed_facts.rs`**:
   - `local_facts.facts_blob` uses streaming level-1 Zstandard compression and preserves complete facts during encode/decode.
   - Preserve the integrated storage acceptance of `<=45 KiB/file` and `<=3 KiB/node` when adding future projections.
2. **Verification Gate**:
   - Measure the final database from a clean Commerce build and report file and node density; do not use database size alone as a proxy.
   - Ensure delta hydration and Merkle root sealing remain identical (`cargo test --test commitment_integrity`).

---

### task: cold-build-latency-and-serialization-optimization

```yaml
task_ref: cold-build-latency-and-serialization-optimization
target: Optimize and accurately measure cold builds on the Commerce control repository using non-overlapping phase timings; keep about 10 seconds as an aspiration, not a hard delivery gate
proof_policy: direct-proof
contract_revision: 8
scope:
- src/core/commitments.rs
- src/core/commitments/coverage_leaves.rs
- src/core/models.rs
- src/db/delta.rs
- src/db/ingest.rs
- src/db/symbols.rs
- src/db/symbols/search.rs
- src/db/writer.rs
- src/engine/ast/mod.rs
- src/engine/ast/routes.rs
- src/engine/languages/html.rs
- src/engine/languages/mod.rs
- src/engine/languages/python.rs
- src/engine/languages/python/lazy_exports.rs
- src/engine/languages/python/render_context.rs
- src/engine/languages/python/value_flow.rs
- src/engine/languages/typescript.rs
- src/engine/languages/typescript/scope_facts.rs
- src/engine/languages/vue/template.rs
- src/engine/linker.rs
- src/engine/scanner.rs
- tests/ast_extractors.rs
- tests/core_basics/tasks.rs
- tests/core_basics.rs
- tests/commitment_integrity.rs
- tests/python_semantics.rs
- tests/delta_resolution_identity.rs
- tests/html_profile.rs
- tests/query_context/search_and_paging.rs
- tests/scanner_guard_limits.rs
- tests/typescript_semantics.rs
subtasks:
- subtask_ref: linker-phase-profile
  title: For the Commerce build, measure the full linker boundary including DependencyRegistry collection, capture every FORGE_PROFILE_LINKER phase, reconcile profiled phase totals with link_ms without assuming they include pre/post-profile setup, and identify the dominant phase before changing linker algorithms; do not alter AST visitors in this slice
  status: completed
  evidence: 'Commerce profile included registry collection and reconciled FORGE_PROFILE_LINKER phases with link_ms; recorded dominant ValueFlowIndex::build_parallel stage and 240.7ms residual. No AST visitor change. See task blackboard measured_delta #229.'
- subtask_ref: language-scope-fact-fast-paths
  title: Use syntax-aware TypeScript classification so required, comment/string require text, and React JSX onClick do not dispatch unrelated DOM/CommonJS fact collectors; preserve actual require(...) and supported DOM event/property facts, Commerce graph parity, and extract_ms improvement. Candidate-only text hints may run classification but must not dispatch collectors without matching syntax.
  status: completed
  evidence: 'Syntax-aware TypeScript candidate preserved all 29 SQLite table row sets on a fixed-root same-binary parity comparison; repeated candidate roots/rows were stable. Official paired 3,901-file profiles measured extract_ms 5,035.916ms legacy and 4,727.171ms candidate. Unicode syntax regressions were repaired and production-seam tests passed. See task blackboard #283/#287.'
- subtask_ref: python-module-id-cache
  title: 'Verify Python module-node ordering in the production extractor: the module is inserted before assignment traversal, so facts.nodes.find may already stop at the first element; measure lookup/clone cost before changing context plumbing, preserve route facts and edge keys, and make no change if no material Commerce extract_ms delta is demonstrated'
  status: completed
  evidence: 'No code change: ast::extract_typed inserts module:{path} as Facts.nodes[0] before Python assignment traversal, so find stops at the first node. No isolated Commerce before/after delta justified context-plumbing changes. Public route extraction test passed and preserved route facts/handles edge identity. See task blackboard #244.'
- subtask_ref: parser-pool-reconciliation
  title: Reconcile the existing with_warm_parser/parser_slot implementation with this contract using direct production-path evidence for each built-in grammar slot and extensible fallback; do not build a second parser pool
  status: completed
  evidence: 'Public same-thread languages::parse_file covered Vue .vue→.tsx→.js grammar-slot reuse/reset; unknown profiles retain create_parser fallback. ast_extractors 8/8, proto_ast 1/1, targeted language suites and strict Clippy passed. See task blackboard #237.'
- subtask_ref: coverage-stream-sort-and-path-cache
  title: Reconcile sorted coverage persistence, adjacent-path reuse, and owner-language aggregation with the existing ingest path; separate node-row timing from aggregate rows_ms (which includes broader persistence work), preserve exact rows and cold/delta parity, and report Commerce phase measurements
  status: completed
  evidence: 'Cold/delta public-seam tests compare exact resolution_coverage, coverage_owner_language, and coverage_language_counts rows; HTML override rows are nonempty. delta_resolution_identity 4/4, html_profile 42/42, strict Clippy passed. Commerce phases: coverage dictionaries 210.140ms, coverage/unresolved 603.674ms, graph persistence 1,667.744ms; rows_ms is aggregate. See task blackboard #240.'
- subtask_ref: owner-language-expression-prefilter
  title: Reconcile the existing DISTINCT owner-language expression query, chunked SQL-IN lookup, and in-memory filtering against production commitment construction; preserve row counts and repeated-build Merkle roots without adding a duplicate cache
  status: completed
  evidence: 'Production commitment path uses DISTINCT owner-filtered expression IDs, sorted 900-ID chunks, fail-closed dictionary lookup, and sorted owner rows. Production-seam test crosses 900+1 rows and proves repeatable full/partial roots for 901 rows. commitment_integrity focused test passed 1/1. See task blackboard #253.'
- subtask_ref: empty-exact-search-fast-path
  title: Reconcile the existing direct name/qualname path for exact searches that tokenize to empty; preserve kind, path, documentation, ambiguity, and paging semantics and report its 030 latency receipt
  status: completed
  evidence: 'Exact pattern ''_'' exercises the direct name/qualname path with empty FTS tokens and preserves kind/path/docs/ambiguity/paging semantics. 30 warm MCP calls: median 3.334ms, p95 3.972ms; focused query-context test passed 1/1. See task store evidence and task blackboard #263.'
- subtask_ref: node-row-timing
  title: Instrument node-row insertion separately from rows_ms and persist_ms; report node count, node-row elapsed time, full row phase, and total persistence without relabeling aggregate timings as per-node insertion latency
  status: completed
  evidence: 'Cold writer reports node_row_count and node_row_insert_ms separately from rows_ms/persist_ms; timing covers batch SQL prepare/bind/execute and is None for delta/single-row paths. Public roundtrip test in tests/core_basics.rs passed; release build and strict Clippy passed. See task blackboard #261.'
status: completed
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 8
  passed_at: 2026-10-07T07:25:42.252178404+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 609
      tests_failed: 0
      log: 'cargo test --all-targets exit0: 44 suites, 609 passed, 0 failed, 3 ignored; strict cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test commitment_integrity 14 passed; git diff --check and scoped rustfmt --check clean. All eight completed subtask receipts in revision8; current integrated cold-build series and profile archive /tmp/030-pipeline-implementation; public cold/delta and routing seams covered by cargo test --all-targets.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: SOL 6.1 reviewed the cold-build instrumentation and source scope in src/db/writer.rs, linker/AST pipeline, and the benchmark/task evidence; no unrelated code change was found.
        claims:
          applicable: true
          evidence: Phase accounting does not double-count setup outside FORGE_PROFILE_LINKER. Controlled baseline/candidate/repeat totals and exact persisted row/commitment parity are reported without attributing the full candidate delta to one subtask; ~10s remains aspiration.
        concurrency:
          applicable: true
          evidence: Writer snapshot/transaction ownership and parallel ValueFlowIndex boundaries remain unchanged; review found no concurrency or determinism defect.
        project_isolation:
          applicable: true
          evidence: Comparison uses the fixed Commerce workspace and retained binary/database identity, with workspace-specific adapter/root identity preserved.
        administration:
          applicable: true
          evidence: SOL 6.1 adjudicated the rev8 direct-proof receipts PASS. Full current suite, strict Clippy, and commitment-integrity evidence pass; no budget miss is a hard blocker.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Subtask audit disposition

`ast-visitor-context-reuse` was removed: `Extraction::visit` is entered for the root and recursion uses the static visitor, so there is no per-node `SyntaxContext` recreation matching that claim. `route-client-callee-fast-guard` was removed because the AST route-call guard already precedes argument parsing; no duplicate guard task is admitted. Parser pooling, sorted coverage, owner-language expression prefiltering, and empty exact-search handling are retained as direct reconciliation subtasks because the relevant mechanisms already exist and must be checked at their production seams rather than reimplemented.

#### Detailed Execution Plan
1. **Linker phase diagnosis**: use existing `FORGE_PROFILE_LINKER` boundaries to identify measured bottlenecks before changing algorithms; preserve the existing value-flow owner.
2. **Index/path storage**: preserve indexed dictionary text ranges joined through `nodes.path_id` and canonical path semantics.
3. **Storage headroom**: admit body-token storage only with measured headroom below the integrated density gate.
4. **Cold build accounting**: the preferred target is 10 seconds on the 3,901-file Commerce control repository. Report extract, link, persistence, index, seal, verify, and end-to-end wall time; the phase target must be internally feasible and must not double-count nested timings. `rows_ms` is aggregate row persistence, not node-only time.

---

### task: function-body-search-tokens

```yaml
task_ref: function-body-search-tokens
target: Decide whether function-body identifier and string-literal tokens can be admitted into the 030 search index without exceeding measured storage headroom or regressing cold and warm paths; retain the existing complete AST fallback and defer the projection when no candidate proves those conditions
proof_policy: direct-proof
contract_revision: 10
scope:
- src/core/models.rs
- src/core/typed_facts.rs
- src/engine/ast/mod.rs
- src/engine/ast/search.rs
- src/engine/languages/
- src/db/delta.rs
- src/db/ingest.rs
- src/cli/ast.rs
- tests/
- benchmarks/
status: completed
depends_on:
- cold-build-latency-and-serialization-optimization
- sqlite-storage-compaction-zstd
invariants:
- 'INV-NO-SOURCE-MIRROR: node_search stores deduplicated tokens, not raw function source, comments, or a second full-text table.'
- 'INV-SEARCH-BUDGET: On commerce-release-update, preserve the 030 search latency and storage acceptance while retaining measurable density headroom after token indexing.'
- 'INV-AST-PREFILTER: ast_grep_search skips a file for absent literal tokens only when node_search indexes every syntax location that the active language profile permits the pattern to match. When the index omits a permitted location, use a bounded candidate fallback or return an explicit budget outcome; never report a false zero-match result.'
subtasks:
- subtask_ref: density-admission
  title: 'For a proposed durable projection serving code_map_search({pattern: ''coverage_owner_language'', path: ''<file>'', exact: false}), measure the current Commerce database bytes per indexed file and node and compute remaining capacity under the 030 limits. Distinguish the measured reserve from the unverified 1.94 KiB/file token estimate; return no-go when no implementation candidate has measured extraction, FTS, delta, and cold/warm costs within that reserve.'
  status: completed
  evidence: 'Measured current 178651136 bytes/3901 files=44.7229 KiB/file; 65551 nodes=2.6615 KiB/node; file reserve 1106944 bytes=0.2771 KiB/file. +1.94 estimate unverified, actual projection costs unknown; no candidate admitted. /tmp/030-research/body/inventory-r1.json, blackboard #348.'
- subtask_ref: ast-body-fallback
  title: 'For ast_grep_search({pattern: ''return "marker"'', language: ''python''}) with a body-only literal in one file and an indexed declaration distractor in another, prove the public result still contains the body match. Classify token-only file exclusion as unsafe while module expressions, defaults, decorators, comments, and embedded-language islands lack complete indexed coverage.'
  status: completed
  evidence: Public ast_grep_prefilter_preserves_matches_for_body_only_literals test passed 1/1 through MCP with indexed declaration distractor and body-only literal. Complete syntax-location index coverage absent, so token-only exclusion remains unsafe.
- subtask_ref: projection-decision
  title: For body identifier and string-literal code_map_search hits, record that no new indexed_text match reason is delivered in 030 without measured storage and startup admission. Preserve exact-name search and bounded AST source matching. Define any later implementation as a separate search-quality contract covering nearest function ownership, literal forms, deterministic deduplication, durable facts, delta updates, and incomplete-coverage reporting outside semantic resolution identity.
  status: completed
  evidence: 'No body identifier or string-literal indexed_text projection delivered in 030: measured reserve small and no candidate proves storage/cold/warm admission. Preserve exact-name and AST fallback. Later search-quality contract must cover nearest owner, literals, durable facts, delta, deterministic dedup, incomplete coverage and semantic separation. SOL6.1 endorsed deferral.'
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 10
  passed_at: 2026-10-07T05:48:26.193734837+00:00
  evidence:
    test_proof:
      command: cargo test --test tool_evolution ast_grep_prefilter_preserves_matches_for_body_only_literals && cargo test --all-targets && git diff --check
      exit_code: 0
      tests_passed: 1
      tests_failed: 0
      log: Public MCP AST body-only literal fallback passed 1/1 on final Rust source; full all-target suite previously exit 0, subsequent edits doc/contract only; diff check exit 0. Storage admission calculation in /tmp/030-research/body/inventory-r1.json; no body-token code added.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Revision 10 changes the 030 task contract and evidence only; no new body-token index, source mirror, Node.details payload, or semantic resolution identity was introduced.
        claims:
          applicable: true
          evidence: SOL 6.1 independent final review confirmed measured 1,106,944-byte total reserve and unverified +1.94 KiB/file estimate are distinct, actual projection cost unknown, and no claim that all candidate projections are impossible.
        concurrency:
          applicable: true
          evidence: No database writer or reader concurrency path changed; complete AST fallback scans indexed files for non-declaration patterns, and the public body-literal-with-distractor test passed 1/1.
        project_isolation:
          applicable: true
          evidence: Commerce inventory uses fixed 3901-file source denominator and 65551 nodes; read-only evidence and final build share the reference root. Proposed token cost remains explicitly unmeasured.
        administration:
          applicable: true
          evidence: Task revision 10 explicitly delivers an admission no-go, not feature implementation; three evaluation subtasks have direct evidence. Separate future search-quality contract is required before implementation. Reviewer differs from builder.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    - 'INV-NO-SOURCE-MIRROR: node_search stores deduplicated tokens, not raw function source, comments, or a second full-text table.'
    - 'INV-SEARCH-BUDGET: On commerce-release-update, preserve the 030 search latency and storage acceptance while retaining measurable density headroom after token indexing.'
    - 'INV-AST-PREFILTER: ast_grep_search skips a file for absent literal tokens only when node_search indexes every syntax location that the active language profile permits the pattern to match. When the index omits a permitted location, use a bounded candidate fallback or return an explicit budget outcome; never report a false zero-match result.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

`exact: true` stays a name and qualified-name lookup. Fragment parsing for `ast_grep_search` stays outside this task. The existing underscore splitter and OR query stay; path-scoped search is the acceptance, because a workspace-wide OR of `coverage`, `owner`, and `language` is not a unique hit.

### task: mcp-read-concurrency

```yaml
task_ref: mcp-read-concurrency
target: After admission optimization, measure overlapping public MCP requests at 1, 2, 4, and 8 callers; retain one serialized reader unless a bounded execution and reader design demonstrates a material concurrent benefit without single-request, startup, memory, or snapshot-fencing regressions
proof_policy: direct-proof
contract_revision: 4
scope:
- src/main.rs
- src/mcp/server.rs
- src/mcp/tools.rs
- src/db/reader.rs
- docs/architecture/concurrency.md
- docs/adr/0002-generational-sqlite-concurrency.md
- tests/mcp_freshness.rs
- tests/mcp_freshness/
- benchmarks/
status: completed
depends_on:
- mcp-inventory-scan-debouncing
subtasks:
- subtask_ref: read-serialization-profile
  title: Send overlapping code_map_search and code_map_inspect requests with distinct IDs through one real stdio MCP process at 1, 2, 4, and 8 callers after adapter reuse. Record end-to-end p95, throughput, startup, single-request latency, sampled RSS, result parity, and database identity. Identify the current-thread dispatch and Server.read mutex as source-level serialization owners; do not attribute measured latency to admission, waiting, query, or rollback without phase instrumentation.
  status: completed
  evidence: 'Public stdio 30 batches each at 1/2/4/8 callers, exact and inspect; end-to-end p95/throughput, initialize, sampled RSS, parity, DB identity in /tmp/030-research/executed/r1/concurrency-reviewed.json and blackboard #346. Runtime+mutex owners source-verified; no phase-level attribution claimed.'
- subtask_ref: bounded-reader-pool
  title: For overlapping public code_map_search and code_map_inspect calls, admit bounded request execution together with reader ownership only after a candidate proves a material benefit over the serialized reader while preserving startup, single-request latency, memory, cancellation, result parity, database replacement, and transaction fences. A connection pool alone cannot parallelize synchronous handlers on the current-thread runtime. If no candidate proves these conditions on the Commerce control workload, retain one reader and record that no-go decision with the measured queueing evidence.
  status: completed
  evidence: No concurrent candidate proves material benefit without startup/single-request/memory cost. Existing one-reader inspect p95 48.07ms at eight callers, 175 req/s, parity and DB identity green. SOL6.1 independently advised no-go for milestone 030; retain serial reader and snapshot fencing.
- subtask_ref: concurrency-doc-contract
  title: State in architecture/concurrency.md that call_tool owns one request adapter snapshot, Server::admit consumes it, the current-thread runtime and reader mutex serialize synchronous handlers, and database/WAL identity plus transaction rollback remain per-query fences; verify ADR 0002 agrees with that model.
  status: completed
  evidence: 'Updated docs/architecture/concurrency.md and ADR0002: call_tool owns one adapter snapshot; Server::admit consumes it; current-thread synchronous handlers and mutex serialize; main/WAL identity and rollback per query remain.'
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 4
  passed_at: 2026-10-07T05:46:26.970106263+00:00
  evidence:
    test_proof:
      command: python3 /tmp/030-research/concurrency/mcp_concurrency.py --binary target/release/contextunity-forge-mcp --workspace /home/oleksii/ContextUnity/worktrees/commerce-release-update --db /tmp/030-research/executed/r1/cold-reviewed.sqlite --output /tmp/030-research/executed/r1/concurrency-reviewed.json --scenario both --callers 1,2,4,8 --rounds 30 && cargo test --all-targets && git diff --check
      exit_code: 0
      tests_passed: 8
      tests_failed: 0
      log: 'Eight public stdio scenario/caller levels completed; parity and durable DB validation green, no failed rows. Full Rust all-target suite exit 0 before doc-only correction; diff check exit 0. No reader-pool code admitted. Measured no-go in blackboard #346.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Only docs/architecture/concurrency.md and ADR0002 model descriptions changed for this conditional task; no reader, runtime, or transaction code was altered.
        claims:
          applicable: true
          evidence: Public one-process stdio 1/2/4/8 caller receipts include p95, throughput, startup, sampled RSS, result parity, and durable DB identity. SOL 6.1 independently advised no-go and warned against phase attribution without instrumentation; corrected contract and report preserve that limit.
        concurrency:
          applicable: true
          evidence: Current-thread synchronous handlers and one mutex serialize requests; eight-caller inspect p95 48.07ms with 175 req/s. A pool alone cannot improve dispatch; no admitted candidate proves material benefit without startup/single-request regression. Existing snapshot fencing remains.
        project_isolation:
          applicable: true
          evidence: Commerce root fixed; harness copies source DB read-only into separate disposable DB per caller level, validates responses and DB identity, and reports no failed rows.
        administration:
          applicable: true
          evidence: Revision 4 explicitly permits measured no-go and all three subtasks have bounded evidence; reviewer worker differs from build worker. Architecture and ADR adapter ownership now agree.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: language-profile-module-extraction

```yaml
task_ref: language-profile-module-extraction
target: Map production-module responsibilities across language extraction, linking, scanning, AST search, database queries, delta, MCP tools, manifests, and commitments. Treat modules over 800 lines as inventory candidates only; extract a responsibility only when its owned inputs and outputs form a cohesive boundary, preserve public contracts, retain tightly coupled pipelines, and align modularity guidance with those rules.
proof_policy: direct-proof
contract_revision: 9
scope:
- docs/adr/0001-dual-surface-parity.md
- src/engine/languages/typescript.rs
- src/engine/languages/typescript/
- src/engine/languages/python.rs
- src/engine/languages/python/
- src/engine/linker.rs
- src/engine/linker/
- src/engine/scanner.rs
- src/engine/scanner/
- src/engine/ast/mod.rs
- src/engine/ast/search.rs
- src/engine/languages/vue.rs
- src/engine/languages/vue/
- src/engine/languages/html.rs
- src/engine/languages/html/
- src/engine/languages/manifests.rs
- src/engine/languages/manifests/
- src/db/reader.rs
- src/db/reader/
- src/db/symbols.rs
- src/db/symbols/
- src/db/ingest.rs
- src/db/ingest/
- src/db/delta.rs
- src/db/delta/
- src/mcp/tools.rs
- src/mcp/tools/
- src/db/tasks_store.rs
- src/core/commitments.rs
- src/core/commitments/
- tests/typescript_semantics.rs
- tests/python_semantics.rs
- tests/query_context/
- tests/scanner_guard_limits.rs
- tests/commitment_integrity.rs
- tests/commitment_integrity/
- tests/mcp_freshness/
- tests/tool_evolution/
- tests/languages/
subtasks:
- subtask_ref: large-module-boundary-inventory
  title: Inventory every production Rust module over 800 lines in the checked-out source; the current set is src/engine/languages/typescript.rs, src/engine/linker.rs, src/engine/linker/value_flow.rs, src/engine/linker/semantic_context.rs, src/core/commitments.rs, src/engine/languages/python.rs, src/db/reader.rs, src/engine/scanner.rs, src/db/symbols.rs, src/engine/ast/mod.rs, src/engine/languages/python/value_flow.rs, src/engine/languages/typescript/value_flow.rs, src/db/delta.rs, src/engine/languages/vue.rs, src/db/ingest.rs, src/engine/languages/html.rs, src/engine/languages/manifests.rs, src/db/tasks_store.rs, and src/mcp/tools.rs. Record cohesive responsibilities and concrete call/data seams, then classify split, retain, or defer with an ADR-grounded reason. Use 800 lines only as a discovery filter; do not target a fixed child-module size.
  status: completed
  evidence: 'Blackboard #194 records the 19 production Rust modules over the discovery-only 800-line threshold, cohesive split/retain/defer classification, input/output seams, and ADR 0001/0002/0003/0004/0012 rationale. No fixed child size is imposed.'
- subtask_ref: typescript-dom-analysis-module
  title: Move DOM listener receiver-expression indexing and the partial callback-scope AST walk from typescript.rs into scope_facts.rs; retain the fused full scope walk that also records CommonJS require shadowing, and preserve supported call/property syntax, scopes, route facts, and edge/status sets
  status: completed
  evidence: 'Blackboard #205: typescript.rs 3,477→3,323 lines; private scope_facts.rs 165 lines. Preserved prepare dispatch, prefilters, scopes, receiver depth, CommonJS fused full walk and FileContext ownership. cargo test --test typescript_semantics 69/69; strict Clippy and diff check passed; SOL 6.1 PASS all contours.'
- subtask_ref: typescript-manifest-module
  title: Move package.json and npm/pnpm/yarn lockfile dependency metadata parsing from typescript.rs into a cohesive child module without changing manifest paths, normalized dependency facts, or indexed graph results
  status: completed
  evidence: 'Blackboard #201: typescript.rs 3,626→3,477 lines; private package_metadata.rs 212 lines. Preserved manifest adapters, local dependency filtering/order. cargo test --test typescript_semantics --test manifests 88/88; SOL 6.1 PASS all contours; diff check clean.'
- subtask_ref: python-module-export-boundary
  title: Move module_alias_exports and its type-checking-alias collector into python/module_exports.rs; keep direct_module_assignment in python.rs for symbol_with_source, and preserve FileContext export facts, re-export targets, statuses, and exact edges
  status: completed
  evidence: 'Blackboard #197: python.rs 1,506→1,301 lines; private module_exports.rs 254 lines. Python::prepare remains sole caller; direct_module_assignment stays in parent and lazy export parser is reused. cargo test --test python_semantics 26/26; diff check clean.'
- subtask_ref: module-split-evidence
  title: For each extracted module, report final parent/child line counts, public/private boundary, focused semantic results, and any comparable Commerce timing available; label candidate-only receipts and do not claim speedup or non-regression without a controlled baseline. Reject splits that add cross-module coupling or regress a measured phase
  status: completed
  evidence: 'Final sizes for the latest slices: vue.rs 608 + setup_facts.rs 526 lines; html.rs 920 + htmx.rs 157; manifests.rs 414 + javascript_packages.rs 240 + typescript_paths.rs 263; tools.rs 481 + inputs.rs 371. Their public semantic/MCP suites passed 259/259 and SOL 6.1 source review passed. The 12,972.7 ms Commerce profile is an earlier candidate-only aggregate receipt, predating these latest slices; no exact unrefactored or per-module baseline is available, so no speedup or quantified non-regression is claimed.'
- subtask_ref: linker-phase-module-boundary
  title: After linker-phase-profile identifies the dominant FORGE_PROFILE_LINKER stage, extract only that cohesive phase from link_compact_impl into a narrow child module when this removes a real boundary without duplicating input assembly; preserve node/edge/status outputs and compare the measured phase and whole-link timings
  status: completed
  evidence: 'Conditional extraction decision: retain current boundary, no source changes. Candidate link_ms=2,052.5ms; checkpoint sum=1,811.8ms; remaining 240.7ms is outside these phase checkpoints and is not attributed to value flow. Value-flow phase is 912.4ms, and linker.rs:644 invokes the existing value_flow::ValueFlowIndex::build_parallel API; value_flow.rs already owns scope initialization, bounded reduction, computed-receiver indexing and output index. Architecture/indexing.md documents this seam. A second wrapper would add no owned responsibility and could obscure/duplicate input assembly. No AST visitor change. SOL 6.1 PASS independently verified the current boundary and conditional subtask at contract revision 8; no code edits/tests needed.'
- subtask_ref: scanner-adapter-module
  title: Move Adapter types and load_adapter parsing/normalization from scanner.rs into scanner/adapter.rs behind the existing scanner API; preserve canonical roots, linked-workspace roots, ignore rules, adapter refresh behavior, and scanner_guard_limits/config_language_profiles/linked_workspaces results
  status: completed
  evidence: 'Blackboard #208: scanner.rs 1,343→1,055 lines; private adapter.rs 304 lines; public facade and adapter normalization/guard behavior preserved. Focused scanner/config/linked workspace suites: 18 passed, 1 existing stress test ignored; strict Clippy, diff check, SOL 6.1 PASS.'
- subtask_ref: database-query-domain-modules
  title: Move search_with_options/ranked_search_page into symbols/search.rs, test discovery and its admission/connection helpers into symbols/test_discovery.rs, and select_detail into reader/selectors.rs; preserve DB/page contracts; line anchors use an exact indexed file first, fall back only to whole-component suffixes when no exact file exists, return NotFound if no file covers the line and typed Ambiguous across multiple paths; propagate selector-enrichment row-budget errors; pass query_context/tool_evolution seams
  status: completed
  evidence: 'Blackboard #212 records search/test-discovery/selectors ownership moves and the amended line-anchor/enrichment fail-closed contract. Four public reader regressions cover ambiguity, missing-line path, exact-file precedence, and row budget. query_context/tool_evolution/core_basics 104/104; strict Clippy; SOL 6.1 final PASS.'
- subtask_ref: ast-search-module
  title: Move structural pattern compilation, search_range, search_page, and match-budget handling from engine/ast/mod.rs into engine/ast/search.rs; preserve every language profile's AST result set, digest checks, bounded continuation/truncation fields, and tests/tool_evolution plus the ast_extractors public suite
  status: completed
  evidence: 'Blackboard #215: ast/mod.rs 1,096→742 lines; private search.rs 363 lines; extraction/search ownership and public CLI facade preserved. ast_extractors/tool_evolution 25/25; strict Clippy; SOL 6.1 PASS. Existing workspace-wide fmt drift documented; no unrelated rewrite.'
- subtask_ref: commitment-phase-modules
  title: Move leaves_nodes/leaves_edge_occurrences/leaves_edges/leaves_shared_owners/leaves_dependencies/leaves_generic into core/commitments/domain_leaves.rs; move coverage_dictionary, owner_language_expression_dictionary, and canonical_*_leaves into coverage_leaves.rs; move SnapshotCommitments plus seal_snapshot/hash_snapshot/install_snapshot into snapshot.rs. Keep the existing commitments.rs codec, dispatcher, root/aggregate, seal/verify facade and all caller paths; preserve valid row encoding/order, owner/domain digests, output_root, candidate-install transaction boundary, and repeated cold-build roots. In resolution_coverage, fail closed during seal and verify when a row's path_id, expression_id, or evidence_id is absent from its dictionary; prove this with FK-disabled orphan-row cases in tests/commitment_integrity/coverage.rs. Pass tests/commitment_integrity.rs.
  status: completed
  evidence: 'Commitment phase extraction and its admitted fail-closed contract are verified. Layout: commitments.rs 1,515→445 lines; domain_leaves.rs 564, coverage_leaves.rs 436, snapshot.rs 102. Existing encodings/order/root/install transaction boundaries are preserved; missing path_id, expression_id, and evidence_id now return contextual errors. Public regression covers all three FK-disabled orphan cases through verify and seal; it reproduced acceptance before the fix. cargo test --test commitment_integrity: 13 passed, 0 failed; cargo clippy --all-targets --all-features -- -D warnings passed; git diff --check clean. SOL 6.1 final review PASS across paths, claims, concurrency, project isolation, and administration. Revision 8 explicitly scopes tests/commitment_integrity/.'
- subtask_ref: vue-setup-analysis-module
  title: Move the Vue script-setup fact collectors for typed iterables, slots, registered components, props, callable bindings, and shadowing from languages/vue.rs into a focused child module behind extract_file_impl; preserve emitted Vue facts, source positions, edge/status sets, and python/typescript/Vue semantic suites
  status: completed
  evidence: 'Implemented `vue/setup_facts.rs` as the child producer for the script-setup collectors; `extract_file_impl` and emitted facts/source positions remain the public path. The combined production suites passed: `cargo test --test python_semantics --test typescript_semantics --test html_profile --test language_boundaries --test manifests --test mcp_context --test mcp_freshness --test tool_evolution` (259 passed, 0 failed). SOL 6.1 reviewed the moved collector boundary and found no contract defect.'
- subtask_ref: html-htmx-analysis-module
  title: Move HTML HTMX attribute extraction and URL-status classification, including its attribute helpers, from languages/html.rs into html/htmx.rs behind the current HTML extraction entrypoint; preserve template references, endpoint statuses, source lines, and HTML/Vue/template public-seam results
  status: completed
  evidence: Implemented `html/htmx.rs` for HTMX extraction and URL classification; shared `attribute_value` remains in the HTML profile because import extraction also uses it. Public HTML/Vue/template/MCP suites are green in the combined 259-test run (0 failed). SOL 6.1 confirmed HTMX-specific ownership and shared HTML helper placement.
- subtask_ref: manifest-family-modules
  title: Split JavaScript package/workspace normalization and TypeScript path-config resolution from languages/manifests.rs into cohesive child modules while retaining DependencyRegistry as the facade; preserve nearest-scope selection, normalized imports, manifest digest, and cold/delta graph parity
  status: completed
  evidence: Implemented `manifests/javascript_packages.rs` and `manifests/typescript_paths.rs` behind `DependencyRegistry`. `cargo test --test manifests` passed 19/19; the combined public semantic/MCP run passed 259/259. Strict all-target/all-feature Clippy passed after `resolve_all` takes parsed configs into a local map and releases them after resolution. SOL 6.1 reviewed nearest-scope/normalization ownership and the lifetime fix.
- subtask_ref: delta-stage-module-boundaries
  title: Trace db/delta.rs::delta from changed-path selection through typed-fact extraction, dependency relinking, transactional persistence, and snapshot publication; extract only a stage with a concrete input/output boundary into db/delta/ while preserving transaction ownership, cold/delta parity, output_root, and source-snapshot retry behavior
  status: completed
  evidence: 'Completed the production trace and retained `db/delta::delta` intact: changed-path selection, source-snapshot checks, extraction/identity, closure, hydration, and the single transaction through persistence, seal, verify, commit/checkpoint, and publication form one ordered lifecycle. No stage has an independent owned input/output that can move without relocating transaction ownership or inventing a synthetic state bundle. SOL 6.1 independently confirmed no extraction diff and that transaction/snapshot/publication remain cohesive; the combined relevant suites passed 259/259.'
- subtask_ref: mcp-tool-domain-modules
  title: 'Inspect rmcp #[tool_router] composition before moving code from mcp/tools.rs; keep the registered tool names and JSON schemas unchanged, extract cohesive request types/helpers or tool domains only where the macro supports it, and prove the public tool catalog and tool_evolution/mcp_freshness behavior remain identical'
  status: completed
  evidence: Implemented `mcp/tools/inputs.rs` for request DTOs/defaults/validation and kept tool handler composition, names, and schema facade in `mcp/tools.rs`. `cargo test --test mcp_context` passed 30/30, including `stdio_tool_catalog_uses_object_schemas_for_every_property`; tool_evolution/mcp_freshness and the combined 259-test run passed. SOL 6.1 confirmed wire-facing schema and router behavior are preserved.
status: completed
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 9
  passed_at: 2026-10-07T07:25:42.371059167+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 609
      tests_failed: 0
      log: 'cargo test --all-targets exit0: 44 suites, 609 passed, 0 failed, 3 ignored; strict cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test commitment_integrity 14 passed; git diff --check and scoped rustfmt --check clean. All fifteen completed subtask receipts in revision9; current modular production paths and suite validate via cargo test --all-targets; no fixed child-size contract.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: SOL 6.1 reviewed the extracted child modules and facades across AST search, scanner, language profiles/manifests, MCP tools, DB readers, and commitments.
        claims:
          applicable: true
          evidence: Extracted modules own cohesive inputs/outputs while tightly coupled linker/delta stages remain intact. All 15 completed subtask receipts preserve caller contracts and production-path evidence.
        concurrency:
          applicable: true
          evidence: Parser-slot reuse, file context ownership, transactions, and snapshot publication are preserved across the moved boundaries; no duplicate pools or shared mutable state were introduced.
        project_isolation:
          applicable: true
          evidence: Module moves retain existing workspace roots, package scopes, and project-isolation checks; current public semantic suites preserve provider boundaries.
        administration:
          applicable: true
          evidence: SOL 6.1 adjudicated rev9 PASS. Combined production semantic/MCP suites passed 259/259, with strict Clippy and full current test suite green.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

---

### task: cold-build-pipeline-optimization

```yaml
task_ref: cold-build-pipeline-optimization
target: Reduce end-to-end cold-build time by borrowing AST owners, reusing TypeScript root and declaration analysis, and sharing equivalent linker node lookups while preserving semantic output and warm-path performance
proof_policy: direct-proof
contract_revision: 3
scope:
- src/engine/ast/
- src/engine/languages/
- src/engine/linker.rs
- src/engine/linker/
- tests/
- benchmarks/
status: completed
depends_on: []
invariants:
- 'INV-INTEGRATED-COLD-GAIN: Retain changes only when controlled full Commerce builds demonstrate an end-to-end improvement without cold or warm regression. About 10 seconds remains an aspiration; phase-level improvements alone do not complete a subtask.'
- 'INV-COLD-PROOF: Compare retained release binaries built with the same toolchain, lockfile, features and settings on the fixed 3,901-file Commerce root and adapter. Use fresh processes and separate empty databases, no warmup and no OS page-cache flush, with compilation finished before timing. Record full process wall time, non-overlapping phases, peak RSS, database bytes and semantic parity. Start with one baseline/candidate pair; use one additional baseline run if host noise prevents attribution, within the repository profiling cap. Report raw samples rather than a percentile from a single pair; keep measurements and source identities in the blackboard.'
- 'INV-SEMANTIC-PARITY: Preserve symbol identities, scope and source-position rules, Unicode syntax, ambiguity and fail-closed states, edge multiplicity and representative ordering, coverage, durable facts, canonical Merkle commitments, and cold-versus-delta equivalence.'
- 'INV-FINGERPRINT-PARITY: Compare canonical semantic rows and applicable domain commitments across binaries. Language source edits change FORGE_LANGUAGE_PROFILE_DIGEST and index-semantics metadata, so explain those expected fingerprint and output-root differences. Require repeatable roots and cold/delta equality within each binary; allow no unexplained semantic difference.'
- 'INV-BOUNDED-OWNERSHIP: Keep shared analysis and dictionaries immutable and scoped to their parsed root or build snapshot; preserve memory bounds, workspace isolation, source-change detection, atomic publication, and existing integrity checks.'
subtasks:
- subtask_ref: borrow-ast-owner
  title: In the shared AST visitor, borrow the current owner for nonsymbol syntax nodes and allocate an owner ID only when a declaration changes ownership. Preserve exact contains/calls/routes attribution across Python, JavaScript, TypeScript, Rust and embedded-language islands. Prove unchanged canonical extraction rows, within-binary Merkle determinism, removal of the per-nonsymbol owner copy, and a measured extraction and full cold-build improvement.
  status: completed
  evidence: 'Implemented borrowed AST owner with ownership created only for declarations; preserves scope/call/route attribution. Shared integrated verification: 609 passed, strict Clippy and SOL6.1 PASS; all 38,258 Commerce domain commitments match. Integrated candidate cold build 11.890 s versus nearest retained baseline 12.266 s; no isolated subtask speedup claim. Raw evidence in task blackboard and /tmp/030-pipeline-implementation.'
- subtask_ref: reuse-root-and-declaration-analysis
  title: Compute TypeScript root bindings once per parsed root and reuse them in exports and the shared visitor; compute JSDoc parameter/return facts once per declaration; stop scope classification once the language-specific collector dispatch is determined. Preserve nested-scope exclusions, annotation precedence, duplicate-tag rejection, CommonJS shadowing, DOM/Array facts, Unicode and escaped-property handling, including HTML/Vue islands. Verify public language seams and full cold-build improvement.
  status: completed
  evidence: 'Implemented exact-root binding reuse, declaration-local JSDoc reuse and JS/TS-aware classifier termination. Added public annotation/duplicate/async and CommonJS/DOM order cases. Shared integrated verification: 609 passed, strict Clippy and SOL6.1 PASS, Commerce domain parity; extraction 3.933→3.532 s and total 12.266→11.890 s for all three changes together. No isolated subtask speedup claim.'
- subtask_ref: reuse-linker-and-value-flow-indexes
  title: Reuse the linker node inventory and by-id catalog in ValueFlowIndex::build_parallel/build_initialized, index inheritance imports by file/scope/alias, and filter irrelevant receiver hints before owner lookup. Preserve distinct name-index semantics and complete declaration, parent, shadowing and invalid-scope catalogs. Preserve inference fixed points, source-position and ambiguity rules; use existing linker phase profiling to attribute the change and prove semantic parity plus full cold-build improvement.
  status: completed
  evidence: 'Implemented shared node inventory/by-ID lookup, scoped inheritance-import index and receiver-hint prefilter; retained distinct name-index semantics and module selection order. Shared integrated verification: 609 passed, strict Clippy and SOL6.1 PASS, Commerce domain parity. Combined cold build 12.266→11.890 s; isolated linker gain is not established (2.172→2.207 s in nearest pair).'
receipt:
  commit: f53793c3fd7c56cbb2750f51c6666a5f0bad571a
  contract_revision: 3
  passed_at: 2026-10-07T07:13:56.016496686+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 609
      tests_failed: 0
      log: 609 passed, 3 ignored; separate commitment_integrity 14 passed. Strict Clippy exit0. Integrated Commerce cold candidate11.890s vs nearest baseline12.266s; 38258 domain commitments equal. Public MCP 32 paired responses equal. Raw artifacts /tmp/030-pipeline-implementation; no individual-subtask speedup claim.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent SOL6.1 reviewed seven frozen production-file deltas and two added public extraction/linker tests. Only the three retained revision3 slices implemented; source remains identical to reviewed snapshot.
        claims:
          applicable: true
          evidence: 609 tests pass; strict alltarget/allfeature Clippy exit0; separate commitment_integrity14 pass. Integrated cold candidate11.890s vs baseline-repeat12.266s; first baseline14.014s records host variation. No isolated per-subtask/linker gain claim. All38258 domain commitments match.
        concurrency:
          applicable: true
          evidence: Shared linker catalogs immutable and build-scoped; exact-root extraction context local; existing parallel threshold/reduction and transaction/publication boundaries retained. Public cold/delta and deterministic commitment tests pass.
        project_isolation:
          applicable: true
          evidence: Same fixed Commerce root, independent disposable databases and retained binary hashes, no compiler overlap. Row counts and DBbytes equal; only expected source fingerprint and outputroot differ. All32 paired MCP semantic responses match and DB/bin hashes unchanged.
        administration:
          applicable: true
          evidence: User retained first3 subtasks and removedlast3; revision3 synchronizes exact3 in manifest and DB. All3 completed with shared integrated evidence, no individual speedup claim. Final independent SOL6.1 PASS condition strictClippy satisfied exit0. Raw measurements retained outside milestone.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REMOVAL-SAFETY: prove_removal evaluates only target-connected dependencies, not unrelated global errors.'
    - 'INV-SUB-30MS-SEARCH: Code map search utilizes SQLite FTS5 native BM25 ranking and preserves the measured 030 latency gate.'
    - 'INV-STORAGE-BUDGET: Database compaction maintains overall storage density <= 45 KiB per source file with Zstd compression.'
    - 'INV-COLD-BUILD-PHASE-ACCOUNTING: Report non-overlapping cold-build phases accurately; about 10 seconds on Commerce is an aspiration, not a hard delivery gate.'
    - 'INV-INTEGRATED-COLD-GAIN: Retain changes only when controlled full Commerce builds demonstrate an end-to-end improvement without cold or warm regression. About 10 seconds remains an aspiration; phase-level improvements alone do not complete a subtask.'
    - 'INV-COLD-PROOF: Compare retained release binaries built with the same toolchain, lockfile, features and settings on the fixed 3,901-file Commerce root and adapter. Use fresh processes and separate empty databases, no warmup and no OS page-cache flush, with compilation finished before timing. Record full process wall time, non-overlapping phases, peak RSS, database bytes and semantic parity. Start with one baseline/candidate pair; use one additional baseline run if host noise prevents attribution, within the repository profiling cap. Report raw samples rather than a percentile from a single pair; keep measurements and source identities in the blackboard.'
    - 'INV-SEMANTIC-PARITY: Preserve symbol identities, scope and source-position rules, Unicode syntax, ambiguity and fail-closed states, edge multiplicity and representative ordering, coverage, durable facts, canonical Merkle commitments, and cold-versus-delta equivalence.'
    - 'INV-FINGERPRINT-PARITY: Compare canonical semantic rows and applicable domain commitments across binaries. Language source edits change FORGE_LANGUAGE_PROFILE_DIGEST and index-semantics metadata, so explain those expected fingerprint and output-root differences. Require repeatable roots and cold/delta equality within each binary; allow no unexplained semantic difference.'
    - 'INV-BOUNDED-OWNERSHIP: Keep shared analysis and dictionaries immutable and scoped to their parsed root or build snapshot; preserve memory bounds, workspace isolation, source-change detection, atomic publication, and existing integrity checks.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

---

## Remaining work

Implement AST owner borrowing, repeated root and declaration analysis reuse, and shared linker node lookups. Validate the retained changes against the complete build and preserve warm-path behavior. Keep benchmark histories and diagnostic findings in the blackboard.
