---
id: m-documentation-test-graph-and-tool-ergonomics
title: Documentation knowledge graph, test discovery chains, and tool query ergonomics
doc_type: contract
status: active
depends_on:
- m-agent-search-resilience-and-ast-grep-ergonomics
owners:
- src/mcp/
- src/engine/languages/
- src/db/reader.rs
- src/db/reader/
- src/db/symbols.rs
- src/db/symbols/
- src/engine/docs.rs
- src/engine/ast/
- src/engine/linker/
- docs/reference/
- skills/contextunity-forge/
invariants:
- 'INV-EXACT-SEARCH-PRIORITY: When exact is false (default) and symbol search candidates contain exact name or qualname matches, the engine automatically filters out noisy BM25 candidates and returns only exact matches, eliminating surrounding token pollution for known symbol identifiers.'
- 'INV-ERGONOMIC-PREVIEW-BUDGET: Source previews for symbols up to 200 lines are returned completely in a single response without artificial pagination; preview line budgets are clamped between 1 and 500 lines.'
- 'INV-NATURAL-DOC-SEARCH: Documentation search automatically filters common English stop-words and weights section title matches 3x over body text via FTS5 BM25, eliminating empty results caused by rigid AND conjunction of natural-language query phrases.'
- 'INV-TEST-CHAIN-AND-ROLE-TRANSPARENCY: code_map_tests classifies test entities into explicit roles (test_case, test_helper, test_fixture) and returns verified call paths (call_path) from test cases through helpers to target symbols instead of flat scope_or_transitive markers.'
- 'INV-EXPLICIT-EDGE-EVIDENCE: Graph inspection (impact and explain) at depth 1 returns full edge objects specifying edge kind (calls vs possible_calls), direction, and exact source location, strictly distinguishing statically proven calls from heuristic name candidates.'
- 'INV-UNRESOLVED-TAXONOMY: Unresolved references are categorized into specific granular reasons (dynamic_callback, unresolved_receiver_type, framework_magic, external_unindexed) with call expression snippets rather than generic other groupings.'
- 'INV-SCOPE-COVERAGE-TRANSPARENCY: code_map_overview provides coverage diagnostics broken down by primary workspace and linked_workspaces components rather than only global aggregate counts.'
- 'INV-DOC-GRAPH-TRACEABILITY: Markdown relative links and wikilinks are extracted into bidirectional cites_doc/cited_by_doc edges; governing invariants (INV-* rules) from linked doc sections are automatically surfaced on inspect and impact.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-19T20:52:21+00:00
---

# Documentation knowledge graph, test discovery chains, and tool query ergonomics

## Outcome and purpose

Deliver high-precision code discovery, natural documentation retrieval, transparent test paths, and first-class documentation knowledge graph integration across Forge MCP tools:

1. **Exact-Match Priority & Natural Documentation Search:**
   - In `code_map_search`, implement winner-take-all filtering when exact name or qualname matches exist, eliminating noisy BM25 candidate lists for known identifiers while preserving partial search for exploratory queries.
   - In `search_docs`, eliminate brittle `AND` conjunction across natural language queries. Filter stop-words and apply 3x title boosting in FTS5 BM25 ranking so conceptual queries succeed reliably.

2. **Source Preview Limits & Ergonomic Chunking:**
   - Increase default unbroken symbol body previews to 150–200 lines, allowing standard functions to be read in a single response without wasteful token/turn overhead.
   - Clamp requested `max_body_lines` between 1 and 500 lines, triggering pagination only for massive procedures and god-functions.

3. **Test Discovery Call Paths, Role Classification & Untested Public Surface:**
   - In language extractors, distinguish `test_case` (actual test execution units) from `test_helper` (assertion and setup helpers) and `test_fixture` (`conftest.py`, `@pytest.fixture`).
   - In `code_map_tests`, return explicit call paths (`call_path: [test_id, helper_id, target_id]`) explaining indirect test relationships rather than generic `scope_or_transitive` markers.
   - In `code_map_analyze`, report `untested_public_surface`: public API symbols lacking direct or transitive call paths from verified `test_case` entities.

4. **Graph Edge Evidence, Git-Diff Impact & Scope Transparency:**
   - At depth 1, provide explicit `edge` descriptors (`calls` vs `possible_calls`, direction, file, line) in `code_map_impact` and `code_map_explain`.
   - Implement `changes: "staged" | "unstaged" | "head"` in `code_map_impact` to map modified Git diff ranges directly to AST symbols, impacted graph neighbors, and affected tests.
   - Refine the `unresolved` taxonomy into actionable categories (`dynamic_callback`, `unresolved_receiver_type`, `framework_magic`, `external_unindexed`) with code snippets.
   - Provide workspace-level coverage breakdown (`coverage_by_workspace`) in `code_map_overview`.

