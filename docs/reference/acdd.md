---
title: "Augmented Context-Driven Delivery (ACDD)"
doc_type: guide
---

# Augmented Context-Driven Delivery (ACDD)

**Augmented Context-Driven Delivery (ACDD)** is Forge's lifecycle state machine for structured AI and human software engineering. Instead of letting coding agents run open-ended loops over an entire repository, ACDD binds Git-tracked specifications, SQLite state, and declarative YAML profiles into atomic, verifiable **gates** that deliver only the context needed for the active step.

---

## 1. What ACDD Is

ACDD separates *what* must be built from *how* work is governed and verified:

1. **Git-Tracked Authority (`docs/milestones/*.md`, `docs/plans/*.md`)**: Humans and architects keep contracts, task write perimeters (`scope`), dependency edges (`depends_on`), and proof policies in Markdown under Git.
2. **Persistent Queue and Proof Store (`.forge/tasks.sqlite`)**: Forge synchronizes contracts into SQLite (`tasks`, `gate_transitions`, `task_claims`, `task_blackboard`). Rebuilding `.forge/code-map.sqlite` never resets active task state or receipts.
3. **Declarative Governance Profile (`profile.yaml`)**: Defines the ordered gate graph, required proof schemas, role assignments, command catalogs, HITL escalation rules, and review contours.

### Stage-Tailored Context Injection

Claiming a gate returns two complementary payloads ([ADR 0016](../adr/0016-lean-stage-tailored-task-context-bundles.md)):

- **`workflow_guidance`**: Names the active `role`, operational instructions, acceptance criteria, `NO_PREFLIGHT_SUBAGENTS`, scoped verification commands (`scoped_commands`), and the exact expected evidence template (`expected_evidence_template`).
- **`context_bundle`**: Supplies only the artifacts required for the active gate:
  - **Contract and build gates**: Admitted write scope, indexed symbol signatures, external callers, covering tests, ADR invariant summaries, target RFC/blackboard messages, and the task's own inherited root-gate proof slice (such as its per-task `blueprint_synthesis` slice from `gate_transitions` with `subject = <full_task_id>`).
  - **Review gates**: The immutable candidate snapshot diff and claimed contract context.
  - **Delivery gates**: Receipt rules, verification status, and subtask completion checks.

Recent `task_blackboard` entries ride along inside the bundle; historical gate logs remain in SQLite until inspected via `task inspect`. MCP task tools and CLI task commands render Markdown by default (preserving guidance, evidence, and caution callouts for blockers) or structured JSON via `format: "json"` / `--format json`; see the [task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle).

### Three Execution Tiers

ACDD applies one shared gate, proof, and guidance engine across three operational models ([ADR 0021](../adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md)):

| Tier | Authority Source | Scope & Task Queue | Save Mode | Primary Use Case |
|---|---|---|---|---|
| **Milestones** | `docs/milestones/*.md` | Root pre-task & closure gates + `- tasks:` DAG waves in isolated worktrees | `commit` (receipts + fast-forward integration) | Multi-task feature delivery with upfront architecture synthesis and verified handoff |
| **Plans** | `docs/plans/*.md` (`doc_type: plan`) | `- tasks:` container executed in-place without root preflight or handoff gates | `snapshot` (no auto-commits) | Multi-step documentation-driven changes on the current branch |
| **Workflows** | `docs/workflows/*.yaml` | Root gates only (no `- tasks:` container) | `none` or `log` | Autonomous sweeps, quality loops, and standalone artifact authoring |

---

## 2. How Gates Work

Everything in an ACDD workflow is a **gate** declared under `gates:` in a YAML profile. Every gate has one primary driver:

- **Agent gate** (`role: <role_id>`): Claimed by a worker session (`task claim` / `task_claim`) and completed via `task submit` / `task_submit` with the required typed proof. An agent gate may also bind a registered `command:` so `task verify` can observe compiler and test results on the worker's behalf.
- **Command gate** (`command: <command_id>`, no `role`): Executed deterministically by Forge (zero LLM tokens), parsing test counts and failure codes directly from command output.
- **Container gate** (`gates: [...]`): Groups sequential child gates under a dotted hierarchical path (for example `blueprint_synthesis.synthesize` $\to$ `blueprint_synthesis.review`, or `seam.integrate` $\to$ `seam.review`).
- **Task container** (`- tasks: { gates: [...] }`): Expands its child gate sequence for every admitted task, scheduled in topological DAG waves derived from `depends_on` / `task_depends_on`.

