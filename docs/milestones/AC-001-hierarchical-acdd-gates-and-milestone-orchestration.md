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
| **Wave 2** | **Core Engine** | `zero-arg-transition-engine-and-command-runner` | Auto-proof compiler, Seam Engine Red-Green verifier, by_pattern command runner | Wave 1 |
| **Wave 2** | **Automation** | `upfront-contract-synthesis-and-shared-helper-extraction` | Automated synthesis gate, helper extraction, and preflight milestone init | Wave 1 |
| **Wave 3** | **Runtime** | `container-gate-and-claim-engine` | In-place execution (`--in-place`), silent telemetry, state-driven claims | Wave 2 |
| **Wave 3** | **Tooling** | `scope-extension-manifest-sync-and-review-audit` | Atomic `task extend-scope`, manifest sync, SQLite checkpoints tool migration | Wave 2 |

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
| 2 | Tasks 3 and 4 | Task 3 owns transition/command-runner sources and runner.rs; Task 4 owns synthesis/milestone sources and synthesis.rs. |
| 3 | Tasks 5 and 6 | Task 5 owns claim/container and reader sources plus seam_reachability.rs; Task 6 owns scope-sync, handoff, CLI/MCP and governance documents plus scope_sync.rs. |

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
target: "Define AST data structures, serde schemas, and field access validators for universal polymorphic gates, policies catalog, reject_routing (scope -> build, symbols -> contract), symmetrical scope and universal symbols boundaries (kind:identifier), guidance slot binding schema (guidance: { slot: key }), manifest-to-gate slot validation with unbound warnings, detailed hitl guidance catalog (hitl.careless, hitl.curious, hitl.pro, hitl.paranoic), by_pattern command dispatch, two-tier scopes, modular subsystem registry with dynamic tool filtering, 96 KiB response budget compliance for modular tools, manifest linked_to milestone metadata, task-level cross-milestone dependencies (task_depends_on), speculative concurrency schema (mode: speculative, sync_on: snapshot), linked milestone task status context injection, composable persistence policies (pre/post/immutable/git_commit_note), on-demand snapshot and rollback safety-net tools, payload overflow limits (git_commit_note.limits.max_bytes: 2048), and architect DAG wave optimization"
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
status: planned
subtasks:
  - subtask_ref: universal-gate-ast-serde
    title: "Implement universal polymorphic GateDef AST data structures with Serde serialization"
  - subtask_ref: zero-language-hardcode-profile-invariant
    title: "Enforce INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE in embedded default profile"
  - subtask_ref: fail-fast-document-validator-cli
    title: "Implement fail-fast CLI document and manifest schema validator command"
  - subtask_ref: command-catalog-by-pattern-schema
    title: "Define declarative command catalog schema with by_pattern polyglot dispatch"
  - subtask_ref: policies-catalog-schema-and-enforcement-validator
    title: "Define policies catalog schema and machine-level gate enforcement validators"
  - subtask_ref: two-tier-scope-governance-schema
    title: "Define two-tier scope governance schema (scope and extended_scope) in TaskSpec"
  - subtask_ref: reject-routing-and-symbols-confinement-schema
    title: "Implement reject_routing and universal symbols (kind:identifier) schema in gate definitions"
  - subtask_ref: guidance-slot-binding-and-hitl-catalog
    title: "Implement guidance slot binding schema and detailed 4-tier HITL guidance catalog"
  - subtask_ref: gate-field-access-matrix-validator
    title: "Implement gate driver field validation matrix enforcing mutually exclusive gate kinds"
  - subtask_ref: unified-guidance-catalog-schema
    title: "Define modular guidance catalog schema and dynamic prompt compilation rules"
  - subtask_ref: required-skills-manifest-serde-and-validator
    title: "Add required_skills frontmatter serde schema and validator to MilestoneManifest"
  - subtask_ref: multi-repo-manifest-repositories-serde
    title: "Implement multi-repository coordination schema and repositories manifest field"
  - subtask_ref: speculative-concurrency-depends-on-schema
    title: "Define speculative concurrency mode and snapshot-sync dependencies schema"
  - subtask_ref: modular-subsystem-registry-and-dynamic-tool-filter
    title: "Implement modular subsystem registry with dynamic MCP tool registration and filtering"
  - subtask_ref: context-aware-guidance-adapter
    title: "Implement context-aware workflow guidance adapter dynamically adapting to enabled subsystems"
  - subtask_ref: modular-subsystem-response-adaptation
    title: "Ensure modular tool responses strictly comply with 96 KiB response budget"
  - subtask_ref: manifest-linked-milestones-and-task-dependencies-schema
    title: "Define linked_to milestones and cross-milestone task_depends_on schema"
  - subtask_ref: linked-milestones-context-and-guidance-injection
    title: "Implement context injection for linked milestone task status and blockers"
  - subtask_ref: composable-persistence-policy-schema
    title: "Implement composable persistence policy schema (pre/post/immutable/git_commit_note)"
  - subtask_ref: payload-overflow-limits-and-git-commit-note-schema
    title: "Implement 2 KiB payload overflow guard and git commit note schema"
  - subtask_ref: ondemand-snapshot-and-rollback-tools
    title: "Implement on-demand task snapshot and rollback tool schemas and safety nets"
  - subtask_ref: architect-synthesis-dag-optimizer
    title: "Implement topological wave optimization schema"
  - subtask_ref: promote-future-profile-v2-example
    title: "Promote docs/future/references/acdd_profile.v2.yaml.example to skills/contextunity-forge/references/acdd_profile.yaml.example after gate AST validation passes"
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
target: "Implement SQLite schema migrations for gate_transitions, gate_execution_traces, gate_checkpoints (breadcrumb-keyed), milestone_contract_matrix, and task_blackboard recipient addressing (recipient_task_id) for inter-agent RFC negotiation"
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
status: planned
subtasks:
  - subtask_ref: gate-transitions-table-migration
    title: "Implement SQLite migration and store APIs for durable gate_transitions audit table"
  - subtask_ref: gate-execution-trace-table-migration
    title: "Implement SQLite migration for gate_execution_traces with 2 KiB output snippets"
  - subtask_ref: gate-checkpoints-breadcrumb-keyed-table
    title: "Implement gate_checkpoints SQLite table keyed by compound turn breadcrumb"
  - subtask_ref: milestone-contract-matrix-table
    title: "Implement milestone_contract_matrix SQLite table for cross-task helper tracking"
  - subtask_ref: blackboard-recipient-and-rfc-support
    title: "Add recipient_task_id column and status indexing to task_blackboard table"
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
target: "Implement zero-argument task submit, auto-proof compiler from gate execution traces, Seam Engine Red-to-Green automated transition verifier, autonomous command runner with by_pattern multi-language dispatch, speculative dependency runner with upstream snapshot auto-rebase, automated task telemetry passport git commit note generator adhering to INV-PAYLOAD-OVERFLOW-GUARD (2 KiB note limit, compact Markdown diff/metrics, complete traces to disk logs), and unread blackboard RFC guidance injection"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - universal-gate-ast-and-modular-catalog-spec
  - sqlite-gate-transitions-and-trace-persistence
