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
target: Classify semantic entrypoints during extraction/linking, expose aspects=['entrypoints'] in code_map_overview, and prevent false-positive entrypoint reporting in code_map_query(operation='unwired')
proof_policy: seam-test-first
contract_revision: 7
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
- src/mcp/tools/inputs.rs
- docs/reference/mcp-tools.md
- src/db/writer.rs
- src/db/delta.rs
- src/db/writer/entrypoints.rs
- src/core/commitments.rs
- tests/incremental/
- src/engine/languages/rust/mod.rs
- src/engine/languages/mod.rs
- src/engine/ast/visitor.rs
- src/engine/languages/python/mod.rs
- src/engine/languages/python/decorated_registration.rs
- src/engine/languages/python/syntax_operations.rs
- src/engine/languages/typescript/mod.rs
- src/engine/languages/manifests/syntax.rs
- src/engine/languages/manifests/framework.rs
- src/engine/languages/manifests/semantic.rs
- src/engine/languages/manifests/click.toml
- src/engine/languages/manifests/typer.toml
- src/engine/languages/manifests/celery.toml
- tests/languages/
- docs/reference/framework-manifests.md
- src/engine/languages/dependency_registry.rs
- src/engine/ast/mod.rs
- src/engine/languages/manifests/django.toml
- src/engine/linker/entrypoints.rs
- src/engine/linker.rs
- src/engine/languages/manifests/clap.toml
- src/engine/languages/rust/cli_registration.rs
- src/engine/languages/manifests/jest.toml
- src/engine/languages/manifests/vitest.toml
status: completed
subtasks:
- subtask_ref: classify-entrypoints-across-languages-and-frameworks
  title: Tag entrypoint nodes with details.entrypoint ('main', 'cli_command', 'http_route', 'mcp_tool', 'event_handler', 'test') during extraction and framework linking
  status: completed
  evidence: Production seams tests/mcp/context.rs and tests/languages/python/modules.rs; Clippy, MCP 90/90, languages 250/250, incremental 31/31 (#1292), independent review candidate 774979.
- subtask_ref: add-entrypoints-aspect-to-code-map-overview
  title: Extend overview_with_options in src/db/reader.rs to support aspects=['entrypoints'], returning grouped entrypoints with kind, signature, path:line, and downstream fan-out
  status: completed
  evidence: Production seams tests/mcp/context.rs and tests/languages/python/modules.rs; Clippy, MCP 90/90, languages 250/250, incremental 31/31 (#1292), independent review candidate 774979.
- subtask_ref: filter-entrypoints-in-unwired-query
  title: Update operation='unwired' in src/db/traversal.rs to exclude classified entrypoints by default (and report excluded_entrypoints_count in summary)
  status: completed
  evidence: Production seams tests/mcp/context.rs and tests/languages/python/modules.rs; Clippy, MCP 90/90, languages 250/250, incremental 31/31 (#1292), independent review candidate 774979.
extended_scope:
- src/mcp/tools/inputs.rs
- docs/reference/mcp-tools.md
- src/engine/languages/dependency_registry.rs
- src/engine/ast/mod.rs
- src/engine/languages/manifests/django.toml
- src/engine/linker/entrypoints.rs
- src/engine/languages/manifests/clap.toml
- src/engine/languages/rust/cli_registration.rs
- src/engine/languages/manifests/jest.toml
- src/engine/languages/manifests/vitest.toml
receipt:
  contract_revision: 7
  passed_at: 2026-10-29T21:22:08.981792830+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/mcp/context.rs
      - tests/languages/python/modules.rs
      tests_passed: 371
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent auditor reviewed candidate 774979, prior findings and scope; no new confirmed defect. Builder reports Clippy and targets green (#1292), auditor did not rerun. Conditional transition has state assertions but no dedicated cold root comparison.
        claims:
          applicable: true
          evidence: Independent auditor reviewed candidate 774979, prior findings and scope; no new confirmed defect. Builder reports Clippy and targets green (#1292), auditor did not rerun. Conditional transition has state assertions but no dedicated cold root comparison.
        concurrency:
          applicable: true
          evidence: Independent auditor reviewed candidate 774979, prior findings and scope; no new confirmed defect. Builder reports Clippy and targets green (#1292), auditor did not rerun. Conditional transition has state assertions but no dedicated cold root comparison.
        project_isolation:
          applicable: true
          evidence: Independent auditor reviewed candidate 774979, prior findings and scope; no new confirmed defect. Builder reports Clippy and targets green (#1292), auditor did not rerun. Conditional transition has state assertions but no dedicated cold root comparison.
        administration:
          applicable: true
          evidence: Independent auditor reviewed candidate 774979, prior findings and scope; no new confirmed defect. Builder reports Clippy and targets green (#1292), auditor did not rerun. Conditional transition has state assertions but no dedicated cold root comparison.
      extended_scope_review:
        approved_paths:
        - src/mcp/tools/inputs.rs
        - docs/reference/mcp-tools.md
        - src/engine/languages/dependency_registry.rs
        - src/engine/ast/mod.rs
        - src/engine/languages/manifests/django.toml
        - src/engine/linker/entrypoints.rs
        - src/engine/languages/manifests/clap.toml
        - src/engine/languages/rust/cli_registration.rs
        - src/engine/languages/manifests/jest.toml
        - src/engine/languages/manifests/vitest.toml
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
    - 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
    - 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
    - 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
    architectural_notes:
    - 'decisions: Adjacent SQL lint fixture repair: stored_syntax_lint_covers_compiled_profiles_without_mutating_index previously panicked because SQL is now an indexed grammarless language profile. Added a valid SQL fixture, kept SQL in indexed coverage, and exclude it only from the set required to have stored parser diagnostics. cargo test --test mcp lint_tools::stored_syntax_lint_covers_compiled_profiles_without_mutating_index passed (1 passed). This green repair is not entrypoint contract-red evidence.'
    - 'decisions: Build owner scope extension: src/mcp/tools/inputs.rs holds the MCP overview input schema, and docs/reference/mcp-tools.md is the actual documentation owner (the task''s docs/reference/tools.md does not exist). These are needed to present and document the admitted entrypoints aspect. Classification requires persisted node projection consistent with cold and delta commitments; architecture investigation continues before extending its storage owners.'
    - 'decisions: Contract revision 5 adds the minimum storage, language extraction, manifest and test owners necessary to satisfy the existing task: cold/delta node commitments and cross-file registration reclassification, plus declarative Click/Typer/Celery registration rules and their proofs. Persisted local extraction details.entrypoint_base supplies main/test; exact handles edges from route, tool_registration, cli_command_registration and event_handler_registration supply linked categories. Effective details.entrypoint precedence is test > main > http_route > mcp_tool > cli_command > event_handler. No framework-specific API matching in generic linker/storage. Retain current in-progress reader/CLI/traversal and red tests across re-admission; rerun the contract gates for revision 5.'
    - 'decisions: To implement existing guide rules for pyproject scripts and package main/bin, scope now admits dependency_registry, AST integration, Django manifest and linker entrypoints helper. linker.rs is outside prior scope roots; revision 7 admits that phase owner. Manifest metadata is indexed within dependency context; exact proven Python script targets and TypeScript top-level calls are represented by main_registration -> handles edges, consumed by the existing graph-to-node projection. Package path alone does not establish a specific exported function. Changes to dependency manifests trigger the existing full rebuild. Preserve current code and tests while re-running contract gates.'
    - 'decisions: Extended scope with manifests/clap.toml and rust/cli_registration.rs inside admitted roots for the already specified clap Parser/Subcommand dispatch rule. The production-seam fixture uses a temporary workspace Cargo manifest to activate clap; no repository Cargo.toml change is required. The fixture verifies an exact parse/match handler and an unrelated local enum negative.'
    - 'decisions: Extended scope for jest.toml and vitest.toml within admitted manifest roots to cover the guide''s test()/it() roots. Require verified imported runner and activated dependency; a dependency alone does not prove that unconfigured globals are test APIs. MCP fixture includes node:test, Jest and Vitest imported callbacks plus lexical shadow negatives.'
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
target: Enhance code_map_query(operation='slice') for directories with 1-hop inbound/outbound boundary contracts (neighborMap) and add aspects=['topology'] to code_map_overview
proof_policy: seam-test-first
contract_revision: 7
depends_on:
- deterministic-entrypoint-classification-and-unwired-filtering
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
- src/core/schema.rs
- src/core/config.rs
- src/core/commitments.rs
- src/db/writer.rs
- src/db/delta.rs
- src/db/traversal.rs
- src/db/reader.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/cli/mod.rs
- src/core/format/discovery.rs
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- tests/mcp/
- tests/commitment_integrity.rs
- tests/incremental/directory_slice.rs
status: completed
subtasks:
- subtask_ref: add-one-hop-boundary-contract-to-directory-slice
  title: Extend slice_paged in src/db/traversal.rs so directory selectors include a boundary_contract section listing exported_symbols (called from outside), inbound_callers grouped by external directory, and outbound_dependencies grouped by external directory
  status: completed
  evidence: Snapshot e0b79be; production incremental 3/3, MCP 90/90, strict Clippy; independent review and seam review passed.
- subtask_ref: add-module-topology-aspect-to-overview
  title: Extend overview_with_options in src/db/reader.rs with aspects=['topology'] computing directory-level architectural layers (entrypoint, orchestrator, domain, data, foundation) and top inter-directory dependency edges
  status: completed
  evidence: Snapshot e0b79be; production incremental 3/3, MCP 90/90, strict Clippy; independent review and seam review passed.
- subtask_ref: document-slice-boundaries-and-topology-in-reference-docs
  title: Update docs/reference/mcp-tools.md and docs/reference/cli.md with examples of directory boundary slices and overview topology
  status: completed
  evidence: Snapshot e0b79be; production incremental 3/3, MCP 90/90, strict Clippy; independent review and seam review passed.
receipt:
  contract_revision: 7
  passed_at: 2026-10-29T22:03:13.635398732+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/incremental/directory_slice.rs
      - tests/mcp/context.rs
      tests_passed: 93
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent reviewer verified all 14 changed files against rev7 scope, protected fields untouched.
        claims:
          applicable: true
          evidence: Reviewed production SQLite boundary, topology, normalized selector and seam tests; builder reports incremental 3/3, integrity 16/16, Clippy and MCP 90/90.
        concurrency:
          applicable: true
          evidence: Reviewed atomic delta projection updates, rollback, cold and verified-cache publication.
        project_isolation:
          applicable: true
          evidence: Reviewed directory selectors, normalization, ambiguity and workspace-local production storage.
        administration:
          applicable: true
          evidence: Reviewed schema upgrade rebuild, preserved v2 domains and candidate snapshot; no protected profile changes.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
    - 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
    - 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
    - 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
    architectural_notes:
    - 'decisions: Architectural synthesis rev 5: slice_paged resolves files/symbols via traversal first, then directory_path and path_bounds; extend only the directory branch, joining indexed path_dictionary, nodes and edges from in-directory nodes with source/destination hash indexes. Do not read source bodies or use LIKE wildcard matching. Preserve directory resolution, existing nodes page/generation and file semantics; aggregate bounded deterministic inbound exports and module pairs/outbound targets. overview_with_options is the shared DB producer used by both CLI and MCP; add opt-in topology there, preserving generation/limits, aggregate indexed node/edge facts with deterministic module path mapping and classification precedence documented in reference. Existing tests/incremental/directory_slice.rs is the real scanner->SQLite->traversal seam and tests/mcp/context.rs covers CLI/MCP parity. Rev 5 replaces nonexistent reference owner docs/reference/tools.md with docs/reference/mcp-tools.md and admits existing seam; protected Cargo.toml, build.rs and profile files are not needed. Clean compilation and runtime assertion red: cargo test --test incremental directory_suffix_lists_real_nodes_and_exact_file_keeps_graph_traversal exited 101 on missing boundary_contract.exported_symbols.'
    - 'decisions: Rev 6 synthesis: prior #1293 remains architecture and directory proof. Newly inspected src/core/format/discovery.rs render_overview is a closed list with no fallback, so topology-only overview disappears from default Markdown although JSON contains it. Scope admits this renderer and src/mcp/tools/inputs.rs (MCP argument description), with no Cargo/build/profile changes. Extend existing stdio/CLI production parity seam to require visible topology in default Markdown; runtime red will prove the omitted renderer section. Preserve bounded pages and their continuation metadata in rendered output.'
    - 'decisions: Proposed rev7 architecture for user-directed strict incident locality: E_dir = indexed edges with at least one endpoint inside directory, including internal. Add edge_endpoint_paths(path,direction,src_hash,kind,dst_hash), path/direction PK plus edge-key index, exactly two rows per canonical edge. Schema version 16 rejects/rebuilds v15; v2 ALGORITHM and DOMAINS unchanged. Cold writer builds projection from canonical nodes/edges after persist_graph and validates bidirectional equality after secondary indexes before seal/verified receipt. Full commitments::verify checks every canonical endpoint projection and every projection row in reverse; missing/wrong/extra rows fail closed. Delta IMMEDIATE transaction materializes touched edge keys as changed_edge_keys UNION pre-delete incident edges of replaced_nodes, removes old projection and reinserts both endpoint rows after all graph aggregation, validates touched keys in both directions including wrong paths, and verifies before commit/cache publication. Prior reader admission verifies full projection on cache miss, receipt binds snapshot identity; schema upgrade rebuild invalidates old index/receipt. Transaction rollback preserves canonical edges, projection, commitments and previous receipt. No source IO; cold/delta same-schema output_root parity; v2 algorithm unchanged though schema metadata changes root. Indexed range reads visit projection records for directory endpoints, including twice for internal edges; SQLite B-tree seek cost and grouping/sort costs are explicit, no unsupported strict mathematical linear claim. Tests: cold/delta changed/deleted/moved endpoints, corruption delete/insert/wrong direction/path, old schema rebuild, rollback, cache invalidation, parity, isolated/high incident directory; no benchmark. Needed scope additions src/core/schema.rs, src/core/config.rs, src/core/commitments.rs, src/db/writer.rs, src/db/delta.rs, tests/commitment_integrity.rs. Existing src/db/reader.rs,traversal.rs and tests/incremental/directory_slice.rs already admitted.'
    - 'decisions: Rev7 architecture follows #1301, with scope amended to canonical schema/config, commitments, cold writer, delta and integrity seam. No Cargo.toml, build.rs or profile changes. Production scanner→index→SQLite test tests/incremental/directory_slice.rs compiles and fails 101 because the required incident adjacency table is absent, and additionally asserts two records per actual graph edge. Derived-state verification preserves v2 algorithm and rejects missing, extra and wrong endpoint rows; cold publication and delta transaction verify before trusted receipt.'
    - 'decisions: Independent reviewer reclassified the earlier unbounded intermediate materialization finding: milestone invariant requires O(E_dir) and no unrelated file-body reads, but neither the invariant nor ADR 0011 establishes an intermediate RAM cap or page-sized memory requirement. O(E_dir + key bytes) working memory remains a disclosed risk, not a standalone contract violation; do not introduce an unapproved new query failure limit. Complexity, pagination correctness, response limits, and integrity remain subject to independent review.'
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
3. CLI and MCP surfaces match on both operations, and `docs/reference/mcp-tools.md` + `docs/reference/cli.md` are updated.
4. `cargo test --test mcp` passes.

---

### task: type-and-signature-aware-node-search-and-ergonomics

```yaml
task_ref: type-and-signature-aware-node-search-and-ergonomics
target: Index param_types, return_type, and decorators into FTS5 node_search, add signature filters to code_map_search, and update agent skill guidance
proof_policy: seam-test-first
contract_revision: 7
depends_on:
- directory-boundary-slice-and-module-topology
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
- src/engine/languages/rust/mod.rs
- src/db/ingest.rs
- src/db/writer.rs
- src/db/symbols/search.rs
- src/core/config.rs
- src/mcp/tools.rs
- skills/contextunity-forge/SKILL.md
- docs/reference/tools.md
- docs/reference/mcp-tools.md
- tests/mcp/query_ranking.rs
status: completed
subtasks:
- subtask_ref: fts5-signature-and-decorator-indexing
  title: Update write_node_search_text in src/db/ingest.rs to include param_types, return_type, and decorators in FTS5 node_search
  status: completed
  evidence: Snapshot dfafada; strict Clippy and MCP 90/90, real production seam and independent review passed.
- subtask_ref: type-query-filter-support
  title: 'Add returns: and takes: query prefix evaluation in src/db/symbols/search.rs leveraging JSON details columns'
  status: completed
  evidence: Snapshot dfafada; strict Clippy and MCP 90/90, real production seam and independent review passed.
- subtask_ref: agent-skill-and-tool-schema-type-guidance
  title: Document type-aware search patterns in skills/contextunity-forge/SKILL.md and update MCP tool descriptions
  status: completed
  evidence: Snapshot dfafada; strict Clippy and MCP 90/90, real production seam and independent review passed.
extended_scope:
- docs/reference/mcp-tools.md
receipt:
  contract_revision: 7
  passed_at: 2026-10-29T22:24:08.474214454+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/mcp/query_ranking.rs
      tests_passed: 90
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewer verified 9 changed paths against rev7 scope; Cargo.toml and build.rs unchanged.
        claims:
          applicable: true
          evidence: Traced typed parameter/return/decorator through cold/delta FTS and JSON to bound filters and CLI/MCP; tests include collision, literals and Rust attributes; builder reports MCP 90/90.
        concurrency:
          applicable: true
          evidence: Existing transactional delta and verified cache paths retained; semantics version invalidates old indexes.
        project_isolation:
          applicable: true
          evidence: Path bounds, parameter binding, pagination and workspace-scoped production seams reviewed.
        administration:
          applicable: true
          evidence: Semantics version bumped without schema change; scoped candidate snapshot verified.
      extended_scope_review:
        approved_paths:
        - docs/reference/mcp-tools.md
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
    - 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
    - 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
    - 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
    architectural_notes:
    - 'decisions: Synthesis rev 6: preserve existing language extraction and caller surfaces; no Cargo.toml/build.rs or DB schema change. Python/Rust extract param_types into Node native_semantic (Node::parameter_types), decorators into metadata/details, and return annotations in TypedFacts.flows (FlowStore::get(node).return_type); plain node.details alone misses them. src/db/writer.rs::prepare_catalog_rows prepares cold search texts before stripping details; src/db/ingest.rs::persist_files_impl either uses those prepared texts or calls write_node_search_text for incremental, so one shared projection must receive real typed flow on both paths and remain deterministic, bounded and free of full-source FTS. Delta deletes old owner postings and repersists facts; exercise changed/deleted values, reopen, cold-vs-delta output_root. Search via src/db/symbols/search.rs has exact-name-first early return and FTS tokenization: incorporate type matches without breaking name priority and provide returns:/takes: role-sensitive filters, not a global match on types; preserve path, kind, exact, ranking, paging/generation and 2s bounds. Current navigation_details strips value_flow and need not expose return_type: implement filter against an intentionally persisted bounded projection or FTS representation without assuming json_extract(n.details,''$.return_type'') exists. Resolve malformed/wildcard filter inputs safely. INDEX_SEMANTICS_VERSION in src/core/config.rs incorporates FORGE_LANGUAGE_PROFILE_DIGEST, whose build.rs input includes writer.rs but not ingest.rs: bump version explicitly on changed projection to force stale persisted indices/caches to rebuild, with no build.rs edit. Scope amended to exact tests/mcp/query_ranking.rs, src/db/writer.rs and src/core/config.rs for these verified owners; correct docs/reference/mcp-tools.md admitted (tools.md is absent). Entry point/topology downstream already delivered; keep outputs unchanged. Red proof: cargo test --test mcp query_ranking::hybrid_search_ranks_connected_exact_symbols_before_path_order exit 101 at runtime for missing GateProfile; returns:/takes:/decorator cases are present but not reached and are not yet proven. Contract author/build must add negative/role collision, return-only, decorators, MCP+CLI, changed/delete/reopen and cold/delta root cases to same seam; run full mcp target and strict Clippy before review. Test-owned temp workspace; no external services. Baseline exact-name ordering is asserted first.'
    - 'decisions: Replan: acceptance explicitly requires Rust derive attribute search. Rust LanguageProfile::metadata owner src/engine/languages/rust/mod.rs:1061 returns SymbolMetadata without decorators for both owner/no-owner paths. Visitor calls the LanguageProfile metadata seam; indexing cannot recover missing attribute from persisted metadata. Add exactly that production owner to task scope; preserve existing contract, no schema change. Builder red `cargo test --test mcp query_ranking::hybrid_search_ranks_connected_exact_symbols_before_path_order` exit 101, left Null / right Example for #[derive(Debug)] struct Example, archive 2ab7e48ef50a2281; other typed assertions passed before this assertion. Require actual Rust production extraction -> stored metadata -> FTS -> MCP result; cold/delta and cache invalidation remain necessary. Builder retains unstaged production changes; do not reset.'
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
target: Add operation='untested' and operation='implementations' to code_map_query and traversal engine, surfacing test coverage gaps and class/trait hierarchies from existing SQLite edges
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- type-and-signature-aware-node-search-and-ergonomics
concurrency:
  mode: speculative
  sync_on: snapshot
scope:
- src/db/traversal.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/cli/mod.rs
- docs/reference/tools.md
- docs/reference/mcp-tools.md
- skills/contextunity-forge/SKILL.md
- tests/mcp/context.rs
status: completed
subtasks:
- subtask_ref: untested-symbols-traversal-operation
  title: Implement operation='untested' in src/db/traversal.rs finding production functions/methods with 0 inbound is_test=1 call edges in the selected path
  status: completed
  evidence: Snapshot 2bc8dd7; Clippy and MCP 90/90; production seam, independent review and seam review passed.
- subtask_ref: implementations-hierarchy-traversal-operation
  title: Implement operation='implementations' in src/db/traversal.rs traversing extends and implements edges for a given class or trait selector
  status: completed
  evidence: Snapshot 2bc8dd7; Clippy and MCP 90/90; production seam, independent review and seam review passed.
- subtask_ref: agent-guidance-for-test-gaps-and-hierarchies
  title: Document untested and implementations query operations in skills/contextunity-forge/SKILL.md and tool references
  status: completed
  evidence: Snapshot 2bc8dd7; Clippy and MCP 90/90; production seam, independent review and seam review passed.
receipt:
  contract_revision: 5
  passed_at: 2026-10-29T22:35:05.925337491+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/mcp/context.rs::stdio_compact_navigation_preserves_symbol_and_page_contracts
      tests_passed: 90
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent reviewer verified six scoped paths and protected files against snapshot.
        claims:
          applicable: true
          evidence: Reviewed direct test-call AND, signatures, transitive cycle-safe type relations, CLI/MCP and production seam; builder reports MCP 90/90.
        concurrency:
          applicable: true
          evidence: New operations are read-only, pagination uses existing QueryBudget and generation checks.
        project_isolation:
          applicable: true
          evidence: Reviewed path boundaries, ambiguous selectors and workspace-scoped SQLite.
        administration:
          applicable: true
          evidence: No schema changes; protected scope and candidate verified.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
    - 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
    - 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
    - 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
    architectural_notes:
    - 'decisions: Rev5 synthesis: producer stored graph nodes/edges -> traversal::query_with_path_options -> CLI query run and MCP code_map_query -> paged JSON. Guide SQL `(e.kind=''calls'' OR caller.is_test=1)` contradicts target: exclude only an inbound `calls` from a test caller (`AND`); production callers and non-call/possible_calls test relations do not count. Direct indexed caller metric is not proof of runtime coverage. Hierarchy child->base stored relations use `inherits` in Python/TS/Java plus `implements`; do not blindly trust guide `extends` alone; avoid returning method-level implementation edges as concrete types; respect selector disambiguation, path bounds, deterministic ordering, pagination/generation and query budget. MCP Query struct src/mcp/tools/inputs.rs:302 lacks admitted example `target`; transport rejects it as rejected_arguments, so admit exact owner plus tests/mcp/context.rs and live docs/reference/mcp-tools.md (tools.md absent). No migration, Cargo.toml or build.rs required. Extend existing parity seam: real Python Base/Child + production and test callers, MCP stdio and CLI. Architect observed cargo test --test mcp context::stdio_compact_navigation_preserves_symbol_and_page_contracts compile then exit 101 rejected_arguments target; hierarchy assertion not yet reached. Follow-up test matrix must cover no-test, direct-test, production-call, test non-call, multi-hop hierarchy, duplicate/ambiguous selectors, exact path-vs-prefix collisions, pagination and CLI parity. No test/source writes outside admitted scope.'
    - 'decisions: Coordinator resolved #1315: audit the full fixture and preserve both explicit freshness assertions while updating 7→10 to reflect three added source files. Audited all freshness references in the test; only JSON and Markdown contain a file count. Both now assert 10; retrying seam once.'
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
