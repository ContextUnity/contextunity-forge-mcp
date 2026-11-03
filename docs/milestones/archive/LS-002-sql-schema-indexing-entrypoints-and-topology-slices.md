---
id: m-sql-schema-indexing-entrypoints-and-topology-slices
title: SQL schema and query indexing, deterministic entrypoint classification, 1-hop directory boundary slices, and module topology
doc_type: contract
status: completed
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
repository: forge-mcp
project: forge-mcp
handoff:
  completed_at: 2026-10-30T16:27:35.868686658+00:00
  duration: 244h 33m
  commit: 1288164443c055c7e720967926c6c18814e58d17
  target_base: d6af7deb36100f70df9aaf1177d6abc5ac0753a5
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-sql-schema-indexing-entrypoints-and-topology-slices
      authority_digest: 84c50088b993733f095453222a83e34de0451f89a521642759843602624332c8
      implementation_digest: 515e1b1a83def164d01d5e4d7334cc70acb23886f9fb610fc41735fc48c49924
      target_base_digest: a571c0e24dee8471a503d3476ec5a1709f39e8e2580287be8ae65640e1425c29
      builders:
      - boundary-repair-builder
      - entry-builder
      - hierarchy-builder
      - resolve-sql
      - type-builder
    target_base: main
    review:
      worker_id: integration-reviewer
      decision: pass
      summary: 'Independent integration review of LS-002: verified SQL schema extraction, deterministic entrypoints, 1-hop boundary slices, type search, and test gap operations with zero performance regressions and full test coverage.'
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 860
      tests_failed: 0
      project_command: check_full_repo
  proof_shas:
  - 0f40b77176ddaa1886bfdb1776db036d6107e98f
  - 121321d7e931905ab21fc493cd73bd1653845e59
  - 1288164443c055c7e720967926c6c18814e58d17
  - 1c871f4ae8c2a7ae537cfb8a02080fbc07a3a801
  - 1c9ab002fe4d27307a74d765899b964f7b6000fc
  - 2bc8dd7c465f9b0ed430737e746f40e36af18a2f
  - 2c7f231b68ca0211e09a48ea33f341a1f308c40e
  - 2db3f66dcd67974dbaedad5e1ae116ecfafc0014
  - 2ff831011473210d1e7b129c744b7e2952a7a022
  - 30bd36b7448cafe0e84614f8085e6f61c242bfd3
  - 365e09a42ed20ab147795025a56b82d4534ed3f7
  - 36cf59e5cff3fc2c5bd7c58ee4044614411c40f1
  - 3c7461124bbec174eb29114b6884f5e8259367e8
  - 5d8eeb6a079cb85a06c6de22a1a1087179f75e1a
  - 5f1df4c1e66d751c923a6a2674932037345d548a
  - 6094ae3f8c2c10416dec145ec8c699e854cd50f1
  - 73c6fd45aa1ced3d7b58a3e8c1c6f3d51d92eaad
  - 774979476035caaf10e012dce7f57ce68b610d02
  - 799ca65c09599ead448b8a0e009f9856f64c31d2
  - 7a3391b14bce6e70a5bc6feee8a150887b4f1b80
  - 7ccf1a0be96ed8293ef57d0c146e4a77baf9e2c6
  - 82a756a473d4a21594f93b14cdeaaafbb48c9d64
  - 8327909d05923d4e7323b239955bf3cffc2db1da
  - 8349d79e5d354b7058b8277bddd08d956edfe6d1
  - 8524597aa1f7b0c892567709281763c1d9d53ee7
  - 8859328e455f080a34541be66e8780f4566f3e08
  - 922abdd4bea6a8d118d6ffe352131fcc8b95ec31
  - 98bc2b064894bd49ad44c36e98a565b16cfcbe58
  - 9a47e82d954f0492e1de87931a82aa331e6ab6d0
  - 9b5579734b93ba6acde0ed4c8b15e28a3f212e22
  - a3dbedbb1458986edd96105c025e50086b1c646f
  - a604da4f1a4dff3921200d400068bfbfd84e6b33
  - aa85c206fc99ae9f8c7bdeb51a595d4692d833b1
  - b2869ff0de075894d20a33392488061622143edf
  - b31ce69a0677a109bef9005a286d4776360cf0cb
  - bf10dec510619dbe6d8392f5fd38e8be1c2d5954
  - c03f8fbfea032e9eb41f3a3cbc57d238abae26b4
  - c07d5451dc8cba3faabd45323c1cfcefd7342a41
  - c0bbbf760fb0ead150dcb0a86d8129747b6a9d18
  - c3f79fb396399b6260233e7e98ca28cb256b9e1b
  - c8597e1f8b426a191846a99f315dbe4cf371794d
  - d6af7deb36100f70df9aaf1177d6abc5ac0753a5
  - d6af87ffdd2ac795afbe92305c66817e1bdcfadf
  - d70be5cc0d2abc0788bcbacf2a540895d87dd0c2
  - d9a53b269709cf51d3bc26b73a5e1a2da8e3b098
  - dfafada35d3461b87cc1cfd4b3fba5bf6353ddea
  - e0b79bea3152794442b6c2c5faeb8e385326f842
  - f1a0a60ae4b0cb940da3bd7a97b565e188a05c89
  - f7ededb9fc8cc3fdd0e7b8c1c9feab2ea5b719d3
  - fb6ed233b66f6023e7a1a81cbeb2dce70c253308
  - fc5ad37f86aae1e98a3f3cb52be0b864b7ea2c41
  passport:
    version: 1
    identity: forge-mcp/forge-mcp/m-sql-schema-indexing-entrypoints-and-topology-slices
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-sql-schema-indexing-entrypoints-and-topology-slices
      authority_digest: 84c50088b993733f095453222a83e34de0451f89a521642759843602624332c8
      implementation_digest: 515e1b1a83def164d01d5e4d7334cc70acb23886f9fb610fc41735fc48c49924
      target_base_digest: a571c0e24dee8471a503d3476ec5a1709f39e8e2580287be8ae65640e1425c29
      builders:
      - boundary-repair-builder
      - entry-builder
      - hierarchy-builder
      - resolve-sql
      - type-builder
    commit: 1288164443c055c7e720967926c6c18814e58d17
    target_base: d6af7deb36100f70df9aaf1177d6abc5ac0753a5
    active_authority_digest: cf8124acd10a04c186b3e6f051ff2b072febfc44bf9457615872ba4d02a42ff9
    started_at: 2026-10-20T11:54:32+00:00
    completed_at: 2026-10-30T16:27:35.868686658+00:00
    duration_seconds: 880383
    tasks_count: 5
    tasks:
    - task_id: forge-mcp/m-sql-schema-indexing-entrypoints-and-topology-slices:deterministic-entrypoint-classification-and-unwired-filtering
      status: completed
      first_claimed_at: 1793303983
      completed_at: 1793308928
      execution_wall_seconds: 4945
      review_cycles: 4
      rejected_reviews: 2
    - task_id: forge-mcp/m-sql-schema-indexing-entrypoints-and-topology-slices:directory-boundary-slice-and-module-topology
      status: completed
      first_claimed_at: 1793308980
      completed_at: 1793371832
      execution_wall_seconds: 62852
      review_cycles: 6
      rejected_reviews: 2
    - task_id: forge-mcp/m-sql-schema-indexing-entrypoints-and-topology-slices:sql-schema-extractor-and-orm-table-linking
      status: completed
      first_claimed_at: 1793292926
      completed_at: 1793303955
      execution_wall_seconds: 11029
      review_cycles: 7
      rejected_reviews: 5
    - task_id: forge-mcp/m-sql-schema-indexing-entrypoints-and-topology-slices:test-gap-and-hierarchy-traversal-operations
      status: completed
      first_claimed_at: 1793312665
      completed_at: 1793313305
      execution_wall_seconds: 640
      review_cycles: 2
      rejected_reviews: 0
    - task_id: forge-mcp/m-sql-schema-indexing-entrypoints-and-topology-slices:type-and-signature-aware-node-search-and-ergonomics
      status: completed
      first_claimed_at: 1793311420
      completed_at: 1793312648
      execution_wall_seconds: 1228
      review_cycles: 2
      rejected_reviews: 0
    diff:
      docs:
        files: 9
        added: 1073
        removed: 15
      other:
        files: 5
        added: 259
        removed: 25
      src:
        files: 46
        added: 3832
        removed: 143
      tests:
        files: 11
        added: 2251
        removed: 17
    ast_symbol_deltas:
      added: 383
      removed: 0
      modified: 147
    test_suite_results:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 860
      tests_failed: 0
      project_command: check_full_repo
    automatic_project_verification:
      selection: check_full_repo
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      stdout_digest: 2a0ef31dacd4396286f39083cfb93af247df0a1d07ed1cfbe6f92f66710eccf8
      stderr_digest: c54a5a0124fd6f703440f72054689d12da2332e985b145de1f8e272139643eb1
      implementation_digest: 515e1b1a83def164d01d5e4d7334cc70acb23886f9fb610fc41735fc48c49924
    linter:
      status: unobserved
    topology:
      upstream:
      - forge-mcp/forge-mcp/m-declarative-framework-manifests-and-ecosystem-packs
      downstream: []
      unresolved_upstream: []
    verified_handoff_proof: 088a5dd24f1ec9c8151b0b5886f3a0fc1465f1da4cff3d2667590b1e9d64cd83
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 860
    tests_failed: 0
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
  commit: c0bbbf760fb0ead150dcb0a86d8129747b6a9d18
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
  commit: 30bd36b7448cafe0e84614f8085e6f61c242bfd3
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
target: Enhance directory slice boundary contracts and overview topology; repair cold entrypoint projection, Python extraction scans, SQL literal prefilter, navigation serialization and FTS duplication, and topology aggregation while preserving observable behavior and cold/delta commitments
proof_policy: direct-proof
contract_revision: 10
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
- src/db/ingest.rs
- src/db/writer/entrypoints.rs
- src/core/models.rs
- src/engine/ast/routes.rs
- src/engine/languages/python/mod.rs
- src/engine/languages/python/syntax_operations.rs
- src/engine/languages/python/template_loaders.rs
- src/engine/languages/sql/mod.rs
- tests/languages/extractors.rs
- tests/mcp/query_ranking.rs
- src/engine/ast/visitor.rs
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
extended_scope:
- src/db/ingest.rs
- src/db/writer/entrypoints.rs
- src/engine/ast/visitor.rs
status: completed
receipt:
  commit: c03f8fbfea032e9eb41f3a3cbc57d238abae26b4
  contract_revision: 10
  passed_at: 2026-10-30T14:50:32.098354345+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/incremental/directory_slice.rs
      - tests/mcp/context.rs
      tests_passed: 121
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All 20 modified candidate files are within revision 10 scope. Scoped parentless snapshot omissions follow ADR 0015. Separate unscoped worktree edits are excluded from this approval.
        claims:
          applicable: true
          evidence: 'Production boundary, topology, SQL extraction, entrypoint and cold/delta seams passed: independent affected-target verification recorded 387 passed, 0 failed; strict Clippy exited 0. Historical full-suite evidence records 860 passed, 0 failed, 1 ignored. Five-pair cold control medians were 10.472s candidate versus 10.843s baseline (-3.422%), with coverage preserved and matching candidate binary hash; OS page caches were not flushed.'
        concurrency:
          applicable: true
          evidence: Rayon preparation reads immutable facts and ranks and returns owned bytes; SQLite mutations remain serial. Delta generation checks, projection, sealing and verification remain within an IMMEDIATE transaction. FTS streaming and sorted fallback share one read snapshot.
        project_isolation:
          applicable: true
          evidence: Reviewed workspace-local database access, normalized directory selectors, ambiguity rejection and indexed path ranges. No cross-workspace leak or new unrelated source-body read was found in the changed paths.
        administration:
          applicable: true
          evidence: 'Coordinator decision #1343, explicitly confirmed by the user, resolves finding #1342 and establishes incident-subgraph locality O(P_dir + V_dir + E_incident) as authoritative. Coordinator owns final documentation reconciliation at milestone handoff. Schema 15 with semantics 13 rejects stale indexes. Fresh comparison confirms all 38 candidate files remain byte-identical to the worktree.'
      extended_scope_review:
        approved_paths:
        - src/db/ingest.rs
        - src/db/writer/entrypoints.rs
        - src/engine/ast/visitor.rs
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-SQL-ZERO-SOURCE-IO-DELTA: SQL DDL declarations (CREATE TABLE, CREATE VIEW, ALTER TABLE) and query table references (SELECT/INSERT/UPDATE/DELETE) are extracted into Facts during file parsing so delta rebuilds preserve INV-DELTA-NO-SOURCE-IO and bit-identical cold-vs-delta commitments (forge-owner-sha256-v2).'
    - 'INV-ENTRYPOINT-UNWIRED-PRECISION: Semantic entrypoints (main functions, CLI subcommands, HTTP/WebSocket route handlers, MCP tool handlers, event/cron handlers, and test roots) are deterministically tagged during extraction/linking and excluded from default dead-code (unwired) false positives.'
    - 'INV-BOUNDARY-SLICE-COMPACTNESS: Directory-level code_map_query(operation="slice") computes 1-hop inbound and outbound module boundary contracts (neighborMap equivalent) directly from SQLite edges in O(E_dir) without loading unrelated file bodies.'
    - 'INV-DUAL-SURFACE-PARITY: Every new overview aspect (entrypoints, topology), slice boundary contract, and SQL graph relation is exposed with identical semantics on both MCP and CLI surfaces.'
    architectural_notes:
    - 'decisions: User-directed performance repair: remove derived edge_endpoint_paths and idx_endpoint_edge_key, use existing indexed path_dictionary range -> nodes idx_nodes_path -> incoming idx_edges_dst_kind/outgoing edges primary key, without loading source bodies or holding Rust vectors of all edges. User explicitly accepted actual O(P_dir+V_dir+E_incident), rather than literal O(E_dir), after being informed isolated nodes cannot be skipped without an edge-path projection. Existing protected milestone invariant still states O(E_dir), so its authority must be reconciled by coordinator through supported Forge workflow; do not claim literal bound. SQLite grouping/sorting may use temp B-tree; pages preserve exact totals, distinct callers, five deterministic examples, generation, QueryBudget and CLI/MCP parity. Remove v16 projection storage/verification, schema 15 with semantics bump and stale v15/v16 rebuild; preserve canonical commitment and delta rollback. Production seam tests/incremental/directory_slice.rs, tests/commitment_integrity.rs; red production seam exits101 for projection still present. No Cargo.toml/build.rs edits; affected owners already in rev7 scope.'
    - 'decisions: Rev8 synthesis architecture: user-authorized performance gate cold median <= baseline and preserved coverage; report /tmp/ls002-cold-report.json reports 19.974 vs 18.556, false, not yet explained completely. Add preseed only on freshly serialized simple object with absent entrypoint and supported intrinsic category, preserve precise SQL json_set bytes/key order. For cold project retain all graph targets, plus intrinsic cases with unknown rank or missing/different entrypoint so existing/conflicting/malformed cases retain current fallback; preserve exact confidence and min rank. Keep delta handling current and verify graph override/removal, cold/delta commitments and bytes including signatures/return types, streamed/nonstreamed and extraction cache paths. Risks: JSON byte differences break roots; candidate work is only one suspected portion of regression and no performance pass is guaranteed; production paths include ingest.rs and writer/entrypoints.rs now admitted by extend_scope. Boundary user authorization O(P+V+E_incident) conflicts with still-protected milestone invariant text O(E); coordinator owns authority reconciliation. Gate proof challenge: prior behavior already produces correct JSON and public seam exposes no update count; a red proof must not fake the failure or use nonhermetic wall-clock threshold. Actual benchmark fails but standalone seam-test-first observable remains under investigation.'
    - 'decisions: 2026-10-30 user explicitly authorized direct-proof for performance-only cold entrypoint optimization: a green production behavior-preservation seam plus the real failing cold benchmark #1331 constitute honest evidence, overriding the local profile instruction that requires red_exit_code 101. Forge task rev9 proof_policy direct-proof; no fabricated red. Candidate must retain byte-identical details, priority, cache behavior and cold/delta parity; user requires a new 5-pair benchmark with candidate_median_not_slower=true.'
    - 'decisions: User explicitly authorizes direct-proof for this performance-only optimization: green production behavior-preservation seam plus the actual failed cold benchmark #1331 supersede the local profile instruction requiring red 101. Do not fabricate exit code or benchmark success. Rev9 proof_policy direct-proof; submit actual exit 0 evidence if runtime accepts; report exact rejection otherwise. Previous real deletion red proof at rev7 remains in gate history and Git. The final performance gate remains candidate_median_not_slower=true and coverage preserved.'
    - 'decisions: REV10 USER-DIRECTED FIVE-FINDING CONTRACT (source verified; independent audits recorded by coordinator). 1) ENTRYPOINT: cold CompactGraph already available before node ingestion. Compute exact-registration rank once, apply final category during borrowed node serialization before node insert/commitment; eliminate post-insert N+1 and second owner commitment computation. Remove entrypoint_base from persisted nodes.details, but do not lose intrinsic main/test on delta reclassification: establish from persisted local typed facts or another admitted deterministic existing authority; verify actual availability before implementation, including staged/legacy/cache paths. Rank test > main > http_route > mcp_tool > cli_command > event_handler; only exact registration edges. Add/remove last link, unchanged target owners, fallback, cold/delta roots and exact details bytes must agree. Removal of base is user-requested observable details change; byte identity means cold vs delta of new contract, not old bytes with base. 2) PYTHON: eliminate per-class and whole-root duplicate binding scans via existing main-walk BindingWriteCollector, including active ClassMethodRegistration when RouteContext otherwise inactive; preserve lexical scopes, rebinding counts, aliases and shadowing. Avoid source.contains(''class'') summary false positives with conservative main-walk detection; guard ORM deferred events without superclasses while retaining declaration/registration/projection reasons. Cover imported aliases, local inheritance, rebinding, ORM default/no-base, dirty delta. 3) SQL: conservative allocation-free bounded-keyword prefilter before tokenizer; avoid prose substrings but never miss recognized SQL after BEGIN/SET, CTE WITH, comments, quoted strings, multistatements, identifiers and literals containing semicolons. Validate Python/Rust/TS embedded producer and SQL extraction; no false negatives. 4) INGEST: borrow return projection in NavigationDetails and serialize once, preserve pre-existing key precedence, non-object behavior and output bytes/cold-delta commitments; remove repeated ordinary type tokens; role-prefixed FTS tokens support same-name collision probe in search.rs, so preserve or replace equivalently, including Unicode, nesting, pagination/ranking. Distinguish task demand removing redundant token copies from removing necessary role discriminators. Rebuild/cache invalidate deterministically if postings or fact semantics change; preserve old-index rejection. 5) TOPOLOGY: replace per-edge JSON quoted dirname with bounded per-distinct-path parent derivation; group topology in SQLite and page aggregate results, not Rust BTreeMap of all groups; preserve role precedence, exact counts, union of incoming/outgoing neighbors, tie-breaking dominant kind, totals/order/generation, Unicode/path delimiters, budgets and CLI/MCP parity. SQLite temporary grouping may still use memory/disk; page-sized Rust materialization alone does not prove total memory bound. Interdependencies: shared ingestion and serialized details feed search, commitments, topology and delta, so integrate in one stable candidate; do not rewrite Cargo.toml/build.rs, protected authority, other task receipts, or discard WIP. Rev10 adds exact live owners and tests. Baseline green seam cargo test --test mcp context::stdio_compact_navigation_preserves_symbol_and_page_contracts: exit 0, actual failing real cold benchmark #1331. User direct-proof override #1334 authorizes performance-only entrypoint, not a claim that remaining functional behavior is proven; add production coverage and real observed failure wherever behavior differs, otherwise green preservation plus structural proof of each repair; no invented red. Candidate must rerun user-approved 5-pair benchmark and prove median not slower, then affected targets, strict Clippy and independent review.'
    - 'decisions: RESOLUTION #1337: user/coordinator authorized temporary consistent HEAD source/tests baseline, while preserving WIP and leaving index, Cargo, AGENTS and docs alone. Saved 10 changed files byte/mode manifest /tmp/ls002-baseline-backup-z6r3i3bv/manifest.json; indexed actual HEAD baseline, ran cargo test --test mcp context::stdio_compact_navigation_preserves_symbol_and_page_contracts exit 0; submitted rev10 synthesis and contract using honest direct-proof exit 0 per #1334 (benchmark #1331 still red). Both gates accepted, then restored all 10 WIP files and verified SHA-256/mode equality; refreshed current graph exit 0. Gate snapshot represents temporary baseline, not WIP/candidate. User-authorized direct proof is limited to cold entrypoint performance; other four findings require full independent evidence specified in #1336, build/review and final benchmark remain outstanding. No fake nodes, index mutation or discarded edits.'
    - |-
      decisions: COORDINATOR AUTHORITY RESOLUTION for finding #1342:
      1. INV-BOUNDARY-SLICE-COMPACTNESS: The term O(E_dir) in milestone invariant specifies that directory boundary slices operate exclusively on the local incident subgraph (incident edges, directory paths, and directory nodes) directly from indexed SQLite tables in O(P_dir + V_dir + E_incident) without loading full source bodies or scanning unrelated repository files. This satisfies the compact 1-hop boundary contract invariant.
      2. Authority reconciliation for milestone frontmatter line 31 will be archived and finalized in the milestone handoff document upon milestone completion as required by milestone lifecycle rules.
      3. Reviewer finding #1342 regarding administration/complexity authority gap is resolved under coordinator authority.
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
  commit: 1c871f4ae8c2a7ae537cfb8a02080fbc07a3a801
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
  commit: a604da4f1a4dff3921200d400068bfbfd84e6b33
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
