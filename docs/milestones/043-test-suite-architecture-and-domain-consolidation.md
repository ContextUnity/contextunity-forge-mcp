---
id: m-test-suite-architecture-and-domain-consolidation
title: Test suite architecture, domain consolidation, and harness standardization
doc_type: contract
status: planned
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
target: "Consolidate remaining engine micro-binaries into coherent central suites (core, linker, incremental, mcp)"
proof_policy: seam-test-first
scope:
  - tests/
status: planned
subtasks:
  - subtask_ref: consolidate-core-suite
    title: "Unify pure engine storage tests (compact_schema, coverage_diagnostics, edge_aggregation, debug_logging, scanner_guard_limits, adapters) into tests/core.rs and tests/core/"
    status: pending
  - subtask_ref: consolidate-linker-suite
    title: "Merge typed_receiver_resolution, inherited_receiver_resolution, method_semantics, overload_disambiguation, and linker_optimizations into tests/linker.rs and tests/linker/"
    status: pending
  - subtask_ref: consolidate-incremental-suite
    title: "Merge incremental_scope, delta_module_scope, delta_resolution_identity, delta_doc_parity, and directory_slice into tests/incremental.rs and tests/incremental/"
    status: pending
  - subtask_ref: consolidate-mcp-suite
    title: "Consolidate code navigation MCP tools (mcp_context, mcp_freshness, fastmcp_registration, tool_evolution, lint_tools) into tests/mcp.rs, isolating selectors.rs and navigation.rs for milestone 042"
    status: pending
```

### task: language-tests-isolation-and-monolith-decomposition

```yaml
task_ref: language-tests-isolation-and-monolith-decomposition
target: "Isolate all language tests into tests/languages/ and decompose monolithic files along architectural seams"
proof_policy: seam-test-first
scope:
  - tests/languages/
status: planned
subtasks:
  - subtask_ref: isolate-python-tests
    title: "Consolidate python tests under tests/languages/python/ separating pure grammar (grammar.rs, modules.rs) from framework rules (frameworks.rs) for milestone 041"
    status: pending
  - subtask_ref: isolate-and-decompose-typescript-tests
    title: "Decompose monolithic typescript_semantics into focused submodules under tests/languages/typescript/ (grammar.rs, dom.rs, modules.rs, frameworks.rs)"
    status: pending
  - subtask_ref: isolate-html-tests
    title: "Consolidate html_profile tests under tests/languages/html/ separating syntax.rs from templates.rs"
    status: pending
  - subtask_ref: isolate-ast-patterns-and-manifest-schemas
    title: "Establish tests/languages/ast_patterns.rs for universal AST fragments (042) and tests/languages/manifests.rs for framework TOML schema validation (041)"
    status: pending
  - subtask_ref: isolate-other-language-profiles
    title: "Relocate remaining language tests (language_boundaries, language_features, language_profiles, config_language_profiles, ast_extractors, proto_ast, builtin_call_coverage) into tests/languages/"
    status: pending
```
