---
title: ACDD Profile Ergonomics, Forge Engine Remediation, and Token-Diet Overhaul
doc_type: plan
status: completed
purpose: Record empirical post-mortem findings from milestones PF-001 and LS-001 and specify parallel remediation tracks across ACDD profile configuration, Forge MCP task/index engines, and token-bounded agent responses.
depends_on: []
---

# ACDD Profile Ergonomics, Forge Engine Remediation, and Token-Diet Overhaul

Return to the [plans index](../README.md).

## 1. Empirical Post-Mortem Baseline (`PF-001` & `LS-001`)

Telemetry extracted from `.forge/command-log-debug--2026-10-30..11-03.jsonl` across milestones `PF-001` (`docs/milestones/archive/PF-001-cold-build-rss-bound-and-linker-diet.md`) and `LS-001` (`docs/milestones/archive/LS-001-rust-ecosystem-pack-and-trait-resolution.md`) establishes the following verified baseline:

- **Total tool invocations**: `3,343` calls (`2,994` MCP + `349` CLI).
- **Total serialized output**: `25,511,180` bytes (**~6.38M tokens** at 4 chars/token).
- **Total tool errors**: `354` failed calls (`10.6%` overall failure rate; `42.2%` on `mcp:task_submit`).

| Surface | Calls | Errors | Error Rate | Output Volume | Avg Output / Call |
| :--- | ---: | ---: | ---: | ---: | ---: |
| `mcp:code_map_search` | 960 | 2 | 0.2% | 12,777.5 KB (~3.20M tok) | 13,629 chars |
| `mcp:task_claim` | 192 | 9 | 4.7% | 2,839.3 KB (~710K tok) | 15,143 chars |
| `mcp:task_manage` | 183 | 34 | 18.6% | 2,752.6 KB (~688K tok) | 15,402 chars |
| `mcp:task_submit` | 251 | 106 | **42.2%** | 1,438.5 KB (~360K tok) | 5,869 chars |
| `mcp:ast_grep_search` | 572 | 84 | **14.7%** | 917.5 KB (~229K tok) | 1,642 chars |
| `mcp:task_blackboard` | 445 | 65 | **14.6%** | 609.1 KB (~152K tok) | 1,402 chars |
| `cli:task inspect` | 5 | 0 | 0.0% | 540.4 KB (~135K tok) | **110,673 chars** |

---

## 2. Verified Root Causes in ACDD Profile & Configuration

### 2.1. Missing `crates` Root in `forge-mcp.yaml` (`forge-mcp.yaml:2-5`)

`forge-mcp.yaml` lists only `roots: [src, tests, build.rs]`. After `PF-001` (`be3733967`) extracted `crates/forge-core`, `crates/forge-index`, and `crates/forge-tasks` (`208` `.rs` files + `30` `.toml` files), every cold `contextunity-forge-mcp build` indexed **0 files from `crates/`** (`353` files / `5,491` nodes / `29.02%` health instead of `593` files / `16,735` nodes / `32,298` edges / `22,336` resolved + `52,404` external in `3.74s`). Running `build` wiped `crates/**` symbols from `.forge/code-map.sqlite`, triggering `TASK_SEAM_DISCONNECTED` across `LS-001`.

### 2.2. Silently Discarded `criteria:` in `GuidanceEntry::Structured` (`crates/forge-tasks/src/tasks/profile.rs:350-363, 1171-1201`)

In `.forge/acdd/profile.yaml`, `guidance.milestone.synthesis`, `guidance.task.contract`, `guidance.task.review`, and `guidance.milestone.review` use `GuidanceEntry::Structured { description, criteria }`. However, `GuidanceEntry::text(&self)` returns only `description`, and `GateProfile::resolve_gate_guidance` calls `entry.text()`. Consequently, 100% of the structured `criteria` lists (including anti-hack, anti-duplication, and review rules) were dropped before reaching agents in `task_claim`.

### 2.3. Silently Ignored Command-Scoped `scope_args_by_pattern` (`.forge/acdd/profile.yaml:93` & `crates/forge-tasks/src/tasks/profile.rs:615, 947`)

