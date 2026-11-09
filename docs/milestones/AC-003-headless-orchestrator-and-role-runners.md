---
id: AC-003-headless-orchestrator-and-role-runners
title: Headless CLI lifecycle orchestrator, role runner subagent execution, and rejection routing
doc_type: contract
status: active
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
- 'INV-PLAN-WORKFLOW-HARNESS: All tests for structured plans (docs/plans/*.md) and standalone workflows (docs/workflows/*.yaml) must use PlanHarness and WorkflowHarness with canonical templates in tests/acdd/fixtures/plans/ and workflows/. Ad-hoc markdown heredocs and subprocess CLI spawning are strictly banned.'
required_skills:
- contextunity-forge
- agent-instructions
related_plans: []
started_at: 2026-11-08T21:24:45+00:00
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
target: Define runner execution models, process spawner interfaces, prompt context schemas, and pre-register orchestrator seam tests in a dedicated orchestrator module
proof_policy: seam-test-first
contract_revision: 2
depends_on: []
scope:
- crates/forge-tasks/src/tasks/profile.rs
- crates/forge-tasks/src/engine/mod.rs
- crates/forge-tasks/src/engine/orchestrator/mod.rs
- crates/forge-tasks/src/engine/orchestrator/types.rs
- tests/acdd/mod.rs
- tests/acdd/orchestrator.rs
- crates/forge-tasks/src/tasks/profile/mod.rs
- tests/acdd/orchestrator
symbols:
- struct:RunnerConfig
- struct:RunnerInvocationContext
- struct:AgentDef
- enum:OrchestratorEvent
- struct:OrchestratorState
status: completed
subtasks:
- subtask_ref: runner-config-and-context-types
  title: Define RunnerConfig and RunnerInvocationContext in profile and engine/orchestrator/types.rs
  status: completed
  evidence: RunnerConfig projection and RunnerInvocationContext::render_runner; cargo test --test acdd orchestrator::types::, 8 passed, 0 failed; strict all-target Clippy passed on HEAD a813f830c.
- subtask_ref: profile-agent-schema-reconciliation
  title: Rename RoleDef and 'roles' to AgentDef and 'agents' in GateProfile with backward-compatible Serde aliases (alias = 'roles', alias = 'role')
  status: completed
  evidence: AgentDef with RoleDef compatibility alias; agent/role and agents/roles Serde handling, including duplicate rejection, exercised by orchestrator::types tests; 8 passed, 0 failed on HEAD a813f830c.
- subtask_ref: orchestrator-event-and-state-models
  title: Define state machines and event types for headless lifecycle tracking
  status: completed
  evidence: OrchestratorState tracks waves, claims, workers, and attempts with typed events; orchestrator::types state and event tests pass in scoped suite (8 passed, 0 failed) on HEAD a813f830c.
- subtask_ref: orchestrator-seam-test-stubs
  title: Pre-register compiling test stubs in tests/acdd/orchestrator.rs
  status: completed
  evidence: 'tests/acdd/orchestrator/mod.rs registers types, lifecycle, spawner and waves domain modules; cargo test --test acdd orchestrator::types:: compiled and passed 8 tests on HEAD a813f830c.'
extended_scope:
- crates/forge-tasks/src/tasks/profile/mod.rs
- tests/acdd/orchestrator
receipt:
  delivery_gate: deliver
  candidate_gate: build
  contract_revision: 2
  passed_at: 2026-11-09T14:10:59.942571795+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/acdd/orchestrator/types.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/AC-003-headless-orchestrator-and-role-runners.md
        PASS rust-check-and-test::tests/acdd/orchestrator/types.rs: exit=0 passed=1 failed=0 (322 ms)
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All 10 candidate blobs match the admitted scope. Reviewed engine/module registration, profile model changes and the complete new types/test modules. No protected configuration or neighboring workspace changes. Pre-submit HEAD, milestone branch, index, Git status and milestone source match the starting state. Detailed review: task blackboard #1632.'
        claims:
          applicable: true
          evidence: 'RunnerConfig projects the existing agent definition without changing its wire fields; RunnerInvocationContext expands model, reasoning, session_id, prompt and worktree once and preserves placeholder-looking substituted data. Native agents/agent and legacy roles/role use one Serde model, reject duplicate aliases and keep a Rust RoleDef compatibility alias. In-memory state records waves, tasks, workers and cumulative attempts while the task engine remains authoritative. Exact-call graph confirms the contract seam enters the real profile validator/parser/normalizer/merger. Tests: cargo test --test acdd orchestrator::types:: = 8 passed/0 failed/0 ignored. Detailed review: task blackboard #1632.'
        concurrency:
          applicable: true
          evidence: 'New models own their data and mutations require exclusive &mut access; no new thread, lock, async, database or publication boundary. State/events observe existing authoritative task transitions. Scope snapshots and baseline protections remain enforced by the engine. Relevant command-proof tests pass (5/0). Detailed review: task blackboard #1632.'
        project_isolation:
          applicable: true
          evidence: 'No new filesystem access, process spawning or cross-project data writes in bootstrap code. Worktree is a typed PathBuf input. Existing confined profile loading and normalized relative path checks remain intact; SHA pin checks still hash the original profile bytes. Profile regression tests pass (9/0) and gate integration tests pass (6/0). Detailed review: task blackboard #1632.'
        administration:
          applicable: true
          evidence: 'Contract revision 2 has accepted runtime-red proof and accepted green build claim 65. Current review claim 68 succeeded with actual reviewer session distinct from builder 01a1c09d-0043-7601-b745-a82481fcbfe7. All four subtasks are completed with nonempty evidence. Current HEAD equals accepted candidate baseline a813f830c; no history rewrite. Full strict Clippy cargo clippy --all-targets --all-features -- -D warnings exited 0. Delivery will use CLI-generated evidence and Forge-managed receipt/commit on the existing milestone branch. Detailed review: task blackboard #1632.'
      extended_scope_review:
        approved_paths:
        - crates/forge-tasks/src/tasks/profile/mod.rs
        - tests/acdd/orchestrator
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-HEADLESS-ROLE-RUNNER-ORCHESTRATION: When roles declare runner and prompt_template, Forge CLI (task run / milestone run) autonomously claims gates, renders role prompts with gate context bundles, spawns subagent runner processes in isolated POSIX process groups with distinct session fingerprints, executes command and delivery gates with zero tokens, and routes review rejections via reject_routing until completion.'
    - 'INV-INDEPENDENT-REVIEWER-SESSION: Reviewer runners execute with an isolated session identity and fingerprint distinct from the builder runner session, satisfying reviewer separation and proof independence invariants.'
    - 'INV-REJECT-ROUTING-FEEDBACK-LOOP: When review gates reject with structured findings, the orchestrator routes execution back to the configured upstream stage (scope -> build, symbols -> contract) and passes the review findings into the resumed builder session without cold restarts.'
    - 'INV-TOPOLOGICAL-WAVE-EXECUTION: Milestone run executes tasks in topological wave order, collecting and verifying evidence envelopes automatically and gating subsequent waves until dependencies complete.'
    - 'INV-IN-PLACE-EXECUTION: Task, milestone, and plan orchestration supports execution directly in the current workspace (--in-place) without mandatory worktree cloning or branching when isolated worktrees are not requested.'
    - 'INV-PLAN-WORKFLOW-ORCHESTRATION: The orchestrator supports standalone workflows (docs/workflows/*.yaml) via contextunity-forge-mcp workflow [list|show|run] and structured plans (doc_type: plan in docs/plans/*.md) via contextunity-forge-mcp plan [list|show|run], executing in-place under universal gate governance.'
    - 'INV-DELIVERY-SUBTASK-COMPLETION-GUARD: Mechanical delivery verification fails closed with TASK_SUBTASKS_INCOMPLETE if any declared subtask under the active task or plan step is not completed or cancelled, and with TASK_SUBTASK_EVIDENCE_MISSING if any completed subtask lacks non-empty verification evidence.'
    - 'INV-CLI-UNIFIED-ARGUMENTS: Global CLI and orchestrator execution accept explicit path parameters (--root, --db, --config for custom forge-mcp.yaml, and --profile for custom profile.yaml) with deterministic resolution order.'
    - 'INV-PLAN-WORKFLOW-HARNESS: All tests for structured plans (docs/plans/*.md) and standalone workflows (docs/workflows/*.yaml) must use PlanHarness and WorkflowHarness with canonical templates in tests/acdd/fixtures/plans/ and workflows/. Ad-hoc markdown heredocs and subprocess CLI spawning are strictly banned.'
    architectural_notes:
    - 'decisions: User approved re-admission of bootstrap task with new contract revision, unchanged substantive scope and acceptance, to recover missing session fingerprint of accepted build claim 53. Preserve all current work and HEAD. Re-run required authentic contract, build and independent review; builders/reviewers must use genuine separate session identities, using CLI processes with FORGE_AGENT_SESSION_ID matching their actual Codex session if required. Do not fabricate evidence or change SQLite directly.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `RunnerConfig` supports single-pass template interpolation of `{model}`, `{reasoning}`, `{session_id}`, `{prompt}`, and `{worktree}` without mutating `AgentDef` / `RoleDef` wire serialization.
2. `GateProfile` recognizes `agents:` and gate `agent:` natively while transparently admitting legacy `roles:` and `role:` keys via Serde aliases without breaking existing profile documents.
3. `OrchestratorState` tracks active wave indices, claimed task IDs, worker identities, and cumulative attempt counts in memory without adding parallel SQLite tables.
4. Seam test harness compiles cleanly in `tests/acdd/orchestrator.rs`.

---

### task: subagent-process-spawner-and-runner-environment