### Hierarchical gate cursors and seam proofs

A profile gate can contain child gates. [`claim_nested_gate`](../../crates/forge-tasks/src/engine/claim.rs) resolves the persisted stage to its active leaf gate, applies that leaf's role and proof requirements, and accepts qualified child stage IDs. Passing a child advances the cursor to the next child.

Any applicable (`when`) root gate positioned before `- tasks:` (`cursor.root_index < task_position`) executes with `subject = "milestone"` and blocks task claims (`MILESTONE_PRE_TASK_GATE_PENDING`) until it passes in `gate_transitions`. When a pre-task root gate passes with a `proof.scheme` payload containing `tasks: { "<task_ref>": { ... } }`, the engine fans out each task's slice into `gate_transitions` with `subject = <full_task_id>` (`<milestone_ref>:<task_ref>`) and automatically injects only that task's slice into `context_bundle` upon `task claim`. Claims expose the active gate path and breadcrumb in `workflow_guidance`; task-specific RFCs and gate-targeted blackboard messages appear there alongside active broadcasts, while the general `blackboard` field retains task, sibling, and milestone context.

[`validate_seam_reachability`](../../crates/forge-tasks/src/engine/gates/seam.rs) checks the current test source against indexed call and import evidence, follows module imports and public re-exports, and permits up to four test-helper hops before requiring an admitted production target. When source drift invalidates an indexed route, the gate reports `TASK_SEAM_STALE` with an index-refresh action. A disconnected seam reports discovered calls, coverage, reached targets, and the reason traversal stopped. The task engine rejects disconnected contract seams and public signature changes whose indexed callers fall outside admitted scope.

### Independence, Routing, Conditions, and Persistence

