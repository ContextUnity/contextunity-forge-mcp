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