5. **First-Class Documentation Knowledge Graph & Invariant Traceability:**
   - Extract Markdown relative links and wikilinks into `cites_doc` / `cited_by_doc` edges.
   - Index `INV-*` invariants across all `docs/**/*.md` and `AGENTS.md` as first-class graph nodes with `governs` / `governed_by` edges to referenced symbols.
   - Automatically surface direct and 1-hop transitive `governing_invariants` on `code_map_inspect` and `code_map_impact`.
   - Automatically surface `affected_docs` in `code_map_impact` and `documented_in` in `code_map_inspect`.
   - Surface verified `referenced_symbols` in `get_doc`.
   - Detect documentation drift (`dead_doc_symbol`, `broken_doc_link`, `stale_doc_sections`) in `code_map_analyze` with `suggested_symbol` remediation hints.

---

## Tasks in this milestone

### task: search-precision-and-natural-docs-retrieval

```yaml
task_ref: search-precision-and-natural-docs-retrieval
target: Implement winner-take-all exact matching in symbol search and stop-word filtered title-boosted natural doc search
proof_policy: seam-test-first
contract_revision: 7
invariants:
- 'INV-EXACT-SEARCH-PRIORITY: When exact is false (default) and symbol search candidates contain exact name or qualname matches, the engine automatically filters out noisy BM25 candidates and returns only exact matches, eliminating surrounding token pollution for known symbol identifiers.'
- 'INV-NATURAL-DOC-SEARCH: Documentation search automatically filters common English stop-words and weights section title matches 3x over body text via FTS5 BM25, eliminating empty results caused by rigid AND conjunction of natural-language query phrases.'
scope:
- src/db/symbols/search.rs
- src/db/reader.rs
- src/mcp/tools.rs
- tests/query_context/search_and_paging.rs
- tests/query_context/ranking_and_coverage.rs
- tests/mcp/context.rs
- tests/mcp/query_ranking.rs
- docs/reference/mcp-tools.md
status: completed
subtasks:
- subtask_ref: winner-take-all-symbol-exact-match
  title: 'Filter search candidates to exact name or qualname matches when present in candidate pool without requiring exact: true'
  status: completed
  evidence: Revision7, accepted snapshot d9a7f2b04d4af7d8922c01e46e78e109bed753a0. Independent reviewer claim17 ran cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test query_context exit0,43passed0failed; cargo test --test mcp exit0,85passed0failed. All commands set CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/053-documentation-test-graph-and-tool-ergonomics/target; MCP additionally set CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/forge053-review17-skill-2ust354x/contextunity-forge (fresh10-file byte-identical copy of current repo skill; existing host drift not fixed). Default filtered exact isolation and bounded connectivity ranking verified in both SQL branches (graph window50,total probe1001). Permanent process and rankterm exact z-before-a assertions pass; real CLI process/PROCESS/__ returns exact2 in connected-first order. F053-SEARCH-CONNECTED-EXACT ledger499 confirmed-fixed.
- subtask_ref: natural-language-doc-search-query-pipeline
  title: Filter common stop-words and construct weighted FTS5 queries with 3x title boost instead of rigid AND token conjunction
  status: completed
  evidence: Revision7, accepted snapshot d9a7f2b04d4af7d8922c01e46e78e109bed753a0. Independent reviewer claim17 ran cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test query_context exit0,43passed0failed; cargo test --test mcp exit0,85passed0failed. All commands set CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/053-documentation-test-graph-and-tool-ergonomics/target; MCP additionally set CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/forge053-review17-skill-2ust354x/contextunity-forge (fresh10-file byte-identical copy of current repo skill; existing host drift not fixed). Existing natural docs public SQLite seam continues to prove English stopword removal, OR retrieval and 3:1:1 title/content/invariants BM25; original MCP/reference documentation remains aligned.
- subtask_ref: search-precision-seam-tests
  title: Add unit and integration tests proving exact match isolation and natural documentation query retrieval
  status: completed
  evidence: Revision7, accepted snapshot d9a7f2b04d4af7d8922c01e46e78e109bed753a0. Independent reviewer claim17 ran cargo clippy --all-targets --all-features -- -D warnings exit0; cargo test --test query_context exit0,43passed0failed; cargo test --test mcp exit0,85passed0failed. All commands set CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/053-documentation-test-graph-and-tool-ergonomics/target; MCP additionally set CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/forge053-review17-skill-2ust354x/contextunity-forge (fresh10-file byte-identical copy of current repo skill; existing host drift not fixed). Revision7 contract claim15 authentic graph-order RED exit101 retained; new permanent oracles preserve exact2connected order, process_* prefix connectivity,503 wildcard membership/unique/paging/tail, and stdio grouping target1/target*2. Production wildcard scoring unchanged. Earlier F053-SEARCH-DOCS remains confirmed-fixed.
receipt:
  contract_revision: 7
  passed_at: 2026-10-20T00:12:55.769280115+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context
      exit_code: 0
      tests_passed: 43
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed frozen revision7 candidate against baseline8ab09e1713bfa22105110cb4c6a2bf4ed417276a and verified all eight scoped files exactly match accepted build snapshotd9a7f2b04d4af7d8922c01e46e78e109bed753a0. Tracked diff SHA256875be96ef852d96e53cdd40385d4efd44d976a3d0d7011ec1665e0c8ed0edde4. Implementation changes only src/db/symbols/search.rs and admitted existing tests/mcp/context.rs plus tests/mcp/query_ranking.rs. Manifest162-line diff is externally admitted revision6 planning plus accepted revision7 scope admission, preserved as current authority; no reviewer manifest edits. No crate/linker/language, skill, profile/configuration/credential, generated or neighboring-project source changes. git diff --check exit0.
        claims:
          applicable: true
          evidence: 'Cumulative SEARCH authority INV-EXACT-SEARCH-PRIORITY and INV-NATURAL-DOC-SEARCH verified. F053-SEARCH-CONNECTED-EXACT ledger499 is confirmed-fixed: both default-exact SQL branches now filter eligible exact name/qualname matches before pagination, probe at most1001 results for totals, rank graph connectivity within a deterministic50-candidate graph window, and retain stable path/line/id tie-breaking. Authentic contract15 RED exit101 showed a.py before connected z.py after corrected total2; current permanent process and dense rankterm exact2 connected-order assertions pass. Real compiled CLI process/PROCESS/__ fixtures return only2 exacts, all z.py-before-a.py, covering FTS and punctuation-only branches. Reverting to path-only ordering is the observed red mutation; removing isolation fails total/name assertions; graph/total bounds remain explicit50/1001 in both queries; pagination bypass fails retained503 unique-result continuity/tail assertions. Revised wildcard rankterm* checks503 total/membership/unique IDs/pages/tail; default exact ranking independently remains at test103-109 and process_* connected-prefix assertion remains. Removing stale wildcard positional exact oracles is a scope-preserving correction; wildcard production scoring unchanged. Stdio grouping covers target=>1 and target*=>2. Natural docs stopwords/OR/3:1:1 BM25 and F053-SEARCH-DOCS correction unchanged and reverified by full target. During review17 independently ran sequential cargo clippy --all-targets --all-features -- -D warnings exit0 (archivefc0d59f630eef6bd), cargo test --test query_context exit0,43passed0failed0ignored0filtered (archive0ea2619070d0f17f), cargo test --test mcp exit0,85passed0failed0ignored0filtered (archivea5f8fa401d5a7b69). All used CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/053-documentation-test-graph-and-tool-ergonomics/target. MCP additionally used CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/forge053-review17-skill-2ust354x/contextunity-forge, a fresh actual10-file byte-identical copy of current repo skill. Existing default-global-skill drift84passed1failed was separately reported by builder/coordinator; this review does not claim that host installation fixed. No unresolved required findings.'
        concurrency:
          applicable: true
          evidence: Ranking repair changes query-local CTEs only; no transaction, snapshot lock, receipt, task-state, writer, revision-fencing or connection ownership code changed. Exact probes and returned items use identical filters; deterministic graph window is independent of requested page offset, preventing page-boundary rank drift. Existing generation validation remains in ranked_search_page/paging and full MCP/query_context continuation/freshness/isolation tests pass. Independent reviewer17 differs from builder16, contract7 and baseline8ab09e1 still bind the same frozen candidate. No source edits occurred during review.
        project_isolation:
          applicable: true
          evidence: All lifecycle/discovery commands explicitly target milestone053 root and task forge-mcp/m-documentation-test-graph-and-tool-ergonomics:search-precision-and-natural-docs-retrieval. All Cargo calls explicitly use its absolute target directory. External049 binary is an operational tool only; no049 source moved. Live Forge AST predicates confirmed shared CLI src/cli/mod.rs:432 and MCP src/mcp/tools.rs:290 search adapters; current query-tests mapping reaches real reader::open and SearchOptions, matching current164-line production-seam test. Existing workspace-relative path rejection, kind/path/include_docs predicates, bound SQL parameters, and single-workspace locked reader remain unchanged. Temporary real CLI fixtures and copied skill tree live only under/tmp, no host skill installation edited.
        administration:
          applicable: true
          evidence: Reviewed against current accepted revision7 authority, without restoring old receipts/runtime or externally authored planning. No schema, migration, administrative engine, profile pin, credential, protected configuration or archive edits. Strict linter and both complete affected targets passed under explicit attributable environment. Three retained completed subtasks reassessed and updated via Forge with current revision7 evidence; no source or receipt/status file edits. Existing automatic-delivery baseline/hook/candidate/typed-receipt enforcement remains unchanged. Cumulative earlier docs finding and connected-exact regression each have attributable closure evidence; no scope expansion, manual commits, benchmarks or publication actions.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-EXACT-SEARCH-PRIORITY: When exact is false (default) and symbol search candidates contain exact name or qualname matches, the engine automatically filters out noisy BM25 candidates and returns only exact matches, eliminating surrounding token pollution for known symbol identifiers.'
    - 'INV-ERGONOMIC-PREVIEW-BUDGET: Source previews for symbols up to 200 lines are returned completely in a single response without artificial pagination; preview line budgets are clamped between 1 and 500 lines.'
    - 'INV-NATURAL-DOC-SEARCH: Documentation search automatically filters common English stop-words and weights section title matches 3x over body text via FTS5 BM25, eliminating empty results caused by rigid AND conjunction of natural-language query phrases.'
    - 'INV-TEST-CHAIN-AND-ROLE-TRANSPARENCY: code_map_tests classifies test entities into explicit roles (test_case, test_helper, test_fixture) and returns verified call paths (call_path) from test cases through helpers to target symbols instead of flat scope_or_transitive markers.'
    - 'INV-EXPLICIT-EDGE-EVIDENCE: Graph inspection (impact and explain) at depth 1 returns full edge objects specifying edge kind (calls vs possible_calls), direction, and exact source location, strictly distinguishing statically proven calls from heuristic name candidates.'
    - 'INV-UNRESOLVED-TAXONOMY: Unresolved references are categorized into specific granular reasons (dynamic_callback, unresolved_receiver_type, framework_magic, external_unindexed) with call expression snippets rather than generic other groupings.'
    - 'INV-SCOPE-COVERAGE-TRANSPARENCY: code_map_overview provides coverage diagnostics broken down by primary workspace and linked_workspaces components rather than only global aggregate counts.'
    - 'INV-DOC-GRAPH-TRACEABILITY: Markdown relative links and wikilinks are extracted into bidirectional cites_doc/cited_by_doc edges; governing invariants (INV-* rules) from linked doc sections are automatically surfaced on inspect and impact.'
    - 'INV-EXACT-SEARCH-PRIORITY: When exact is false (default) and symbol search candidates contain exact name or qualname matches, the engine automatically filters out noisy BM25 candidates and returns only exact matches, eliminating surrounding token pollution for known symbol identifiers.'
    - 'INV-NATURAL-DOC-SEARCH: Documentation search automatically filters common English stop-words and weights section title matches 3x over body text via FTS5 BM25, eliminating empty results caused by rigid AND conjunction of natural-language query phrases.'
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

### task: source-preview-budget-and-chunking-limits

```yaml
task_ref: source-preview-budget-and-chunking-limits
target: Expand default unbroken function preview limits to 200 lines and clamp max_body_lines up to 500 lines
proof_policy: seam-test-first
contract_revision: 8
invariants:
- 'INV-ERGONOMIC-PREVIEW-BUDGET: Source previews for symbols up to 200 lines are returned completely in a single response without artificial pagination; preview line budgets are clamped between 1 and 500 lines.'
scope:
- src/core/response.rs
- src/db/symbols.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- tests/query_context/diagnostics_and_source.rs
- tests/query_context/ranking_and_coverage.rs
- tests/mcp/context.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- docs/reference/mcp-tools.md
- skills/contextunity-forge/SKILL.md
status: completed
subtasks:
- subtask_ref: raise-default-source-preview-limit
  title: Increase default max_body_lines from 35 to 200 lines in response options and tool schemas
  status: completed
  evidence: 'Revision 8 snapshot 5fa29485bde4d8476836cbc5b1a2f5e1a9144b53: core SourceContextPolicy default=200; MCP schema/tool descriptions, reference and repository skill co-evolve. Independent review: cargo test --test query_context exit 0, 43 passed/0 failed (archive 72a6b4db0f0449d9); cargo test --lib exit 0, 85/0 (af9557d5b7761bb2).'
