---
id: AC-003-headless-orchestrator-and-role-runners
title: Headless CLI lifecycle orchestrator, role runner subagent execution, and rejection routing
doc_type: contract
status: planned
depends_on:
  - UX-002-lean-verification-and-forge-read
guidance:
  hitl: pro
owners:
  - src/cli/
  - crates/forge-tasks/src/engine/
  - crates/forge-tasks/src/milestones/
  - docs/reference/cli.md
  - docs/reference/acdd.md
invariants:
  - 'INV-HEADLESS-ROLE-RUNNER-ORCHESTRATION: When roles declare runner and prompt_template, Forge CLI (task run / milestone run) autonomously claims gates, renders role prompts with gate context bundles, spawns subagent runner processes in isolated POSIX process groups with distinct session fingerprints, executes command and delivery gates with zero tokens, and routes review rejections via reject_routing until completion.'
  - 'INV-INDEPENDENT-REVIEWER-SESSION: Reviewer runners execute with an isolated session identity and fingerprint distinct from the builder runner session, satisfying reviewer separation and proof independence invariants.'
  - 'INV-REJECT-ROUTING-FEEDBACK-LOOP: When review gates reject with structured findings, the orchestrator routes execution back to the configured upstream stage (scope -> build, symbols -> contract) and passes the review findings into the resumed builder session without cold restarts.'
  - 'INV-TOPOLOGICAL-WAVE-EXECUTION: Milestone run executes tasks in topological wave order, collecting and verifying evidence envelopes automatically and gating subsequent waves until dependencies complete.'
  - 'INV-IN-PLACE-EXECUTION: Task, milestone, and plan orchestration supports execution directly in the current workspace (--in-place) without mandatory worktree cloning or branching when isolated worktrees are not requested.'
  - 'INV-PLAN-WORKFLOW-ORCHESTRATION: The orchestrator supports standalone workflows (docs/workflows/*.yaml) via contextunity-forge-mcp workflow [list|show|run] and structured plans (doc_type: plan in docs/plans/*.md) via contextunity-forge-mcp plan [list|show|run], executing in-place under universal gate governance.'
  - 'INV-DELIVERY-SUBTASK-COMPLETION-GUARD: Mechanical delivery verification fails closed with TASK_SUBTASKS_INCOMPLETE if any declared subtask under the active task or plan step is not completed or cancelled, and with TASK_SUBTASK_EVIDENCE_MISSING if any completed subtask lacks non-empty verification evidence.'
  - 'INV-CLI-UNIFIED-ARGUMENTS: Global CLI and orchestrator execution accept explicit path parameters (--root, --db, --config for custom forge-mcp.yaml, and --profile for custom profile.yaml) with deterministic resolution order.'
required_skills:
  - contextunity-forge
  - agent-instructions
related_plans: []
---

# Headless CLI lifecycle orchestrator, role runner subagent execution, and rejection routing

## Outcome and purpose

Deliver a thin, modular headless CLI orchestrator (`crates/forge-tasks/src/engine/orchestrator/`) over the existing Forge gate primitives without duplicating or rewriting the core task engine:

1. **Subagent Process Spawning (`role.runner`)**:
   - Parse `runner` command templates from profile roles (e.g., `codex exec resume -c model="{model}" {session_id} "{prompt}"` or custom CLI agents).
   - Render contextual role prompts injecting gate requirements, task scopes, active invariants, and evidence expectations.
   - Execute subagents in dedicated POSIX process groups with process lifecycle supervision, timeout cancellation (`libc::kill(-pgid, SIGKILL)`), and streaming output capture.

2. **Autonomous Headless Task Loop (`task run <task_id>`)**:
   - Claim gates sequentially through the active task lifecycle (`contract` $\to$ `build` $\to$ `review` $\to$ `seam` $\to$ `deliver`).
   - Run command gates and delivery commits autonomously with zero LLM tokens using existing engine verification and submission primitives.
   - Route review rejections according to profile `reject_routing` / `reject_to`, feeding reviewer findings into the resumed builder session.

3. **Autonomous Milestone Wave Orchestration (`milestone run <milestone_id>`)**:
   - Resolve topological wave dependencies from the milestone manifest.
   - Dispatch wave tasks, coordinate worktree creation and cleanup (or `--in-place`), and execute `milestone_verification` upon completion.
   - Assemble integration evidence envelopes and complete handoff autonomously.

4. **In-Place Execution, Plans & Standalone Workflows (`--in-place`, `plan run`, `workflow run`)**:
   - Execute standalone workflows (`docs/workflows/*.yaml`, `save: none`) via `contextunity-forge-mcp workflow [list|show|run]`.
   - Execute structured markdown plans (`docs/plans/*.md`, `doc_type: plan`, `save: snapshot`) directly within the current workspace (`--in-place`) via `contextunity-forge-mcp plan [list|show|run]`.