`GateProfile.scope_args_by_pattern` is deserialized only at the top level of `GateProfile` (`profile.rs:947`). In `.forge/acdd/profile.yaml:93`, `scope_args_by_pattern` is nested under `commands.rust-check-and-test`. Because `CommandDef::Detailed` (`profile.rs:615`) uses `#[serde(untagged)]` without a `scope_args_by_pattern` field, serde silently dropped the entire repository-specific pattern map. Additionally, neither `.forge/acdd/profile.yaml` nor `acdd.default.yaml` mapped `crates/forge-core/**/*.rs`, `crates/forge-index/**/*.rs`, `crates/forge-tasks/**/*.rs`, or `tests/framework_manifests/**/*.rs`.

### 2.4. Missing Orphan-File & Dead-Shim Criteria in Crate Extraction Workflows

During `PF-001` Task 6, agents left `pub mod` wrapper directories (`src/core/tasks/mod.rs`, `src/db/tasks_store/mod.rs`, `src/engine/milestones/mod.rs`) and `92` orphaned dead source files (`src/engine/ast/`, `src/engine/scanner/`, `src/engine/languages/`, `src/engine/docs.rs`, `src/engine/linker.rs`) on disk after moving ownership to `crates/forge-index` and `crates/forge-tasks`. Because `contract_synthesis` and `review` lacked a mandatory `mod` reachability / orphan-file check and because `criteria` were dropped by `GuidanceEntry::text()`, reviewers instead rejected tasks over doc-comment bikeshedding (`23` review rejections across `PF-001` and `LS-001`).

---

## 3. Verified Root Causes in Forge MCP Engine & CLI

### 3.1. `task_blackboard` Gate Validation & Task Resolution Bugs (`crates/forge-tasks/src/engine/blackboard.rs:146-215, 427-485`)

1. **Nested Gate Rejection (`TASK_BLACKBOARD_SCHEMA_INVALID`)**: `blackboard_schema_hint` (`blackboard.rs:427`) and `validate_blackboard_schema` (`blackboard.rs:473`) iterate `profile.gates.iter()` instead of `profile.executable_gates()`. When a profile nests task gates under `- tasks:` (as `.forge/acdd/profile.yaml` does for `contract`, `build`, `review`, `deliver`), `profile.gates` contains a container entry with `id: ""`, causing valid posts like `gate: "build"` or `gate: "review"` to fail with `TASK_BLACKBOARD_SCHEMA_INVALID`.
2. **Ambiguous Task Resolution in Multi-Task Worktrees (`TASK_BLACKBOARD_AMBIGUOUS_TASK`)**: When `task_id` is omitted in a milestone worktree where multiple tasks were claimed over time, `resolve_blackboard_target` fails with `TASK_BLACKBOARD_AMBIGUOUS_TASK` (`21` errors) even when only one task has `status == "in_progress"`.
3. **Action Alias Friction**: Agents invoked `action: "get"` or `action: "list"` (`12` errors) expecting read semantics.

### 3.2. `TASK_SEAM_DISCONNECTED` & `TASK_SEAM_STALE` False Positives (`crates/forge-tasks/src/engine/gates.rs:1300-1475`)

1. **Hop Depth & Declarative Scope Targets**: `MAX_SEAM_HOPS = 4` (`gates.rs:1374`) rejects legitimate integration test harnesses that traverse 5–8 hops to reach crate internals, and rejects declarative manifest tasks whose non-test scope consists solely of `.toml` / `.yaml` files (forcing agents in `LS-001` to insert dummy `rust::kind()` calls and append unrelated assertions into `test_rust_ast_extractor`).
2. **Stale Line Numbers on Edited Files**: All deliberate staleness and unindexed-seam checks in `tests/acdd/tasks/seams.rs` must remain strictly enforced, while `MAX_SEAM_HOPS` is raised to `8` and declarative manifest scopes (`.toml`/`.yaml` with no `.rs` production files in scope) are recognized when the seam test exercises the manifest loader/extractor pipeline.

