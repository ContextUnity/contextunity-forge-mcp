---
id: m-documentation-test-graph-and-tool-ergonomics
title: Documentation knowledge graph, test discovery chains, and tool query ergonomics
doc_type: contract
status: completed
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
handoff:
  completed_at: 2026-10-20T21:39:10.237437243+00:00
  duration: 24h 46m
  commit: cbbddfeb7c64a336db0c249dfa388b8115049ce1
  target_base: 2c06defd57c80503ad18bd73137a4f3ef084f9b0
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-documentation-test-graph-and-tool-ergonomics
      authority_digest: 212c10043c09da68c38b70dad65441314bffc05d2ee1612415f60428778c1c93
      implementation_digest: 51016989c0b203a6470e7896acf44c829b2be99554e40ef3295b1b69c3ee756d
      target_base_digest: c9d3ba212bbb8dcb8158a6b919f1aa1cdde584cffa1d914621cd88213d883449
      builders:
      - codex-053-handoff-builder
      - codex-053-preview-builder
      - codex-053-search-builder
      - codex-053-test-discovery-builder
    target_base: 2c06defd57c80503ad18bd73137a4f3ef084f9b0
    review:
      worker_id: codex-053-final-reviewer
      decision: pass
      summary: 'Independent combined integration review of final cbbddfeb7c64a336db0c249dfa388b8115049ce1 against main2c06defd57c80503ad18bd73137a4f3ef084f9b0: all61 changed paths are admitted by six completed tasks. Exact search/ranking, preview bounds, verified test-role/call-path discovery, Git/edge/coverage evidence, unique verified documentation and candidate navigation/governance, and cancelled-history handoff remain contract-aligned. Fresh current-CLI context validates all immutable accepted proofs and matching cancelled Task6 without completion credit; main049 crate fixes and other packaged skills are preserved. Final repack tree and canonical bindings are identical to original automatic delivery; all tested source/test/docs/config inputs remain unchanged apart from renderer-owned milestone metadata. Independent full compound811/0 and strictClippy0 therefore apply to this exact binding. No unresolved scope or contract findings; packaging/docs scan and benchmark source-only provenance boundaries retained.'
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 811
      tests_failed: 0
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 811
    tests_failed: 0
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
target: Classify test entities into test_case, test_helper, and test_fixture roles, trace explicit call paths in code_map_tests, and report untested public surface
proof_policy: seam-test-first
contract_revision: 9
invariants:
- 'INV-TEST-CHAIN-AND-ROLE-TRANSPARENCY: code_map_tests classifies test entities into explicit roles (test_case, test_helper, test_fixture) and returns verified call paths (call_path) from test cases through helpers to target symbols instead of flat scope_or_transitive markers.'
- 'INV-UNTESTED-PUBLIC-SURFACE: code_map_analyze identifies and reports public symbols that have zero call paths originating from verified test_case nodes, distinguishing untested public surface from internal implementation details.'
scope:
- src/engine/ast/
- src/engine/languages/mod.rs
- src/engine/languages/python.rs
- src/engine/languages/rust.rs
- src/engine/languages/typescript/mod.rs
- src/db/symbols.rs
- src/db/symbols/test_discovery.rs
- src/db/reader.rs
- src/db/traversal.rs
- src/mcp/tools.rs
- tests/query_context/ranking_and_coverage.rs
- tests/query_context/search_and_paging.rs
- tests/languages/profiles/legacy_hooks.rs
- tests/languages/profiles.rs
- tests/languages/fixtures/three_tier/pre_refactor.json
- docs/reference/mcp-tools.md
- docs/reference/mcp-markdown-contracts.md
- skills/contextunity-forge/SKILL.md
status: completed
subtasks:
- subtask_ref: test-role-ast-extraction
  title: Extract granular test roles (test_case, test_helper, test_fixture) across Python, Rust, and TypeScript profiles
  status: ready
  evidence: null
- subtask_ref: test-call-path-traversal
  title: Trace explicit call chains from test cases through intermediate helpers to target symbols in test_discovery
  status: ready
  evidence: null
- subtask_ref: test-chains-seam-tests
  title: Add tests verifying test role attributes, call_path arrays, and untested public surface reports
  status: ready
  evidence: null
- subtask_ref: untested-public-surface-diagnostic
  title: Diagnose public API symbols lacking direct or transitive test_case call paths in code_map_analyze
  status: ready
  evidence: null
receipt:
  contract_revision: 9
  passed_at: 2026-10-20T02:37:57.591509339+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context
      log: CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/053-documentation-test-graph-and-tool-ergonomics/target; strict Clippy exit 0; query_context 43 passed, 0 failed; additionally languages 262/0, lib 85/0, MCP 85/0 with isolated current skill copy
      exit_code: 0
      tests_passed: 43
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: acceptedrev9 admitted15 implementation blobs equal candidate; latest repair exactly four admitted owner/test paths. External authority manifest/futureTask4/5 scopes preserved. Generated proof relocated out of worktree before pass; no protected config/source edits by reviewer.'
        claims:
          applicable: true
          evidence: 'PASS: both invariants and existing acceptance proof reviewed cumulatively; all3 findingIDs confirmed-fixed. Actual module-declaration identity and actual exported class-body member ownership independently verified in source, original/block current CLI fixtures, and permanent class/member SQLite/analyze seam. Direct/named/default export positives, internal/nested/block/private negatives, JSunknown/PythonRust and genuine/shadowed runner roles preserved. Exact shortest calls-only cycle-safe paths and complete fail-closed absence/paging proof unchanged from prior reviewed source. Current strictClippy0/query43/languages262/lib85/MCP85 passed; golden/fingerprint proof retained where unchanged. No optional authority expansion.'
        concurrency:
          applicable: true
          evidence: 'PASS for changed mechanisms: no task transactions, revision/claim fencing, receipt publication, rollback or lock changes. Independent worker distinct from builder, frozen candidate checked, sequential Cargo, all jobs completed.'
        project_isolation:
          applicable: true
          evidence: 'PASS: explicit active worktree root for operations, separately rooted authorized /tmp actual fixtures/copied skill. No049 implementation transfer, host skill mutation, linked workspace profile/config resolution or project namespace changes.'
        administration:
          applicable: true
          evidence: 'PASS for changed mechanisms: no schema/migration/profile/gate/hook/receipt changes, no manual commit. Bound contract9/claim18/snapshota7a7282/baselineb18. Actual Forge candidate validation governs review/delivery. No Markdown/ADR/Task6 or crate/linker work.'
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
    - 'INV-TEST-CHAIN-AND-ROLE-TRANSPARENCY: code_map_tests classifies test entities into explicit roles (test_case, test_helper, test_fixture) and returns verified call paths (call_path) from test cases through helpers to target symbols instead of flat scope_or_transitive markers.'
    - 'INV-UNTESTED-PUBLIC-SURFACE: code_map_analyze identifies and reports public symbols that have zero call paths originating from verified test_case nodes, distinguishing untested public surface from internal implementation details.'
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

