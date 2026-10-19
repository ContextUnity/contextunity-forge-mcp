---
id: m-agent-search-resilience-and-ast-grep-ergonomics
title: Agent search resilience, AST grep error diagnostics, and symbol boundary recovery
doc_type: contract
status: active
depends_on: []
owners:
  - src/db/symbols.rs
  - src/db/symbols/search.rs
  - src/db/tasks_store/mod.rs
  - src/core/response.rs
  - src/engine/languages/python.rs
  - src/engine/languages/rust.rs
  - src/engine/tasks/lifecycle.rs
  - src/engine/tasks/mod.rs
  - src/mcp/response.rs
  - src/mcp/tools.rs
  - src/mcp/tools/inputs.rs
  - src/cli/args.rs
  - src/cli/ast.rs
  - src/cli/task.rs
  - docs/reference/cli.md
  - docs/reference/mcp-tools.md
  - docs/reference/tasks.md
  - tests/acdd/tasks.rs
  - tests/languages/ast_patterns.rs
  - tests/mcp/tool_evolution/mcp_boundaries.rs
invariants:
  - 'INV-SEARCH-PREVIEW-RESILIENCE: Source preview failures for an individual hit (unresolved AST boundary, stale file, or digest mismatch) must never fail code_map_search; matching symbol records, file paths, line numbers, and inspect_selector hints must always be returned.'
  - 'INV-SYMBOL-WITH-SOURCE: Tree-sitter symbol boundary recovery in symbol_range must invoke profile.symbol_with_source(syntax, text) and fall back to verified indexed line..=end_line bounds for field: and template/relational nodes so code_map_inspect, code_map_explain, and get_code_snippet do not fail with unavailable boundaries.'
  - 'INV-AST-LANGUAGE-INFERENCE: When the language parameter is omitted in ast_grep_search, the engine must infer it from the file extension of path; if path is missing or lacks a recognized extension, it must return an informative error listing all compiled languages and usage examples.'
  - 'INV-AST-ACTIONABLE-DIAGNOSTICS: AST pattern search syntax errors must return structured, actionable diagnostics with actionable hints and concrete language-specific pattern examples rather than raw JSON serialization dumps.'
  - 'INV-BODYLESS-DECLARATION-PATTERNS: AST pattern normalization and completion must support bodyless Python class/function declarations without trailing colons (class Foo, def bar($$$)) and Rust function header patterns without parameter lists (fn foo).'
  - 'INV-AST-PREFILTER-SOUNDNESS: Pattern symbol candidate prefiltering in cli::ast::search_paged must fall back to all candidate files of the language if the declaration identifier is not found in the indexed top-level nodes table, preventing false negatives for local/nested constructs.'
  - 'INV-ERGONOMIC-PARAMETER-TOLERANCE: MCP and CLI entrypoints must normalize empty path strings, clamp or clearly guide out-of-range source preview line budgets, and support common parameter and subcommand aliases.'
  - 'INV-TASK-MUTATION-RESPONSE-BUDGET: State-mutating task lifecycle calls (task_claim, task_manage next/sync) must bound their serialized payload below the 64 KiB MCP response limit and avoid write transactions when opening already-initialized SQLite task stores for read operations.'
related_plans: []
started_at: 2026-10-19T00:23:28.230838525+00:00
---

# Agent search resilience, AST grep error diagnostics, and symbol boundary recovery

## Outcome and purpose

Eliminate agent-facing search failures, obscure error dumps, false negatives, parameter friction, and lifecycle response overflows discovered across Codex and Antigravity agent sessions:

1. **Symbol boundary recovery & preview resilience**: Resolve Python type aliases (`ModelPoolStrategy = Literal[...]`), module variables, `field:` nodes (`field:{path}:{line}:{qualname}`), and template/relational nodes in `symbol_range` by utilizing `profile.symbol_with_source(syntax, text)` and falling back to verified `line..=end_line` bounds. Prevent isolated preview failures from aborting `code_map_search`, ensuring symbol records and `inspect_selector` navigation hints are always returned.
2. **AST grep language inference & ergonomics**: Allow `language` to be omitted in `ast_grep_search` when inferrable from the file extension of `path`. If `language` is missing and cannot be inferred, return an informative error listing compiled languages and usage guidance.
3. **Actionable pattern diagnostics**: Format tree-sitter AST syntax diagnostics into structured, human- and agent-readable messages containing explicit hints, language-specific valid pattern examples, and JSON details.
4. **Declaration pattern flexibility**: Support bodyless Python class and function declaration patterns with or without trailing colons (`class Foo`, `def bar()`) and Rust function header patterns without parameter lists (`fn prepare_search_pattern`).
5. **Prefilter soundness**: Ensure `cli::ast::search_paged` falls back to scanning all language files when identifier prefiltering against the `nodes` table finds zero candidate files, avoiding false negatives for local assignments and expressions.
6. **MCP & CLI parameter ergonomics**: Normalize empty `path: ""` to `None`, clamp `max_body_lines` / `leading_lines` preview budgets instead of hard-failing, support `pattern: "*"` when scoped by `path` or `kind`, and accept common MCP/CLI aliases (`query` $\to$ `pattern`, `milestone` $\to$ `milestone_ref`, `task show`, `--task`, `--detail`, `--file`).
7. **Task response budget & SQLite read concurrency**: Bound `task_claim` and `task_manage(action: "next" | "sync" | "context")` payloads so state-mutating operations never fail the 64 KiB MCP response limit after committing SQLite changes, and avoid executing DDL/DML writes in `TasksStore::open_project` when the schema is already initialized.

---

## Tasks in this milestone

### task: symbol-boundary-recovery-and-preview-resilience

