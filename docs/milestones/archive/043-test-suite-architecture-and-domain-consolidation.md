---
id: m-test-suite-architecture-and-domain-consolidation
title: Test suite architecture, domain consolidation, and harness standardization
doc_type: contract
status: completed
depends_on:
- m-framework-manifests-and-parser-modularity
- m-tool-performance-and-storage-compaction
- m-universal-ast-search-and-tool-navigation
owners:
- tests/
invariants:
- 'INV-CENTRAL-VS-LANGUAGE-ISOLATION: Central subsystem suites (acdd, core, linker, incremental, mcp, commitment_integrity) reside at tests/; all language extraction and semantic tests are strictly isolated inside tests/languages/.'
- 'INV-ACDD-SUITE-COHESION: All ACDD state machines, task lifecycles, subtasks, blackboard messages, and milestone operations reside in tests/acdd/ rather than core storage or MCP symbol modules.'
- 'INV-SHARED-HARNESS: Common workspace setup, file writing, database building, and MCP client lifecycle are managed through canonical shared helpers in tests/common/ rather than duplicated struct Workspace per test file.'
- 'INV-DOMAIN-DECOMPOSITION: Test modules are organized by cohesive architectural and contractual purpose. Monolithic files are split along natural architectural sub-boundaries.'
- 'INV-FORWARD-COMPAT-041-042: Language suites cleanly decouple pure grammar from framework manifests (frameworks.rs / manifests.rs) for milestone 041, and isolate universal AST patterns and path-scoped selectors (ast_patterns.rs, selectors.rs, navigation.rs) for milestone 042.'
- 'INV-CONTRACT-PARITY: Refactoring preserves 100% of observable positive contract assertions, fail-closed boundaries, and Merkle tree determinism (commitment_integrity).'
- 'INV-TABLE-DRIVEN-PERMUTATIONS: Multiple assertions over syntax permutations, keyword tables, or builtins use table-driven loops instead of copy-pasted test functions.'
related_plans: []
started_at: 2026-10-08T06:01:25+00:00
handoff:
  completed_at: 2026-10-08T18:51:09.941220240+00:00
  duration: 12h 49m
  commit: 022e2102b80e0b38c1576f7df1d73c43ebeb6f0c
  verification:
    command: cargo test --all-targets
    status: passed
    tests_passed: 637
    tests_failed: 0
---

# Test suite architecture, domain consolidation, and harness standardization

## Outcome and purpose

Consolidate 42 independent Cargo test executables into coherent central domain suites with a dedicated `tests/acdd.rs` layer and an isolated `tests/languages/` subtree, standardize shared fixtures in `tests/common/` to eliminate 37 duplicated `Workspace` harnesses, decompose monolithic test files along natural architectural sub-boundaries, and adopt table-driven tests without weakening contract coverage or Merkle determinism invariants.

Work starts after active milestones 014, 030, 041, 042 conclude, ensuring clean merge baseline and avoiding rebasing collisions across parallel agents.

## Tasks in this milestone

### task: common-test-harness-and-workspace

