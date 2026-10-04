---
id: m-unified-agent-context-and-task-lifecycle
title: Unified agent context bundling, task lifecycle reopening, and monorepo isolation
doc_type: contract
status: active
depends_on:
- m-task-blackboard-and-context-retention:completed
owners:
- src/core/tasks/
- src/db/tasks_store.rs
- src/engine/tasks.rs
- src/engine/milestones.rs
- src/engine/scanner.rs
- src/mcp/
- src/cli/
- docs/
- tests/
invariants:
  - "INV-UNIFIED-AGENT-CONTEXT: Claiming a task or querying task context optionally bundles the task contract, active blackboard state, related ADRs/documentation mapped to scope, code symbols in scope, and existing test harnesses in a single roundtrip, eliminating disjoint orientation loops."
  - "INV-COMPLETED-TASK-REOPEN: Reopening or resetting a completed task clears the milestone receipt before resetting SQLite to ready at contract/v1, restores Markdown on a returned SQLite error, and preserves historical subtasks."
  - "INV-MONOREPO-SUBPROJECT-INFERENCE: Subprojects across any repository layout infer project identities structurally from their owning directory hierarchy (without arbitrary folder name whitelists or Forge dictating repository structure), avoiding namespace collisions."
  - "INV-SCANNER-CONTRACT-ISOLATION: Milestone and plan directories configured with wildcard patterns (e.g. extensions/*/docs/milestones) are strictly isolated from source code, AST, and doc search indices to prevent contract drafts from polluting graph queries."
  - "INV-GATE-AWARE-GUIDANCE: Task guidance provides stage-specific tool recommendations tailored to each ACDD gate (code_map_overview on contract/v1, ast_grep_search on build/v1, code_map_impact on review/v1)."
started_at: 2026-10-04T05:45:00+00:00
---

# Unified agent context bundling, task lifecycle reopening, and monorepo isolation

## Outcome and purpose

ContextUnity Forge MCP was conceived as an all-in-one semantic intelligence system where an autonomous agent can seamlessly transition from high-level strategic plans and architectural documentation down to task contracts, code graphs, and live verification. In production multi-agent sessions, concrete operational limitations emerged:

1. **Terminal Task Lock & Friction**: Completed tasks were permanently locked in `TASK_TERMINAL` status. Reopening for audits or regressions forced manual SQLite manipulation.
2. **Project Inference Limitations**: Determining project namespaces in monorepos relied on hardcoded folder whitelists, which violated Forge's principle of never dictating workspace layout.
3. **Draft Specification Pollution**: Milestone and plan specification documents leaked into the code and doc search indices, polluting search results with unadmitted draft contracts.
4. **Context Fragmentation**: Agents had to execute 5–6 disjoint roundtrips (`task_claim` -> `task_blackboard` -> `get_doc` -> `code_map_inspect` -> `code_map_tests`) to orient on a task, risking context starvation and isolated unit tests.

This milestone resolves these issues across five dedicated, domain-scoped tasks:
- Reopening and resetting completed tasks across SQLite and milestone Markdown.
- Universal structural subproject inference without folder name whitelists.
- Wildcard scanner isolation for milestone/plan directories and Serde configuration backward compatibility.
- Focused task proof and a final test suite review at the milestone boundary.
- Zero-shot unified task context bundling and gate-aware workflow guidance.

---

## Tasks in this milestone

### task: completed-task-reopen-and-contract-readmission