```yaml
task_ref: symbol-boundary-recovery-and-preview-resilience
target: Fix Python type alias, variable, field:, and template boundary recovery in symbol_range and ensure code_map_search degrades gracefully on preview failure
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/db/symbols.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
status: completed
subtasks:
- subtask_ref: graceful-preview-fallback
  title: Wrap source_preview and verified_source in code_map_search so preview failures do not abort symbol results
  status: completed
  evidence: 'Both verified_source and source_preview are guarded per search hit; real stdio MCP cases cover unresolved AST bounds, changed digest, missing source, preserved hit paths/lines/inspect_selector, and healthy later previews. cargo test --test mcp: 79 passed, 0 failed (archive 52fc6ab21f720e1d).'
- subtask_ref: symbol-with-source-and-field-dispatch
  title: Call profile.symbol_with_source in symbol_range, match field:{path}:{line}:{qualname}, and fall back to verified line..=end_line bounds
  status: completed
  evidence: 'symbol_range uses symbol_with_source, matches field qualnames, and limits indexed-line fallback to field:/template: IDs. Focused lib seam 1 passed/0 failed; full MCP 79 passed/0 failed. Python aliases/module variables keep same-line precision; multiline class/instance fields and HTML template source recover through inspect/explain/snippet. Corrected red c0d4749fe1cb25ca rejects broad ordinary-AST fallback.'
receipt:
  contract_revision: 2
  passed_at: 2026-10-19T02:03:15.068238157+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib
      exit_code: 0
      tests_passed: 65
      tests_failed: 0
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Candidate 9396ded correctly excludes AST assertions R1; candidate snapshot is immutable and cleanly builds on baseline 1924fa0cd934fa1413aa30bfcf7e22833ee0ebc1.
        claims:
          applicable: true
          evidence: INV-SEARCH-PREVIEW-RESILIENCE and INV-SYMBOL-WITH-SOURCE verified by unit seam test db::symbols::tests::test_symbol_boundary_recovery_and_preview_resilience and MCP test symbol_boundary_recovery_and_search_preview_resilience. Strict Clippy clean, 65 lib tests, 79 MCP tests pass.
        concurrency:
          applicable: true
          evidence: Reader snapshot isolation preserved; fallback in search_results_with_navigation does not mutate shared state and propagates clean failure-free preview degradation.
        paths:
          applicable: true
          evidence: 'Candidate 9396ded touches strictly the admitted scope: src/db/symbols.rs and tests/mcp/tool_evolution/mcp_boundaries.rs. Unrelated dirty changes in worktree remain unstaged and excluded from candidate snapshot.'
        project_isolation:
          applicable: true
          evidence: All inspected and searched paths are verified through verified_source against active repository root; no traversal or unverified filesystem reads.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SEARCH-PREVIEW-RESILIENCE: Source preview failures for an individual hit (unresolved AST boundary, stale file, or digest mismatch) must never fail code_map_search; matching symbol records, file paths, line numbers, and inspect_selector hints must always be returned.'
    - 'INV-SYMBOL-WITH-SOURCE: Tree-sitter symbol boundary recovery in symbol_range must invoke profile.symbol_with_source(syntax, text) and fall back to verified indexed line..=end_line bounds for field: and template/relational nodes so code_map_inspect, code_map_explain, and get_code_snippet do not fail with unavailable boundaries.'
    - 'INV-AST-LANGUAGE-INFERENCE: When the language parameter is omitted in ast_grep_search, the engine must infer it from the file extension of path; if path is missing or lacks a recognized extension, it must return an informative error listing all compiled languages and usage examples.'
    - 'INV-AST-ACTIONABLE-DIAGNOSTICS: AST pattern search syntax errors must return structured, actionable diagnostics with actionable hints and concrete language-specific pattern examples rather than raw JSON serialization dumps.'
    - 'INV-BODYLESS-DECLARATION-PATTERNS: AST pattern normalization and completion must support bodyless Python class/function declarations without trailing colons (class Foo, def bar($$$)) and Rust function header patterns without parameter lists (fn foo).'
    - 'INV-AST-PREFILTER-SOUNDNESS: Pattern symbol candidate prefiltering in cli::ast::search_paged must fall back to all candidate files of the language if the declaration identifier is not found in the indexed top-level nodes table, preventing false negatives for local/nested constructs.'
    - 'INV-ERGONOMIC-PARAMETER-TOLERANCE: MCP and CLI entrypoints must normalize empty path strings, clamp or clearly guide out-of-range source preview line budgets, and support common parameter and subcommand aliases.'
    - 'INV-TASK-MUTATION-RESPONSE-BUDGET: State-mutating task lifecycle calls (task_claim, task_manage next/sync) must bound their serialized payload below the 64 KiB MCP response limit and avoid write transactions when opening already-initialized SQLite task stores for read operations.'
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

1. `code_map_search`, `code_map_inspect(show_source=true)`, `code_map_explain(show_source=true)`, and `get_code_snippet` finding a Python type alias (`ModelPoolStrategy = Literal[...]`), module constant, `field:` node (`field:{path}:{line}:{qualname}`), or template/relational node resolve its source range in `symbol_range` without throwing `indexed symbol boundary is unavailable`.
2. Any failure in `verified_source` or `source_preview` for an individual hit is caught gracefully; `code_map_search` preserves the hit with its `id`, `kind`, `name`, `path`, `line`, and `inspect_selector`.
3. Strict Clippy passes and MCP boundary regression tests pass.

---

### task: ast-grep-language-inference-and-actionable-diagnostics

```yaml
task_ref: ast-grep-language-inference-and-actionable-diagnostics
target: Infer AST language from file path, format actionable diagnostic errors, and support colonless Python and parameterless Rust declarations
proof_policy: seam-test-first
contract_revision: 3
depends_on:
- symbol-boundary-recovery-and-preview-resilience
scope:
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/engine/languages/python.rs
- src/engine/languages/rust.rs
- docs/reference/mcp-tools.md
- tests/languages/ast_patterns.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
status: completed
subtasks:
- subtask_ref: language-inference-and-missing-hint
  title: Make language optional in Ast input, infer from path extension, and return compiled language list on failure
  status: completed
  evidence: 'cargo test --test mcp: 80 passed, 0 failed. ast_language_inference_and_actionable_diagnostics verifies omitted/null/empty language inference from .py/.rs, explicit-language workspace search, and compiled-profile guidance for missing/empty/unrecognized paths.'
- subtask_ref: actionable-diagnostic-formatting
  title: Format AST syntax error diagnostics into readable messages with hints and valid pattern examples
  status: completed
  evidence: 'cargo test --test mcp: 80 passed, 0 failed. Contract test verifies diagnostic code, one-based location, hint, each profile''s indented valid examples, and absence of raw diagnostic JSON for Python and Rust.'
- subtask_ref: colonless-python-and-rust-bodyless-declarations
  title: Add colon prefix to Python declaration completions and parameterless () { __forge_meta_body(); } completion to Rust declarations
  status: completed
  evidence: 'cargo test --lib: 66 passed; cargo test --test languages: 259 passed; cargo test --test mcp: 80 passed; 0 failed. Rust in-module seam failed before implementation with exit 101 and passes now; profile and MCP seams cover colonless Python class/functions including $$$ and Rust fn target.'
