---
title: "Blueprint Synthesis, Profile Inheritance, and Artifact Authoring Workflows"
doc_type: plan
purpose: >-
  Establish standalone authoring workflows for milestones, plans, and workflows;
  fix declarative profile inheritance to deep-merge gate overrides by ID so
  repository profiles act as thin command overlays over acdd.default.yaml;
  replace flat contract_synthesis with a reviewed blueprint_synthesis container
  under a universal topological root-gate state machine; persist per-task proof
  slices in gate_transitions (subject = task_id) for isolated context injection
  on task claim; and fix nested seam container delivery resolution.
depends_on: []
---

# Blueprint Synthesis, Profile Inheritance, and Artifact Authoring Workflows

This plan specifies the architectural separation between **artifact authoring workflows** (creating a milestone, plan, or workflow before execution) and **artifact execution profiles**, aligns declarative profile inheritance in [`parse_profile_document`](../../crates/forge-tasks/src/tasks/profile/normalize.rs) and [`merge_profile_document`](../../crates/forge-tasks/src/tasks/profile/normalize.rs), upgrades the pre-task architectural gate to a reviewed `blueprint_synthesis` container, and fixes nested `seam` subgraph delivery in [`submit_with_pre_persist`](../../crates/forge-tasks/src/engine/gates/submit.rs).

> [!IMPORTANT]
> Invariant: The task and milestone engine never hardcodes gate identifiers (`"blueprint_synthesis"`, `"verify"`, `"handoff"`) or bespoke blueprint table names; root-gate ordering is governed strictly by topological position relative to the `- tasks:` container (`cursor.root_index < task_position` versus `cursor.root_index > task_position`), and per-task upstream context is projected from [`GateTransition`](../../crates/forge-tasks/src/store/trace.rs) rows keyed by `subject = task_id`.

- **Architectural Principles & Negative Boundaries**:
  - **Zero Gate-ID or Table-Name Hardcoding**: Do not branch on string gate IDs such as `"blueprint_synthesis"` and do not introduce dedicated SQLite tables such as `milestone_task_blueprints`. Reuse `gate_transitions` (`subject = "milestone"` for milestone transitions and `subject = <full_task_id>` for per-task proof slices).
  - **Strict Separation of Authoring vs Execution**: Authoring a milestone (`docs/milestones/`), plan (`docs/plans/`), or workflow (`docs/workflows/`) runs as a standalone workflow (`inherit: none`, `save: none`, no `tasks:` container). Execution of a milestone runs under the hierarchical execution profile (`blueprint_synthesis` $\to$ `tasks` $\to$ `verify` $\to$ `milestone_review` $\to$ `handoff`, with failure routing `polish` $\to$ `verify`).
  - **No Test Authoring in Architectural Blueprint Synthesis**: Role `architect` in `blueprint_synthesis.synthesize` synthesizes grounded architectural blueprints, Clean Architecture layer boundaries, negative boundaries, and cross-task seam specifications without writing compiling red tests. Compiling runtime-red seam tests belong strictly to per-task `tasks.contract` (`role: contract_author`).
  - **Overlay Profile Inheritance**: Repository profiles (`.forge/acdd/profile.yaml`) inherit the full gate graph, roles, policies, and guidance from [`crates/forge-tasks/src/tasks/acdd.default.yaml`](../../crates/forge-tasks/src/tasks/acdd.default.yaml) via `inherit: default` and override only `commands`, `scope_args_by_pattern`, and gate-level `command` bindings by gate `id`.
  - **End-to-End Trace**:
    `milestone run / task claim -> collect_root_cursors (root_index < task_position) -> blueprint_synthesis.synthesize -> blueprint_synthesis.review -> fan-out proof.tasks[task_ref] into gate_transitions(subject = task_id) -> task claim context_bundle injects subject == task_id proofs -> contract -> build -> review -> [when: seam -> seam.integrate -> seam.review] -> deliver -> post-task root gates (root_index > task_position)`

## 1. Declarative Profile Inheritance and Proof Catalog Resolution