### task: graph-edge-evidence-unresolved-taxonomy-and-scope-transparency

```yaml
task_ref: graph-edge-evidence-unresolved-taxonomy-and-scope-transparency
target: Provide explicit edge descriptors in impact/explain, Git-diff impact analysis, granular unresolved taxonomy, and workspace coverage breakdown
proof_policy: seam-test-first
contract_revision: 9
invariants:
- 'INV-EXPLICIT-EDGE-EVIDENCE: Graph inspection (impact and explain) at depth 1 returns full edge objects specifying edge kind (calls vs possible_calls), direction, and exact source location, strictly distinguishing statically proven calls from heuristic name candidates.'
- 'INV-DIFF-IMPACT-ANALYSIS: code_map_impact supports a changes parameter (staged, unstaged, head) to map modified Git diff ranges directly to AST symbols, impacted graph neighbors, and affected tests.'
- 'INV-UNRESOLVED-TAXONOMY: Unresolved references are categorized into specific granular reasons (dynamic_callback, unresolved_receiver_type, framework_magic, external_unindexed) with call expression snippets rather than generic other groupings.'
- 'INV-SCOPE-COVERAGE-TRANSPARENCY: code_map_overview provides coverage diagnostics broken down by primary workspace and linked_workspaces components rather than only global aggregate counts.'
scope:
- src/db/reader.rs
- src/db/symbols.rs
- src/db/traversal.rs
- src/engine/linker.rs
- src/engine/linker/resolution.rs
- src/mcp/tools/inputs.rs
- src/cli/mod.rs
- src/mcp/tools.rs
- tests/query_context/symbol_context.rs
- tests/query_context/ranking_and_coverage.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/impact_context.rs
- docs/reference/mcp-tools.md
- docs/reference/mcp-markdown-contracts.md
- docs/reference/cli.md
- skills/contextunity-forge/SKILL.md
status: completed
subtasks:
- subtask_ref: explicit-edge-descriptors-in-impact-and-explain
  title: Include edge kind, direction, and source location on neighbor nodes at depth 1 in traversal queries
  status: completed
  evidence: 'REVIEW12 candidate19240fa contract9: INV-EXPLICIT-EDGE-EVIDENCE: current depth1 edge evidence and both changed traversal directions/public via IDs independently verified; F053-T4-REVERSE-PREDECESSOR confirmed-fixed. query43/0, impact4/0, MCP85/0, exit0. Raw outbound MCP transcript and original fixtures in /tmp/forge053-task4-closure-cgd4t45l'
- subtask_ref: granular-unresolved-reference-taxonomy
  title: Categorize unresolved references into dynamic callbacks, unknown receiver types, and framework magic with source snippets
  status: completed
  evidence: 'REVIEW12 candidate19240fa contract9: INV-UNRESOLVED-TAXONOMY: full current source read; exactly3 guarded nonstdlib marker producers retained; original known/computed negatives now null, explicit shadowedcallback positive retained. F053-T4-UNPROVEN-DIAGNOSTIC-REASON confirmed-fixed. MCP85/0 + languages262/0 exit0; original-taxonomy.json in /tmp/forge053-task4-closure-cgd4t45l'
- subtask_ref: per-workspace-coverage-breakdown
  title: Report resolved, ambiguous, and unresolved counts segmented by workspace component in code_map_overview
  status: completed
  evidence: 'REVIEW12 candidate19240fa contract9: INV-SCOPE-COVERAGE-TRANSPARENCY: persisted primary/linked root/prefix/count page and reopen/config-removal/rebuild behavior independently reviewed in full current scope; current MCP85/0, query43/0, languages262/0 exit0. Fresh current repo skill copied via documented override; host untouched. /tmp/forge053-task4-closure-cgd4t45l/closure.json'
- subtask_ref: git-diff-impact-traversal
  title: Implement changes filter in code_map_impact mapping Git diff line ranges to impacted AST symbols and covering tests
  status: completed
  evidence: 'REVIEW12 candidate19240fa contract9: INV-DIFF-IMPACT-ANALYSIS: all3 Git modes/continuations/postimages/deletion/rename/path negatives, parserprefix counts, public reverse IDs, complete cyclic689 parity/tail16 and explicit10000-state failclosed independently verified. F053-T4-DIFF-HUNK-PREFIX and F053-T4-TRUNCATED-REACHABILITY confirmed-fixed. query43/0 impact4/0 MCP85/0 cli7/0; /tmp/forge053-task4-closure-cgd4t45l/closure.json'
receipt:
  contract_revision: 9
  passed_at: 2026-10-20T12:21:43.884750922+00:00
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
          evidence: 'PASS: all 14 changed implementation/doc/test blobs match accepted candidate19240fa; all are admitted revision9 owners. Only three repair files differ from rejected d462. Coordinator milestone authority is preserved. Task6/Markdown, crate/linker expansion and metadata are untouched. No source edits by reviewer.'
        claims:
          applicable: true
          evidence: 'PASS: all four revision9 invariants checked against real parser/linker/SQLite/CLI and stdio MCP seams. Depth1 actual edge endpoints/direction/location/count/evidence and deeper public via IDs retained; Git staged/unstaged/head, postimage binding, continuation invalidation, deletion/rename/odd paths and bounded failclosed traversal covered; taxonomy uses exactly three guarded nonstdlib markers and conservative nulls; persisted workspace coverage survives reopen/config removal until rebuild. Four prior IDs independently confirmed fixed, complete three-owner-cluster closure ledger read; no residual finding.'
        concurrency:
          applicable: true
          evidence: 'PASS: immutable graph generation and recaptured Git-mode/diff/hunk/postimage continuation identity prevent mixed snapshots; final postimage/git recapture rejects drift including staged-only changes. SQLite2s and graph10000-state limits remain; visited-path walk prevents cycles consuming the useful reachability budget. Independent live workcap errors explicitly rather than returning partial complete=true. Cargo gates ran sequentially with explicit shared target; builder idle.'
        project_isolation:
          applicable: true
          evidence: 'PASS: explicit-root fixed operational CLI and current runtime CLI used only assigned worktree or isolated /tmp real Git fixtures. Workspace source paths use persisted owner/root/digest and linked workspace partitions. Current overview root/generation matches assigned worktree; coverage persisted-state negatives pass. Fresh byte-identical repo skill copy selected through documented CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR; host installation untouched. All artifacts /tmp, no untracked worktree files.'
        administration:
          applicable: true
          evidence: 'PASS: independent reviewer worker differs from accepted builder; actual REVIEW12 binds contract9, candidate19240fa, baseline9bfe35, full diff8fc4a3ea. Original introduced findings and authentic RED/green/mutation history retained in cumulative ledger; all current required gates independently pass with exact counts/exit0. Canonical Forge subtask evidence updated; no manual status/receipt edits. Same session will deliver with profile auto_commit, no duplicate commit or branch integration.'
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
    - 'INV-EXPLICIT-EDGE-EVIDENCE: Graph inspection (impact and explain) at depth 1 returns full edge objects specifying edge kind (calls vs possible_calls), direction, and exact source location, strictly distinguishing statically proven calls from heuristic name candidates.'
    - 'INV-DIFF-IMPACT-ANALYSIS: code_map_impact supports a changes parameter (staged, unstaged, head) to map modified Git diff ranges directly to AST symbols, impacted graph neighbors, and affected tests.'
    - 'INV-UNRESOLVED-TAXONOMY: Unresolved references are categorized into specific granular reasons (dynamic_callback, unresolved_receiver_type, framework_magic, external_unindexed) with call expression snippets rather than generic other groupings.'
    - 'INV-SCOPE-COVERAGE-TRANSPARENCY: code_map_overview provides coverage diagnostics broken down by primary workspace and linked_workspaces components rather than only global aggregate counts.'
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

### task: documentation-knowledge-graph-and-invariant-traceability

```yaml
task_ref: documentation-knowledge-graph-and-invariant-traceability
target: Extract documentation citation graph, surface affected docs on impact and documented_in on inspect, index governing invariants from all docs as first-class graph entities, and diagnose doc drift with suggested symbol remediation
proof_policy: seam-test-first
contract_revision: 13
invariants:
- 'INV-DOC-GRAPH-TRACEABILITY: Markdown relative links and wikilinks are extracted into bidirectional cites_doc/cited_by_doc edges; governing invariants (INV-* rules) from linked doc sections are automatically surfaced on inspect and impact.'
- 'INV-UNIVERSAL-INVARIANT-INDEX: INV-* invariants across all docs/**/*.md and AGENTS.md are indexed as first-class graph nodes with bidirectional governs/governed_by edges to referenced symbols, replacing single-directory ADR token scraping.'
- 'INV-IMPACT-AWARE-INVARIANTS: code_map_inspect and code_map_impact surface direct and 1-hop transitive governing invariants for inspected and impacted code symbols.'
- 'INV-AFFECTED-DOCS-IMPACT: Symbol modification or deletion in code_map_impact surfaces affected_docs pointing to citing Markdown documentation sections and ADRs.'
- 'INV-BIDIRECTIONAL-DOC-CODE-NAVIGATION: code_map_inspect surfaces documented_in sections for code symbols, and get_doc surfaces referenced_symbols with live symbol verification.'
- 'INV-DOC-DRIFT-REMEDIATION-SUGGESTIONS: dead_doc_symbol diagnostics in code_map_analyze provide suggested_symbol candidates based on symbol similarity and module proximity.'
- 'INV-COMPACT-INVARIANT-PAYLOAD: Governing invariants in tool responses and task context are bounded to compact ID, single-line rule, and source reference; transitive invariants are summarized by ID with a strict cap of 3-5 direct invariants to prevent context-window bloat.'
- 'INV-CONTEXT-SAFE-DOC-REFERENCES: Documentary claims resolve only inside the source document workspace unless an explicit normalized target path confines the claim to another file. Within that eligible context, preserve qualified-name, declared-name, then segment-suffix priority and distinct-ID ambiguity. Document, invariant, and component nodes do not verify code claims. Only one eligible target creates verified documentary edges; full-detail get_doc exposes ambiguous selected-tier targets separately as paged referenced_symbol_candidates with source-claim and target identities. Compact detail omits candidate discovery without reading heavy columns; CLI docs get defaults to full detail, matching MCP get_doc. Candidates never establish verified references or governance; invalid explicit paths never fall back globally. Cold and delta indexing and CLI/MCP readers preserve these semantics.'
scope:
- src/engine/docs.rs
- src/engine/scanner/mod.rs
- src/engine/pipeline.rs
- src/engine/tasks/context.rs
- src/engine/linker/resolution.rs
- src/cli/mod.rs
- src/db/reader.rs
- src/db/traversal.rs
- src/mcp/tools.rs
- tests/mcp/context.rs
- src/db/symbols.rs
- tests/mcp/freshness/stdio_and_response.rs
- tests/incremental/doc_parity.rs
- tests/linker/optimizations.rs
- tests/acdd/mcp.rs
- tests/acdd/milestones.rs
- tests/acdd/tasks.rs
- src/core/config.rs
- docs/adr/0007-fail-closed-selector-disambiguation.md
- docs/AGENTS.md
- docs/reference/mcp-tools.md
- docs/reference/tasks.md
- docs/reference/configuration.md
- skills/contextunity-forge/SKILL.md
subtasks:
- subtask_ref: markdown-citation-graph-extraction
  title: Extract Markdown relative links and wikilinks into cites_doc and cited_by_doc bidirectional edges
  status: completed
  evidence: 'Independent REVIEW29 contract13 candidate9df31b2635d7c3c32623b9ca283290f080a43051: exact final commitment_integrity+strictClippy+all-targets exit0,811passed0failed1ignored (standalone16 not double-counted). Complete affected incremental31/MCP91/linker102/query43/ACDD116 pass. This retained slice reassessed against current source, public seams and accepted docs semantics; no open defect. Full closure /tmp/forge053-doc-review29-findings.json; log/counts/env /tmp/forge053-doc-review29-verification.json; all24 blobs match. Existing markdown-citation-graph-extraction oracles retained; candidate discovery never verifies governance/coverage.'