```yaml
task_ref: completed-task-reopen-and-contract-readmission
target: Enable resetting and reopening completed tasks back to contract/v1 ready status across SQLite and milestone Markdown, and permit contract re-admission upon contract_revision increment
agent_type: worker
proof_policy: direct-proof
contract_revision: 2
scope:
- src/db/tasks_store.rs
- src/engine/tasks.rs
- src/engine/milestones.rs
- src/cli/task.rs
- src/mcp/tools.rs
- docs/reference/tasks.md
- docs/reference/cli.md
- tests/core_basics/tasks.rs
status: ready
subtasks:
- subtask_ref: completed-task-reset-reopen
  title: Permit resetting and reopening completed tasks back to contract/v1 ready status across SQLite and milestone Markdown
  status: completed
  evidence: 'cargo test --test core_basics tasks::completed_task_reopen_and_mcp_manage_action_lifecycle passes'
- subtask_ref: clear-task-receipt-markdown-sync
  title: Atomically strip status completed and receipt YAML blocks from milestone Markdown on task reset or reopen
  status: completed
  evidence: 'cargo test --test core_basics tasks::completed_task_reopen_and_mcp_manage_action_lifecycle proves receipt clearing and missing-document fail-closed behavior'
- subtask_ref: contract-readmission-on-revision-bump
  title: Allow contract re-admission during task sync when contract_revision is incremented on completed tasks
  status: completed
  evidence: 'cargo test --test core_basics tasks::contract_readmission_and_reset_fence_old_evidence passes'
- subtask_ref: mcp-manage-action-symmetry
  title: Expose reset and reopen actions in MCP task_manage tool router with schema validation
  status: completed
  evidence: 'task_manage supports actions reset and reopen with full test coverage'
```

1. **Store Mechanics**: Reopen and reset remove the terminal barrier (`TASK_TERMINAL`) for completed tasks. Resets `gate = 0`, sets `status = "ready"`, clears `completed_at` and `receipt` from SQLite, increments `claim_revision`, and inserts a pending gate 0 record in `task_gates`.
2. **Subtask Retention**: Existing subtasks in `task_subtasks` are preserved across resets to retain implementation history.
3. **Dual Surface**: Both CLI (`contextunity-forge-mcp task reset` / `reopen`) and MCP (`task_manage` with `action: "reset"` and `"reopen"`) are supported.
4. **Markdown Synchronization**: `clear_task_receipt` removes `status: completed` and `receipt:` from the milestone document, then SQLite resets the task. A missing or unwritable document leaves SQLite unchanged; a returned SQLite error restores the old document. Reset and delivery hold one advisory receipt lock to prevent concurrent document overwrites. An interrupted process between the two writes requires manual reconciliation.

---

### task: monorepo-structural-subproject-inference

```yaml
task_ref: monorepo-structural-subproject-inference
target: Infer project namespace identities structurally from directory hierarchy without folder name whitelists, supporting arbitrary workspace and subproject layouts
agent_type: worker
proof_policy: direct-proof
contract_revision: 2
scope:
- src/core/tasks/mod.rs
- src/engine/tasks/workspaces.rs
- src/engine/milestones.rs
- src/cli/milestone.rs
- tests/core_basics/tasks.rs
status: ready
subtasks:
- subtask_ref: structural-project-inference-engine
  title: Infer subproject name from structural hierarchy containing docs/milestones, docs/plans, milestones, or plans without arbitrary folder whitelists
  status: completed
  evidence: 'cargo test --test core_basics tasks::infer_project_from_path_supports_arbitrary_monorepo_structures_universally passes'
- subtask_ref: workspace-registry-subproject-mapping
  title: Map discovered subproject milestone directories to isolated TaskWorkspace entries in registry
  status: completed
  evidence: 'Registry::load creates distinct project workspaces for subprojects'
- subtask_ref: milestone-init-dir-and-contract-stage-claim
  title: Support explicit milestone directory flag in init and preserve contract stage claim guidance
  status: completed
  evidence: 'cargo test --test core_basics tasks::milestone_init_scaffolds_numbered_planned_and_active_documents passes'
```

1. **Universal Structural Inference**: Eliminates folder name whitelists (`extensions`, `packages`, `services`, etc.). Any directory enclosing `docs/milestones`, `docs/plans`, `milestones`, or `plans` is dynamically identified as the owning subproject.
2. **Namespace Binding**: Binds tasks to `<repository>/<project>/<manifest>:<task_ref>`, preventing namespace collisions across workspaces.
   Project names keep ordinary underscores and hyphens; dots and tildes within a path component use reversible `~d` and `~~` escapes, and other unsupported characters use decimal Unicode escapes before components are joined with dots.
3. **Explicit Override**: Frontmatter `project: <name>` continues to serve as the primary override when explicitly authored.