- subtask_ref: expand-clamped-preview-ceiling
  title: Expand allowed max_body_lines range from 1..=100 to 1..=500 lines in QueryOptions validation
  status: completed
  evidence: SourceOptions::resolve clamps requested 0/1/200/500/501/usize::MAX to 1..=500; configured policy validates 1..=500, overflow and leading <=20 checks remain. DB with_source accepts bounded 500; MCP all three tools prove high/low clamp. Independent cargo test --lib 85/0; cargo test --test mcp 85/0 (b4aef54f3cf37042), CARGO_TARGET_DIR=milestone/target and actual copied skill override /tmp/forge053-preview-review15-skill-omb9960n/contextunity-forge.
- subtask_ref: preview-budget-seam-tests
  title: Verify complete single-page retrieval of 100-200 line functions without pagination tokens
  status: completed
  evidence: 'Real indexed SQLite inspect/explain seam proves 116/200 complete defaults, 201=200+1 and explicit 500, CRLF/UTF-8 reconstruction, no neighboring symbol leakage, explicit 35 continuation, generation rejection and byte bounds. Independent complete query_context 43/0, lib 85/0, MCP 85/0; strict Clippy exit 0 archive 58d67f74c047a8dd. MCP snippet proves 500+11 continuation; preexisting inspect/explain metadata-generation gap deferred in blackboard #506 to mcp-markdown-output-and-dead-metadata-compaction, no all-three continuation claim.'