```yaml
task_ref: subagent-process-spawner-and-runner-environment
target: Implement POSIX process group subagent spawner with timeout handling, stream logging, and session fingerprint isolation
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- orchestrator-types-and-runner-contracts-bootstrap
scope:
- crates/forge-tasks/src/engine/orchestrator/mod.rs
- crates/forge-tasks/src/engine/orchestrator/spawner.rs
- tests/acdd/orchestrator.rs
- tests/acdd/orchestrator/spawner.rs
- tests/acdd/orchestrator/runner_fixture.py
symbols:
- fn:spawn_role_runner
- fn:render_role_prompt
- fn:monitor_runner_process
status: completed
subtasks:
- subtask_ref: prompt-template-renderer
  title: Render role prompt_template with task scope, target, gate evidence expectations, and prior rejection findings
  status: completed
  evidence: 'Preserves the existing engine-prepared role prompt verbatim and attaches complete canonical claim JSON including scope, invariants, evidence and rejection. Required fields and truncation checks fail closed. cargo test --test acdd orchestrator::spawner::: 18 passed, 0 failed, 0 ignored; full strict Clippy passed.'
- subtask_ref: process-group-spawner
  title: Spawn runner processes in isolated process groups with Stdio pipe handling and SIGKILL timeout supervision
  status: completed
  evidence: Original trusted command words are parsed before per-token single-pass interpolation; Command argv, null stdin, child cwd, isolated POSIX group, concurrent bounded byte readers, deadline through pipe EOF, SIGKILL plus direct-child wait, owned Drop cleanup and typed I/O errors. Real process tests cover exact argv/group/cwd, streams, timeout descendants, ECHILD, sink failure, signal/nonzero exits, binary output, flood deadline and moved child. Spawner suite 18/0/0; bootstrap 8/0/0; cargo clippy --all-targets --all-features -- -D warnings exit 0.
- subtask_ref: session-fingerprint-generator
  title: Generate distinct child-process session fingerprints ensuring builder and reviewer sessions remain strictly independent
  status: completed
  evidence: Explicit Resume requires and retains nonblank supplied identity; Fresh uses 256 bits of OS entropy per invocation. Resolved identity is inserted once into original argv slots and child FORGE_AGENT_SESSION_ID only; parent environment unchanged. Handle and outcome expose same identity. Real child readback verifies Resume and two distinct Fresh invocations. Spawner suite 18/0/0, bootstrap 8/0/0, full strict Clippy exit 0.
extended_scope:
- tests/acdd/orchestrator/spawner.rs
- tests/acdd/orchestrator/runner_fixture.py
receipt:
  delivery_gate: deliver
  candidate_gate: build
  contract_revision: 1
  passed_at: 2026-11-09T15:00:35.656710433+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/acdd/orchestrator/spawner.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/AC-003-headless-orchestrator-and-role-runners.md
        PASS rust-check-and-test::tests/acdd/orchestrator/spawner.rs: exit=0 passed=1 failed=0 (464 ms)
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All four candidate blobs match the live files and admitted scope. Module registration, production spawner, domain seam tests and Python fixture contribute directly to the contract. Extended test paths are necessary for real POSIX process behavior; no protected configuration or neighboring project edits. Starting HEAD, branch, index, status and existing milestone bytes remain unchanged before submission. Full review and checks: /tmp/ac003-spawner-review-y54m_9iq/review-report.md.'
        claims:
          applicable: true
          evidence: 'Acceptance 1-3 and architectural contract #1633/#1635 traced to real public APIs. Trusted command syntax is tokenized before single-pass per-argument interpolation; no implicit shell. Prepared engine prompt is preserved with full canonical claim context and malformed/truncated authority rejected. Resume retains identity; Fresh uses 256-bit OS entropy, matching argv/handle/outcome and child-only environment override. cargo test --test acdd orchestrator::spawner::: exit 0, 18 passed/0 failed/0 ignored; bootstrap types: exit 0, 8 passed/0 failed/0 ignored.'
        concurrency:
          applicable: true
          evidence: 'Owned child handle and WNOWAIT exit observation retain PID ownership until original-group SIGKILL, direct-child kill and wait/reap. Two nonblocking reader threads use an eight-chunk bounded channel, cancellation and joins; deadline remains active through both pipe EOFs and continuous output. RAII covers setup errors, callback errors, timeout and abandonment. Four additional compiled production-API probes passed: full-channel sink failure, Drop descendant cleanup, escaped inherited-pipe bound, and static-shell positional argument safety. No database/async/lock ownership changes.'
        project_isolation:
          applicable: true
          evidence: Command current_dir binds only the supplied worktree; parent cwd and environment are unchanged. Dedicated process group never signals the parent or a child-moved group; directly owned child is independently killed/reaped. No new repository discovery, profile loading, filesystem publication, global session mutation or persistence. Live Linux process tests verify cwd/group/session boundaries. Non-Unix paths explicitly return Unsupported; other platforms were statically inspected, not executed.
        administration:
          applicable: true
          evidence: Accepted runtime-red contract claim 10 and green build claim 14 at contract revision 1; all three subtasks completed with nonempty evidence. Full task inspect confirms accepted candidate 4b98a3f6 is bound to unchanged baseline HEAD a3b37935759acf7d249e1a2d1cb1757deca83a49. Forge snapshots are intentionally parentless. Review claim 20 uses actual independent session 01a1bdd1-3650-7c50-9ba7-5bf4cdd6f747, distinct from builder session. cargo clippy --all-targets --all-features -- -D warnings exited 0 with zero warnings; git diff --check exited 0. Delivery will use Forge-generated evidence and receipt/commit on the milestone branch.
      extended_scope_review:
        approved_paths:
        - tests/acdd/orchestrator/spawner.rs
        - tests/acdd/orchestrator/runner_fixture.py
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-HEADLESS-ROLE-RUNNER-ORCHESTRATION: When roles declare runner and prompt_template, Forge CLI (task run / milestone run) autonomously claims gates, renders role prompts with gate context bundles, spawns subagent runner processes in isolated POSIX process groups with distinct session fingerprints, executes command and delivery gates with zero tokens, and routes review rejections via reject_routing until completion.'
    - 'INV-INDEPENDENT-REVIEWER-SESSION: Reviewer runners execute with an isolated session identity and fingerprint distinct from the builder runner session, satisfying reviewer separation and proof independence invariants.'
    - 'INV-REJECT-ROUTING-FEEDBACK-LOOP: When review gates reject with structured findings, the orchestrator routes execution back to the configured upstream stage (scope -> build, symbols -> contract) and passes the review findings into the resumed builder session without cold restarts.'
    - 'INV-TOPOLOGICAL-WAVE-EXECUTION: Milestone run executes tasks in topological wave order, collecting and verifying evidence envelopes automatically and gating subsequent waves until dependencies complete.'
    - 'INV-IN-PLACE-EXECUTION: Task, milestone, and plan orchestration supports execution directly in the current workspace (--in-place) without mandatory worktree cloning or branching when isolated worktrees are not requested.'
    - 'INV-PLAN-WORKFLOW-ORCHESTRATION: The orchestrator supports standalone workflows (docs/workflows/*.yaml) via contextunity-forge-mcp workflow [list|show|run] and structured plans (doc_type: plan in docs/plans/*.md) via contextunity-forge-mcp plan [list|show|run], executing in-place under universal gate governance.'
    - 'INV-DELIVERY-SUBTASK-COMPLETION-GUARD: Mechanical delivery verification fails closed with TASK_SUBTASKS_INCOMPLETE if any declared subtask under the active task or plan step is not completed or cancelled, and with TASK_SUBTASK_EVIDENCE_MISSING if any completed subtask lacks non-empty verification evidence.'
    - 'INV-CLI-UNIFIED-ARGUMENTS: Global CLI and orchestrator execution accept explicit path parameters (--root, --db, --config for custom forge-mcp.yaml, and --profile for custom profile.yaml) with deterministic resolution order.'
    - 'INV-PLAN-WORKFLOW-HARNESS: All tests for structured plans (docs/plans/*.md) and standalone workflows (docs/workflows/*.yaml) must use PlanHarness and WorkflowHarness with canonical templates in tests/acdd/fixtures/plans/ and workflows/. Ad-hoc markdown heredocs and subprocess CLI spawning are strictly banned.'
    architectural_notes:
    - |
      decisions: # AC-003 spawner contract and evidence map

      ## Authority and ownership

      Task: forge-mcp/AC-003-headless-orchestrator-and-role-runners:subagent-process-spawner-and-runner-environment. Contract revision 1, claim 10, worker ac003-spawner-contract-01a1c08b, actual session 01a1c08b-4515-7e72-9466-b989c7b20048. Baseline HEAD a3b37935759acf7d249e1a2d1cb1757deca83a49, clean before claim. Policy seam-test-first, compiling runtime-red assertion 101, fresh CLI observation and automatic evidence compilation. This role ends at contract.

      Authority: task Acceptance 1-3 and three declared subtasks, milestone synthesis #1618, active task.contract guidance, src/AGENTS.md, tests/AGENTS.md, tests/acdd/AGENTS.md, TESTS.md, test-harness architecture, ADRs 0015/0020/0021, contextunity-forge and test-contract-seam skills. No acceptance or protected profile/configuration changes.

      Write owners: production spawner.rs and orchestrator/mod.rs; already registered tests/acdd/orchestrator/spawner.rs and runner_fixture.py are admitted by Forge scope extension. The original tests/acdd/orchestrator.rs scope names an obsolete flat layout; do not recreate it or add another test binary. Extension is engine-published manifest metadata only. Preserve all concurrent work.

      ## Architectural synthesis

      | Surface | Existing authority / current consumer | Decision / future consumer |
      | --- | --- | --- |
      | Agent schema | AgentDef/RoleDef and GateProfile/GateDef in tasks/profile; RunnerConfig::from_role | Preserve these sole registries and wire formats. No role-name heuristics or second configuration table. |
      | Command interpolation | RunnerInvocationContext::render_runner in orchestrator/types.rs; eight delivered bootstrap tests | This renderer is single-pass text, not shell escaping. Spawner parses the original trusted template into argv BEFORE substituting runtime values, then reuses the bootstrap renderer on each original token. Command::new + args, no implicit shell. |
      | Prompt template | engine/context.rs:1003-1068 already renders configured/default role prompt once into workflow_guidance.role_runner.rendered_prompt | render_role_prompt consumes the real claim JSON, validates required task/goal/scope/evidence/prepared-prompt fields, preserves the existing rendered prompt verbatim, and appends the canonical claim JSON as gate context. Include scope, invariants, evidence and rejection findings without a second template renderer or parallel prompt schema. Truncated/malformed authority fails closed. |
      | Command execution | milestones/command_gate.rs private run_shell uses sh -c, process_group(0), concurrent readers, deadline, kill(-pgid,SIGKILL), wait; integration.rs has similar private helpers | Reuse the std Unix process_group(0) mechanism and existing dependency libc. The private shell runner cannot safely transport role argv or supply a streaming callback/session policy; do not expose its shell semantics or refactor unrelated owners in this contract. |
      | Session identity | store/lifecycle.rs resolve_session_fingerprint uses FORGE_AGENT_SESSION_ID before CODEX_THREAD_ID/other host fallbacks | Explicit Resume/Fresh enum, independent of role spelling. Resume preserves nonempty invocation.session_id in argv and child FORGE_AGENT_SESSION_ID. Fresh allocates a distinct nonempty per-invocation identity using OS entropy available through existing dependencies, replaces the session slot with that fresh identity, and overrides child FORGE_AGENT_SESSION_ID without modifying the parent's environment. No inherited builder session reuse. |
      | Gate progression and durable state | Existing task claim/verify/submit, store, traces, rejection context | Spawner returns observations only. Next task headless-task-lifecycle-and-rejection-router owns gate selection, model resolution, resuming the builder after review rejection, log persistence, event recording and transition decisions. No new tables, gate engine or CLI surface here. |

      Complete path: next lifecycle task resolves validated profile role and RunnerConfig -> obtains complete real claim + prepared prompt -> render_role_prompt -> fills RunnerInvocationContext -> selects explicit session policy -> spawn_role_runner parses template/expands original argv tokens once, sets cwd and child identity, null stdin and piped stdout/stderr, isolated POSIX group -> owned RunnerProcess -> monitor_runner_process drains both pipes incrementally into fallible caller callback while checking one deadline -> group termination if needed, direct-child wait/reap and reader shutdown -> RunnerOutcome -> lifecycle consumes outcome/events and existing gate APIs. There is no lifecycle implementation in this contract.

      ## Interface and failure boundaries

      - `render_role_prompt(&serde_json::Value) -> Result<String, RunnerError>`: canonical JSON is the existing engine's transport boundary, not a new persistence model. Required root fields are task_id, goal, allowed_write_scope and workflow_guidance.expected_evidence_template plus a string prepared role prompt. Do not re-expand inserted braces. Preserve original claim JSON as context.
      - `spawn_role_runner(&RunnerConfig, &RunnerInvocationContext, RunnerSessionPolicy) -> Result<RunnerProcess, RunnerError>`: `RunnerSessionPolicy::{Resume, Fresh}`; opaque uniquely owned process, observable `id()`. Blank Resume identity, malformed/empty quoting and invalid argv fail closed. Template grammar is POSIX-like words, whitespace, single/double quotes and backslash escapes; expansion/globbing/pipelines/redirection are not implicit. A caller can explicitly configure a shell executable and static script, with runtime data in separate arguments. Preserve empty arguments, Unicode, quotes, newlines, dollar/backtick text and placeholder-looking substituted values as data.
      - `monitor_runner_process(RunnerProcess, Duration, &mut dyn FnMut(RunnerStream, &[u8]) -> io::Result<()>) -> Result<RunnerOutcome, RunnerError>`: `RunnerStream::{Stdout, Stderr}`. Outcome carries process_id, session_fingerprint, exit_code: Option<i32>, timed_out. Nonzero/signal child termination is an outcome, not fabricated gate success or an infrastructure error. Callback receives raw bytes in stream order, without requiring newline/UTF-8 and before process completion; caller owns accumulation/logging so spawner need not retain unbounded output.
      - Concurrent pipe readers feed a bounded channel; monitor services both while checking deadline even under sustained output. Lifetime includes pipe EOF after leader exit: descendants retaining pipes cannot cause an unbounded join. Timeout sends SIGKILL to negative pgid even if leader already exited, waits/reaps the direct child, drains/closes readers, and returns timed_out. Child/pipe/callback errors and dropping the owned handle must kill the owned group and reap the direct child before abandoning it. Surface typed InvalidCommand/InvalidSession/InvalidContext/Io boundaries; retain the underlying I/O error. Unix-only process behavior; unsupported platforms fail explicitly.
      - Reap every directly owned Child with wait; prove ECHILD on a subsequent waitpid. Terminate descendants in the group and prove none remains running. Adoption/reaping of orphaned grandchildren is an OS/init responsibility; do not introduce process-global subreaper mutations into parallel tests or claim userspace can wait for nonchildren. No global env mutation, retry loop, credential logging, dependency/manifest/build.rs change or implicit shell.
      - Contract source contains only necessary type/signature declarations, the handle id projection, and explicit io::ErrorKind::Unsupported stubs. These represent genuinely missing capabilities. No artificial panic, manufactured process failure, fake output, or success implementation. Builder replaces these stubs and supplies ownership/Drop/supervision behavior.

      ## Proof map

      All assertions live in dedicated test functions. Reusable constructors/capture/readiness helpers have at least two callers; no test duplicates Git/SQLite lifecycle setup. Real-process tests enter the production spawn/monitor seam using validated profile projection and a local Python standard-library child fixture; Python is an external child, not a replacement for the process boundary under test. Physical files and process observation are the admitted test subject, not syntax-test setup.

      | Claim | Focused test / scenario | Evidence / sensitivity break |
      | --- | --- | --- |
      | A1 command safety + group | runner_preserves_arguments_and_uses_an_isolated_process_group | Real child echoes JSON argv/cwd/pid/pgid. Values include shell syntax, whitespace, Unicode, newlines and nested placeholders. Require no marker side effect, exact argv and pgid=pid distinct from test group. Detect render-before-split or shell execution and missing process_group(0). Selected red gate seam. |
      | A1 input errors | runner_rejects_malformed_command_templates; runner_rejects_blank_resume_session; runner_reports_spawn_failure | Tables for same malformed command unit; typed errors and no unintended side effect. Detect permissive tokenization, silent session fallback or hidden executable failures. |
      | Stream logging subtask | monitor_delivers_both_streams_before_child_exit | Child emits more than pipe capacity to both streams and waits for callback acknowledgment. Assert exact bytes, callback-before-exit, normal status. Detect sequential reads or buffering until exit. |
      | A2 owned lifetime | monitor_times_out_and_reaps_the_owned_process_group | Real forked descendant ignores SIGTERM. Timeout must kill group, return timed_out, show no running descendant and ECHILD for owned direct child. Detect leader-only kill or missing wait. |
      | A2 inherited pipes | monitor_bounds_descendant_pipes_after_leader_exit | Leader exits but descendant retains pipes; deadline still terminates/drains group. Detect wait-before-drain hangs and missing group cleanup after leader exit. |
      | A2 I/O failure/cancellation | monitor_cleans_up_when_output_sink_fails; dropping_runner_reaps_its_child | Fallible callback and owned-handle drop must release/reap child. Detect detached/leaked process ownership. |
      | A3 sessions | runner_resumes_builder_identity_and_isolates_fresh_sessions | Real child env + argv readback for Resume then two Fresh invocations, all fingerprints observed, parent environment unchanged. Detect inherited/stable fresh identity or loss of Resume identity. |
      | Prompt subtask | role_prompt_preserves_prepared_text_and_complete_gate_context; role_prompt_rejects_incomplete_claim_authority | Prepared prompt copied unchanged and canonical claim data roundtrips including scope, target, expected evidence, rejection. Malformed-field table for the same context-validation unit. Detect dropped authority or recursive rendering. Supplemental integration-boundary proof, not a substitute for real-process tests. |
      | Child exit reporting | monitor_preserves_nonzero_exit_status | Real child exit 7 is an observed outcome with timed_out=false. Detect interpreting all child exits as success or losing exit state. |

      No benchmarks or latency SLOs are introduced. Fixture watchdogs and bounded readiness waits prevent broken implementations from leaving infinite child processes; they are test cleanup, not production outcome fakes. Invalid paths and non-Unix behavior remain builder responsibilities through typed IO/unsupported errors; no cross-platform execution proof is claimed on this Linux host. Durability/restart/SQLite/remote transport are inapplicable to this in-memory POSIX process primitive.

      ## Validation and status

      - Blueprint recorded before source/test edits; scope extension approved via CLI with genuine session.
      - Planned registered proof command: `contextunity-forge-mcp task verify <task> --test tests/acdd/orchestrator/spawner.rs::runner_preserves_arguments_and_uses_an_isolated_process_group`.
      - Planned raw corroboration: scoped Cargo invocation to retain the actual assertion, compile result, counts and exit code.
      - Runtime red / accepted candidate / final task state: pending.
      - Build obligation: all focused tests green, delivered bootstrap tests unchanged, strict Clippy per TESTS.md; independent review and delivery are subsequent roles. No subtasks marked completed from red stubs.
    - 'decisions: Clarification to blueprint #1633 before contract submission: the opaque RunnerProcess exposes session_fingerprint() as well as id(), so the downstream lifecycle owner can record RunnerStarted with the resolved fresh identity before monitor blocks. The selected seam checks that handle identity equals the final outcome identity. No process behavior implemented; only the necessary private String field and read-only accessor were declared. Descendant-liveness readback uses bounded readiness polling to avoid assuming SIGKILL scheduling is synchronous. Scope/acceptance/policy unchanged.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Runner command templates are interpolated safely and spawned in a new process group via `setpgid(0, 0)`.
2. Process timeouts trigger clean process group termination (`kill(-pgid, SIGKILL)`) without zombie subprocesses.
3. Builder sessions can be resumed via session ID, while reviewer sessions receive a distinct fresh session fingerprint via child-process environment overrides.

---

### task: headless-task-lifecycle-and-rejection-router

```yaml
task_ref: headless-task-lifecycle-and-rejection-router
target: Implement CLI task run command advancing a task through active profile gates with rejection routing and subtask completion guard
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
- tests/acdd/orchestrator/lifecycle.rs
- tests/acdd/orchestrator/lifecycle/harness.rs
- tests/acdd/orchestrator/lifecycle/role_fixture.py
- crates/forge-tasks/src/engine/orchestrator/spawner.rs
- src/cli/mod.rs
symbols:
- fn:run_task_lifecycle
- fn:handle_gate_rejection
status: completed
subtasks:
- subtask_ref: task-run-gate-progression
  title: Iterate across active task gates, triggering runner execution on role gates and zero-token verification on command gates
  status: completed
  evidence: 'Complete task run CLI and profile-driven persisted cursor loop implemented; src/cli/mod.rs admitted under #1648/#1649 and exhaustive response-title arm integrated. cargo test --test acdd orchestrator::lifecycle::: 11 passed, 0 failed, 0 ignored, including task_run_cli_accepts_in_place_and_uses_lifecycle. Command-only gates and autonomous delivery use normal zero-argument submission; role+command gates still run the role. Spawner 18/0/0, bootstrap 8/0/0; cargo clippy --all-targets --all-features -- -D warnings exit 0.'