- subtask_ref: governing-invariants-injection
  title: Surface direct and 1-hop transitive INV-* invariants on code_map_inspect and code_map_impact
  status: completed
  evidence: 'Independent REVIEW29 contract13 candidate9df31b2635d7c3c32623b9ca283290f080a43051: exact final commitment_integrity+strictClippy+all-targets exit0,811passed0failed1ignored (standalone16 not double-counted). Complete affected incremental31/MCP91/linker102/query43/ACDD116 pass. This retained slice reassessed against current source, public seams and accepted docs semantics; no open defect. Full closure /tmp/forge053-doc-review29-findings.json; log/counts/env /tmp/forge053-doc-review29-verification.json; all24 blobs match. Existing governing-invariants-injection oracles retained; candidate discovery never verifies governance/coverage.'
- subtask_ref: affected-docs-and-bidirectional-navigation
  title: Surface affected_docs in impact, documented_in in inspect, and referenced_symbols in get_doc
  status: completed
  evidence: 'Independent REVIEW29 contract13 candidate9df31b2635d7c3c32623b9ca283290f080a43051: exact final commitment_integrity+strictClippy+all-targets exit0,811passed0failed1ignored (standalone16 not double-counted). Complete affected incremental31/MCP91/linker102/query43/ACDD116 pass. This retained slice reassessed against current source, public seams and accepted docs semantics; no open defect. Full closure /tmp/forge053-doc-review29-findings.json; log/counts/env /tmp/forge053-doc-review29-verification.json; all24 blobs match. Existing affected-docs-and-bidirectional-navigation oracles retained; candidate discovery never verifies governance/coverage.'