receipt:
  contract_revision: 8
  passed_at: 2026-10-20T00:34:31.604188156+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context
      exit_code: 0
      tests_passed: 43
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: exactly 10 revision-8 admitted implementation/proof/documentation paths plus authorized manifest contract_revision 7->8 and repository-skill scope admission. Every scoped blob equals accepted BUILD14 snapshot 5fa29485bde4d8476836cbc5b1a2f5e1a9144b53; scoped binary diff SHA256 0a55685bd23452eb0aea7b64207515e8333bfbcd4a17bf95140a26ea987ca309. No untracked/staged neighboring files, profile/config/schema/crate/linker/metadata edits. git diff --check exit 0. Documentation and skill prose match shared runtime; links unchanged and MCP skill link checks passed.'
        claims:
          applicable: true
          evidence: 'PASS INV-ERGONOMIC-PREVIEW-BUDGET: src/core/response.rs:35-40 defaults to 200; :84-87 strictly validates configured 1..=500; :167-175 clamps requests 0/1/200/500/501/usize::MAX to 1..=500 and preserves offset overflow rejection. src/db/symbols.rs:676-687 validates 500 ceiling and retains verified-source/digest/generation checks and 8192 escaped-byte bound; unchanged source_preview and 64KiB envelope/search 3-hit/20-line/12KiB caps. CLI cli_source and MCP Inspect/Snippet/Explain share SourceOptions. Real indexed SQLite test diagnostics_and_source.rs:136-335 proves 116/200 default completeness, 201=200+1, explicit 500, UTF8/CRLF reconstruction, neighbor exclusion, explicit35 continuation, exhausted/missing/stale generation. MCP mcp_boundaries.rs:72-110 proves 0/501 clamping all3 tools and snippet500+11 continuation. No old behavioral oracle removed: earlier default35-dependent continuation calls now explicitly request35. Independent sequential commands after last source/test edit, cwd assigned preview worktree, CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/053-documentation-test-graph-and-tool-ergonomics/target: cargo clippy --all-targets --all-features -- -D warnings exit0 (archive58d67f74c047a8dd); cargo test --test query_context exit0 43passed/0failed (72a6b4db0f0449d9); cargo test --lib exit0 85/0 (af9557d5b7761bb2); cargo test --test mcp exit0 85/0 (b4aef54f3cf37042). Initial query runner timeout124 at1000ms (ab2c52517f2a1aff) provided no test result; after confirming no remaining process, complete rerun passed. MCP used CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/forge053-preview-review15-skill-omb9960n/contextunity-forge, actual currentbranch copy10files byte-identical; host global installation drift remains separate, no host-sync claim. Existing unchanged src/mcp/metadata.rs:142-164 generation removal is linked-deferred in blackboard506 to mcp-markdown-output-and-dead-metadata-compaction; no all3 MCP continuation claim. Historical authentic RED archives b37dcbdea787fea0/ac622ba6a3744e6e retained; no fabricated RED. Forge graph ff19eb5df8feb06c2b948779db4476f2d862acf6bebe01c634db9a08d5ccb2c0 shows direct test->SourceOptions/inspect_with_options/explain_with_options witnesses.'
        concurrency:
          applicable: true
          evidence: 'PASS: reviewer codex-053-preview-reviewer differs from accepted builder codex-053-preview-builder; active review claim15 is bound to contract8, accepted build14 and HEAD a4991a9513075f286266a092074d8d481ea78289. No task engine, transaction, receipt, lock, database publication or rollback code changed. Cargo commands ran sequentially against explicit shared target with builder finished. Candidate scoped bytes rechecked after all verification and before submission. Existing generation validation remains fail-closed and full MCP freshness/concurrent rebuild/read fence tests passed. Source freeze remains in force through actual Forge delivery.'
        project_isolation:
          applicable: true
          evidence: 'PASS: all CLI task/graph operations use fixed operational binary with explicit --root assigned preview worktree; graph metadata workspace_root matches. Full task/project identifier and worktree bound in proof. No 049 implementation sources moved. verified_source still resolves adapter-authorized file paths, regular files, maximum file bytes and indexed digest; selector/path/symlink/traversal and linked-workspace MCP tests passed. Temporary isolated copied skill resides under /tmp; no external agent installation or linked project changed.'
        administration:
          applicable: true
          evidence: 'PASS: no schema migration, protected profile/config/hash, gate implementation, Git hook, archived receipt or credential changes. Manifest delta is accepted revision8 skill-scope coevolution only; operational subtasks now completed through task subtask update with current independent evidence, not manual receipt/status edits. Accepted contract13 and build14 bound to baseline a4991a9513075f286266a092074d8d481ea78289 and candidate5fa29485bde4d8476836cbc5b1a2f5e1a9144b53. Deliver will use same independent reviewer session and auto_commit through Forge; no manual commit. Deferred metadata owner durably recorded #506 before delivery. Closure/contract-planning learning found no additional independent local rule requiring a source or guidance edit.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-EXACT-SEARCH-PRIORITY: When exact is false (default) and symbol search candidates contain exact name or qualname matches, the engine automatically filters out noisy BM25 candidates and returns only exact matches, eliminating surrounding token pollution for known symbol identifiers.'
    - 'INV-ERGONOMIC-PREVIEW-BUDGET: Source previews for symbols up to 200 lines are returned completely in a single response without artificial pagination; preview line budgets are clamped between 1 and 500 lines.'
    - 'INV-NATURAL-DOC-SEARCH: Documentation search automatically filters common English stop-words and weights section title matches 3x over body text via FTS5 BM25, eliminating empty results caused by rigid AND conjunction of natural-language query phrases.'
    - 'INV-TEST-CHAIN-AND-ROLE-TRANSPARENCY: code_map_tests classifies test entities into explicit roles (test_case, test_helper, test_fixture) and returns verified call paths (call_path) from test cases through helpers to target symbols instead of flat scope_or_transitive markers.'
    - 'INV-EXPLICIT-EDGE-EVIDENCE: Graph inspection (impact and explain) at depth 1 returns full edge objects specifying edge kind (calls vs possible_calls), direction, and exact source location, strictly distinguishing statically proven calls from heuristic name candidates.'
    - 'INV-UNRESOLVED-TAXONOMY: Unresolved references are categorized into specific granular reasons (dynamic_callback, unresolved_receiver_type, framework_magic, external_unindexed) with call expression snippets rather than generic other groupings.'
    - 'INV-SCOPE-COVERAGE-TRANSPARENCY: code_map_overview provides coverage diagnostics broken down by primary workspace and linked_workspaces components rather than only global aggregate counts.'
    - 'INV-DOC-GRAPH-TRACEABILITY: Markdown relative links and wikilinks are extracted into bidirectional cites_doc/cited_by_doc edges; governing invariants (INV-* rules) from linked doc sections are automatically surfaced on inspect and impact.'
    - 'INV-ERGONOMIC-PREVIEW-BUDGET: Source previews for symbols up to 200 lines are returned completely in a single response without artificial pagination; preview line budgets are clamped between 1 and 500 lines.'
    architectural_notes:
    - 'deferred: {"finding":"Bounded linked continuation-token gap found during MCP boundary verification: code_map_inspect/code_map_explain can return source_preview.next_source_offset while MCP metadata compaction removes the root generation when a nested paging envelope carries the same generation, so continuation is rejected with `source continuation requires generation from the previous preview`.","owner":"src/mcp/metadata.rs (existing generation compaction at lines 142-164); unchanged by this task.","linked_future_owner":"mcp-markdown-output-and-dead-metadata-compaction","scope_boundary":"No MCP metadata fix in source-preview task. Keep 500-line clamp/low-clamp assertions across all three tools; prove 500+11 line continuation through get_code_snippet, whose response retains generation."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: test-discovery-chains-and-role-classification

