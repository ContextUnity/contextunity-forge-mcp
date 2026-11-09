---
title: "ADR 0020: Hierarchical ACDD Gates, Modular Catalogs, and Milestone Orchestration"
doc_type: adr
status: accepted
date: 2026-10-21
decision_makers:
  - Oleksii (Architecture Owner)
  - Antigravity (ACDD Coordinator)
consulted:
  - Forge MCP Core Team
informed:
  - Autonomous Agent Coordinators and Builders
---

# ADR 0020: Hierarchical ACDD Gates, Modular Catalogs, and Milestone Orchestration

> [!NOTE]
> This ADR describes the accepted architecture. Round 4 identified gaps in root graph execution and observed proof enforcement; see the [audit plan](../plans/gate-engine-bugs-and-coverage-audit.md). Native headless execution and role dispatch are implemented; their current behavior is described in the [ACDD reference](../reference/acdd.md#headless-task-milestone-plan-and-workflow-execution). Statements below specify the target where gaps remain.

## Context

ACDD task and milestone workflows use one gate model. Task gates consume profile commands and proofs; milestone gates coordinate task dependency waves, checkpoints, verification, and handoff. The same lifecycle state must remain consistent across SQLite, milestone manifests, command evidence, and reviewer decisions.

The architecture therefore needs declarative gate composition, claim-bound proof compilation, breadcrumb-addressed checkpoint persistence, atomic scope synchronization, and a bounded verification path that completes before documentation and handoff.

---

## Decision

We establish the **Universal Hierarchical Gate Architecture**: **Everything is a Gate (`GateDef`)**.

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              MODULAR CATALOGS                                   │
│  • roles:          [architect, builder, reviewer, doc_writer]                   │
│  • guidance:       [milestone.*, task.*, contour.paths, contour.concurrency, ...]│
│  • commands:       [check_task (60s), check_full_repo (10m)]                    │
│  • policies:       [default_review, default_delivery, strict_audit, ...]        │
│  • gate_templates: [build_review_cycle (build -> review)]                        │
└──────────────────────────────────────┬──────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           Hierarchical Gates Tree                               │
│                                                                                 │
│  [ gate: contract_synthesis ] ──────────────────► [ tasks: (per-task DAG) ]      │
│   (role: architect :xhigh)                          (when: code / doc / ...)    │
│   • Synthesizes cross-task seams & helpers                 │                    │
│                                                            ▼                    │
│                            ┌─────────────────────────────────────────────────┐  │
│                            │ Task Gates (Executed Per-Task via DAG):         │  │
│                            │  1. contract (role: architect)                  │  │
│                            │  2. build (role: builder, command: check_task)   │  │
│                            │  3. review (role: reviewer, contours,           │  │
│                            │             loop: {scope: build,                │  │
│                            │                    symbols: contract}           │  │
│                            │  4. deliver (autonomous: save: commit, 0 tokens)          │  │
│                            └────────────────────────┬────────────────────────┘  │
│                                                     │                           │
│                                                     ▼                           │
│  [ gate: verify ] ◄─────────────────────────────────┘                           │
│   (command: check_full_repo, max_attempts: 2)                                   │
│   ├─ pass ──────────────────────────────► [ gate: documentation ] (when: doc)   │
│   │                                                     │                       │
│   │                                                     ▼ (pass)                │
│   │                                             [ gate: handoff ]               │
│   │                                              (post: commit, 0 tokens)       │
│   ▼ (fail)                                                                      │
│  [ gate: polish ] ───(pass)───► [ gate: review ] ───(pass)───► [ verify ]       │
│   (role: architect)              (role: reviewer)                               │
│         ▲                               │ (fail)                                │
│         └───────────────────────────────┘                                       │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

### Pillar 1: Universal Gate Polymorphism & Transition Primitives

Every node in the lifecycle is a `GateDef`. Structural driver selection is distinguished from proof-kind selection and determined by declarative fields:

| Driver Kind | Execution Mechanism | Supported Fields | Structural Exclusions |
| :--- | :--- | :--- | :--- |
| **Agent Gate** | Dispatched to an LLM agent via `task claim` / role runner | `role`, `command`, `guidance`, `tools`, `when`, `pass`, `fail`, `save` | `gates`, `tasks`, `use` |
| **Command Gate** | Executed autonomously by platform shell (0 tokens) | `command`, `max_attempts`, `when`, `pass`, `fail`, `save` | `role`, `guidance`, `gates` |
| **Delivery Gate** | Executed autonomously or supervised (0 tokens: git commit + receipt) | `receipt: true`, `role`, `save: commit`, `when`, `pass`, `fail`, `need` | `gates`, `tasks`, `use` |
| **Container Gate**| Nests child gates recursively, expands templates, or organizes tasks | `gates`, `use`, `tasks`, `when`, `pass`, `fail` | `role`, `command` |

*Precedence*: If `role` is specified, it is dispatched to the agent runner (with optional registered command pre-validation). If `command` is specified without `role`, it is an autonomous Command Gate (command timeouts are owned by command definitions in `commands:`). If `gates`, `tasks`, or `use` is specified, it is a Container Gate. If `receipt: true` is specified, it marks terminal task delivery or milestone handoff.

#### Transition Primitives: `when`, `pass`, `fail`, `loop`
1. **`when:` (Entry Guard)**: Closed set of typed conditions (`code`, `doc`, `db_migration`, `seam`, `public_api`). `code`, `doc`, and `db_migration` inspect task scope; `seam` selects Speculative or Parallel concurrency; `public_api` detects an AST public callable export in scope. If evaluated to false, the gate is skipped (pass-through to next node).
2. **`pass:` (Success Edge)**: Target gate ID executed when this gate succeeds (exit code 0 / review approved / task delivered). If omitted, falls through to the next sequential gate in `gates:`.
3. **`fail:` (Failure Edge)**: Target gate ID executed when this gate fails (exit non-zero / review rejected). If omitted, retries the current gate in `ready` state (or halts if `max_attempts` is exhausted).

4. **`loop:` (Classified Failure Edges)**: Maps a finding class such as `scope` or `symbols` to its target gate.

#### Modular Catalogs: Reusable Templates (`gate_templates:`)
Templates in `gate_templates:` define reusable subgraphs (e.g. `build_review_cycle`). Any container gate can instantiate a template using `use: <template_id>`, binding its entry and exit edges (`pass:`, `fail:`).

#### First-Class Task Lifecycle Container (`tasks:`)
Rather than relying on generic imperative loop annotations like `for_each: tasks`, task execution is represented by the first-class declarative block `- tasks:`:

```yaml
gates:
  - id: contract_synthesis
    role: architect

  # First-class task lifecycle container (executes per-task in DAG order)
  - tasks:
      when: code               # Optional filter: code, doc, or omitted for universal execution
      gates:
        - id: contract
        - id: build
        - id: review
        - id: seam
          when: seam
        - id: deliver

  - id: verify
    command: check_full_repo
    pass: documentation
    fail: polish

  - id: polish
    role: architect
    pass: milestone_review

  - id: milestone_review
    role: independent_reviewer
    pass: verify
    fail: polish

  - id: documentation
    role: doc_writer
    when: doc
    pass: handoff

  - id: handoff
    receipt: true
    save:
      post: commit
```

**Benefits for Sub-Profiles and Modular Composition**:
1. **Zero-Overhead Single-Task & In-Place Execution**: When a single task specifies a custom profile (`task.profile: <id>`) or runs in-place, the task engine extracts and executes strictly the `tasks:` block, effortlessly ignoring milestone-level preflight (`contract_synthesis`) and post-verification (`handoff`) gates.
2. **Cardinality Flexibility (1 or Several `tasks:` Blocks)**:
   - **Single Universal Track**: A profile can declare exactly one `tasks:` block containing multiple child gates (`contract`, `build`, `review`, `seam`, `deliver`), where child gates optionally use granular `when:` conditions (e.g., `when: code` or `when: doc`).
   - **Multiple Specialized Tracks**: A profile can declare multiple independent `tasks:` blocks partitioned by condition (e.g., `- tasks: { when: code, gates: [...] }` alongside `- tasks: { when: doc, gates: [...] }`). The engine matches each admitted task to its corresponding `tasks:` block by the applicable scope condition.
3. **Multiple Gates Inside Each `tasks:`**: Inside any `tasks:` block, `gates:` defines the ordered progression for matching tasks, maintaining clear separation of concerns without deeply nested or synthetic wrapper structures.

#### Command Gate Polymorphism & Multi-Language Dispatch (`by_pattern`)
A command in the `commands:` catalog can either be a direct shell command or a multi-language pattern dispatcher:
```yaml
commands:
  check_task:
    by_pattern:
      "**/*.py": "basedpyright {scope_target} && pytest {scope_target}"
      "**/*.{ts,tsx,vue}": "npm run typecheck && npm test -- {scope_target}"
      "**/*.rs": "cargo clippy --lib --test {scope_target} && cargo test --test {scope_target} -- --nocapture"
    timeout_seconds: 60
```
- **Automatic Scope Inspection**: Forge matches files in the active task's `scope` against the glob patterns.
- **Polyglot Execution**: If files match a single pattern, that command executes. If a task touches multiple languages (e.g. both Python backend and TypeScript frontend), matching commands execute sequentially; any failure halts and triggers `fail`.
- **Task Manifest Override**: An individual task in the milestone manifest can explicitly specify `command: <catalog_id>` to override the gate default (e.g. for e2e tests or specialized linters).
- **Zero Language Hardcoding Invariant (`INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE`)**: The embedded default profile (`acdd.default.yaml`) and compiled internal profile templates must remain strictly language-agnostic. Gate definitions, step instructions, and proof validators must never hardcode language-specific binaries (such as `cargo clippy`, `cargo test`, `pytest`, or `npm`). All verification commands must use either pattern-based multi-language dispatch (`by_pattern`) or project-level configuration. Guidance texts must not mention specific languages or compilers.

#### Built-in Typed Proof Dispatch & Automated Evidence Envelope Compilation
The Forge engine dispatches proofs via built-in typed proof kinds (`ProofKind` enum: `Contract`, `Command`, `Review`, `Delivery`, `SeamTest`, `None`) with JSON Schema validation and engine-level execution verification:

- **Automatic Proof Inference Invariant (`INV-PROOF-TYPED-INFERENCE`)**:
  When `proof:` is omitted on a gate definition, the engine automatically infers the required proof kind during profile normalization strictly from typed structural fields:
  1. `expected_failure` present $\implies$ `ProofKind::Contract`
  2. `command` present $\implies$ `ProofKind::Command`
  3. `policy` or `contours` present $\implies$ `ProofKind::Review`
  4. `receipt: true` (with `role` or `need` or task-level delivery) $\implies$ `ProofKind::Delivery`
  5. Otherwise $\implies$ `ProofKind::None` (freeform execution traces, blackboard notes, platform closure)
  The engine strictly forbids string heuristics on gate IDs (`id == "contract"`, etc.); gates may be assigned arbitrary domain-specific tokens. Explicit `proof:` declarations always take precedence.

```yaml
proofs:
  contract:
    description: "Proof of runtime assertion failure (exit 101/1/134) proving red seam baseline"
    schema:
      type: object
      required: [exit_code, stderr]
      properties:
        exit_code: { type: integer }
        stderr: { type: string }
        contract_test_ref: { type: string }

  command:
    description: "Autonomous command execution proof (exit code 0, passed linters/tests)"
    schema:
      type: object
      required: [exit_code]
      properties:
        exit_code: { type: integer, const: 0 }
        tests_passed: { type: integer }
        tests_failed: { type: integer, const: 0 }

  review:
    description: "Independent cognitive review sign-off across focus lenses"
    schema:
      type: object
      required: [decision, contours]
      properties:
        decision: { type: string, enum: ["pass", "reject"] }
        contours:
          type: object
          additionalProperties:
            type: object
            required: [applicable, evidence]
            properties:
              applicable: { type: boolean }
              evidence: { type: string }

  seam_test:
    description: "Joint seam integration proof verifying all referenced cross-task tests passed"
    schema:
      type: object
      required: [exit_code, seam_test_refs]
      properties:
        exit_code: { type: integer, const: 0 }
        seam_test_refs: { type: array, items: { type: string } }
        tests_passed: { type: integer }
        tests_failed: { type: integer, const: 0 }

  delivery:
    description: "Autonomous delivery receipt and atomic Git commit proof"
    schema:
      type: object
      required: [commit_sha, receipt_id]
      properties:
        commit_sha: { type: string }
        receipt_id: { type: string }
```

**Automated Evidence Envelope Compilation**:
During gate execution, the Forge engine automatically captures runtime evidence into SQLite (`gate_execution_traces` and session context) forming an **evidence envelope**:
- `exit_code`: process return code.
- `executed_test_refs`: full list of test identifiers executed during the run.
- `tests_passed`, `tests_failed`, `tests_ignored`: execution tallies.
- `stdout_sha256`, `output_snippet` ($\le$ 2 KiB): bounded output trace.
- `touched_files`: working tree paths modified relative to the pre-snapshot.
- `candidate_tree_sha`: content-addressed Git tree SHA.
- `blackboard_signals`: active RFCs and blocker status on `task_blackboard`.

When the builder calls `task submit` with zero arguments, Forge compiles this captured envelope directly into the proof payload required by the gate's `proof:` schema.

**Strict Element-by-Element `seam_test_refs` Verification**:
For `proof: seam_test`, Forge does not merely check `tests_passed > 0` in aggregate. The engine strictly cross-references `seam_test_refs: Vec<String>` element-by-element against `executed_test_refs` captured in the trace. Every declared test reference must demonstrably have run and passed (`exit_code == 0`). If any referenced seam test was omitted, skipped, or failed, proof validation fails closed immediately.

#### Modular Catalog: Policies (`policies:`)
Gates reference typed behavioral policy sets rather than hardcoded inline flags:
```yaml
policies:
  default_review:
    independent_worker: true          # Reviewer worker_id must differ from builder
    require_all_contours: true        # Must evaluate every contour listed in guidance
    audit_extended_scope: true        # Explicit review proof required if extended_scope is present

  default_delivery:
    save:
      post: commit                    # Engine creates atomic git commit and receipt via single persistence hook
    enforce_clean_worktree: true      # Working tree must be completely clean before delivery

  strict_audit:
    independent_worker: true
    require_all_contours: true
    enforce_baseline_parity: true     # Verifies test count does not drop without observable justification
```
- **Review Policy Invariants**: Enforces cognitive independence between workers, mandates full contour evaluation, and prevents unreviewed scope creep.
- **Delivery Policy Invariants**: Enforces snapshot immutability, clean git state, and automated zero-token commit generation.

---

### Pillar 2: Two-Tier Scope Governance with Manifest Synchronization

To maintain absolute fidelity between SQLite and the Git milestone manifest:

1. **`scope: [...]` (Immutable Baseline)**:
   The original admitted file paths defined at task creation. Never mutated mid-flight.
2. **`extended_scope: [...]` (Audited Additions)**:
   Any additional file required during implementation must be added via:
   - CLI:
     ```bash
     contextunity-forge-mcp task extend-scope <task_id> <path> --reason "<justification>"
     ```
   - MCP:
     ```json
     task_manage(action: "extend_scope", task_id: "<task_id>", path: "<path>", reason: "<justification>")
     ```

   **Atomic Dual-Synchronization**: The tool updates **both** SQLite and the markdown manifest (`docs/milestones/<TAG>-<NNN>-<slug>.md`) under the task block in a single transaction.
3. **Mandatory Review Proof Verification**:
   If `extended_scope` is non-empty, `review_proof.extended_scope_review` must contain an explicit approval. The approval value has this shape:
   ```json
   {
     "approved_paths": [
       "docs/reference/tasks.md",
       "src/engine/tasks/gates.rs"
     ],
     "reason_verified": true
   }
   ```
   `approved_paths` must exactly match the task's admitted `extended_scope` set; missing, extra, or duplicate paths fail validation. `reason_verified` must be `true`.
   Advancement to `deliver` is strictly blocked if `extended_scope` lacks reviewer sign-off.
4. **Protected Paths Guard (`TASK_SCOPE_PROTECTED`)**:
   Protected system configuration and infrastructure paths (including `.forge/acdd/**`, `.forge-mcp.yaml`, and lockfiles) are strictly forbidden from mid-flight addition via `task extend-scope`. Any attempt to add a protected path fails closed with `TASK_SCOPE_PROTECTED`.

---

### Pillar 3: Unifying Checkpoints, Breadcrumbs, and SQLite Gate Tables

We completely eliminate the ad-hoc filesystem file `.forge/checkpoints.json`.
Checkpoints are now **native SQLite entities (`gate_checkpoints`) keyed directly by the turn breadcrumb**:

**Breadcrumb** = `{repository}/{project}/{milestone_id}/{gate_path}/{task_ref}/{stage_id}`

where:
- `repository` and `project`: workspace namespace components, e.g. `forge-mcp/forge-mcp`
- `milestone_id`: e.g. `AC-001-hierarchical-acdd-gates-and-milestone-orchestration`
- `gate_path`: dot-delimited path down the gate tree, e.g. `tasks` or `gates`
- `task_ref`: specific task slug (e.g. `sqlite-gate-transitions-and-trace-persistence`) or `milestone` for top-level gates
- `stage_id`: specific active leaf gate ID (e.g. `contract`, `build`, `review`, `deliver`)

1. **State Machine Cursor `(gate_path, subject)`**:
   The active position in the gate DAG is addressed by the compound cursor `(gate_path, subject)`.
2. **Automatic Lifecycle Checkpoint**:
   On every gate transition, Forge writes a snapshot into `gate_checkpoints` containing:
   - `breadcrumb`: unique composite key
   - `transition_id`: reference to `gate_transitions(id)`
   - `git_tree_sha`: immutable snapshot of the working tree
   - `state_payload`: active scratchpad, compiler logs, and changed files.
   Engine-owned checkpoint rows keep their `transition_id`; generic session checkpoint saves and deletes cannot replace or remove those rows. Milestone verification validates its stored state against that transition and the state digest in its proof.
3. **On-Demand Inspection Tool (`session_checkpoint`)**:
   Agents query `session_checkpoint(action="get", name="<full breadcrumb>")` to inspect the checkpoint at a breadcrumb without reading raw files or running manual shell checks. The name includes repository and project namespaces so shared-store rows remain isolated.

---

### Pillar 4: Command Observation and Typed Proofs

Command and contract proofs must consume engine-observed execution bound to the claim, candidate, command plan, and output policy. A runtime-red contract must prove the declared seam and failure signature; a successful build must verify the candidate. Reviewers submit typed `decision` (`pass` or `reject`) and a `contours` object mapping each configured contour to `applicable` and nonblank `evidence`, with `extended_scope_review` when needed; see the [typed proof reference](../reference/tasks.md#gates-and-evidence). `task submit` can compile evidence from traces, but Round 4 found that typed submissions can bypass observation (R4-01 and R4-02). The common acceptance path must enforce the same proof binding for all submission surfaces.

Task execution can use a selected working tree. Native plan execution, role dispatch, and headless runners are implemented. Dedicated taskless workflow run logs are not implemented.

---

### Pillar 5: Milestone Lifecycle and Verification

Milestone initialization admits tasks and performs structural preflight. The accepted root `gates:` graph specifies synthesis, the task DAG, verification, repair, independent review, and handoff. Existing root verify and handoff operations provide verification, binding, and archival primitives, but Round 4 found that their phase-driven implementation does not enforce the declared graph or mandatory observed verification (R4-03, R4-05, R4-06, R4-19). Root operations must share the admitted graph and its actual transitions before those guarantees are complete. Handoff binds the final content and target base, records evidence, archives the manifest, and prunes completed task state under its applicable checks.

---

### Pillar 6: Markdown-First Message Ergonomics (Milestone 054 Alignment)

Under the invariants established in [ADR 0019 (Markdown Contracts)](../reference/mcp-markdown-contracts.md) and [Milestone 054](../milestones/archive/054-markdown-first-dual-surface-and-agent-ergonomics.md):
1. **Tool Output Default**:
   All CLI and MCP operations default to compact GitHub Markdown. Raw JSON envelopes are only emitted when explicitly requested via `--format json` or `format: "json"`.
2. **Clickable Coordinates**:
   All file paths, symbol locations, and document anchors are formatted as absolute `file:///` URIs with exact line fragments. Lists of files use clean bullet points, never bloated markdown tables.
3. **Structured Alert Blocks**:
   - `> [!IMPORTANT] Governing Invariants`: displays governing rules and active contracts.
   - `> [!CAUTION] BLOCKER (#id)`: emitted immediately with execution duration, exit code, breadcrumb, and failure snippets whenever a check fails or scope is violated.
4. **Execution Budgets & Fast Seams**:
   `workflow_guidance` provides the exact fast seam test target (inner loop ~2s), leaving heavy whole-repo checks to autonomous command gates.

---

### Pillar 7: Modular Subsystem Decoupling & Adaptive Guidance

To prevent token waste, reduce memory footprints, and tailor Forge MCP for specialized agent roles (e.g. pure code exploration vs umbrella orchestration), the platform decomposes into **5 decoupled subsystems**:

```yaml
# forge-mcp.yaml
profile: full         # Presets: full | navigator | orchestrator | minimal

modules:
  codemap: true       # 11 tools: code_map_* and ast_grep_search (Tree-sitter, code_map.sqlite)
  acdd: true          # 4 tools: task_claim, task_submit, task_manage, task_list (tasks.sqlite, gates)
  blackboard: false   # 1 tool: task_blackboard (structured message board)
  docs: true          # 2 tools: search_docs, get_doc (Markdown FTS5 index)
  checkpoints: true   # 1 tool: session_checkpoint (SQLite gate_checkpoints)
```

#### Presets Matrix

| Preset | Enabled Modules | Primary Role & Benefits |
| :--- | :--- | :--- |
| **`full`** | `codemap`, `acdd`, `blackboard`, `docs`, `checkpoints` | Complete dual-surface platform for end-to-end milestone development. |
| **`navigator`** | `codemap`, `docs` | Pure code exploration & architectural search. Saves ~1500 tokens of task tool schemas; zero task-store database overhead. |
| **`orchestrator`** | `acdd`, `blackboard`, `checkpoints` | Pure task & milestone lifecycle orchestrator. Eliminates Tree-sitter indexing, saving ~3000 tokens of code-navigation schemas and ~500MB RAM. |
| **`minimal`** | `forge_guide` only | Minimal diagnostic surface with zero background databases. |

#### Comprehensive Systems Impact Analysis

1. **Dynamic MCP Tool Filtering (`tools/list`)**:
   - The MCP server filters registered tool schemas dynamically against the active workspace's enabled modules.
   - Calling a disabled tool fails closed immediately: `FEATURE_DISABLED: module '<module>' is disabled in forge-mcp.yaml. Do: enable it in configuration or adjust active profile.`
   - Eliminates 1500 to 3000 tokens of irrelevant JSON schemas from every single LLM prompt.

2. **Adaptive Workflow Guidance (`workflow_guidance`)**:
   - `task_claim` and `task_manage(action="context")` dynamically inspect active modules:
     - **When `blackboard: false`**: `workflow_guidance.blackboard_messages` is cleanly suppressed (`null`), and prompt rules omit instructions to post to `task_blackboard`.
     - **When `codemap: false`**: `covering_tests`, `external_callers`, and `seam_graph` are omitted gracefully rather than failing on missing `code_map.sqlite`.
     - Prompt instructions never suggest or reference disabled tools.

3. **Portable ACDD Gate Execution without `codemap`**:
   - Traditionally, the `contract` gate performed AST reachability checks (`capture_baseline_boundary_callers`).
   - With modular decoupling, if `codemap: false`, ACDD gracefully switches to **Pure Execution Trace Verification** via the Seam Engine:
     - The contract author writes a seam test.
     - Forge validates the runtime transition: Red assertion failure (matching configured `expected_failure.exit_codes`) $\to$ Green implementation pass (exit 0) using `gate_execution_traces`.
     - This allows running ACDD workflows on **any arbitrary tech stack or language**, even without Tree-sitter parsers or code index databases!

4. **Review Gate Fallback**:
   - Review contours that normally traverse the code graph (`symbol_parity`, `impact_radius`) automatically fall back to Git diff inspection when `codemap` is disabled, ensuring cognitive reviews never stall due to absent index databases.

5. **Tool Discovery (`forge_guide`)**:
   - `forge_guide` dynamically prunes disabled tools from its sitemap and recommendations, providing only currently active operational capabilities.

6. **Fail-Closed Profile Governance (`FEATURE_REQUIRED_BY_PROFILE`)**:
   - If a specific milestone profile explicitly declares a tool requirement (e.g. `gate.contract.tools: [code_map_inspect]`), but that module is disabled in `forge-mcp.yaml`, `milestone init` and `task claim` fail fast with `FEATURE_REQUIRED_BY_PROFILE`, preventing agents from attempting tasks without required capabilities.

---

### Pillar 8: Upfront Contract Synthesis, DAG Optimization & Topological Priority Waves (Task 0 Pattern)

To avoid circular dependencies, type drift between parallel builder sessions, and overlapping scope edits, milestone orchestration enforces the **Upfront Contract Synthesis Pattern** driven by an active Architect agent:

1. **Task 0: Bootstrap, Impact Analysis & Dynamic DAG Optimization**:
   - Every complex multi-task milestone begins with **Task 0: Upfront Contract Synthesis & DAG Prioritization**.
   - Rather than executing tasks in an arbitrary or linear developer sequence, the Architect agent:
     - Accepts raw task objectives and linked milestones (`linked_to`).
     - Queries codebase impact radius via `code_map_impact` and Tree-sitter AST to identify potential file overlap across tasks.
     - **Reshuffles and DAG-optimizes tasks into deterministic parallel waves (`Wave 0`, `Wave 1`, `Wave 2...`)**, setting fine-grained `task_depends_on` blockers across linked milestones.
     - Synthesizes the global cross-task data contract matrix, standardizes shared error variants, and freezes disjoint file scopes upfront.
     - **Safety Net**: Uses `save.pre: snapshot` to capture a pristine restore point of the manifest and repository before any rewrite, and `save.post: commit` once the optimized plan is proven.

2. **Deterministic Topological Execution Waves**:
   Tasks are grouped into strictly ordered execution waves:
   - **Wave 0 (Bootstrap)**: Contract synthesis, interface contracts, error taxonomy, and scope boundary freezing (Task 0).
   - **Wave 1 (Foundation & Disjoint Execution)**: Tasks with guaranteed mutually disjoint file scopes (`disjoint scopes`), enabling multiple builder subagents to execute concurrently in parallel worktrees without Git merge conflicts.
   - **Wave 2 (Engine & Runner)**: Zero-argument transition runner, automated synthesis gate, and Seam Engine Red-to-Green verifier (`src/engine/`).
   - **Wave 3 (Runtime, Consumers & Tooling)**: Downstream consumers, transition runner, in-place execution runner, atomic scope extension, and CLI commands (`src/cli/`).

3. **Task 0 Contract Matrix, Deterministic Waves & Scope Freeze**:
   - Contract rows point from provider to consumer. A local task dependency from consumer to provider requires a seam_dependency matrix row with the reverse, provider-to-consumer direction.
   - The shared core models are ContractType (shared_helper, seam_dependency, shared_type), ContractStatus (synthesized, verified), MilestoneContractEntry, [MilestoneContractMatrix](../../crates/forge-tasks/src/tasks/mod.rs), [AcddErrorCode](../../crates/forge-tasks/src/tasks/mod.rs), and AcddError. Each entry uses the milestone_contract_matrix columns, requires a non-empty contract_ref and positive revision, and is unique by milestone_id, source_task, target_task, and contract_ref.
   - Task 2 exposes TasksStore::replace_contract_matrix for an atomic full-matrix replacement and TasksStore::list_contract_matrix for deterministic reads ordered by source_task, target_task, and contract_ref.
   - [verify_contract_matrix_parity](../../crates/forge-tasks/src/milestones/mod.rs) fails closed with CONTRACT_MATRIX_MISMATCH when the matrix belongs to a different milestone, references unknown tasks, repeats a row key, contains an empty reference or non-positive revision, or omits a local manifest dependency. Extra shared_helper and shared_type entries are allowed.
   - AC-001 uses these deterministic topological waves; milestone task order breaks ties: Wave 0, Task 0; Wave 1, Tasks 1 and 2; Wave 2, Tasks 3 and 4; Wave 3, Tasks 5 and 6. The manifest depends_on graph remains the execution authority.
   - Same-wave scopes are disjoint: Task 1 owns core/profile/CLI/MCP/engine schema work and gates_ast.rs while Task 2 owns src/db/tasks_store/ and persistence.rs; Task 3 owns transition/command-runner work and runner.rs while Task 4 owns synthesis/milestone work and synthesis.rs; Task 5 owns claim/container and reader work with seam_reachability.rs while Task 6 owns scope-sync, handoff, CLI/MCP and governance files with scope_sync.rs. Cross-wave path reuse is serialized by dependencies.

4. **Shared Error Taxonomy**:
   AC-001 uses one stable, machine-readable diagnostic set in core, the profile, and operator-facing errors. AcddErrorCode::as_str() defines the canonical spelling; profile keys must match exactly.
   - GateProfile.error_codes is the profile representation. It must contain the same identifiers as AcddErrorCode and reject unknown identifiers during profile validation.

   | Code | Meaning |
   | :--- | :--- |
   | CONTRACT_MATRIX_MISMATCH | A dependency or matrix entry does not match the milestone contract. |
   | DEPENDENCY_MUTATED | An upstream dependency changed after the task contract was admitted. |
   | FEATURE_DISABLED | A requested Forge subsystem is disabled. |
   | FEATURE_REQUIRED_BY_PROFILE | The active profile requires a disabled subsystem. |
   | GATE_INVALID_CONFIGURATION | A gate has missing, conflicting, or invalid driver fields. |
   | MILESTONE_DAG_CYCLE | The task dependency graph contains a cycle. |
   | MILESTONE_PREFLIGHT_FAILED | Manifest, skill, repository, or worktree preflight failed. |
   | TASK_CANDIDATE_MISMATCH | The delivery tree differs from the reviewed immutable candidate. |
   | TASK_CANDIDATE_OUT_OF_SCOPE | The task candidate changes a path outside admitted scope. |
   | TASK_SCOPE_INVALID | A scope path is malformed or outside declared roots. |
   | TASK_SCOPE_PROTECTED | A protected path cannot be added to task scope. |
   | TASK_SEAM_DISCONNECTED | No current, proven test-to-production path reaches the target. |
   | TASK_SEAM_STALE | Source drift invalidated indexed seam evidence; refresh the index. |
   | UNADMITTED_SYMBOL_EXPANSION | A public symbol change was not admitted at the contract gate. |

5. **Disjoint Scope Boundaries & Speculative Stacked Execution**:
   - **Disjoint Scope Foundation**: Parallel tasks within the same wave (e.g. Task 1 in `src/core/` and Task 2 in `src/db/`) are guaranteed by the Architect to have disjoint file paths, eliminating git merge conflicts across parallel builder sessions.
   - **Speculative Concurrency (`mode: speculative`, `sync_on: snapshot`)**:
     Dependent tasks across waves (or even within a wave) do NOT have to wait for upstream tasks to reach terminal `deliver`. When declared with `mode: speculative`:
     - Downstream tasks can claim and begin work immediately in parallel worktrees against the Architect's synthesized contract.
     - **Snapshot-Driven Sync Loop**: As upstream tasks progress and generate in-flight keeper snapshots (`refs/forge/state/<milestone>/<task_slug>`), the downstream runner automatically pulls and rebases on the latest snapshot before gate verification or check commands.
     - **Autonomous Semantic Conflict Resolution**: If merge conflicts occur on shared files, the downstream builder agent resolves them autonomously using semantic AST understanding and verifies correctness against seam tests.
     - **Contract RFC Negotiation**: When scope drift or parameter changes occur, agents do not block synchronously; they negotiate modifications asynchronously via Pillar 9's Signal Bus / Blackboard (`to: "task:<upstream_slug>"`, `topic: "contract:rfc"`), while continuing unblocked work on non-conflicting components.

4. **Coordinated Seam Integration & Delivery Barrier (`when: seam`)**:
   - **Non-Speculative Delivery Barrier Deadlock Resolution**:
     The coordinated delivery barrier applies strictly to concurrent or speculative tasks on a shared seam (`mode: speculative` or intra-wave parallel tasks). In contrast, strictly sequential dependencies (Task A blocking Task B where Task B cannot start until A delivers) do NOT deadlock: Task A delivers immediately once its own individual `review` passes, unblocking Task B without waiting for B.
   - **Task `type:` Model & Universal `when:` Condition Model**:
     Tasks in the manifest declare an optional `type: [code | doc | db | frontend | api | observer | ...]` (or shorthand `type: doc`, `type: code`), defaulting strictly to `type: ["code"]` when omitted (ensuring 0 boilerplate for 95% of standard code tasks).
     `when:` conditions evaluate against the admitted task scope and concurrency:
     - `when: code`: evaluates to `true` if any scoped path is not documentation.
     - `when: doc`: evaluates to `true` if scope is nonempty and every path is documentation.
     - `when: db_migration`: matches scoped SQL files or paths in a migrations directory.
     - `when: seam`: evaluates to `true` for Speculative or Parallel concurrency.
   - **Conditional Child Gates within `seam`**:
     Inside the `seam` container, child gates specify their own conditions:
     - `integrate`: specifies `when: code`. Code tasks execute `check_task` and validate `seam_test_refs`. Non-code tasks (such as `doc` or `observer` tasks) skip test compilation and proceed directly to documentation review, seam event logging, or telemetry dispatch.
     - `review`: evaluates the cross-task changes (code changes, doc sync, or contract alignment).
   - **Targeted `when:` Condition Catalog**:
     To prevent arbitrary procedural scripting while addressing real production workflows without Ansible-style complexity, Forge supports a closed set of typed conditions in `GateCondition`:
     - `when: seam`: Speculative or Parallel concurrency.
     - `when: code`: Triggers when any scoped path is not documentation.
     - `when: doc`: Triggers when all scoped paths are documentation and scope is nonempty.
     - `when: db_migration`: Triggers for scoped SQL files or paths in a migrations directory.
     - `when: public_api`: Triggers when AST analysis detects a public callable export in task scope.
   - **Fail-Closed Migration Verification**:
     `verify_migration` must never contain loose `**/*: echo 'No migration verification required'` fallbacks. If a task touches migrations, verification commands must be explicitly configured or fail closed.
   - **Milestone-Level SemVer Bumping**:
     SemVer version incrementation (`api_version_bump`) and release notes generation are moved out of per-task gates and placed into the milestone integration pipeline (after verification before handoff), avoiding per-task candidate immutability conflicts and version churn.
   - **`seam.integrate` (`proof: seam_test`)**:
     - When `when: seam` activates on a code task, the builder executes `seam.integrate` using guidance `task.deferred_final_test` (loading `test-suite-refactor` and `test-contract-seam`).
     - The agent audits test reachability across task boundaries, replaces synthetic mocks with real production callers, and cleans up duplicate stubs.
     - Submits typed `SeamTestProof` containing:
       - `seam_test_refs: Vec<String>`: array of test references covering the reconciled seam.
       - `command: String`: registered verification command (`check_task`).
       - `exit_code: 0`: strictly zero exit code proving the seam passes.
       - `tests_passed: u64` (> 0) and `tests_failed: 0`.
     - The engine validates element-by-element that every single test reference in `seam_test_refs` actually executed and passed in the captured trace.
   - **`seam.review` (`proof: review`)**:
     - An independent reviewer (`independent_from: seam.integrate`) inspects the reconciled seam diff, verifying that stubs and mocks were safely eliminated without dropping production assertions or introducing regressions.
     - If defects are found, `seam.review` rejects back to `seam.integrate` (`fail: integrate`) or `build`.
   - **Simultaneous `deliver`**:
     - Once both individual `review` and joint `seam.review` pass across the dependency contour, Forge opens the coordinated delivery barrier for speculative/parallel tasks, landing candidate snapshots atomically into the milestone branch HEAD.

---

### Pillar 9: Unified Operational Blackboard & Contract RFC Coordination

Analysis of production multi-agent sessions revealed that separating messaging into multiple tables creates state desynchronization. Operational signals, RFCs, and targeted coordination live unified within `task_blackboard`:

1. **Targeted Addressing & Semantic Topics (`task_blackboard`)**:
   - `task_blackboard` is extended with `recipient_task_id TEXT` and `status TEXT DEFAULT 'active'`.
   - Messages are routed by topic:
     - `contract:rfc`: Interface proposals between speculative or coupled tasks. When an owner accepts an RFC, Forge automatically increments `milestone_contract_matrix.revision`, which reopens dependent downstream gates for contract verification.
     - `blocker`: Transient blocker notification addressed to a specific task (`recipient_task_id: "core-formatter"`).
     - `info`: Broadcast notes across the milestone (`recipient_task_id IS NULL`).
2. **Zero-Token Workflow Guidance Injection**:
   - On `task claim` or before gate transitions, unread blackboard messages targeted to `recipient_task_id == self` or `topic == 'contract:rfc'` are automatically injected into `workflow_guidance`, ensuring builder agents are immediately aware of contract mutations or upstream inquiries without polling.
3. **Lifecycle Pruning at Milestone Handoff**:
   - Upon milestone handoff (`INV-HANDOFF-SQLITE-PRUNING`), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; `milestone_tombstones` is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in `docs/milestones/archive/*.md` and Git history.

---

### Pillar 10: Task List Scoping, 96 KiB Response Budget & Graceful Boundary Truncation

To eliminate the recurring `Output exceeds the response byte limit` failures identified during multi-agent benchmark analysis, the response transport and task listing subsystems are overhauled:

1. **Response Budget of 96 KiB (`MAX_OUTPUT_BYTES = 96 * 1024`)**:
   - The transport ceiling is 96 KiB (98,304 bytes), preserving bounded MCP responses under ADR 0011.
   - Provides safe, verified headroom for multi-file graph listings, large integration test bodies, and complete task trees without triggering client-side truncation or stdio drops.

2. **Context-Aware `task_list` Scoping & Multi-Milestone Aggregation**:
   - **Worktree-Aware Default**: When called within a registered milestone worktree (`.worktrees/<id>`), `task_list` automatically scopes `milestone_ref` exclusively to that worktree's milestone by default, preventing cross-milestone pollution.
   - **Multi-Active Milestone Default at Root**: When called from the canonical server root (`main`) with `milestone_status="active"` (the default), `task_list` returns tasks across **all currently open/active milestones** (e.g. both `AC-001` and `UX-001`), cleanly grouped under milestone section headers (`## Milestone: AC-001`, `## Milestone: UX-001`).
   - **Milestone Linkage vs Task-Level Dependencies**:
     - **Milestone-Level Linkage (`linked_to: [milestones...]`)**: Milestones declare logical association or coordination contours (e.g., `linked_to: ["UX-001"]`), without coarse-grained whole-milestone serialization blocks.
     - **Task-Level Cross-Milestone Dependencies (`task_depends_on: [milestone:task_slug]`)**: Fine-grained dependencies are declared strictly at the task level (e.g., `task_depends_on: ["UX-001:byte-bounded-single-symbol-snippets"]`). A task only blocks on the exact prerequisite tasks it actually relies upon.
     - **Linked Milestone Dependency Traversal (`include_dependencies=true`)**: When querying `task_list`, setting `include_dependencies: true` automatically traverses `linked_to` and `task_depends_on` to pull in active prerequisite tasks from linked milestones, displaying their real-time execution status (`**Blocked by:** <upstream_milestone>:<upstream_task> (<status>)`).
     - **Reliable Agent Guidance & Context Injection**: When an agent claims a task or requests context in a milestone with `linked_to` entries, Forge's `workflow_guidance` automatically injects a dedicated guidance section displaying linked milestones, their current lifecycle state, and a focused status table of blocker tasks, instructing the agent on whether to proceed, wait on an unblock signal, or integrate upstream receipts.
   - **Targeted Multi-Milestone Selection**: `milestones: ["AC-001", "UX-001"]` allows coordinators to query exactly the subset of coupled milestones they are actively orchestrating.
   - **Archived Milestone Exclusion**: Archived milestones (under `docs/milestones/archive/` or with `status: archived`) are strictly excluded from active task queries.

3. **Milestone Handoff State Purging**:
   - Upon milestone handoff (`INV-HANDOFF-SQLITE-PRUNING`), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; `milestone_tombstones` is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in `docs/milestones/archive/*.md` and Git history.

4. **Task List Presentation & Subtask Checkboxes**:
   - Clean, readable Markdown layout displaying:
     - Task headings and current status: `### [task: <slug>] (<status>)`
     - Explicit dependency chains: `**Depends on:** <task_refs>`
     - Subtasks rendered with interactive Markdown checkboxes:
       - `- [x] <subtask_ref>` (completed)
       - `- [ ] <subtask_ref>` (in progress / planned)

5. **CLI Parity & Status Filtering**:
   - Direct status filters mirroring CLI options:
     - `active`: In-progress tasks.
     - `planned`: Ready / planned tasks.
     - `completed`: Finished tasks in open milestones.
     - `all`: All tasks within the targeted milestone scope.
   - Native pagination: `limit` (default: 30) and `offset` (default: 0).

6. **Graceful Boundary Truncation (Zero-Crash Fail-Safe)**:
   - Forge MCP **never fails closed** with `Output exceeds the response byte limit` on collection queries.
   - When rendered output approaches the 96 KiB boundary (~85 KiB), Forge truncates cleanly on a **complete task or subtask boundary** (never slicing mid-sentence or mid-JSON).
   - Appends an actionable pagination block with ready-to-run queries:
     ```markdown
     > [!NOTE] Showing 1..30 of 85 tasks. Output truncated to fit 96 KiB budget.
     > Next page: `task_list(offset=30, limit=30)` | Filter active: `task_list(status="active")`
     ```

---

### Pillar 11: Unified Persistence Policy (`save:`) & Overflow-Guarded Git Commit Notes

To decouple snapshots, commits, and immutability from rigid hardcoded stage names (e.g. `sha_snapshot` only on `build` and `auto_commit` only on `deliver`), gates declare a unified, composable `save` schema with built-in `git_commit_note` and explicit payload overflow limits:

```yaml
# Global or profile-level defaults:
git_commit_note:
  limits:
    max_bytes: 2048          # Hard overflow limit (default 2048 bytes / 2 KiB)

save:
  pre: snapshot              # snapshot | commit (omitted = None). Captured on claim/entry.
  post: commit               # snapshot | commit (omitted = None). Captured/landed on pass/exit.
  immutable: true            # bool (default false). Enforces zero working tree deviation from snapshot.
  git_commit_note: command.benchmark # Shorthand: attaches structured output of configured command to refs/notes/forge
```

Or fully specified with per-gate limit override:

```yaml
save:
  post: commit
  git_commit_note:
    command: benchmark       # Named command from commands catalog whose structured output is captured
    limits:
      max_bytes: 2048        # Hard overflow limit (default 2048 bytes / 2 KiB)
```

#### Core Semantics:
1. **`snapshot` (Moving Keeper Git Ref)**:
   - Captures a pure parentless content-addressed tree snapshot (`git commit-tree` without `-p`), protected from `git gc` by a moving keeper ref (`refs/forge/state/<milestone>/<task_slug>`).
   - Completely lightweight, zero-token restore point that preserves working tree state without cluttering public Git history, deleted at milestone handoff.
2. **`commit` (Branch HEAD)**:
   - Commits reviewed and accepted changes directly into the active milestone branch HEAD with a standardized `forge-task:` commit message. This is the single task commit mechanism.
3. **`pre` vs `post` Lifecycle Hooks**:
   - `pre`: Runs before agent begins editing (on `task claim`). Essential for the Architect or major refactoring tasks to preserve a pristine baseline before rewriting documents.
   - `post`: Runs when the stage passes (`task submit --action pass`), locking in verified changes.
4. **`immutable: true`**:
   - Replaces ad-hoc candidate baseline checks. When set (e.g. during `review` or `deliver`), Forge verifies that working tree files match the active candidate snapshot. Any modification fails closed with `TASK_CANDIDATE_MISMATCH`.
5. **`git_commit_note`: Full Automated Telemetry Passport (Markdown-First)**:
   - Git commit notes serve as an **automated engineering passport** for every delivered task, computed in milliseconds by Forge directly from Git diffs, AST parsing, and SQLite timestamps without requiring any manual flags or agent inputs:
     ```markdown
     ### Forge Telemetry
     - meta:
       - task: sqlite-handoff-pruning-and-tombstone-migration
       - duration: 3m 45s (attempts: 2, repair_cycles: 1)
       - subtasks: 3/3 (100% completed)
       - scope: 2/2 files touched (extended_scope: 0)

     - diff:
       - src: +48 / -12 (3 files: 1 created, 2 modified)
       - tests: +92 / -0 (test/code ratio: 1.9x)
       - docs: +15 / -4 (docs/adr/0020-*.md, docs/reference/tasks.md)

     - ast_symbols:
       - public_api: +2 pub fn, +1 pub struct
       - internal: +4 fn, -1 enum variant, ~3 modified methods
       - breaking_changes: 0

     - verification:
       - seam_target: tests/tasks_store/migration.rs::test_v6_tombstones
       - test_suite: 42 passed, 0 failed, 0 ignored (run_time: 840ms)
       - flakiness: 0 flaky tests
       - linter: clean (0 warnings, 4 auto-fixed in cycle 1)

     - swarm_concurrency:
       - upstream_dependency: task-0:contract
       - downstream_dependents: [task-2, task-3]
       - upstream_rebases: 2 snapshots synced
       - merge_conflicts: 1 auto-resolved (src/db/tasks_store/mod.rs)
       - blackboard: 1 rfc sent, 0 blockers
     ```
   - **Primary Use Cases**:
     1. Automated task telemetry passport (default on delivery commits).
     2. Performance benchmarks and resource budgets (cold start, throughput, binary size) when configured via `git_commit_note: command.<id>`.
   - **Explicit Overflow Guard (`git_commit_note: limits: max_bytes: 2048`)**: Note payloads are strictly bounded by `max_bytes` (default `2048` bytes / 2 KiB). The complete telemetry passport comfortably fits within ~750 bytes. If a custom command output exceeds 2048 bytes, Forge writes a compact in-budget Markdown fallback (`command_id`, `status: exceeded_budget`, `original_bytes`, `sha256`), never writing broken/sliced JSON.
   - Anyone inspecting the repository can cleanly view notes without noise: `git log --show-notes=forge` or `git notes --ref=forge show <commit>`.
6. **Lifecycle Pruning & Telemetry at Milestone Handoff**:
   - Upon milestone handoff (`INV-HANDOFF-SQLITE-PRUNING`), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite.
   - The markdown archive (`docs/milestones/archive/*.md`) and Git commits are the permanent, durable ground truth.
   - `milestone_tombstones` in SQLite serves as a fast local cache to satisfy cross-milestone dependencies without zombie tasks; safe eviction occurs only when discovery across all linked worktrees (`git worktree list --porcelain`) proves no active manifest remains for that milestone and the archive contains a verified handoff proof token.
   - Milestone handoff absorbs working tree polish directly into the integration commit, runs project verification if code was altered, and attaches a comprehensive milestone telemetry passport (total duration, completed tasks with per-task elapsed time, total review cycles/rejections, diff breakdown across `src`/`tests`/`docs`, AST symbol deltas, test suite results, generic clean linter status, and resolved upstream/downstream topology).

#### On-Demand Safety Net (`task_manage`)
In addition to gate declarations, agents can manually snapshot and rollback at any point during complex turns:
- `task_manage(action: "snapshot", name: "before_dag_restructure")`: Creates an immediate named shadow snapshot.
- `task_manage(action: "rollback", name: "before_dag_restructure")`: Restores the working tree cleanly to that named snapshot if an experiment fails.

---

### Pillar 12: Unified Fail-Fast Schema Validator CLI (`validate`)

To eliminate runtime crashes during `task claim` or `task sync` caused by malformed profile YAML, circular milestone dependencies, or corrupted plan manifests, Forge introduces a dedicated top-level CLI command:
`contextunity-forge-mcp validate [PATH]`

#### 1. Document Auto-Detection & Targeted Inspection
- **Profile Validation (`validate <path_to_profile.yaml>`)**:
  - Parses profile AST and validates against the hierarchical gate schema.
  - Verifies reachability of all `fail` and `independent_from` gate path references.
  - Checks that all referenced roles, commands, guidance keys, and review contours are defined in their respective catalogs.
  - Validates `when:` conditions against the closed typed catalog (`code`, `doc`, `db_migration`, `seam`, `public_api`). Normalized aliases: `docs` -> `doc`, `db` -> `db_migration`.
- **Milestone Manifest Validation (`validate docs/milestones/<slug>.md`)**:
  - Validates YAML frontmatter, `id`, `invariants`, `owners`, and `depends_on`.
  - Parses `# Tasks` definitions, verifying disjoint write scopes and valid proof policies.
  - Constructs the in-memory execution DAG and asserts strict acyclicity (detects cycles before task initialization).
- **Plan Document Validation (Deferred)**:
  - The planned validator would validate `doc_type: plan` frontmatter, step declarations, and container references. The `plan` commands validate their input separately.
- **Whole-Workspace Scan (`validate` with no arguments)**:
  - Scans active profile and all open milestone manifests across the repository in under 100 ms (plan document scanning is not implemented).
  - Emits machine-readable coordinates with line, column, and diagnostic code (`PASS` / `FAIL`), enabling agents to pre-flight any document modification before initiating workflow execution.

---

## SQLite Storage Architecture: `gate_transitions`, `gate_execution_traces`, `gate_checkpoints`, `milestone_contract_matrix`, `task_blackboard`

All tables follow strict plural naming matching repository invariants:

```sql
-- 1. Full audit history of gate advancements and rejections
CREATE TABLE gate_transitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    milestone_id TEXT NOT NULL,
    subject TEXT NOT NULL,            -- e.g. "task:core-formatter" or "milestone"
    gate_path TEXT NOT NULL,          -- e.g. "tasks.build"
    action TEXT NOT NULL,             -- "advance" | "reject" | "reopen"
    proof_kind TEXT NOT NULL,         -- "none" | "contract" | "command" | "review" | "delivery" | "seam_test"
    proof_payload JSON NOT NULL,      -- typed proof payload or failure metadata
    git_tree_sha TEXT NOT NULL,
    worker_id TEXT NOT NULL,          -- "codex-sol-builder", "codex-sol-reviewer", "forge:command"
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 2. Trace of all command executions associated with gates
CREATE TABLE gate_execution_traces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    milestone_id TEXT NOT NULL,
    subject TEXT NOT NULL,            -- e.g. "task:core-formatter" or "milestone"
    gate_path TEXT NOT NULL,          -- e.g. "tasks.build"
    attempt_id INTEGER NOT NULL DEFAULT 1,
    command_id TEXT NOT NULL,         -- Identifier of the executed command (supports polyglot by_pattern execution)
    command_str TEXT NOT NULL,
    status TEXT NOT NULL,             -- "completed" | "timeout" | "spawn_error"
    exit_code INTEGER,                -- Exit code or NULL on timeout/spawn_error
    log_path TEXT,                    -- Optional link to on-disk execution log (NULL for clean exit 0 runs)
    output_snippet TEXT,              -- Bounded snippet (<= 2 KiB) of stdout/stderr for instant blocker alert
    stdout_sha256 TEXT,               -- SHA-256 of stdout (NULL on spawn_error)
    duration_ms INTEGER NOT NULL,
    executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(milestone_id, subject, gate_path, attempt_id, command_id)
);

-- 3. Checkpoints keyed by turn breadcrumb
CREATE TABLE gate_checkpoints (
    breadcrumb TEXT PRIMARY KEY,      -- e.g. "forge-mcp/forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration/tasks/container-gate-and-claim-engine/build"
    milestone_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    gate_path TEXT NOT NULL,
    transition_id INTEGER REFERENCES gate_transitions(id),
    git_tree_sha TEXT NOT NULL,
    state_payload JSON NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 4. Cross-task dependency & shared helper contract matrix
CREATE TABLE milestone_contract_matrix (
    milestone_id TEXT NOT NULL,
    source_task TEXT NOT NULL,
    target_task TEXT NOT NULL,
    contract_type TEXT NOT NULL,       -- "shared_helper" | "seam_dependency" | "shared_type"
    contract_ref TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,              -- "synthesized" | "verified"
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (milestone_id, source_task, target_task, contract_ref)
);

-- 5. Unified task_blackboard extensions for targeted messaging and RFCs
-- Extends existing task_blackboard table with recipient addressing and status tracking, eliminating redundant signal tables:
-- ALTER TABLE task_blackboard ADD COLUMN recipient_task_id TEXT;
-- ALTER TABLE task_blackboard ADD COLUMN status TEXT DEFAULT 'active';
CREATE INDEX IF NOT EXISTS task_blackboard_recipient_idx ON task_blackboard (milestone_ref, recipient_task_id, status);
```

---

### Pillar 13: Two-Loop ACDD Governance: `symbols:`, `scope:`, and `loop:` (Anti-Drift Confinement)

To eliminate unadmitted architectural improvisation by builder agents while avoiding human micromanagement, ACDD formalizes the **Two-Loop Execution Model**:

```
┌────────────────────────────────────────────────────────────────────────┐
│                   FAST INNER LOOP (Builder Iteration)                  │
│                                                                        │
│   ┌───────────────────────────┐         ┌──────────────────────────┐   │
│   │           build           │ ──────► │          review          │   │
│   │  (builder + check_task)   │         │        (reviewer)        │   │
│   └───────────────────────────┘         └────────────┬─────────────┘   │
│                 ▲                                    │                 │
│                 └─────────── [scope / test defect] ──┘                 │
└────────────────────────────────────────────────────────────────────────┘
                                                       │
                                            [symbols violation]
                                                       │
┌──────────────────────────────────────────────────────▼─────────────────┐
│                 LONG OUTER LOOP (Architectural Governance)             │
│                                                                        │
│   ┌─────────────┐       ┌─────────────┐       ┌─────────────┐          │
│   │  architect  │ ────► │  contract   │ ────► │  (dispatch  │          │
│   │ (arbitrate) │       │ (seam test) │       │ to builder) │          │
│   └─────────────┘       └─────────────┘       └─────────────┘          │
│   • decides A/B         • enforces Red test   • builder runs           │
│   • extended_symbols      on new interface      build gate             │
│   • revision bump                                                      │
└────────────────────────────────────────────────────────────────────────┘
```

#### 1. Symmetrical Boundaries: `scope:` vs `symbols:`
- **`scope:` (Files on disk)**: Declares physical files mutable by the task. The builder extends files autonomously via `task extend-scope` (checked for sibling locks and protected system paths).
- **`symbols:` (Public Interface Delta & Universal Format)**: Declares the public symbols (types, functions, traits, methods, schema entities) that the task is admitted to **create (new)** or **modify (public signature changes)**.
  - **Universal Format (`kind:identifier`)**: To maintain complete language and domain agnosticism across polyglot codebases, databases, APIs, and tooling, symbols are declared using typed prefixes:
    - Code structures: `struct:Config`, `fn:execute_gate`, `trait:GateRunner`, `type:Breadcrumb`, `class:TaskRunner`, `interface:IGate`
    - Persistence schemas: `table:gate_transitions`, `column:recipient_task_id`, `index:task_blackboard_recipient_idx`
    - API endpoints: `route:POST /api/v1/tasks/claim`, `schema:TaskClaimResponse`
    - CLI & tooling: `cmd:task extend-scope`, `flag:--reason`
    - Architecture & config: `catalog:policies`, `slot:hitl`
    - Unprefixed identifiers default to semantic code-symbol matching within the target file scope.
  - Builder is strictly prohibited from creating unadmitted public symbols or altering unlisted public signatures (`UNADMITTED_SYMBOL_EXPANSION`).
  - Private helpers, local variables, and unexported module details inside scoped files remain fully unconstrained for builder agility.

#### 2. Declarative Review Reject Routing (`loop:`)
In gate definitions (e.g. `review`), reject rewind is declared concisely via `loop:`:
```yaml
loop:
  scope: build         # Target gate ID for unadmitted files or implementation defects
  symbols: contract    # Target gate ID for interface mutations or architectural drift
```
- By default, standard verification failures (test assertions, compilation errors, linter warnings) route to the build gate (`build`, role `builder`).
- Scope boundary violations route to the build gate (`build`) to either revert unneeded file modifications or run `task extend-scope`.
- Interface/symbol violations bypass the builder and route directly to the contract gate (`contract`, role `architect`). The builder cannot self-admit new architectural types; the architect must arbitrate, update `symbols:`, and produce a runtime-red contract seam proof.

#### 3. Human-In-the-Loop Governance: Guidance Slot Binding (`guidance.hitl`)
To avoid rigid hardcoding in workflow profiles while eliminating ad-hoc bespoke manifest fields, ACDD establishes the **Guidance Slot Binding** architecture:

1. **Parameterized Profile Slots**: Gates in the profile declare generic guidance slots that require milestone-level binding:
   ```yaml
   gates:
     - id: contract
       role: architect
       guidance:
         - task.contract    # Static concrete guidance
         - hitl             # Parameterized guidance slot
   ```

2. **Milestone Frontmatter Binding**: The milestone binds active slots directly under the standard `guidance:` block:
   ```yaml
   guidance:
     hitl: pro             # Binds slot 'hitl' with tier 'pro' -> resolves to catalog key 'hitl.pro'
   ```
   *Resolution Rule*: The engine resolves `format!("{}.{}", slot, value)` (`hitl.pro`). If the value already contains a dot (e.g. custom key `security: security.pci_dss`), it resolves directly. Any domain or governance contour can be extracted as a slot without engine modifications.

3. **Validation & Default Fallback**: During milestone initialization (`milestone init`) and verification (`forge validate`), the validator cross-references slots declared across profile gates against the milestone manifest:
   - If a required slot is unbound in the manifest, the engine emits `WARNING [ACDD-SLOT-UNBOUND]: Gate 'contract' requires guidance slot 'hitl', using profile default 'hitl.pro'.`
   - During `task claim`, the engine injects *strictly that single resolved guidance entry* into `workflow_guidance`.

4. **Detailed HITL Governance Rules**:
   The `guidance:` catalog provides detailed, unambiguous behavioral boundaries for each escalation tier:

   - **`hitl.careless` (Maximum Agent Autonomy)**:
     - *Scope & Symbols Autonomy*: Architect has 100% autonomous authority to admit, rename, split, or create new structs, traits, helper methods, and database columns.
     - *Zero Human Escalation*: Escalation to human operators is strictly prohibited for interface, schema, or API changes.
     - *Escalation Exceptions*: Escalate to human ONLY in unrecoverable infrastructure deadlocks (circular DAG blockers or fatal unrecoverable environment crashes).
     - *Arbitration Loop*: On reviewer rejection, synthesize `extended_symbols`, increment `contract_revision`, produce Red Seam Test, and immediately dispatch back to builder.

   - **`hitl.curious` (Informational Telemetry with High Autonomy)**:
     - *Autonomous Approvals*: Architect autonomously approves module-internal symbols, refactorings, helper functions, and non-breaking trait implementations.
     - *Blackboard Telemetry*: When a notable public signature or data structure is modified, Architect MUST post an informational notice to `task_blackboard` (`topic="telemetry"`, `kind="interface_evolution"`) detailing the changes, but continues execution without blocking for human input.
     - *Mandatory Human Escalation*: Escalates via A/B Decision Card ONLY on: breaking public library exports, irreversible database migrations (dropping tables/columns), or introducing new external package or library dependencies.

   - **`hitl.pro` (Professional Engineering Balance — Default)**:
     - *Permitted Autonomous Additions*: Architect is authorized to self-admit into `extended_symbols` without human sign-off: module-internal helper functions, private classes, structs, or internal types, non-breaking error variants, internal test helpers/fixtures, documentation, and invariant annotations.
     - *Mandatory Human Escalation Triggers*: Architect is STRICTLY PROHIBITED from self-approving and MUST escalate to a human on:
       1. **Public API Surface**: Any alteration to public exports/interfaces exposed in root package entrypoints or public module exports.
       2. **CLI Commands & Flags**: Any new subcommand, altered flag, or changed default behavior.
       3. **Database & Storage**: Any new SQLite/DB table, column schema modification, or index creation.
       4. **MCP Protocol Surface**: Any new tool, altered tool arguments, or schema changes.
       5. **Cross-Milestone Contracts**: Any change to symbols consumed by `linked_to` milestone tasks.
     - *Escalation Protocol*: Post structured A/B Decision Card to `task_blackboard` and halt with human blocker.

   - **`hitl.paranoic` (Zero Autonomous Interface Mutation)**:
     - *Absolute Zero Autonomy*: Architect is strictly forbidden from self-approving ANY new symbol or signature modification into `extended_symbols`, including private helpers.
     - *Universal Escalation*: Every single symbol deviation detected by the reviewer requires an explicit human sign-off through an A/B Decision Card on `task_blackboard`.
     - *Full Contract Freezing*: Task cannot transition out of architect arbitration until a human explicitly posts an approval response on the blackboard with the chosen option.

#### 4. Anti-Micromanagement Human Escalation Cards
When the Architect escalates to a human, raw logs are forbidden. The human receives a single-choice decision card:
```markdown
> [!IMPORTANT]
> **Architectural Escalation (Milestone AC-001, HITL Level: PRO)**
> **Task**: `zero-arg-transition-engine`
> **Finding**: Builder requires persistent storage for command traces.
>
> **Architect Recommendation:**
> - **[A] (Recommended)** Approve new `gate_execution_traces` SQLite table with TTL pruning.
> - **[B]** Reject table — mandate writing trace logs purely to disk in `.forge/traces/`.
```

---

## Consequences

### Positive
- **Zero Scope Drift**: `extended_scope` is synchronized atomically between SQLite and the Git manifest with mandatory review proof audit.
- **No Loose JSON Files**: Entire state is encapsulated in ACID SQLite database (`gate_checkpoints`).
- **Unified Breadcrumb Keying**: The same locator in logs acts as the database primary key for checkpoint state.
- **Zero Quoting Bugs**: Zero-argument `task submit` eliminates shell string formatting errors.
- **Zero Token Waste**: Command gates run deterministically in shell with 0 LLM tokens.
- **Clean Fallback on Milestone Seams**: Defect polish on the integrated milestone branch uses the reusable `build_review_cycle` subgraph without task reopening churn.

### Negative
- Scope extension must atomically publish the updated milestone manifest alongside the SQLite transaction.
- Milestone verification runs a full-repository command and an independent review before documentation, increasing the final integration gate's resource and review cost.