```yaml
task_ref: common-test-harness-and-workspace
target: Establish canonical shared test harness in tests/common/ for workspace lifecycle, database construction, and MCP testing
proof_policy: seam-test-first
scope:
- tests/common/
status: completed
subtasks:
- subtask_ref: canonical-test-workspace
  title: Implement tests/common/workspace.rs with RAII temp dir, file writing, and build/delta execution
  status: completed
  evidence: cargo test --test core workspace_contract_tests::workspace_builds_and_applies_delta_in_its_owned_temp_directory passed; exercises production writer build/delta, reader, and RAII cleanup.
- subtask_ref: canonical-assertions-and-mcp-client
  title: Implement tests/common/assertions.rs and tests/common/mcp_client.rs for unified assertions and fast in-process/subprocess client spawning
  status: completed
  evidence: 'Independent review confirmed tests/common/assertions.rs and mcp_client.rs are exercised across the public seams: harness_contract.rs covers in-process Server, stdio StdioClient, payload/assertion helpers, and CLI; consolidated MCP suites consume these helpers. Central accepted build passed 216 tests (2 ignored) and strict Clippy. Common snapshot fdfd133 contains exactly the allowed common files and passed independent review.'
receipt:
  commit: fdfd133b8c1f381be9d569ab4e957f1606bb8221
  contract_revision: 1
  passed_at: 2026-10-08T08:06:59.453931227+00:00
  evidence:
    test_proof:
      command: cargo test --test core workspace_contract_tests::workspace_builds_and_applies_delta_in_its_owned_temp_directory
      exit_code: 0
      tests_passed: 1
      tests_failed: 0
      log: The core target ran the workspace lifecycle contract through production build, delta, read, and RAII cleanup.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Snapshot fdfd133 contains exactly the five files in the task's tests/common/ allowlist. Each current file matches the snapshot blob, and git diff --check 476fc1f fdfd133 -- tests/common is clean.
        claims:
          applicable: true
          evidence: Workspace provides collision-safe RAII roots, guarded relative paths, writing, production writer build/delta, and reader opening; its contract test exercises write/build/read/edit/delta/read/Drop cleanup. Shared assertions cover command status, JSON-RPC errors, and MCP payloads. The client provides real in-process Server, CLI, and initialized subprocess stdio seams, including custom binary, request/call/payload, PID access, frame-size bound, timeout, and kill/wait cleanup. The central MCP review confirmed the repaired suites consume these helpers; its accepted build includes the harness contract and reports 216 passed, 0 failed, 2 ignored with strict Clippy passing. Therefore canonical-assertions-and-mcp-client is fully evidenced.
        concurrency:
          applicable: true
          evidence: Workspace naming includes process ID, timestamp, and an atomic sequence and uses exclusive create_dir with collision retry. Stdio responses are read through a channel and bounded timeout; each spawned child is killed and waited on Drop. Central reader/rebuild tests retain explicit channel synchronization.
        project_isolation:
          applicable: true
          evidence: Each Workspace owns a unique temporary root, rejects absolute paths and parent traversal, and removes only its own root on Drop. build/open/client helpers bind operations to that root; foreign-root tests allocate separate Workspace values.
        administration:
          applicable: true
          evidence: Reviewer codex-043-common-review-final-v1 differs from builder codex-043-common-builder. The accepted build snapshot fdfd133 includes the production-seam workspace contract test (1 passed, 0 failed). I made no edits; common files match the reviewed snapshot.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CENTRAL-VS-LANGUAGE-ISOLATION: Central subsystem suites (acdd, core, linker, incremental, mcp, commitment_integrity) reside at tests/; all language extraction and semantic tests are strictly isolated inside tests/languages/.'
    - 'INV-ACDD-SUITE-COHESION: All ACDD state machines, task lifecycles, subtasks, blackboard messages, and milestone operations reside in tests/acdd/ rather than core storage or MCP symbol modules.'
    - 'INV-SHARED-HARNESS: Common workspace setup, file writing, database building, and MCP client lifecycle are managed through canonical shared helpers in tests/common/ rather than duplicated struct Workspace per test file.'
    - 'INV-DOMAIN-DECOMPOSITION: Test modules are organized by cohesive architectural and contractual purpose. Monolithic files are split along natural architectural sub-boundaries.'
    - 'INV-FORWARD-COMPAT-041-042: Language suites cleanly decouple pure grammar from framework manifests (frameworks.rs / manifests.rs) for milestone 041, and isolate universal AST patterns and path-scoped selectors (ast_patterns.rs, selectors.rs, navigation.rs) for milestone 042.'
    - 'INV-CONTRACT-PARITY: Refactoring preserves 100% of observable positive contract assertions, fail-closed boundaries, and Merkle tree determinism (commitment_integrity).'
    - 'INV-TABLE-DRIVEN-PERMUTATIONS: Multiple assertions over syntax permutations, keyword tables, or builtins use table-driven loops instead of copy-pasted test functions.'
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

## Task commits

- `common-test-harness-and-workspace`: `4953bf5c760549683df3abca9bb78a9c8c345a33`
- `acdd-suite-extraction-and-consolidation`: `77cd21e498b25f6487067c2763b3b64e63e8eb88`
- `central-engine-suites-consolidation`: `a09d8a916fda1a6c506ab6761d9e909da04a7865`
- `language-tests-isolation-and-monolith-decomposition`: `cccfb51e18475cc00d4e83b1b19b665ea9182ecc`
- `full-test-suite-quality-refactor`: `022e2102b80e0b38c1576f7df1d73c43ebeb6f0c`

### task: full-test-suite-quality-refactor

```yaml
task_ref: full-test-suite-quality-refactor
target: Audit and refactor every repository test suite per test-suite-refactor, removing weak coverage and optimizing test execution
proof_policy: direct-proof
contract_revision: 3
scope_roots:
- tests/
- benchmarks/
- src/
scope:
- tests/
- benchmarks/
- src/core/models.rs
- src/core/semantic.rs
- src/db/dictionary.rs
- src/db/writer.rs
- src/db/writer/memory_budget_tests.rs
- src/db/writer/multi_value_batch_tests.rs
- src/db/tasks_store.rs
- src/engine/languages/build_manifest.rs
- src/engine/languages/dependency_registry.rs
- src/engine/linker/value_flow.rs
- src/engine/scanner.rs
- src/engine/scanner/adapter.rs
- src/mcp/metadata.rs
- src/mcp/response.rs
invariants:
- 'INV-TEST-CONTRACT-PARITY: Removed or rewritten tests retain every unique reachable observable contract through a stronger production or composition seam; uncertain cases are not deleted.'
- 'INV-TEST-ONLY-SOURCE: Source behavior, public APIs, schemas, and configuration remain unchanged; formatter-only normalization in src/db/tasks_store.rs is allowed to satisfy cargo fmt --check, while semantic source changes remain confined to test-only modules.'
- 'INV-TEST-REFACTOR-EVIDENCE: Every REMOVE and REWRITE has a classification and replacement map; final unique-test identities and any additions are reconciled against the frozen baseline.'
subtasks:
- subtask_ref: freeze-full-test-inventory
  title: Freeze Cargo test identities, target counts, setup inventory, production seams, and baseline runtime before edits
  status: completed
  evidence: 'Pre-edit baseline frozen in /tmp/043-test-ids-baseline.txt using `cargo test --all-targets -- --list`: 646 unique test identities across 15 Cargo test executables. `cargo test --all-targets` passed 643, failed 0, ignored 3. Sum of Cargo per-target reported runtimes: 113.49s; tests/languages 69.76s (255 tests), tests/mcp 17.25s (75 tests), tests/incremental 6.52s (23 passed, 1 ignored), tests/core 6.23s (26 passed, 1 ignored). Existing domain directories and canonical `tests/common/` harness were inventoried; no edits made.'