- subtask_ref: doc-drift-diagnostics-and-suggestions
  title: Report dead_doc_symbol, broken_doc_link, and stale_doc_sections in code_map_analyze with suggested_symbol candidates
  status: completed
  evidence: 'Independent REVIEW29 contract13 candidate9df31b2635d7c3c32623b9ca283290f080a43051: exact final commitment_integrity+strictClippy+all-targets exit0,811passed0failed1ignored (standalone16 not double-counted). Complete affected incremental31/MCP91/linker102/query43/ACDD116 pass. This retained slice reassessed against current source, public seams and accepted docs semantics; no open defect. Full closure /tmp/forge053-doc-review29-findings.json; log/counts/env /tmp/forge053-doc-review29-verification.json; all24 blobs match. Existing doc-drift-diagnostics-and-suggestions oracles retained; candidate discovery never verifies governance/coverage.'
- subtask_ref: universal-invariants-indexing-and-traversal
  title: Index INV-* invariants across all docs and AGENTS.md with governs/governed_by edges to symbols
  status: completed
  evidence: 'Independent REVIEW29 contract13 candidate9df31b2635d7c3c32623b9ca283290f080a43051: exact final commitment_integrity+strictClippy+all-targets exit0,811passed0failed1ignored (standalone16 not double-counted). Complete affected incremental31/MCP91/linker102/query43/ACDD116 pass. This retained slice reassessed against current source, public seams and accepted docs semantics; no open defect. Full closure /tmp/forge053-doc-review29-findings.json; log/counts/env /tmp/forge053-doc-review29-verification.json; all24 blobs match. Existing universal-invariants-indexing-and-traversal oracles retained; candidate discovery never verifies governance/coverage.'
- subtask_ref: context-safe-documentary-resolution-and-candidate-discovery
  title: Reject cross-workspace short-name hijacking and expose ambiguous documentary candidates without verified or governing edges; prove cold/delta and public-query parity
  status: completed
  evidence: 'Independent REVIEW29 contract13 candidate9df31b2635d7c3c32623b9ca283290f080a43051: exact final commitment_integrity+strictClippy+all-targets exit0,811passed0failed1ignored (standalone16 not double-counted). Complete affected incremental31/MCP91/linker102/query43/ACDD116 pass. This retained slice reassessed against current source, public seams and accepted docs semantics; no open defect. Full closure /tmp/forge053-doc-review29-findings.json; log/counts/env /tmp/forge053-doc-review29-verification.json; all24 blobs match. Existing context-safe-documentary-resolution-and-candidate-discovery oracles retained; candidate discovery never verifies governance/coverage.'