- **Independent Worker Enforcement (`independent_from`)**: For any gate declaring `independent_from`, claim and submission compare its `worker_id` against the accepted worker of that predecessor. Session metadata is optional and does not decide gate independence. Lifecycle metadata uses explicit in-process identity or `FORGE_AGENT_SESSION_ID`, without host scraping. Configured runners retain distinct real session fingerprints, isolated process groups, and rejection resumption.
- **Dispatch Directives**: Submission to an independent next gate returns a structured `dispatch` object derived from the active role and prior worker, along with the `[INDEPENDENT GATE DISPATCH REQUIRED]` directive and suggested worker `{task_slug}-{gate}`. The coordinator performs that dispatch; reading guidance launches no process. See [gate dispatch and evidence](tasks.md#gates-and-evidence).
- **Failure & Loop Routing (`pass`, `fail`, `loop`)**: Passing advances sequentially or to the named `pass:` target; rejecting or failing routes to `fail:`. Declaring `loop: { scope: <gate>, symbols: <gate> }` on a review gate rewinds context back to the builder session (`reuse_on_reject: true`) with structured reviewer findings (`task_findings`) without losing session history or widening admitted scope or symbols.
- **Conditional Gates (`when`) & Multi-Attempt Limits (`max_attempts`)**: Gates with `when: seam` run only when the task participates in cross-task seam dependencies. `max_attempts` caps automated retry loops before escalating to human or architect intervention.
- **Save & Immutability (`save`)**: Controls snapshot and commit semantics around a gate (`pre: snapshot`, `post: snapshot | commit`, `immutable: true` to reject working-tree tampering via `TASK_CANDIDATE_MISMATCH`, `git_commit_note: telemetry`, `receipt: true`, and `absorb: true` on terminal milestone handoff).

---

## 3. How Universal Profiles Work

ACDD profiles are **universal and language-agnostic**. The gate graph, roles, review policies, proof schemas, and guidance rules live in the profile catalog independently of programming language or test runner; repository profiles only bind local commands and path patterns.

### Loading Precedence

Forge resolves the active profile from highest to lowest priority ([ACDD profiles](profiles.md)):

1. **Task pin**: `acdd_profile: "<path>:<sha256_prefix>"` in task YAML metadata.
2. **Milestone frontmatter**: `acdd_profile: "<path>:<sha256_prefix>"` in the milestone Markdown header.
3. **Workspace adapter**: `acdd_profile: "<path>"` in `forge-mcp.yaml`.
4. **Repository default**: `.forge/acdd/profile.yaml`.
5. **Embedded baseline**: Built-in [`acdd.default.yaml`](../../crates/forge-tasks/src/tasks/acdd.default.yaml).

### Inheritance (`inherit: default` vs `inherit: none`)

Every profile specifies one of two compilation modes:

- **`inherit: default`** (also the default when `inherit` is omitted): Inherits all `proofs`, `roles`, `guidance`, `policies`, `limits`, `polish_scope`, and `gates` from the embedded baseline. Keyed maps (`roles`, `commands`, `guidance`, `policies`, `proofs`, `scope_args_by_pattern`) merge by key. The `gates:` tree merges **recursively by gate `id` (and by the `- tasks:` container)** before normalization, so an overlay only needs to bind project commands (for example `command: rust-check-and-test`) onto inherited gates without repeating roles, contours, or proofs.
- **`inherit: none`**: Compiles strictly from the file's own declarations with no inherited gates, roles, policies, proofs, or guidance. Standalone workflows (such as artifact authoring under `docs/workflows/`) and fully custom team lifecycles use `inherit: none`.

### Language-Agnostic Customization Mechanisms

- **Pattern-Dispatched `commands` & `scope_args_by_pattern`**: Command definitions support `by_pattern:` (matching `**/*.rs`, `**/*.py`, `**/*.{ts,tsx,js,jsx,vue}`, `**/*.go`, etc.) and `{scope_target}` expansion. When verifying a scoped test or module, `scope_args_by_pattern` turns touched file paths into target-specific flags (such as `--lib` or `--test <target>`). Declarative `output:` regex rules extract test counts (`tests_passed`, `tests_failed`) and individual test names so zero-exit runs without recognized tests cannot satisfy test proof gates.
- **Guidance Slots (`hitl` and `contours`)**: Gates can request parameterized guidance slots such as `hitl`, which milestone frontmatter binds via `guidance: { hitl: pro }` (resolving to `hitl.pro` by default, or `hitl.careless`, `hitl.curious`, `hitl.paranoic`). Review gates reference `contours: guidance.contours` to pull all `contour.*` entries from the active guidance catalog.

### Typical Repository Overlay (`.forge/acdd/profile.yaml`)

Most repositories only need a compact overlay that inherits the full default gate machine and binds their language commands:

```yaml
version: 2
inherit: default

scope_args_by_pattern:
  "src/**/*.rs": "--lib"
  "tests/*.rs": "--test {file_stem}"

commands:
  test:
    command: "cargo test"
  rust-check-and-test:
    command: "cargo clippy {scope_target} --all-features -- -D warnings && cargo test {scope_target}"
  check_full_repo:
    command: "cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets"

gates:
  - tasks:
      gates:
        - id: contract
          command: test
        - id: build
          command: rust-check-and-test
        - id: seam
          gates:
            - id: integrate
              command: rust-check-and-test
  - id: verify
    command: check_full_repo
```

For full field schemas, output parser rules, and multi-domain templates, see [ACDD profiles](profiles.md), the standalone template [`acdd_profile.yaml.example`](acdd_profile.yaml.example) (symlinked from the Forge skill), and the [domain examples catalog](../../skills/contextunity-forge/references/acdd-profile-examples/README.md).

---

## Built-in Default Profile Walkthrough (`acdd.default.yaml`)

The embedded baseline [`crates/forge-tasks/src/tasks/acdd.default.yaml`](../../crates/forge-tasks/src/tasks/acdd.default.yaml) (`version: 2`) defines a complete three-phase milestone lifecycle. Because `commands: {}` and `scope_args_by_pattern: {}` are intentionally empty in the baseline, repository overlays supply concrete build/lint/test commands via `inherit: default`.

```text
Phase 1: Pre-Task Root Container (`blueprint_synthesis`)
  └─ synthesize (architect, proof: blueprint) ──► review (independent_reviewer, fail -> synthesize)
          │
          ▼  (fans out per-task blueprint slices into gate_transitions; lifts task claim barrier)
Phase 2: Per-Task DAG Container (`- tasks:`, executed in topological waves)
  └─ contract (contract_author, red exit [1, 101, 134])
       └─► build (builder, snapshot)
             └─► review (independent_reviewer, 5 contours, immutable, fail -> build)
                   └─► [if seam] seam.integrate (builder, seam_test) ──► seam.review (independent_reviewer)
                         └─► deliver (independent_reviewer, need: [review], receipt + commit + telemetry)
          │
          ▼  (all tasks terminal)
Phase 3: Post-Task Milestone Closure
  └─ verify (command: check_full_repo, max 2)
       ├─ fail ──► polish (architect, polish_scope, pass -> verify)
       └─ pass ──► milestone_review (independent_reviewer, immutable, fail -> polish)
                     └─ pass ──► handoff (receipt + absorb + commit)
```

### Global Settings, Proofs, Roles, and Contours

- **Limits & Telemetry (`limits`, `git_commit_note`)**: Bounds metadata (`max_metadata_bytes: 2048`), trace summaries (`max_trace_summary_bytes: 2048`), inline diagnostics (`diagnostic_inline_bytes: 4096`), and command timeout (`max_command_timeout_seconds: 1800`). Attaches Markdown commit telemetry (duration, tasks summary, review cycles, diff breakdown, AST delta, test suite counts, and topology) under `git_commit_note`.
- **`polish_scope`**: Permits bounded root-level polish adjustments inside `.forge/**`, `docs/**`, `skills/**`, `AGENTS.md`, `**/AGENTS.md`, `README.md`, and `.gitignore`.
- **Proof Catalog (`proofs`)**:
  - `blueprint`: Grounded architectural plan requiring `cited_invariants`, `cited_symbols`, `global_negative_boundaries`, `blackboard_decision_id`, and a per-task dictionary `tasks` (each specifying `layer`, `summary`, `exported_interfaces`, `negative_boundaries`, `incoming_seams`, `outgoing_seams`).
  - `contract`: Requires `contract_proof` (`seam_test_ref`, non-zero `red_exit_code`).
  - `command`: Requires `command_proof` (`command`, `exit_code: 0`, plus `tests_passed` / `tests_failed: 0` when required).
  - `review`: Requires `review_proof` (`decision: pass | reject`, `contours`, optional `extended_scope_review` and `notes`).
  - `seam_test`: Requires `seam_test_proof` (`exit_code: 0`, `seam_test_refs`, optional test counts).
  - `delivery`: Requires `delivery_proof` (`delivered`, `commit_sha`, `receipt_id`).
- **Roles (`roles`)**: All four roles use `mode: subagent` and `reuse_on_reject: true` so rejected workers resume with full context:
  - `architect` (`sol-6.1:high`): Synthesizes the milestone blueprint, grounds decisions on `task_blackboard`, and performs milestone integration polish.
  - `contract_author` (`sol-6.1:high`): Designs each task's data flow, invariants, negative boundaries, and compiling runtime-red seam test, and posts a `build`-targeted `draft` to `task_blackboard`.
  - `builder` (`gpt6-luna:xhigh`): Implements approved contracts and joint seam integrations strictly inside admitted scope.
  - `independent_reviewer` (`sol-6.1:xhigh`): Audits blueprint synthesis, individual tasks (and claims/submits `deliver` in the same session), seam integration, and final milestone cohesion while staying strictly independent (`worker_id`) from the author/builder being reviewed.
- **Policies (`policies`)**: Enforces `require_zero_exit: true` (`max_log_bytes: 98304`) for commands, `atomic_receipt: true` for delivery, and `default_review` (`independent_worker: true`, `require_all_contours: true`, `audit_extended_scope: true`).
- **Review Contours (`contour.*`)**: Every review gate receives five analytical focus lenses (`guidance.contours`):
  1. `contour.paths`: Changed/staged/untracked/generated files remain inside initial scope and admitted extensions; protected paths untouched.
  2. `contour.claims`: Contract invariants and claims trace directly to production code and real seam tests, including fail-closed error paths.
  3. `contour.concurrency`: Transaction bounds, revision fencing, lock order, and rollback semantics hold under concurrent updates.
  4. `contour.project_isolation`: Workspace roots, task/project IDs, symlink confinement, and profile isolation stay confined to the owning repository.
  5. `contour.administration`: Schema guards, profile pins, Git hooks, candidate parent integrity, and receipts remain intact.
- **HITL Tiers (`hitl.*`)**: Supports `careless`, `curious`, `pro` (default across built-in gates), and `paranoic`.

### Phase 1: `blueprint_synthesis` (Pre-Task Root Container)

Runs once for the milestone (`subject = "milestone"`) before any task inside `- tasks:` can be claimed:

1. **`blueprint_synthesis.synthesize`**:
   - **Role & Tools**: `architect`, tools `[code_map_inspect, code_map_impact, code_map_overview, code_map_search, ast_grep_search, forge_read, task_blackboard]`.
   - **Guidance & Save**: `[milestone.synthesis, hitl.pro]`; `save: { pre: snapshot, post: snapshot }`.
   - **Behavior**: Explores existing symbols, invariants, and extension seams (without authoring tests yet), records an architectural decision on `task_blackboard`, and submits `proof: blueprint`. On submission, Forge validates grounded citations and awaits review before unlocking tasks.
2. **`blueprint_synthesis.review`**:
   - **Role & Independence**: `independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`, `policy: default_review`, `contours: guidance.contours`, `save: { immutable: true }`.
   - **Guidance & Tools**: `[milestone.blueprint_review, hitl.pro]`; read/blackboard navigation tools.
   - **Behavior**: Verifies cited symbols/invariants, Clean Architecture layer placement, per-task completeness, seam consistency across tasks, and the recorded blackboard decision. Accepting `blueprint_synthesis.review` fans out `proof.tasks[<task_ref>]` slices into `gate_transitions` (`subject = <full_task_id>`) and lifts the `MILESTONE_PRE_TASK_GATE_PENDING` barrier.

### Phase 2: `- tasks:` (Per-Task DAG Container)

Executed for each task in topological `depends_on` order inside `.worktrees/<milestone>-<task-slug>`:

1. **`contract`**:
   - **Role & Tools**: `contract_author`, `tools: [forge_read]`, `guidance: [task.contract, hitl.pro]`.
   - **Validation & Save**: `expected_failure: { exit_codes: [1, 101, 134] }`, `save: snapshot`.
   - **Behavior**: Reads its injected blueprint slice, synthesizes the task's typed invariants and negative boundaries, writes a compiling runtime-red seam test against authentic production paths (no mocks/stubs around complexity), verifies the red exit code, and posts a `build`-targeted blackboard `draft`.
2. **`build`**:
   - **Role & Guidance**: `builder`, `guidance: [task.code, hitl.pro]`, `save: { pre: snapshot, post: snapshot }`.
   - **Behavior**: Inherits the repository's `command:` binding (such as `rust-check-and-test`) from `.forge/acdd/profile.yaml`. Implements the contract within admitted scope until strict linters and the seam test pass (exit code `0`) observed via `task verify`.
3. **`review`**:
   - **Role & Policy**: `independent_reviewer`, `policy: default_review`, `contours: guidance.contours`, `guidance: [task.review, hitl.pro]`.
   - **Independence & Loop**: `independent_from: build`, `fail: build`, `loop: { scope: build, symbols: contract }`, `save: { immutable: true }`.
   - **Behavior**: Audits the immutable candidate snapshot across subsystems (duplicate detection, anti-monolith test hygiene, performance alignment, heuristics rejection, and the 5 contours). Rejections post structured `task_findings` and resume the same `builder` session without expanding admitted scope or symbols.
4. **`seam` (`when: seam` Conditional Container)**:
   - Runs only for tasks participating in cross-task seam dependencies:
   - **`seam.integrate`**: `builder`, `proof: seam_test`, `guidance: [task.deferred_final_test, hitl.pro]`, `save: { pre: snapshot, post: snapshot }`. Integrates and verifies joint cross-task tests (`seam_test_refs`).
   - **`seam.review`**: `independent_reviewer`, `guidance: [task.seam_review, hitl.pro]`, `policy: default_review`, `contours: guidance.contours`, `independent_from: integrate`, `fail: integrate`, `loop: { scope: integrate, symbols: contract }`, `save: { immutable: true }`.
5. **`deliver`**:
   - **Role & Evidence**: `independent_reviewer` (claims and submits in the same session as `review`, remaining `independent_from: build`), `guidance: [hitl.pro]`, `receipt: true`, `need: [review]`, `fail: build`.
   - **Save**: `save: { immutable: true, post: commit, git_commit_note: telemetry }`.
   - **Behavior**: Verifies all subtasks are terminal with non-empty evidence and candidate hash matches `review`, creates the atomic delivery commit with telemetry Git note, and records the immutable cryptographic receipt in SQLite.

### Phase 3: Post-Task Milestone Closure Gates

Once all tasks in `- tasks:` reach terminal delivery (or matching cancellation), the milestone advances through its root closure pipeline:

1. **`verify`**:
   - **Driver**: Command gate (`proof: command`, `max_attempts: 2`; project overlays bind `command: check_full_repo`).
   - **Routing**: `pass: milestone_review`, `fail: polish`. Runs full repository verification against the integrated milestone branch with zero token cost.
2. **`polish`**:
   - **Driver**: Agent gate (`role: architect`, `max_attempts: 2`, `guidance: [milestone.polish, hitl]`, `tools: [forge_read, search_docs, get_doc, task_blackboard]`, `save: { pre: snapshot, post: snapshot }`).
   - **Routing**: `pass: verify`. Repairs integration failures or milestone-review findings within `polish_scope` and sends the milestone back through a fresh automated `verify` check.
3. **`milestone_review`**:
   - **Driver**: Agent gate (`role: independent_reviewer`, `max_attempts: 2`, `policy: default_review`, `guidance: [milestone.review]`, `contours: guidance.contours`, `independent_from: polish`, `save: { immutable: true }`).
   - **Routing**: `pass: handoff`, `fail: polish`. Performs holistic integration audit of the milestone diff against the target base (`main`), performance invariants, and duplicate checks. Any edits or documentation gaps send the candidate back to `polish` $\to$ `verify`.
4. **`handoff`**:
   - **Driver**: Terminal gate (`receipt: true`, `absorb: true`, `save: commit`).
   - **Behavior**: Binds final authority, implementation, and target-base digests alongside the observed `verify` trace and independent integration review, records the milestone receipt and `milestone_tombstones`, prunes completed SQLite task rows, and archives the manifest to `docs/milestones/archive/`.

---

## Headless task, milestone, plan and workflow execution

The [CLI orchestration commands](cli.md#structured-plans) share canonical profiles, gate cursors, proof validation, and process supervision. Task runners advance the persisted task lifecycle. Milestone runners validate dependency waves, dispatch that lifecycle serially in-place or through owned isolated worktrees, and integrate accepted deliveries before the next task. Root gate observations stay in transition records; profile, authority, and content bindings control restart reuse without introducing a second task or gate registry.

[`run_standalone_workflow`](../../crates/forge-tasks/src/engine/orchestrator/workflow.rs) executes a workflow's taskless root graph with ephemeral observations and `save: none`. Commands reuse the existing output parser and supervised process executor. Agent roles receive distinct sessions and must return a structured decision with typed proof; rejections carry structured findings to the retained target session, and exit status alone cannot supply role acceptance. Task containers and persistent receipts are refused for these workflows.

### Standalone Artifact Authoring Workflows

Before execution begins, teams can author new milestones, plans, or workflows under `docs/workflows/` (`inherit: none`, `save: none`):

- **`author-milestone.workflow.yaml`**: `synthesize` (`role: architect`, inline `proof.scheme`) $\to$ `review` (`role: independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`) $\to$ `preflight` (`command: validate_milestone` running `contextunity-forge-mcp milestone init {manifest_path} --in-place`). Produces grounded manifests in `docs/milestones/<id>.md` with acyclic waves, disjoint scopes, and Clean Architecture layers.
- **`author-plan.workflow.yaml`**: `synthesize` (`role: architect`, inline `proof.scheme`) $\to$ `review` (`role: independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`) $\to$ `preflight` (`command: validate_plan` running `contextunity-forge-mcp plan show {plan_name}`). Produces structured plans in `docs/plans/<slug>.md`.
- **`author-workflow.workflow.yaml`**: `synthesize` (`role: architect`, inline `proof.scheme`) $\to$ `review` (`role: independent_reviewer`, `independent_from: synthesize`, `fail: synthesize`) $\to$ `preflight` (`command: validate_workflow` running `contextunity-forge-mcp workflow show {workflow_name}`). Produces standalone taskless workflows in `docs/workflows/<name>.workflow.yaml`.

[`run_plan_workflow`](../../crates/forge-tasks/src/engine/orchestrator/plan.rs) projects indexed `doc_type: plan` sections to source-owned steps in validated dependency order. Snapshot persistence preserves HEAD and staged content, explicit execution profiles must be task-only and snapshot-only, delivery enforces the subtask completion/evidence guard, completed steps with evidence are not replayed, and concurrent source edits fail closed.

Milestone verification executes the exact current root command cursor. Handoff validates an independent role decision and Forge-observed command results bound to final authority, implementation, and explicit target-base content. See [milestone commands](cli.md#repository-milestones) and [global options](cli.md#global-options).

---

## 6. Documentation and instructions co-evolution

A task that changes observable behavior, public interfaces, workflows, configuration, or operational assumptions updates matching documentation and agent instructions during its `build` stage (and includes those paths in its initial `scope`). Follow the [shared skill's task guidance](../../skills/contextunity-forge/SKILL.md#acdd-profiles-and-task-operations).