- **Target**: Support `inherit: default` and `inherit: none` in profile parsing, deep-merge gate overrides by `id` (including `- tasks:` and nested containers) before gate hierarchy normalization, and resolve named proof references (`proof: <name>`) from the top-level `proofs:` catalog (`ProofCatalogDef`) alongside inline `proof: { scheme: ... }`.
- **Scope**: [crates/forge-tasks/src/tasks/profile/mod.rs, crates/forge-tasks/src/tasks/profile/normalize.rs, crates/forge-tasks/src/tasks/profile/validate.rs, crates/forge-tasks/src/tasks/gates.rs]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "1.1", title: "Extend inherit parsing in normalize.rs to accept default/true and none/false", status: "completed", evidence: "Inherit parsing handles default/none and bools in parse_profile_document; covered in tasks::profile::tests"}, {subtask_ref: "1.2", title: "Implement recursive gate-by-id and tasks-container merge before normalize_gate_hierarchy", status: "completed", evidence: "Recursive gate merge implemented in normalize.rs with track condition matching (S1-R1) and child condition clean recomputation (S1-R2); 60 tests passed"}, {subtask_ref: "1.3", title: "Support named proof catalog references (proof: blueprint) in ProofDef resolved against profile.proofs alongside inline proof.scheme", status: "completed", evidence: "ProofDef::Named variant added with schema resolution against profile.proofs and validation in validate.rs; verified with unit tests and independent review"}]

- **Implementation Details**:
  - In [`parse_profile_document`](../../crates/forge-tasks/src/tasks/profile/normalize.rs):
    - Accept `inherit: default` (or `true` / omitted) as `inherit_defaults = true` and `inherit: none` (or `false`) as `inherit_defaults = false`.
    - Defer `normalize_gate` / `normalize_gate_hierarchy` until after [`merge_profile_document`](../../crates/forge-tasks/src/tasks/profile/normalize.rs) completes when `inherit_defaults == true`, preventing partial overlay gates (such as `- id: build` with only `command: rust-check-and-test`) from clobbering inherited `role`, `guidance`, `save`, `policy`, `contours`, `independent_from`, `fail`, and `loop` fields with default empty values.
  - In [`GateProfile::merge_override`](../../crates/forge-tasks/src/tasks/profile/normalize.rs):
    - Replace the whole-sequence overwrite (`self.gates = other.gates`) with a recursive merge by gate `id` and `- tasks:` container:
      - When an overlay gate matches an existing base gate `id` (or both entries represent the `- tasks:` container), merge only the explicitly supplied fields and recursively merge child `gates:` / `tasks.gates:` by `id`.
      - When an overlay gate has a new `id` not present in the base sequence, append it in declared order.
  - **Top-Level Reusable `proofs:` Catalog (`ProofCatalogDef`) and Inline `proof.scheme`**:
    - Extend [`ProofDef`](../../crates/forge-tasks/src/tasks/profile/mod.rs) with a named catalog reference variant (e.g. `Named(String)`) so a gate can declare `proof: blueprint` referencing `proofs.blueprint` in `GateProfile.proofs` (`BTreeMap<String, ProofCatalogDef>`), allowing multiple gates and inherited profiles (`inherit: default`) to share the same schema definition.
    - During profile normalization (`normalize_gate_hierarchy`) and validation (`validate_gate_proof` in [`crates/forge-tasks/src/tasks/gates.rs`](../../crates/forge-tasks/src/tasks/gates.rs)), resolve `ProofDef::Named(name)` against `profile.proofs.get(name).and_then(|entry| entry.schema.as_ref())` (and fail closed with `ACDD_PROFILE_SCHEMA_INVALID` if `name` is neither a built-in `ProofKind` nor a defined entry with a valid `schema` in `profile.proofs`).
    - Keep inline `proof: { scheme: { type: object, ... } }` (`ProofDef::Scheme`) for single-gate inline schemas, and [`infer_gate_proof`](../../crates/forge-tasks/src/tasks/profile/normalize.rs) for automatic inference when `proof:` is omitted.

## 2. Canonical Default Profile and Slimmed Repository Profile Overlay