status: completed
receipt:
  commit: 7d22366844133a28363d0b3b24d1c5c481cc14a6
  contract_revision: 13
  passed_at: 2026-10-20T20:36:31.590513520+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test incremental
      exit_code: 0
      tests_passed: 31
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: all24 admitted current files equal accepted BUILD28 snapshot9df31b2635d7c3c32623b9ca283290f080a43051 before/after final verification; /tmp/forge053-doc-review29-source-binding.json. Complete repair diff reviewed under contract13. Since REVIEW24 runtime/tests remain byte-identical; only mcp-tools clarification and admitted docs/AGENTS + ADR0007 prose changed. Held benchmark runner/helper and untracked054 drafts are explicitly excluded from task delivery and hash-bound in verification.json. No schema/DocSection/DTO/cache codec/profile/credential/crate/unrelated linker changes; no reviewer candidate edits.'
        claims:
          applicable: true
          evidence: 'PASS: contract13 original Task5 graph/provenance/Decision/paging/range obligations retained, with eligibility before exact qualname→declared name→suffix in both documentary consumers. Only singleton distinct IDs verify reciprocal documentary/governing edges; ambiguous winning tiers are full-only, occurrence-selected before public dedup and bounded generation-aware paging. Invalid explicit paths do not fall back; compact retains no-heavy-column guard. CLI DocsGet alone defaults full. Current public MCP91/incremental31/linker102/query43/ACDD116 pass within independent full811; actual writer/reader/stdio/CLI seams and source-only cold/delta transitions retained. Authentic pre-fix REDs are sensitivity evidence, not invented mutations. Final user-approved docs/ADR explanation matches runtime; README obsolete component oracle corrected with retained real reciprocal positive and full root/edge parity. Detailed /tmp/forge053-doc-review29-findings.json and earlier source audit /tmp/forge053-doc-review24-interim.md.'
        concurrency:
          applicable: true
          evidence: 'PASS: added candidate SQL is read-only, does not add shared mutable cache, schema writer, lock order or task mutation. Existing MCP admission transaction/shared filesystem lock and generation fencing remain unchanged; count/page use the same existing paging budget and continuation validation. Per-occurrence tier/cardinality precedes final dedup, avoiding same-line explicit-claim cross-talk. Complete independent ACDD116/MCP91/core44+1ignored/commitment16 and all-targets811 pass on frozen candidate. No claim that sequential fixtures alone prove new concurrency behavior; no new concurrency protocol is introduced.'
        project_isolation:
          applicable: true
          evidence: 'PASS: source workspace resolved by canonical core::config::workspace_path; exact normalized explicit target confines the cross-workspace lane, unsafe/missing targets never fall back globally. Actual configured linked-workspace fixtures cover primary/linked decoys, foreign-only absence, explicit target and ambiguity. Lifecycle binds assigned milestone WT plus explicit authoritative primary tasks DB through fixed049 CLI; runtime reports attributed to candidate binary, never external lifecycle tool. Supported copied-skill override matches current repo SKILL.md; no host installation or main edits. Source binding and proof files stay /tmp. Full linked_workspaces3/cli_roots7 and other affected targets pass.'
        administration:
          applicable: true
          evidence: 'PASS: REVIEW29 contract13 worker codex-053-doc-graph-reviewer differs from accepted BUILD28 builder. User-approved docs admission followed canonical REVIEW24 reject→contract13→BUILD28; no manual status/receipt edits or invented source defect. Independent exact compound commitment_integrity && strictClippy && all-targets exit0,811passed/0failed/1ignored; standalone16 not double-counted. Source freeze rechecked after gates. All6 current subtasks reassessed with canonical evidence. No profile/schema/migration changes. Same reviewer will claim deliver and use returned auto_commit proof; no duplicate/manual commit or branch integration. Deferred audit proposals remain bounded outside current repair.'
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
    - 'INV-DOC-GRAPH-TRACEABILITY: Markdown relative links and wikilinks are extracted into bidirectional cites_doc/cited_by_doc edges; governing invariants (INV-* rules) from linked doc sections are automatically surfaced on inspect and impact.'
    - 'INV-UNIVERSAL-INVARIANT-INDEX: INV-* invariants across all docs/**/*.md and AGENTS.md are indexed as first-class graph nodes with bidirectional governs/governed_by edges to referenced symbols, replacing single-directory ADR token scraping.'
    - 'INV-IMPACT-AWARE-INVARIANTS: code_map_inspect and code_map_impact surface direct and 1-hop transitive governing invariants for inspected and impacted code symbols.'
    - 'INV-AFFECTED-DOCS-IMPACT: Symbol modification or deletion in code_map_impact surfaces affected_docs pointing to citing Markdown documentation sections and ADRs.'
    - 'INV-BIDIRECTIONAL-DOC-CODE-NAVIGATION: code_map_inspect surfaces documented_in sections for code symbols, and get_doc surfaces referenced_symbols with live symbol verification.'
    - 'INV-DOC-DRIFT-REMEDIATION-SUGGESTIONS: dead_doc_symbol diagnostics in code_map_analyze provide suggested_symbol candidates based on symbol similarity and module proximity.'
    - 'INV-COMPACT-INVARIANT-PAYLOAD: Governing invariants in tool responses and task context are bounded to compact ID, single-line rule, and source reference; transitive invariants are summarized by ID with a strict cap of 3-5 direct invariants to prevent context-window bloat.'
    - 'INV-CONTEXT-SAFE-DOC-REFERENCES: Documentary claims resolve only inside the source document workspace unless an explicit normalized target path confines the claim to another file. Within that eligible context, preserve qualified-name, declared-name, then segment-suffix priority and distinct-ID ambiguity. Document, invariant, and component nodes do not verify code claims. Only one eligible target creates verified documentary edges; full-detail get_doc exposes ambiguous selected-tier targets separately as paged referenced_symbol_candidates with source-claim and target identities. Compact detail omits candidate discovery without reading heavy columns; CLI docs get defaults to full detail, matching MCP get_doc. Candidates never establish verified references or governance; invalid explicit paths never fall back globally. Cold and delta indexing and CLI/MCP readers preserve these semantics.'
    architectural_notes:
    - 'decisions: Per direct user instruction, publish intermediate technical reports to task blackboard using canonical topics: notes for proof/status, findings for verified defects, blockers for actual stalls, decisions/deferred for retained dispositions. Use concise summaries plus exact evidence paths and inspect IDs for full payload; supply those IDs explicitly to the next agent/gate. Conversation and host agent messages remain notification transport. This is current task operating direction, not an unadmitted source/profile change.'
    - 'deferred: User-requested read-only audits completed: documentation report /tmp/forge053-documentation-audit-20261020/report.md (six bounded findings), blackboard instruction report /tmp/forge053-blackboard-audit-20261020/report.md (messages571/574). Proposed guide/invariant examples, stale admission prose/ADR0014, citation triage, code/test links, preview defaults, and reporting triggers are recommendations; no repository/profile edits were authorized by those analysis requests. Handle each through its owning admitted task and accepted ADR authority. Current Task5 repair updates its canonical MCP reference and Forge skill. /tmp artifacts are session evidence, not a durable archive promise.'
    - 'decisions: User explicitly approved candidate semantics and requested docs/ADR clarification: discovery candidates do not create verified documentary/governance edges or count as verified coverage. Reviewer returning24 to build before edits. Fresh benchmark both code_coverage_preserved=true; Commerce paired median -6.195%, Forge +0.444%. Real MCP123 candidates/13pages and40331 unchanged inputs PASS. Post-benchmark full gate running; no delivery approval yet.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```