### 3.3. `task_claim` / `task_manage` Self-Inflicted `byte_limited` Truncation (`crates/forge-tasks/src/engine/context.rs:920-1146`, `lifecycle.rs:49-114`, `src/core/format/lifecycle.rs:262-375`)

1. **Multi-Copy JSON Bloat Before `bound_task_response`**: `context_bundle` (`context.rs:1051-1145`) inserts `workflow_guidance` twice (`/workflow_guidance` and `/context_bundle/guidance`), `blackboard` twice (`/blackboard` and `/context_bundle/blackboard`), `linked_milestones` three times, and leaves `/authority_snapshot` in the root object. This inflates the intermediate JSON `Value` past `TASK_RESPONSE_BYTES` (`24 KiB`).
2. **Destructive Field Dropping**: `bound_task_response` (`lifecycle.rs:55-90`) removes whichever field is largest in bytes without distinguishing redundant duplicate copies from primary guidance. As a result, `/blackboard`, `/applicable_invariants`, and `/workflow_guidance/scoped_commands` are deleted and replaced with `byte_limited: true` and `retrieval_hint: "use CLI task inspect <task_id>"`, which dumps `110 KB` into the agent's context.
3. **Markdown Presentation Duplication**: `src/core/format/lifecycle.rs` renders both `steps` and `actionable_steps` even when identical, and renders `initial_scope`, `breadcrumb`, `active_gate_path`, `active_stage_id`, and `subagent_role` under `## Additional task data` even when identical to fields already rendered in `## Contract` and `## Stage guidance`.

### 3.4. `milestone handoff` 8-Minute Duplicate Verification & 1.4 MB Archive YAML (`crates/forge-tasks/src/milestones/handoff.rs:2175-2415`, `integration.rs:145-183`)

1. **Duplicate 8-Minute `check_full_repo` Execution**: `handoff-context` (`handoff.rs:848`) runs `absorb_polish()`, creating a commit with trailer `Forge-Handoff-Source-Polish`. Even after the coordinator runs `check_full_repo` on that exact `HEAD` commit and records the passing result in `.forge/integration-evidence.json`, `telemetry_passport` (`handoff.rs:2396`) sees `source_polish(root, item)? == true` and unconditionally spawns `check_full_repo` a second time via `integration::observe_command`.
2. **Unbounded `ast_symbol_deltas` & Subprocess Storm**: `telemetry_passport` (`handoff.rs:2325-2326`) spawns `2–4` sequential `git show` / `git cat-file` processes per changed file and serializes every added, removed, and modified AST symbol into the milestone Markdown YAML frontmatter (`PF-001` archive grew to `13,674` lines / `1.4 MB`).
3. **Missing `crates/` Classification in Diff Breakdown**: `handoff.rs:2315` checks `path.starts_with("src/")` but not `path.starts_with("crates/")`, misclassifying all workspace crate changes as `"other"`.
4. **Sequential `git update-ref -d` Loop**: `cleanup_handoff_refs` (`handoff.rs:2175-2184`) and `prune_snapshot_refs` (`handoff.rs:2835`) spawn one `git update-ref -d` process per ref instead of batching via `git update-ref --stdin`.

### 3.5. `ast_grep_search` & `code_map_search` Ergonomics and Precision (`crates/forge-index/src/ast/search.rs:352-590`, `crates/forge-index/src/db/symbols/search.rs:262-271`, `src/cli/ast.rs`, `src/mcp/tools.rs`)