- **Target**: Upgrade `acdd.default.yaml` to the complete language-neutral lifecycle graph (`proofs.blueprint` schema, `blueprint_synthesis` container referencing `proof: blueprint`, `tasks` with conditional `seam` container, and post-task `verify`/`polish`/`milestone_review`/`handoff` gates) and shrink `.forge/acdd/profile.yaml` to a compact command and scope-pattern overlay.
- **Scope**: [crates/forge-tasks/src/tasks/acdd.default.yaml, .forge/acdd/profile.yaml]
- **Depends On**: [Declarative Profile Inheritance and Proof Catalog Resolution]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "2.1", title: "Define proofs.blueprint schema and replace contract_synthesis with blueprint_synthesis container (synthesize + review) in acdd.default.yaml", status: "completed", evidence: "Defined proofs.blueprint schema and replaced contract_synthesis with blueprint_synthesis container in acdd.default.yaml; verified in 62 passed tests"}, {subtask_ref: "2.2", title: "Add language-neutral conditional seam container and full post-task root gates to acdd.default.yaml", status: "completed", evidence: "Added conditional seam container under tasks and retained post-task root gates; verified with 62 library tests and strict Clippy"}, {subtask_ref: "2.3", title: "Reduce .forge/acdd/profile.yaml to inherit: default with Rust commands, scope_args_by_pattern, and gate command bindings", status: "completed", evidence: "Slimmed .forge/acdd/profile.yaml to inherit: default overlay with Rust commands and gate bindings; validated with independent review"}]

- **Implementation Details**:
  - **Updates to [`crates/forge-tasks/src/tasks/acdd.default.yaml`](../../crates/forge-tasks/src/tasks/acdd.default.yaml)**:
    - Update `roles.architect` and `guidance.milestone.synthesis` to remove test authoring requirements and enforce grounded context, surgical Clean Architecture layer isolation, negative boundaries, and blackboard decision recording.
    - Add `guidance.milestone.blueprint_review` for the independent reviewer auditing the blueprint.
    - Declare reusable `proofs.blueprint` in the top-level `proofs:` catalog and reference `proof: blueprint` on `blueprint_synthesis.synthesize`:
      ```yaml
      proofs:
        blueprint:
          description: "Architectural blueprint with grounded citations and per-task slices"
          schema:
            type: object
            required:
              - cited_invariants
              - cited_symbols
              - global_negative_boundaries
              - blackboard_decision_id
              - tasks
            properties:
              cited_invariants:
                type: array
                items:
                  type: string
              cited_symbols:
                type: array
                items:
                  type: string
              global_negative_boundaries:
                type: array
                items:
                  type: string
              blackboard_decision_id:
                type: string
              tasks:
                type: object
                additionalProperties:
                  type: object
                  required:
                    - layer
                    - summary
                    - exported_interfaces
                    - negative_boundaries
                    - incoming_seams
                    - outgoing_seams
                  properties:
                    layer:
                      type: string
                    summary:
                      type: string
                    exported_interfaces:
                      type: array
                      items:
                        type: string
                    negative_boundaries:
                      type: array
                      items:
                        type: string
                    verification_seam:
                      type: string
                    incoming_seams:
                      type: array
                      items:
                        type: object
                        required: [source_task, contract_type, contract_ref, specification]
                        properties:
                          source_task: { type: string }
                          contract_type:
                            type: string
                            enum: ["seam_dependency", "shared_type", "shared_helper"]
                          contract_ref: { type: string }
                          specification: { type: string }
                    outgoing_seams:
                      type: array
                      items:
                        type: object
                        required: [target_task, contract_type, contract_ref, specification]
                        properties:
                          target_task: { type: string }
                          contract_type:
                            type: string
                            enum: ["seam_dependency", "shared_type", "shared_helper"]
                          contract_ref: { type: string }
                          specification: { type: string }

      gates:
        - id: blueprint_synthesis
          gates:
            - id: synthesize
              role: architect
              guidance: [milestone.synthesis, hitl.pro]
              tools:
                - code_map_inspect
                - code_map_impact
                - code_map_overview
                - code_map_search
                - ast_grep_search
                - forge_read
                - task_blackboard
              proof: blueprint
              save:
                pre: snapshot
                post: snapshot
            - id: review
              role: independent_reviewer
              guidance: [milestone.blueprint_review, hitl.pro]
              policy: default_review
              contours: guidance.contours
              independent_from: synthesize
              fail: synthesize
              tools:
                - code_map_inspect
                - code_map_impact
                - code_map_search
                - ast_grep_search
                - forge_read
                - task_blackboard
              save:
                immutable: true
      ```
    - Inside `- tasks: gates:`, add the language-neutral `- id: seam` (`when: seam`) container between `review` and `deliver`:
      - `integrate` (`proof: seam_test`, `role: builder`, `guidance: [task.deferred_final_test, hitl.pro]`, `save: {pre: snapshot, post: snapshot}`),
      - `review` (`role: independent_reviewer`, `guidance: [task.seam_review, hitl.pro]`, `policy: default_review`, `contours: guidance.contours`, `independent_from: integrate`, `fail: integrate`, `loop: {scope: integrate, symbols: "contract"}`, `save: {immutable: true}`).
    - Include post-task root gates `verify` (`proof: command`, `max_attempts: 2`, `pass: milestone_review`, `fail: polish`), `polish` (`role: architect`, `max_attempts: 2`, `pass: verify`, `save: {pre: snapshot, post: snapshot}`), `milestone_review` (`role: independent_reviewer`, `max_attempts: 2`, `policy: default_review`, `independent_from: polish`, `pass: handoff`, `fail: polish`, `save: {immutable: true}`), and `handoff` (`receipt: true`, `absorb: true`, `save: commit`).
  - **Updates to [`.forge/acdd/profile.yaml`](../../.forge/acdd/profile.yaml)**:
    - Set `version: 2` and `inherit: default`.
    - Retain only repository-specific `commands` (`check-and-test`, `rust-check-and-test`, `check_full_repo`, `test`), `scope_args_by_pattern`, and minimal gate command bindings (`tasks.contract.command: test`, `tasks.build.command: rust-check-and-test`, `tasks.seam.integrate.command: rust-check-and-test`, `verify.command: check_full_repo`).