- subtask_ref: audit-weak-coverage
  title: Classify tests KEEP, REWRITE, REMOVE, or UNCERTAIN and map each rewrite or removal to retained seam coverage
  status: completed
  evidence: 'KEEP: scanner default 100,000-file stress case. REWRITE(harness only): cli_roots, impact_context, query_context, commitment_integrity and cold_sealing; observable assertion dimensions remain. REWRITE(one scenario): Python overflow case now proves one importing-provider reparse and one restored fact file during delta while retaining fail-closed checks. MOVE to benchmarks/: frozen main/linked cold/delta/MCP profile and 1,600-TOML document suffix baseline profile; no residual test-scope finding.'
- subtask_ref: consolidate-and-optimize-harnesses
  title: Remove weak duplicates, consolidate domain suites and shared fixtures, and reduce measured setup or execution overhead
  status: completed
  evidence: 'Shared Workspace and CLI harness consolidation remains; Python focused case improved 42.18s→23.82s. Added standalone benchmarks Cargo package with the two paired profile runners. Final tests/ diff: 133 insertions and 505 deletions (638 changed lines); profile Cargo identities are removed by relocation.'
- subtask_ref: prove-parity-and-final-gates
  title: Reconcile unique-test identities and added dimensions, then pass full tests, strict Clippy, commitment integrity, formatting, and independent review
  status: completed
  evidence: 'Final candidate snapshot 6764282 passed independent review/v1 at claim revision 19 (all five contours). Final cargo test --all-targets: 637 passed, 0 failed, 1 expected ignored stress test; cargo test --test commitment_integrity: 14/14; cargo clippy --all-targets --all-features -- -D warnings: pass; cargo fmt --check and cargo fmt --manifest-path benchmarks/Cargo.toml --check: pass; git diff --check: pass. Final inventory: 638 identities vs 641 immediately before this follow-up (7 rehomed, 3 removed, 0 added). Contract revision 3 permits only formatter-only line wrapping in src/db/tasks_store.rs; no semantic source change.'
- subtask_ref: migrate-ignored-profiles
  title: Move the two ignored baseline/candidate performance scenarios from tests/ into benchmarks/ and reconcile the Cargo test inventory
  status: completed
  evidence: 'Moved frozen_workspace_cold_and_delta_profile and profile_document_suffix_delta_against_baseline into benchmarks/src/bin/{frozen_workspace_profile,document_suffix_delta_profile}. `cargo check --manifest-path benchmarks/Cargo.toml --all-targets` and nested strict Clippy pass. Final identity comparison: 646→644; exactly the two profile identities removed, no additions. Full Cargo suite: 643 passed, 0 failed, 1 ignored.'