receipt:
  contract_revision: 3
  passed_at: 2026-10-19T02:25:08.589329746+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib && cargo test --test languages && cargo test --test mcp
      exit_code: 0
      tests_passed: 405
      tests_failed: 0
      log: 'Strict Clippy exit 0. Complete targets: --lib 66 passed, --test languages 259 passed, --test mcp 80 passed; zero failed/ignored. Focused unit/language/MCP contract chain also passed (3 tests). Baseline in-module Rust seam observed exit 101. git diff --check passed.'
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Candidate 2cbc625 cleanly isolates the admitted files against baseline c9d0ffd; candidate snapshot is immutable and verifiable.
        claims:
          applicable: true
          evidence: INV-AST-LANGUAGE-INFERENCE, INV-AST-ACTIONABLE-DIAGNOSTICS, INV-BODYLESS-DECLARATION-PATTERNS fully proven by in-module seam test src/engine/languages/rust.rs::test_ast_parameterless_declaration, 259 language tests, and 80 MCP tests. Strict Clippy clean.
        concurrency:
          applicable: true
          evidence: AST inference and search operations are stateless and thread-safe; error formatting does not mutate shared state.
        paths:
          applicable: true
          evidence: Candidate touches strictly the admitted scope of 7 files; uncontracted drafts remain excluded.
        project_isolation:
          applicable: true
          evidence: Path inference is scoped to workspace file extensions and recognized profiles; no traversal or unverified filesystem operations.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SEARCH-PREVIEW-RESILIENCE: Source preview failures for an individual hit (unresolved AST boundary, stale file, or digest mismatch) must never fail code_map_search; matching symbol records, file paths, line numbers, and inspect_selector hints must always be returned.'
    - 'INV-SYMBOL-WITH-SOURCE: Tree-sitter symbol boundary recovery in symbol_range must invoke profile.symbol_with_source(syntax, text) and fall back to verified indexed line..=end_line bounds for field: and template/relational nodes so code_map_inspect, code_map_explain, and get_code_snippet do not fail with unavailable boundaries.'
    - 'INV-AST-LANGUAGE-INFERENCE: When the language parameter is omitted in ast_grep_search, the engine must infer it from the file extension of path; if path is missing or lacks a recognized extension, it must return an informative error listing all compiled languages and usage examples.'
    - 'INV-AST-ACTIONABLE-DIAGNOSTICS: AST pattern search syntax errors must return structured, actionable diagnostics with actionable hints and concrete language-specific pattern examples rather than raw JSON serialization dumps.'
    - 'INV-BODYLESS-DECLARATION-PATTERNS: AST pattern normalization and completion must support bodyless Python class/function declarations without trailing colons (class Foo, def bar($$$)) and Rust function header patterns without parameter lists (fn foo).'
    - 'INV-AST-PREFILTER-SOUNDNESS: Pattern symbol candidate prefiltering in cli::ast::search_paged must fall back to all candidate files of the language if the declaration identifier is not found in the indexed top-level nodes table, preventing false negatives for local/nested constructs.'
    - 'INV-ERGONOMIC-PARAMETER-TOLERANCE: MCP and CLI entrypoints must normalize empty path strings, clamp or clearly guide out-of-range source preview line budgets, and support common parameter and subcommand aliases.'
    - 'INV-TASK-MUTATION-RESPONSE-BUDGET: State-mutating task lifecycle calls (task_claim, task_manage next/sync) must bound their serialized payload below the 64 KiB MCP response limit and avoid write transactions when opening already-initialized SQLite task stores for read operations.'
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

1. When `language` is omitted in `ast_grep_search` and `path` has a recognized extension (e.g. `path: "src/app.py"`), `language` is automatically inferred as `python`.
2. When `language` is omitted and cannot be inferred, `ast_grep_search` returns an informative error listing all compiled languages (`rust, python, typescript...`).
3. Syntax error diagnostics from tree-sitter return formatted messages with `Unsupported AST pattern`, `Hint`, `Examples of valid patterns`, and `Details`.
4. Python bodyless declaration patterns without trailing colons (`class Widget`, `def target()`) and Rust function header patterns without parameter lists (`fn prepare_search_pattern`) match successfully against declared symbols.
5. All unit and MCP boundary tests pass.