## 3. Universal Topological Root-Gate State Machine and Nested Seam Delivery Fix

- **Target**: Enforce universal topological barriers for pre-task (`root_index < task_position`) and post-task (`root_index > task_position`) root gates without gate-ID hardcoding, and fix nested `seam` branch review resolution during `deliver`.
- **Scope**: [crates/forge-tasks/src/engine/gate_graph.rs, crates/forge-tasks/src/engine/lifecycle.rs, crates/forge-tasks/src/engine/gates/submit.rs, crates/forge-tasks/src/engine/orchestrator/waves.rs]
- **Depends On**: [Declarative Profile Inheritance and Proof Catalog Resolution]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "3.1", title: "Enforce topological pre-task root gate barrier (root_index < task_position) in task claim and task next", status: "completed"}, {subtask_ref: "3.2", title: "Support unified claim/submit/next transitions for pre-task and post-task root gates (subject = milestone)", status: "completed"}, {subtask_ref: "3.3", title: "Fix submit.rs line 370 to split _snapshot_gate.canonical_stage_id instead of gate.id for nested seam.review lookup", status: "completed"}]

- **Implementation Details**:
  - **Topological Barrier 1 (`cursor.root_index < task_position`)**:
    - In [`crates/forge-tasks/src/engine/lifecycle.rs`](../../crates/forge-tasks/src/engine/lifecycle.rs) and [`collect_root_cursors`](../../crates/forge-tasks/src/engine/gate_graph.rs), before allowing `task claim` on any task in a milestone, inspect all applicable (`when`) root cursors with `cursor.root_index < task_position`.
    - If any pre-task root cursor lacks an accepted pass transition in `gate_transitions` (`subject = "milestone"`), reject `task claim` with `MILESTONE_PRE_TASK_GATE_PENDING` and report the active root cursor (`canonical_stage_id` and `gate_path`).
  - **Topological Barrier 2 (`cursor.root_index > task_position`)**:
    - Unlock post-task root cursors (`cursor.root_index > task_position`) only after all non-cancelled milestone tasks reach `status == "completed"`.
    - Update `task next` so it surfaces the active pre-task root gate first, then ready wave tasks, and finally active post-task root gates.
  - **Nested `- id: seam` Delivery Fix in [`submit_with_pre_persist`](../../crates/forge-tasks/src/engine/gates/submit.rs)**:
    - At line 370 of `crates/forge-tasks/src/engine/gates/submit.rs`, replace `_snapshot_gate.gate.id.rsplit_once('.')` with `_snapshot_gate.canonical_stage_id.rsplit_once('.')`.
    - When `seam.integrate` (`canonical_stage_id = "seam.integrate"`, `gate.id = "integrate"`) produces the latest candidate snapshot and `seam.review` (`canonical_stage_id = "seam.review"`) approves it, `deliver` (`need: [review]`) resolves `branch_prefix = "seam"` and validates `seam.review` against the `seam.integrate` commit instead of failing with `TASK_CANDIDATE_MISMATCH`.

## 4. Per-Task Proof Recording in Gate Transitions and Automatic Context Injection