status: completed
receipt:
  commit: 676428289f89cc4d820506366c14a614ec11f1ad
  contract_revision: 3
  passed_at: 2026-10-08T15:38:44.386213482+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets
      exit_code: 0
      tests_passed: 637
      tests_failed: 0
      log: '1 expected ignored scanner stress test (>100,000 files). Standalone cargo test --test commitment_integrity: 14 passed. cargo clippy --all-targets --all-features -- -D warnings: pass. cargo fmt --check, cargo fmt --manifest-path benchmarks/Cargo.toml --check, git diff --check: pass. Final identity inventory: 638; immediate pre-follow-up 641 (7 rehomed, 3 removed, 0 additions). The task code/test diff is unchanged from the prior reviewed candidate; only the milestone task receipt was reopened and its completed-gate evidence retained.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Inspected snapshot 6764282 and the current worktree diff. Changes are confined to tests/, benchmarks/, the milestone manifest, and the single formatter-only line wrap in src/db/tasks_store.rs. The latest user steering to fix all formatting errors and contract revision 3 authorize that exact formatting-only normalization; no semantic source changes are present.
        claims:
          applicable: true
          evidence: 'Inspected the current replacement map in blackboard message 124 and verified it matches the candidate: the three weak tests are removed; the previously absent untyped_logger_parameters test is correctly marked as confirmation only; seven core_basics tests are rehomed, core_basics.rs is deleted, and the TTL waits are replaced by database cache-identity invalidation. Persisted logger assertions retain cold-build and delta coverage. Test inventory reconciles 641→638 (7 rehomed, 3 removed, no new dimensions). Current build/v1 claim 23 records all gates: 637 passed/0 failed/1 expected ignored; commitment integrity 14/14; strict Clippy; root and benchmark fmt checks; and git diff --check.'
        concurrency:
          applicable: true
          evidence: The shared invalidation helper mutates only the SQLite database owned by each unique temporary Workspace. Refresh sequences are local to each test; no shared database or process-global mutable state was added. Existing synchronized active-reader/rebuild coverage remains.
        project_isolation:
          applicable: true
          evidence: The fixtures, SQLite stores, CLI/MCP operations, and mtime invalidation all use disposable Workspace roots. The helper does not touch the developer workspace or benchmark reference corpus.
        administration:
          applicable: true
          evidence: Reviewer `codex-043-quality-receipt-review-v2` is distinct from build worker `codex-043-quality-receipt-build-v2`. Message 124 is the current replacement-map note; the stale prior receipt was not used. I made no file changes, commits, pushes, or task-scope changes.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CENTRAL-VS-LANGUAGE-ISOLATION: Central subsystem suites (acdd, core, linker, incremental, mcp, commitment_integrity) reside at tests/; all language extraction and semantic tests are strictly isolated inside tests/languages/.'
    - 'INV-ACDD-SUITE-COHESION: All ACDD state machines, task lifecycles, subtasks, blackboard messages, and milestone operations reside in tests/acdd/ rather than core storage or MCP symbol modules.'
    - 'INV-SHARED-HARNESS: Common workspace setup, file writing, database building, and MCP client lifecycle are managed through canonical shared helpers in tests/common/ rather than duplicated struct Workspace per test file.'
    - 'INV-DOMAIN-DECOMPOSITION: Test modules are organized by cohesive architectural and contractual purpose. Monolithic files are split along natural architectural sub-boundaries.'
    - 'INV-FORWARD-COMPAT-041-042: Language suites cleanly decouple pure grammar from framework manifests (frameworks.rs / manifests.rs) for milestone 041, and isolate universal AST patterns and path-scoped selectors (ast_patterns.rs, selectors.rs, navigation.rs) for milestone 042.'
    - 'INV-CONTRACT-PARITY: Refactoring preserves 100% of observable positive contract assertions, fail-closed boundaries, and Merkle tree determinism (commitment_integrity).'
    - 'INV-TABLE-DRIVEN-PERMUTATIONS: Multiple assertions over syntax permutations, keyword tables, or builtins use table-driven loops instead of copy-pasted test functions.'
    - 'INV-TEST-CONTRACT-PARITY: Removed or rewritten tests retain every unique reachable observable contract through a stronger production or composition seam; uncertain cases are not deleted.'
    - 'INV-TEST-ONLY-SOURCE: Source behavior, public APIs, schemas, and configuration remain unchanged; formatter-only normalization in src/db/tasks_store.rs is allowed to satisfy cargo fmt --check, while semantic source changes remain confined to test-only modules.'
    - 'INV-TEST-REFACTOR-EVIDENCE: Every REMOVE and REWRITE has a classification and replacement map; final unique-test identities and any additions are reconciled against the frozen baseline.'
    architectural_notes:
    - '{"replacement_map":{"REMOVE":[{"test":"languages::profiles::unsupported_invocations_and_unbound_aliases_remain_unresolved","reason":"Removed absence-only Ruby/C#/C++/Java linker assertions.","retained_contract":"Python unresolved-provider behavior remains covered by tests/languages/python/modules.rs::cyclic_reexports_and_missing_providers_remain_unresolved, which checks cold build, delta, unresolved coverage, and no call target."},{"test":"languages::python::semantics::logger_process_is_not_a_logger_method","reason":"Removed the standalone negative-only logger test.","retained_contract":"tests/languages/python/semantics.rs::logging_factory_methods_have_verified_builtin_origin_after_persistence covers known logger methods and unresolved logger.process/untyped logger.info across cold build and delta."},{"test":"test_adapter_change_auto_rebuild","reason":"Removed duplicate metadata-only adapter rebuild test.","retained_contract":"tests/mcp/freshness/stdio_and_response.rs::stdio_response_only_adapter_transitions_preserve_facts_and_continuations changes only adapter_version in its third transition and checks rebuild, files_checked, and generation."}],"CONFIRM":{"test":"untyped_logger_parameters_remain_unresolved_in_persisted_coverage","finding":"Already absent from frozen pre-follow-up inventory and checkout; no identity removed by this change.","retained_contract":"tests/receivers/python_value_flow.rs::persisted_untyped_logger_parameters_without_provider_stay_unresolved checks persisted logger.info and log.warning; logger factory cold/delta coverage also checks untyped logger.info."},"CORE_BASICS_MOVES":[["public_linker_preserves_arbitrary_edge_tags_and_orders_edges_stably","tests/linker/public_output.rs (registered in tests/linker.rs)"],["default_scope_excludes_common_generated_directories","tests/core/scanner_guard_limits.rs"],["rows_enforces_the_exact_serialized_json_limit","tests/core/reader_row_limits.rs"],["database_roundtrip_build_and_query","tests/core/build_query_roundtrip.rs"],["cold_build_preserves_extracted_storage_across_full_and_remainder_batches","tests/core/storage_batches.rs"],["navigation_storage_preserves_compressed_analysis_and_delta_calls","tests/core/fact_persistence.rs"],["hybrid_search_ranks_connected_exact_symbols_before_path_order","tests/mcp/query_ranking.rs"]],"CORE_BASICS_DELETE":"Deleted tests/core_basics.rs and removed its Cargo test binary. Markdown extraction behavior is covered through writer/reader document section and search assertions in database_roundtrip_build_and_query.","REWRITE":[["tests/mcp/context.rs","Removed wait_for_source_inventory_ttl(); all three callers use invalidate_mcp_connection_cache(database)."],["tests/mcp/freshness.rs","Removed wait_for_inventory_ttl(); callers invalidate cache identity instead of waiting for TTL."],["tests/core/adapters/source_only_adapter.rs","Replaced sleep(5100) with invalidate_mcp_connection_cache."],["tests/common/mcp_client.rs","Shared helper advances the disposable SQLite DB mtime by one second, making server admission observe a new cache identity."]],"inventory":{"immediate_pre_followup_identities":641,"final_identities":638,"rehomed":7,"removed":3,"added":0,"all_targets":{"passed":637,"failed":0,"ignored":1,"ignored_reason":"Expected >100,000-file scanner stress lane."}},"verification":{"commitment_integrity":"cargo test --test commitment_integrity: 14 passed, 0 failed.","all_targets":"cargo test --all-targets: 637 passed, 0 failed, 1 ignored.","clippy":"cargo clippy --all-targets --all-features -- -D warnings: passed.","formatting":"cargo fmt --check and cargo fmt --manifest-path benchmarks/Cargo.toml --check: passed. 49 existing formatter hunks were normalized; src/db/tasks_store.rs has formatter-only line wrapping and no semantic change.","diff_check":"git diff --check: passed."}}}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: acdd-suite-extraction-and-consolidation