5. **Mechanical Delivery Subtask Guard & Unified Path Arguments**:
   - Prevent premature task delivery by validating that all declared subtasks are either `completed` (with non-empty verification evidence) or explicitly `cancelled`, failing closed with `TASK_SUBTASKS_INCOMPLETE` if any subtask remains unfinished (`ready`, `in_progress`, `blocked`, `pending`) and `TASK_SUBTASK_EVIDENCE_MISSING` if any completed subtask lacks verification evidence.
   - Global deterministic CLI arguments (`--root`, `--db`, `--config`, `--profile`) for all commands.

---

## Tasks in this milestone

### task: orchestrator-types-and-runner-contracts-bootstrap

```yaml
task_ref: orchestrator-types-and-runner-contracts-bootstrap
target: "Define runner execution models, process spawner interfaces, prompt context schemas, and pre-register orchestrator seam tests in a dedicated orchestrator module"
proof_policy: seam-test-first
contract_revision: 1
depends_on: []
scope:
  - crates/forge-tasks/src/tasks/profile.rs
  - crates/forge-tasks/src/engine/mod.rs
  - crates/forge-tasks/src/engine/orchestrator/mod.rs
  - crates/forge-tasks/src/engine/orchestrator/types.rs
  - tests/acdd/mod.rs
  - tests/acdd/orchestrator.rs
symbols:
  - struct:RunnerConfig
  - struct:RunnerInvocationContext
  - enum:OrchestratorEvent
  - struct:OrchestratorState
status: ready
subtasks:
  - subtask_ref: runner-config-and-context-types
    title: "Define RunnerConfig and RunnerInvocationContext in profile and engine/orchestrator/types.rs"
  - subtask_ref: orchestrator-event-and-state-models
    title: "Define state machines and event types for headless lifecycle tracking"
  - subtask_ref: orchestrator-seam-test-stubs
    title: "Pre-register compiling test stubs in tests/acdd/orchestrator.rs"
```

#### Acceptance

1. `RunnerConfig` supports single-pass template interpolation of `{model}`, `{reasoning}`, `{session_id}`, `{prompt}`, and `{worktree}` without mutating `RoleDef` wire serialization.
2. `OrchestratorState` tracks active wave indices, claimed task IDs, worker identities, and cumulative attempt counts in memory without adding parallel SQLite tables.
3. Seam test harness compiles cleanly in `tests/acdd/orchestrator.rs`.

---

### task: subagent-process-spawner-and-runner-environment

```yaml
task_ref: subagent-process-spawner-and-runner-environment
target: "Implement POSIX process group subagent spawner with timeout handling, stream logging, and session fingerprint isolation"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - orchestrator-types-and-runner-contracts-bootstrap
scope:
  - crates/forge-tasks/src/engine/orchestrator/mod.rs
  - crates/forge-tasks/src/engine/orchestrator/spawner.rs
  - tests/acdd/orchestrator.rs
symbols:
  - fn:spawn_role_runner
  - fn:render_role_prompt
  - fn:monitor_runner_process
status: planned
subtasks:
  - subtask_ref: prompt-template-renderer
    title: "Render role prompt_template with task scope, target, gate evidence expectations, and prior rejection findings"
  - subtask_ref: process-group-spawner
    title: "Spawn runner processes in isolated process groups with Stdio pipe handling and SIGKILL timeout supervision"
  - subtask_ref: session-fingerprint-generator
    title: "Generate distinct child-process session fingerprints ensuring builder and reviewer sessions remain strictly independent"
```

#### Acceptance

1. Runner command templates are interpolated safely and spawned in a new process group via `setpgid(0, 0)`.
2. Process timeouts trigger clean process group termination (`kill(-pgid, SIGKILL)`) without zombie subprocesses.
3. Builder sessions can be resumed via session ID, while reviewer sessions receive a distinct fresh session fingerprint via child-process environment overrides.

---

### task: headless-task-lifecycle-and-rejection-router

```yaml
task_ref: headless-task-lifecycle-and-rejection-router
target: "Implement CLI task run command advancing a task through active profile gates with rejection routing and subtask completion guard"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - subagent-process-spawner-and-runner-environment
scope:
  - crates/forge-tasks/src/engine/orchestrator/mod.rs
  - crates/forge-tasks/src/engine/orchestrator/task_runner.rs
  - crates/forge-tasks/src/engine/submit.rs
  - src/cli/task.rs
  - docs/reference/cli.md
  - tests/acdd/orchestrator.rs
symbols:
  - fn:run_task_lifecycle
  - fn:handle_gate_rejection
status: planned
subtasks:
  - subtask_ref: task-run-gate-progression
    title: "Iterate across active task gates, triggering runner execution on role gates and zero-token verification on command gates"
  - subtask_ref: rejection-routing-feedback-injection
    title: "Route review rejections according to reject_routing/reject_to and feed structured findings to the next builder turn"
  - subtask_ref: delivery-subtask-completion-guard
    title: "Validate that all declared subtasks are completed (with evidence) or cancelled, failing closed with TASK_SUBTASKS_INCOMPLETE or TASK_SUBTASK_EVIDENCE_MISSING"
  - subtask_ref: delivery-snapshot-and-receipt
    title: "Finalize delivery gate automatically using existing candidate snapshot and receipt persistence"
```

