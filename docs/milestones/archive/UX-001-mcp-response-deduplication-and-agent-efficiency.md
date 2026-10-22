---
id: m-mcp-response-deduplication-and-agent-efficiency
title: MCP response deduplication, Markdown presentation compaction, and agent call efficiency
doc_type: contract
status: completed
depends_on:
- m-markdown-first-dual-surface-and-agent-ergonomics:completed
owners:
- src/cli/mod.rs
- src/core/format/
- src/db/lint.rs
- src/db/paging.rs
- src/db/reader.rs
- src/db/symbols.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- tests/acdd/
- tests/mcp/context.rs
- tests/mcp/lint_tools.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/query_context/
- docs/reference/mcp-tools.md
- docs/reference/tasks.md
- docs/reference/mcp-markdown-contracts.md
- skills/contextunity-forge/
invariants:
- 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
- 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
- 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
- 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
- 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
- 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
- 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
- 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
related_plans:
- docs/adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md
- docs/plans/forge-mcp-audit-and-architecture.md
- docs/plans/universal-multi-worktree-resolution.md
started_at: 2026-10-21T16:00:08+00:00
handoff:
  completed_at: 2026-10-22T17:15:58.190081668+00:00
  duration: 25h 15m
  commit: 3ae3866d594c93802d21e455171d9214a68fbd51
  target_base: fa79618b2dc2455940033d176f0cc66c438a7b7c
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-mcp-response-deduplication-and-agent-efficiency
      authority_digest: 78dd5f7a6a3b2554c5ebdb84764cfe8ad72f8c25beeb83e787488fe699b1affd
      implementation_digest: 2ac58669e272f3ff08906211d45188046a354f744379a47b65c60a949180f0ec
      target_base_digest: 707fb8336291440ad0329cda73f47a1cb42528c66bba289fa382accdfd3ad5c3
      builders:
      - kilo-ux001-budget-coordinator-builder
      - kilo-ux001-compact-builder
      - kilo-ux001-continuation-builder
      - kilo-ux001-coordinates-builder
      - kilo-ux001-dedup-builder
      - kilo-ux001-diagnostics-builder
      - kilo-ux001-final-doc-coordinator-builder
      - kilo-ux001-inspect-docs-builder
      - kilo-ux001-references-builder
      - kilo-ux001-source-builder
      - kilo-ux001-sql-builder
    target_base: fa79618b2dc2455940033d176f0cc66c438a7b7c
    review:
      worker_id: kilo-ux001-final-integration-reviewer
      decision: pass
      summary: Final integrated content and documentation satisfy the admitted milestone authority. Accepted proofs are preserved, final scope is admitted, and no blocking integration defects remain. Source continuations require explicit generation, source pages contain complete lines only, and the user-authorized simplification supersedes the historical cache contract.
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 815
      tests_failed: 0
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 815
    tests_failed: 0
---

# MCP response deduplication, Markdown presentation compaction, and agent call efficiency

## Outcome and purpose

Compact MCP and CLI Markdown without changing structural graph JSON or rejecting valid read-only SQL.

Tasks share `src/db/reader.rs` in this order: SQL fidelity, diagnostic scope, inspection projection, then compact document pages. Task-record deduplication is independent. Clickable coordinates start after the formatters they edit. The documentation task starts after every behavior task.

## Audit cases and delivery boundary