```yaml
task_ref: acdd-suite-extraction-and-consolidation
target: Extract and consolidate the complete ACDD and task lifecycle layer into tests/acdd.rs and tests/acdd/
proof_policy: seam-test-first
contract_revision: 2
scope:
- tests/acdd/
- tests/acdd.rs
- tests/core_basics.rs
- tests/core_basics/
- tests/mcp_context/tasks.rs
status: completed
subtasks:
- subtask_ref: extract-task-lifecycle
  title: Relocate and modularize task lifecycle tests from core_basics/tasks.rs into tests/acdd/tasks.rs
  status: completed
  evidence: Relocated task lifecycle suite from tests/core_basics/tasks.rs to tests/acdd/tasks.rs; `cargo test --test acdd` passed (61 tests), and baseline identity comparison found no missing or extra task tests.
- subtask_ref: extract-subtask-and-blackboard-tests
  title: Modularize subtask deepening and blackboard tests into tests/acdd/subtasks.rs and tests/acdd/blackboard.rs
  status: completed
  evidence: Consolidated subtask and blackboard lifecycle tests into tests/acdd/subtasks.rs and tests/acdd/blackboard.rs; `cargo test --test acdd` passed (61 tests).
- subtask_ref: extract-milestone-and-mcp-task-tests
  title: Relocate milestone CLI tests and MCP task tool tests (mcp_context/tasks.rs) into tests/acdd/milestones.rs and tests/acdd/mcp.rs
  status: completed
  evidence: Moved milestone and MCP task tests into tests/acdd/milestones.rs and tests/acdd/mcp.rs; `cargo test --test acdd` passed (61 tests), with all 61 legacy task test names preserved.
receipt:
  commit: dea036b97a6f314562eb9906d985a50621e8a650
  contract_revision: 2
  passed_at: 2026-10-08T06:59:04.140056090+00:00
  evidence:
    test_proof:
      command: cargo test --test acdd
      exit_code: 0
      tests_passed: 61
      tests_failed: 0
      log: 'cargo test: 61 passed [1.81s]; git diff --check d7ddf7b -- tests/acdd.rs tests/acdd tests/core_basics.rs passed.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Inspected snapshot dea036b and its diff from d7ddf7b. Changed paths are tests/acdd.rs, tests/acdd/*.rs, tests/core_basics.rs, and removal of tests/core_basics/tasks.rs; all are in the task allowlist.
        claims:
          applicable: true
          evidence: 'Collect-only comparison against /tmp/043-test-ids-baseline.txt yields 61 identities on both sides, with no missing, extra, or duplicate names. The 812 legacy assertion macro lines match the candidate assertion-line multiset exactly. Accepted build proof for dea036b records cargo test --test acdd: 61 passed, 0 failed.'
        concurrency:
          applicable: true
          evidence: Retained blackboard concurrency coverage uses two independent SQLite connections and verifies all 40 writes and delete cascade. Concurrent subtask coverage joins two writers and verifies all 20 subtasks in the listing and stored descriptor.
        project_isolation:
          applicable: true
          evidence: ACDD fixtures delegate temp-root creation and cleanup to tests/common::Workspace; each root is unique by process, timestamp, and atomic sequence and is removed on Drop. Tests retain independent builder/reviewer roots and nested repository/project write-perimeter cases.
        administration:
          applicable: true
          evidence: Reviewer codex-043-acdd-review-v2 differs from build worker codex-043-acdd-builder-v3. The required `git diff --check d7ddf7b dea036b -- tests/acdd.rs tests/acdd tests/core_basics.rs` is clean. Snapshot dea036b differs from rejected snapshot 73187f4 only by removing the trailing blank line in tests/acdd/subtasks.rs.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CENTRAL-VS-LANGUAGE-ISOLATION: Central subsystem suites (acdd, core, linker, incremental, mcp, commitment_integrity) reside at tests/; all language extraction and semantic tests are strictly isolated inside tests/languages/.'
    - 'INV-ACDD-SUITE-COHESION: All ACDD state machines, task lifecycles, subtasks, blackboard messages, and milestone operations reside in tests/acdd/ rather than core storage or MCP symbol modules.'
    - 'INV-SHARED-HARNESS: Common workspace setup, file writing, database building, and MCP client lifecycle are managed through canonical shared helpers in tests/common/ rather than duplicated struct Workspace per test file.'
    - 'INV-DOMAIN-DECOMPOSITION: Test modules are organized by cohesive architectural and contractual purpose. Monolithic files are split along natural architectural sub-boundaries.'
    - 'INV-FORWARD-COMPAT-041-042: Language suites cleanly decouple pure grammar from framework manifests (frameworks.rs / manifests.rs) for milestone 041, and isolate universal AST patterns and path-scoped selectors (ast_patterns.rs, selectors.rs, navigation.rs) for milestone 042.'
    - 'INV-CONTRACT-PARITY: Refactoring preserves 100% of observable positive contract assertions, fail-closed boundaries, and Merkle tree determinism (commitment_integrity).'
    - 'INV-TABLE-DRIVEN-PERMUTATIONS: Multiple assertions over syntax permutations, keyword tables, or builtins use table-driven loops instead of copy-pasted test functions.'
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

### task: central-engine-suites-consolidation

```yaml
task_ref: central-engine-suites-consolidation
target: Consolidate remaining engine micro-binaries into coherent central suites (core, linker, incremental, mcp)
proof_policy: seam-test-first
contract_revision: 2
scope:
- tests/adapter_source_boundary.rs
- tests/compact_schema.rs
- tests/coverage_diagnostics.rs
- tests/debug_logging.rs
- tests/delta_doc_parity.rs
- tests/delta_module_scope.rs
- tests/delta_resolution_identity.rs
- tests/directory_slice.rs
- tests/edge_aggregation.rs
- tests/fastmcp_registration.rs
- tests/incremental_scope.rs
- tests/inherited_receiver_resolution.rs
- tests/linker_optimizations.rs
- tests/lint_tools.rs
- tests/mcp_context.rs
- tests/mcp_context/ast_search.rs
- tests/mcp_freshness.rs
- tests/mcp_freshness/
- tests/method_semantics.rs
- tests/overload_disambiguation.rs
- tests/receivers/persistence.rs
- tests/scanner_guard_limits.rs
- tests/source_only_adapter.rs
- tests/tool_evolution.rs
- tests/tool_evolution/
- tests/typed_receiver_resolution.rs
- tests/core.rs
- tests/core/
- tests/incremental.rs
- tests/incremental/
- tests/linker.rs
- tests/linker/
- tests/mcp.rs
- tests/mcp/
- tests/receivers/python_value_flow.rs
- tests/receivers/rust_value_flow.rs
subtasks:
- subtask_ref: consolidate-core-suite
  title: Unify pure engine storage tests (compact_schema, coverage_diagnostics, edge_aggregation, debug_logging, scanner_guard_limits, adapters) into tests/core.rs and tests/core/
  status: completed
  evidence: 'Moved compact_schema, coverage_diagnostics, edge_aggregation, debug_logging, scanner_guard_limits, and adapter suites into tests/core.rs and tests/core/. cargo test --test core: 26 passed, 0 failed, 1 pre-existing ignored; cargo clippy --test core -- -D warnings passed.'