### task: mcp-markdown-output-and-dead-metadata-compaction

This scope belongs to milestone 054, `m-markdown-first-dual-surface-and-agent-ergonomics`.
Its core formatter and metadata task owns metadata compaction; its discovery,
traversal, task-lifecycle, coordination/documentation, and CLI parity tasks own
the corresponding response formatters and seam tests. This retained task is
cancelled in 053 to preserve its execution history. It contributes no delivery
claim or implementation requirement to 053.

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
status: cancelled
subtasks:
  - subtask_ref: inspect-markdown-formatter-and-relation-contracts
    title: "Implement structured Markdown formatter for code_map_inspect per docs/reference/mcp-markdown-contracts.md#21-code_map_inspect"
    status: pending
  - subtask_ref: dead-metadata-and-generation-hash-stripping
    title: "Omit generation hashes and empty collections from responses when has_more is false"
    status: pending
  - subtask_ref: code-map-markdown-formatter
    title: "Implement structured Markdown formatters for search, impact, and overview per mcp-markdown-contracts.md"
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

### task: cancelled-task-handoff-and-finalization

Close the user-authorized milestone delivery while retaining the cancelled task
transferred to milestone 054. This task does not implement that transferred work
or alter any accepted task receipt. The final closure artifacts include the
requested lossless skill routing and benchmark evidence integration; benchmark
execution remains subject to the repository's explicit authorization rule.