1. **`unsupported_ast_pattern` on `{ $$$ }` Declarations (`43` errors)**: `ast/search.rs` replaces `$$$` with `__FORGE_MANY_`. Inside `struct Foo { $$$ }`, `impl Foo { $$$ }`, or `trait Foo { $$$ }`, `{ __FORGE_MANY_ }` is a Tree-sitter syntax error because `field_declaration_list` and `declaration_list` reject bare identifiers. Supporting `{ $$$ }` / `{ $$$IDENT }` -> `{}` normalization for `PatternMode::Declaration` (which sets `omit_declaration_body = true`) and `;` in `declaration_completions` allows `pub struct Foo { $$$ }`, `impl Foo { $$$ }`, `pub trait Foo { $$$ }`, `let x = $$$`, and `mod rust` to match cleanly while preserving `unsupported_ast_pattern` diagnostics on genuinely invalid syntax (`def (`, `function (`, `let = ;`).
2. **`Cannot infer AST language` (`41` errors)**: `tests/mcp/tool_evolution/mcp_boundaries.rs:1386` enforces that `ast_grep_search` rejects requests without `language` when `path` lacks a recognized file extension. However, the MCP input schema (`src/mcp/tools/inputs.rs`) and ACDD profile guidance do not prominently warn agents that `language` is mandatory whenever `path` is omitted or a directory. Strengthening the `Ast.language` schema description and ACDD tool guidance eliminates this guesswork while preserving the `mcp_boundaries.rs` contract.
3. **`code_map_search` Multi-Token `OR` Flood (`3.2M` tokens)**: `db/symbols/search.rs:262-271` splits multi-token queries (e.g. `TASK_CANDIDATE_MISMATCH` or `omitted_fields`) on non-alphanumeric separators and joins them with `" OR "`, returning hundreds of weak single-token matches (`392` hits for `TASK_CANDIDATE_MISMATCH`). Querying FTS with `" AND "` across tokens first and falling back to `" OR "` only when `" AND "` yields fewer than `limit` (or `0`) matches restores high-precision ranking while preserving recall.

---

## 4. Disjoint Parallel Remediation Tracks

To remediate all findings without merge conflicts, work is partitioned into four strictly disjoint write scopes executed in parallel by Codex Sol 6.1 builder subagents:

| Track | Subagent Scope (Disjoint Files) | Key Deliverables |
| :--- | :--- | :--- |
| **Track 1: Profile, Config & Guidance** | `forge-mcp.yaml`, `.forge/acdd/profile.yaml`, `crates/forge-tasks/src/tasks/acdd.default.yaml`, `crates/forge-tasks/src/tasks/profile.rs` | Add `crates` to `forge-mcp.yaml` `roots`; render `criteria` in `GuidanceEntry::text` / `resolve_gate_guidance`; support `scope_args_by_pattern` on `CommandDef::Detailed` and merge with top-level patterns (including `crates/**` and `tests/framework_manifests/**`); add orphan-file/shim checks and anti-bikeshedding review rules to `.forge/acdd/profile.yaml` and `acdd.default.yaml`. |
| **Track 2: Blackboard, Seam Gate & Claim Context Diet** | `crates/forge-tasks/src/engine/blackboard.rs`, `crates/forge-tasks/src/engine/gates.rs`, `crates/forge-tasks/src/engine/context.rs`, `crates/forge-tasks/src/engine/lifecycle.rs`, `src/core/format/lifecycle.rs` | Fix `blackboard.rs` to validate gates via `executable_gates()`, accept `get`/`list` read aliases, and prefer the single `in_progress` task on ambiguity; raise `MAX_SEAM_HOPS` to `8` and support declarative `.toml`/`.yaml`-only scopes in `gates.rs`; prioritize dropping duplicate JSON fields first in `bound_task_response` so `/workflow_guidance/scoped_commands` is preserved, and deduplicate `actionable_steps` / `initial_scope` in `src/core/format/lifecycle.rs`. |
| **Track 3: Milestone Handoff Performance & Archive Diet** | `crates/forge-tasks/src/milestones/handoff.rs`, `crates/forge-tasks/src/milestones/integration.rs` | Reuse verified `evidence.verification` in `observe_command` when `evidence.verification.command` already executed the exact registered `project_command` on the bound commit; classify `crates/` as `"src"` in diff breakdown; skip AST extraction on non-source files and cap `ast_symbol_deltas` arrays (`<= 200` items with `total_*` counts) to prevent multi-megabyte YAML frontmatter; batch ref deletions with `git update-ref --stdin`. |
| **Track 4: AST Pattern Normalization & Search Precision** | `crates/forge-index/src/ast/search.rs`, `crates/forge-index/src/db/symbols/search.rs`, `src/mcp/tools/inputs.rs` | Normalize `{ $$$ }` / `{ $$$IDENT }` declaration bodies to `{}` fallback and add `;` to `declaration_completions` in `ast/search.rs`; clarify mandatory `language` semantics in `src/mcp/tools/inputs.rs` (`Ast`); run `AND`-first with `OR` fallback on multi-token symbol queries and boost `AND` matches on natural-language `OR` queries in `db/symbols/search.rs`. |

