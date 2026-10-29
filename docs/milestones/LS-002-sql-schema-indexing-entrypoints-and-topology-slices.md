---
id: m-sql-schema-indexing-entrypoints-and-topology-slices
title: SQL schema and query indexing, deterministic entrypoint classification, 1-hop directory boundary slices, and module topology
doc_type: contract
status: active
depends_on:
- m-declarative-framework-manifests-and-ecosystem-packs
guidance:
  hitl: pro
concurrency:
  mode: speculative
  sync_on: snapshot
owners:
- src/engine/languages/
- src/engine/linker/
- src/db/reader.rs
- src/db/traversal.rs
- src/db/ingest.rs
- src/db/symbols/
- src/mcp/tools.rs
- src/cli/mod.rs
- docs/architecture/
- docs/reference/
- skills/contextunity-forge/
required_skills:
- contextunity-forge
- agent-instructions
invariants:
- 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
- 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
- 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
- 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-20T11:54:32+00:00
---

# SQL schema and query indexing, deterministic entrypoint classification, 1-hop directory boundary slices, and module topology

## Outcome and purpose

Extend ContextUnity Forge MCP's polyglot semantic graph into database schemas, application entrypoints, and architectural module boundaries:

1. **SQL Schema, Migration & Query Graph (`src/engine/languages/sql.rs` & ORM Linking):**
   Index `.sql` files (schemas, migrations, queries) and embedded SQL strings in Rust (`rusqlite`/`sqlx`), Python (`cursor.execute`/`raw`), and TypeScript (`prisma.$queryRaw`/`knex`) to extract `table`, `view`, and `column` nodes (`CREATE TABLE`, `ALTER TABLE`, `CREATE VIEW`) and link application functions and ORM models (`__tablename__`, `Meta.db_table`, `#[sqlx::FromRow]`) to SQL tables via `defines_table`, `queries_table`, and `mutates_table` edges.

2. **Deterministic Entrypoint Classification & `unwired` Precision:**
   Automatically classify semantic entrypoints across Rust, Python, and TypeScript (`fn main`, `#[tokio::main]`, CLI commands, HTTP route handlers, MCP tool registrations, framework lifecycle hooks, and test roots), expose them via `code_map_overview(aspects=["entrypoints"])`, and filter them out of `code_map_query(operation="unwired")` so agents never mistake top-level entrypoints for dead code.

3. **1-Hop Directory Boundary Slices (`boundary_contract`) & Architectural Layer Topology (`aspects=["topology"]`):**
   - Upgrade `code_map_query(operation="slice")` when given a directory/module path to return a compact **1-Hop Boundary Contract** (`exported_symbols` called externally, `inbound_callers` grouped by external module, and `outbound_dependencies` grouped by external module), allowing agents to refactor or understand a package's external contract in a single call.
   - Add `aspects=["topology"]` to `code_map_overview` (`src/db/reader.rs`) computing deterministic architectural layers (`entrypoint`, `orchestrator`, `domain`, `data`, `foundation`, `test`) and cross-directory coupling edges from graph in/out degrees.

Static ACDD seam verification and its diagnostics belong to AC-001,
under `nested-implement-gate-and-claim-engine`.

---

## Tasks in this milestone

The five feature tasks below belong to LS-002:

### task: sql-schema-extractor-and-orm-table-linking