- subtask_ref: rejection-routing-feedback-injection
  title: Route review rejections according to reject_routing/reject_to and feed structured findings to the next builder turn
  status: completed
  evidence: 'Full lifecycle suite including CLI smoke: 11 passed, 0 failed, 0 ignored. Real role process and real CLI submissions prove scope-to-build and symbols-to-contract routing, exact structured findings, target worker/session reuse and persisted rejection restart. Uses existing guidance producer and gate graph, no second routing authority. Spawner 18/0/0, bootstrap 8/0/0, full strict all-target Clippy exit 0. Prior CLI scope blocker resolved by #1648/#1649.'
- subtask_ref: delivery-subtask-completion-guard
  title: Validate that all declared subtasks are completed (with evidence) or cancelled, failing closed with TASK_SUBTASKS_INCOMPLETE or TASK_SUBTASK_EVIDENCE_MISSING
  status: completed
  evidence: Existing delivery guard unchanged. Full lifecycle suite 11/0/0 proves ready, in_progress, blocked, legacy pending, completed-with-missing/empty/whitespace-evidence rejection without candidate/receipt/HEAD/manifest mutation. Primary fixture accepts cancelled subtasks. CLI in-place smoke passes; spawner 18/0/0 and bootstrap 8/0/0; full strict all-target Clippy exit 0.
- subtask_ref: delivery-snapshot-and-receipt
  title: Finalize delivery gate automatically using existing candidate snapshot and receipt persistence
  status: completed
  evidence: Full lifecycle suite 11/0/0, including CLI smoke, proves durable completed status, receipt commit matching accepted candidate content, manifest receipt/completion, completed rerun idempotence and candidate-drift refusal. Existing candidate/receipt/submit paths reused without guard or proof weakening. Spawner 18/0/0, bootstrap 8/0/0, full strict all-target Clippy exit 0. Only isolated test fixtures were delivered; project review/delivery remains outside this build.