---

### task: ast-search-candidate-fallback

```yaml
task_ref: ast-search-candidate-fallback
target: Fall back to all language candidate files when pattern symbol prefiltering finds no top-level indexed nodes
proof_policy: seam-test-first
contract_revision: 3
depends_on:
- ast-grep-language-inference-and-actionable-diagnostics
scope:
- src/cli/ast.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
status: completed
subtasks:
- subtask_ref: prefilter-empty-candidate-fallback
  title: Fall back to all files for the language when symbol name prefilter returns 0 candidate files
  status: completed
  evidence: 'src/cli/ast.rs::test_prefilter_empty_candidate_fallback proves real search_paged local assignment matching with zero indexed name candidates. Sensitivity: cargo test --lib test_prefilter_empty_candidate_fallback exited 101 with fallback bypassed (0 passed, 1 failed); restored code exited 0 (1 passed, 0 failed). MCP coverage verifies all files, directory/file scope, continuation, and absent name. cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp exited 0; 81 passed, 0 failed. Logs: red 206f53e8b3f2a965, green seam 8f421e7a10336def, full checks 25f7d3cdcc546bf9.'
receipt:
  contract_revision: 3
  passed_at: 2026-10-19T02:39:02.422283382+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp
      exit_code: 0
      tests_passed: 81
      tests_failed: 0
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Candidate 2727cfc cleanly isolates admitted files against baseline 295b462; candidate snapshot is immutable.
        claims:
          applicable: true
          evidence: INV-AST-PREFILTER-SOUNDNESS verified by in-module seam test src/cli/ast.rs::test_prefilter_empty_candidate_fallback and MCP integration test ast_grep_empty_symbol_prefilter_preserves_local_matches. 81 MCP tests pass, strict Clippy clean.
        concurrency:
          applicable: true
          evidence: Search operations remain stateless read queries; empty-candidate fallback executes within reader transactions without race conditions.
        paths:
          applicable: true
          evidence: 'Candidate touches strictly the admitted scope of 2 files: src/cli/ast.rs and tests/mcp/tool_evolution/mcp_boundaries.rs.'
        project_isolation:
          applicable: true
          evidence: All path bounds and candidate file queries are strictly confined to the active repository root.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SEARCH-PREVIEW-RESILIENCE: Source preview failures for an individual hit (unresolved AST boundary, stale file, or digest mismatch) must never fail code_map_search; matching symbol records, file paths, line numbers, and inspect_selector hints must always be returned.'
    - 'INV-SYMBOL-WITH-SOURCE: Tree-sitter symbol boundary recovery in symbol_range must invoke profile.symbol_with_source(syntax, text) and fall back to verified indexed line..=end_line bounds for field: and template/relational nodes so code_map_inspect, code_map_explain, and get_code_snippet do not fail with unavailable boundaries.'
    - 'INV-AST-LANGUAGE-INFERENCE: When the language parameter is omitted in ast_grep_search, the engine must infer it from the file extension of path; if path is missing or lacks a recognized extension, it must return an informative error listing all compiled languages and usage examples.'
    - 'INV-AST-ACTIONABLE-DIAGNOSTICS: AST pattern search syntax errors must return structured, actionable diagnostics with actionable hints and concrete language-specific pattern examples rather than raw JSON serialization dumps.'
    - 'INV-BODYLESS-DECLARATION-PATTERNS: AST pattern normalization and completion must support bodyless Python class/function declarations without trailing colons (class Foo, def bar($$$)) and Rust function header patterns without parameter lists (fn foo).'
    - 'INV-AST-PREFILTER-SOUNDNESS: Pattern symbol candidate prefiltering in cli::ast::search_paged must fall back to all candidate files of the language if the declaration identifier is not found in the indexed top-level nodes table, preventing false negatives for local/nested constructs.'
    - 'INV-ERGONOMIC-PARAMETER-TOLERANCE: MCP and CLI entrypoints must normalize empty path strings, clamp or clearly guide out-of-range source preview line budgets, and support common parameter and subcommand aliases.'
    - 'INV-TASK-MUTATION-RESPONSE-BUDGET: State-mutating task lifecycle calls (task_claim, task_manage next/sync) must bound their serialized payload below the 64 KiB MCP response limit and avoid write transactions when opening already-initialized SQLite task stores for read operations.'
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

1. `cli::ast::search_paged` correctly locates local/nested assignments and expressions (such as `worker = make()`) even if the identifier is not indexed as a top-level symbol in the `nodes` table.
2. Top-level declarations continue to benefit from indexed candidate narrowing when matches exist in `nodes`.
3. All AST search tests pass.

---

### task: mcp-and-cli-parameter-ergonomics

```yaml
task_ref: mcp-and-cli-parameter-ergonomics
target: Normalize empty path filters, clamp source preview line budgets, and support common MCP parameter and CLI task aliases
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- ast-search-candidate-fallback
scope:
- src/core/response.rs
- src/db/symbols/search.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/engine/tasks/mod.rs
- src/cli/args.rs
- src/cli/task.rs
- docs/reference/cli.md
- docs/reference/mcp-tools.md
- docs/reference/tasks.md
- tests/mcp/tool_evolution/mcp_boundaries.rs
status: completed
subtasks:
- subtask_ref: mcp-search-and-task-parameter-normalization
  title: 'Normalize empty path: '''' to None, accept query/milestone aliases in code_map_search and task_list, and support pattern: ''*'' when scoped by path or kind'
  status: completed
  evidence: 'cargo test --test mcp: 84 passed, 0 failed. ergonomic_search_parameters_preserve_scope_and_paging proves query alias, empty path normalization, path/kind wildcard filtering and continuation; ergonomic_task_aliases_reach_existing_operations proves milestone alias. Strict all-feature Clippy exit 0.'
- subtask_ref: source-preview-line-budget-clamping
  title: Clamp out-of-range max_body_lines and leading_lines in SourceOptions::resolve instead of rejecting inspect/snippet requests
  status: completed
  evidence: 'Baseline cargo test --lib source_preview_line_budgets_are_clamped: exit 101, 0 passed/1 failed. Final focused command: exit 0, 1 passed. cargo test --lib: 68 passed; cargo test --test mcp: 84 passed. Three MCP source tools clamp identically; existing adapter policy boundary retains strict defaults validation.'
- subtask_ref: cli-task-ergonomic-aliases
  title: Support task show alias for inspect, --task on inspect/submit, --detail on list, and --file on sync
  status: completed
  evidence: 'cargo test --test mcp: 84 passed, 0 failed. ergonomic_task_aliases_reach_existing_operations exercises actual CLI show/inspect --task, submit --task error-path equivalence, sync --file, list --detail compact/full and conflicts. Definitions are in src/cli/task.rs; scoped args.rs does not exist. Strict all-feature Clippy exit 0.'
receipt:
  contract_revision: 1
  passed_at: 2026-10-19T03:00:01.495728803+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib source_preview_line_budgets_are_clamped && cargo test --lib && cargo test --test mcp
      exit_code: 0
      tests_passed: 153
      tests_failed: 0
      log: 'Final command exit 0. Strict Clippy all-targets/all-features: exit 0. Focused contract seam: 1 passed, 0 failed. Complete library target: 68 passed, 0 failed. Complete MCP target: 84 passed, 0 failed (includes 3 new ergonomics boundary cases and existing adapter-policy range validation). 153 passing executions across focused and target lanes, 152 unique tests. Baseline focused seam failed exit 101 before production changes. git diff --check exit 0. Actual Cargo target is mcp; generated guidance''s tool_evolution target is stale. CLI args.rs is absent; changes are solely in admitted task.rs. Clamps preserve zero-body/overflow validation and strict adapter default validation; aliases retain unknown-argument/conflicting-selector rejection.'
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Candidate 1a135fb cleanly isolates admitted files against baseline 15683fb; candidate snapshot is immutable.
        claims:
          applicable: true
          evidence: INV-ERGONOMIC-PARAMETER-TOLERANCE proven by seam test src/core/response.rs::source_preview_line_budgets_are_clamped and 3 comprehensive integration tests in mcp_boundaries.rs. 84 MCP tests pass, strict Clippy clean.
        concurrency:
          applicable: true
          evidence: Parameter normalization and clamping are purely functional and thread-safe without mutation of shared state.
        paths:
          applicable: true
          evidence: Candidate touches strictly the admitted scope of files; CLI changes are properly placed in src/cli/task.rs.
        project_isolation:
          applicable: true
          evidence: All path normalization and task operations remain strictly bounded to the active repository root.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SEARCH-PREVIEW-RESILIENCE: Source preview failures for an individual hit (unresolved AST boundary, stale file, or digest mismatch) must never fail code_map_search; matching symbol records, file paths, line numbers, and inspect_selector hints must always be returned.'
    - 'INV-SYMBOL-WITH-SOURCE: Tree-sitter symbol boundary recovery in symbol_range must invoke profile.symbol_with_source(syntax, text) and fall back to verified indexed line..=end_line bounds for field: and template/relational nodes so code_map_inspect, code_map_explain, and get_code_snippet do not fail with unavailable boundaries.'
    - 'INV-AST-LANGUAGE-INFERENCE: When the language parameter is omitted in ast_grep_search, the engine must infer it from the file extension of path; if path is missing or lacks a recognized extension, it must return an informative error listing all compiled languages and usage examples.'
    - 'INV-AST-ACTIONABLE-DIAGNOSTICS: AST pattern search syntax errors must return structured, actionable diagnostics with actionable hints and concrete language-specific pattern examples rather than raw JSON serialization dumps.'
    - 'INV-BODYLESS-DECLARATION-PATTERNS: AST pattern normalization and completion must support bodyless Python class/function declarations without trailing colons (class Foo, def bar($$$)) and Rust function header patterns without parameter lists (fn foo).'
    - 'INV-AST-PREFILTER-SOUNDNESS: Pattern symbol candidate prefiltering in cli::ast::search_paged must fall back to all candidate files of the language if the declaration identifier is not found in the indexed top-level nodes table, preventing false negatives for local/nested constructs.'
    - 'INV-ERGONOMIC-PARAMETER-TOLERANCE: MCP and CLI entrypoints must normalize empty path strings, clamp or clearly guide out-of-range source preview line budgets, and support common parameter and subcommand aliases.'
    - 'INV-TASK-MUTATION-RESPONSE-BUDGET: State-mutating task lifecycle calls (task_claim, task_manage next/sync) must bound their serialized payload below the 64 KiB MCP response limit and avoid write transactions when opening already-initialized SQLite task stores for read operations.'
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

1. `code_map_search` treats `path: ""` as `None`, accepts `query` as an alias for `pattern`, and allows `pattern: "*"` when filtered by `path` or `kind`.
2. `task_list` accepts `milestone` as an alias for `milestone_ref`.
3. `code_map_inspect` and `get_code_snippet` clamp `max_body_lines > 100` to `100` (and `leading_lines > 20` to `20`) instead of returning a validation error.
4. `contextunity-forge-mcp task` supports `task show`, `task inspect --task <ID>`, `task submit --task <ID>`, `task list --detail <LEVEL>`, and `task sync --file <PATH>`.
5. Strict Clippy and MCP/CLI boundary tests pass.

---

### task: task-response-budget-and-sqlite-read-concurrency

```yaml
task_ref: task-response-budget-and-sqlite-read-concurrency
target: Bound task_claim and task_manage sync/context payloads under the 64 KiB MCP response limit and eliminate write locks when opening initialized SQLite task stores
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- mcp-and-cli-parameter-ergonomics
scope:
- src/engine/tasks/lifecycle.rs
- src/db/tasks_store/mod.rs
- src/mcp/response.rs
- docs/reference/tasks.md
- tests/acdd/tasks.rs
status: completed
subtasks:
- subtask_ref: bounded-task-claim-and-sync-payloads
  title: Compact task_manage(sync) task records and bound context_bundle in task_claim/next/context so serialized responses stay within the 64 KiB MCP limit
  status: completed
  evidence: 'tests/acdd/tasks.rs::task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks passed; full cargo test --test acdd: 104 passed, 0 failed. Covers full/minimal claim, next/context, escape-heavy authority, 202-task sync with complete SQLite readback; compact spec.task_ref retained for MCP compatibility. Final gate archive 3fc9f67fd83c6986.'
- subtask_ref: sqlite-read-only-fast-path
  title: Skip DDL/DML writes in TasksStore::open_project when schema_version is already initialized to prevent SQLITE_BUSY during concurrent reads
  status: completed
  evidence: 'src/db/tasks_store/mod.rs::initialized_task_store_opens_during_a_write_transaction: observed red SQLITE_BUSY exit 101, now green. Supported schema skips schema writes and journal-mode writes; new initialization is atomic. cargo test --lib: 69 passed, 0 failed; strict all-targets/all-features Clippy passed. Final gate archive 3fc9f67fd83c6986.'
receipt:
  contract_revision: 1
  passed_at: 2026-10-19T03:28:23.801111878+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd && cargo test --lib
      exit_code: 0
      tests_passed: 173
      tests_failed: 0
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Candidate afa9c13 cleanly isolates admitted files against baseline b83956c; schema version 5 preserved.
        claims:
          applicable: true
          evidence: INV-TASK-MUTATION-RESPONSE-BUDGET proven by seam test src/db/tasks_store/mod.rs::initialized_task_store_opens_during_a_write_transaction and tests/acdd/tasks.rs::task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks. Strict Clippy clean, 104 ACDD + 69 lib + 84 MCP tests pass.
        concurrency:
          applicable: true
          evidence: Initialized SQLite task stores avoid write transactions during open, allowing concurrent readers during active write locks without SQLITE_BUSY.
        paths:
          applicable: true
          evidence: Changes strictly bounded to src/db/tasks_store/mod.rs, src/engine/tasks/lifecycle.rs, docs/reference/tasks.md, tests/acdd/tasks.rs.
        project_isolation:
          applicable: true
          evidence: All task bounds and SQLite transactions remain strictly scoped to workspace task database.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SEARCH-PREVIEW-RESILIENCE: Source preview failures for an individual hit (unresolved AST boundary, stale file, or digest mismatch) must never fail code_map_search; matching symbol records, file paths, line numbers, and inspect_selector hints must always be returned.'
    - 'INV-SYMBOL-WITH-SOURCE: Tree-sitter symbol boundary recovery in symbol_range must invoke profile.symbol_with_source(syntax, text) and fall back to verified indexed line..=end_line bounds for field: and template/relational nodes so code_map_inspect, code_map_explain, and get_code_snippet do not fail with unavailable boundaries.'
    - 'INV-AST-LANGUAGE-INFERENCE: When the language parameter is omitted in ast_grep_search, the engine must infer it from the file extension of path; if path is missing or lacks a recognized extension, it must return an informative error listing all compiled languages and usage examples.'
    - 'INV-AST-ACTIONABLE-DIAGNOSTICS: AST pattern search syntax errors must return structured, actionable diagnostics with actionable hints and concrete language-specific pattern examples rather than raw JSON serialization dumps.'
    - 'INV-BODYLESS-DECLARATION-PATTERNS: AST pattern normalization and completion must support bodyless Python class/function declarations without trailing colons (class Foo, def bar($$$)) and Rust function header patterns without parameter lists (fn foo).'
    - 'INV-AST-PREFILTER-SOUNDNESS: Pattern symbol candidate prefiltering in cli::ast::search_paged must fall back to all candidate files of the language if the declaration identifier is not found in the indexed top-level nodes table, preventing false negatives for local/nested constructs.'
    - 'INV-ERGONOMIC-PARAMETER-TOLERANCE: MCP and CLI entrypoints must normalize empty path strings, clamp or clearly guide out-of-range source preview line budgets, and support common parameter and subcommand aliases.'
    - 'INV-TASK-MUTATION-RESPONSE-BUDGET: State-mutating task lifecycle calls (task_claim, task_manage next/sync) must bound their serialized payload below the 64 KiB MCP response limit and avoid write transactions when opening already-initialized SQLite task stores for read operations.'
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

1. `task_claim`, `task_manage(action: "next")`, `task_manage(action: "context")`, and `task_manage(action: "sync")` remain within the 64 KiB MCP response limit even on milestones with many tasks, long contracts, or multiple historical review attempts/findings.
2. `TasksStore::open_project` does not execute `CREATE TABLE` or `INSERT OR IGNORE INTO meta` write statements when opening an already-initialized v5 `.forge/tasks.sqlite`, preventing unnecessary write locks during concurrent `task inspect` / `task list` invocations.
3. Strict Clippy and ACDD task lifecycle tests pass.

---

## Deferred and out-of-scope defects

```yaml
deferred_defects: []
```