```yaml
task_ref: cancelled-task-handoff-and-finalization
target: Accept retained cancelled task authority during milestone handoff without completion credit, preserve completed proofs and cancellation history atomically, integrate the authorized final instruction packaging and benchmark evidence artifacts, and align the six audited documentation cases before handoff
proof_policy: seam-test-first
contract_revision: 2
invariants:
- 'INV-TERMINAL-HANDOFF-AUTHORITY: Milestone handoff accepts only an exact manifest and SQLite task set whose tasks are completed with valid accepted proofs or identically cancelled without completion receipts. Cancellation identity, revision, digest, status and milestone location are bound and revalidated; all other task states, mismatches and missing proofs fail closed.'
- 'INV-CANCELLED-NONDELIVERY: Cancelled tasks retain their status, attempts, gates, subtasks, dependencies and evidence through archive relocation and rollback. They supply no accepted builder, completion receipt, dependency satisfaction or implementation scope credit; completed task proofs remain immutable.'
- 'INV-LOSSLESS-CLOSURE-ARTIFACTS: Final skill routing preserves actionable guidance through packaged repository references, and benchmark evidence records attributable observed results without inventing performance thresholds or changing benchmark authorization. Transferred milestone 054 implementation and held architecture decisions remain excluded.'
scope:
- src/engine/milestones/handoff.rs
- src/db/tasks_store/mod.rs
- tests/acdd/milestones.rs
- docs/reference/tasks.md
- docs/reference/cli.md
- docs/README.md
- docs/AGENTS.md
- docs/milestones/README.md
- docs/architecture/indexing.md
- docs/reference/configuration.md
- docs/adr/0014-milestone-task-subtask-hierarchy-and-visibility.md
- AGENTS.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
- benchmarks/cold_test_coverage_benchmark.py
- benchmarks/cold_coverage_metrics.py
subtasks:
- subtask_ref: cancelled-terminal-authority-and-proof-partition
  title: Bind matching cancelled authority separately from completed proofs and reject incomplete, forged or changed task authority without scope, builder or dependency credit
  status: completed
  evidence: 'Independent REVIEW7: exact matching terminal task set/digest/revision/status/location, completed-only proof/builders/scope, real existing11-case CLI/Git/SQLite seam all green. Full final compound exit0; ACDD116/0, all-targets811/0/1ignored and strictClippy0. Candidatead0e6fa all30 scopedblobs equal. /tmp/forge053-close-review7-findings.json; /tmp/forge053-close-review7-verification.json.'
- subtask_ref: atomic-terminal-archive-relocation
  title: Relocate completed and cancelled history atomically with exact task and completed-receipt sets, old-reference fencing and unchanged rollback guarantees
  status: completed
  evidence: 'Independent REVIEW7: IMMEDIATE transaction fences SQLidentity/oldref and entire expectedTask snapshot, exact alltask/completedreceipt sets; cancelled history fields/tables unchanged. Real trigger rollback, oldref/forgedreceipt and public archive cases pass, independent fullACDD116/0+all811/0/1ignored/Clippy0. /tmp/forge053-close-review7-findings.json.'
- subtask_ref: lossless-skill-route-packaging
  title: Preserve actionable instruction content through repository-owned routed skill references and verify packaged reference availability
  status: completed
  evidence: Independent REVIEW7:208-line skill + four routed references and two portable examples read; before/after map verified, all30 scopedblobs current and complete copied skill tree byteequal. Independent fullMCP91/0 includes recursive supported-layout/link tests. No external installer/global sync claim. /tmp/forge053-close-review7-binding.json; /tmp/forge053-close-review7-verification.json.
- subtask_ref: benchmark-evidence-integration
  title: Integrate authorized benchmark metric collection and attributable final evidence without new performance promises or unauthorized benchmark execution
  status: completed
  evidence: 'Independent REVIEW7: runnerSHA931a987a6c5e78b2038621ab12a1495a7ab71cfe750f004edf38df7553a57634 and helperSHAc1bb7bec78a28cb45fcfe47fe306297fa5f598dd5cd2886a7ce126a28d3c610c match prior independent source/realDB metric audit PASS exactly. Separate timing/code coverage provenance and comparison policy retained. No benchmark execution or invented threshold/speed claim in closure. /tmp/forge053-installed-benchmark-20261020/benchmark-independent-review.md; currentbinding /tmp/forge053-close-review7-binding.json.'
- subtask_ref: current-documentation-alignment
  title: Align the overview symbol-link promise, explicit same-line INV authoring example, universal documentation admission versus planning authority and ADR0014, actual link-diagnostic triage, extractor/linker/test navigation, and preview policy defaults with verified current behavior
  status: completed
  evidence: 'Independent REVIEW7: all six current cases verified against exact resolver/extractor/indexing/response source and finalscan. Terminal completed/cancelled wording aligned in root/CLI/tasks/template; actual docsAGENTS same-line explicit source unique INV-DOC-FIRST-MATCH-TIER surfaces direct with0omitted. Historical accepted receipt unchanged. Fullfinal811/0/1ignored+Clippy0. /tmp/forge053-close-review7-findings.json; /tmp/forge053-close-final-governing-inspect.json.'
status: completed
receipt:
  commit: cbbddfeb7c64a336db0c249dfa388b8115049ce1
  contract_revision: 2
  passed_at: 2026-10-20T21:35:08.747854331+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd && cargo test --test mcp
      exit_code: 0
      tests_passed: 207
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: all30 scoped source/reference blobs equal accepted BUILD6 candidate ad0e6fae741884e6bef762a97cacaf66683883c6, baseline dfa2b75e5967edfffce642b74d75a71c2fcec76a. Tracked diff SHA2569360538d7340f5b89ab67284ec7ca1a869c8e18036d1b5732f4ca381a08ff121. All16 tracked changes and7 new files are admitted scope or engine-managed milestone authority; prior completed receipts unchanged. No054, heldADR, config, schema or installed-skill edits. /tmp/forge053-close-review7-binding.json.'
        claims:
          applicable: true
          evidence: 'PASS: handoff.rs278-427 enforces exact terminal task set/digests/revisions/status/location and separates cancelled authority from completed proof/builders;429-475 grants only completed scope;518-668 preserves completed proof receipts and cancelled history. tasks_store/mod.rs1324-1408 and existing milestones.rs1312-1547 provide real Git/CLI/SQLite11-case acceptance, status/digest/revision/missing-task, forged receipt, old-ref, dependency, scope and injected rollback proof. Skill208-line routing retains actionable content through four references plus two portable templates; MCP supported-layout proof covers full copied tree, not external installers. Six docs cases match actual current source and governing proof. Benchmark files exactly match prior reviewed runner931a987a/helperc1bb7bec; no new timing or performance claims. Independent final full compound exit0, all-targets811/0/1ignored, Clippy0; standalone16 excluded from duplicate count. /tmp/forge053-close-review7-verification.json.'
        concurrency:
          applicable: true
          evidence: 'PASS: tasks_store/mod.rs1333-1406 uses IMMEDIATE transaction and fences SQL identity/old location plus complete expected Task snapshot before updates; exact full task and completed-only receipt sets gate commit. Cancellation changes only milestone_ref; save1426-1432 does not mutate attempts/gates/subtasks/dependency/evidence tables. Engine archive publication removes archive on transaction failure and reverses full archived snapshot on source-removal failure; original rollback path preserved. Trigger-injected real relocation failure and complete detail equality checked in existing handoff table; all ACDD116 passed independently in full gate. No new lock order or profile edge.'
        project_isolation:
          applicable: true
          evidence: 'PASS: active root explicit throughout lifecycle/runtime and Cargo. Handoff ready_milestone opens repository/project store; relocation SQL intersects milestone_prefix and self.id_prefix, requires both stored and descriptor old_ref. Existing core filesystem/path, adapter/profile, Git and publication guards retained without new bypass; full gate includes project, symlink/path and cross-worktree guards. Fresh copied repository skill under /tmp only; all skill files byte-equal and no host installation writes. Runtime graph57156849461818118cf758b7428779b7ce054cae680fdcd1a9c27d7e47661104 is current WT, external049 used for lifecycle only.'
        administration:
          applicable: true
          evidence: 'PASS: contract2/BUILD6/candidatead0e6fa bound to independent reviewer codex-053-final-reviewer distinct builder codex-053-handoff-builder. No schema/profile pin/protected header or accepted receipt rewriting. Exact candidate fence and diff-check0 retained through final full test. Completed receipt matches_proofs validation/provenance retention unchanged; cancellation never obtains completion authority. Six resolved deferrals described in new manifest section while historical Task5 receipt immutable. Five subtasks reassessed through canonical Forge operations; review/delivery use claim templates and auto_commit, no manual commit. Final delivery result/parent/hash/clean-state proof is recorded separately after automatic commit.'
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
    - 'INV-TERMINAL-HANDOFF-AUTHORITY: Milestone handoff accepts only an exact manifest and SQLite task set whose tasks are completed with valid accepted proofs or identically cancelled without completion receipts. Cancellation identity, revision, digest, status and milestone location are bound and revalidated; all other task states, mismatches and missing proofs fail closed.'
    - 'INV-CANCELLED-NONDELIVERY: Cancelled tasks retain their status, attempts, gates, subtasks, dependencies and evidence through archive relocation and rollback. They supply no accepted builder, completion receipt, dependency satisfaction or implementation scope credit; completed task proofs remain immutable.'
    - 'INV-LOSSLESS-CLOSURE-ARTIFACTS: Final skill routing preserves actionable guidance through packaged repository references, and benchmark evidence records attributable observed results without inventing performance thresholds or changing benchmark authorization. Transferred milestone 054 implementation and held architecture decisions remain excluded.'
    architectural_notes:
    - 'decisions: Final closure contract2 fulfilled: cancellation remains terminal non-delivery authority, completed proofs/builders/scope remain separately verified; all5 subtasks completed via API with evidence. Final registered-prefix command Clippy+ACDD116+MCP91 exit0 /tmp/forge053-close-final-build.log, focused seam1 not double counted; original RED101 retained. Six documentation findings resolved and independently scanned BB636–637. Skill208 lines routes packaged references/examples tested in copied supported layouts. Benchmark artifacts integrated from prior authorized work; no benchmark execution in closure, no new speed promise or external installation. Framework example audit BB635 remains read-only informational, no unauthorized cleanup. Current graph proves docs/AGENTS.md INV-DOC-FIRST-MATCH-TIER directly governs resolver: /tmp/forge053-close-final-governing-inspect.json. BUILD submission is not final review or milestone handoff.'
    - 'decisions: Closure acceptance evidence, not new authority: independent REVIEW7 contract2 accepted BUILD6 candidatead0e6fae741884e6bef762a97cacaf66683883c6 across all5contours. Exact full compound cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets exited0:811passed0failed1ignored, standalone16 not doublecounted, strictClippy0; logSHA256fe217408b04f2120b084b805df7f8ac0672b26115c24507d4fbe6a8cb4751c58. All30 scopedblobs frozen, tracked diffSHA9360538d7340f5b89ab67284ec7ca1a869c8e18036d1b5732f4ca381a08ff121. All5subtasks independently completed. Cancelled Task6 retains history, no receipt/builders/scope/dependency credit. Skill208lines routes losslessly; full copied repository skill supported-layout tests pass, no external installer/host sync claim. Benchmark runner931a987a6c5e78b2038621ab12a1495a7ab71cfe750f004edf38df7553a57634/helperc1bb7bec78a28cb45fcfe47fe306297fa5f598dd5cd2886a7ce126a28d3c610c match prior independent metric audit. No benchmark execution or new performance promise. Six authorized documentation cases and terminal wording F1 confirmedfixed; earlier accepted Task5 receipt immutable. Framework example635 inventory remains informational with no cleanup authorization. Full findings/proof artifacts /tmp/forge053-close-review7-{findings.json,findings.md,binding.json,verification.json}. Parent owns subsequent final integration binding/review, archive and landing; no new deferred implementation.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

## Resolved documentation audit follow-up

The six documentation proposals retained from blackboard 581 are addressed in
the closure task. [The overview](../../README.md) qualifies symbol linkage;
[authoring guidance](../../AGENTS.md) distinguishes first-class same-line INV claims
from alert classification. The overview, [milestone navigation](../README.md), and
[ADR0014](../../adr/0014-milestone-task-subtask-hierarchy-and-visibility.md) align
universal Markdown indexing with separate planning authority. [Indexing
architecture](../../architecture/indexing.md) provides verified extractor, linker,
and test navigation; [configuration](../../reference/configuration.md) distinguishes
the 200-line default and 1..500 range from this repository's explicit 35-line
adapter setting.

Triage of the audit's 36 `broken_doc_link` diagnostics identifies 22 existing
physical targets: 21 admitted documents have multiple sections and valid
unanchored whole-page navigation, and `skills/README.md` is outside documentation
admission. No missing physical target or broken anchor was found. The six typed
deferrals are therefore removed; the immutable Task5 receipt retains its
historical audit disposition. The documentation owner's final diff check passes.

## Task delivery commits

Each completed task has one integrated commit. These navigation pointers record
the repacked history; accepted task proofs remain unchanged.

| Task | Commit |
| --- | --- |
| search-precision-and-natural-docs-retrieval | `1f826b5f3372581098bd1e226cc09011c3737f3c` |
| source-preview-budget-and-chunking-limits | `1de1ca0d81877b4d80be027fdfe0a1fab8f52d27` |
| test-discovery-chains-and-role-classification | `cb643376c9abf198b89660cfd1e3e6735b0a44e9` |
| graph-edge-evidence-unresolved-taxonomy-and-scope-transparency | `7c92226ac734a6534a7cbd50dee92aa447f8097e` |
| documentation-knowledge-graph-and-invariant-traceability | `7d22366844133a28363d0b3b24d1c5c481cc14a6` |
| cancelled-task-handoff-and-finalization | `cbbddfeb7c64a336db0c249dfa388b8115049ce1` |

The retained cancelled Markdown-first task belongs to milestone 054 and receives
no delivery commit or completion credit in 053.
