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
required_skills:
  - contextunity-forge
  - agent-instructions
related_plans: []
---

# Headless CLI lifecycle orchestrator, role runner subagent execution, and rejection routing

## Outcome and purpose

Deliver autonomous end-to-end task and milestone execution directly from the Forge CLI without requiring a human-driven or interactive coordinator agent:

1. **Subagent Process Spawning (`role.runner`)**:
   - Parse `runner` command templates from profile roles (e.g., `codex exec resume -c model="{model}" {session_id} "{prompt}"` or custom CLI agents).
   - Render contextual role prompts injecting gate requirements, task scopes, active invariants, and evidence expectations.
   - Execute subagents in dedicated POSIX process groups with process lifecycle supervision, timeout cancellation (`libc::kill(-pgid, SIGKILL)`), and streaming output capture.

2. **Autonomous Headless Task Loop (`task run <task_id>`)**:
   - Claim gates sequentially through the active task lifecycle (`contract` $\to$ `build` $\to$ `review` $\to$ `deliver`).
   - Run command gates and delivery commits autonomously with zero LLM tokens.
   - Route review rejections according to profile `reject_routing` (e.g. `scope` $\to$ `build`, `symbols` $\to$ `contract`), feeding reviewer findings into the next builder turn.

3. **Autonomous Milestone Wave Orchestration (`milestone run <milestone_id>`)**:
   - Resolve topological wave dependencies from the milestone manifest.
   - Dispatch wave tasks, coordinate worktree creation and cleanup, and execute `milestone_verification` upon wave completion.
   - Assemble integration evidence envelopes and prepare handoff context autonomously.

---

## Tasks in this milestone

### task: orchestrator-types-and-runner-contracts-bootstrap

```yaml
task_ref: orchestrator-types-and-runner-contracts-bootstrap
target: "Define runner execution models, process spawner interfaces, prompt context schemas, and pre-register orchestrator seam tests"
proof_policy: seam-test-first
contract_revision: 1
depends_on: []
scope:
  - crates/forge-tasks/src/tasks/profile.rs
  - crates/forge-tasks/src/engine/lifecycle.rs
  - tests/acdd/orchestrator.rs
symbols:
  - struct:RunnerConfig
  - struct:RunnerInvocationContext
  - enum:OrchestratorEvent
  - struct:OrchestratorState
status: ready
subtasks:
  - subtask_ref: runner-config-and-context-types
    title: "Define RunnerConfig and RunnerInvocationContext in profile and lifecycle engines"
  - subtask_ref: orchestrator-event-and-state-models
    title: "Define state machines and event types for headless lifecycle tracking"
  - subtask_ref: orchestrator-seam-test-stubs
    title: "Pre-register compiling test stubs in tests/acdd/orchestrator.rs"
```

#### Acceptance

1. `RunnerConfig` supports template interpolation of `{model}`, `{reasoning}`, `{session_id}`, `{prompt}`, and `{worktree}`.
2. `OrchestratorState` tracks active wave indices, claimed task IDs, worker identities, and cumulative attempt counts.
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
  - crates/forge-tasks/src/engine/claim.rs
  - crates/forge-tasks/src/milestones/command_gate.rs
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
    title: "Generate distinct session fingerprints ensuring builder and reviewer sessions remain strictly independent"
```

#### Acceptance

1. Runner command templates are interpolated safely and spawned in a new process group via `setpgid(0, 0)`.
2. Process timeouts trigger clean process group termination without zombie subprocesses.
3. Builder sessions can be resumed via session ID, while reviewer sessions receive a distinct fresh session fingerprint.

---

### task: headless-task-lifecycle-and-rejection-router

```yaml
task_ref: headless-task-lifecycle-and-rejection-router
target: "Implement CLI task run command advancing a task through contract, build, review, and deliver gates with rejection routing"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - subagent-process-spawner-and-runner-environment
scope:
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
    title: "Iterate across task gates, triggering runner execution on role gates and execute_command_gate on command gates"
  - subtask_ref: rejection-routing-feedback-injection
    title: "Route review rejections according to reject_routing and feed structured findings to the next builder turn"
  - subtask_ref: delivery-snapshot-and-receipt
    title: "Finalize delivery gate automatically with snapshot candidate and receipt generation"
```

#### Acceptance

1. `contextunity-forge-mcp task run <task_id>` advances a task from initial claim to delivery without manual intervention.
2. Review rejections route back to the appropriate gate (`build` or `contract`), passing structured review findings to the builder.
3. Delivery stage automatically records the delivery receipt commit and snapshot hash.

---

### task: milestone-wave-runner-and-autonomous-delivery

```yaml
task_ref: milestone-wave-runner-and-autonomous-delivery
target: "Implement CLI milestone run executing DAG waves in topological order, running verification, and assembling integration evidence"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - headless-task-lifecycle-and-rejection-router
scope:
  - crates/forge-tasks/src/milestones/mod.rs
  - src/cli/milestone.rs
  - docs/reference/cli.md
  - docs/reference/acdd.md
  - tests/acdd/orchestrator.rs
symbols:
  - fn:run_milestone_waves
  - fn:execute_wave_verification
status: planned
subtasks:
  - subtask_ref: topological-wave-scheduler
    title: "Schedule and dispatch milestone tasks in wave order respecting depends_on dependencies"
  - subtask_ref: wave-barrier-verification
    title: "Execute milestone verification commands at wave boundaries and verify zero failures"
  - subtask_ref: autonomous-handoff-assembly
    title: "Construct integration evidence file and invoke milestone handoff when all tasks pass"
```

#### Acceptance

1. `contextunity-forge-mcp milestone run <milestone_id>` executes all milestone tasks in topological wave order.
2. Parallel tasks execute in isolated worktrees; dependent waves start only after upstream tasks complete and pass verification.
3. Once all tasks are delivered, `milestone run` verifies repository integrity and archives the milestone.