extended_scope:
- tests/acdd/orchestrator/lifecycle.rs
- tests/acdd/orchestrator/lifecycle/harness.rs
- tests/acdd/orchestrator/lifecycle/role_fixture.py
- crates/forge-tasks/src/engine/orchestrator/spawner.rs
- src/cli/mod.rs
receipt:
  delivery_gate: deliver
  candidate_gate: build
  contract_revision: 1
  passed_at: 2026-11-09T15:57:09.031664559+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/acdd/orchestrator/lifecycle.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/AC-003-headless-orchestrator-and-role-runners.md
        PASS rust-check-and-test::tests/acdd/orchestrator/lifecycle.rs: exit=0 passed=1 failed=0 (3874 ms)
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All nine accepted candidate blobs match current files, with baseline HEAD 50d99e88caf801f4239d3d061ff74b65c70a010d. Full source/test/doc diff reviewed. Five extended paths directly support lifecycle seam tests, pre-claim session selection and the exhaustive CLI Run presentation arm admitted by #1648/#1649. No protected config/store edits. Starting index, status, branch and preexisting milestone metadata remain unchanged.'
        claims:
          applicable: true
          evidence: 'Milestone acceptance 1-4 and blueprint #1641/#1643 traced through production CLI -> run_task_lifecycle -> canonical profile/cursor -> claim -> complete internal context -> real spawner -> child verify/submit -> persisted gate evidence. Agent driver precedence is preserved. Accepted rejections and structured findings drive retained target worker/session lookup, including persisted restart. Command-only gates and delivery call existing zero-argument verification/submission. Existing subtask guard, candidate drift checks and receipt pipeline remain unchanged. cargo test --test acdd orchestrator::lifecycle::: exit 0; 11 passed, 0 failed, 0 ignored.'
        concurrency:
          applicable: true
          evidence: No new durable state or transition authority. Role claims bind prepared identity before spawning; parent environment remains unchanged and child submissions use that identity. Store handles are dropped before process monitoring; no transaction/lock is held across the child lifetime. Foreign active role claims are preserved; mechanical resumption requires matching worker, session and canonical worktree. Exit zero alone cannot advance; persisted submission for the exact claim is required. Real timeout, failure, rejection, active-claim and restart seams pass. Existing owned POSIX group supervision and bounded dual stream readers are preserved.
        project_isolation:
          applicable: true
          evidence: 'Registry ownership and canonical worktree checks precede execution; normal claim and submit enforce pinned authority, scope, namespace and snapshot boundaries. Read-only claim/session queries bind the exact task and claim revision with SQL parameters. No new profile path discovery, global environment mutation, Git history operation, cross-project write or shell interpolation. CLI uses selected canonical root. cargo test --test acdd orchestrator::spawner::: 18 passed/0 failed/0 ignored; cargo test --test acdd orchestrator::types::: 8 passed/0 failed/0 ignored; both exit 0.'
        administration:
          applicable: true
          evidence: 'Accepted contract claim 7 and build claim 12 at contract revision 1 verified through complete task inspect; all four subtasks completed with evidence. Review claim 27 uses genuine independent session 01a1bdd1-3650-7c50-9ba7-5bf4cdd6f747, distinct from builder session. No manual proof/receipt/guard bypass. Strict cargo clippy --all-targets --all-features -- -D warnings exited 0 with zero warnings; git diff --check exited 0. CLI reference matches implemented behavior. Shared skill usage sync is already routed to admitted guide owner in #1645. No authority-grounded blocking findings.'
      extended_scope_review:
        approved_paths:
        - tests/acdd/orchestrator/lifecycle.rs
        - tests/acdd/orchestrator/lifecycle/harness.rs
        - tests/acdd/orchestrator/lifecycle/role_fixture.py
        - crates/forge-tasks/src/engine/orchestrator/spawner.rs
        - src/cli/mod.rs
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-HEADLESS-ROLE-RUNNER-ORCHESTRATION: When roles declare runner and prompt_template, Forge CLI (task run / milestone run) autonomously claims gates, renders role prompts with gate context bundles, spawns subagent runner processes in isolated POSIX process groups with distinct session fingerprints, executes command and delivery gates with zero tokens, and routes review rejections via reject_routing until completion.'
    - 'INV-INDEPENDENT-REVIEWER-SESSION: Reviewer runners execute with an isolated session identity and fingerprint distinct from the builder runner session, satisfying reviewer separation and proof independence invariants.'
    - 'INV-REJECT-ROUTING-FEEDBACK-LOOP: When review gates reject with structured findings, the orchestrator routes execution back to the configured upstream stage (scope -> build, symbols -> contract) and passes the review findings into the resumed builder session without cold restarts.'
    - 'INV-TOPOLOGICAL-WAVE-EXECUTION: Milestone run executes tasks in topological wave order, collecting and verifying evidence envelopes automatically and gating subsequent waves until dependencies complete.'
    - 'INV-IN-PLACE-EXECUTION: Task, milestone, and plan orchestration supports execution directly in the current workspace (--in-place) without mandatory worktree cloning or branching when isolated worktrees are not requested.'
    - 'INV-PLAN-WORKFLOW-ORCHESTRATION: The orchestrator supports standalone workflows (docs/workflows/*.yaml) via contextunity-forge-mcp workflow [list|show|run] and structured plans (doc_type: plan in docs/plans/*.md) via contextunity-forge-mcp plan [list|show|run], executing in-place under universal gate governance.'
    - 'INV-DELIVERY-SUBTASK-COMPLETION-GUARD: Mechanical delivery verification fails closed with TASK_SUBTASKS_INCOMPLETE if any declared subtask under the active task or plan step is not completed or cancelled, and with TASK_SUBTASK_EVIDENCE_MISSING if any completed subtask lacks non-empty verification evidence.'
    - 'INV-CLI-UNIFIED-ARGUMENTS: Global CLI and orchestrator execution accept explicit path parameters (--root, --db, --config for custom forge-mcp.yaml, and --profile for custom profile.yaml) with deterministic resolution order.'
    - 'INV-PLAN-WORKFLOW-HARNESS: All tests for structured plans (docs/plans/*.md) and standalone workflows (docs/workflows/*.yaml) must use PlanHarness and WorkflowHarness with canonical templates in tests/acdd/fixtures/plans/ and workflows/. Ad-hoc markdown heredocs and subprocess CLI spawning are strictly banned.'
    architectural_notes:
    - |
      decisions: Architectural synthesis: revision 1 / claim 7. Genuine session 01a1c08b-4515-7e72-9466-b989c7b20048. Baseline 50d99e88caf801f4239d3d061ff74b65c70a010d, initially clean. Read current authority, claim workflow, synthesis #1618, spawner blueprint #1633, repository instructions and test-contract-seam.

      DECISION: a synchronous thin task_runner drives the existing persisted cursor. No new task/gate tables, registries, proof authority, hardcoded gate sequence or orchestration checkpoints. Public run_task_lifecycle(root, task_id, TaskRunOptions, event_observer) returns the authoritative task result including snapshot/receipt. Options carry worktree, coordinator worker/fingerprint and runner timeout. OrchestratorEvent is observation only. CLI task run TASK --in-place uses current CLI root resolution. The subsequent wave task consumes this API and owns worktree provisioning, global argument extensions and wave/handoff.

      REAL PATH: CLI -> lifecycle claim_inner/finish_claim -> pinned GateProfile/cursor -> TasksStore::claim/checkpoint -> context_bundle/workflow_guidance -> spawner render/spawn/monitor -> child task verify/submit -> evidence_compiler/submit_inner -> gates::submit_with_pre_persist -> durable gates/findings/transitions/candidate/receipt. Reinspect after accepted transitions. GateDef::driver gives role precedence over command. Do not auto-pass a role gate merely because it names a command. Command-only gates use existing verification/evidence compilation/submission. Delivery proof is mechanical even if its gate names a reviewer role. Traverse canonical graph, conditions, pass/fail and max_attempts. Completion requires durable completed status and receipt, never child exit zero.

      IDENTITY: claims and child submissions share the selected fingerprint. Spawner currently selects Fresh identity inside spawn, after claim context is needed. Approved spawner scope permits extracting its existing entropy selection into a small prepare-session API during Build: select before claim, spawn with that bound identity. Preserve literal argv transport, isolated process group, timeout, drainage and cleanup. Do not copy entropy or use coordinator fingerprint for an independent reviewer. Reuse-on-reject recovers the prior accepted target-stage worker/session from task_claims by accepted claim revision. Build target resumes builder; contract target resumes its accepted contract worker. Missing prior identity fails closed. A store/lifecycle.rs scope extension was denied as outside admitted roots and is NOT authorized. Use an engine-owned read-only lookup of the existing claim through its store connection; no store edit or new registry. Restart derives cursor, sessions and findings from persisted authority. Completed invocation is idempotent. Do not steal or release another in-progress claim.

      REJECTION: handle_gate_rejection consumes accepted rejection records, task_findings and workflow_guidance.rejection. gate_graph::resolve_failure_reference/resolve_fail_target remain routing authority: scope/symbols, configured fail/reject_to fallback and exhaustion. No second routing table or stdout prose parsing. Preserve exact structured findings in the next role context. Emit GateRejected only for a persisted rejection. Runner error, timeout or no accepted progress stops without invented proof; spawner owns cleanup.

      DELIVERY: manifest engine/submit.rs is obsolete. Actual owners are engine/mod.rs::submit_inner and engine/gates/submit.rs::submit_with_pre_persist. They already check completed/cancelled subtasks and nonblank completed evidence before receipt writing/commit callback. Prove unchanged through lifecycle and direct submission; do not duplicate the guard or create the obsolete facade. Reuse candidate/baseline, drift validation, commit/receipt rollback and idempotence. No live .forge, Cargo.toml, build.rs or history changes. Existing isolated canonical test profile/store/Git infrastructure is permitted.

      PROOF DRAFT: tests in registered orchestrator/lifecycle.rs; domain harness composes TaskHarness::disk_with_profile, MilestoneFixtureBuilder and canonical command runner fixture. A protocol-faithful external role fixture substitutes only the unavailable LLM: invokes real task CLI verify/submit, uses canonical typed reviewer proof, records session/prompt, and can withhold submission or exit. No fake engine, command proof, store, snapshot or claim. Cover mixed role/command/delivery progression; persisted restart/idempotence; scope/symbol rejection routing with exact structured feedback/session reuse; exit zero without submission and process error/timeout; ready/in_progress/blocked/pending and completed missing/empty/whitespace proof rejection with nonmutation; cancelled acceptance; receipt commit/accepted snapshot; drift refusal; CLI --in-place ingress. Distinct behavior gets a dedicated test. Runtime-red is actual missing orchestration returning Unsupported from a minimal compiling signature stub, not a panic or fabricated command failure. Execute scoped Cargo, refresh graph, then Forge verify --test exact ref and CLI Contract pass with this genuine identity. Record green existing behavior honestly.

      SENSITIVITY: omit loop -> progression fails; wrong driver precedence -> missing role invocation; exit-zero-as-proof -> stall test fails; fresh-on-reject/drop-findings -> recorded session/payload differs; omit guard -> unfinished delivery succeeds; skip candidate validation -> drift succeeds; bypass submission -> no accepted receipt/candidate. Build must prove these green; Contract proves the current missing orchestration red. No existing green implementation is removed.

      HANDOFF: implement task_runner/CLI glue and approved additive identity preparation; preserve delivered process semantics and existing guard. Reconcile docs/reference/cli.md during Build after reading documentation owner instructions. No Build/Review/Delivery or main landing in this role.
    - 'decisions: Blueprint refinement from observed real protocol execution: canonical verbose review context can be byte-limited by lifecycle::bound_task_response; delivered render_role_prompt correctly refuses incomplete authority. Build must obtain the complete context through the existing internal context_bundle producer (same pinned task/profile, no clearing omission flags or fabricating fields), or fail closed with its actual diagnostic. The focused external-agent fixture uses a valid compact role guidance profile so its complete claim envelope is transportable. A real claim -> prompt -> process -> real CLI verify/submit baseline test now passes across configured role gates, proving the fixture is not a fake engine. No production behavior was weakened. CLI run smoke uses a dedicated binary subprocess to set its isolated coordinator session without process-global env mutation; engine lifecycle tests stay in-process. Guard acceptance of cancelled subtasks is covered by the primary delivery fixture; no duplicate standalone cancelled test is needed. Foreign active-claim nonmutation has a dedicated lifecycle test. All product-loop assertions remain red until Build.'
    - 'deferred: Genuine builder session 01a1c09d-0043-7601-b745-a82481fcbfe7, CLI claim 12. CLI documentation updated within admitted docs/reference/cli.md. docs/AGENTS.md also requests shared skill synchronization for CLI changes; Forge CLI extend-scope skills/contextunity-forge/SKILL.md was rejected TASK_SCOPE_INVALID outside admitted roots. No skill edit made. Route that bounded task-run usage synchronization to the admitted forge-guide-lifecycle-gates-and-subtasks owner/milestone documentation scan; CLI reference remains current authority. Initial compile exposed exhaustive response-title matching in src/cli/mod.rs requiring a single new Run match arm; requesting admitted-root scope extension rather than a wildcard or bypass.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
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
target: Implement CLI milestone run, plan [list|show|run], and workflow [list|show|run] with wave scheduling and global CLI path options
proof_policy: seam-test-first
contract_revision: 4
depends_on:
- headless-task-lifecycle-and-rejection-router
scope:
- crates/forge-tasks/src/engine/orchestrator/mod.rs
- crates/forge-tasks/src/engine/orchestrator/waves.rs
- crates/forge-tasks/src/milestones/mod.rs
- src/cli/milestone.rs
- src/cli/plan.rs
- src/cli/workflow.rs
- docs/reference/cli.md
- docs/reference/acdd.md
- tests/acdd/orchestrator.rs
- tests/acdd/fixtures/plans/
- tests/acdd/fixtures/workflows/
- tests/acdd/harness/plans.rs
- tests/acdd/harness/workflows.rs
- src/cli/mod.rs
- tests/acdd/orchestrator/waves.rs
- crates/forge-tasks/src/milestones/command_gate.rs
- crates/forge-tasks/src/engine/orchestrator/workflow.rs
- crates/forge-tasks/src/engine/orchestrator/spawner.rs
- crates/forge-tasks/src/engine/orchestrator/plan.rs
- src/cli/paths.rs
- crates/forge-tasks/src/milestones/handoff/verification.rs
- crates/forge-tasks/src/milestones/handoff/mod.rs
- crates/forge-tasks/src/milestones/integration.rs
- crates/forge-tasks/src/engine/orchestrator/types.rs
- crates/forge-tasks/src/engine/orchestrator/role.rs
- crates/forge-tasks/src/tasks/gates.rs
- crates/forge-tasks/src/engine/gates/submit.rs
- crates/forge-tasks/src/engine/mod.rs
- crates/forge-tasks/src/engine/workspaces.rs
- crates/forge-tasks/src/engine/lifecycle.rs
- crates/forge-tasks/src/engine/blackboard.rs
- crates/forge-tasks/src/engine/evidence_compiler.rs
- crates/forge-tasks/src/tasks/configuration.rs
- crates/forge-tasks/src/tasks/mod.rs
- crates/forge-tasks/src/tasks/profile/mod.rs
- crates/forge-tasks/src/engine/gates/scope.rs
- tests/acdd/orchestrator/waves/harness.rs
- src/mcp/server.rs
- src/engine/pipeline.rs
- src/mcp/tools.rs
- src/cli/ast.rs
- crates/forge-tasks/src/milestones/synthesis.rs
- src/cli/validate.rs
- src/cli/guide.rs
- tests/acdd/runner/transition_runner.rs
- src/cli/task.rs
symbols:
- fn:run_milestone_waves
- fn:execute_wave_verification
- fn:run_plan_workflow
- fn:run_standalone_workflow
status: completed
subtasks:
- subtask_ref: topological-wave-scheduler
  title: Schedule and dispatch milestone tasks in wave order respecting depends_on dependencies
  status: completed
  evidence: 'Claim98: R1 remains independently confirmed fixed in review95 and unchanged in this repair. Final ACDD302/0 includes real FF-refusal/zero-successor-claims restart regression in isolated and in-place modes, preserved receipts/worktrees, wave dependency ordering and normal integrated handoff. Strict all-target/all-feature Clippy exit0.'