```yaml
task_ref: test-discovery-chains-and-role-classification
target: "Classify test entities into test_case, test_helper, and test_fixture roles, trace explicit call paths in code_map_tests, and report untested public surface"
proof_policy: seam-test-first
contract_revision: 6
invariants:
  - 'INV-TEST-CHAIN-AND-ROLE-TRANSPARENCY: code_map_tests classifies test entities into explicit roles (test_case, test_helper, test_fixture) and returns verified call paths (call_path) from test cases through helpers to target symbols instead of flat scope_or_transitive markers.'
  - 'INV-UNTESTED-PUBLIC-SURFACE: code_map_analyze identifies and reports public symbols that have zero call paths originating from verified test_case nodes, distinguishing untested public surface from internal implementation details.'
scope:
  - src/engine/ast/
  - src/db/symbols/test_discovery.rs
  - src/db/reader.rs
  - src/mcp/tools.rs
  - tests/query_context/ranking_and_coverage.rs
  - docs/reference/mcp-tools.md
status: ready
subtasks:
  - subtask_ref: test-role-ast-extraction
    title: "Extract granular test roles (test_case, test_helper, test_fixture) across Python, Rust, and TypeScript profiles"
    status: pending
  - subtask_ref: test-call-path-traversal
    title: "Trace explicit call chains from test cases through intermediate helpers to target symbols in test_discovery"
    status: pending
  - subtask_ref: untested-public-surface-diagnostic
    title: "Diagnose public API symbols lacking direct or transitive test_case call paths in code_map_analyze"
    status: pending
  - subtask_ref: test-chains-seam-tests
    title: "Add tests verifying test role attributes, call_path arrays, and untested public surface reports"
    status: pending
```