| Case | Evidence | Owning task |
| --- | --- | --- |
| **SQL statement boundaries** | [`analyze_paged`](../../../src/db/reader.rs#L1649) rejects any `sql.contains(';')` and classifies a statement by the first whitespace token. `SELECT ';' AS marker` fails. A leading comment such as `/* ; */ SELECT 1` is not recognized as SQL. | `preserve-read-only-sql-results` |
| **Shared row projection** | [`rows`](../../../src/db/reader.rs#L525) drops `node_hash`, `path_hash`, `path_id`, and `owner_path_id` for every caller, and JSON-decodes text in `details`, `invariants`, and `referenced_symbols`. `SELECT path_id, '{"a":1}' AS details, 'raw text' AS note` returns an object `details` and omits `path_id`. `'raw text'` already stays a string. [`document_reference_candidate_page`](../../../src/db/reader.rs#L1341) and `document_diagnostics` read `details.symbol_claims` as JSON. | `preserve-read-only-sql-results` |
| **Inspect document dump** | Inspect builds `documented_in` and `documents` for the same `references_doc` edges. Markdown renders the first as rows and the second through [`render_json_section`](../../../src/core/format/discovery.rs#L285) under the heading `## documents`. Pretty JSON starts with `"total"`, not `"items"`. | `single-document-projection-for-inspection` |
| **Compact hint and citations** | [`render_candidate_page`](../../../src/core/format/coordination.rs#L245) prints the omission stub in page metadata and again when `items` is absent. `get_doc` referenced symbols omit the citing section and `e.line`, so distinct citations of one symbol render as identical rows. Empty relation pages still emit headings. | `emit-compact-document-hint-once` |
| **Task inspect repetition** | `task_manage(action="inspect")` uses [`render_record`](../../../src/core/format/lifecycle.rs#L360) and the heading `## Task record`. `## Contract` belongs to claim context. Equal scope is stored as `spec.scope`, `authority_snapshot.task_scope`, `initial_scope`, and `allowed_write_scope`. Equal target is stored as `spec.target`, `authority_snapshot.task_target`, and `goal`. There is no `current_scope` field. | `deduplicate-task-inspection-presentation` |
| **Diagnostic scope** | A missing analyze path returns zero totals and tells the caller to pick from `top_files`. `overview_with_options` ignores unknown aspects and the JSON envelope is freshness only. Lint already rejects a missing path with a compiled-profile sentence, not `PATH_NOT_INDEXED`. Targets `diagnostics` and `cycles` have their own mode error. | `resolve-diagnostic-scope-ambiguity` |
| **Coordinates** | [`file_uri_for_path`](../../../src/core/format/discovery.rs#L21) encodes an absolute path. [`attach_uri`](../../../src/cli/mod.rs#L492) resolves the workspace path first. MCP Markdown calls that helper from `src/mcp/tools.rs`. The literal `(link unavailable)` exists only in `src/core/format/discovery.rs`. Coordination falls back to inline code, and traversal `coordinate` falls back to `` `path:line` ``, when `uri` is absent. | `universal-clickable-coordinates-and-heading-compaction` |
| **Navigation docs** | `skills/contextunity-forge/references/tool-catalog.md` is present. Guidance still has to describe the narrowed calls, opaque SQL, diagnostic errors, and explicit `worktree` selection. | `document-efficient-agent-navigation` |

## Tasks in this milestone

### task: preserve-read-only-sql-results

```yaml
task_ref: preserve-read-only-sql-results
target: Classify one read-only statement before the path branch, and project opaque user SQL separately from structural graph rows
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/db/reader.rs
- src/db/paging.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/mcp/context.rs
- tests/query_context/diagnostics_and_source.rs
- docs/reference/mcp-tools.md
invariants:
- 'SQL-FIDELITY: User SQL rows are opaque. Structural rows keep parsed details, parsed referenced_symbols, and hidden internal ids.'
status: completed
receipt:
  contract_revision: 4
  passed_at: 2026-10-21T16:43:01.574348308+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp
      exit_code: 0
      tests_passed: 91
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Candidate snapshot 1f990d26297919d0bacd02422cb99fff3bbc12ac; scoped git diff binding 53f5ccad67f8e6e7d45d2fe812c23f1b36170359f703132b6b662080ba6c756a matches the builder note. Candidate changes are confined to the six admitted paths. Pre-existing lifecycle.rs, engine/tasks/mod.rs, engine/tasks/workspaces.rs, tests/acdd/mcp.rs edits and the untracked UX-001 manifest were observed and left untouched.
        claims:
          applicable: true
          evidence: PASS. classify_read_only_sql runs before the analyze path branch; its lexer skips whitespace/comments, tracks SQLite quote forms and semicolons, allows one trailing terminator, and rejects stacked statements. prepare().readonly() rejects writes/writable CTEs. MCP seam tests exercise both code_map_analyze and code_map_query(operation=sql), opaque ids/hashes/JSON-looking text, structural parsing/hidden ids, write and multi-statement rejection. The corrected stdio metadata seam asserts exact details text and retains nested metadata assertions through test-only parsing. Docs were updated.
        concurrency:
          applicable: true
          evidence: PASS. No task-store, receipt, or shared lifecycle mutation was added. SQL remains on the supplied read-only connection; generation checks, count/page execution, query time and byte bounds are preserved. Rejected SQL tests compare persisted database state before and after.
        project_isolation:
          applicable: true
          evidence: PASS. Analyze/query calls use the selected workspace database connection; tests use isolated Workspace fixtures. ATTACH and write statements are rejected, the database remains unchanged, and no attached database is created. No profile, workspace resolution, or path scope code changed.
        administration:
          applicable: true
          evidence: PASS. No profile, schema, migration, credential, hook, or delivery-policy files changed. The task snapshot and receipt remain Forge-managed; no manual commit or receipt edit was made during review.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'SQL-FIDELITY: User SQL rows are opaque. Structural rows keep parsed details, parsed referenced_symbols, and hidden internal ids.'
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

#### Acceptance

1. Skip whitespace and comments, then accept one `SELECT` or `WITH` statement. Accept `SELECT ';' AS marker`, a quoted identifier containing `;`, semicolons inside `/* */` and `--` comments, a leading comment before that statement, and a trailing semicolon whose remainder is whitespace.
2. Reject a second statement. Reject writes and writable CTEs. Keep the existing `tests/mcp/tool_evolution/mcp_boundaries.rs` rejections for those cases.
3. `code_map_analyze` and `code_map_query(operation="sql")` share this classifier. A comment-led `SELECT` stays SQL and does not enter the path branch.
4. Add an opaque mode on [`rows`](../../../src/db/reader.rs#L525) / [`query`](../../../src/db/paging.rs#L60). User SQL uses it. The default structural mode stays.
5. Opaque mode returns selected `path_id`, `owner_path_id`, `node_hash`, and `path_hash`, and returns `details`, `invariants`, and `referenced_symbols` as SQL text. `'{"a":1}'` and `'null'` stay strings.
6. Structural mode still JSON-decodes those names. `document_reference_candidate_page`, `document_diagnostics`, and ambiguous-candidate signature lookup keep working.
7. Add the new cases to `tests/mcp/tool_evolution/mcp_boundaries.rs`. Align the existing opaque-SQL metadata seam in `tests/mcp/context.rs` with preserved SQL text while retaining its nested metadata checks. Use `tests/query_context/diagnostics_and_source.rs` only for an analyze seam that already lives there.

### task: resolve-diagnostic-scope-ambiguity

```yaml
task_ref: resolve-diagnostic-scope-ambiguity
target: Fail unindexed diagnostic targets and unsupported overview aspects without treating indexed empty results as missing
proof_policy: seam-test-first
contract_revision: 4
depends_on:
- preserve-read-only-sql-results
scope:
- src/db/reader.rs
- src/db/lint.rs
- src/mcp/tools/inputs.rs
- tests/query_context/diagnostics_and_source.rs
- tests/mcp/lint_tools.rs
- docs/reference/mcp-tools.md
invariants:
- 'DIAGNOSTIC-SCOPE-TRUTH: PATH_NOT_INDEXED means the path is outside the indexed inventory. Zero totals mean the indexed scope has no matching rows.'
status: completed
receipt:
  contract_revision: 4
  passed_at: 2026-10-21T17:49:37.030388171+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context && cargo test --test mcp
      exit_code: 0
      tests_passed: 134
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Snapshot 49fac563fb846db3960e125142352511840857d2 against base b4562f32c18d5b75926a9ca6f007763f9d1e1ad5. The six admitted paths' git diff --binary SHA-256 is 1425c80ab3bcf16af50e9845233c45a1985d13d49d86db4c92dfb94e4080d88b, matching build note 900 before and after checks. Pre-existing lifecycle/tasks/workspaces edits and sibling tests/acdd/mcp red-test changes are outside scope and untouched; no config/profile paths changed.
        claims:
          applicable: true
          evidence: 'PASS. analyze_paged classifies SQL before its indexed-path check; empty workspace, PATH_NOT_INDEXED, diagnostics/cycles mode errors, indexed exact-file zero totals and file-local continuation satisfy the contract. syntax_paged checks inventory before compiled-source availability and retains README''s compiled-profile error. Overview rejects unknown values with all 11 accepted names; omitted/empty defaults and cycles opt-in persist. The four adapter-owned aspects remain accepted and are still composed in src/cli/mod.rs, per decision #894. MCP and CLI parity is exercised.'
        concurrency:
          applicable: true
          evidence: PASS. The change adds read-only inventory checks and does not mutate shared database/task/receipt state. Generation, pagination, and snapshot admission paths are unchanged. The stored lint seam verifies database bytes remain unchanged.
        project_isolation:
          applicable: true
          evidence: PASS. Production calls use the selected workspace connection and indexed file inventory; MCP and CLI seams use temporary Workspace fixtures. SQL classification precedes path validation, and lint retains workspace-relative path guards. No linked-workspace/profile resolution or cross-project behavior changed.
        administration:
          applicable: true
          evidence: PASS. No schema, migration, profile/configuration, credentials, or delivery-policy files changed. The candidate is bound to base b4562f3; delivery and receipt are deferred to Forge's atomic task-submit path.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'DIAGNOSTIC-SCOPE-TRUTH: PATH_NOT_INDEXED means the path is outside the indexed inventory. Zero totals mean the indexed scope has no matching rows.'
    architectural_notes:
    - 'decisions: Compatibility reconciliation: acceptance5 names seven database overview aspects, but does not authorize removing adapter-owned discovery aspects already supported by src/cli/mod.rs::discovery::includes_discovery (lines60-73): discovery,manifest_components,documentation_hubs,milestones (components shared). INV-RESPONSE-CONTRACT-PRESERVATION governs this live consumer; do not rewrite the existing freshness regression or change scanner ownership. Keep these existing discovery aspect names valid through reader validation, without adding new behavior or moving composition into DB. Unsupported truly unknown aspects must still fail and list all valid names (including the required seven database names). Implement minimally within reader.rs/inputs.rs/docs already admitted; add additive case coverage to in-scope existing test if needed. No src/cli/mod.rs or freshness test edits required. Source trace: MCP overview -> adapter discovery::overview passes unchanged aspects -> reader::overview_with_options validates -> adapter includes_discovery composes existing evidence. Classified regression: new validation rejected existing supported discovery names. Mutation matrix: remove any discovery name -> existing freshness/new additive cases fail; permit unknown -> new unsupported tests fail; default/empty -> opt-in cycle preservation tests. Clippy sibling needless_as_bytes routed to dedup owner; wait for stable sibling before final gates. No acceptance/perimeter change, contract revision4 unchanged.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Apply `PATH_NOT_INDEXED: '{path}' was not found in the indexed inventory` only after SQL classification, and only when a non-empty path is absent from the file inventory. An empty analyze target remains the workspace.
2. Keep the existing mode error for unindexed targets `diagnostics` and `cycles`.
3. An indexed file with zero errors stays success. Its continuation names that file's own pages and does not send the caller to an empty `top_files` list.
4. `syntax_paged` uses `PATH_NOT_INDEXED` when the path is not in the inventory. An indexed path with no compiled-language source keeps the current compiled-profile error.
5. `code_map_overview` rejects an unsupported aspect and lists `counts`, `components`, `languages`, `coverage_by_workspace`, `cycles`, `compiled_profiles`, and `metadata`. An omitted or empty aspect list keeps the current default, including cycles remaining opt-in.
6. `code_map_analyze(target="nonexistent.rs")` is an error. `/* ; */ SELECT 1` remains the SQL task's success path.

### task: single-document-projection-for-inspection

```yaml
task_ref: single-document-projection-for-inspection
target: Emit one inspect documentation page and stop the parallel documents JSON dump
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- resolve-diagnostic-scope-ambiguity
scope:
- src/cli/mod.rs
- src/mcp/tools.rs
- src/core/format/discovery.rs
- src/db/reader.rs
- src/db/symbols.rs
- tests/mcp/context.rs
- tests/query_context/symbol_context.rs
- docs/reference/mcp-tools.md
status: completed
receipt:
  contract_revision: 5
  passed_at: 2026-10-21T20:01:10.059867751+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context && cargo test --test mcp
      exit_code: 0
      tests_passed: 134
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Candidate 3ac5222 scoped diff SHA-256 is e26add8206ce50bd139af231f261195c1e3a3a669818f910a3faa83c7c37e6b1, matching the supplied binding. Current admitted paths still match the immutable snapshot (`git diff --quiet candidate -- scope`); all eight changed paths are in initial scope and contribute to the accepted projection/adapter seam. The concurrent manifest, engine-task, and tests/acdd changes are outside the candidate and untouched.
        claims:
          applicable: true
          evidence: 'PASS. Finding #922 is introduced-regression and now confirmed-fixed. `attach_inspect_document_contents` only runs for Markdown/full-detail inspect, reuses same-page `documents` by doc_id, and fetches only missing IDs from the current bounded documented_in page. Reader uses parameter-bound `doc_id IN` values, unique ID count <= page cap, existing 8 MiB rows bound, and rejects incomplete lookup; formatter emits one body per doc_id without positional zip. The task ledger records pre-fix stdio red at limit=1/offset=0 and a full-table mutation failing on the unreferenced sentinel. Current real stdio seam covers offsets 0..2/limit=1 and limit=2, dedicated and generic inspect, content/path identity, duplicate links (3 section rows), JSON/Explain, and pagination. JSON/compact/coverage/generation/ordering and Explain projection remain unchanged.'
        concurrency:
          applicable: true
          evidence: PASS. Both inspect adapter paths execute enrichment inside `read_with_adapter` on the same admitted connection. The method holds the connection lock and shared database read lock, executes the callback within the read transaction, checks database identity before rollback, and fails/retries on generation replacement. The content lookup introduces no mutation or independent snapshot.
        project_isolation:
          applicable: true
          evidence: PASS. Lookup IDs originate only from the already workspace-scoped documented_in page and are queried on that same workspace connection. URI attachment remains the existing selected-root resolver; dedicated/generic stdio tests validate exact file URI coordinates. No path resolver, symlink policy, or workspace boundary changed.
        administration:
          applicable: true
          evidence: PASS. Candidate makes no profile, schema, migration, credential, or delivery-policy changes; contract revision 5 and initial scope remain unchanged. Candidate is bound to validated baseline eddd450d. Forge will manage the delivery receipt/commit; no manual Git operation is performed. Out-of-scope concurrent ACDD tests remain unstaged and excluded from the candidate.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
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

#### Acceptance

1. `code_map_inspect` and `code_map_query(operation="inspect")` share one `documented_in` page per cited section. Each entry keeps `doc_id`, path, section title, document type, citation line, and the `file:///` URI attached by [`attach_inspect_uris`](../../../src/cli/mod.rs#L404).
2. Remove the inspect `documents` JSON dump. Markdown has no `## documents` code fence. The coverage page stays; it is not a copy of `documented_in`.
3. Explain continues to render its `documents` relationship page and does not also receive `documented_in`.
4. Pagination and `include_coverage` stay intact.
5. A negative fixture asserts that inspect Markdown does not contain the heading `## documents` followed by a `json` fence. Matching only a block that starts with `"items"` is not sufficient, because the page object starts with `"total"`.

### task: deduplicate-task-inspection-presentation

```yaml
task_ref: deduplicate-task-inspection-presentation
target: Render equal inspect contract fields once under the task-record heading
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/core/format/lifecycle.rs
- src/core/format/mod.rs
- tests/acdd/
- docs/reference/tasks.md
- docs/reference/mcp-markdown-contracts.md
invariants:
- 'TASK-EVIDENCE-PRESERVATION: Equal scope, target, proof policy, and task invariants collapse in presentation. The contract digest hash is omitted. Milestone invariants, revisions, receipts, gates, attempts, and findings stay visible.'
status: completed
receipt:
  contract_revision: 4
  passed_at: 2026-10-21T18:26:34.056402164+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 116
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Candidate a1a374226935d75fda0fb83a3c7a70973ed8c85d based on 5541f0ba7c82fd29c7ec3e65fdeeab799f7dbdea. Six-path scoped diff SHA-256 f0c0599759e7313cfbd4eab66b29a5832d437ddcfaba2c37be2b851a2593fa7c matches build note 912 before and after verification. Candidate changes are within lifecycle.rs, tests/acdd/, and the two admitted reference pages; format/mod.rs is in scope but unchanged. Pre-existing engine/tasks/mod.rs and workspaces.rs plus out-of-scope mcp/context.rs and query_context/symbol_context.rs remain untouched.
        claims:
          applicable: true
          evidence: 'PASS. The live MCP task_manage inspect response renders ## Task record and matches core::format::render; task claim context still renders ## Contract. Equal scope, target, proof-policy, and task-invariant projections collapse only when matching. Initial/extended scopes preserve additions; unequal authority values and milestone invariants remain visible. The contract digest is omitted while revisions, receipt, gates, attempts, and findings remain. The fixture verifies every scope path once and Markdown UTF-8 byte length below compact JSON. Docs match. Historical red for the unchanged seam on base 5541f0b before implementation is preserved in raw notes 905/907 and the revision-4 amendment note 911; current candidate passes.'
        concurrency:
          applicable: true
          evidence: PASS. Changes are pure presentation over a resolved Value; no task-store writes, revision fencing, receipt handling, transactions, locks, or Git behavior changed. The ACDD suite, including lifecycle/concurrency seams, passes 116 tests.
        project_isolation:
          applicable: true
          evidence: PASS. Actual MCP linked-workspace task inspection remains exercised. The formatter performs no filesystem, workspace, project, profile, or Git access, and no out-of-scope workspace/task-engine source was changed.
        administration:
          applicable: true
          evidence: PASS. No schema, migration, profile/configuration, credentials, hooks, or privileged delivery code changed. Candidate remains based on validated HEAD 5541f0b; task delivery and receipt remain Forge-managed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'TASK-EVIDENCE-PRESERVATION: Equal scope, target, proof policy, and task invariants collapse in presentation. The contract digest hash is omitted. Milestone invariants, revisions, receipts, gates, attempts, and findings stay visible.'
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

#### Acceptance

1. Inspect Markdown keeps the heading `## Task record`. `## Contract` remains the claim-context heading.
2. When `spec.scope`, `authority_snapshot.task_scope`, `initial_scope`, and `allowed_write_scope` are equal, render that path list once. When `spec.scope` extends `initial_scope`, render the initial paths and the added paths as extended scope.
3. When `spec.target`, `authority_snapshot.task_target`, and `goal` are equal, render the target once. Do the same for equal proof-policy fields and for equal task invariant lists. Keep `authority_snapshot.milestone_invariants` visible as milestone authority.
4. On a fixture that repeats those equal fields, `markdown.len()` is strictly smaller than the JSON byte length. Do not require that inequality for a small inspect payload whose Markdown headings exceed the JSON.
5. In the equal-scope fixture, each admitted scope path occurs once in the Markdown.

### task: byte-bounded-single-symbol-snippets

```yaml
task_ref: byte-bounded-single-symbol-snippets
target: Return a complete single-symbol snippet when it fits the response byte budget instead of imposing a 500-line ceiling
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- single-document-projection-for-inspection
scope:
- src/core/response.rs
- src/db/symbols.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/mcp/response.rs
- src/cli/mod.rs
- tests/query_context/diagnostics_and_source.rs
- tests/mcp/context.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- docs/reference/mcp-tools.md
- docs/reference/configuration.md
- docs/reference/cli.md
- docs/runbooks/limits-and-freshness.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/tool-catalog.md
invariants:
- 'SINGLE-SYMBOL-BYTE-BOUND: Complete symbol source is limited by the configured serialized response budget, not an implicit 500-line ceiling. Exact source boundaries, explicit smaller requests, and byte-safe continuation remain enforced.'
status: completed
receipt:
  contract_revision: 5
  passed_at: 2026-10-21T21:56:04.242915921+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context && cargo test --test mcp
      exit_code: 0
      tests_passed: 134
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. The 14 changed paths are admitted by rev5; src/cli/mod.rs is unchanged. Candidate content remains bound to baseline 8121c1803a0177bf873fe8c122213cba8b41da58 and the scoped binary diff SHA-256 36ba88bde58fbfd1200fdf694e5e94ae35c9086e9d11cf013d6b53bf7bed0d4a. The only additional dirty path is pre-existing docs/adr/0011-bounded-mcp-response-budgets.md, excluded from the task binding and untouched. No generated/profile/credential paths changed.
        claims:
          applicable: true
          evidence: 'PASS. Server::get_code_snippet uses SourceOptions::resolve_full_symbol and snippet_paged_with_response_budget; the dedicated CompleteSymbol mode uses the active response cap, then result_for_presented trims by serialized CallToolResult size after compact/render, preserving metadata and Markdown/JSON framing. SourceOptions::resolve and Configured inspect/explain mode retain 500 lines and the 8-KiB source budget. Actual stdio tests prove exact 852-line / 33,195-byte escaped UTF-8 source fits, explicit 17-line limits, 501-line requests, 4-KiB page reconstruction, frame-size bounds, CLI characterization, oversized-line no-cursor behavior, and existing digest/generation/path rejection. Builder mutation evidence #953 records detection of restored 500-line, 8-KiB and response-budget-bypass mutations. Independent strict Clippy plus query_context 43/43 and mcp 91/91 passed; each of the three focused boundary/stdio/options tests passed 1/1.'
        concurrency:
          applicable: true
          evidence: PASS. The change remains on the read-only symbol/source path and introduces no shared mutable state, task-store writes, receipt changes, or lock-order changes. Existing generation capture/validation, source-offset requirement, adapter reader, and digest verification remain in force.
        project_isolation:
          applicable: true
          evidence: PASS. Source continues through verified_source and configured workspace/linked-workspace resolution. New boundary coverage rejects ../ traversal and symlink escape without leaking the outside sentinel; full MCP coverage exercises stale digest/missing-source failure. CLI characterization runs against the fixture workspace. Review task CLI inspect/context used --root .; Forge claim used the explicit assigned worktree.
        administration:
          applicable: true
          evidence: PASS. No migration, profile, repository config, credential, or runtime-cap change is present; the active cap remains 65,536 bytes and the separate 96-KiB work is untouched. Contract rev5 and validated baseline were preserved. Review made no source edits, manual commits, hook bypasses, or receipt edits; Forge-owned deliver will create the scoped task commit and receipt against the validated base.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'SINGLE-SYMBOL-BYTE-BOUND: Complete symbol source is limited by the configured serialized response budget, not an implicit 500-line ceiling. Exact source boundaries, explicit smaller requests, and byte-safe continuation remain enforced.'
    architectural_notes:
    - 'decisions: Blocker951 environment drift RESOLVED using preexisting supported isolated-test hook tests/mcp/skill_sync.rs:18 CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR. Root copied entire repository skill to independent owned/tmp/kilo/ux001-skill-fixture-20261021T214349, not a symlink or same sourcepath, using allowedcp-a. No ~/.agents files or securityconfig changed. Actual skill_sync::test_global_skill_matches_local_skill_when_present run with that env passed1/1; all filetreecomparisonassertions enabled. Samebuilder reruns fullregisteredClippy+query_context+mcp with envmap and records testfixture/contentbinding. No gatepassed basedonlyfocusedtest; no partialsuitecredit. Originaluserglobalmirror stillold; this lane proves isolated installationmatchescurrentcandidate, notactualglobaldeployment. Each independentreview/finalverification should create fresh fullcopyfromitsfrozencandidate and record sameisolatedhook. Repository docs prohibit directglobalwrites; installoutside task belongs dedicated distributionworkflow. No source/testchanges to hidefailure.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `get_code_snippet` for one resolved function, method, or test returns its entire body on the first request when the caller has not requested a smaller body and the serialized response fits the active byte budget, including a body longer than 500 lines. Do not silently concatenate multiple declarations or return unrelated source.
2. Remove the implicit 500-line clamp from this full-symbol request path, including input normalization and the fixed source-preview sub-budget. Explicit smaller `max_body_lines` values still bound the requested slice. Preserve configured default previews for inspect/explain unless the caller explicitly requests the full-symbol behavior.
3. Use the configured response ceiling enforced at execution time. The owner-approved 96 KiB ceiling in ADR 0011 is implemented by `response-budget-96kib-and-graceful-task-list-truncation`; this snippet task does not raise the global cap. Count escaped UTF-8 source, metadata, Markdown/JSON framing, and the existing response envelope under the same budgeting rules as other responses. Do not return an over-budget payload or discard source merely because the old 8 KiB source sub-budget was reached.
4. When the full symbol exceeds the response budget, retain bounded paging with truthful offsets, `has_more`, omitted counts, and continuation hints. Preserve oversized-single-line handling, checked offset arithmetic, indexed-digest verification, stale-generation rejection, linked-workspace confinement, and symlink/traversal rejection.
5. Extend the existing source/MCP seams with a greater-than-500-line body that fits, an explicit smaller line request, an escaped/non-ASCII body near the byte boundary, an over-budget body, and a too-large single line. Exercise CLI/MCP output and configured smaller budgets. Mutations restoring either the 500-line clamp or 8 KiB sub-budget must fail the fitting-body case; mutations bypassing the response ceiling must fail the overflow case.
6. Update the existing tool, configuration, CLI, runbook, and agent-navigation guidance in this task. The limits-and-freshness runbook distinguishes byte-bounded full-symbol `get_code_snippet` from preserved configured inspect/explain previews. Required Build/Review checks are strict all-target/all-feature Clippy and the complete `query_context` and `mcp` targets, not only the new focused case.

### task: safe-source-snippet-continuation

```yaml
task_ref: safe-source-snippet-continuation
target: Recover an omitted source continuation generation only when the prior source request proves the active snapshot is unchanged
proof_policy: seam-test-first
contract_revision: 4
depends_on:
- byte-bounded-single-symbol-snippets
scope:
- src/mcp/server.rs
- src/mcp/tools.rs
- src/mcp/response.rs
- src/mcp/tools/inputs.rs
- src/db/symbols.rs
- src/core/response.rs
- src/cli/mod.rs
- tests/mcp/context.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/query_context/diagnostics_and_source.rs
- tests/common/mcp_client.rs
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/tool-catalog.md
invariants:
- 'SOURCE-CONTINUATION-SNAPSHOT: Auto-heal never combines source pages from different snapshots, sessions, selected worktrees, databases, or symbols. Explicit generation validation remains authoritative.'
status: completed
receipt:
  contract_revision: 4
  passed_at: 2026-10-22T00:09:54.477756336+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context && cargo test --test mcp
      exit_code: 0
      tests_passed: 134
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Candidate is bound to base f2c64447c576c25de0e2fc610403ebdb951ee70d and scoped diff SHA-256 1d418441f3dadec2b68b92b918c51c713881dd46e477d4d843dec726e2e70ed0. The accepted paths are within rev4 scope; task snapshot matches the current scoped worktree. Dirty ADR0011 and milestone/compact manifest changes are excluded and preserved. No reviewer edits.
        claims:
          applicable: true
          evidence: 'PASS. F1 receipt is a private typed value derived from compacted final source metadata only after presentation/render and byte-budget success; unsuccessful, oversized, complete/no-cursor, and source-disabled results do not seed. No source/rendered-text parsing or pre-trim staging remains. F1''s contract-red JSON cursor 37, default Markdown cursor 35, and marker-spoof false-success failures are recorded in #969; #973 records the killed pre-trim and marker mutations. Independent real-stdio F1 seam passed. Explicit token authority, collection-generation separation, and 4-KiB JSON/default-Markdown trimmed-page inference are covered.'
        concurrency:
          applicable: true
          evidence: PASS. F2's production stdio seam queues both requests before receiving either, tests both submission orders, receives by response ID without FIFO assumptions, validates per-request results, and leaves the G1/G2 request ambiguous after an index change. Current finalization state is returned as a per-call tuple; no global staged generation can cross requests. Focused seam passed; full MCP suite passed.
        project_isolation:
          applicable: true
          evidence: 'PASS. Request identity still binds canonical root/database plus exact tool/request selector and selected worktree; generation and symbol digest remain checked by the ordinary reader path. New server has no session seed; foreign symbol, stale explicit generation, changed source/index, wrong cursor, source-disabled and collection-offset cases fail closed. Existing linked-worktree continuation/selection seam passed. The prior #962 mutation claims for unchanged key, reader, and CLI guards were not rerun; corresponding code is unchanged in this repair and both complete targets were rerun.'
        administration:
          applicable: true
          evidence: PASS. No profile, schema/migration, credential, or response-cap changes; runtime remains bounded by the existing 64-KiB ceiling and the 64-entry cache. Only the Forge deliver gate may write the generated receipt/commit. Verification used a fresh copied skill fixture and command-scoped CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR; no ~/.agents writes, skip, or source-path alias.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'SOURCE-CONTINUATION-SNAPSHOT: Auto-heal never combines source pages from different snapshots, sessions, selected worktrees, databases, or symbols. Explicit generation validation remains authoritative.'
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

#### Acceptance

1. For a source continuation with `source_offset > 0` and no explicit `generation`, reuse the prior successful source page's generation only when the same MCP session has an unambiguous matching request and the selected worktree's active generation still equals that captured generation. Never substitute the current hash without evidence of the previous page's snapshot.
2. Keep any inferred continuation state bounded and session-local, keyed by resolved workspace/database identity and exact symbol/request identity. Only successfully returned source pages with a retained continuation cursor seed it, using typed source metadata after final byte trimming and presentation, not a pre-trim cursor or strings found in rendered Markdown/source. Failed, oversized, unrelated, source-disabled, and no-cursor results do not seed it. Inference must not leak between parallel requests, sessions, worktrees, linked workspace sources, or selectors that resolve to different symbols. No durable task-store or code-index writes are required for this convenience.
3. An explicit generation always uses the existing validation path, including stale rejection. After an index change, unknown prior request, restart, or ambiguous cached match, fail closed with an actionable error explaining that the caller must restart at `source_offset=0` or provide the previous page's generation. A stateless CLI continuation without that proof gets the same actionable requirement rather than guessing a generation.
4. Preserve source-file digest checks and validation within the same read snapshot used for source extraction; a change between inference and reading cannot produce a mixed-version success. Missing or inferred tokens must not relax unrelated collection pagination requirements.
5. Extend existing production-seam cases for an unchanged first-page/continuation sequence with omitted generation, explicit valid and stale tokens, an intervening source/index change, a new session, different worktrees and symbols, ambiguous/failed seed requests, source-disabled requests that must not seed state, bounded-cache eviction, concurrent requests, and a stateless CLI continuation that returns the actionable token/restart error. Include a 4-KiB trimmed-page continuation without an explicit token in both Markdown and JSON, and source-body cursor-marker text that cannot establish provenance. For concurrency, send at least two requests on the same stdio transport before receiving either response, correlate their response IDs, exercise both request orders and available finalization orders without assuming FIFO replies, and check continuation state per request. Assert the chosen finite cache bound and behavior at and beyond it. Mutation checks must reject unconditional active-generation substitution, missing workspace identity, omitted generation validation, unbounded retention, pre-trim/text-marker seeding, seeding from a source-disabled result, and token-free stateless CLI continuation. Document successful inference without exposing internal cache contents.
6. This task may add only the source-continuation state to the existing MCP server owner; the deferred default-worktree selection behavior remains out of scope. Co-evolve the admitted tool/CLI/agent guidance. Run strict all-target/all-feature Clippy and complete `mcp` and `query_context` targets at Build and Review.

### task: normalize-and-resolve-milestone-references

```yaml
task_ref: normalize-and-resolve-milestone-references
target: Normalize milestone selector spellings and resolve an unambiguous task selector to its parent milestone with visible provenance
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- deduplicate-task-inspection-presentation
scope:
- src/engine/tasks/workspaces.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/blackboard.rs
- src/engine/tasks/lifecycle.rs
- src/core/format/lifecycle.rs
- src/core/format/coordination.rs
- tests/acdd/mcp.rs
- tests/acdd/tasks.rs
- tests/acdd/blackboard.rs
- docs/reference/tasks.md
- docs/reference/mcp-markdown-contracts.md
invariants:
- 'MILESTONE-REFERENCE-IDENTITY: Equivalent safe milestone spellings resolve to the same canonical manifest. Task-to-parent fallback is explicit, unambiguous, workspace-confined, and visible in both human and machine responses.'
status: completed
receipt:
  contract_revision: 5
  passed_at: 2026-10-21T21:05:04.517524845+00:00
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
          evidence: PASS. Candidate scoped diff SHA-256 18c01081e0b92d74f6d339b64932dcbc3737fb8112b540c57e3dd658c2680b84 matches supplied snapshot 396bdd6ac401abb10cc3d4e803cbc60e583e9bcb against baseline 8975f04e6f39bbe71375ed1292953c0dbcf31335. Ten scoped files changed within the 11-path initial scope; src/engine/tasks/lifecycle.rs is unchanged. Current scoped files match the snapshot and scoped diff-check is clean. Parent-owned ADR and future manifest edits remain outside candidate scope; source-snippet test paths are clean. No staged unrelated paths.
        claims:
          applicable: true
          evidence: PASS. normalize_milestone_reference rejects ParentDir components before stripping only documented logical aliases; exact confined paths pass through confined_path, then configured manifest matching supports descriptive stems, UX-number prefixes, numeric prefixes, IDs, and configured archive paths. Existing invalid manifests propagate parse errors; direct milestone resolution runs before fallback. Fallback matches full stored task IDs or unique task_ref slugs only at task list and milestone-scoped blackboard call sites; AST residual scan found exactly these two production calls, none in tests or lifecycle/destructive code. Same-workspace and all-workspace ambiguity, repo filters, unknown refs, invalid manifests, traversal/foreign roots, and direct-milestone precedence are covered. Structured resolution preserves original input and canonical task/milestone fields; Markdown emits one correction note. Blackboards preserve existing IDs/scopes/counts on read/inspect and one post writes one canonical milestone-scope message. Both admitted reference docs accurately cover task list and blackboard behavior.
        concurrency:
          applicable: true
          evidence: PASS. Task-parent discovery uses existing project-scoped TasksStore list reads and does not mutate tasks, credentials, profiles, or receipts. Blackboard reads/inspects remain read-only; post delegates to the existing serialized write path exactly once. No new lock ordering, transaction, schema, lifecycle, or receipt mutation was introduced. Existing ACDD concurrency/lifecycle seams pass.
        project_isolation:
          applicable: true
          evidence: PASS. Explicit repository selection is retained; all-workspace lookup reports ambiguity rather than choosing a task. Manifest paths and stored task-parent paths are confined to the selected workspace root; parsed repository/project must match the owning workspace. Foreign absolute roots and traversal fail closed. Linked workspace selection and CLI/MCP parity are exercised by the existing seams.
        administration:
          applicable: true
          evidence: PASS. Candidate changes no credentials, configuration, profiles, schema, migrations, or gate pins. Existing ID/output compatibility is additive; task fallback does not reach claim, submit, reset, delete, reopen, sync, or subtask lifecycle operations. Candidate is bound to validated baseline 8975f04e; Forge-managed delivery will create the one scoped task commit and receipt. Out-of-scope ADR/future manifest state is preserved.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'MILESTONE-REFERENCE-IDENTITY: Equivalent safe milestone spellings resolve to the same canonical manifest. Task-to-parent fallback is explicit, unambiguous, workspace-confined, and visible in both human and machine responses.'
    architectural_notes:
    - 'decisions: Accepted decision for revision 5: normalize safe milestone spellings in the existing resolver; retain direct milestone precedence; allow task-ID/unique-task-ref parent resolution only for task listing and milestone-scoped blackboard; publish structured provenance and one Markdown correction note. Independent REVIEW found no defects. No additional architecture, API, task-store, or schema decision was made. This candidate does not implement the separately planned 96 KiB budget; runtime behavior remains as coded in the reviewed snapshot.'
    - 'deferred: No defect or implementation work is deferred from this task. Separate `response-budget-96kib-and-graceful-task-list-truncation` work remains owned by its task/coordinator; this candidate does not implement that task. Preserve parent-owned `docs/adr/0011` and future milestone manifest edits. After this delivery, the source-snippet contract author/coordinator owns reapplying the prepared draft and rerunning its contract proof against the new HEAD before submitting Contract PASS. No review worktree source/test/docs changes were made by this reviewer.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `task_list` and milestone-scoped `task_blackboard` accept equivalent references such as `UX-001`, `UX-001.md`, `docs/milestones/UX-001`, `docs/milestones/UX-001.md`, and `milestones/UX-001.md`, even when the actual manifest has a longer descriptive filename. Accept leading slashes on these documented logical aliases without treating them as arbitrary external filesystem paths.
2. Normalize aliases in the canonical workspace milestone resolver, not separate ad-hoc CLI and MCP parsers. Preserve exact existing manifest-path lookup, configured manifest directories, milestone IDs such as `m-mcp-response-deduplication-and-agent-efficiency`, complete slugs, and existing numeric prefixes. An `m-...` value that already matches a milestone remains an ordinary milestone match, not a task fallback.
3. Validate confinement before lossy normalization. Do not turn traversal, arbitrary absolute paths, foreign workspace paths, or an existing invalid manifest into a successful basename match. Ambiguous normalized milestone names still fail with candidate information; do not choose the first match.
4. Only when milestone lookup has no match, resolve a full task ID or a task slug to its parent milestone if the task match is unique within the selected workspace set. Preserve the existing repository/workspace selection and ownership rules. Ambiguous task slugs and missing tasks remain errors; direct milestone matches always take precedence.
5. A task-to-parent fallback returns structured resolution information and a visible Markdown note such as `[auto-resolved task '<input>' to parent milestone 'UX-001']`. Include the original input and canonical task/milestone identity so the caller can correct its next request. Do not fabricate a warning for normal milestone-ID lookup or duplicate it in the response. Preserve task/blackboard result data, paging, budgets, filters, and message scope.
6. Apply fallback only to the admitted list and explicit milestone-blackboard selection surfaces, including CLI/MCP equivalents. Do not silently reinterpret task claim, reset, delete, submission, or lifecycle-admission requests. Blackboard reads/posts/inspect retain their existing scope and authorization checks and never migrate or duplicate messages as a side effect of resolution.
7. Extend existing ACDD list/blackboard seams with the spelling matrix, direct milestone ID, full task ID and unique task slug, collisions within/across selected workspaces, unknown references, archived/configured manifest paths, traversal rejection, canonical precedence, and exact warning provenance. In the existing blackboard isolation seam, snapshot existing message IDs, scopes, and counts before fallback reads/inspect and assert they remain unchanged; a fallback post creates exactly one new message in the canonical resolved scope, leaving unrelated scopes intact. Mutations skipping suffix/prefix normalization, ignoring ambiguity, omitting the warning, crossing workspace boundaries, migrating existing messages, or duplicating a fallback post must fail the applicable cases.
8. Preserve the pre-existing digest-omission changes in the task engine files. Co-evolve the admitted task and Markdown reference pages here; `document-efficient-agent-navigation` owns the shared agent-skill and tool-catalog updates after this task. Include all newly admitted UX capabilities in the final independent documentation scan. Required Build/Review proof is strict all-target/all-feature Clippy and complete `acdd` and `mcp` targets.

### task: emit-compact-document-hint-once

```yaml
task_ref: emit-compact-document-hint-once
target: Print each compact omission hint once, drop empty relation headings, and keep citation identity on referenced symbols
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- single-document-projection-for-inspection
scope:
- src/core/format/coordination.rs
- src/db/reader.rs
- tests/query_context/symbol_context.rs
- tests/mcp/freshness/stdio_and_response.rs
- docs/reference/mcp-markdown-contracts.md
status: completed
receipt:
  commit: ae26dc6aed6bb51d30a36f1f7edb7e81cdbcefdc
  contract_revision: 5
  passed_at: 2026-10-22T00:28:55.762898676+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp && cargo test --test query_context
      exit_code: 0
      tests_passed: 134
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: eeb6e35 snapshot and current postimages bind exactly the five admitted paths; recomputed scoped diff SHA-256 e85d4c201042cbb399999e737e454771387dfea7954bf7a3824c60c3844cb618. The parent-owned active milestone manifest spec edits remain untouched.'
        claims:
          applicable: true
          evidence: 'PASS: F1 is confirmed fixed. Reader keeps generic declaration coordinate line unchanged and separately returns citation_line plus citing section doc_id; the shared CLI/MCP URI adapter is unchanged. Focused query seam 1/1 and real MCP stdio seam 1/1 prove baseline line=1, matching #L1 Markdown anchors for four citations at lines 7/10/13/16, full/compact output and limit-1 pagination; full candidate evidence and details=''{}'' fallback remain. Builder closure ledger #977 records pre-fix runtime failure and restored mutations killing coordinate swap, missing doc_id, collapsed citation lines, duplicate hint, and zero-total headings.'
        concurrency:
          applicable: true
          evidence: 'PASS: change is a read-only query projection; paging, generation validation and snapshot/lock behavior are unchanged. Strict Clippy and full affected targets pass.'
        project_isolation:
          applicable: true
          evidence: 'PASS: rows remain constrained to selected section hashes; existing workspace-resolved adapter and URI path behavior are unchanged. The production stdio test exercises the actual workspace URI.'
        administration:
          applicable: true
          evidence: 'PASS: no schema, migration, profile, configuration, credentials or privileged delivery behavior changed. Reviewer is kilo-ux001-compact-reviewer, distinct from builder kilo-ux001-compact-builder. Delivery remains Forge-managed.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
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

#### Acceptance

1. `get_doc(detail="compact")` prints `Request detail=full for documentary reference candidates` once. The omission stub uses one heading.
2. Full detail still returns candidate rows. Structural parsing of `details` and `referenced_symbols` stays in place for that page.
3. Omit a relation heading when its page total is 0 and the page is not an omission stub. This task owns empty headings in `src/core/format/coordination.rs` only.
4. Referenced-symbol rows include the citing section id and citation line. Four citations of `code_map_overview` from `docs/reference/mcp-tools.md` stay four rows and are distinguishable by those fields.

### task: universal-clickable-coordinates-and-heading-compaction

```yaml
task_ref: universal-clickable-coordinates-and-heading-compaction
target: Resolve presentation coordinates before encoding file:/// URIs, and omit empty traversal headings
proof_policy: seam-test-first
contract_revision: 4
depends_on:
- emit-compact-document-hint-once
- deduplicate-task-inspection-presentation
scope:
- src/cli/mod.rs
- src/mcp/tools.rs
- src/core/format/mod.rs
- src/core/format/discovery.rs
- src/core/format/lifecycle.rs
- src/core/format/coordination.rs
- src/core/format/traversal.rs
- tests/mcp/context.rs
- tests/acdd/mcp.rs
- docs/reference/mcp-markdown-contracts.md
invariants:
- 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Resolve with the workspace path resolver, then encode with file_uri_for_path. Omit (link unavailable) for a resolvable path.'
status: completed
receipt:
  contract_revision: 4
  passed_at: 2026-10-22T03:13:26.645345579+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/kilo/ux001-f3-buildskill-final-20261022T0246Z cargo test --test acdd && CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/kilo/ux001-f3-buildskill-final-20261022T0246Z cargo test --test mcp
      exit_code: 0
      tests_passed: 303
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate is bound to the accepted scope and baseline. Scoped diff SHA-256 is 70c14c332b78e60181a8aa2581d3ca941256962add49ad1c7508fb843b45b395. Unrelated dirty ADR and future manifest fields are preserved.
        claims:
          applicable: true
          evidence: F1 semantic equality-before-presentation and F2 configured MCP output policy are independently confirmed fixed. F3 generic arbitrary path/URI promotion is removed; exact findings JSON remains literal after durable restart, while resolved task-scope links remain positive. Focused real CLI/MCP seam and mutation receipts cover every ledger case.
        concurrency:
          applicable: true
          evidence: No task transaction, claim fencing, revision, receipt, or lock semantics changed. Production submission persists transactionally, and the seam test closes/reopens the MCP client before re-inspecting SQLite-backed history.
        project_isolation:
          applicable: true
          evidence: URI attachment uses selected workspace_root and workspace path resolution. Traversal labels, foreign file/http/javascript URIs, nested objects/arrays, and forged presentation metadata remain literal after persistence and restart.
        administration:
          applicable: true
          evidence: No schema/migration/profile/config/dependency changes. Build snapshot is bound to original milestone HEAD 08a876b6. Auto-commit delivery will run through task_submit; no manual Git operation was performed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Resolve with the workspace path resolver, then encode with file_uri_for_path. Omit (link unavailable) for a resolvable path.'
    architectural_notes:
    - 'decisions: Closure-authority adjudication of#995 before edits: F1 semantic-scope equality owner task_inspection_fields andF2 response-policy owner MCPtask_manage are independentlyconfirmedfixed. F3 is pre-existing-missed in genericJSONrenderer(render_value), introducedinoriginal136bec9 andunchangedbyF1/F2repair—notcausedbylastremediation andnotincomplete-remediation-dominant round. Samefile is notsameownercluster: existing INVCLICKABLE+projectisolation authorizes only resolved trusted coordinate links, not arbitrarypersistedfindings. Thus blanket same-owner anti-loop claim is notapplicable; current user implementationauthority+same frozen rev4 writeunion authorize ONE distinct trust-boundary cluster repair. No newscope/protocol/securitypolicy: no genericdata-shape promotion; preserve arbitraryfindingsasdata andlegitresolvedscopelinks. Beforecode: freshrequiredgraph+live Submit.findings->SQLite->inspect->genericrenderer map and runtime-red regression+mutationmatrix. Matrix: direct/nested/map/array exacttwo-string objects; validlabel+foreignURI; traversallabel; file/http/javascript URI strings remainliteraldata, rawJSONunchanged; knownadapter-resolvedscope link remainspositive. Mutationrestoregenericshape-linkbranchmustfail. Cumulativeledger retainsF1/F2fixed,F3unresolveduntilredgreenmutations/fullgates+sameindependentreview. No furtherreviewadmissionwithoutcompleteledger; ifgenuineresidualincompleteclusterreturns,stop. This explicitcoordinatordirectiondoesnotalteruserauthority, scope, receipts, orscopeguards.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Close gaps in [`attach_presentation_uris`](../../../src/cli/mod.rs#L248), [`attach_document_presentation_uris`](../../../src/cli/mod.rs#L183), and [`attach_traversal_presentation_uris`](../../../src/cli/mod.rs#L285) so every Markdown coordinate those formatters display receives `uri` when [`resolve_file_path`](../../../src/core/config.rs#L456) succeeds. Pass the absolute path to [`file_uri_for_path`](../../../src/core/format/discovery.rs#L21).
2. Delete the `(link unavailable)` fallback in `src/core/format/discovery.rs`. When attachment cannot resolve a path, render the plain coordinate without that marker.
3. Coordination and traversal use the attached `uri` for `[path#Lline](file:///...)`. Their current fallbacks are inline code and `` `path:line` ``, not the discovery marker.
4. After task-record deduplication, render a resolvable inspect scope path as a `file:///` link.
5. In `src/core/format/traversal.rs`, suppress `Analysis gaps` and other empty item headings. Discovery already suppresses empty `documented_in` and `analysis_gaps`; leave that behavior. Empty coordination headings stay owned by `emit-compact-document-hint-once`.
6. Formatted Markdown for a known workspace file does not contain `(link unavailable)`.

### task: document-efficient-agent-navigation

```yaml
task_ref: document-efficient-agent-navigation
target: Document the narrowed navigation, opaque SQL, diagnostic errors, and explicit worktree selection
proof_policy: direct-proof
contract_revision: 7
depends_on:
- preserve-read-only-sql-results
- resolve-diagnostic-scope-ambiguity
- single-document-projection-for-inspection
- emit-compact-document-hint-once
- deduplicate-task-inspection-presentation
- universal-clickable-coordinates-and-heading-compaction
- normalize-and-resolve-milestone-references
- byte-bounded-single-symbol-snippets
- safe-source-snippet-continuation
scope:
- README.md
- docs/reference/mcp-tools.md
- docs/reference/tasks.md
- docs/reference/mcp-markdown-contracts.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/tool-catalog.md
- src/db/reader.rs
- tests/query_context/symbol_context.rs
invariants:
- 'DOC-NAV-READONLY-PROOF: The admitted reader and existing symbol-context test are unchanged proof anchors for the documented get_doc/inspection capability. This task edits only its documentation and skill files; source and test blobs must match the candidate baseline.'
status: completed
receipt:
  commit: 3ae3866d594c93802d21e455171d9214a68fbd51
  contract_revision: 7
  passed_at: 2026-10-22T17:12:53.436338997+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test query_context
      exit_code: 0
      tests_failed: 0
      tests_passed: 43
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All8 formally admitted paths match accepted snapshot. Only four docs files changed; source/test proof anchors identical baseline0c291f5. ScopedSHA714d35074017190f27812c9a1bc88c4ff5eabf0a47f036a85e653f51e9328786. No reviewer file edits.
        claims:
          applicable: true
          evidence: Live source confirms96default/max vs repo64, active-task selection, same-line unique-source-target governance, inspect citations vs declaration coordinates, snippet paging vs8KiB previews and explicit-generation wholelines. Caller2135/call2145 verified. Independent Clippy/query43/portablelinks1 pass; root full815pass0fail1ignored attributed separately.
        concurrency:
          applicable: true
          evidence: Docs-only correction; no runtime, source/test, transaction, revision or history semantics changed. Full coordinator gates were completed on identical runtime; no duplicate heavy tests.
        project_isolation:
          applicable: true
          evidence: Merged universal20tools routing and canonical server default remain correctly documented. No legacy no-worktree claim or active-claim fallback reintroduced. Relative links unchanged and portable fixture test passes.
        administration:
          applicable: true
          evidence: README legitimately readmitted by taskAPI rev7; no manual receipt edit. Fresh owned skillfixture, no globals, migration, profile change, code integration or history mutation. No unresolved docs findings; futurehandoff remains coordinator-owned.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'DOC-NAV-READONLY-PROOF: The admitted reader and existing symbol-context test are unchanged proof anchors for the documented get_doc/inspection capability. This task edits only its documentation and skill files; source and test blobs must match the candidate baseline.'
    architectural_notes:
    - 'decisions: {"decision":"PASS rev7 final documentation correction","candidate":"808df163991be12ad22fb7f996ac18f5a6b776e2","base":"0c291f5d5c3cdedd852fff86943af2379c72c445","scope_sha256":"714d35074017190f27812c9a1bc88c4ff5eabf0a47f036a85e653f51e9328786","findings":[],"scope":"README formally readmitted with existing7 paths; four doc changes only, no source/test changes vs current baseline. All8 scoped candidate paths match. Reader and symbol-context proof anchors unchanged; diffcheck clean.","claims":"96default/max vs configured64; unique-active list/default; governance requires unique normalized sourcepath claim on same invariant line; get_doc citation_line vs declline separated from inspect line/doc_line; caller2135/call2145 exact; snippet complete serialized paging differs from configured inspect/explain8KiB source budget; explicit tokens and wholeline/error, universal20tool merged runtime, opaqueSQL/diagnostics/history/URI remain truthful.","independent_checks":"strictClippy alltargets/allfeatures-Dwarnings; full query_context43/43; exact portable skill link1/1. Fresh /tmp/kilo/ux001-final-doc-review-skill-20261022T1710Z via env for all, no globalwrites.","coordinator_checks":"Independently inspected tool_164946bb0001ALHOry3K0QVhJe: separate commitment_integrity16, strictClippy, cargo test--all-targets815passed0failed1ignored including integrity16 once. These full gates are coordinator-run, not reviewer-run. Finaldocscan changes only docs; runtime immutable since that verified code.","remaining":"No task defects/deferred functional work. Coordinator alone performs authorized finalhandoff archival/final commit after external integration evidence. Reviewer only ordinary doc delivery then read-only final integration binding. No source edits, manualcommit, history repack, handoff/archive/cleanup, mainlanding or push."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Update the existing `skills/contextunity-forge/references/tool-catalog.md`. It is already in the authoring checkout.
2. Tell agents to reuse search previews, call `get_code_snippet` only for a range the preview does not contain, and choose `code_map_inspect` for one declaration and `code_map_explain` for relationships.
3. Document opaque SQL boundaries, preserved structural JSON, the fixture-scoped inspect byte budget, `PATH_NOT_INDEXED`, aspect validation, milestone aliases and task-to-parent resolution provenance, byte-bounded full-symbol snippets, and safe source-continuation inference.
4. Tell agents to pass `worktree` when the MCP server root is not the checkout they are editing. Leave default server-root selection to `docs/plans/universal-multi-worktree-resolution.md`.

### task: response-budget-96kib-and-graceful-task-list-truncation

```yaml
task_ref: response-budget-96kib-and-graceful-task-list-truncation
target: Increase the MCP response budget to 96 KiB and implement graceful boundary truncation with scoped task listing and subtask checkboxes
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- normalize-and-resolve-milestone-references
- safe-source-snippet-continuation
scope:
- src/core/response.rs
- src/mcp/response.rs
- src/mcp/server.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/db/symbols.rs
- src/db/tasks_store/mod.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/workspaces.rs
- src/core/format/lifecycle.rs
- src/cli/mod.rs
- src/cli/task.rs
- tests/mcp/context.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/acdd/tasks.rs
- tests/acdd/mcp.rs
- tests/acdd/milestones.rs
- tests/query_context/diagnostics_and_source.rs
- docs/adr/0011-bounded-mcp-response-budgets.md
- docs/reference/mcp-tools.md
- docs/reference/configuration.md
- docs/reference/cli.md
- docs/reference/tasks.md
- docs/reference/mcp-markdown-contracts.md
- docs/runbooks/limits-and-freshness.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/tool-catalog.md
invariants:
- 'RESPONSE-BUDGET-96KIB-CEILING: The serialized response budget ceiling is increased to 96 KiB (98,304 bytes). Payloads that fit must not be rejected or arbitrarily truncated.'
- 'GRACEFUL-BOUNDARY-TRUNCATION: Overflowing responses trim collection items (such as ''tasks'' in task_list and ''items'' in search) cleanly at item boundaries rather than failing with hard ''Output exceeds the response byte limit'' errors. A truthful pagination and continuation hint with active filters is attached.'
- 'TASK-LIST-WORKTREE-CONFINEMENT: task_list uses the existing selected CLI/server root and namespace, defaults to ready tasks in its unique active milestone, and excludes archived records from active defaults. Task-view pagination is snapshot-bound independently of the code-index generation; cancelled subtasks never appear completed.'
status: completed
receipt:
  contract_revision: 5
  passed_at: 2026-10-22T16:32:14.322984484+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test acdd && cargo test --test mcp && cargo test --test query_context
      exit_code: 0
      tests_failed: 0
      tests_passed: 345
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All27 admitted snapshot paths match candidate05abc722, baseline0d61c00. Scoped diffSHA4297479651840b28334775f4230ef940ccc29a046fb399477d8999c4d02a5e57. Production, tests, skills and ADR96 co-evolved within scope. No reviewer edits.
        claims:
          applicable: true
          evidence: 96KiB actual serialized framing/lowercaps and whole-record paging independently verified. Task cursor advances actual emitted count; snapshots bind task/subtask/filter/root not codeindex. Empty/final/oversize and CLI parity pass. Explicit-source generation/wholelines follows superseding user13:27. Independent strictClippy/lib96/acdd116/mcp90/query_context43 passed345total; full matrix and root mutation evidence documented#1014.
        concurrency:
          applicable: true
          evidence: Live TasksStore.list read transaction returns descriptors already containing atomically persisted subtasks; rendering no longer re-reads per-task table state. Existing writer transactions preserved. Changedtask/subtask and index-only continuation cases pass. No new cache/session state machine.
        project_isolation:
          applicable: true
          evidence: Shared taskDB distinct from selected request root and namespace. Universal20tools routing, adapter lowercaps and root/filter-bound task token preserved. Linkedroot and repositoryall CLI/MCP fixtures pass. Checkpoint/arbitrary JSON arrays remain opaque; source digest and path confinement unchanged.
        administration:
          applicable: true
          evidence: Approved compatible manager used for current claim. No schema migration/profile/global edits, manualcommit, repack, merge or push. Current user explicitly superseded old cache feature, historical receipts remain untouched. Fresh owned skillfixture env used for every independent check; no ~/.agents writes.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SINGLE-DOCUMENT-PROJECTION: Symbol inspection exposes one documentation page per cited section, with doc_id, path, section title, document type, citation line, and a resolved file:/// URI. Explain keeps its own documents page and does not grow a second inspect projection.'
    - 'INV-COMPACT-DOC-HINT-ONCE: A compact get_doc response emits each omitted-detail hint once. Referenced-symbol rows keep the citing section and citation line. Full detail still returns candidate evidence.'
    - 'INV-NARROWEST-NAVIGATION: Agent guidance selects the narrowest useful tool, reuses sufficient previews, and passes worktree when the server root is not the checkout being edited.'
    - 'INV-RESPONSE-CONTRACT-PRESERVATION: Deduplication preserves pagination, generation validation, detail-level behavior, response budgets, exact evidence, and CLI/MCP parity.'
    - 'INV-READ-ONLY-SQL-FIDELITY: Opaque user SQL keeps literals, comments, trailing semicolons of one statement, requested columns, and SQLite text. Structural graph rows keep parsed details and stripped internal ids. Writes and multiple statements stay rejected.'
    - 'INV-TASK-PRESENTATION-DEDUPLICATION: Inspect Markdown uses the task-record heading and renders equal scope, target, proof policy, and task invariants once. A fixture with those repeated fields is smaller in Markdown than in JSON. The contract digest hash is omitted from task responses. Revisions, receipts, gates, attempts, and findings stay visible.'
    - 'INV-EXPLICIT-DIAGNOSTIC-SCOPE: An unindexed analyze or lint target fails with PATH_NOT_INDEXED. An indexed scope with zero diagnostics stays success. The diagnostics and cycles mode errors stay specific. Unsupported overview aspects fail with the allowed set.'
    - 'INV-CLICKABLE-COORDINATE-COMPLETENESS: Presentation attachment resolves a workspace path before file_uri_for_path. A resolvable coordinate is a file:/// link. Markdown omits the (link unavailable) fallback.'
    - 'RESPONSE-BUDGET-96KIB-CEILING: The serialized response budget ceiling is increased to 96 KiB (98,304 bytes). Payloads that fit must not be rejected or arbitrarily truncated.'
    - 'GRACEFUL-BOUNDARY-TRUNCATION: Overflowing responses trim collection items (such as ''tasks'' in task_list and ''items'' in search) cleanly at item boundaries rather than failing with hard ''Output exceeds the response byte limit'' errors. A truthful pagination and continuation hint with active filters is attached.'
    - 'TASK-LIST-WORKTREE-CONFINEMENT: task_list uses the existing selected CLI/server root and namespace, defaults to ready tasks in its unique active milestone, and excludes archived records from active defaults. Task-view pagination is snapshot-bound independently of the code-index generation; cancelled subtasks never appear completed.'
    architectural_notes:
    - 'decisions: Explicit USER owner approval 2026-10-21T20:09:38Z: update ADR to resolve64KiB conflict. Architectural-cap portion of blocker#936 RESOLVED: ADR0011 decision now96KiB98304 with authoritative smaller configuredlimits and serializedUTF8metadata/framing. No protected milestone metadata changed; preservation invariant applies bounded safety, notoldnumber. Taskrev2 admitsADR+actualcli/task.rs+ACDDmcpseam+tasks/Markdownrefs/toolcatalog; source-snippettaskrev5 now usescurrentenforcedcap and leaves96implementationto this task. Explicitdependency safe-source-continuation added and duplicate generation-hint behavior replaced by preservationclause. Runtime still64untilthis taskBuild; docsreference-runtimeclaims intentionally updatedwithcode later. Independent boundedADR/scopeclosure requested sameauditor ses_ea039fd03ffer7Izn9DiQ7vPop. Other audit findings exacttasklistdefault/status/selection semantics and task-storecontinuation/trimming/boundaryproof remain Contract-author planning obligations, notpermissiontoweaken-generationorchange-deferredserverworktreeresolver. No need further userapproval foralreadyapproved96 ceiling; do not leave stale#936 capwarning blocking it.'
    - 'decisions: All original planning-audit findings closed by same independent auditor ses_ea039fd03ffer7Izn9DiQ7vPop: capauthority+missingscopesPASSinrev2, default/status/root+taskpaginationconsistency/proofPASSinrev3. Priorblocker#936 superseded, not an unresolveduserdecision. ADR0011 ownerapproved96KiB; currentruntime64untilBuild. Canonicalowner scopeincludesTasksStoremod snapshot, all7statusesreadydefault+cancelleduncheckedstatuslabel, existingselectedCLI/serverrootonly (nodeferredworktreeinference),0/1/multipleactivemilestone explicitbehavior, tasks+subtaskslogicalview tokennotcodeindexgeneration, validbounds/nextoffset actualemitted/UTF8wholeitemtrimming and oversize-singleitemboundederror. Requiredtestmatrix namedandmutations. No durablecache/schema and no profile/protectedmetadatachanges. This is VERIFIED PLANNING only; taskstillawaits normal Contract runtimeproof and predecessordelivery. Rawplanningreviewhistory preserved; noimplementationdonehere.'
    - 'decisions: {"accepted":"Rev5 candidate05abc722: 98304-byte default/max with configuredlowercaps, honest task-view paging and intact subtasks. Independent all5contours clean;345tests plus strictClippy pass. Details#1014.","explicit_user_decision":"USER13:27 supersedes former source-cache feature. No64FIFO/hash/ambiguity/inference/finalized-receipt state; every nonzero source offset requires explicit generation. Complete-line source paging or bounded actionable error replaces character-prefix output. This deliberate authority change is not a regression requiring old cache restoration; historical accepted receipt payloads stay historical.","administration":"Admitted ADR0011 now records96KiB and task-view generations independent of codeindex. Shared external schema6 needs auxiliary compatible manager; no UX schema migration performed.","next_owner":"Coordinator root owns post-delivery history repack into owning task6/7 while preserving11acceptedreceipts/onecommitpertask, followed by independent final doc/integration scans and full gates before any handoff. Reviewer performs only this task Deliver auto_commit. No pending task-level findings or deferred functional work; no merge/push/landing authority."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Implement the owner-approved 96 KiB (`98_304` bytes) default and maximum from [ADR 0011](../../adr/0011-bounded-mcp-response-budgets.md) in the canonical response policy and its MCP consumers. Configured smaller limits remain authoritative. Ensure JSON framing and Markdown responses honor this ceiling uniformly across MCP tools; budget preservation means preserving bounded serialization, not retaining the superseded numeric ceiling.
2. In `src/mcp/response.rs` and the response trimmer, trim overflowing collections at intact item boundaries, supporting the existing `items` pages and the `task_list` `tasks` array. Preserve each retained task and all its subtasks, UTF-8 validity, metadata, active filters, and snapshot token. For task pages, `pagination` reports `total`, `limit`, `offset`, `returned`, `next_offset`, `has_more`, the logical task-view `generation`, and the number of requested-page items omitted by byte trimming. `total` counts all matching tasks before slicing; `next_offset` advances by the number actually returned, not the requested limit, and is absent at the end. A continuation hint repeats the effective canonical workspace/milestone/status/detail filters and generation. If one intact requested record or the required metadata cannot fit even alone, return a bounded actionable budget error identifying the case and suggesting a narrower or compact request, rather than an empty non-advancing continuation or a partial record.
3. In `task_list` (MCP tool and CLI):
   - Scope default: Use the root and namespace already selected by CLI `--root` or the current MCP server task handler. This task does not change the deferred default-worktree resolver or infer an agent's unrelated checkout. When no milestone is supplied under the default active-milestone filter, choose its unique active milestone; no active milestone returns an empty result, and multiple active milestones return an actionable ambiguity error listing candidates instead of choosing one or leaking unrelated history. Explicit milestone selection and explicit all-milestone selection retain their existing meaning and workspace confinement.
   - Archive exclusion: Exclude archived and handed-off milestones and their tasks from active defaults unless `--all` or `--milestone-status all` is explicitly requested.
   - Pagination: Support `limit` from 1 through 100 (default 30), nonnegative checked `offset` (default 0), and an optional task-view `generation` in tool arguments and CLI options. Capture the filtered task records and rendered subtask state under one task-store read snapshot. Bind the token to that logical state, canonical workspace/milestone identity, and normalized filters, independently of any code-index generation. A continuation with `offset > 0` requires the previous task-page token; a changed logical task view or different filters rejects it with a restart hint. Index-only changes do not invalidate an unchanged task view. This requires no new durable token cache or task-store schema.
   - Filtering: Preserve the omitted-status default `ready` and every existing status value: `ready`, `in_progress`, `blocked`, `parked`, `completed`, `cancelled`, and `all`, with full CLI/MCP parity. An explicit all-task-status filter does not implicitly include archived milestones; all-milestone selection remains separately explicit.
   - Formatting: Render tasks and subtasks with Markdown checkboxes, using `- [x]` only for completed records and `- [ ]` for all other statuses. Keep explicit labels and the existing status domains: tasks include `parked`; subtasks retain `ready`, `in_progress`, `blocked`, `completed`, and `cancelled`. Do not introduce `parked` subtasks. Cancellation does not grant completion credit.
4. Apply the owner's superseding simplification decision: every nonzero source offset requires an explicit generation, with a clear restart hint. Do not restore session caches, token inference, source receipts, or character-prefix source trimming. Source pages contain complete lines only; an individually oversized line returns an actionable budget error. The expanded source/test scope captures the coordinator's predecessor simplifications for integration verification; after delivery those changes are repacked into their owning task commits, not attributed to new task-list behavior.
5. Seam tests in `tests/mcp/context.rs`, `tests/mcp/tool_evolution/mcp_boundaries.rs`, and `tests/acdd/tasks.rs` verify:
   - Serialized responses below, at, and above the 96 KiB ceiling and a configured smaller ceiling, counting escaped UTF-8 content and framing.
   - Whole-record trimming of both `tasks` and `items`, including a non-ASCII task and nested subtasks at the boundary. Assert exact filtered totals, returned and omitted counts, advancing next offset, generation, and repeated continuation filters; exercise a final page, an empty match, and one individually over-budget record without panic, invalid JSON/Markdown, or a continuation loop.
   - Default and explicit task-list pagination, limit values 0, 1, 30, 100, and 101, checked offset overflow, all existing statuses, no/one/multiple active milestones in the selected root, explicit archived/all selection, and linked-workspace confinement.
   - Valid and stale task-view continuation tokens, changed task and subtask state, changed filters, index-only changes, and a consistent task/subtask read snapshot. Mutations using a code-index token, ignoring subtask changes, or advancing by requested rather than emitted count must fail the applicable production seam.
   - Checkbox/status rendering across all seven existing task statuses and the five existing subtask statuses, with no cancellation-as-completion regression or new persisted subtask status.
6. Documentation and skills update:
   - Co-evolve `docs/reference/mcp-tools.md`, `docs/reference/cli.md`, `docs/reference/configuration.md`, and `skills/contextunity-forge/SKILL.md` to reflect the 96 KiB response ceiling, `task_list` filters and pagination parameters, and boundary truncation behavior.

## Deferred and out-of-scope defects

```yaml
deferred_defects:
  - id: server-default-worktree-root
    source: milestone-055-review
    title: Unscoped MCP calls use the server process root and can answer from a different worktree
    path: src/mcp/server.rs
    disposition: subsequent_milestone
    notes: "resolve_target_root_and_db returns self.root when worktree is omitted. Default selection belongs to docs/plans/universal-multi-worktree-resolution.md."
```