#### Acceptance

1. `contextunity-forge-mcp task run <task_id>` advances a task from initial claim to delivery without manual intervention, supporting `--in-place` execution.
2. Review rejections route back to the configured target gate (`build` or `contract`), passing structured review findings into the resumed builder session.
3. Delivery verification fails closed with `TASK_SUBTASKS_INCOMPLETE` if any declared subtask remains unfinished (`ready`, `in_progress`, `blocked`, `pending`), and with `TASK_SUBTASK_EVIDENCE_MISSING` if any completed subtask lacks non-empty verification evidence (while `cancelled` subtasks are permitted).
4. Delivery stage records the delivery receipt commit and snapshot hash through the existing submission pipeline.

---

### task: milestone-wave-runner-and-autonomous-delivery

```yaml
task_ref: milestone-wave-runner-and-autonomous-delivery
target: "Implement CLI milestone run, plan [list|show|run], and workflow [list|show|run] with wave scheduling and global CLI path options"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - headless-task-lifecycle-and-rejection-router
scope:
  - crates/forge-tasks/src/engine/orchestrator/mod.rs
  - crates/forge-tasks/src/engine/orchestrator/waves.rs
  - crates/forge-tasks/src/milestones/mod.rs
  - src/cli/mod.rs
  - src/cli/milestone.rs
  - src/cli/plan.rs
  - src/cli/workflow.rs
  - docs/reference/cli.md
  - docs/reference/acdd.md
  - tests/acdd/orchestrator.rs
symbols:
  - fn:run_milestone_waves
  - fn:execute_wave_verification
  - fn:run_plan_workflow
  - fn:run_standalone_workflow
status: planned
subtasks:
  - subtask_ref: topological-wave-scheduler
    title: "Schedule and dispatch milestone tasks in wave order respecting depends_on dependencies"
  - subtask_ref: in-place-plan-and-workflow-runners
    title: "Implement plan [list|show|run] (save: snapshot) and workflow [list|show|run] (save: none) in-place execution"
  - subtask_ref: wave-barrier-and-autonomous-handoff
    title: "Execute milestone verification commands and invoke milestone handoff when all tasks pass"
  - subtask_ref: global-cli-path-resolution
    title: "Support deterministic --root, --db, --config, and --profile CLI arguments across commands"
```

#### Acceptance

1. `contextunity-forge-mcp milestone run <milestone_id>` executes all milestone tasks in topological wave order, supporting isolated worktrees or `--in-place` mode, and completes verification and handoff.
2. `contextunity-forge-mcp plan [list|show|run]` executes structured documentation plans (`doc_type: plan`) in-place without worktree spawning.
3. `contextunity-forge-mcp workflow [list|show|run]` executes standalone root-gate workflows (`docs/workflows/*.yaml`) with `save: none`.
4. Global CLI arguments (`--root`, `--db`, `--config`, `--profile`) resolve deterministically across CLI commands.

---

### task: forge-guide-lifecycle-gates-and-subtasks

```yaml
task_ref: forge-guide-lifecycle-gates-and-subtasks
target: "Extend forge_guide with compact agent guidance for lifecycle, ACDD, code, documentation and gates"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - milestone-wave-runner-and-autonomous-delivery
scope:
  - src/cli/guide.rs
  - src/mcp/tools.rs
  - src/mcp/tools/inputs.rs
  - docs/reference/mcp-tools.md
  - docs/reference/cli.md
  - skills/contextunity-forge/references/tool-catalog.md
  - tests/acdd/mcp.rs
  - tests/mcp/context.rs
  - tests/cli_roots.rs
symbols: []
status: planned
subtasks:
  - subtask_ref: lifecycle-guidance
    title: "Explain task lifecycle operations and ACDD execution subjects"
  - subtask_ref: gate-and-proof-guidance
    title: "Explain profile transitions, persistence, observations and proof"
  - subtask_ref: subtask-guidance
    title: "Explain subtask management and the implemented delivery guard"
  - subtask_ref: code-and-docs-guidance
    title: "Guide agents through code navigation and documentation tools"
  - subtask_ref: discovery-and-verification
    title: "Advertise topics, honor enabled modules and verify MCP and CLI responses"
```

#### Guide structure

