---
id: AC-001-hierarchical-acdd-gates-and-milestone-orchestration
title: Hierarchical ACDD gates, modular catalogs, and milestone orchestration
doc_type: contract
status: active
depends_on:
- m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance
owners:
- src/core/tasks/
- src/engine/tasks/
- src/db/tasks_store/
- src/db/reader.rs
- tests/acdd/
- src/cli/
- skills/contextunity-forge/
- docs/
invariants:
- 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
- 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
- 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
- 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
- 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
- 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
- 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
- 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
- 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
- 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
- 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
- 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
- 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
- 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
- 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
- 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
- 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
- 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
- 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
- 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
required_skills:
- contextunity-forge
- agent-instructions
related_plans: []
started_at: 2026-10-24T02:28:16+00:00
---

# Hierarchical ACDD gates, modular catalogs, and milestone orchestration

## Outcome and purpose

Evolve ContextUnity Forge MCP into a **Universal Hierarchical Gate Platform** where **Everything is a Gate**:

1. **Universal Gate Polymorphism**: Every step in the lifecycle is a Gate. Gates are executed either by an LLM agent (`role:`), the platform shell (`command:`), or recursively as a container (`gates:` or declarative `- tasks:`).
2. **Atomic Extended Scope Synchronization**: Eliminate scope drift. The `task extend-scope` command atomically updates SQLite and the Git markdown manifest, and the `review` gate strictly validates `extended_scope_review` before delivery.
3. **Breadcrumb-Keyed SQLite Checkpoints**: Eliminate `.forge/checkpoints.json`. Store checkpoints directly in SQLite `gate_checkpoints` keyed by the turn locator (`{milestone_id}/{gate_path}/{task_id}/{stage_id}`).
4. **Auto-Proof Submissions**: Eliminate shell JSON quoting nightmares. Automated gates (any gate configured with `proof: command`, `proof: contract`, or `proof: delivery`) require zero manual JSON arguments, while review gates (`proof: review`) accept typed cognitive review judgments.
5. **In-Place Lifecycle Operation**: Allow executing tasks directly in the current workspace or branch (`--in-place`) without forcing new worktree or branch isolation. (Native execution of plans from `docs/plans/*.md` is decoupled as milestone `AC-003`, inheriting from AC-001).
6. **Silent Telemetry (Anti-Spam Blackboard)**: Automatically log transitions and command traces into SQLite; suppress automated engine progress spam while reserving `task_blackboard` for RFCs, synthesized contract matrices, and human escalations (HITL).
7. **Seam Engine Red-to-Green Automation**: Preserve rigorous static caller and topological reachability analysis on the Contract gate, while automating the runtime Red (matching configured `expected_failure.exit_codes`) to Green (0) transition check via gate execution traces.
8. **Transition & Execution Trace Tables**: Record every lifecycle advancement in `gate_transitions` and capture command execution metrics in `gate_execution_traces`.
9. **Pre-Doc Verification, Fallback Subgraph & Autonomous Delivery**: Run the full repository check and review within `milestone_verification`, with fallback to the `build_review_cycle` subgraph on failure, and autonomous delivery commit (0 tokens) before documentation starts.
10. **Modular Subsystem Decoupling & Adaptive Guidance**: Expose configurable subsystem toggles (`codemap`, `acdd`, `blackboard`, `docs`, `checkpoints`) with preset profiles (`full`, `navigator`, `orchestrator`, `minimal`), dynamic tool registration eliminating up to 3000 tokens of schema overhead, and context-aware guidance adaptation ensuring gates and prompts function cohesively in decoupled modes.

---

## Tasks in this milestone

### Priority Waves and Execution DAG

| Wave | Priority | Task Ref | Focus & Deliverable | Dependencies |
| :--- | :--- | :--- | :--- | :--- |
| **Wave 0** | **Bootstrap** | **`upfront-contract-synthesis-and-dag-prioritization`** (Task 0) | Cross-task contract matrix, waves, error taxonomy, and disjoint scope freeze | None |
| **Wave 1** | **Foundation** | `universal-gate-ast-and-modular-catalog-spec` | Polymorphic AST serde, modular subsystem filtering, and guidance schemas | Task 0 |
| **Wave 1** | **Foundation** | `sqlite-gate-transitions-and-trace-persistence` | SQLite schema migrations (`gate_transitions`, `gate_execution_traces`, `gate_checkpoints`) | Task 0 |
| **Wave 2** | **Core Engine** | `zero-arg-transition-engine-and-command-runner` | Zero-argument submission engine, auto-proof compiler, Seam Engine Red-Green verifier, and by_pattern command runner | Wave 1 |
| **Wave 3** | **Automation** | `upfront-contract-synthesis-and-shared-helper-extraction` | Automated synthesis gate, helper extraction, and preflight milestone init | Wave 1; scope-ordered after Task 3 |
| **Wave 4** | **Runtime** | `container-gate-and-claim-engine` | In-place execution (`--in-place`), silent telemetry, state-driven claims, and unread RFC guidance injection | Wave 2 and 3 |
| **Wave 4** | **Tooling** | `scope-extension-manifest-sync-and-review-audit` | CLI zero-argument submission, atomic `task extend-scope`, manifest sync, and SQLite checkpoints tool migration | Wave 2 and 3 |

---

### Canonical cross-task contract matrix