- **Target**: Automatically fan out per-task slices (`proof.tasks[<task_ref>]`) from any passed pre-task root gate into `gate_transitions` with `subject = <full_task_id>`, and automatically inject only that task's accepted proofs into `context_bundle` upon `task claim` / `task context`.
- **Scope**: [crates/forge-tasks/src/store/trace.rs, crates/forge-tasks/src/engine/gates/submit.rs, crates/forge-tasks/src/engine/context.rs, crates/forge-tasks/src/engine/lifecycle.rs]
- **Depends On**: [Universal Topological Root-Gate State Machine and Nested Seam Delivery Fix]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "4.1", title: "Validate task key parity and fan out proof.tasks[task_ref] into gate_transitions with subject = full_task_id", status: "completed", evidence: "Task key parity and seam coverage validated before recording root pass transitions; verified by Sol Round 2 and Round 3"}, {subtask_ref: "4.2", title: "Query gate_transitions for subject = full_task_id in context_bundle and inject task-scoped inherited proofs", status: "completed", evidence: "Queried gate_transitions for subject = full_task_id in context_bundle; confirmed by Sol review"}, {subtask_ref: "4.3", title: "Preserve task-scoped inherited proof slice in bound_task_response without truncation", status: "completed", evidence: "Preserved per-task proof slice in bound_task_response; 75 library tests and 342 ACDD tests passed"}]

- **Implementation Details**:
  - **Universal Fan-Out on Root Gate Pass**:
    - When a pre-task root gate (or its review container) completes with `action = "pass"` and its validated `proof_payload` (`proof: { scheme: ... }`) contains a `tasks` map (`tasks: { "<task_ref>": { ... } }`):
      - Verify that every non-cancelled task in the milestone has a corresponding entry in `tasks` and that every declared `depends_on` or parallel seam edge is represented in the task's `incoming_seams` / `outgoing_seams`.
      - Insert a [`GateTransition`](../../crates/forge-tasks/src/store/trace.rs) row into `gate_transitions` for each task with:
        - `milestone_id`: full namespace-qualified milestone ID,
        - `subject`: `<full_task_id>` (`<milestone_ref>:<task_ref>`),
        - `gate_path`: the root gate's `gate_path` (e.g. `gates.blueprint_synthesis.synthesize`),
        - `action`: `"advance"`,
        - `proof_kind`: the catalog proof entry name (e.g. `"blueprint"`) or `"scheme"` for inline gate schemas,
        - `proof_payload`: the isolated JSON object for `<task_ref>` (plus shared `cited_invariants`, `cited_symbols`, `global_negative_boundaries`, and `blackboard_decision_id`).
  - **Automatic Per-Task Context Injection on `task claim` / `task context`**:
    - In [`context_bundle`](../../crates/forge-tasks/src/engine/context.rs), query `gate_transitions` for the latest accepted (`action = 'advance'`) rows matching `WHERE milestone_id = ?1 AND subject = ?2` (`subject == task_id`).
    - Populate `context_bundle` with only that task's inherited proof payloads (and for tasks entering `seam.integrate`, include the current snapshot SHA and stage status of partner tasks referenced in `incoming_seams` / `outgoing_seams`).
    - Protect the injected per-task proof slice in [`bound_task_response`](../../crates/forge-tasks/src/engine/lifecycle.rs) so response size bounding never strips the task's architectural blueprint.

## 5. Standalone Artifact Authoring Workflow Profiles

- **Target**: Create the three standalone authoring workflow profiles (`author-milestone.workflow.yaml`, `author-plan.workflow.yaml`, `author-workflow.workflow.yaml`) with inline `proof.scheme` definitions in `docs/workflows/` and skill reference examples.
- **Scope**: [docs/workflows/author-milestone.workflow.yaml, docs/workflows/author-plan.workflow.yaml, docs/workflows/author-workflow.workflow.yaml, skills/contextunity-forge/references/acdd-profile-examples/author-milestone.workflow.yaml, skills/contextunity-forge/references/acdd-profile-examples/author-plan.workflow.yaml, skills/contextunity-forge/references/acdd-profile-examples/author-workflow.workflow.yaml]
- **Depends On**: [Canonical Default Profile and Slimmed Repository Profile Overlay, Per-Task Proof Recording in Gate Transitions and Automatic Context Injection]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "5.1", title: "Create author-milestone.workflow.yaml with inline proof.scheme and milestone init preflight", status: "completed", evidence: "Created docs/workflows/author-milestone.workflow.yaml with inline proof.scheme, mirrored to skills, validated via contextunity-forge-mcp validate and workflow show"}, {subtask_ref: "5.2", title: "Create author-plan.workflow.yaml with inline proof.scheme and plan show preflight", status: "completed", evidence: "Created docs/workflows/author-plan.workflow.yaml with inline proof.scheme, mirrored to skills, validated via contextunity-forge-mcp validate and workflow show"}, {subtask_ref: "5.3", title: "Create author-workflow.workflow.yaml with inline proof.scheme and workflow show preflight", status: "completed", evidence: "Created docs/workflows/author-workflow.workflow.yaml with inline proof.scheme, mirrored to skills, validated via contextunity-forge-mcp validate and workflow show"}]