### task: graph-edge-evidence-unresolved-taxonomy-and-scope-transparency

```yaml
task_ref: graph-edge-evidence-unresolved-taxonomy-and-scope-transparency
target: "Provide explicit edge descriptors in impact/explain, Git-diff impact analysis, granular unresolved taxonomy, and workspace coverage breakdown"
proof_policy: seam-test-first
contract_revision: 6
invariants:
  - 'INV-EXPLICIT-EDGE-EVIDENCE: Graph inspection (impact and explain) at depth 1 returns full edge objects specifying edge kind (calls vs possible_calls), direction, and exact source location, strictly distinguishing statically proven calls from heuristic name candidates.'
  - 'INV-DIFF-IMPACT-ANALYSIS: code_map_impact supports a changes parameter (staged, unstaged, head) to map modified Git diff ranges directly to AST symbols, impacted graph neighbors, and affected tests.'
  - 'INV-UNRESOLVED-TAXONOMY: Unresolved references are categorized into specific granular reasons (dynamic_callback, unresolved_receiver_type, framework_magic, external_unindexed) with call expression snippets rather than generic other groupings.'
  - 'INV-SCOPE-COVERAGE-TRANSPARENCY: code_map_overview provides coverage diagnostics broken down by primary workspace and linked_workspaces components rather than only global aggregate counts.'
scope:
  - src/db/reader.rs
  - src/db/reader/impact.rs
  - src/db/reader/explain.rs
  - src/db/reader/coverage.rs
  - src/mcp/tools.rs
  - tests/query_context/symbol_context.rs
  - tests/query_context/ranking_and_coverage.rs
  - docs/reference/mcp-tools.md
status: ready
subtasks:
  - subtask_ref: explicit-edge-descriptors-in-impact-and-explain
    title: "Include edge kind, direction, and source location on neighbor nodes at depth 1 in traversal queries"
    status: pending
  - subtask_ref: git-diff-impact-traversal
    title: "Implement changes filter in code_map_impact mapping Git diff line ranges to impacted AST symbols and covering tests"
    status: pending
  - subtask_ref: granular-unresolved-reference-taxonomy
    title: "Categorize unresolved references into dynamic callbacks, unknown receiver types, and framework magic with source snippets"
    status: pending
  - subtask_ref: per-workspace-coverage-breakdown
    title: "Report resolved, ambiguous, and unresolved counts segmented by workspace component in code_map_overview"
    status: pending
```