- subtask_ref: consolidate-linker-suite
  title: Merge typed_receiver_resolution, inherited_receiver_resolution, method_semantics, overload_disambiguation, and linker_optimizations into tests/linker.rs and tests/linker/
  status: completed
  evidence: 'Merged typed receiver resolution, inherited receiver resolution, persistence delta checks, method semantics, overload disambiguation, and linker optimization tests under tests/linker.rs and tests/linker/. Kept Python/Rust value-flow source modules in tests/receivers and imported them from the typed receiver module. cargo test --test linker: 92 passed; cargo clippy --test linker -- -D warnings passed.'
- subtask_ref: consolidate-incremental-suite
  title: Merge incremental_scope, delta_module_scope, delta_resolution_identity, delta_doc_parity, and directory_slice into tests/incremental.rs and tests/incremental/
  status: completed
  evidence: 'Merged incremental_scope, delta_module_scope, delta_resolution_identity, delta_doc_parity, and directory_slice under tests/incremental.rs and tests/incremental/. Replaced their duplicate temporary Workspace lifecycle helpers with tests/common::Workspace while preserving cold-versus-delta assertions and the existing ignored paired-profile test. cargo test --test incremental: 23 passed, 1 ignored; cargo clippy --test incremental -- -D warnings passed.'
- subtask_ref: consolidate-mcp-suite
  title: Consolidate code navigation MCP tools (mcp_context, mcp_freshness, fastmcp_registration, tool_evolution, lint_tools) into tests/mcp.rs, isolating selectors.rs and navigation.rs for milestone 042
  status: completed
  evidence: 'Consolidated MCP domain suites and migrated remaining freshness, context, and tool-evolution CLI/stdio subprocess setup to tests/common::mcp_client::{in_process_server, StdioClient, run_cli}. Retained only MCP-specific payload/error assertions locally; reload still targets a replacement binary. cargo test --test core --test linker --test incremental --test mcp: 216 passed, 0 failed, 2 ignored; strict Clippy passed.'