- **Implementation Details**:
  - **Three Standalone Authoring Workflows (`inherit: none`, `save: none`, `synthesize` $\to$ `review` $\to$ `preflight`)**:
    1. **`author-milestone.workflow.yaml`**:
       - `synthesize` (`role: architect`, inline `proof: { scheme: ... }`): grounds symbols via `code_map_inspect` and invariants via `search_docs`/`get_doc`, partitions tasks into acyclic waves with disjoint same-wave write scopes (`scope_A ∩ scope_B = ∅`), forbids protected files (`Cargo.toml`, `build.rs`, `.forge/**`), and writes `docs/milestones/<id>.md`.
       - `review` (`role: independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`): audits scope disjointness, layer isolation, negative boundaries, and symbol authenticity.
       - `preflight` (`command: validate_milestone`): runs `contextunity-forge-mcp milestone init <manifest_path> --in-place` to verify schema, wave DAG, and scope confinement.
    2. **`author-plan.workflow.yaml`**:
       - `synthesize` (`role: architect`, inline `proof: { scheme: ... }`): authors `docs/plans/<slug>.md` with `doc_type: plan`, numbered `## N. <Step Title>` headings, valid `- **Target**:`, `- **Scope**:`, `- **Status**: ready`, and acyclic `- **Depends On**:` references matching [`PlanDocument::from_sections`](../../crates/forge-tasks/src/engine/orchestrator/plan.rs).
       - `review` (`role: independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`): audits step granularity, in-place `save: snapshot` safety, and scope validity.
       - `preflight` (`command: validate_plan`): runs `contextunity-forge-mcp plan show <plan_name>`.
    3. **`author-workflow.workflow.yaml`**:
       - `synthesize` (`role: architect`, inline `proof: { scheme: ... }`): authors `docs/workflows/<name>.workflow.yaml` with `inherit: none`, `save: none`, no `tasks:` container, no `receipt: true`, and valid `pass:` / `fail:` / `independent_from` transitions.
       - `review` (`role: independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`): audits gate transitions, role separation, and inline `proof.scheme` definitions.
       - `preflight` (`command: validate_workflow`): runs `contextunity-forge-mcp workflow show <workflow_name>`.

## 6. Production-Seam Verification and Commitment Integrity

- **Target**: Prove profile deep-merge inheritance, topological root-gate barriers, per-task blueprint proof fan-out and claim injection, nested `seam` delivery, and authoring workflow validation with dedicated domain tests.
- **Scope**: [tests/acdd/milestones/mod.rs, tests/acdd/tasks/mod.rs, tests/commitment_integrity.rs]
- **Depends On**: [Standalone Artifact Authoring Workflow Profiles]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "6.1", title: "Author domain test for inherit: default gate-by-id deep merge and .forge/acdd/profile.yaml overlay loading", status: "completed", evidence: "Authored inherit_default_deep_merges_gate_overrides_and_loads_repository_overlay in tests/acdd/tasks/guidance/profile_defaults.rs; passes cleanly"}, {subtask_ref: "6.2", title: "Author domain test for pre-task blueprint_synthesis container barrier and per-task gate_transitions proof injection on claim", status: "completed", evidence: "Authored pre_task_blueprint_synthesis_container_barrier_and_per_task_proof_injection in tests/acdd/orchestrator/waves.rs; verified gate barrier, fan-out advance transition, and per-task proof injection"}, {subtask_ref: "6.3", title: "Author domain test for nested when: seam (seam.integrate -> seam.review -> deliver) candidate snapshot resolution", status: "completed", evidence: "Authored nested_when_seam_candidate_snapshot_resolves_cleanly_at_delivery in tests/acdd/tasks/lifecycle/delivery.rs; verified candidate snapshot resolution at deliver stage without mismatch"}, {subtask_ref: "6.4", title: "Author domain test validating author-milestone, author-plan, and author-workflow profiles", status: "completed", evidence: "Authored authoring_workflow_profiles_validate_cleanly_and_enforce_standalone_authoring_invariants in tests/acdd/harness/workflows.rs; verified all three standalone authoring workflow profiles validate with zero errors"}]

