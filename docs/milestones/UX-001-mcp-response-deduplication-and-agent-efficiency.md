---
id: m-mcp-response-deduplication-and-agent-efficiency
title: MCP response deduplication, Markdown presentation compaction, and agent call efficiency
doc_type: contract
status: active
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
---

# MCP response deduplication, Markdown presentation compaction, and agent call efficiency

## Outcome and purpose

Compact MCP and CLI Markdown without changing structural graph JSON or rejecting valid read-only SQL.

Tasks share `src/db/reader.rs` in this order: SQL fidelity, diagnostic scope, inspection projection, then compact document pages. Task-record deduplication is independent. Clickable coordinates start after the formatters they edit. The documentation task starts after every behavior task.

## Audit cases and delivery boundary

| Case | Evidence | Owning task |
| --- | --- | --- |
| **SQL statement boundaries** | [`analyze_paged`](../../src/db/reader.rs#L1649) rejects any `sql.contains(';')` and classifies a statement by the first whitespace token. `SELECT ';' AS marker` fails. A leading comment such as `/* ; */ SELECT 1` is not recognized as SQL. | `preserve-read-only-sql-results` |
| **Shared row projection** | [`rows`](../../src/db/reader.rs#L525) drops `node_hash`, `path_hash`, `path_id`, and `owner_path_id` for every caller, and JSON-decodes text in `details`, `invariants`, and `referenced_symbols`. `SELECT path_id, '{"a":1}' AS details, 'raw text' AS note` returns an object `details` and omits `path_id`. `'raw text'` already stays a string. [`document_reference_candidate_page`](../../src/db/reader.rs#L1341) and `document_diagnostics` read `details.symbol_claims` as JSON. | `preserve-read-only-sql-results` |
| **Inspect document dump** | Inspect builds `documented_in` and `documents` for the same `references_doc` edges. Markdown renders the first as rows and the second through [`render_json_section`](../../src/core/format/discovery.rs#L285) under the heading `## documents`. Pretty JSON starts with `"total"`, not `"items"`. | `single-document-projection-for-inspection` |
| **Compact hint and citations** | [`render_candidate_page`](../../src/core/format/coordination.rs#L245) prints the omission stub in page metadata and again when `items` is absent. `get_doc` referenced symbols omit the citing section and `e.line`, so distinct citations of one symbol render as identical rows. Empty relation pages still emit headings. | `emit-compact-document-hint-once` |
| **Task inspect repetition** | `task_manage(action="inspect")` uses [`render_record`](../../src/core/format/lifecycle.rs#L360) and the heading `## Task record`. `## Contract` belongs to claim context. Equal scope is stored as `spec.scope`, `authority_snapshot.task_scope`, `initial_scope`, and `allowed_write_scope`. Equal target is stored as `spec.target`, `authority_snapshot.task_target`, and `goal`. There is no `current_scope` field. | `deduplicate-task-inspection-presentation` |
| **Diagnostic scope** | A missing analyze path returns zero totals and tells the caller to pick from `top_files`. `overview_with_options` ignores unknown aspects and the JSON envelope is freshness only. Lint already rejects a missing path with a compiled-profile sentence, not `PATH_NOT_INDEXED`. Targets `diagnostics` and `cycles` have their own mode error. | `resolve-diagnostic-scope-ambiguity` |
| **Coordinates** | [`file_uri_for_path`](../../src/core/format/discovery.rs#L21) encodes an absolute path. [`attach_uri`](../../src/cli/mod.rs#L492) resolves the workspace path first. MCP Markdown calls that helper from `src/mcp/tools.rs`. The literal `(link unavailable)` exists only in `src/core/format/discovery.rs`. Coordination falls back to inline code, and traversal `coordinate` falls back to `` `path:line` ``, when `uri` is absent. | `universal-clickable-coordinates-and-heading-compaction` |
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
4. Add an opaque mode on [`rows`](../../src/db/reader.rs#L525) / [`query`](../../src/db/paging.rs#L60). User SQL uses it. The default structural mode stays.
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
target: "Emit one inspect documentation page and stop the parallel documents JSON dump"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - resolve-diagnostic-scope-ambiguity
scope:
  - src/core/format/discovery.rs
  - src/db/reader.rs
  - src/db/symbols.rs
  - tests/mcp/context.rs
  - tests/query_context/symbol_context.rs
  - docs/reference/mcp-tools.md
status: planned
```

#### Acceptance

1. `code_map_inspect` and `code_map_query(operation="inspect")` share one `documented_in` page per cited section. Each entry keeps `doc_id`, path, section title, document type, citation line, and the `file:///` URI attached by [`attach_inspect_uris`](../../src/cli/mod.rs#L404).
2. Remove the inspect `documents` JSON dump. Markdown has no `## documents` code fence. The coverage page stays; it is not a copy of `documented_in`.
3. Explain continues to render its `documents` relationship page and does not also receive `documented_in`.
4. Pagination and `include_coverage` stay intact.
5. A negative fixture asserts that inspect Markdown does not contain the heading `## documents` followed by a `json` fence. Matching only a block that starts with `"items"` is not sufficient, because the page object starts with `"total"`.

### task: emit-compact-document-hint-once

```yaml
task_ref: emit-compact-document-hint-once
target: "Print each compact omission hint once, drop empty relation headings, and keep citation identity on referenced symbols"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - single-document-projection-for-inspection
scope:
  - src/core/format/coordination.rs
  - src/db/reader.rs
  - tests/mcp/context.rs
  - docs/reference/mcp-tools.md
status: planned
```

#### Acceptance

1. `get_doc(detail="compact")` prints `Request detail=full for documentary reference candidates` once. The omission stub uses one heading.
2. Full detail still returns candidate rows. Structural parsing of `details` and `referenced_symbols` stays in place for that page.
3. Omit a relation heading when its page total is 0 and the page is not an omission stub. This task owns empty headings in `src/core/format/coordination.rs` only.
4. Referenced-symbol rows include the citing section id and citation line. Four citations of `code_map_overview` from `docs/reference/mcp-tools.md` stay four rows and are distinguishable by those fields.

### task: deduplicate-task-inspection-presentation

```yaml
task_ref: deduplicate-task-inspection-presentation
target: "Render equal inspect contract fields once under the task-record heading"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/core/format/lifecycle.rs
  - tests/acdd/
  - docs/reference/tasks.md
  - docs/reference/mcp-markdown-contracts.md
invariants:
  - "TASK-EVIDENCE-PRESERVATION: Equal scope, target, proof policy, and task invariants collapse in presentation. The contract digest hash is omitted. Milestone invariants, revisions, receipts, gates, attempts, and findings stay visible."
status: planned
```

#### Acceptance

1. Inspect Markdown keeps the heading `## Task record`. `## Contract` remains the claim-context heading.
2. When `spec.scope`, `authority_snapshot.task_scope`, `initial_scope`, and `allowed_write_scope` are equal, render that path list once. When `spec.scope` extends `initial_scope`, render the initial paths and the added paths as extended scope.
3. When `spec.target`, `authority_snapshot.task_target`, and `goal` are equal, render the target once. Do the same for equal proof-policy fields and for equal task invariant lists. Keep `authority_snapshot.milestone_invariants` visible as milestone authority.
4. On a fixture that repeats those equal fields, `markdown.len()` is strictly smaller than the JSON byte length. Do not require that inequality for a small inspect payload whose Markdown headings exceed the JSON.
5. In the equal-scope fixture, each admitted scope path occurs once in the Markdown.

### task: universal-clickable-coordinates-and-heading-compaction

```yaml
task_ref: universal-clickable-coordinates-and-heading-compaction
target: "Resolve presentation coordinates before encoding file:/// URIs, and omit empty traversal headings"
proof_policy: seam-test-first
contract_revision: 2
depends_on:
  - emit-compact-document-hint-once
  - deduplicate-task-inspection-presentation
scope:
  - src/cli/mod.rs
  - src/core/format/discovery.rs
  - src/core/format/lifecycle.rs
  - src/core/format/coordination.rs
  - src/core/format/traversal.rs
  - tests/mcp/context.rs
  - docs/reference/mcp-markdown-contracts.md
invariants:
  - "INV-CLICKABLE-COORDINATE-COMPLETENESS: Resolve with the workspace path resolver, then encode with file_uri_for_path. Omit (link unavailable) for a resolvable path."
status: planned
```

#### Acceptance

1. Close gaps in [`attach_presentation_uris`](../../src/cli/mod.rs#L248), [`attach_document_presentation_uris`](../../src/cli/mod.rs#L183), and [`attach_traversal_presentation_uris`](../../src/cli/mod.rs#L285) so every Markdown coordinate those formatters display receives `uri` when [`resolve_file_path`](../../src/core/config.rs#L456) succeeds. Pass the absolute path to [`file_uri_for_path`](../../src/core/format/discovery.rs#L21).
2. Delete the `(link unavailable)` fallback in `src/core/format/discovery.rs`. When attachment cannot resolve a path, render the plain coordinate without that marker.
3. Coordination and traversal use the attached `uri` for `[path#Lline](file:///...)`. Their current fallbacks are inline code and `` `path:line` ``, not the discovery marker.
4. After task-record deduplication, render a resolvable inspect scope path as a `file:///` link.
5. In `src/core/format/traversal.rs`, suppress `Analysis gaps` and other empty item headings. Discovery already suppresses empty `documented_in` and `analysis_gaps`; leave that behavior. Empty coordination headings stay owned by `emit-compact-document-hint-once`.
6. Formatted Markdown for a known workspace file does not contain `(link unavailable)`.

### task: document-efficient-agent-navigation

```yaml
task_ref: document-efficient-agent-navigation
target: "Document the narrowed navigation, opaque SQL, diagnostic errors, and explicit worktree selection"
proof_policy: direct-proof
contract_revision: 4
depends_on:
  - preserve-read-only-sql-results
  - resolve-diagnostic-scope-ambiguity
  - single-document-projection-for-inspection
  - emit-compact-document-hint-once
  - deduplicate-task-inspection-presentation
  - universal-clickable-coordinates-and-heading-compaction
scope:
  - docs/reference/mcp-tools.md
  - docs/reference/tasks.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/SKILL.md
  - skills/contextunity-forge/references/tool-catalog.md
status: planned
```

#### Acceptance

1. Update the existing `skills/contextunity-forge/references/tool-catalog.md`. It is already in the authoring checkout.
2. Tell agents to reuse search previews, call `get_code_snippet` only for a range the preview does not contain, and choose `code_map_inspect` for one declaration and `code_map_explain` for relationships.
3. Document opaque SQL boundaries, preserved structural JSON, the fixture-scoped inspect byte budget, `PATH_NOT_INDEXED`, and aspect validation.
4. Tell agents to pass `worktree` when the MCP server root is not the checkout they are editing. Leave default server-root selection to `docs/plans/universal-multi-worktree-resolution.md`.

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