scope:
  - src/engine/tasks/submit.rs
  - src/engine/milestones/command_gate.rs
  - src/engine/tasks/scheduler.rs
  - tests/acdd/runner.rs
  - docs/reference/tasks.md
status: planned
subtasks:
  - subtask_ref: zero-arg-proof-compiler
    title: "Implement zero-argument proof payload compiler assembling traces and candidate trees"
  - subtask_ref: seam-engine-red-green-verifier
    title: "Implement Seam Engine transition verifier proving red baseline before green admission"
  - subtask_ref: autonomous-command-executor-daemon
    title: "Implement autonomous command runner daemon executing shell commands with 0 LLM tokens"
  - subtask_ref: multi-language-by-pattern-command-evaluator
    title: "Implement by_pattern polyglot scope matcher evaluating commands sequentially"
  - subtask_ref: failure-back-routing-evaluator
    title: "Implement reject_routing evaluator routing scope to build gate and symbols to contract gate"
  - subtask_ref: automated-telemetry-passport-note-writer-and-overflow-guard
    title: "Implement automated git commit note writer enforcing 2 KiB bounded Markdown payload"
  - subtask_ref: speculative-dependency-snapshot-rebase-runner
    title: "Implement speculative dependency runner auto-rebasing branches on snapshot updates"
  - subtask_ref: workflow-guidance-blackboard-rfc-injector
    title: "Implement unread blackboard RFC message injector during task claim guidance assembly"