- **Implementation Details**:
  - Verify that `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --test acdd`, and `cargo test --test commitment_integrity` pass with zero warnings or regressions.

## 7. Dynamic Limit Derivation from Configuration and Unified Policy

- **Target**: Eliminate hardcoded magic numbers across storage, lifecycle responses, metadata, profile validation, command traces, and evidence compilers, ensuring exactly one authoritative constant in code per limit category, explicitly regulatable via `forge-mcp.yaml` (Point 1: payload/response limit elevated to 96 KiB / 98,304 bytes regulated via `response.max_output_bytes`; Point 2: metadata/notes limit regulated via `limits.max_metadata_bytes` and `MAX_METADATA_BYTES`).
- **Scope**: [forge-mcp.yaml, crates/forge-core/src/response.rs, crates/forge-core/src/config.rs, crates/forge-tasks/src/config.rs, crates/forge-tasks/src/store/trace.rs, crates/forge-tasks/src/engine/lifecycle.rs, crates/forge-tasks/src/tasks/profile/validate.rs, crates/forge-tasks/src/tasks/acdd.default.yaml, crates/forge-tasks/src/tasks/gates.rs, crates/forge-tasks/src/engine/evidence_compiler.rs, crates/forge-tasks/src/milestones/integration.rs, tests/acdd/tasks/lifecycle/delivery.rs, tests/acdd/runner/command_gate_proofs.rs]
- **Depends On**: [Production-Seam Verification and Commitment Integrity]
- **Status**: completed
- **Subtasks**: [{subtask_ref: "7.1", title: "Unify response and payload limits under forge-mcp.yaml response.max_output_bytes and forge_core::response::MAX_OUTPUT_BYTES (98,304 bytes / 96 KiB)", status: "completed", evidence: "Unified response policy and storage limits under forge_core::response::MAX_OUTPUT_BYTES (98,304 bytes); updated forge-mcp.yaml to 98304"}, {subtask_ref: "7.2", title: "Replace hardcoded TASK_RESPONSE_BYTES (24 KiB) in lifecycle.rs with dynamic scaling derived from ResponsePolicy::max_output_bytes", status: "completed", evidence: "Replaced hardcoded 24 KiB constant with dynamic task_response_budget derived as (max_output_bytes * 5) / 8 from ResponsePolicy in lifecycle.rs"}, {subtask_ref: "7.3", title: "Upgrade SQLite trace table check constraint and migration in trace.rs to <= 98304 bytes and bind MAX_OUTPUT_SNIPPET_BYTES to MAX_OUTPUT_BYTES", status: "completed", evidence: "Bound MAX_OUTPUT_SNIPPET_BYTES to MAX_OUTPUT_BYTES; cleaned SQLite trace table schema check constraint formatted via constant; eliminated all legacy migrations and rename shims"}, {subtask_ref: "7.4", title: "Expose limits.max_metadata_bytes in forge-mcp.yaml and ScannerLimits, backed by authoritative forge_core::config::MAX_METADATA_BYTES (2048 bytes)", status: "completed", evidence: "Exposed limits.max_metadata_bytes: 2048 in forge-mcp.yaml and ScannerLimits backed by MAX_METADATA_BYTES"}, {subtask_ref: "7.5", title: "Replace hardcoded 2048 literals in evidence_compiler.rs, integration.rs, trace.rs, and validate.rs with MAX_METADATA_BYTES and config limits", status: "completed", evidence: "Replaced all magic 2048 literals across evidence_compiler.rs, integration.rs, trace.rs, and validate.rs with MAX_METADATA_BYTES and profile.max_log_bytes()"}, {subtask_ref: "7.6", title: "Update forge-mcp.yaml, acdd.default.yaml, profile limits, and add regression tests verifying 96 KiB payload and metadata bounds", status: "completed", evidence: "Updated acdd.default.yaml max_log_bytes to 98304; verified delivery and command gate regression tests pass across 346/346 ACDD and 75/75 library tests"}]