- subtask_ref: in-place-plan-and-workflow-runners
  title: 'Implement plan [list|show|run] (save: snapshot) and workflow [list|show|run] (save: none) in-place execution'
  status: completed
  evidence: Claim98 final full ACDD302 passed/0failed includes canonical plan/workflow list/show/run, snapshots/save:none, structured role decisions/rejection and immutable/subtask guards. Plan/workflow implementation unchanged from build86. Current repair only wave journal composition; strict all-target/all-feature Clippy exit0.
- subtask_ref: plan-and-workflow-harnesses-and-fixtures
  title: Build PlanHarness and WorkflowHarness with canonical static fixtures in tests/acdd/fixtures/plans/ and workflows/ according to INV-PLAN-WORKFLOW-HARNESS
  status: completed
  evidence: Claim98 adds dedicated real WaveHarness partial-prefix/third-restart, failed retry-budget and corrupt durable-route tests. Existing canonical PlanHarness/WorkflowHarness fixtures and production execution preserved. No ad-hoc plan/workflow markdown or substitute engine. Final ACDD302/0 and strict all-target Clippy0.
- subtask_ref: plan-and-workflow-documentation-reconciliation
  title: Reconcile documentation in docs/reference/cli.md and docs/reference/acdd.md covering plan run and workflow run execution
  status: completed
  evidence: Claim98 restores existing documented bound-root-decision resume behavior for retained prefix plus fresh suffix. CLI/acdd reference bytes preserved; their existing contract remains accurate, so no documentation edit required. Documentation check173files statusok; git diff --check0, final ACDD302/0 and strict all-target Clippy0.
- subtask_ref: wave-barrier-and-autonomous-handoff
  title: Execute milestone verification commands and invoke milestone handoff when all tasks pass
  status: completed
  evidence: 'Claim98 R2 incomplete remediation repaired in canonical transition journal: fresh suffix links to the exact reused/observed predecessor row, reconstructing complete route without deleting audit history or rebinding proof. Exact third-unchanged-restart regression and failed/pass retry-budget route both fail before fix (compile0/runtime101, archive8ca125c36d00038e) and pass after. Corrupt-link fail-closed test and whole-prefix committed/uncommitted/unchanged/delivery-resume neighbors pass. Focused milestone17/0; final ACDD302/0, strict all-target Clippy0.'
- subtask_ref: global-cli-path-resolution
  title: Support deterministic --root, --db, --config, and --profile CLI arguments across commands
  status: completed
  evidence: Claim98 immutable selected config/profile, server lifetime, delta selection and in-process CLI owners unchanged byte-for-byte from build86. Current full ACDD302/0 includes selected-policy, configuration propagation and concurrency regressions. Strict all-target/all-feature Clippy exit0. No later identity/independence-task change.