### task: documentation-knowledge-graph-and-invariant-traceability

```yaml
task_ref: documentation-knowledge-graph-and-invariant-traceability
target: "Extract documentation citation graph, surface affected docs on impact and documented_in on inspect, index governing invariants from all docs as first-class graph entities, and diagnose doc drift with suggested symbol remediation"
proof_policy: seam-test-first
contract_revision: 7
invariants:
  - 'INV-DOC-GRAPH-TRACEABILITY: Markdown relative links and wikilinks are extracted into bidirectional cites_doc/cited_by_doc edges; governing invariants (INV-* rules) from linked doc sections are automatically surfaced on inspect and impact.'
  - 'INV-UNIVERSAL-INVARIANT-INDEX: INV-* invariants across all docs/**/*.md and AGENTS.md are indexed as first-class graph nodes with bidirectional governs/governed_by edges to referenced symbols, replacing single-directory ADR token scraping.'
  - 'INV-IMPACT-AWARE-INVARIANTS: code_map_inspect and code_map_impact surface direct and 1-hop transitive governing invariants for inspected and impacted code symbols.'
  - 'INV-AFFECTED-DOCS-IMPACT: Symbol modification or deletion in code_map_impact surfaces affected_docs pointing to citing Markdown documentation sections and ADRs.'
  - 'INV-BIDIRECTIONAL-DOC-CODE-NAVIGATION: code_map_inspect surfaces documented_in sections for code symbols, and get_doc surfaces referenced_symbols with live symbol verification.'
  - 'INV-DOC-DRIFT-REMEDIATION-SUGGESTIONS: dead_doc_symbol diagnostics in code_map_analyze provide suggested_symbol candidates based on symbol similarity and module proximity.'
  - 'INV-COMPACT-INVARIANT-PAYLOAD: Governing invariants in tool responses and task context are bounded to compact ID, single-line rule, and source reference; transitive invariants are summarized by ID with a strict cap of 3-5 direct invariants to prevent context-window bloat.'
scope:
  - src/engine/docs.rs
  - src/engine/linker/resolution.rs
  - src/db/reader.rs
  - src/db/traversal.rs
  - src/mcp/tools.rs
  - tests/mcp/context.rs
  - docs/reference/mcp-tools.md
status: ready
subtasks:
  - subtask_ref: markdown-citation-graph-extraction
    title: "Extract Markdown relative links and wikilinks into cites_doc and cited_by_doc bidirectional edges"
    status: pending
  - subtask_ref: universal-invariants-indexing-and-traversal
    title: "Index INV-* invariants across all docs and AGENTS.md with governs/governed_by edges to symbols"
    status: pending
  - subtask_ref: affected-docs-and-bidirectional-navigation
    title: "Surface affected_docs in impact, documented_in in inspect, and referenced_symbols in get_doc"
    status: pending
  - subtask_ref: governing-invariants-injection
    title: "Surface direct and 1-hop transitive INV-* invariants on code_map_inspect and code_map_impact"
    status: pending
  - subtask_ref: doc-drift-diagnostics-and-suggestions
    title: "Report dead_doc_symbol, broken_doc_link, and stale_doc_sections in code_map_analyze with suggested_symbol candidates"
    status: pending
```