---

### task: scanner-milestone-isolation-and-config-compat

```yaml
task_ref: scanner-milestone-isolation-and-config-compat
target: Isolate milestone and plan directories from code and doc search indices using path pattern wildcards and preserve backward compatibility for doc_roots
agent_type: worker
proof_policy: direct-proof
contract_revision: 2
scope:
- src/engine/scanner.rs
- src/db/writer.rs
- forge-mcp.yaml
- docs/reference/configuration.md
- tests/core_basics/tasks.rs
status: ready
subtasks:
- subtask_ref: wildcard-path-pattern-matcher
  title: Implement path pattern matching supporting single wildcards and prefix/suffix component globs
  status: completed
  evidence: 'src/engine/scanner.rs unit test matches_path_pattern_supports_wildcards_and_partial_globs passes'
- subtask_ref: scanner-milestone-and-plan-exclusion
  title: Exclude configured milestone and plan directory trees from candidate file walk during index builds
  status: completed
  evidence: 'cargo test --test core_basics tasks::milestone_and_plan_directories_configured_and_excluded_from_scanner passes'
- subtask_ref: db-writer-policy-cache-invalidation
  title: Include configured milestones and plans lists in SQLite policy hash to trigger rebuild on config change
  status: completed
  evidence: 'writer.rs policy table records milestones and plans arrays'
- subtask_ref: doc-roots-backward-compatibility-alias
  title: Preserve legacy doc_roots configuration via Serde alias on docs fields in AdapterFile and LinkedWorkspaceConfig
  status: completed
  evidence: 'src/engine/scanner.rs unit test doc_roots_backward_compatibility_deserializes_to_docs passes'
```

1. **Isolation Law**: Draft specification contracts and milestone execution state must not pollute code graph symbols or documentation search.
2. **Wildcard Support**: `matches_path_pattern` evaluates wildcard components (`*`, prefix `*-api`, suffix), enabling patterns like `extensions/*/docs/milestones`.
3. **Serde Alias**: `#[serde(default, alias = "doc_roots")]` prevents silent configuration dropouts in existing repositories.

---

### task: test-evidence-simplification-and-anti-proliferation

```yaml
task_ref: test-evidence-simplification-and-anti-proliferation
target: Simplify test proof policies, support deferred-final-test natively with milestone test-suite-refactor review, and codify Feature Task versus Scope Task testing invariants
agent_type: worker
proof_policy: direct-proof
contract_revision: 2
scope:
- src/core/tasks/gates.rs
- docs/reference/acdd.md
- docs/reference/tasks.md
- docs/runbooks/acdd.md
- tests/core_basics/tasks.rs
status: ready
subtasks:
- subtask_ref: deferred-final-test-support
  title: Support deferred-final-test policy in gates validator accepting exit code 0 alongside direct-proof
  status: completed
  evidence: 'cargo test --test core_basics direct_proof_and_deferred_final_test passes'
- subtask_ref: feature-vs-scope-task-test-boundaries
  title: "Codify testing boundaries: Feature Tasks define 1 root seam test (subtasks do not write tests); Scope Tasks operate across broad domains via table-driven harnesses"
  status: completed
  evidence: 'docs/reference/acdd.md and docs/runbooks/acdd.md codify Feature Task and Scope Task test boundaries'
- subtask_ref: deferred-final-test-suite-refactor-harmonization
  title: "Formalize deferred-final-test as the milestone boundary review gate via test-suite-refactor, authorized to refactor legacy tests outside task scopes"
  status: completed
  evidence: 'docs/reference/acdd.md and docs/runbooks/acdd.md define deferred-final-test suite refactoring gate'
```

1. **Feature Task versus Scope Task Invariants**:
   - **Feature Task**: Delivers a single coherent architectural feature. Requires exactly ONE root seam test (`contract/v1`). Subtasks represent sequential implementation milestones and are strictly prohibited from authoring independent micro-unit test binaries or functions.
   - **Scope Task**: Operates across an entire architectural domain or subsystem (e.g. Language Semantics in Milestone 020). Replaces ad-hoc test function sprawl with a unified, parameterized table-driven harness (`cases: [...]`), where each subtask contributes a case row to prove red-to-green resolution.
