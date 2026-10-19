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
target: "Infer AST language from file path, format actionable diagnostic errors, and support colonless Python and parameterless Rust declarations"
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
status: planned
subtasks:
  - subtask_ref: language-inference-and-missing-hint
    title: "Make language optional in Ast input, infer from path extension, and return compiled language list on failure"
    status: pending
  - subtask_ref: actionable-diagnostic-formatting
    title: "Format AST syntax error diagnostics into readable messages with hints and valid pattern examples"
    status: pending
  - subtask_ref: colonless-python-and-rust-bodyless-declarations
    title: "Add colon prefix to Python declaration completions and parameterless () { __forge_meta_body(); } completion to Rust declarations"
    status: pending
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
target: "Fall back to all language candidate files when pattern symbol prefiltering finds no top-level indexed nodes"
proof_policy: seam-test-first
contract_revision: 3
depends_on:
  - ast-grep-language-inference-and-actionable-diagnostics
scope:
  - src/cli/ast.rs
  - tests/mcp/tool_evolution/mcp_boundaries.rs
status: planned
subtasks:
  - subtask_ref: prefilter-empty-candidate-fallback
    title: "Fall back to all files for the language when symbol name prefilter returns 0 candidate files"
    status: pending
```

#### Acceptance

1. `cli::ast::search_paged` correctly locates local/nested assignments and expressions (such as `worker = make()`) even if the identifier is not indexed as a top-level symbol in the `nodes` table.
2. Top-level declarations continue to benefit from indexed candidate narrowing when matches exist in `nodes`.
3. All AST search tests pass.

---

### task: mcp-and-cli-parameter-ergonomics

```yaml
task_ref: mcp-and-cli-parameter-ergonomics
target: "Normalize empty path filters, clamp source preview line budgets, and support common MCP parameter and CLI task aliases"
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
status: planned
subtasks:
  - subtask_ref: mcp-search-and-task-parameter-normalization
    title: "Normalize empty path: '' to None, accept query/milestone aliases in code_map_search and task_list, and support pattern: '*' when scoped by path or kind"
    status: pending
  - subtask_ref: source-preview-line-budget-clamping
    title: "Clamp out-of-range max_body_lines and leading_lines in SourceOptions::resolve instead of rejecting inspect/snippet requests"
    status: pending
  - subtask_ref: cli-task-ergonomic-aliases
    title: "Support task show alias for inspect, --task on inspect/submit, --detail on list, and --file on sync"
    status: pending
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
target: "Bound task_claim and task_manage sync/context payloads under the 64 KiB MCP response limit and eliminate write locks when opening initialized SQLite task stores"
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
status: planned
subtasks:
  - subtask_ref: bounded-task-claim-and-sync-payloads
    title: "Compact task_manage(sync) task records and bound context_bundle in task_claim/next/context so serialized responses stay within the 64 KiB MCP limit"
    status: pending
  - subtask_ref: sqlite-read-only-fast-path
    title: "Skip DDL/DML writes in TasksStore::open_project when schema_version is already initialized to prevent SQLITE_BUSY during concurrent reads"
    status: pending
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