### task: mcp-markdown-output-and-dead-metadata-compaction

```yaml
task_ref: mcp-markdown-output-and-dead-metadata-compaction
target: "Deliver structured Markdown-first MCP responses, eliminate dead metadata hashes and zero-noise wrappers, and retain complete actionable workflow guidance and invariants"
proof_policy: seam-test-first
contract_revision: 1
invariants:
  - 'INV-MARKDOWN-FIRST-MCP-OUTPUT: MCP tool responses across code_map, task, and doc queries default to structured GitHub-flavored Markdown for LLM consumption, eliminating JSON syntax overhead while retaining --format json on CLI and programmatic endpoints.'
  - 'INV-DEAD-METADATA-ELIMINATION: Internal engine state (raw 64-char generation hashes, empty pagination envelopes, zero-count collections) is omitted from agent responses unless active pagination continuation is required.'
  - 'INV-PRESERVED-CONTRACT-GUIDANCE: Task lifecycle contracts (task context, task claim, task inspect) strictly retain all actionable workflow guidance, stage rules, applicable invariants, and subtask DoD, while stripping redundant snapshot duplicate dumps.'
  - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Static boilerplate instructions are replaced with concise, contextual one-line hints (continuation offsets, diagnostic references, and command remediation).'
scope:
  - src/mcp/response.rs
  - src/mcp/metadata.rs
  - src/mcp/tools.rs
  - src/engine/tasks/context.rs
  - src/core/response.rs
  - tests/mcp/context.rs
  - docs/reference/mcp-tools.md
status: ready
subtasks:
  - subtask_ref: dead-metadata-and-generation-hash-stripping
    title: "Omit generation hashes and empty collections from responses when has_more is false"
    status: pending
  - subtask_ref: code-map-markdown-formatter
    title: "Implement structured Markdown formatters for code_map_inspect, search, impact, and overview"
    status: pending
  - subtask_ref: task-context-compact-markdown-formatter
    title: "Format task context, claim, and inspect as structured Markdown retaining all rules, guidance, and DoD"
    status: pending
  - subtask_ref: blackboard-and-docs-markdown-formatter
    title: "Format blackboard messages and doc search results as concise Markdown entries"
    status: pending
  - subtask_ref: format-contract-seam-tests
    title: "Add unit and integration tests verifying Markdown fidelity, token density, and contract guidance preservation"
    status: pending
```