status: completed
receipt:
  commit: b3c26af2ffbbf2db5c285dd5ec35c07cace00788
  contract_revision: 2
  passed_at: 2026-10-08T08:06:15.695158982+00:00
  evidence:
    test_proof:
      command: cargo test --test core --test linker --test incremental --test mcp
      exit_code: 0
      tests_passed: 216
      tests_failed: 0
      log: core 26 passed + 1 ignored; incremental 23 passed + 1 ignored; linker 92 passed; mcp 75 passed. Total 216 passed, 0 failed, 2 ignored. cargo clippy --test core --test linker --test incremental --test mcp -- -D warnings passed. Scoped rustfmt --check, source-architecture guard for duplicate MCP/CLI lifecycle wrappers, and git diff --check passed.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'The b3c26af snapshot contains only the central suite paths admitted by this contract: tests/core.rs and tests/core/, tests/linker.rs and tests/linker/, tests/incremental.rs and tests/incremental/, tests/mcp.rs and tests/mcp/, and the admitted receiver source fixtures. Snapshot paths were checked against the allowlist; git diff --check a0d1ffd b3c26af -- tests passed.'
        claims:
          applicable: true
          evidence: The MCP migration uses tests/common::mcp_client::{in_process_server, StdioClient, run_cli}; freshness.rs no longer defines a local stdio lifecycle, and tool_evolution.rs aliases the common client and delegates CLI execution. The Linux SIGHUP test still copies/replaces the binary, invokes reload, checks the signaled PID, waits for the replacement inode, and verifies the same PID can serve tools/list. Direct Server::new calls remain only for behavior requiring an explicit DB path or a deliberately foreign workspace root. The accepted build evidence is 216 passed, 0 failed, 2 ignored, with strict Clippy passing.
        concurrency:
          applicable: true
          evidence: Workspace uses an atomic sequence plus PID and timestamp with exclusive directory creation; each test owns its workspace. MCP subprocess output is read on a channel with a bounded response timeout. Reader/rebuild concurrency tests retain explicit entry/release channel coordination.
        project_isolation:
          applicable: true
          evidence: The shared Workspace rejects absolute and parent-traversal paths and removes its unique root on Drop. Foreign database admission tests intentionally allocate a second Workspace and assert rejection without changing the original database identity.
        administration:
          applicable: true
          evidence: Review worker codex-043-central-wrapper-review-v1 is independent of builder codex-043-central-wrapper-builder-v3. I verified every file in snapshot b3c26af matches its worktree blob and made no edits. Build proof is bound to b3c26af.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CENTRAL-VS-LANGUAGE-ISOLATION: Central subsystem suites (acdd, core, linker, incremental, mcp, commitment_integrity) reside at tests/; all language extraction and semantic tests are strictly isolated inside tests/languages/.'
    - 'INV-ACDD-SUITE-COHESION: All ACDD state machines, task lifecycles, subtasks, blackboard messages, and milestone operations reside in tests/acdd/ rather than core storage or MCP symbol modules.'
    - 'INV-SHARED-HARNESS: Common workspace setup, file writing, database building, and MCP client lifecycle are managed through canonical shared helpers in tests/common/ rather than duplicated struct Workspace per test file.'
    - 'INV-DOMAIN-DECOMPOSITION: Test modules are organized by cohesive architectural and contractual purpose. Monolithic files are split along natural architectural sub-boundaries.'
    - 'INV-FORWARD-COMPAT-041-042: Language suites cleanly decouple pure grammar from framework manifests (frameworks.rs / manifests.rs) for milestone 041, and isolate universal AST patterns and path-scoped selectors (ast_patterns.rs, selectors.rs, navigation.rs) for milestone 042.'
    - 'INV-CONTRACT-PARITY: Refactoring preserves 100% of observable positive contract assertions, fail-closed boundaries, and Merkle tree determinism (commitment_integrity).'
    - 'INV-TABLE-DRIVEN-PERMUTATIONS: Multiple assertions over syntax permutations, keyword tables, or builtins use table-driven loops instead of copy-pasted test functions.'
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

### task: language-tests-isolation-and-monolith-decomposition