For each row, source_task is the provider and target_task is the consumer.
Every local depends_on edge has at least one seam_dependency row. Rows start at
revision 1 with status synthesized; revision increases only when the owning
contract changes, and status becomes verified after its seam is checked. The
model and validator are
[MilestoneContractMatrix](../../src/core/tasks/mod.rs#L61),
[AcddErrorCode](../../src/core/tasks/mod.rs#L71), and
[verify_contract_matrix_parity](../../src/engine/milestones/mod.rs#L32).

| Source task | Target task | Type | Contract reference |
| :--- | :--- | :--- | :--- |
| Task 0 upfront-contract-synthesis-and-dag-prioritization | Task 1 universal-gate-ast-and-modular-catalog-spec | seam_dependency | GateProfile.error_codes |
| Task 0 upfront-contract-synthesis-and-dag-prioritization | Task 2 sqlite-gate-transitions-and-trace-persistence | seam_dependency | core::tasks::MilestoneContractMatrix |
| Task 1 universal-gate-ast-and-modular-catalog-spec | Task 3 zero-arg-transition-engine-and-command-runner | seam_dependency | GateDef; CommandDef |
| Task 1 universal-gate-ast-and-modular-catalog-spec | Task 4 upfront-contract-synthesis-and-shared-helper-extraction | seam_dependency | GateProfile; PolicyDef |
| Task 2 sqlite-gate-transitions-and-trace-persistence | Task 3 zero-arg-transition-engine-and-command-runner | seam_dependency | GateTransition; GateExecutionTrace |
| Task 2 sqlite-gate-transitions-and-trace-persistence | Task 4 upfront-contract-synthesis-and-shared-helper-extraction | seam_dependency | TasksStore::replace_contract_matrix; TasksStore::list_contract_matrix |
| Task 3 zero-arg-transition-engine-and-command-runner | Task 5 container-gate-and-claim-engine | seam_dependency | execute_command_gate; compile_evidence_envelope |
| Task 4 upfront-contract-synthesis-and-shared-helper-extraction | Task 5 container-gate-and-claim-engine | seam_dependency | synthesize_milestone_contracts; validate_wave_dag |
| Task 3 zero-arg-transition-engine-and-command-runner | Task 6 scope-extension-manifest-sync-and-review-audit | seam_dependency | compile_evidence_envelope |
| Task 4 upfront-contract-synthesis-and-shared-helper-extraction | Task 6 scope-extension-manifest-sync-and-review-audit | seam_dependency | synthesize_milestone_contracts |

verify_contract_matrix_parity rejects a wrong milestone identity, unknown task
references, duplicate row keys, empty references, non-positive revisions, and
any local dependency without the directed provider-to-consumer seam_dependency
entry. Additional shared_helper and shared_type rows may describe contracts
that do not correspond to manifest dependencies.
Task 2's replace_contract_matrix writes the full matrix atomically; its
list_contract_matrix read returns entries ordered by source_task, target_task,
and contract_ref.

The manifest task order is the stable tie-breaker within each topological wave.
depends_on remains the source of execution blockers; a task may run only after
all earlier-wave dependencies pass.

| Wave | Concurrent task pair | Disjoint ownership |
| :--- | :--- | :--- |
| 0 | Task 0 | Owns the bootstrap contract, tests/acdd/mod.rs, and reserved seam files before later waves start. |
| 1 | Tasks 1 and 2 | Task 1 owns core/profile/CLI/MCP/engine sources and gates_ast.rs; Task 2 owns src/db/tasks_store/ and persistence.rs. |
| 2 | Task 3 | Owns transition/command-runner, task-store lifecycle, module registration seams, runner.rs, and task reference updates. |
| 3 | Task 4 | Owns synthesis sources and synthesis.rs; shared store/module files are serialized after Task 3 by the scope dependency. |
| 4 | Tasks 5 and 6 | Task 5 owns claim/container, reader, context guidance, and seam_reachability.rs; Task 6 owns scope-sync, handoff, CLI/MCP, governance documents, and scope_sync.rs. |

Same-wave tasks have no shared write path. Cross-wave path reuse is intentional
and serialized by the dependency graph.

---

### task: upfront-contract-synthesis-and-dag-prioritization

```yaml
task_ref: upfront-contract-synthesis-and-dag-prioritization
target: Synthesize cross-task contract matrix, establish deterministic topological execution waves, standardize shared data models & error variants, pre-register seam test harness modules in tests/acdd/mod.rs, and freeze disjoint scopes for all AC-001 tasks
proof_policy: seam-test-first
contract_revision: 1
depends_on: []
scope:
- docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/future/references/acdd_profile.v2.yaml.example
- src/core/tasks/mod.rs
- src/engine/milestones/mod.rs
- tests/acdd/mod.rs
- tests/acdd/contract_synthesis.rs
- tests/acdd/gates_ast.rs
- tests/acdd/persistence.rs
- tests/acdd/runner.rs
- tests/acdd/synthesis.rs
- tests/acdd/seam_reachability.rs
- tests/acdd/scope_sync.rs
status: completed
subtasks:
- subtask_ref: cross-task-data-contract-matrix
  title: Define canonical cross-task contract matrix and shared data structures
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: deterministic-priority-waves-dag
  title: Establish deterministic topological execution waves and explicit DAG dependencies
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: seam-test-harness-pre-registration
  title: Pre-register seam test stub files in tests/acdd/mod.rs and freeze test module boundaries
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: disjoint-scope-boundaries-freeze
  title: Freeze disjoint file write scopes and seam test allocations across all tasks
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: shared-error-taxonomy-and-diagnostic-codes
  title: Standardize shared error variants and diagnostic error codes in ADR and reference profile
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
receipt:
  contract_revision: 1
  passed_at: 2026-10-24T03:01:11.598244668+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 117
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: I inspected the claim-bound candidate snapshot 3d30822b78544a627df91842ec919474ed098e0f and the worktree status. The changed/untracked paths are exactly the 13 paths in the initial Task 0 scope; no scope extensions or unrelated configuration, credential, generated-state, or neighboring-project paths are present.
        claims:
          applicable: true
          evidence: The accepted contract matrix, dependency waves, stable error catalog, and reserved seam modules match the Task 0 contract. The production validator verify_contract_matrix_parity checks matrix/milestone identity, task references, row uniqueness, contract references/revisions, and dependency-row parity; tests/acdd/contract_synthesis.rs calls that production validator for malformed and valid cases. The cycle diagnostic is stable. The accepted build proof for this exact snapshot records strict Clippy plus cargo test --test acdd passing (117 passed, 0 failed).
        concurrency:
          applicable: false
          evidence: Task 0 adds value types and a pure parity validator; it does not add shared-state writes, transactions, locks, claims, or receipt publication. Atomic full-matrix replacement is explicitly assigned to the dependent Task 2 TasksStore API, so there is no Task 0 concurrent writer or transaction boundary to review.
        project_isolation:
          applicable: true
          evidence: The matrix is manifest-local and validated against one parsed milestone. Persistence is assigned to TasksStore::replace_contract_matrix/list_contract_matrix. Existing TasksStore::open_project binds repository and project, and milestone_identity qualifies a milestone id with that store's namespace; the matrix value therefore need not carry a portable repository/project identity under the accepted contract.
        administration:
          applicable: true
          evidence: The candidate changes a future profile example and future ADR only; it changes no active profile/configuration, migration, hook, receipt, or delivery path. The existing profile/invariant proof-shape mismatch is a deferred Task 6 item and is not introduced by this Task 0 candidate.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: Deferred to Task 6 (scope-extension-manifest-sync-and-review-audit): AC-001 invariant INV-EXTENDED-SCOPE-MANIFEST-SYNC says review proof must validate extended_scope_review approval, but the current active ReviewProof schema only accepts `decision` and the five contour results, and rejects that field. Task 0 has no scope extensions. Task 6 must align the typed review proof/schema and add production-seam coverage before milestone handoff.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 0)
- `doc:milestone_contract_matrix`
- `doc:shared_error_taxonomy`
- `fn:verify_contract_matrix_parity`

- `enum:ContractType`
- `enum:ContractStatus`
- `struct:MilestoneContractEntry`
- `struct:MilestoneContractMatrix`
- `enum:AcddErrorCode`
- `struct:AcddError`
- `method:AcddErrorCode::as_str`
- `method:AcddError::new`

### task: universal-gate-ast-and-modular-catalog-spec

```yaml
task_ref: universal-gate-ast-and-modular-catalog-spec
target: 'Define AST data structures, serde schemas, and field access validators for universal polymorphic gates, policies catalog, reject_routing (scope -> build, symbols -> contract), symmetrical scope and universal symbols boundaries (kind:identifier), guidance slot binding schema (guidance: { slot: key }), manifest-to-gate slot validation with unbound warnings, detailed hitl guidance catalog (hitl.careless, hitl.curious, hitl.pro, hitl.paranoic), by_pattern command dispatch, two-tier scopes, modular subsystem registry with dynamic tool filtering, 96 KiB response budget compliance for modular tools, manifest linked_to milestone metadata, task-level cross-milestone dependencies (task_depends_on), speculative concurrency schema (mode: speculative, sync_on: snapshot), linked milestone task status context injection, composable persistence policies (pre/post/immutable/git_commit_note), on-demand snapshot and rollback safety-net tools, payload overflow limits (git_commit_note.limits.max_bytes: 2048), and architect DAG wave optimization'
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- upfront-contract-synthesis-and-dag-prioritization
scope:
- src/core/config.rs
- src/core/response.rs
- src/core/tasks/mod.rs
- src/core/tasks/profile.rs
- src/core/tasks/gates.rs
- src/core/tasks/catalog.rs
- src/core/tasks/manifest.rs
- src/core/tasks/acdd.default.yaml
- src/cli/mod.rs
- src/cli/validate.rs
- src/mcp/server.rs
- src/mcp/response.rs
- src/mcp/tools.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/context.rs
- tests/acdd/gates_ast.rs
- docs/future/references/acdd_profile.v2.yaml.example
- skills/contextunity-forge/references/acdd_profile.yaml.example
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
status: completed
subtasks:
- subtask_ref: universal-gate-ast-serde
  title: Implement universal polymorphic GateDef AST data structures with Serde serialization
  status: ready
  evidence: null
- subtask_ref: zero-language-hardcode-profile-invariant
  title: Enforce INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE in embedded default profile
  status: ready
  evidence: null
- subtask_ref: fail-fast-document-validator-cli
  title: Implement fail-fast CLI document and manifest schema validator command
  status: ready
  evidence: null
- subtask_ref: command-catalog-by-pattern-schema
  title: Define declarative command catalog schema with by_pattern polyglot dispatch
  status: ready
  evidence: null
- subtask_ref: policies-catalog-schema-and-enforcement-validator
  title: Define policies catalog schema and machine-level gate enforcement validators
  status: ready
  evidence: null
- subtask_ref: two-tier-scope-governance-schema
  title: Define two-tier scope governance schema (scope and extended_scope) in TaskSpec
  status: ready
  evidence: null
- subtask_ref: reject-routing-and-symbols-confinement-schema
  title: Implement reject_routing and universal symbols (kind:identifier) schema in gate definitions
  status: ready
  evidence: null
- subtask_ref: guidance-slot-binding-and-hitl-catalog
  title: Implement guidance slot binding schema and detailed 4-tier HITL guidance catalog
  status: ready
  evidence: null
- subtask_ref: gate-field-access-matrix-validator
  title: Implement gate driver field validation matrix enforcing mutually exclusive gate kinds
  status: ready
  evidence: null
- subtask_ref: unified-guidance-catalog-schema
  title: Define modular guidance catalog schema and dynamic prompt compilation rules
  status: ready
  evidence: null
- subtask_ref: required-skills-manifest-serde-and-validator
  title: Add required_skills frontmatter serde schema and validator to MilestoneManifest
  status: ready
  evidence: null
- subtask_ref: multi-repo-manifest-repositories-serde
  title: Implement multi-repository coordination schema and repositories manifest field
  status: ready
  evidence: null
- subtask_ref: speculative-concurrency-depends-on-schema
  title: Define speculative concurrency mode and snapshot-sync dependencies schema
  status: ready
  evidence: null
- subtask_ref: modular-subsystem-registry-and-dynamic-tool-filter
  title: Implement modular subsystem registry with dynamic MCP tool registration and filtering
  status: ready
  evidence: null
- subtask_ref: context-aware-guidance-adapter
  title: Implement context-aware workflow guidance adapter dynamically adapting to enabled subsystems
  status: ready
  evidence: null
- subtask_ref: modular-subsystem-response-adaptation
  title: Ensure modular tool responses strictly comply with 96 KiB response budget
  status: ready
  evidence: null
- subtask_ref: manifest-linked-milestones-and-task-dependencies-schema
  title: Define linked_to milestones and cross-milestone task_depends_on schema
  status: ready
  evidence: null
- subtask_ref: linked-milestones-context-and-guidance-injection
  title: Implement context injection for linked milestone task status and blockers
  status: ready
  evidence: null
- subtask_ref: composable-persistence-policy-schema
  title: Implement composable persistence policy schema (pre/post/immutable/git_commit_note)
  status: ready
  evidence: null
- subtask_ref: payload-overflow-limits-and-git-commit-note-schema
  title: Implement 2 KiB payload overflow guard and git commit note schema
  status: ready
  evidence: null
- subtask_ref: ondemand-snapshot-and-rollback-tools
  title: Implement on-demand task snapshot and rollback tool schemas and safety nets
  status: ready
  evidence: null
- subtask_ref: architect-synthesis-dag-optimizer
  title: Implement topological wave optimization schema
  status: ready
  evidence: null
- subtask_ref: promote-future-profile-v2-example
  title: Promote docs/future/references/acdd_profile.v2.yaml.example to skills/contextunity-forge/references/acdd_profile.yaml.example after gate AST validation passes
  status: ready
  evidence: null
receipt:
  contract_revision: 1
  passed_at: 2026-10-24T06:53:11.028570505+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 120
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate 25e77d96c4703fbec0c71722d53322fb015e3348 matches the current worktree; all 21 snapshot blobs are in the admitted Task1 scope/extensions; git diff --check is clean.
        claims:
          applicable: true
          evidence: 'The candidate closes all historical implementation findings. The latest three paths are covered by production MCP seams: target adapter governs calls in both configuration directions; nonbundled task_claim and task_manage inspect prune disabled blackboard guidance; docs-only query removes codemap recovery advice while retaining docs pages. Earlier validator, modular registry, universal profile, named snapshot/rollback, and context preflight findings remain covered by source and existing tests.'
        concurrency:
          applicable: true
          evidence: Direct task_claim and task_manage(next) preflight disabled requirements before store mutation; production seam verifies task claim state and planned milestone state remain unchanged. Named rollback retains per-worktree locking and HEAD/index/worktree fencing.
        project_isolation:
          applicable: true
          evidence: call_tool resolves and loads the selected worktree adapter before disabled-tool checks; MCP seam proves primary full/selected minimal rejects and primary minimal/selected full succeeds. Task context uses owning workspace module settings.
        administration:
          applicable: true
          evidence: Profile and CLI validators, named task_manage snapshot/rollback, and receipt delivery boundaries remain intact. No receipt, migration, hook, credential, or protected milestone changes are present. Two reference-page findings are deferred to the milestone documentation scan.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: {"candidate_baseline_head":"ad339da22a33bf1feb7e5ee479ec5940e048fa8c","finding":"CLI task actions are modeled separately in src/cli/task.rs; adding snapshot/rollback there would expose the same API from CLI.","evidence":"Forge task_manage(action=extend_scope, paths=[src/cli/task.rs]) returned TASK_SCOPE_CONFLICT because that path belongs to scope-extension-manifest-sync-and-review-audit.","deferred_work":"Keep Task1''s admitted MCP task_manage snapshot/rollback surface complete. Route CLI task snapshot/rollback through the owning scope-extension-manifest-sync-and-review-audit task when it becomes eligible, or reconcile during milestone docs/integration if its contract includes that surface.","next_owner":"scope-extension-manifest-sync-and-review-audit","next_action":"Confirm CLI task command surface when that task claims its CLI scope."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 1)
- `struct:GateDef`
- `struct:GateProfile`
- `struct:PolicyDef`
- `struct:CommandDef`
- `struct:GuidanceSlotBinding`
- `struct:RejectRouting`
- `struct:SubsystemRegistry`
- `fn:validate_gate_profile`
- `fn:resolve_guidance_slot`
- `fn:filter_subsystem_tools`
- `cmd:validate`
- `slot:hitl`

- field:GateProfile.error_codes

### task: sqlite-gate-transitions-and-trace-persistence

```yaml
task_ref: sqlite-gate-transitions-and-trace-persistence
target: Implement SQLite schema migrations for gate_transitions, gate_execution_traces, gate_checkpoints (breadcrumb-keyed), milestone_contract_matrix, and task_blackboard recipient addressing (recipient_task_id) for inter-agent RFC negotiation
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- upfront-contract-synthesis-and-dag-prioritization
scope:
- src/db/tasks_store/mod.rs
- src/db/tasks_store/schema.rs
- src/db/tasks_store/lifecycle.rs
- src/db/tasks_store/matrix.rs
- src/db/tasks_store/trace.rs
- tests/acdd/persistence.rs
subtasks:
- subtask_ref: gate-transitions-table-migration
  title: Implement SQLite migration and store APIs for durable gate_transitions audit table
  status: ready
  evidence: null
- subtask_ref: gate-execution-trace-table-migration
  title: Implement SQLite migration for gate_execution_traces with 2 KiB output snippets
  status: ready
  evidence: null
- subtask_ref: gate-checkpoints-breadcrumb-keyed-table
  title: Implement gate_checkpoints SQLite table keyed by compound turn breadcrumb
  status: ready
  evidence: null
- subtask_ref: milestone-contract-matrix-table
  title: Implement milestone_contract_matrix SQLite table for cross-task helper tracking
  status: ready
  evidence: null
- subtask_ref: blackboard-recipient-and-rfc-support
  title: Add recipient_task_id column and status indexing to task_blackboard table
  status: ready
  evidence: null
status: completed
receipt:
  contract_revision: 1
  passed_at: 2026-10-24T08:44:07.263926214+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 121
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'The reopened repair changes only src/db/tasks_store/mod.rs, src/db/tasks_store/blackboard.rs, and tests/acdd/persistence.rs against baseline 5dbbcbec22848d936b0d6af2217a40055cfafcea. All nine paths in scoped candidate 6047fabd10626ad3b40802fb28da05346a3522b5 belong to Forge allowed_write_scope, and the complete allowed scope is unchanged from that candidate in the worktree. extended_scope_review: APPROVED for the Forge-authorized extension src/db/tasks_store/blackboard.rs, the canonical persistence owner for recipient posting, inbox paging, and acknowledgement. Other task paths retain accepted content; concurrent coordinator documentation edits are outside this scoped candidate. No protected configuration, credentials, generated receipt, or neighboring project file belongs to this repair.'
        claims:
          applicable: true
          evidence: Contract draft blackboard message 1146 requires recipient_task_id independently of task_id, store posting/inspection round-trip, and same-project/same-milestone validation. BlackboardMessage adds an optional backward-compatible serialized field, and all row decoders return it. Posting preserves owning hierarchy scope. Recipient pages select active messages for the validated milestone/task with bounded payload-free summaries in insertion order. Acknowledgement matches recipient, milestone, status, and message id. The additive real TasksStore seam at tests/acdd/persistence.rs:148-220 proves persistence, serialization, unread summaries, intended-recipient acknowledgement, repeated/wrong-recipient rejection, and foreign-project rejection.
        concurrency:
          applicable: true
          evidence: Posting uses one conditional INSERT SELECT that rechecks owner, optional subtask, recipient milestone, and non-completed owner/recipient states within the SQLite write, including recipient-only milestone posts. This preserves the existing completion-race fence. Acknowledgement is a single conditional UPDATE from active to read, so competing acknowledgements cannot both succeed. The repair adds no filesystem/Git mutation, lock order, receipt publication, or task revision change.
        project_isolation:
          applicable: true
          evidence: Posting, recipient paging, and acknowledgement use TasksStore::assert_id/inspect, the repository/project task namespace, and milestone_scope_ref. Posting and inbox reads reject recipient/milestone mismatch; insert SQL independently checks persisted milestone identity. Acknowledgement matches the namespace-qualified milestone and recipient. SQL values are parameter bound. Existing inspect/read/page APIs retain their project/owner scope while decoding recipient_task_id. No workspace, symlink, profile, or path authority behavior changes.
        administration:
          applicable: true
          evidence: 'The repair leaves accepted version-7 migration, recipient/status columns and index, profile pins, hooks, receipts, task authority, and gate lifecycle untouched. Existing posts retain recipient None. Legacy messages remain decodable via serde(default), and absent recipients remain omitted. Accepted builder proof: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd; exit 0, 121 passed, 0 failed. Independent review verification: cargo test --test acdd; exit 0, 121 passed, 0 failed (352.01 s).'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
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

#### Admitted Symbols (Wave 1)
- `table:gate_transitions`
- `table:gate_execution_traces`
- `table:gate_checkpoints`
- `table:milestone_contract_matrix`
- `column:task_blackboard.recipient_task_id`
- `struct:GateTransition`
- `struct:GateExecutionTrace`
- `struct:GateCheckpoint`
- `fn:record_transition`
- `fn:record_trace`
- `fn:save_checkpoint`
- `fn:TasksStore::record_gate_transition`
- `fn:TasksStore::record_execution_trace`
- `fn:TasksStore::save_checkpoint`

- `fn:TasksStore::replace_contract_matrix`
- `fn:TasksStore::list_contract_matrix`

### task: zero-arg-transition-engine-and-command-runner

```yaml
task_ref: zero-arg-transition-engine-and-command-runner
target: Implement the zero-argument task submission engine and auto-proof compiler from gate execution traces, Seam Engine Red-to-Green automated transition verifier, autonomous command runner with by_pattern multi-language dispatch, speculative dependency runner with upstream snapshot auto-rebase, and automated task telemetry passport git commit note generator adhering to INV-PAYLOAD-OVERFLOW-GUARD (2 KiB note limit, compact Markdown diff/metrics, complete traces to disk logs)
proof_policy: seam-test-first
contract_revision: 2
depends_on:
- universal-gate-ast-and-modular-catalog-spec
- sqlite-gate-transitions-and-trace-persistence
scope:
- src/engine/tasks/submit.rs
- src/engine/milestones/command_gate.rs
- src/engine/tasks/scheduler.rs
- src/db/tasks_store/lifecycle.rs
- src/db/tasks_store/mod.rs
- src/engine/tasks/mod.rs
- src/engine/milestones/mod.rs
- tests/acdd/runner.rs
- docs/reference/tasks.md
status: completed
subtasks:
- subtask_ref: zero-arg-proof-compiler
  title: Implement zero-argument proof payload compiler assembling traces and candidate trees
  status: completed
  evidence: 'Claim-bound proof compilation verifies latest complete run, candidate, claim, and registered command identity. Cross-project isolation regression rejects foreign run IDs through all four store APIs. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: seam-engine-red-green-verifier
  title: Implement Seam Engine transition verifier proving red baseline before green admission
  status: completed
  evidence: 'Red-to-green acceptance requires same verified seam, claim/run provenance, distinct gate labels, and red-before-green chronology; focused seam covers mismatched seam and reversed order. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: autonomous-command-executor-daemon
  title: Implement autonomous command runner daemon executing shell commands with 0 LLM tokens
  status: completed
  evidence: 'Command runner persists reserved attempts and full logs, terminates process groups, and bounds pipe reader completion. R5 regression rejects a .forge symlink without creating the outside task-traces directory. R7 fixed cfg imports: fs/OpenOptions are cfg(not(unix)); File is cfg(unix). Only x86_64-unknown-linux-gnu is installed, so a non-Unix target compile was unavailable; source branch/import pairing was verified structurally after scan. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: multi-language-by-pattern-command-evaluator
  title: Implement by_pattern polyglot scope matcher evaluating commands sequentially
  status: completed
  evidence: 'Sequential by_pattern dispatch evaluates matching paths using language-neutral profile types and preserves command order. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: failure-back-routing-evaluator
  title: Implement reject_routing evaluator routing scope to build gate and symbols to contract gate
  status: completed
  evidence: 'Pure reject-routing resolver proves scope -> build and symbol -> contract; persisted routing stays assigned to Task5 because gates.rs is outside Task3 scope. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: automated-telemetry-passport-note-writer-and-overflow-guard
  title: Implement automated git commit note writer enforcing 2 KiB bounded Markdown payload
  status: completed
  evidence: 'Git telemetry passport note stays <=2 KiB while complete execution logs remain persisted. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: speculative-dependency-snapshot-rebase-runner
  title: Implement speculative dependency runner auto-rebasing branches on snapshot updates
  status: completed
  evidence: 'Speculative snapshot rebase validates a clean worktree and safely restores branch/worktree on failure. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
receipt:
  contract_revision: 2
  passed_at: 2026-10-24T16:35:04.039804165+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 124
      tests_failed: 0
      log: 'R7 fix is limited to cfg imports in command_gate.rs: File is imported only under cfg(unix); std::fs::{self, OpenOptions} is imported under cfg(not(unix)), matching the non-Unix create_confined_log branch''s fs::symlink_metadata, fs::create_dir, and OpenOptions uses. rustup target list --installed reported only x86_64-unknown-linux-gnu, so a non-Unix cross-target compile was unavailable; the conditional import/branch pairing was checked structurally after scan. Focused Task3 seam passed: 1 passed, 0 failed, 123 filtered out. Strict lane exited 0: cargo clippy --all-targets --all-features -- -D warnings passed, then cargo test --test acdd reported 124 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 497.61s. Refreshed scan completed successfully. Fresh code_map_explain shows runner outbound edges to 13 production symbols (35 call occurrences), including execute_command_gate, compile_evidence_envelope, submit_zero_arg, verify_red_to_green, and evaluate_reject_routing, with no analysis gaps; create_confined_log source is indexed with no analysis gaps. R5/R6 behavior remains covered by the focused seam: outside .forge symlink creates no task-traces child, and all four foreign project run-ID APIs reject with TASK_PROJECT_MISMATCH. rustfmt --edition 2024 --check src/engine/milestones/command_gate.rs and git diff --check passed.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All nine admitted snapshot blobs match the worktree; authorized manifest adjustment unchanged; no protected source or receipt changes.
        claims:
          applicable: true
          evidence: R1-R6 remain closed; R7 cfg(not(unix)) std::fs imports are now correctly scoped.
        concurrency:
          applicable: true
          evidence: Atomic attempts, exclusive logs, process-group timeout and bounded readers remain; regression evidence verified.
        project_isolation:
          applicable: true
          evidence: Guarded traversal precedes mutation; all four run-ID paths enforce project ownership and two-project tests cover them.
        administration:
          applicable: true
          evidence: 'No administration issues. Task6 pruning of both provenance tables is recorded in handoff #1171; wrapper graph-edge limitation is non-blocking.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: Task 6 handoff: when implementing the CLI zero-argument task lifecycle / milestone handoff cleanup, prune the Task 3 provenance rows in gate_execution_runs and gate_execution_attempts together with their associated execution traces. These tables are created by the Task 3 task-store schema; this cleanup remains intentionally deferred to Task 6 ownership.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 2)
- `fn:execute_command_gate`
- `fn:compile_evidence_envelope`
- `fn:evaluate_reject_routing`
- `fn:TasksStore::submit_zero_arg`

### task: upfront-contract-synthesis-and-shared-helper-extraction

```yaml
task_ref: upfront-contract-synthesis-and-shared-helper-extraction
target: Implement milestone contract synthesis gate with cross-task shared helper extraction, parallel seam decoupling, blackboard matrix publication, targeted task_blackboard messaging with recipient_task_id (role/task/path ownership addressing), and preflight lifecycle initialization (renaming template scaffolding to milestone draft, while milestone init validates manifest, checks required_skills, coordinates repositories, and scaffolds worktrees)
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- universal-gate-ast-and-modular-catalog-spec
- sqlite-gate-transitions-and-trace-persistence
scope:
- src/engine/milestones/contract.rs
- src/engine/milestones/synthesis.rs
- src/engine/milestones/mod.rs
- src/engine/tasks/blackboard.rs
- src/cli/milestone.rs
- tests/acdd/synthesis.rs
- docs/reference/cli.md
- src/cli/task.rs
- src/db/tasks_store/blackboard.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/matrix.rs
scope_roots:
- src/engine/milestones/contract.rs
- src/engine/milestones/synthesis.rs
- src/engine/milestones/mod.rs
- src/engine/tasks/blackboard.rs
- src/cli/milestone.rs
- tests/acdd/synthesis.rs
- docs/reference/cli.md
- src/cli/task.rs
- src/db/tasks_store/blackboard.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/matrix.rs
- src/engine/linker/resolution.rs
- tests/languages/profiles.rs
status: completed
subtasks:
- subtask_ref: shared-helper-extraction-validator
  title: Implement upfront shared helper and seam dependency extraction validator
  status: ready
  evidence: null
- subtask_ref: parallel-disjoint-scope-analyzer
  title: Implement static scope disjointness analyzer validating same-wave tasks
  status: ready
  evidence: null
- subtask_ref: synthesis-blackboard-matrix-publisher
  title: Implement contract matrix publisher posting synthesized contracts to task_blackboard
  status: ready
  evidence: null
- subtask_ref: blackboard-recipient-router-and-dispatcher
  title: Implement targeted blackboard router dispatching messages by recipient_task_id
  status: ready
  evidence: null
- subtask_ref: milestone-draft-rename-and-preflight-init-scaffolding
  title: Implement milestone init preflight validation, manifest verification, and worktree scaffolding
  status: ready
  evidence: null
receipt:
  contract_revision: 5
  passed_at: 2026-10-24T12:46:06.542426037+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 123
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent review of build snapshot 45d6725f64d715b3ea7b4e6efa69ce8e39ad952f against base 4c78414680da53df8514903b3ba8d66597cc4b10 confirmed all 11 changed paths are admitted by contract revision 5; src/engine/linker/resolution.rs and tests/languages/profiles.rs are absent from the diff.
        claims:
          applicable: true
          evidence: The milestone preflight reaches TasksStore::sync_with_contract_matrix_and_initial_message. Task rows, contract matrix, and initial broadcast are written through the same ManagedTransaction; tests inject both matrix and broadcast insert failures and assert no persisted rows.
        concurrency:
          applicable: true
          evidence: Task sync, matrix replacement, and initial message insertion complete before the unified transaction commits. Failure-injection seam assertions cover zero rows across tasks, milestone_contract_matrix, and task_blackboard, and cleanup of the created worktrees.
        project_isolation:
          applicable: true
          evidence: Reviewer traced milestone and participant path validation, including symlink/traversal rejection and canonical workspace boundaries; targeted blackboard delivery validates source and recipient task IDs within the selected milestone.
        administration:
          applicable: true
          evidence: 'Preflight validates required skills, module/profile requirements, and SQLite revision bounds before worktree or database side effects. Git subprocess calls use core::git. Strict Clippy and the scoped ACDD suite passed: 123 passed, 0 failed.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
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

#### Admitted Symbols (Wave 2)
- `fn:synthesize_milestone_contracts`
- `fn:extract_shared_helpers`
- `fn:validate_wave_dag`
- `struct:MilestoneContractMatrix`
- `cmd:milestone init`
- `cmd:milestone draft`

### task: container-gate-and-claim-engine

```yaml
task_ref: container-gate-and-claim-engine
target: Implement hierarchical container gate state machine advancing through child gates, with in-place execution (--in-place), silent engine telemetry, state-driven zero-flag claims and breadcrumbs, unread blackboard RFC guidance injection into task claim guidance, and precise static seam reachability with stale-index and test-helper diagnostics
proof_policy: seam-test-first
contract_revision: 3
depends_on:
- zero-arg-transition-engine-and-command-runner
- upfront-contract-synthesis-and-shared-helper-extraction
scope:
- src/engine/tasks/context.rs
- src/engine/tasks/claim.rs
- src/engine/tasks/gates.rs
- src/db/reader.rs
- tests/acdd/seam_reachability.rs
- docs/reference/acdd.md
- src/engine/tasks/mod.rs
- src/engine/tasks/lifecycle.rs
- tests/acdd/tasks.rs
status: completed
subtasks:
- subtask_ref: nested-subgate-state-machine
  title: Implement hierarchical container gate state machine advancing through nested child gates
  status: ready
  evidence: null
- subtask_ref: in-place-lifecycle-executor
  title: Implement in-place task execution (--in-place) bypassing worktree cloning for single-task runs
  status: ready
  evidence: null
- subtask_ref: silent-telemetry-blackboard-filter
  title: Filter automated engine telemetry from task_blackboard, preserving human blockers and RFCs
  status: ready
  evidence: null
- subtask_ref: automated-scoped-verification-runner
  title: Implement scoped verification runner executing matching commands on task build submission
  status: ready
  evidence: null
- subtask_ref: agent-breadcrumb-formatter
  title: Format locator breadcrumbs ({milestone_id}/{gate_path}/{task_id}/{stage_id}) injected during task claim
  status: ready
  evidence: null
- subtask_ref: admit-candidate-seams-from-resolution-coverage
  title: Inspect indexed coverage and canonical import/re-export bindings when direct call edges are absent; require a proven in-scope production connection before admission
  status: ready
  evidence: null
- subtask_ref: distinguish-stale-index-drift-with-actionable-error
  title: Report TASK_SEAM_STALE with an index-refresh action when source drift invalidates the indexed seam instead of misreporting TASK_SEAM_DISCONNECTED
  status: ready
  evidence: null
- subtask_ref: support-multi-hop-test-fixture-harnesses
  title: Trace up to four hops through verified test helpers or fixtures to the in-scope production target without widening production scope to include test helpers
  status: ready
  evidence: null
- subtask_ref: provide-actionable-seam-diagnostics-in-gate-errors
  title: Report discovered test calls, coverage statuses, reached targets, and why static seam traversal stopped
  status: ready
  evidence: null
- subtask_ref: workflow-guidance-blackboard-rfc-injector
  title: Inject unread task-targeted and RFC blackboard messages into task claim guidance
  status: ready
  evidence: null
receipt:
  contract_revision: 3
  passed_at: 2026-10-24T20:36:49.277953074+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 125
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Independent reviewer compared candidate 397468a10cfc149aec471d10940ea77e84535885 with the task record: all nine admitted paths are represented, including tests/acdd/tasks.rs and the approved module/lifecycle extensions. Scoped change analysis found only admitted task engine and ACDD test paths; no extra staged paths or post-snapshot edits were recorded.'
        claims:
          applicable: true
          evidence: Independent reviewer traced nested leaf selection through claim_inner and cursor advancement on pass, recipient-targeted blackboard guidance, and four-hop current-source seam validation with stale-index diagnostics. ACDD tests exercise real task submission/storage paths and direct, multi-hop, and stale-source cases.
        concurrency:
          applicable: true
          evidence: Independent reviewer checked transactional submission/recovery, reload and stage/profile validation, claim/contract revision fencing, worker/worktree checks, idempotency, and rollback of provisional manifest writes. No partial-write or stale-authority defect was found.
        project_isolation:
          applicable: true
          evidence: Independent reviewer verified workspace ownership, worktree, authority, scope, and confined-path checks before claim, source reads, and submission mutations. No cross-project or path-escape defect was found.
        administration:
          applicable: true
          evidence: 'Candidate does not alter schema migrations, profile configuration, or protected milestone authority. Build evidence is bound to candidate 397468a with strict Clippy success and ACDD 125/125. The independent review noted one non-blocking unresolved indexed call edge; it is durably recorded as deferred message #1184.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: Non-blocking follow-up from independent Task 5 review: the refreshed code map still does not resolve the direct source call `lifecycle.rs::claim_inner -> claim_nested_gate` as an indexed call edge, though the source contains it and graph freshness is matched. The reviewer verified the direct source path and production-seam tests; all five review contours pass and the full ACDD target is green. Track as an index call-resolution coverage gap with its owning indexing work, not as a Task 5 blocker.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 3)
- `fn:claim_nested_gate`
- `fn:advance_gate_cursor`
- `struct:GateCursor`
- `fn:resolve_subagent_role`
- `cmd:task claim`

#### Static seam verification acceptance

1. A current, proven test call through a canonical import or re-export reaches
   the in-scope production target without a false `TASK_SEAM_DISCONNECTED`.
   Coverage expressions identify candidates for verification; an unresolved
   name match or successful compilation alone does not establish that connection.
2. Source changes that invalidate indexed call or import evidence produce
   `TASK_SEAM_STALE` and an explicit refresh action when no other current proven
   path exists. Preserve configured profile admission rules.
3. A bounded path of up to four hops through test helpers reaches its verified
   production target; unrelated or genuinely disconnected tests remain rejected.
4. Rejections show the relevant calls, statuses, reached intermediate targets,
   and stop reason. Verify these cases in the existing shared ACDD task seam and
   update task/ACDD reference documentation with the implementation.

### task: scope-extension-manifest-sync-and-review-audit

```yaml
task_ref: scope-extension-manifest-sync-and-review-audit
target: Wire zero-argument task submission into the CLI, implement atomic task extend-scope manifest synchronization and extended_scope_review enforcement, migrate session checkpoints to SQLite, prune milestone gate telemetry on handoff, and implement the pre-documentation milestone verification container with a full-repository check, independent review, bounded build_review_cycle fallback, and autonomous delivery commit
proof_policy: seam-test-first
contract_revision: 14
depends_on:
- zero-arg-transition-engine-and-command-runner
- upfront-contract-synthesis-and-shared-helper-extraction
scope:
- src/cli/task.rs
- src/cli/guide.rs
- src/mcp/tools.rs
- src/engine/milestones/contract.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/integration.rs
- tests/acdd/scope_sync.rs
- tests/acdd/tasks.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/mcp/freshness/stdio_and_response.rs
- tests/mcp/context.rs
- tests/acdd/persistence.rs
- tests/acdd/milestones.rs
- tests/acdd/mcp.rs
- src/db/tasks_store/recovery.rs
- src/engine/tasks/mod.rs
- docs/reference/profiles.md
- docs/reference/tasks.md
- docs/reference/cli.md
- skills/contextunity-forge/SKILL.md
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/README.md
- docs/future/milestones/AGENTS.md
- docs/milestones/AGENTS.md
- docs/milestones/README.md
- docs/future/references/acdd_profile.v2.yaml.example
- docs/future/AGENTS.md
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/lifecycle.rs
- src/core/tasks/gates.rs
- src/engine/tasks/gates.rs
- src/db/tasks_store/trace.rs
- src/db/tasks_store/mod.rs
- docs/reference/mcp-tools.md
- docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/reference/profiles.md
- src/core/tasks/profile.rs
- src/engine/tasks/context.rs
- .forge/acdd/profile.yaml
- tests/acdd/subtasks.rs
- tests/mcp/skill_sync.rs
- src/cli/milestone.rs
- src/engine/milestones/mod.rs
scope_roots:
- src/cli/task.rs
- src/cli/guide.rs
- src/mcp/tools.rs
- src/engine/milestones/contract.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/integration.rs
- tests/acdd/scope_sync.rs
- docs/reference/tasks.md
- docs/reference/cli.md
- skills/contextunity-forge/SKILL.md
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/README.md
- docs/future/milestones/AGENTS.md
- docs/milestones/AGENTS.md
- docs/milestones/README.md
- docs/future/references/acdd_profile.v2.yaml.example
- docs/future/AGENTS.md
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/lifecycle.rs
- src/core/tasks/gates.rs
- src/engine/tasks/gates.rs
- src/db/tasks_store/trace.rs
- src/db/tasks_store/mod.rs
- docs/reference/mcp-tools.md
- docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
- src/core/tasks/profile.rs
- src/engine/tasks/context.rs
- .forge/acdd/profile.yaml
- docs/reference/profiles.md
- tests/acdd/
- tests/mcp/
- src/engine/tasks/mod.rs
- src/db/tasks_store/recovery.rs
- src/engine/milestones/
- src/cli/milestone.rs
- src/core/tasks/acdd.default.yaml
status: completed
subtasks:
- subtask_ref: extend-scope-markdown-manifest-sync
  title: Implement atomic two-tier scope extension synchronizing SQLite and markdown manifest
  status: completed
  evidence: 'Source path verified: TasksStore::extend_scope -> extend_scope_with_pre_commit; atomically updates manifest with rollback closure around SQLite commit. ACDD tests scope_extension_updates_sqlite_and_milestone_manifest and scope_extension_rejects_invalid_manifest_dependencies_before_persisting passed within `cargo test --test acdd` (132 passed, 0 failed, exit 0); strict Clippy exit 0.'
- subtask_ref: extended-scope-review-proof-enforcement
  title: Implement review gate validator enforcing extended_scope_review sign-off on scope additions
  status: completed
  evidence: 'cargo test --test acdd review_requires_exact_extended_scope_approval_before_delivery: exit 0, 1 passed. Existing seam now rejects missing proof, wrong/extra/duplicate paths, and reason_verified=false; exact approved_paths with reason_verified=true advances to deliver. Forge code_map_explain resolves test calls into src/engine/tasks/gates.rs::TasksStore::submit and src/cli/task.rs::run. Full strict Clippy and ACDD will be rerun before parent build submission.'
- subtask_ref: session-checkpoint-sqlite-migration
  title: Migrate session_checkpoint MCP tool to query gate_checkpoints SQLite table by breadcrumb
  status: completed
  evidence: 'cargo test --test acdd session_checkpoints_use_sqlite_breadcrumb_rows: exit 0, 1 passed; cargo test --test mcp repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged: exit 0, 1 passed; rustfmt --edition 2021 --check src/mcp/tools.rs tests/acdd/scope_sync.rs: exit 0. MCP and ACDD seams both call cli::guide::checkpoint; after `contextunity-forge-mcp scan`, Forge graph shows both edges with matched freshness. Test covers SQLite breadcrumb save/get/list/delete, project namespace isolation, no legacy .forge/checkpoints.json, opaque payload and MCP response behavior. Full strict Clippy and ACDD will be rerun before parent build submission.'
- subtask_ref: milestone-handoff-trace-and-checkpoint-pruning
  title: Extend milestone handoff pruning to purge gate_transitions, traces, and checkpoints tables
  status: completed
  evidence: '`cargo test --test acdd milestone_handoff_prunes_only_its_gate_telemetry_identity`: exit 0, 1 passed. The fixture creates a checkpoint, execution trace, and transition for both m_test and m-test, calls TasksStore::handoff_milestone, and proves only the exact m_test identity is pruned. Forge code_map_explain confirms tests/acdd/milestones.rs -> TasksStore::handoff_milestone -> handoff_milestone_with_unlink -> prune_milestone_gate_records.'
- subtask_ref: milestone-verification-container-and-autonomous-deliver
  title: Implement milestone verification container gate with full repo check, review, fallback subgraph, and autonomous delivery
  status: completed
  evidence: 'Targeted rustfmt check passed. `cargo clippy --all-targets --all-features -- -D warnings`: exit 0. `cargo test --test acdd`: exit 0, 132 passed, 0 failed (697.67s).'
- subtask_ref: promote-future-adr-and-milestone-governance
  title: Promote docs/future/adr/0020-*.md to docs/adr/ and docs/future/milestones/AGENTS.md to docs/milestones/AGENTS.md, registering ADR 0020 in docs/adr/README.md and pruning docs/future/
  status: completed
  evidence: Verified already satisfied in current worktree; no edits needed. docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md exists with accepted status and Forge get_doc retrieves it; docs/adr/README.md lists ADR 0020 as item 20; docs/milestones/AGENTS.md contains promoted governance rules; docs/future/adr/ and docs/future/milestones/ are absent. docs/future/AGENTS.md states accepted architecture and milestone rules were promoted and retains only the v2 profile example routed to Task 1. Forge scan completed and document retrieval is current.
- subtask_ref: zero-argument-task-submit-cli-wiring
  title: Wire the CLI task submit command to Task 3's zero-argument submission engine
  status: completed
  evidence: cargo test --test acdd task_submit_cli_accepts_omitted_evidence_and_uses_automatic_proof — exit 0, 1 passed. CLI parses omitted --evidence and dispatches to the claim-bound automatic proof engine.
- subtask_ref: subtask-completion-delivery-guard
  title: Implement mechanical delivery blocker enforcing 100% terminal subtask completion and evidence verification
  status: completed
  evidence: '`cargo test --test acdd delivery_requires_terminal_subtasks_with_verification_evidence`: exit 0, 1 passed. Cases verify incomplete subtasks and completed subtasks with missing or whitespace evidence block delivery without advancing task state. Forge code_map_explain confirms the test calls engine::tasks::submit.'
- subtask_ref: profile-clean-room-inheritance-control
  title: Implement inherit none clean-room profile isolation without embedded catalog leakage
  status: completed
  evidence: '`cargo test --test acdd declarative_profile_compiles_defaults_and_respects_role_overrides`: exit 0, 1 passed. The existing profile seam verifies inherit:none clears embedded roles, commands, error codes, proofs, guidance, policies, contours, and templates; Forge code_map_explain confirms calls to load_for_workspace and validate_profile_document.'
- subtask_ref: profile-universal-commands-catalog-fix
  title: Support empty commands catalog in compiler and eliminate hardcoded language dispatch from context selector
  status: completed
  evidence: '`cargo test --test acdd declarative_profile_compiles_defaults_and_respects_role_overrides` and `cargo test --test acdd boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths`: each exit 0, 1 passed. The profile seam proves explicit commands:{} clears the inherited registry when command gates are absent; the boundary seam proves all configured non-Cargo commands keep the ordinary scoped path.'
receipt:
  contract_revision: 14
  passed_at: 2026-10-25T09:49:48.629563663+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 133
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed immutable candidate 8a9d7166884bef8747dadc304c7a6a4f70a54104 against captured base 1a000bc6d1a9774854fb7e93f36b659166b2f382 using the full explicit effective scope. Its only delta from reviewed 566c92e4 is ADR0020 line 353 link correction plus trailing whitespace cleanup. All 41 surviving snapshot paths and 3 admitted deletions remain authorized; protected .forge/acdd/profile.yaml was initially admitted. command_gate.rs is explicitly in Forge allowed_scope and authorized beneath manifest scope_roots src/engine/milestones/. I approve that extension as necessary for the full-log caller at integration.rs:268. Scoped bytes match worktree, including untracked promoted ADR blob 725c7769f8b619bd225279e37c5a3e3df667f95f. Unrelated dirty tests/AGENTS.md and sibling source/test paths remain excluded and untouched.
        claims:
          applicable: true
          evidence: 'All T6-F1–F12 are closed. F12 corrected ../../src link resolves to src/engine/milestones/integration.rs; matched Forge graph now verifies ADR0020:353 -> run_milestone_check:204. F1–F11 source/tests are byte-identical to 566c92e4 independently reviewed in #1230: optional CLI evidence reaches submit_zero_arg; exact extended_scope_review required; scope synchronization uses managed publication; SQLite breadcrumbs preserve opaque payloads and protect lifecycle ownership/provenance; full log persistence includes bounded trace snippets; Delivered source/executable-skill drift requires fresh verify/review; admitted persist_log has pub(super); Exhausted action matches persisted writer and retries stay terminal. Existing three production CLI milestone verification seams independently passed on identical source/tests (exit 0, 3 passed, 14.40s). Fresh accepted builder strict Clippy plus ACDD proof in #1231 is exit 0, 133 passed, 0 failed. No new behavior or test change requires another runtime rerun.'
        concurrency:
          applicable: true
          evidence: Revalidated unchanged managed transaction/revision/root/conflict fences and preimage-checked recoverable manifest publication. Existing rollback, blocked-writer and replay seam coverage remains unchanged. Milestone verification retains prepared store lock, candidate/binding/HEAD checks, clean-tree validation, bounded two-attempt fallback and claim-bound repair review. Exhausted reload/retries reject without additional command/history. Generic scratch checkpoint SQL upsert/delete predicates cannot replace transition-owned state; lifecycle loader verifies identity, tree, phase/action and proof digest. Candidate immutability preserved throughout review.
        project_isolation:
          applicable: true
          evidence: Unchanged namespace-confined breadcrumb save/get/list/delete and exact milestone transition lookup prevent cross-project access. Handoff deletes checkpoint/trace/transition rows by fully qualified equality, preserving neighbors. Scope extensions remain normalized, protected-path checked and confined to admitted roots. Full logs use hashed identifiers, canonical worktree root, no-follow directory traversal and exclusive files. Reviewer codex-ac001-task6-reviewer-20261025-formal-r5 differs from accepted builder codex-ac001-task6-builder-20261025. Every task operation uses the claimed worktree; no sibling edits.
        administration:
          applicable: true
          evidence: F12 closes the sole unresolved Markdown target from the prior full scoped link check; all other Markdown links are byte-unchanged. Forge now returns the ADR section as verified documentation for run_milestone_check, with matched freshness on 387 files and 0 parse errors. Scoped diff --check passes. Promoted ADR/governance, updated CLI/MCP/reference behavior, clean-room/empty-command profiles, terminal subtask evidence guard, telemetry pruning and durable archive/tombstone authority remain as reviewed. No receipt/schema/Git mutations and no delivery performed. All twelve prior findings closed; no remaining scoped findings.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
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

#### Admitted Symbols (Wave 3)
- `fn:extend_scope_dual_sync`
- `fn:verify_extended_scope_review`
- `fn:purge_milestone_handoff_traces`
- `fn:execute_milestone_verification_container`
- `cmd:task submit`
- `cmd:task extend-scope`
- `cmd:milestone handoff`
