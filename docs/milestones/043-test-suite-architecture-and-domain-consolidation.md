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
target: "Extract and consolidate the complete ACDD and task lifecycle layer into tests/acdd.rs and tests/acdd/"
proof_policy: seam-test-first
scope:
  - tests/acdd/
  - tests/acdd.rs
status: planned
subtasks:
  - subtask_ref: extract-task-lifecycle
    title: "Relocate and modularize task lifecycle tests from core_basics/tasks.rs into tests/acdd/tasks.rs"
    status: pending
  - subtask_ref: extract-subtask-and-blackboard-tests
    title: "Modularize subtask deepening and blackboard tests into tests/acdd/subtasks.rs and tests/acdd/blackboard.rs"
    status: pending
  - subtask_ref: extract-milestone-and-mcp-task-tests
    title: "Relocate milestone CLI tests and MCP task tool tests (mcp_context/tasks.rs) into tests/acdd/milestones.rs and tests/acdd/mcp.rs"
    status: pending
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