---

## 5. Verification & Review Protocol

1. **Automated Test Gates**:
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test --test commitment_integrity`
   - `cargo test --test acdd`
   - `cargo test --test languages`
   - `cargo test --test mcp`
   - `cargo test --test query_context`
   - `cargo test --test framework_manifests`
2. **Live Index Rebuild**:
   - Build `target/release/contextunity-forge-mcp` and run `./target/release/contextunity-forge-mcp build` to verify all `593` workspace files (including `crates/`) are indexed in `.forge/code-map.sqlite`.
3. **Independent Codex Sol 6.1 Review (`xhigh`)**:
   - Dispatch an independent `codex exec -c model="gpt-6.1-sol" -c model_reasoning_effort="xhigh"` reviewer over the plan and full diff, and close any findings before final report.

## 6. Phase 2: Documentation Intersection, MCP Parameter Unification, First-Class SQL & Skill Refactoring

1. **Multi-Term `search_docs` Ranking (`crates/forge-index/src/db/reader.rs`)**:
   - Prioritize sections matching all query terms (`fts_and_matches` CTE with `" AND "`) ahead of partial `" OR "` matches while preserving `bm25(doc_search, 3.0, 1.0, 1.0)` scores, excerpts, and pagination.
2. **Bidirectional Code-Documentation Context (`crates/forge-tasks/src/engine/context.rs`)**:
   - Traverse both `governs` and `documents` edges in `query_indexed_adrs` (including per-section `"Section: <title>"` summaries when a directly linked section has no `Decision:`/`INV-` lines).
   - Attach `"documented_by": ["path#section_title", ...]` to each symbol in `scope_context.symbols` (`query_scope_symbols`).
3. **Cross-Gate Blackboard Retention & Module-Filtered Recommended Tools (`crates/forge-tasks/src/engine/context.rs`)**:
   - Retain active blackboard entries with `topic IN ('decisions', 'findings', 'blockers')` across gate transitions (`contract` -> `build`/`implement` -> `review` -> `deliver`), preserving their original gate attribute.
   - Populate default advisory `recommended_tools` (`code_map_overview`, `code_map_search`, `code_map_inspect`, `get_code_snippet`, `search_docs`, `get_doc`, `task_blackboard`) filtered by active `profile.modules` (`filter_subsystem_tools`) when a gate leaves `tools: []` empty, keeping `acdd.default.yaml` compatible with `profile: minimal` and `profile: orchestrator`.
4. **Unified MCP Call Parameters & Published JSON Schemas (`src/mcp/tools/inputs.rs`, `src/mcp/tools.rs`, `src/cli/mod.rs`, `src/cli/guide.rs`)**:
   - Expose explicit optional alias properties in `tools/list` JSON Schemas (`schemars`) without requiring the canonical field when an alias is supplied, while rejecting conflicting duplicate values:
     - `path` <-> `component` (`code_map_overview`, `code_map_search`, `ast_grep_search`, `search_docs`)
     - `pattern` <-> `query` (`code_map_search`, `ast_grep_search`, `search_docs`)
     - `path_or_id` <-> `path` <-> `selector` (`get_doc`)
     - `target` <-> `path` <-> `selector` (`code_map_analyze`)
     - `target` <-> `symbol` <-> `path` (`code_map_prove_removal`)
   - Support first-class SQL in `code_map_query` via `sql: "SELECT ..."`, `query: "SELECT ..."`, or `operation: "SELECT ..."` (detecting `SELECT`/`WITH` across spaces, tabs, and CRLF via `looks_like_sql`), plus CLI `query sql "SELECT ..."` and `query run "SELECT ..."`.
   - Remove duplicate legacy operations (`overview`, `inspect`, `explain`, `impact`) from advertised `Query` schemas and `forge_guide`, retaining the 5 unique operations (`sql`, `untested`, `implementations`, `slice`, `unwired`).
5. **Skill Refactoring & Global Installer (`skills/contextunity-forge/SKILL.md`, `skills/contextunity-forge/references/acdd_profile.yaml.example`, `skills/install.sh`)**:
   - Focus `SKILL.md` and `tool-catalog.md` on the 20 MCP tools, unified parameter table, SQL schema joins, and context budget discipline; delegate workflow sequence to `workflow_guidance` from `task_claim` and reference `acdd_profile.yaml.example` (updated to canonical `version: 2`).
   - Provide `skills/install.sh` to sync bundled skills into `~/.agents/skills` and link/update `~/.gemini/config/skills`, `~/.codex/skills`, `~/.claude/skills`, and `~/.cursor/skills`.

---

## 7. Phase 3: Unified Documentation Scope, SQL Schema Hints, Anti-Spoofing & Custom Role Runners

### 7.1. Unified Documentation Scope & Milestone/Archive Exclusion (`crates/forge-index`, `crates/forge-tasks`, `docs/adr/`)
- **Problem**:
  - `480` of `1,097` `doc_sections` (`44%`) and `~1,450` of `1,521` `invariant` nodes (`95%`) originated from `docs/milestones/archive/**`, polluting `search_docs`, `documented_in`, `affected_docs`, and invariant governance.
  - Meanwhile, `query_indexed_adrs` in `crates/forge-tasks/src/engine/context.rs` previously hardcoded `docs/adr/%`, `docs/architecture/%`, and `docs/reference/%`, ignoring `README.md`, `AGENTS.md`, `TESTS.md`, `docs/testing/**`, `docs/runbooks/**`, and `docs/plans/**`.
- **Unified Rule Across Both Crates**:
  1. **Exclude Archives by Default (`archive/%` and `%/archive/%`)**: Archived documents (`docs/milestones/archive/**`, `docs/plans/archive/**`, or `**/archive/**`) are excluded from default `search_docs`, `documentation_hubs`, `phase_documentation` (`documents`/`references_doc`/`governs`/`governed_by` edges), `affected_docs`, and task context (`query_indexed_adrs`, `query_scope_symbols`).
  2. **Semantic `doc_type != 'contract'` Rule in Task Context (`crates/forge-tasks/src/engine/context.rs`, `crates/forge-index/src/docs.rs`)**: Instead of hardcoded directory lists or path exclusions, `docs/milestones/**` and `docs/contracts/**` default to `doc_type = "contract"` when frontmatter omits `doc_type`, and `query_indexed_adrs` / `query_scope_symbols` filter by `d.doc_type != 'contract'` (plus `archive/` exclusion), matching all non-contract documentation (`README.md`, `AGENTS.md`, `TESTS.md`, `docs/adr/**`, `docs/architecture/**`, `docs/reference/**`, `docs/testing/**`, `docs/runbooks/**`, `docs/plans/**`, etc.) while keeping contracts in `context_bundle.contract` and `context_bundle.governing_invariants`.
  3. **Relaxed `governs` Resolution & Link Repair**: `phase_documentation` resolves `governs` / `governed_by` edges across the full line span of an `INV-*` node (`node.line..=node.end_line`) using the same unique workspace symbol resolution tier as `documents` edges when `explicit_path` is false, and all stale pre-crate-split links across `docs/` (`../../src/engine/...` -> `../../crates/forge-index/src/...`) are repaired.
  4. **ADR Governance**: Recorded in [ADR 0016](../../adr/0016-lean-stage-tailored-task-context-bundles.md), [ADR 0019](../../adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md), and [ADR 0020](../../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md).

### 7.2. Compact SQL Schema Hints (`crates/forge-index/src/db/reader.rs`, `src/cli/guide.rs`, `src/mcp/tools/inputs.rs`)
- Preserve the compact integer-keyed SQLite schema (`nodes.path_id -> path_dictionary.path_id`, `edges.src_hash/dst_hash -> nodes.node_hash`, zero SQL views per `tests/core/compact_schema.rs`).
- Enrich SQLite `no such column` / `no such table` errors in `analyze_paged` with an inline schema hint and document the exact column and `JOIN` patterns in `forge_guide(topic="query")`, `Query` MCP schema descriptions, and `SKILL.md`.

### 7.3. Universal Anti-Spoofing at `task_claim` & `task_submit` (`crates/forge-tasks/src/store/lifecycle.rs`, `crates/forge-tasks/src/engine/gates.rs`)
- **Fail-Fast Independence on `task_claim`**: Enforce `gate.independent_from` directly in `ClaimContext::claim` (not only on `task_submit`), rejecting attempts by the builder `worker_id` to claim `review`.
- **Session Fingerprint Binding (`session_fingerprint`)**: Persist `session_fingerprint` on `task_claims` (derived from `FORGE_AGENT_SESSION_ID`, `CODEX_THREAD_ID`, `AGY_CONVERSATION_ID`, or `CLAUDE_SESSION_ID`). When an independent gate (`review`) is claimed from the same non-empty `session_fingerprint` as the gate it is `independent_from` (`build`/`implement`), reject at `task_claim` with `TASK_REVIEW_NOT_INDEPENDENT` even if the caller changed `worker_id`.
- **Remediation Guidance**: Replace the misleading `"Do: submit review with a worker_id other than..."` hint in `gates.rs` with instructions to dispatch a separate reviewer subagent session.

### 7.4. Optional Custom Role `runner` & `prompt_template` (`crates/forge-tasks/src/tasks/profile.rs`, `crates/forge-tasks/src/engine/context.rs`)
- **Schema & Default Profile Boundary**:
  - `RoleDef` supports optional `runner: Option<String>` and `prompt_template: Option<String>`.
  - **Embedded default profile (`crates/forge-tasks/src/tasks/acdd.default.yaml`) MUST NOT contain `runner` or `prompt_template`** so the default profile remains runtime- and client-agnostic.
  - **Custom profiles (`.forge/acdd/profile.yaml`, `skills/contextunity-forge/references/acdd_profile.yaml.example`, and `acdd-profile-examples/`)** may configure `runner` and `prompt_template` per role to bind roles to arbitrary CLI agent clients (`codex exec`, `claude -p`, `gemini`, etc.) with on-the-fly prompt interpolation (`{task_id}`, `{stage}`, `{role}`, `{worktree}`, `{model}`, `{reasoning}`, `{role_guidance}`, `{goal}`) exposed in `workflow_guidance.role_runner`.

### 7.5. Milestone Handoff Telemetry Compaction & Archive De-bloat (`crates/forge-tasks/src/milestones/handoff.rs`, `docs/milestones/archive/`)
- **Compact `handoff.passport.diff`**: Instead of dumping unbounded per-file objects `{path, added, removed}` across all files, aggregate 4 compact category summaries `{files, added, removed}` for `"src"`, `"tests"`, `"docs"`, `"other"`.
- **Compact `handoff.passport.ast_symbol_deltas`**: Instead of dumping arrays of up to 600 raw symbol identifier strings, emit compact count aggregates `{added: usize, removed: usize, modified: usize}`.
- **Archive De-bloat**: De-bloated 5 existing archived milestones in `docs/milestones/archive/` (`PF-001`, `LS-001`, `AC-001`, `AC-002`, `LS-002`), removing **18,818 lines net** of raw file diff and symbol string dumps, bringing total archive size down from 42,903 to 24,085 lines (44% reduction) while preserving 100% YAML frontmatter validity and byte-identical Markdown bodies.