- **Implementation Details**:
  - **Point 1 (Response & Payload Bounding - 96 KiB / 98,304 bytes regulated via `forge-mcp.yaml response.max_output_bytes`)**:
    - **Single authoritative constant**: `forge_core::response::MAX_OUTPUT_BYTES: usize = 96 * 1024;` (98,304 bytes).
    - **Explicit configuration in `forge-mcp.yaml`**: set `response.max_output_bytes: 98304` (elevating from 65,536 bytes to 96 KiB).
    - **Storage snippet bounds (`crates/forge-tasks/src/store/trace.rs`)**:
      - Replace hardcoded `64 * 1024` with `pub const MAX_OUTPUT_SNIPPET_BYTES: usize = forge_core::response::MAX_OUTPUT_BYTES;` (98,304 bytes).
      - In `trace.rs`, update table `gate_execution_traces` schema `CHECK (output_snippet IS NULL OR length(CAST(output_snippet AS BLOB)) <= 98304)`.
      - Enforce the single clean `schema_version = '7'` baseline atomically on fresh initialization and fail closed on missing tables/triggers or version mismatches, with no legacy schema migrations or automatic table-rebuild upgrades.
    - **Lifecycle task response bounding (`crates/forge-tasks/src/engine/lifecycle.rs`)**:
      - Eliminate static `const TASK_RESPONSE_BYTES: usize = 24 * 1024;`.
      - Scale task response bounding dynamically from the workspace's configured `ResponsePolicy::max_output_bytes` (e.g. `(policy.max_output_bytes * 5) / 8` $\approx$ 60 KiB, or `(policy.max_output_bytes * 3) / 4`), preventing artificial truncation of inherited proofs and partner state in `bound_task_response`.
    - **Command log bounding (`crates/forge-tasks/src/tasks/gates.rs` and `evidence_compiler.rs`)**:
      - Replace raw literal `64 * 1024` in `gates.rs:205` and `evidence_compiler.rs:802` with `profile.policies.proof.command.max_log_bytes` falling back to `forge_core::response::MAX_OUTPUT_BYTES`.
    - **Profile validation (`crates/forge-tasks/src/tasks/profile/validate.rs`)**:
      - Permit `limits.diagnostic_inline_bytes` up to `forge_core::response::MAX_OUTPUT_BYTES` (instead of hardcoded `<= 65536`).
    - **Default ACDD profile (`crates/forge-tasks/src/tasks/acdd.default.yaml`)**:
      - Update default command policy `max_log_bytes: 98304`.

  - **Point 2 (Metadata & Git Notes Bounding - 2048 bytes regulated via `forge-mcp.yaml limits.max_metadata_bytes`)**:
    - **Single authoritative constant**: define `pub const MAX_METADATA_BYTES: usize = 2048;` and `pub const DEFAULT_MAX_METADATA_BYTES: usize = MAX_METADATA_BYTES;` in `crates/forge-core/src/config.rs`, and re-export in `crates/forge-tasks/src/config.rs`.
    - **Explicit configuration in `forge-mcp.yaml`**: add explicit `limits.max_metadata_bytes: 2048` under `limits:` in `forge-mcp.yaml`.
    - **ScannerLimits & AdapterFile**: add `max_metadata_bytes: usize` to `ScannerLimits` with default `DEFAULT_MAX_METADATA_BYTES`, allowing explicit overriding via `forge-mcp.yaml`.
    - **Elimination of scattered `2048` literals**:
      - In `evidence_compiler.rs:646`: replace literal `truncate_utf8(&mut note, 2048)` with `truncate_utf8(&mut note, MAX_METADATA_BYTES)`.
      - In `integration.rs:670`: replace `note_json.len() >= 2048` with `note_json.len() >= MAX_METADATA_BYTES`.
      - In `trace.rs:658`: replace `breadcrumb.len() > 2048` with `breadcrumb.len() > MAX_METADATA_BYTES`.
      - In `validate.rs:548-556`: replace literal `2048` in `limits.max_metadata_bytes > 2048`, `limits.max_trace_summary_bytes > 2048`, and `git_commit_note.limits.max_bytes > 2048` with `MAX_METADATA_BYTES`.