```

#### Admitted Symbols (Wave 2)
- `fn:execute_command_gate`
- `fn:compile_evidence_envelope`
- `fn:evaluate_reject_routing`
- `fn:TasksStore::submit_zero_arg`

### task: upfront-contract-synthesis-and-shared-helper-extraction

```yaml
task_ref: upfront-contract-synthesis-and-shared-helper-extraction
target: "Implement milestone contract synthesis gate with cross-task shared helper extraction, parallel seam decoupling, blackboard matrix publication, targeted task_blackboard messaging with recipient_task_id (role/task/path ownership addressing), and preflight lifecycle initialization (renaming template scaffolding to milestone draft, while milestone init validates manifest, checks required_skills, coordinates repositories, and scaffolds worktrees)"
proof_policy: seam-test-first
contract_revision: 1
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
status: planned
subtasks:
  - subtask_ref: shared-helper-extraction-validator
    title: "Implement upfront shared helper and seam dependency extraction validator"
  - subtask_ref: parallel-disjoint-scope-analyzer
    title: "Implement static scope disjointness analyzer validating same-wave tasks"
  - subtask_ref: synthesis-blackboard-matrix-publisher
    title: "Implement contract matrix publisher posting synthesized contracts to task_blackboard"
  - subtask_ref: blackboard-recipient-router-and-dispatcher
    title: "Implement targeted blackboard router dispatching messages by recipient_task_id"
  - subtask_ref: milestone-draft-rename-and-preflight-init-scaffolding
    title: "Implement milestone init preflight validation, manifest verification, and worktree scaffolding"
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
target: "Implement hierarchical container gate state machine advancing through child gates, with in-place execution (--in-place), silent engine telemetry, state-driven zero-flag claims and breadcrumbs, and precise static seam reachability with stale-index and test-helper diagnostics"
proof_policy: seam-test-first
contract_revision: 1
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
status: planned
subtasks:
  - subtask_ref: nested-subgate-state-machine
    title: "Implement hierarchical container gate state machine advancing through nested child gates"
  - subtask_ref: in-place-lifecycle-executor
    title: "Implement in-place task execution (--in-place) bypassing worktree cloning for single-task runs"
  - subtask_ref: silent-telemetry-blackboard-filter
    title: "Filter automated engine telemetry from task_blackboard, preserving human blockers and RFCs"
  - subtask_ref: automated-scoped-verification-runner
    title: "Implement scoped verification runner executing matching commands on task build submission"
  - subtask_ref: agent-breadcrumb-formatter
    title: "Format locator breadcrumbs ({milestone_id}/{gate_path}/{task_id}/{stage_id}) injected during task claim"
  - subtask_ref: admit-candidate-seams-from-resolution-coverage
    title: "Inspect indexed coverage and canonical import/re-export bindings when direct call edges are absent; require a proven in-scope production connection before admission"
  - subtask_ref: distinguish-stale-index-drift-with-actionable-error
    title: "Report TASK_SEAM_STALE with an index-refresh action when source drift invalidates the indexed seam instead of misreporting TASK_SEAM_DISCONNECTED"
  - subtask_ref: support-multi-hop-test-fixture-harnesses
    title: "Trace up to four hops through verified test helpers or fixtures to the in-scope production target without widening production scope to include test helpers"
  - subtask_ref: provide-actionable-seam-diagnostics-in-gate-errors
    title: "Report discovered test calls, coverage statuses, reached targets, and why static seam traversal stopped"
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
target: "Implement atomic task extend-scope manifest synchronization, extended_scope_review verification gate, extending milestone handoff SQLite pruning to trace, transition, and checkpoint tables, and session-checkpoint tool migration to gate_checkpoints"
proof_policy: seam-test-first
contract_revision: 1
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
  - docs/reference/tasks.md
  - docs/reference/cli.md
  - skills/contextunity-forge/SKILL.md
  - docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
  - docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
  - docs/adr/README.md
  - docs/future/milestones/AGENTS.md
  - docs/milestones/AGENTS.md
  - docs/future/references/acdd_profile.v2.yaml.example
  - docs/future/AGENTS.md
status: planned
subtasks:
  - subtask_ref: extend-scope-markdown-manifest-sync
    title: "Implement atomic two-tier scope extension synchronizing SQLite and markdown manifest"
  - subtask_ref: extended-scope-review-proof-enforcement
    title: "Implement review gate validator enforcing extended_scope_review sign-off on scope additions"
  - subtask_ref: session-checkpoint-sqlite-migration
    title: "Migrate session_checkpoint MCP tool to query gate_checkpoints SQLite table by breadcrumb"
  - subtask_ref: milestone-handoff-trace-and-checkpoint-pruning
    title: "Extend milestone handoff pruning to purge gate_transitions, traces, and checkpoints tables"
  - subtask_ref: milestone-verification-container-and-autonomous-deliver
    title: "Implement milestone verification container gate with full repo check, review, fallback subgraph, and autonomous delivery"
  - subtask_ref: promote-future-adr-and-milestone-governance
    title: "Promote docs/future/adr/0020-*.md to docs/adr/ and docs/future/milestones/AGENTS.md to docs/milestones/AGENTS.md, registering ADR 0020 in docs/adr/README.md and pruning docs/future/"
```

#### Admitted Symbols (Wave 3)
- `fn:extend_scope_dual_sync`
- `fn:verify_extended_scope_review`
- `fn:purge_milestone_handoff_traces`
- `fn:execute_milestone_verification_container`
- `cmd:task submit`
- `cmd:task extend-scope`
- `cmd:milestone handoff`