```yaml
task_ref: sql-schema-extractor-and-orm-table-linking
target: Index .sql DDL/migrations and embedded SQL queries into table/column nodes and link ORM models and application functions via defines_table, queries_table, and mutates_table edges
proof_policy: seam-test-first
contract_revision: 7
depends_on: []
scope:
- src/engine/languages/mod.rs
- src/engine/languages/sql.rs
- src/engine/languages/rust.rs
- src/engine/languages/python.rs
- src/engine/languages/typescript/mod.rs
- src/engine/ast/routes.rs
- src/engine/linker/framework_relations.rs
- src/engine/linker/resolution/mod.rs
- src/db/traversal.rs
- docs/architecture/indexing.md
- docs/reference/framework-manifests.md
- tests/languages/
- src/engine/languages/rust
- src/engine/languages/python
- src/engine/languages/manifests
- src/engine/languages/manifests.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/sql
- src/engine/languages/manifests/projections.rs
- src/engine/languages/manifests/syntax.rs
- src/engine/languages/manifests/semantic.rs
- src/engine/languages/python/mod.rs
- src/engine/languages/python/syntax_operations.rs
- src/engine/languages/rust/mod.rs
- src/engine/languages/python/dependencies.rs
- src/engine/languages/sql/mod.rs
- src/engine/languages/semantic.rs
- src/engine/languages/python/lazy_exports.rs
- src/engine/languages/python/module_exports.rs
- src/engine/languages/python/linker.rs
- src/engine/languages/python/template_loaders.rs
- src/engine/languages/python/computed_receivers.rs
- src/engine/languages/python/value_flow.rs
- src/engine/languages/rust/linker.rs
- src/engine/languages/rust/value_flow.rs
status: completed
subtasks:
- subtask_ref: implement-sql-file-ddl-and-dml-extractor
  title: Create src/engine/languages/sql.rs (and register .sql in src/engine/languages/mod.rs) to extract CREATE TABLE, CREATE VIEW, ALTER TABLE, columns, foreign keys (references_table), and DML table usages from .sql schema and migration files
  status: completed
  evidence: 'Production seam tests/languages/extractors.rs; cargo test --test languages 250/250 and strict Clippy exit 0, receipt #1279; independent review candidate d9a53b.'
- subtask_ref: extract-embedded-sql-and-orm-table-facts
  title: Extract embedded SQL table references (SELECT FROM, INSERT INTO, UPDATE, DELETE FROM) from Rust/Python/TS string literals and ORM table bindings (Django Meta.db_table / model name, SQLAlchemy __tablename__) into Facts
  status: completed
  evidence: 'Production seam tests/languages/extractors.rs; cargo test --test languages 250/250 and strict Clippy exit 0, receipt #1279; independent review candidate d9a53b.'
- subtask_ref: link-orm-models-and-queries-to-sql-tables
  title: In src/engine/linker/framework_relations.rs, resolve extracted SQL table facts and ORM bindings to table nodes, emitting defines_table, queries_table, mutates_table, and references_table edges
  status: completed
  evidence: 'Production seam tests/languages/extractors.rs; cargo test --test languages 250/250 and strict Clippy exit 0, receipt #1279; independent review candidate d9a53b.'
extended_scope:
- src/engine/languages/rust
- src/engine/languages/python
- src/engine/languages/manifests
- src/engine/languages/manifests.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/sql
- src/engine/languages/manifests/projections.rs
- src/engine/languages/manifests/syntax.rs
- src/engine/languages/manifests/semantic.rs
- src/engine/languages/python/mod.rs
- src/engine/languages/python/syntax_operations.rs
- src/engine/languages/rust/mod.rs
- src/engine/languages/python/dependencies.rs
- src/engine/languages/sql/mod.rs
- src/engine/languages/semantic.rs
- src/engine/languages/python/lazy_exports.rs
- src/engine/languages/python/module_exports.rs
- src/engine/languages/python/linker.rs
- src/engine/languages/python/template_loaders.rs
- src/engine/languages/python/computed_receivers.rs
- src/engine/languages/python/value_flow.rs
- src/engine/languages/rust/linker.rs
- src/engine/languages/rust/value_flow.rs
receipt:
  contract_revision: 7
  passed_at: 2026-10-29T19:59:15.546768700+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/languages/extractors.rs
      tests_passed: 250
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Independent source review candidate d9a53b, prior findings verified closed; rev7 scope justified by #1270-1277 and actual Forge outputs; tests and Clippy builder receipts #1279; no benchmark.'
        claims:
          applicable: true
          evidence: 'Independent source review candidate d9a53b, prior findings verified closed; rev7 scope justified by #1270-1277 and actual Forge outputs; tests and Clippy builder receipts #1279; no benchmark.'
        concurrency:
          applicable: true
          evidence: 'Independent source review candidate d9a53b, prior findings verified closed; rev7 scope justified by #1270-1277 and actual Forge outputs; tests and Clippy builder receipts #1279; no benchmark.'
        project_isolation:
          applicable: true
          evidence: 'Independent source review candidate d9a53b, prior findings verified closed; rev7 scope justified by #1270-1277 and actual Forge outputs; tests and Clippy builder receipts #1279; no benchmark.'
        administration:
          applicable: true
          evidence: 'Independent source review candidate d9a53b, prior findings verified closed; rev7 scope justified by #1270-1277 and actual Forge outputs; tests and Clippy builder receipts #1279; no benchmark.'
      extended_scope_review:
        approved_paths:
        - src/engine/languages/rust
        - src/engine/languages/python
        - src/engine/languages/manifests
        - src/engine/languages/manifests.rs
        - src/engine/languages/dependency_registry.rs
        - src/engine/languages/sql
        - src/engine/languages/manifests/projections.rs
        - src/engine/languages/manifests/syntax.rs
        - src/engine/languages/manifests/semantic.rs
        - src/engine/languages/python/mod.rs
        - src/engine/languages/python/syntax_operations.rs
        - src/engine/languages/rust/mod.rs
        - src/engine/languages/python/dependencies.rs
        - src/engine/languages/sql/mod.rs
        - src/engine/languages/semantic.rs
        - src/engine/languages/python/lazy_exports.rs
        - src/engine/languages/python/module_exports.rs
        - src/engine/languages/python/linker.rs
        - src/engine/languages/python/template_loaders.rs
        - src/engine/languages/python/computed_receivers.rs
        - src/engine/languages/python/value_flow.rs
        - src/engine/languages/rust/linker.rs
        - src/engine/languages/rust/value_flow.rs
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
    - 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
    - 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
    - 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
    architectural_notes:
    - 'decisions: Scope extension needed: Rust and Python profiles now live at src/engine/languages/{rust,python}/mod.rs (manifest points to nonexistent flat files); SQL profile registration uses Cargo.toml lang-* feature. ORM bindings must remain manifest-driven per src/engine/languages/AGENTS.md, requiring manifests and their generic rule processing owners. Existing References are persistable, but delta invalidation must be verified before claiming full scope.'
    - 'decisions: Correction to blocker #1246: mandatory Cargo.toml/Tree-sitter was not established. The accepted task explicitly permits a dependency-free SQL scanner, and extraction already invokes LanguageProfile::extract_file for non-AST special cases. Manual profile registration plus a grammar-optional path is a candidate; code paths that currently assume grammar are identified in milestone draft #1247. Do not treat prior Cargo scope rejection as a proved blocker. Other task requirements (persistent SQL facts and declarative framework semantics) remain to be implemented and tested; candidate design must handle unsupported AST search safely.'
    - 'decisions: Admitted manifests.rs and dependency_registry.rs to guard grammar-bound preparation for dependency-free SQL scanner profile. Semantic manifest operations project configured literal class attributes and Django model defaults to table references during extraction; linker consumes persisted references. Cargo.toml is unchanged. SQL impact default traversal omits new relation kinds; Forge denied traversal scope due downstream task ownership. Acceptance impact remains unproved until shared owner coordination; docs/reference/framework-manifests.md scope also rejected.'
    - 'decisions: Admitted src/engine/languages/sql/ and moved the grammar-free profile to sql/mod.rs: generated registry treats every flat .rs file as an available lang-* feature, so sql.rs would incorrectly advertise a nonexistent Cargo feature. Directory profile remains manually registered and requires no Cargo manifest edit.'
    - 'decisions: Rev 5 architectural synthesis: reuse grammar-free SQL LanguageProfile manually registered in languages/mod.rs, parsed by extract_file to persisted Facts, existing manifest-driven ORM projections and framework_relations linker. No Cargo.toml or build.rs edits. Impact uses src/db/traversal.rs resolve_traversal_kinds shared by traversal and changed-node callers; admit file as initial scope before downstream owners so default table impact can include SQL relation kinds, preserve default behavior for non-table selectors. Document new semantic rules in admitted docs/reference/framework-manifests.md; scope ordering synchronized as defined in tasks.md. Existing seam tests/languages/extractors.rs executes real writer, reader, linker, impact, and cold/delta. On rev 5 candidate cargo test --test languages extractors::test_rust_ast_extractor compiled and failed 101 at missing SQL view in impact (earlier table, edge and ORM assertions passed; subsequent cold/delta not yet reached). Full task acceptance and independent review remain required.'
    - 'decisions: Rev 5: SQLx derive alone is not table evidence. A package-activated declarative macro_type_to_table rule recognizes explicit sqlx::query_as! and persists a syntax candidate. Linker accepts a unique same-file same-lexical-parent struct and unique indexed table; imported/ambiguous/shadowed/unconfigured identities fail closed. SQL table relations participate in default traversal. Runtime seam proves SQL schema, embedded SQL, ORM, SQLx, impact and cold/delta output-root equality. Focused cargo test --test languages extractors::test_rust_ast_extractor exit 0, 1 passed; cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages exit 0, 250 passed. Resolves impact/docs blockers #1253. SQLx cross-file/imported result types deliberately unresolved; SQL scope remains bounded and no table inferred from derive.'
    - 'decisions: Review-repair clusters: SQL scanner/Facts/linker preserve schema-qualified table IDs and fail closed for ambiguous short names, including synthesized embedded tables; comma FROM and BEGIN/PRAGMA batches are exercised through workspace SQLite indexing. Django configured model proof guards Meta override, non-model and reassigned alias; public grammarless parse_file dispatches profile extraction. Focused seam failed before repairs (exit 101) and passed during repair; full strict Clippy and languages lane underway on final candidate. Matrix: qualified/short/ambiguous/embedded, default/explicit/non-model/reassigned, batch/list, public parser. Mutation runs are not yet established per cluster; independent review must not treat this as a closure receipt.'
    - 'decisions: Contract revision 6 architectural synthesis: source-verified python_summary_candidates gates RouteContextBuilder; ORM-only files currently have lexical_write_count 0 and fail the runtime seam despite real model import. Extend initial scope solely to src/engine/ast/routes.rs so active declarative table-base rules admit the existing shared main-walk collector, with exact tests and no extra module walk. Changes use existing manifest IR, dependency activation, statement-aware scanner, and linker. No Cargo.toml/build.rs or protected milestone field changes. code_map impact shows two RouteContext callers, existing production seam tests/lang extractor cover downstream SQLite/cold-delta; prior audit and cross-task sequencing #1255, current red matrix #1260. Scope sync approved; other milestone tasks remain in their prior stages.'
    - 'decisions: Second systemic replan, independently checked by sql_architect. Shared SQL scanner: typed tokens, quote/comment and statement boundaries, context/depth-aware FROM and update parser shared by schema/literal/macro, ignore transaction/control statements yet process later DDL/DML; fail closed unproven dynamic and CTE identities. ORM: once per file build immutable indexes from completed Facts (owner, module imports with provenance, classes, inheritance and factory initializers); memoized bounded traversal with source order and existing lexical write counts, admitted factory identities configured declaratively, no per-class scan of all Facts. Matrix includes COMMIT/ROLLBACK, ONLY/AS/bare alias, mixed joins/lists and nested ON, fake UPDATE in UPSERT/FK, quoted identities; SQLAlchemy factory aliases, reassignment/nonmodel/no dependency/local Base, graph cycle, cold/delta. Compiling runtime red fe7872e6a3b4b9b8 exit 101; existing source remains in-scope; do not claim pass before proof.'
    - 'decisions: Fourth-round design review by independent sql_architect: shared segment codec canonicalizes unquoted lowercase and quoted exact without preserving quote marker (orders == \"orders\", Foo == foo == \"foo\", != \"Foo\"; quoted dot is one segment). Linker full/short maps, DDL/query/SQLx and CTE use same codec. CTE parser reads WITH [RECURSIVE] name [(column list)] AS [NOT MATERIALIZED] (balanced body), per-statement visibility spans distinguish recursive/self/prior and qualified physical tables. Embedded fast precheck scans entire literal bytes without allocation for effect tokens (FROM/JOIN/INTO/UPDATE/REFERENCES/CREATE/ALTER), retaining transaction/control/comment prefixes. Main visitor assignment event indexes direct and nested configured class attrs by owner/container/attribute and static/dynamic value; deferred rules read keyed index, never rescan bodies per rule. Test matrix and red seam added; no benchmarks.'
    - 'decisions: Revision 7 architecture retains rev6 design #1261 and #1267, grammar-free SQL scanner -> persisted Facts -> existing manifest-driven ORM projections and linker, indexed Python ownership/attribute collection via existing walk, SQL table impact in traversal; no Cargo/build changes. Sole revision removes 13 unused individually listed paths from scope and extended_scope, preserving all production and documentation edits and protected milestone authority. The precise historical Forge errors and absence of individual evidence for removals are in #1270. This correction does not alter outcome, existing acceptance seam, prior architectural rationale, or cross-task ordering. Forge may demand specific callers on contract submit: admit only the exact reported paths, record the error, then repeat. Red proof rerun uses an isolated copy with SQL identifier equivalence deliberately regressed, retaining actual production seam and leaving task candidate intact; prior mutation matrix #1268 remains applicable.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Implementation Guide (Step-by-Step for Builders)

1. **`src/engine/languages/sql.rs` & Registration in `src/engine/languages/mod.rs`:**
   - Register `.sql` in `LanguageRegistry` (`src/engine/languages/mod.rs`).
   - Implement a fast, deterministic SQL scanner/parser in `src/engine/languages/sql.rs` (handling standard SQLite/PostgreSQL/MySQL DDL without requiring external runtime dependencies if a lightweight deterministic lexer/parser suffices, or Tree-sitter SQL if configured):
     - On `CREATE [TEMP|TEMPORARY] TABLE [IF NOT EXISTS] [schema.]table_name (...)`: emit a `Node` with `kind: "table"`, `name: table_name`, `id: format!("{path}:table:{table_name}")`, `details: json!({"columns": [...], "primary_key": [...]})`.
     - For `FOREIGN KEY (...) REFERENCES target_table(...)` or inline `REFERENCES target_table`, emit a `Reference` / semantic fact with `kind: "references_table"` targeting `target_table`.
     - On `CREATE VIEW view_name AS SELECT ...`: emit a `Node` with `kind: "view"` and `queries_table` facts for referenced tables.
     - On `ALTER TABLE table_name ...`: emit a `mutates_table` / `alters_table` reference to `table_name`.
2. **Embedded SQL & ORM Table Extraction (`rust.rs`, `python.rs`, `typescript/mod.rs`):**
   - Provide a shared helper `pub fn extract_embedded_sql_tables(literal: &str) -> Vec<(String, &'static str)>` in `src/engine/languages/sql.rs` that checks if a string literal begins with or contains SQL keywords (`CREATE TABLE`, `SELECT ... FROM`, `INSERT [OR REPLACE] INTO`, `UPDATE ... SET`, `DELETE FROM`) and extracts `(table_name, edge_kind)` where `edge_kind` is `"defines_table"`, `"queries_table"`, or `"mutates_table"`.
   - Call this helper on string literals inside function/method bodies in `rust.rs`, `python.rs`, and `typescript/mod.rs`, recording semantic facts on the enclosing function node.
   - In `python.rs`, extract `__tablename__ = "..."` (SQLAlchemy), `db_table = "..."` inside `class Meta` (Django), or default Django `<app_label>_<model_lower>` convention into ORM table facts.
3. **Linker Resolution (`src/engine/linker/framework_relations.rs`):**
   - Build a global index of `table` and `view` nodes keyed by normalized lowercase table name.
   - Resolve `defines_table`, `queries_table`, `mutates_table`, and `references_table` facts from `.sql` files, ORM models, and functions with embedded SQL to the canonical `table` node (or synthesize a virtual `table:<name>` node when a table is defined inline via `execute_batch("CREATE TABLE ...")` in Rust/Python).

#### Acceptance

1. Indexing a repository with `.sql` migrations or inline `CREATE TABLE nodes (...)` (like Forge's own `src/core/schema.rs` and `src/db/reader.rs` / `src/db/writer.rs`) creates `table` nodes and links functions executing `SELECT ... FROM nodes` or `INSERT INTO nodes` via `queries_table` and `mutates_table` edges.
2. Python ORM classes with `__tablename__ = "orders"` or Django `class Meta: db_table = "orders"` link to the `orders` `table` node via `defines_table`.
3. `code_map_impact` on a `table` node returns all SQL views, ORM models, and backend functions that query or mutate that table.
4. Cold and incremental delta builds produce identical `forge-owner-sha256-v2` commitments.
5. `cargo test --test languages` passes.

---

### task: deterministic-entrypoint-classification-and-unwired-filtering

```yaml
task_ref: deterministic-entrypoint-classification-and-unwired-filtering
target: "Classify semantic entrypoints during extraction/linking, expose aspects=['entrypoints'] in code_map_overview, and prevent false-positive entrypoint reporting in code_map_query(operation='unwired')"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - sql-schema-extractor-and-orm-table-linking
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
  - src/engine/linker/resolution/mod.rs
  - src/engine/linker/framework_relations.rs
  - src/db/reader.rs
  - src/db/traversal.rs
  - src/mcp/tools.rs
  - src/cli/mod.rs
  - docs/reference/tools.md
  - docs/reference/cli.md
  - tests/mcp/
status: ready
subtasks:
  - subtask_ref: classify-entrypoints-across-languages-and-frameworks
    title: "Tag entrypoint nodes with details.entrypoint ('main', 'cli_command', 'http_route', 'mcp_tool', 'event_handler', 'test') during extraction and framework linking"
    status: pending
  - subtask_ref: add-entrypoints-aspect-to-code-map-overview
    title: "Extend overview_with_options in src/db/reader.rs to support aspects=['entrypoints'], returning grouped entrypoints with kind, signature, path:line, and downstream fan-out"
    status: pending
  - subtask_ref: filter-entrypoints-in-unwired-query
    title: "Update operation='unwired' in src/db/traversal.rs to exclude classified entrypoints by default (and report excluded_entrypoints_count in summary)"
    status: pending
```

#### Implementation Guide (Step-by-Step for Builders)

1. **Deterministic Entrypoint Tagging (`src/engine/linker/`):**
   - Classify a node with `details["entrypoint"] = "<category>"` when any of the following deterministic rules hold:
     - `"main"`: Rust `fn main`, functions with `#[tokio::main]` / `#[async_std::main]`, Python `__main__` blocks or `pyproject.toml` `[project.scripts]` targets, TypeScript `package.json` `bin`/`main` entry functions.
     - `"http_route"`: Any function/method that is the target of a `routes_to` edge or has a route decorator (`@app.get`, `@router.post`, Django `urlpatterns` target, NestJS `@Get`/`@Post`, Nuxt `pages/**/*.vue`).
     - `"mcp_tool"` / `"cli_command"`: Functions dispatched by MCP tool handlers or `clap` command handlers (`#[derive(Parser)]`, `#[derive(Subcommand)]`, `Click`/`Typer`/`BaseCommand.handle`).
     - `"event_handler"`: Functions targeted by `dispatches` / `listens_to` edges, Celery `@shared_task`, or cron/worker hooks.
     - `"test"`: Nodes with `kind == "test"` or inside `tests/` test functions (`#[test]`, `def test_*`, `it(...)`, `test(...)`).
2. **`aspects=["entrypoints"]` in `src/db/reader.rs` (`overview_with_options` around line 649):**
   - Add `"entrypoints"` to the supported `aspects` list in `overview_with_options` (`src/db/reader.rs` and `src/mcp/tools.rs`).
   - Query nodes where `json_extract(details, '$.entrypoint') IS NOT NULL` (excluding `"test"` unless requested), grouped by entrypoint category (`main`, `http_route`, `mcp_tool`, `cli_command`, `event_handler`), ordered by `(category, outgoing_calls DESC, id ASC)`.
3. **`operation="unwired"` Precision in `src/db/traversal.rs`:**
   - In the `"unwired"` query branch (`src/db/traversal.rs` around line 560), filter out nodes where `json_extract(n.details, '$.entrypoint') IS NOT NULL` by default, and include `excluded_entrypoints_count` in the response metadata so agents see only genuine dead/unwired symbols.

#### Acceptance

1. `code_map_overview(aspects=["entrypoints"])` returns categorized application entrypoints (`main`, `http_route`, `mcp_tool`, `cli_command`, `event_handler`) with exact `path` and `start_line`.
2. `code_map_query(operation="unwired")` no longer flags `fn main`, HTTP route views, CLI command handlers, or event listeners as dead/unwired symbols, and includes `excluded_entrypoints_count` in its summary.
3. CLI `contextunity-forge-mcp overview --aspects entrypoints` matches MCP output.
4. `cargo test --test mcp` passes.

---

### task: directory-boundary-slice-and-module-topology

```yaml
task_ref: directory-boundary-slice-and-module-topology
target: "Enhance code_map_query(operation='slice') for directories with 1-hop inbound/outbound boundary contracts (neighborMap) and add aspects=['topology'] to code_map_overview"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - deterministic-entrypoint-classification-and-unwired-filtering
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
  - src/db/traversal.rs
  - src/db/reader.rs
  - src/mcp/tools.rs
  - src/cli/mod.rs
  - docs/reference/tools.md
  - docs/reference/cli.md
  - tests/mcp/
status: ready
subtasks:
  - subtask_ref: add-one-hop-boundary-contract-to-directory-slice
    title: "Extend slice_paged in src/db/traversal.rs so directory selectors include a boundary_contract section listing exported_symbols (called from outside), inbound_callers grouped by external directory, and outbound_dependencies grouped by external directory"
    status: pending
  - subtask_ref: add-module-topology-aspect-to-overview
    title: "Extend overview_with_options in src/db/reader.rs with aspects=['topology'] computing directory-level architectural layers (entrypoint, orchestrator, domain, data, foundation) and top inter-directory dependency edges"
    status: pending
  - subtask_ref: document-slice-boundaries-and-topology-in-reference-docs
    title: "Update docs/reference/tools.md and docs/reference/cli.md with examples of directory boundary slices and overview topology"
    status: pending
```

#### Implementation Guide (Step-by-Step for Builders)

1. **1-Hop Boundary Contract in `src/db/traversal.rs` (`slice_paged` around line 392):**
   - When `slice_paged` is called with a directory prefix (e.g. `src/db` or `src/engine/linker`):
     - In addition to internal nodes and internal edges, run two bounded SQL queries against `edges` joined with `nodes source_n` and `nodes target_n`:
       1. **Inbound Cross-Boundary Edges** (`target_n.path LIKE 'dir/%' AND source_n.path NOT LIKE 'dir/%'`):
          - Aggregate `exported_symbols`: internal symbols in `dir/` that have >= 1 external caller/user, with `external_caller_count` and `signature`.
          - Aggregate `inbound_callers_by_module`: external parent directories (`src/mcp`, `src/cli`, etc.) with edge counts and top symbol pairs.
       2. **Outbound Cross-Boundary Edges** (`source_n.path LIKE 'dir/%' AND target_n.path NOT LIKE 'dir/%'`):
          - Aggregate `outbound_dependencies_by_module`: external parent directories (`src/core`, etc.) with edge counts and top target symbols (`target_id`, `name`, `signature`).
     - Attach `"boundary_contract": { "exported_symbols": [...], "inbound_modules": [...], "outbound_modules": [...] }` to the slice response.
2. **Architectural Layer Topology in `src/db/reader.rs` (`overview_with_options`):**
   - When `aspects` includes `"topology"`:
     - Group files into logical module directories (e.g. two-level directory prefixes like `src/core`, `src/db`, `src/engine/linker`, `src/mcp`, `src/cli`).
     - Compute per-module `internal_edges`, `inbound_external_edges`, `outbound_external_edges`, `entrypoint_count`, and `table_query_count`.
     - Deterministically classify each module's architectural role:
       - `"adapter_or_entrypoint"`: contains entrypoints (`mcp_tool`, `cli_command`, `http_route`) or `inbound_external_edges == 0 && outbound_external_edges > 0`.
       - `"foundation"`: `outbound_external_edges == 0 && inbound_external_edges > 0` (e.g., `src/core`).
       - `"data_access"`: high density of `table` nodes or `queries_table`/`mutates_table` edges (e.g., `src/db`).
       - `"orchestrator"`: both high `inbound_external_edges` and high `outbound_external_edges` across >= 3 distinct modules.
       - `"domain"`: high internal cohesion (`internal_edges > external_edges`).
     - Return `modules` (with role, symbol count, cohesion ratio) and `module_edges` (`source_module`, `target_module`, `edge_count`, `dominant_kind`).

#### Acceptance

1. Calling `code_map_query(operation="slice", target="src/db")` returns `boundary_contract` containing `exported_symbols` (only `src/db` symbols actually used outside `src/db`), `inbound_modules` (e.g. `src/mcp`, `src/cli`), and `outbound_modules` (e.g. `src/core`).
2. Calling `code_map_overview(aspects=["topology"])` returns deterministic module clusters with assigned architectural roles (`foundation`, `data_access`, `domain`, `orchestrator`, `adapter_or_entrypoint`) and inter-module dependency weights.
3. CLI and MCP surfaces match on both operations, and `docs/reference/tools.md` + `docs/reference/cli.md` are updated.
4. `cargo test --test mcp` passes.

---

### task: type-and-signature-aware-node-search-and-ergonomics

```yaml
task_ref: type-and-signature-aware-node-search-and-ergonomics
target: "Index param_types, return_type, and decorators into FTS5 node_search, add signature filters to code_map_search, and update agent skill guidance"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - directory-boundary-slice-and-module-topology
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
  - src/db/ingest.rs
  - src/db/symbols/search.rs
  - src/mcp/tools.rs
  - skills/contextunity-forge/SKILL.md
  - docs/reference/tools.md
status: ready
subtasks:
  - subtask_ref: fts5-signature-and-decorator-indexing
    title: "Update write_node_search_text in src/db/ingest.rs to include param_types, return_type, and decorators in FTS5 node_search"
    status: pending
  - subtask_ref: type-query-filter-support
    title: "Add returns: and takes: query prefix evaluation in src/db/symbols/search.rs leveraging JSON details columns"
    status: pending
  - subtask_ref: agent-skill-and-tool-schema-type-guidance
    title: "Document type-aware search patterns in skills/contextunity-forge/SKILL.md and update MCP tool descriptions"
    status: pending
```

#### Implementation Guide (Step-by-Step for Builders)

1. **FTS5 Search Text Enrichment (`src/db/ingest.rs` `write_node_search_text`):**
   - Extract `param_types`, `return_type`, and `decorators` from `node.details`.
   - Append formatted type and decorator tokens into `search` text buffer so full-text search matches them naturally without schema migrations:
     - Parameters: `param_name:ParamType` and plain `ParamType`.
     - Returns: `returns:ReturnType` and plain `ReturnType`.
     - Decorators/Attributes: `@decorator_name` or `#[attribute_name]`.
2. **Signature & Type Query Filters (`src/db/symbols/search.rs`):**
   - Add explicit support for `returns:<type>` and `takes:<type>` query filters mapping to fast JSON extractions:
     `json_extract(n.details, '$.return_type') LIKE '%<type>%'` or `json_extract(n.details, '$.param_types') LIKE '%<type>%'`.
3. **Agent Skill & Tool Schema Updates:**
   - Update `skills/contextunity-forge/SKILL.md` with concrete search examples for types, decorators, and return types.
   - Update `code_map_search` MCP description in `src/mcp/tools.rs` to inform LLMs that types and signatures are searchable.

#### Acceptance

1. `code_map_search(query: "GateProfile")` returns functions whose parameters or return types reference `GateProfile`.
2. `code_map_search(query: "returns:Result")` filters functions returning `Result`.
3. `code_map_search(query: "@pytest.fixture")` or `code_map_search(query: "derive")` matches decorated/attributed nodes.
4. `cargo test --test mcp` passes.

---

### task: test-gap-and-hierarchy-traversal-operations

```yaml
task_ref: test-gap-and-hierarchy-traversal-operations
target: "Add operation='untested' and operation='implementations' to code_map_query and traversal engine, surfacing test coverage gaps and class/trait hierarchies from existing SQLite edges"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - type-and-signature-aware-node-search-and-ergonomics
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
  - src/db/traversal.rs
  - src/mcp/tools.rs
  - src/cli/mod.rs
  - docs/reference/tools.md
  - skills/contextunity-forge/SKILL.md
status: ready
subtasks:
  - subtask_ref: untested-symbols-traversal-operation
    title: "Implement operation='untested' in src/db/traversal.rs finding production functions/methods with 0 inbound is_test=1 call edges in the selected path"
    status: pending
  - subtask_ref: implementations-hierarchy-traversal-operation
    title: "Implement operation='implementations' in src/db/traversal.rs traversing extends and implements edges for a given class or trait selector"
    status: pending
  - subtask_ref: agent-guidance-for-test-gaps-and-hierarchies
    title: "Document untested and implementations query operations in skills/contextunity-forge/SKILL.md and tool references"
    status: pending
```

#### Implementation Guide (Step-by-Step for Builders)

1. **`operation="untested"` in `src/db/traversal.rs`:**
   - Add `"untested"` to supported operations in `query_with_path_options`.
   - Query `nodes n` where `n.kind IN ('function', 'method') AND n.is_test = 0 AND (?1 = '' OR pd.path LIKE ?1 || '%')`:
     - Filter: `NOT EXISTS (SELECT 1 FROM edges e JOIN nodes caller ON caller.node_hash = e.src_hash WHERE e.dst_hash = n.node_hash AND (e.kind = 'calls' OR caller.is_test = 1))`.
     - Return paged list of untested functions with file path, line, and signature.
2. **`operation="implementations"` in `src/db/traversal.rs`:**
   - Add `"implementations"` to supported operations.
   - For a target symbol (trait, interface, or base class), traverse inbound edges with `kind IN ('implements', 'extends')` to find all concrete implementors/subclasses.
   - Return structured list of implementing types with path and line coordinates.
3. **Agent Skill Guidance (`skills/contextunity-forge/SKILL.md`):**
   - Add instructions for contract planning:
     - `code_map_query(operation="untested", target="src/billing")` to discover uncovered production code.
     - `code_map_query(operation="implementations", selector="LanguageLinker")` to discover all implementors of an interface.

#### Acceptance

1. `code_map_query(operation="untested", target="src/core")` lists production functions without test callers.
2. `code_map_query(operation="implementations", selector="LanguageLinker")` returns all structs implementing `LanguageLinker`.
3. `cargo test --test mcp` passes.
