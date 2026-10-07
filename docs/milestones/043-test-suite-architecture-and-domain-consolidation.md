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
target: "Establish canonical shared test harness in tests/common/ for workspace lifecycle, database construction, and MCP testing"
proof_policy: seam-test-first
scope:
  - tests/common/
status: planned
subtasks:
  - subtask_ref: canonical-test-workspace
    title: "Implement tests/common/workspace.rs with RAII temp dir, file writing, and build/delta execution"
    status: pending
  - subtask_ref: canonical-assertions-and-mcp-client
    title: "Implement tests/common/assertions.rs and tests/common/mcp_client.rs for unified assertions and fast in-process/subprocess client spawning"
    status: pending
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