2. **`deferred-final-test` Milestone Harmonization Gate**:
   - Explicitly establishes `deferred-final-test` as the milestone-level test review gate executed prior to handoff.
   - Runs the `test-suite-refactor` skill across all tests added or modified in the milestone.
   - **Boundary Coverage Verification**: Specifically verifies and strengthens coverage at the seams where new contracts interface with pre-existing contracts.
   - **Cross-Scope Authority**: The milestone refactoring phase at `deferred-final-test` is explicitly authorized to refactor, consolidate, or update pre-existing tests that lie outside the narrow `allowed_scope` of individual tasks, eliminating duplicates and preventing test drift across the codebase.

---

### task: unified-task-context-bundler-and-guidance

```yaml
task_ref: unified-task-context-bundler-and-guidance
target: Implement unified zero-shot task context bundling, scope-to-ADR mapping, code skeleton discovery, covering test seams, and gate-aware guidance
agent_type: worker
proof_policy: direct-proof
contract_revision: 2
scope:
- src/core/tasks/
- src/db/reader.rs
- src/db/symbols.rs
- src/engine/tasks.rs
- src/mcp/tools.rs
- docs/reference/tasks.md
- tests/core_basics/tasks.rs
- tests/mcp_context/tasks.rs
status: ready
subtasks:
- subtask_ref: task-context-bundle-model-and-api
  title: Add bundle parameter to task_claim and task_context query aggregating contract, ADRs, scope skeleton, tests, and blackboard
  status: completed
  evidence: 'task_claim with bundle: true and task_manage context return context_bundle'
- subtask_ref: scope-to-adr-knowledge-mapping
  title: Map task allowed_scope paths to governing ADRs and architecture guides in docs/adr/ and docs/architecture/
  status: completed
  evidence: 'context.rs scan_adrs resolves direct and referenced ADRs'
- subtask_ref: scope-symbols-and-test-seam-resolution
  title: Resolve top-level symbols, public signatures, and covering test suites directly within allowed_scope
  status: completed
  evidence: 'context.rs query_scope_symbols and find_covering_tests resolve scope artifacts'
- subtask_ref: gate-aware-workflow-guidance
  title: Provide dynamic, actionable guidance presets tailored to active ACDD gates (contract, build, review, deliver)
  status: completed
  evidence: 'context.rs builds gate-aware recommendations and Universal Subtask DoD'
- subtask_ref: unified-bundle-integration-and-verification
  title: Verify zero-shot agent orientation through public MCP tool router and CLI seams with complete test coverage
  status: completed
  evidence: 'cargo test --test mcp_context task_claim_bundle_and_task_manage_context_returns_unified_agent_context passes'
```

1. **Zero-Shot Task Orientation**: `task_claim(..., bundle: true)` returns contract, governing ADRs, scope skeleton, covering tests, and active blackboard messages in a single structured payload.
2. **Gate-Aware Presets**: Tailors recommendations per ACDD gate:
   - `contract/v1`: Suggests `code_map_overview` and architectural ADR inspection, followed by red seam test authoring.
   - `build/v1`: Recommends targeted AST search patterns (`ast_grep_search`) and symbol inspections, decomposing implementation into subtasks satisfying Universal Subtask DoD.
   - `review/v1`: Suggests `code_map_impact` and removal safety proof (`code_map_prove_removal`).
   - `deliver/v1`: Reminds the agent of rollup requirements and blackboard pruning.

---

## Verification

The milestone is verified when:
1. `cargo test --all-targets` passes with 0 failures across all test suites.
2. `cargo test --test commitment_integrity` confirms deterministic Merkle tree hashing.
3. `cargo clippy --all-targets --all-features -- -D warnings` produces 0 warnings.
4. An end-to-end test validates that `task_claim` with `bundle: true` returns the task contract, mapped ADRs, scope symbols, covering tests, and blackboard messages in one payload.
5. All five tasks reach `completed` status with verified receipts.