```yaml
task_ref: language-tests-isolation-and-monolith-decomposition
target: Isolate all language tests into tests/languages/ and decompose monolithic files along architectural seams
proof_policy: seam-test-first
contract_revision: 2
scope:
- tests/languages.rs
- tests/languages/
- tests/ast_extractors.rs
- tests/builtin_call_coverage.rs
- tests/config_language_profiles.rs
- tests/html_profile.rs
- tests/html_profile/
- tests/language_boundaries.rs
- tests/language_features.rs
- tests/language_profiles.rs
- tests/manifests.rs
- tests/proto_ast.rs
- tests/python_child_module_links.rs
- tests/python_external_call_evidence.rs
- tests/python_semantics.rs
- tests/typescript_semantics.rs
status: completed
subtasks:
- subtask_ref: isolate-python-tests
  title: Consolidate python tests under tests/languages/python/ separating pure grammar (grammar.rs, modules.rs) from framework rules (frameworks.rs) for milestone 041
  status: completed
  evidence: 'Python suite consolidated and split across tests/languages/python/{grammar,modules,frameworks,semantics}.rs. `cargo test --test languages --quiet`: 255 passed; `cargo test --all-targets`: 643 passed, 0 failed.'
- subtask_ref: isolate-and-decompose-typescript-tests
  title: Decompose monolithic typescript_semantics into focused submodules under tests/languages/typescript/ (grammar.rs, dom.rs, modules.rs, frameworks.rs)
  status: completed
  evidence: 'TypeScript monolith decomposed into tests/languages/typescript/{grammar,dom,modules,frameworks}.rs with shared module entry. Test identity inventory remains 255/255; full `cargo test --all-targets`: 643 passed, 0 failed.'
- subtask_ref: isolate-html-tests
  title: Consolidate html_profile tests under tests/languages/html/ separating syntax.rs from templates.rs
  status: completed
  evidence: 'HTML profile consolidated under tests/languages/html/{syntax,templates}.rs and focused child modules classic_wire.rs, template_origins.rs, template_masking.rs. Full `cargo test --all-targets`: 643 passed, 0 failed.'
- subtask_ref: isolate-ast-patterns-and-manifest-schemas
  title: Establish tests/languages/ast_patterns.rs for universal AST fragments (042) and tests/languages/manifests.rs for framework TOML schema validation (041)
  status: completed
  evidence: 'Universal AST and manifest suites live in tests/languages/ast_patterns.rs and tests/languages/manifests.rs. Full `cargo test --all-targets`: 643 passed, 0 failed.'
- subtask_ref: isolate-other-language-profiles
  title: Relocate remaining language tests (language_boundaries, language_features, language_profiles, config_language_profiles, ast_extractors, proto_ast, builtin_call_coverage) into tests/languages/
  status: completed
  evidence: 'Remaining extractor, builtin, config, feature, profile, and proto suites live under tests/languages/. Full `cargo test --all-targets`: 643 passed, 0 failed.'
receipt:
  commit: 54294023a6e1b21c4159eab995314e8a5069ec62
  contract_revision: 2
  passed_at: 2026-10-08T07:45:45.314879946+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets
      exit_code: 0
      tests_passed: 643
      tests_failed: 0
      log: 'Final candidate: cargo test --all-targets: 643 passed, 0 failed, 3 ignored. cargo clippy --all-targets --all-features -- -D warnings: passed. Scoped git diff --check: clean.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: The 5429402 candidate changes only allowlisted language-test paths. `git diff --check 3f4fd88 5429402 -- <language scope>` is clean. The delta from 734e568 is one deletion in tests/languages/typescript/modules.rs.
        claims:
          applicable: true
          evidence: The baseline inventory and final `cargo test --test languages -- --list` both contain 255 tests; comparison shows no missing or extra identities. Assertion source-line inventory is 1169 baseline versus 1168 final; the sole repeated cold-parity assertion line is deduplicated into tests/languages/support.rs, and the shared helper remains called by all prior suites (4 cold_parity calls and 19 assert_cold_equivalent calls).
        concurrency:
          applicable: true
          evidence: Language Workspace delegates to tests/common::Workspace, which creates a unique temp root and removes it on Drop. The redundant common-root create at modules.rs:539 is gone; no manual remove_dir_all remains in tests/languages. Remaining create_dir_all calls make nested file/project fixtures or the required base/linked subworkspace.
        project_isolation:
          applicable: true
          evidence: The linked-project fixtures create separate first/second project roots under one uniquely allocated common Workspace and a nested linked/ config root. Assertions retain per-project provider and target checks; the only explicit linked root creation is for that nested fixture.
        administration:
          applicable: true
          evidence: Review worker codex-043-language-review-final-v3 is independent from builder codex-043-language-builder-final-v3. Accepted build snapshot is 54294023a6e1b21c4159eab995314e8a5069ec62 with cargo test --all-targets at 643 passed, 0 failed, 3 ignored; strict Clippy and scoped diff-check passed in build evidence.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CENTRAL-VS-LANGUAGE-ISOLATION: Central subsystem suites (acdd, core, linker, incremental, mcp, commitment_integrity) reside at tests/; all language extraction and semantic tests are strictly isolated inside tests/languages/.'
    - 'INV-ACDD-SUITE-COHESION: All ACDD state machines, task lifecycles, subtasks, blackboard messages, and milestone operations reside in tests/acdd/ rather than core storage or MCP symbol modules.'
    - 'INV-SHARED-HARNESS: Common workspace setup, file writing, database building, and MCP client lifecycle are managed through canonical shared helpers in tests/common/ rather than duplicated struct Workspace per test file.'
    - 'INV-DOMAIN-DECOMPOSITION: Test modules are organized by cohesive architectural and contractual purpose. Monolithic files are split along natural architectural sub-boundaries.'
    - 'INV-FORWARD-COMPAT-041-042: Language suites cleanly decouple pure grammar from framework manifests (frameworks.rs / manifests.rs) for milestone 041, and isolate universal AST patterns and path-scoped selectors (ast_patterns.rs, selectors.rs, navigation.rs) for milestone 042.'
    - 'INV-CONTRACT-PARITY: Refactoring preserves 100% of observable positive contract assertions, fail-closed boundaries, and Merkle tree determinism (commitment_integrity).'
    - 'INV-TABLE-DRIVEN-PERMUTATIONS: Multiple assertions over syntax permutations, keyword tables, or builtins use table-driven loops instead of copy-pasted test functions.'
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