| Topic | Purpose |
| --- | --- |
| `lifecycle` | How an agent lists and claims work, follows gate guidance, verifies, submits, handles rejection and delivers. |
| `acdd` | Workflow, Plan and Milestone: when to use each, their gate shapes, persistence defaults and available commands. |
| `gates` | Conditions, transitions, persistence, immutable candidates, observed evidence and proof. |
| `subtasks` | Add, update and list subtasks; explain statuses, evidence and delivery checks. |
| `code` | Navigate code, inspect symbols, callers and tests, search AST patterns, and read source. |
| `docs` | Find, read and author documentation, including frontmatter, links and invariants. |

`acdd` and `docs` extend existing topics. `query` remains the default and retains its existing fields; add a short topic index there. Keep `ast`, `adapter`, `init` and `validate` available. Each topic provides a short purpose, actionable steps or rules, a small copyable example and references to the relevant topic or repository guide.

#### Acceptance

1. `lifecycle` shows `task_list`, `task_claim`, `task_manage(action="verify")` and `task_submit`, plus the implemented headless run commands. Explain role runners, independent review and builder resumption with rejection findings. When enabled, show `task_blackboard` for decisions, findings and blockers. Use the active profile and the claim's `workflow_guidance` as authority; gate names and their order are examples, not hardcoded rules. Include concise next actions for common claim, proof or candidate errors.
2. `acdd` explains ADR 0021: a taskless Workflow defaults to `save: none`; a Plan executes steps in the current working tree with snapshots and no automatic commit; a Milestone runs synthesis, task dependency waves, verification, independent review and handoff. Show the implemented `workflow`, `plan` and `milestone` entry points. Describe optional run logging only if supported; otherwise mark it as planned.
3. `gates` explains `when` (`code`, `doc`, `db_migration`, `seam`, `public_api`), configured `pass`/`fail` transitions and finding-class `loop` routing. Explain what each condition tests: `seam` concerns concurrency and `public_api` concerns exported callables. Explain `save.pre`/`save.post`, snapshots without a branch-history commit, configured commits and `immutable: true`; immutability applies when configured, and candidate drift produces `TASK_CANDIDATE_MISMATCH`.
4. Explain that verification observes without advancing, while submission consumes claim-bound proof. Cover `gate_execution_traces`, declarative `commands.output.tests` and individual `seam_test_refs`: caller-supplied JSON alone cannot replace the required execution observation. `subtasks` shows `task_manage` actions `subtask_add`, `subtask_update` and `subtask_list`, status and evidence, and the implemented delivery diagnostics. A recorded evidence string is not independently verified by `subtask_update`.
5. `code` shows how to start with `code_map_overview`, find a symbol with `code_map_search`, inspect it with `code_map_inspect` or `forge_read`, trace callers and impact with `code_map_explain`/`code_map_impact`, inspect tests with `code_map_tests`, and use `ast_grep_search` for code patterns. Explain that candidate relationships and lexical test suggestions are not verified calls or coverage. Reuse the existing `query` guidance without creating a second, divergent tool catalog.
6. Extend `docs` with `search_docs`, `get_doc` and `forge_read` for finding and reading documents, while retaining its existing frontmatter, invariant and type guidance. Give a compact MCP example and the corresponding CLI documentation commands; update the MCP and CLI references and the skill tool catalog.
7. Before writing the guidance, compare the completed prerequisite with the real CLI/MCP and delivery paths. The current manual delivery path accepts `cancelled` subtasks, while this milestone requires all subtasks completed with evidence. If the prerequisite has not resolved that mismatch, return it to its task owner; do not present the stricter rule as already enforced. Describe actual delivery diagnostics, including `TASK_SUBTASKS_INCOMPLETE` and, where applicable, `TASK_SUBTASK_EVIDENCE_MISSING`. Mark other ADR behavior not implemented by this milestone as planned.
8. Preserve existing topic keys, default `query`, blackboard filtering and the unknown-topic error. Advertise only enabled capabilities: filter the topic index and guidance when ACDD, codemap, docs or blackboard is disabled; disabled topics follow the existing empty-guidance behavior.
9. Reuse the shared CLI/MCP response formatter. Omitting `format` returns Markdown, as does `format: "markdown"` in MCP or `--format markdown` in the CLI; `format: "json"` and `--format json` return structured JSON. Both formats present the same guide information. Preserve readable Markdown, existing JSON keys and the current response-budget policy.
10. Through real MCP and CLI calls, test default and explicit Markdown, JSON, content parity, copyable examples, topic discovery, module combinations and existing-topic compatibility. Compare CLI/MCP Markdown and parsed JSON for equivalent inputs, allowing the trailing CLI newline. Use dedicated tests in the scoped suites; run the focused tests and strict Clippy from `TESTS.md`.