extended_scope:
- src/cli/mod.rs
- tests/acdd/orchestrator/waves.rs
- crates/forge-tasks/src/milestones/command_gate.rs
- crates/forge-tasks/src/engine/orchestrator/workflow.rs
- crates/forge-tasks/src/engine/orchestrator/spawner.rs
- crates/forge-tasks/src/engine/orchestrator/plan.rs
- src/cli/paths.rs
- crates/forge-tasks/src/milestones/handoff/verification.rs
- crates/forge-tasks/src/milestones/handoff/mod.rs
- crates/forge-tasks/src/milestones/integration.rs
- crates/forge-tasks/src/engine/orchestrator/types.rs
- crates/forge-tasks/src/engine/orchestrator/role.rs
- crates/forge-tasks/src/tasks/configuration.rs
- crates/forge-tasks/src/tasks/mod.rs
- crates/forge-tasks/src/tasks/profile/mod.rs
- crates/forge-tasks/src/engine/gates/scope.rs
- tests/acdd/orchestrator/waves/harness.rs
- src/mcp/tools.rs
- src/cli/ast.rs
- crates/forge-tasks/src/milestones/synthesis.rs
- src/cli/validate.rs
- src/cli/guide.rs
- tests/acdd/runner/transition_runner.rs
- src/cli/task.rs
receipt:
  delivery_gate: deliver
  candidate_gate: build
  contract_revision: 4
  passed_at: 2026-11-09T20:01:18.726207742+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/acdd/orchestrator/waves.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/AC-003-headless-orchestrator-and-role-runners.md
        PASS rust-check-and-test::tests/acdd/orchestrator/waves.rs: exit=0 passed=1 failed=0 (372 ms)
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All 68 current candidate blobs/modes exactly match live files and snapshot path sets are unchanged. Only wave owner and its focused tests/harness differ from rejected eea896f8; 65/68 blobs retained exactly. Inventoried full admitted scope and 24 extensions against revision4 authority/transfers; changes restore admitted restart behavior. Protected files and unrelated/later task unchanged; HEAD/base, branch, main, index, complete status and manifest bytes unchanged before review; git diff --check0.
        claims:
          applicable: true
          evidence: Complete task acceptance and production seams reviewed against baseline2fbda3cf and prior candidate lineage. R1 and R2 independently confirmed-fixed; no open authority-backed findings. Exact prior-red partial-prefix scratch now passes third unchanged restart. Current ACDD302/0 (waves49,lifecycle11,spawner18,types8), MCP138/0, adapters4/0, CLI-roots7/0; strict Clippy0/warnings0. Seven current-library scratch probes pass. Detailed cumulative closure/mutation evidence in closure-ledger.json under /tmp/ac003-waves-review3-_6wmor4b.
        concurrency:
          applicable: true
          evidence: Actual isolated delivery followed by competing integrated-root commit and FF refusal remains fenced on isolated/in-place restart; no successor claims, receipt/owned HEAD preserved, normal merge recovery/handoff succeeds. Journal links actual persisted row IDs to last consumed valid observation; failed observations remain in replay route and consume canonical retry budgets. Independent unchanged third/fourth restart preserves byte-exact old/new journal evidence and produces no additional effects or rows. No alternate graph, schema or global cache.
        project_isolation:
          applicable: true
          evidence: Current scoped journal query confines predecessor resolution to milestone/subject/orchestrator rows. Real foreign-milestone predecessor existing in SQLite is refused before effects; cyclic/non-earlier/missing links fail closed. Owned worktrees, config/profile/db rebasing, subprocess isolation and server/delta paths retain prior reviewed semantics and current domain regressions. All reviewer mutations are in disposable fixture repositories or scratch files under /tmp.
        administration:
          applicable: true
          evidence: Active profile graph, proof/pin/baseline/receipt guards and shared subtask completion guard retain authority. Replay checks original gate/profile/authority/content bindings; legacy unlinked accepted payloads read without rewrite or new binding. Actual role session/findings restore and bound independent review/verification/handoff consumers retained; missing/stale proof tests pass. Docs already describe restored restart behavior. Genuine reviewer session distinct from builder is used for CLI claim107; task-level pass only, later identity task unchanged.
      extended_scope_review:
        approved_paths:
        - src/cli/mod.rs
        - tests/acdd/orchestrator/waves.rs
        - crates/forge-tasks/src/milestones/command_gate.rs
        - crates/forge-tasks/src/engine/orchestrator/workflow.rs
        - crates/forge-tasks/src/engine/orchestrator/spawner.rs
        - crates/forge-tasks/src/engine/orchestrator/plan.rs
        - src/cli/paths.rs
        - crates/forge-tasks/src/milestones/handoff/verification.rs
        - crates/forge-tasks/src/milestones/handoff/mod.rs
        - crates/forge-tasks/src/milestones/integration.rs
        - crates/forge-tasks/src/engine/orchestrator/types.rs
        - crates/forge-tasks/src/engine/orchestrator/role.rs
        - crates/forge-tasks/src/tasks/configuration.rs
        - crates/forge-tasks/src/tasks/mod.rs
        - crates/forge-tasks/src/tasks/profile/mod.rs
        - crates/forge-tasks/src/engine/gates/scope.rs
        - tests/acdd/orchestrator/waves/harness.rs
        - src/mcp/tools.rs
        - src/cli/ast.rs
        - crates/forge-tasks/src/milestones/synthesis.rs
        - src/cli/validate.rs
        - src/cli/guide.rs
        - tests/acdd/runner/transition_runner.rs
        - src/cli/task.rs
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-HEADLESS-ROLE-RUNNER-ORCHESTRATION: When roles declare runner and prompt_template, Forge CLI (task run / milestone run) autonomously claims gates, renders role prompts with gate context bundles, spawns subagent runner processes in isolated POSIX process groups with distinct session fingerprints, executes command and delivery gates with zero tokens, and routes review rejections via reject_routing until completion.'
    - 'INV-INDEPENDENT-REVIEWER-SESSION: Reviewer runners execute with an isolated session identity and fingerprint distinct from the builder runner session, satisfying reviewer separation and proof independence invariants.'
    - 'INV-REJECT-ROUTING-FEEDBACK-LOOP: When review gates reject with structured findings, the orchestrator routes execution back to the configured upstream stage (scope -> build, symbols -> contract) and passes the review findings into the resumed builder session without cold restarts.'
    - 'INV-TOPOLOGICAL-WAVE-EXECUTION: Milestone run executes tasks in topological wave order, collecting and verifying evidence envelopes automatically and gating subsequent waves until dependencies complete.'
    - 'INV-IN-PLACE-EXECUTION: Task, milestone, and plan orchestration supports execution directly in the current workspace (--in-place) without mandatory worktree cloning or branching when isolated worktrees are not requested.'
    - 'INV-PLAN-WORKFLOW-ORCHESTRATION: The orchestrator supports standalone workflows (docs/workflows/*.yaml) via contextunity-forge-mcp workflow [list|show|run] and structured plans (doc_type: plan in docs/plans/*.md) via contextunity-forge-mcp plan [list|show|run], executing in-place under universal gate governance.'
    - 'INV-DELIVERY-SUBTASK-COMPLETION-GUARD: Mechanical delivery verification fails closed with TASK_SUBTASKS_INCOMPLETE if any declared subtask under the active task or plan step is not completed or cancelled, and with TASK_SUBTASK_EVIDENCE_MISSING if any completed subtask lacks non-empty verification evidence.'
    - 'INV-CLI-UNIFIED-ARGUMENTS: Global CLI and orchestrator execution accept explicit path parameters (--root, --db, --config for custom forge-mcp.yaml, and --profile for custom profile.yaml) with deterministic resolution order.'
    - 'INV-PLAN-WORKFLOW-HARNESS: All tests for structured plans (docs/plans/*.md) and standalone workflows (docs/workflows/*.yaml) must use PlanHarness and WorkflowHarness with canonical templates in tests/acdd/fixtures/plans/ and workflows/. Ad-hoc markdown heredocs and subprocess CLI spawning are strictly banned.'
    architectural_notes:
    - |
      decisions: # AC-003 wave orchestration contract — evidence map

      Authority: forge-mcp/AC-003-headless-orchestrator-and-role-runners:milestone-wave-runner-and-autonomous-delivery, contract revision 2, claim 8, seam-test-first. Worker ac003-waves-contract-01a1c08b; genuine session 01a1c08b-4515-7e72-9466-b989c7b20048. Clean starting HEAD 2fbda3cf518932342c123ef6900b76a9ecaeb368. Lifecycle prerequisite delivered. This role stops after contract; runtime red is not implementation acceptance.

      Read authority: complete milestone, root/src/tests/tests-acdd/docs instructions, active claim guidance and profile synthesis, blackboard #1618/#1648, architecture/test-harness, ADRs 0015/0020/0021, roadmap/milestone direction and TESTS.md. Applied contextunity-forge, test-contract-seam, contract-planning, agent-instructions and code-quality. No preflight agents.

      ## Architectural decision

      The existing GateProfile/GateDef, gate_graph cursor/transition functions, Registry/TasksStore claims and task_runner::run_task_lifecycle remain authoritative. OrchestratorState/events are observations only. No new task registry, DAG algorithm, SQLite schema, hardcoded role/gate-name progression or synthetic task/milestone identities for taskless subjects.

      1. Wave scheduling consumes milestones::validate_wave_dag (synthesis.rs), including deterministic manifest-order ties and same-wave scope collision checks. Reload authoritative status and receipt before dispatch and after every completion; external dependencies still go through engine claim checks. No successor wave starts until all prerequisite tasks have accepted completion/receipt or explicit cancellation semantics admitted by the dependency engine. Cancellation must not silently satisfy a dependent task. In-place mode serializes writers within each wave. Isolated mode creates only uniquely owned task worktrees from the integrated milestone head, runs existing task lifecycle, then integrates accepted delivery commits serially before starting dependents. Never reset/rebase/force user branches, delete foreign/dirty worktrees or infer acceptance from exit zero. On error stop dispatch, drain/terminate owned children, preserve recoverable worktrees and accepted proofs. A serial isolated dispatcher is the smallest correct initial implementation; parallel speed is not an acceptance requirement.

      2. Root execution is a GateProfile graph cursor over existing pass/fail/when/max-attempt/rejection semantics. Root gates run once around the tasks container; collect_root_cursors deliberately excludes tasks, so dispatch waves at the container's root position. Resume from existing content/profile-bound root transitions/checkpoints; reject stale content rather than replaying side effects blindly. Root commands reuse milestone integration execution and observed trace parsing. execute_milestone_verification currently chooses the first applicable command and cannot be repeatedly called as if it advances the root graph; expose a cursor-bound variant using its existing validation/recording primitives. Agent gates use the delivered safe spawner and complete subject/gate context with explicit session identity. Independent review sessions must differ from all relevant builders; structured findings choose declared fail/reject routes; process exit alone is not a review decision.

      3. execute_wave_verification is a barrier/observed-root-command adapter, not another proof authority. It verifies accepted task receipts and root graph evidence for the integrated content. Autonomous handoff uses existing handoff_context -> typed IntegrationEvidence -> handoff with existing atomic recovery/pruning/receipt path. Bind authority, implementation, explicit target base and builders exactly. Collect observed registered-command counts, actual independent reviewer evidence and required documentation scan; edits invalidate verification/review and require refreshed binding. Never manufacture a reviewer or call handoff after only a successful command. The active repository root graph can route verify directly to documentation/handoff without visiting milestone_review: this does not waive the existing independent IntegrationEvidence requirement. Fail closed with an actionable missing current-content review diagnostic when that path lacks evidence; configured graph/proof requirements must be reconciled by their profile authority, never rewritten by this task. No live profile changes in this contract.

      4. Workflows are pure root graphs from docs/workflows/*.yaml, validated by the existing profile parser; forbid tasks containers. save:none uses memory-only observations and requires neither Git nor a task database. Do not call task-bound execute_command_gate with a fake ID. Factor the existing command resolution/process/output parsing into a taskless invocation path under the current milestones owner, with optional observation sink; durable task/milestone wrappers retain claim guards and trace binding. No third shell executor. Configured commands are trusted shell programs; interpolated paths/targets remain quoted data and runner templates retain parse-before-interpolate argv semantics. Timeouts, both stream drainage, bounded capture and cleanup remain real process boundaries. Malformed graph, missing/ambiguous selector, escaping/symlink path, absent runner, unaccepted role decision, timeout and exhausted attempts fail explicitly. list/show never execute gates or create stores.

      5. Plans are doc_type:plan Markdown steps in docs/plans, parsed by the production document extractor plus a small step projection. Resolve canonical step headings/dependencies with stable order, reject duplicate/unknown/cyclic dependencies and invalid scope before side effects. Consume the validated tasks-container graph only, execute in the current worktree without branches/worktrees, default snapshot persistence and no auto-commit. A terminal delivery proof may use save:snapshot: the existing hierarchical validator supports that shape, so no validator weakening is needed. Project the selected tasks block into plan execution; explicit plan profiles containing root synthesis/handoff or commit persistence are rejected rather than silently executing them. Reuse lower gate routing, snapshot primitives and subtask validation, factoring subject-independent validation in admitted engine owners if needed. Preserve cancelled-subtask allowance, incomplete/missing-evidence failures. Plan step status/evidence in its source document is authority; snapshots/checkpoints are observations bound to source/profile/content, not a second step registry. No fake milestone initialization solely to run a plan. Existing pure extraction constructors stay allocation-only; filesystem execution is explicit harness materialization.

      6. CLI ingress resolves typed immutable paths once: root explicit/env/current directory with existing positional-root conflict checks; config explicit path relative to resolved root (or absolute confined by existing adapter rules) before root default; profile explicit before selected config acdd_profile before repository/embedded default. Explicit missing paths fail, without fallback. --db remains the code-index database and must not be confused with config tasks_db (whose existing primary-worktree resolution/isolation must remain). Pass selected configuration/profile context to Registry/TaskSettings, root operations and child runner CLI invocations; retain backward-compatible default APIs and task profile pins. No process-global environment mutation or temporary config rewrites. Audit all five scanner::load_adapter(root,None) paths in src/cli/mod.rs and all task/milestone ingress, not just new commands. --adapter/global --config contradictions must be rejected deterministically. Concurrent calls with different paths must remain isolated.

      ## Owners, scope and implementation order

      Declared owners: engine/orchestrator/{mod,waves}.rs; milestones/mod.rs; CLI {mod,milestone,plan,workflow}.rs; docs/reference/{cli,acdd}.md; existing PlanHarness/WorkflowHarness and canonical fixture roots. Forge approved reacquisition of src/cli/mod.rs per #1648, and tests/acdd/orchestrator/waves.rs (the declared legacy tests/acdd/orchestrator.rs does not exist; never recreate a second test binary).

      Builder must request exact additional paths through Forge before editing them: engine/workspaces.rs and engine/mod.rs for explicit settings/context; engine/orchestrator/task_runner.rs and spawner.rs only if explicit child configuration transport needs an overload; engine/gate_graph.rs for shared cursor policy if needed; engine/git_snapshot.rs for subject-neutral snapshot access; milestones/{command_gate,integration}.rs and handoff/{mod,verification}.rs for shared taskless execution and cursor-bound root verification; small plan/workflow modules under engine/orchestrator/ and CLI path helper under src/cli/. All are admitted owner roots, but are not preemptively registered as unused contract changes. No Cargo.toml/build.rs/.forge edits. Any genuine required owner outside these roots needs authority, never an in-scope shadow implementation.

      Implementation sequence: (a) explicit resolved context with preserved defaults; (b) extract/reuse subject-neutral graph and command invocation policy without changing durable task wrappers; (c) workflow discovery/show/run memory sink; (d) plan step parser/tasks-only projection/snapshot persistence and subtask guard; (e) validated wave dispatcher/worktree ownership/barrier; (f) root graph evidence and autonomous bound handoff; (g) docs and focused negative/recovery tests. Preserve lifecycle delivery at 2fbda3cf and all existing harness tests. Coordinate any actual shared-file conflict through blackboard before writing; extend_scope cannot transfer an active owner's file. No requirement to alter core store/schema/profile catalogs.

      ## Claim coverage and sensitivity

      | Accepted claim | Contract scenario / production seam | Material readback and sensitivity | Evidence now |
      |---|---|---|---|
      | Workflow execution, save:none | workflow_run_executes_root_gates_without_task_state; in-process Cli::try_parse_from -> Cli::run_with_writer, WorkflowHarness canonical ordered.yaml | Exact ordered command effects; status completed; no Git/task DB/runtime ledger. Removing workflow dispatch must fail. | Planned authentic missing-ingress red; post-dispatch assertions remain builder obligations until green. |
      | Workflow list/show | workflow_discovery_does_not_execute_gates; same in-process caller/canonical harness | list/show identify source, preserve bytes and no command marker/store. Detect eager run on inspection. | Planned red. |
      | Plan run, in-place snapshots | plan_run_executes_steps_in_place_without_committing; same caller/PlanHarness linear.md + snapshot.yaml | Two step effects in order; status completed; HEAD unchanged, snapshot refs present, no worktree creation. Detect missing dispatcher/no snapshots/commit policy. | Planned red. |
      | Plan list/show | plan_discovery_preserves_canonical_step_dependencies; PlanHarness dag.md | Canonical step dependency content visible; HEAD and marker unchanged. Detect lost dependency parsing/eager execution. | Planned red. |
      | Global paths | workflow_run_honors_explicit_config_and_profile_paths; same production CLI/canonical workflow+config | Root-relative config points to selected profile but explicit profile wins; selected ordered effect and rejected default effects; explicit index path not task state. Detect ignored config/profile. | Planned red. |
      | Milestone waves + verification/handoff | Builder adds real MilestoneHarness execution against production run_milestone_waves and CLI milestone run, exact root graph/proofs | DAG diamond/manifest ties, out-of-order dependent barrier, scope collision/cycle/external blocked dependency, in-place/isolation, failure no successor, resume without duplicate delivery, integrated receipts, verifier failure/review rejection/documentation changes, stale binding and atomic handoff archive/prune. | Architectural contract and existing DAG/lifecycle/handoff collaborators traced; not claimed executed or proved by a workflow parser failure. |
      | Documentation | Builder reconciles CLI/acdd commands/flags/path precedence/persistence/errors/examples with real execution | Correct help/output and working canonical examples; no claims of functionality before green. Shared skill/example owner work needs admitted scope or explicit defer. | Blueprint; no documentation functionality claims changed in this contract. |

      Security/rejection/cleanup/restart/concurrency: production task/worktree/store ownership retained; plan/workflow no synthetic task authority. Builder must add dedicated canonical fixtures and tests for malformed/escaping subjects, nonzero/timeout, failed/retried graph edges, plan subtask guards, snapshot restart/source mismatch, foreign claims/worktrees and content-bound handoff. Alternate backend not applicable (one local CLI/engine); remote transport and performance benchmarking not admitted. No fake failures, unconditional panics, production fault toggles, mocks inside the seam or external CLI subprocesses for plans/workflows. Shell commands in a workflow are the accepted production subject, not a subprocess CLI test harness.

      ## Test changes and validation

      Before authoring: architectural decision/draft and this map prepared. Planned tests only, canonical fixture/harness additions, no production stubs or behavior changes. Reuse existing Workspace/init_git/git readback; no new single-caller factories or hidden assertion helpers. No tests removed/replaced.

      Selected proof: tests/acdd/orchestrator/waves.rs::workflow_run_executes_root_gates_without_task_state. Raw Cargo corroboration and claim-bound task verify --test will record actual compilation/assertion/exit/counts. The currently absent workflow subcommand is the natural pre-fix failure, not an injected fault. The test contains the complete intended command-effect assertions, but red ingress does not establish that downstream assertions or other acceptances already pass.

      Validation, accepted candidate and final task state: pending. Existing tracked-file hashes retained in initial-tracked-sha256.json; final check must show only admitted test/fixture/harness paths plus the engine-managed manifest extension. HEAD and staged changes must remain unchanged. Full build/independent review/delivery are subsequent roles.
    - 'decisions: Confirmed #1666 requires minimal shared guard extraction and immutable configuration context, both within milestone-owned engine but outside this task''s frozen scope roots. We will re-admit the necessary engine owner boundary via next contract revision under the existing milestone authority, preserving all uncommitted implementation and authentic contract proof; no bypass or duplicate guard. Please finish any in-flight atomic edits/tests, record exact remaining required owner paths and worktree state, then stop the build session at a safe point so the coordinator can change scope roots and re-run contract without concurrent writes. Do not submit partial build.'
    - 'decisions: Revision 3 synchronized after safe stop #1670/#1671: seven exact existing engine/task owner files from #1669 were added to the task scope under the milestone''s existing admitted capability. Acceptance/invariants unchanged, no protected file changes; claim 13 revoked. Preserve 37-file worktree byte-for-byte (backup /tmp/ac003-claim13-safe-stop); original genuine runtime-red contract snapshot 2140260942748c27226b95ed79e22de90489030c remains available for faithful proof replay. Contract author must inspect rules, record exact mechanism, and restore/verify every live file; never manufacture red or overwrite user work.'
    - |-
      decisions: Revision 3 re-admission decision, claim37, worker ac003-waves-r3-contract-01a1c08b, genuine session 01a1c08b-4515-7e72-9466-b989c7b20048. Acceptance/invariants/dependencies and blueprint #1655/#1656/#1657 remain unchanged. Seven newly admitted exact files are justified by #1666/#1669: tasks/gates.rs plus engine/gates/submit.rs expose the existing pure subtask delivery checks unchanged for plans; engine/mod.rs and engine/workspaces.rs thread immutable selected TaskSettings/Registry context; engine/lifecycle.rs and engine/blackboard.rs carry that context through existing administrative/blackboard ingress; engine/evidence_compiler.rs carries it through existing store_for_task proof/verify paths. No schema, parallel state authority, policy relaxation or protected-file edit is authorized. Existing named snapshots remain sufficient; no git_snapshot.rs expansion is needed. Preserve the builder safe-stop source and remaining obligations #1670/#1671; a passing seam is not full acceptance.

      Policy assessment: test-contract-seam explicitly allows an observed failure or recorded pre-fix replay (Admission lines38-40). Active task profile is unchanged seam-test-first, contract command=test, expected failure includes101 and stderr assertion failed, save:snapshot. The active profile and claim37 guidance do not forbid faithful historical replay or require the already implemented live seam to be called red. User and coordinator #1672 explicitly authorize a fresh historical replay without weakening these controls. Proceed only if exact historical bytes compile and the unchanged positive test naturally asserts; otherwise restore and stop. Current live exact test has freshly PASSED: 1 passed/0failed, exit0. Do not represent current implementation as red. Its selected test body is byte-identical to the prior red snapshot (SHA256 1fd81a86fd760e4bd3e5dc85cc88e29cc9a4f38509438a5e1cb9b27a6c6622b7).

      Replay perimeter: exact complete src/cli/mod.rs and src/cli/milestone.rs blobs from genuine accepted red snapshot2140260942748c27226b95ed79e22de90489030c. The first restores the actual pre-feature CLI grammar/dispatch; the second preserves its historical exhaustive MilestoneCommand enum compatibility. No source is patched to force failure; no tests, harnesses, fixtures, engine implementation, manifest, Cargo.toml/build.rs/.forge configuration, branch or index changes are planned. This is a two-file pre-fix seam replay against the remaining live tree, not a claim that the whole tree equals the historical commit. All files must stay in claim scope. The complete verified backup covers1029 tracked paths plus22 nonignored untracked work files (1051 inventory entries), file modes and hashes, archive b8a651b7b0fd7d431529c0b187c5f171f928f1605c5145783b549547121dfa1e at /tmp/ac003-r3-contract-01a1c08b/live-worktree.tar, additional to /tmp/ac003-claim13-safe-stop.

      Run under a restoration transaction with try/finally, signal cleanup and an independent parent-death restore watchdog. Exact raw Cargo seam must finish compilation and assert101; claim-bound task verify --test must independently report101, diagnostic_mode pass, error null with actual session. Submit contract while replay is intact, since the engine binds proof to the observed content; the accepted snapshot will intentionally contain those two historical CLI files and the other live scoped files. Then always restore both live file blobs and modes from the complete backup, compare every inventory entry and path set, refresh the live code graph, and reobserve the live exact test. Only explicit engine-managed manifest changes may be excepted; no manual manifest writes or excluded user files. Preserve HEAD and staged diff. Re-admission proves the unchanged historical red seam only; builder must freshly claim and prove restored current content at revision3. No implementation/build/review/delivery/main landing.
    - 'decisions: Revision3 configuration implementation choice: one immutable InvocationSettings (canonical root, selected config/profile/code-index DB) is scoped around in-process CLI dispatch. Existing TaskSettings/profile/milestone-dir owners consult that selection; normal Registry/store/proof/handoff APIs retain their canonical loaders, pins and primary-worktree task DB rules. No alternate registry/state/schema, parent env mutation or temp config rewriting. Scoped future enters selection only during poll and restores on Pending/error/panic, so concurrent in-process futures/threads cannot leak selections; child processes receive FORGE_CONFIG/FORGE_PROFILE/FORGE_DB only on Command, and owned isolated worktrees explicitly rebase root-relative selections. --db remains code index only. Dedicated concurrent/nested/panic and child propagation tests will verify these boundaries. Newly added context file and affected existing loader paths were acquired via extend-scope.'
    - 'decisions: Milestone restart implementation replays bound observations through canonical run_gate_graph, reading/writing existing gate_transitions only (proof_kind orchestrator distinguishes observational envelopes from canonical command verification proofs). Prefix binds selected profile, normalized manifest authority, and post-task HandoffBinding. Any mismatch invalidates remaining prefix. Recorded role session/findings restore independence and rejection context; delivered tasks remain canonical receipt-owned. No second task/cursor table or handoff evidence authority. Five domain tests pass; restart after failed review reuses accepted polish and delivered tasks. Added guard refusing provisioning another isolated worktree over an active claim; preserved existing lifecycle no-relaunch rule.'
    - 'decisions: Acknowledged exact missing serve and delta owners #1681/#1683, both needed for existing global CLI acceptance. Trace all remaining CLI ingress and downstream owners now, including explicit config/profile across command categories; enumerate all unavoidable outside-root paths with minimal edits so one revision handles them, and check conflicts with dependent guide scope. Complete in-flight checks, preserve full worktree/inventory, post exact results and stop safely without submitting partial build. Coordinator will re-admit these existing capability owner files via ACDD revision while preserving your work and red proof, never bypass scope.'
    - 'decisions: Revision 4 synchronized after builder safe stop #1685/#1686; unchanged acceptance and invariants, exact src/mcp/server.rs and src/engine/pipeline.rs added to scope for global CLI config/profile behavior documented in #1681/#1683. Five subtasks have intermediate evidence but task is not complete. Preserve current uncommitted work and repeat authentic prior red seam under ACDD with byte-for-byte restoration as in #1673-#1677; no protected files or history changes. This is coordinator implementation within user''s AC-003 authorization, not separate user authorization.'
    - |-
      decisions: Revision4 re-admission, claim56, actual session01a1c08b-4515-7e72-9466-b989c7b20048, worker ac003-waves-r4-contract-01a1c08b. Continue existing authorized AC-003 implementation scope; this is coordinator scope admission #1687, not separate/new user approval. Acceptance/invariants/dependencies and architecture #1655/#1656/#1657 remain unchanged. Read #1681/#1683/#1685/#1686/#1687 and active claim56 guidance.

      Exact added-owner rationale: src/mcp/server.rs owns Serve construction/session/request/background/reload boundaries. Its serve_with_cache_dir has only root/db/cache; adapter_snapshot falls back to load_adapter(root,None), and selected CLI task context cannot survive independent requests/background work by relying on CLI-only values. Add a backwards-compatible explicit immutable selection carrier/overload using the existing validated configuration/profile model; carry it through Server clones/worktree resolution/request snapshots/read/rebuild/delta/background contexts, retaining confinement, selected-store/profile pins and session independence. No new registry, global environment mutation, temporary config rewrite or second adapter cache authority. src/engine/pipeline.rs owns delta/delta_with_cache_dir/delta_inner; current delta_inner reconstructs persisted adapter and rebuilds from that path. Add an optional explicit validated adapter selection to this owner path, keep default wrappers preserving persisted behavior, and make explicit selection take precedence. On changed policy, release existing reader/generation lock and reuse canonical guarded rebuild/atomic publication rather than duplicate the pipeline or silently reuse the wrong persisted policy. --db remains code-index storage, not tasks_db.

      Builder blueprint: wire the resolved CLI selection into these two owner APIs and actual consumers; preserve existing defaults, request adapter snapshots, worktree isolation and persistence semantics. Replace the temporary CLI_SELECTION_UNSUPPORTED guards only after production-seam tests prove selected config/profile effects through real Server request/background and persisted-index delta behavior, malformed/missing explicit selection nonmutation, and concurrent selection isolation. The current unresolved_global_owner_selections_fail_before_server_or_delta_effects test is a temporary fail-closed boundary, not final acceptance; replace/subsume it with real positive owner-seam evidence without losing error nonmutation coverage. Reconcile CLI/acdd docs with actual final behavior and run required regressions/strict Clippy before a fresh build proof. Existing completed-subtask evidence #1685 is intermediate; do not infer whole acceptance from one green workflow seam. No functionality is implemented in this contract role.

      Replay assessment follows proven #1673-#1677: test-contract-seam permits recorded pre-fix replay; active seam-test-first gate still requires observed assertion red101 with stderr assertion failed and save:snapshot. No gate controls are relaxed. Fresh live exact workflow seam PASSED (1passed/0failed exit0); selected test body remains byte-identical to snapshot2140260942748c27226b95ed79e22de90489030c, SHA2561fd81a86fd760e4bd3e5dc85cc88e29cc9a4f38509438a5e1cb9b27a6c6622b7. Temporarily replay only complete src/cli/mod.rs and src/cli/milestone.rs blobs from that genuine pre-fix snapshot; the second keeps historical enum/API compatibility. Keep all tests, harnesses, canonical fixtures, remaining live implementation and authority unchanged. This is an exact two-file historical seam replay against the other live scoped files, never a claim that current green implementation is red or that the whole tree is historical.

      Complete backup before any replay covers1029 tracked plus30 nonignored untracked work paths (1059 entries), hashes and modes: /tmp/ac003-r4-contract-01a1c08b/live-worktree.tar, verified archive SHA256254aea338048d6b5d614631f9f7b71c0712cbf2354036c55d8a5ffd3b7c6353a. Prior safe-stop backups remain untouched. Use the same finally/signal/parent-death-watchdog transaction as revision3; explicitly include both replay files in graph delta even if a historical blob equals HEAD. Require clean compilation, natural assertion101, fresh claim-bound CLI verify with actual session, and submit while that historical content is bound. Always restore both live blobs/modes, compare every1059 entry/path set; only genuine engine-managed manifest changes could be excepted, and none is assumed. Record Git index stat metadata refresh separately while requiring identical staged modes/objects/paths, HEAD and branch; never rewrite .git. Refresh the live graph and reobserve live green seam afterward. Historical contract snapshot intentionally differs from restored live at just those CLI paths. If proof/restore cannot satisfy these conditions, stop with precise blocker. No protected-file edits, new implementation, build/review/delivery, history changes or main landing.
    - 'decisions: {"status":"independent-review-pass-ready","review_claim_revision":107,"contract_revision":4,"accepted_build_claim":98,"candidate":"9fd28f191bda89fb648928fa8a6ef5ee01d75450","prior_rejected":"eea896f8bfc422873fd5d5f5ab7ca85a0e9dcc65","baseline":"2fbda3cf518932342c123ef6900b76a9ecaeb368","reviewer_session":"01a1bdd1-3650-7c50-9ba7-5bf4cdd6f747","builder_session":"01a1c09d-0043-7601-b745-a82481fcbfe7","closure":{"task_id":"forge-mcp/AC-003-headless-orchestrator-and-role-runners:milestone-wave-runner-and-autonomous-delivery","candidate":"9fd28f191bda89fb648928fa8a6ef5ee01d75450","prior_candidate":"eea896f8bfc422873fd5d5f5ab7ca85a0e9dcc65","baseline":"2fbda3cf518932342c123ef6900b76a9ecaeb368","contract_revision":4,"review_claim_revision":107,"accepted_build_claim_revision":98,"reviewer":"codex-sol-reviewer","reviewer_session":"01a1bdd1-3650-7c50-9ba7-5bf4cdd6f747","builder_session":"01a1c09d-0043-7601-b745-a82481fcbfe7","findings":[{"id":"AC003-WAVE-R1","origin":"introduced-regression","disposition":"confirmed-fixed","owner":"crates/forge-tasks/src/engine/orchestrator/waves.rs","invariant":"Accepted isolated delivery commits must be integrated before completed-task, dependency, post-run or wave-barrier credit, including mode switching. Recovery preserves accepted proof and owned worktree.","authority":"#1655 section1 retained #1688; task wave/isolation acceptance","traceability":"CLI milestone run -> dispatch_waves -> actual lifecycle/shared SQLite receipt -> four ancestry credit checks -> restart/mode-switch/reconciliation -> bound existing handoff","evidence":"Current permanent real FF-refusal/zero-claim regression passes. Two original reviewer probes and normal merge-reconciliation recovery probe pass against exact current libraries; accepted receipt and recovery HEAD retained through actual handoff.","structural":"Four validated calls at waves.rs:377,396,412,423; helper434; current inbound/outbound graph matched; CLI ingress144 verified.","source_unchanged":"Ancestry helper and dispatcher are unchanged in the three-file repair from eea896f8."},{"id":"AC003-WAVE-R2","origin":"incomplete-remediation","disposition":"confirmed-fixed","owner":"crates/forge-tasks/src/engine/orchestrator/waves.rs","invariant":"The latest persisted root route preserves the exact valid reused prefix and freshly observed suffix, including failures, retry attempts, session/findings context, and original bindings. Unchanged restart reuses it; content/profile/authority changes invalidate relevant observations without rebinding historical proof.","authority":"#1655 section2 retained #1688; documented bound observation restart; original #1712 and residual #1718/#1719","traceability":"Canonical GateDecision/router -> existing transition record IDs -> JSON predecessor links to consumed observations -> scoped durable journal -> restart route reconstruction -> canonical routing/role restore -> unchanged command effects and retry exhaustion","evidence":"Byte-identical original partial-prefix scratch test now passes on third unchanged restart. Additional reviewer retry test preserves old rows byte-for-byte and adds no rows on unchanged third/fourth invocation; failed decisions still exhaust observe_once, not a different gate. Current permanent committed/uncommitted, unchanged, legitimate-delivery and post-task stale-binding neighbors all pass.","negative_evidence":"Current cyclic-link regression passes. Reviewer cross-milestone predecessor test proves an existing physical foreign row is not admitted into this milestone route and fails before effects. Reviewer legacy unlinked observations remain readable without rewriting stored proof.","structural":"One existing record_gate_transition writer returns actual SQLite last_insert_rowid. New links are only to earlier present same-query observations; no new table/schema/registry, old proof rewrite or alternate gate router. roles.restore unchanged and AST-verified.","mutation_sensitivity":"Exact scratch partial-prefix source SHA256 ed27607fd579d5227186d1405be24be33666a9551785bfc88db76236fbe29f89 failed against eea896f8 (3pass/1fail combined) and now passes. Original stale-content reproduction was runtime-red in #1712 and is green. Builder recorded independent clean-runtime red for suffix/retry cases in #1721/#1722; reviewer does not claim to have executed those historical builder runs."}],"mutation_matrix":[{"mutation":"Skip receipt ancestry or bypass it with in-place mode","expected":"zero dependent claims before integration; preserve proof/worktree","result":"pass: current permanent test and original reviewer reproduction"},{"mutation":"Refuse valid recovered delivery or replace its proof","expected":"normal merge reconciliation admits successor and bound handoff with original receipt","result":"pass: reviewer recovery probe"},{"mutation":"Ignore committed or dirty candidate content / rebind stale proof","expected":"fresh observations after actual changes","result":"pass: current source checks, original probe and permanent committed/uncommitted controls"},{"mutation":"Always reexecute unchanged valid observations","expected":"unchanged effects on restart","result":"pass: unchanged control and byte-identical partial-prefix reproduction"},{"mutation":"Hide fresh suffix behind obsolete chronological records","expected":"third unchanged partial-prefix restart reuses fresh suffix","result":"pass: exact prior-red source now green"},{"mutation":"Drop failed decision or reset retry budget","expected":"observe_once exhaustion with no extra effects or journal writes on subsequent restarts","result":"pass: permanent and independent reviewer retry/evidence probe"},{"mutation":"Follow cyclic, forward, missing or foreign journal link","expected":"earlier/present scoped links only; refusal before command effects","result":"source rejects non-earlier/missing; cyclic permanent and foreign-row reviewer tests pass"},{"mutation":"Rewrite accepted unlinked observations to new bindings","expected":"legacy format reads without stored proof mutation","result":"pass: reviewer legacy journal test"},{"mutation":"Replay completed task lifecycles after legitimate task delivery changes content","expected":"refresh pre-task observations without new task claims","result":"pass: permanent delivery-refresh and post-task resume tests"}],"verification":{"standard_commands":[{"command":"cargo clippy --all-targets --all-features -- -D warnings","exit_code":0,"warnings":0},{"command":"cargo test --test acdd","exit_code":0,"passed":302,"failed":0,"ignored":0},{"command":"cargo test --test mcp","exit_code":0,"passed":138,"failed":0,"ignored":0},{"command":"cargo test --test core adapters::","exit_code":0,"passed":4,"failed":0,"ignored":0},{"command":"cargo test --test cli_roots","exit_code":0,"passed":7,"failed":0,"ignored":0},{"command":"git diff --check","exit_code":0}],"standard_passed":451,"standard_failed":0,"domain_counts":{"waves":49,"lifecycle":11,"spawner":18,"types":8},"scratch":{"compile_exit_code":0,"command":"/tmp/ac003-waves-review3-_6wmor4b/acdd-eb447d574a36190a review_probe_ --nocapture --test-threads=1","exit_code":0,"passed":7,"failed":0,"ignored":0,"exact_compiler_argv":"/tmp/ac003-waves-review3-_6wmor4b/probe-build-command.json","result":"/tmp/ac003-waves-review3-_6wmor4b/probe-result.json","log":"/tmp/ac003-waves-review3-_6wmor4b/probe-runtime.log","method":"Scratch copy of current tests under /tmp, exact fresh Cargo-selected production libraries. Real disposable Git/SQLite/role processes/commands/router/lifecycle. No candidate edits."}},"open_findings":[],"excluded":"The later session-identity/gate-verification task remains outside this review and preserved. No main landing or history rewrite.","artifact":"/tmp/ac003-waves-review3-_6wmor4b"},"typed_review_proof":"/tmp/ac003-waves-review3-_6wmor4b/review-proof.json","full_report":"/tmp/ac003-waves-review3-_6wmor4b/review-report.md","next":"Submit typed independent review through genuine-session CLI. Follow actual next applicable gate; if delivery is next, claim and submit delivery in this same reviewer session with Forge-inferred evidence. Preserve all unrelated changes, prior proofs, later identity task and main; no milestone handoff/landing/history rewrite."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `contextunity-forge-mcp milestone run <milestone_id>` executes all milestone tasks in topological wave order, supporting isolated worktrees or `--in-place` mode, and completes verification and handoff.
2. `contextunity-forge-mcp plan [list|show|run]` executes structured documentation plans (`doc_type: plan`) in-place without worktree spawning.
3. `contextunity-forge-mcp workflow [list|show|run]` executes standalone root-gate workflows (`docs/workflows/*.yaml`) with `save: none`.
4. Global CLI arguments (`--root`, `--db`, `--config`, `--profile`) resolve deterministically across CLI commands.
5. All plan and workflow orchestrator tests are driven through `PlanHarness` and `WorkflowHarness` using canonical fixtures from `tests/acdd/fixtures/plans/` and `workflows/`; ad-hoc markdown heredocs and external CLI subprocess execution are strictly banned (`INV-PLAN-WORKFLOW-HARNESS`).
6. Documentation in `docs/reference/cli.md` and `docs/reference/acdd.md` is fully updated to document `plan [list|show|run]` (`--in-place`) and `workflow [list|show|run]` commands, execution flags, and persistence models.

---

### task: forge-guide-lifecycle-gates-and-subtasks

```yaml
task_ref: forge-guide-lifecycle-gates-and-subtasks
target: "Extend forge_guide with compact agent guidance for lifecycle, ACDD, code, documentation and gates"
proof_policy: seam-test-first
contract_revision: 3
depends_on:
  - milestone-wave-runner-and-autonomous-delivery
scope_roots:
  - src/cli/
  - src/mcp/
  - docs/reference/
  - skills/contextunity-forge/references/
  - tests/
scope:
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


---

### task: eliminate-session-scraping-and-enforce-worker-separation

```yaml
task_ref: eliminate-session-scraping-and-enforce-worker-separation
target: "Completely remove ambient host session scraping and fingerprint validation, enforce pure worker separation at independent gates, and emit explicit coordinator directives for subagent dispatch"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - forge-guide-lifecycle-gates-and-subtasks
scope:
  - crates/forge-tasks/src/store/lifecycle.rs
  - crates/forge-tasks/src/store/lifecycle/session_tests.rs
  - crates/forge-tasks/src/engine/workspaces.rs
  - crates/forge-tasks/src/engine/gates/submit.rs
  - crates/forge-tasks/src/engine/mod.rs
  - src/mcp/server.rs
  - src/mcp/tools.rs
  - docs/reference/tasks.md
  - docs/reference/acdd.md
symbols:
  - fn:submit_with_pre_persist
status: planned
subtasks:
  - subtask_ref: remove-ambient-session-scraping
    title: "Delete resolve_session_fingerprint, host_session_fingerprint, UNKNOWN_SESSION_FINGERPRINT, and stop reading CODEX_THREAD_ID/AGY_CONVERSATION_ID from process env and MCP server"
  - subtask_ref: pure-worker-independence-validation
    title: "Remove validate_session_independence; enforce gate independence purely via worker_id != prior_worker without session coupling"
  - subtask_ref: universal-independent-gate-dispatch-guidance
    title: "Emit explicit [INDEPENDENT GATE DISPATCH REQUIRED] directives with {task_slug}-{gate} and {agent} in workflow_guidance and submit dispatch response"
  - subtask_ref: test-suite-and-seam-decoupling
    title: "Replace obsolete session-probing unit tests with clean seam tests proving independent gate progression across CLI and MCP without ambient env coupling"
```

#### Acceptance

1. All ambient process/host environment session scraping (`resolve_session_fingerprint`, `CODEX_THREAD_ID`, `AGY_CONVERSATION_ID`, `FORGE_AGENT_SESSION_ID`, `UNKNOWN_SESSION_FINGERPRINT`) is completely removed from `crates/forge-tasks` and `src/mcp/server.rs`. No process environment sniffing remains in task lifecycle operations.
2. Gate independence checking is decoupled from host process/session concepts: `validate_session_independence` is eliminated. If a gate declares `independent_from: "<prior>"`, Forge verifies only that `worker_id != prior_evidence.worker_id`.
3. When any gate completes and the next gate declares `independent_from`, `task submit` returns an explicit structured `dispatch` block and coordinator directive:
   `[INDEPENDENT GATE DISPATCH REQUIRED] Next gate is '{gate}' (agent: '{agent}'). Rule: Must be independent from '{prior_gate}' worker '{prior_worker}'. Directs coordinator to spawn an independent subagent with worker_id '{task_slug}-{gate}'.`
4. `workflow_guidance` emits the matching directive with suggested worker pattern `{task_slug}-{gate}` and agent role instead of ambiguous text.
5. Obsolete `session_tests.rs` fail-closed assertions are replaced with tests verifying that distinct `worker_id` values advance independent gates reliably across both CLI and MCP without any ambient environment dependencies.
