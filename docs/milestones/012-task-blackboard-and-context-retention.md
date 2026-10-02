---
id: m-task-blackboard-and-context-retention
title: Task blackboard, in-store artifacts, and milestone context retention
doc_type: contract
status: active
depends_on:
- m-milestone-lifecycle-and-automation:completed
owners:
- src/core/tasks/
- src/db/tasks_store.rs
- src/engine/
- src/mcp/
- src/cli/
- docs/
- tests/
invariants:
- 'INV-TASK-BLACKBOARD: Subagents communicate task context and delivery state asynchronously through an in-memory/SQLite task blackboard rather than workspace-littering files or conversational context bloat.'
- 'INV-EPHEMERAL-SCRATCHPAD: Scratchpad blackboard messages are strictly task-scoped and pruned upon task completion, preventing context contamination across tasks.'
- 'INV-IN-STORE-ARTIFACTS: Task specifications, red test proofs, review contours, and receipts are stored directly inside SQLite tables (task_gates, task_blackboard, task_submissions) rather than arbitrary disk YAML/MD files.'
- 'INV-TASK-MILESTONE-ROLLUP: Completing a task automatically rolls up its verified commit SHA, invariant proofs, and durable architectural outcomes into the milestone descriptor and Markdown receipt, preserving vital context while discarding transient scratchpad noise.'
- 'INV-TASK-AGENT-METADATA: Tasks declare an optional agent_type metadata field, allowing milestone authors to prescribe the required agent specialization (e.g. worker, reviewer, pro, flash) for execution.'
- 'INV-CONFIGURABLE-WORKFLOW-GUIDANCE: Task claim and inspect responses dynamically provide actionable workflow guidance derived from the configured instructions file in forge-mcp.yaml (defaulting to AGENTS.md or docs/reference/acdd.md), enabling custom repositories to plug in their own instructions.'
- 'INV-PEER-REVIEWED-CONTRACT: A task contract must be verified and approved by an independent reviewer agent on the blackboard before implementation code may be claimed or authored.'
started_at: 2026-10-02T11:25:41+00:00
---

# Task blackboard, in-store artifacts, and milestone context retention

## Outcome and purpose

Eliminate the primary failure modes of multi-agent task execution: context window exhaustion (50M+ tokens), workspace littering from temporary YAML/markdown evidence files (`.forge/*.yaml`), unreviewed contract hallucinations, lost knowledge upon task closure, and agent confusion regarding which subagent to launch and which gates to advance.

Introduce a first-class **Task Blackboard** table in `.forge/tasks.sqlite` allowing asynchronous, token-lean communication between specialized agents. Store all verification evidence, review contours, and test proofs directly inside SQLite. Add `agent_type` metadata to task specifications. Make workflow instructions configurable via `forge-mcp.yaml` so any repository can provide custom execution playbooks. Upon task claim, dynamically return structured `workflow_guidance` telling the caller exactly which subagent type and role to spawn. Upon task completion, automatically roll up essential durable context (tested commit SHA, invariant verification, architectural decisions) into the parent milestone's typed receipt, while pruning ephemeral scratchpad discussions.

## Tasks in this milestone

### task: task-blackboard-sqlite-store

```yaml
task_ref: task-blackboard-sqlite-store
target: "Implement task_blackboard SQLite storage, indices, and CRUD API for asynchronous agent communication"
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/core/tasks/
  - src/db/tasks_store.rs
  - tests/core_basics/tasks.rs
status: completed
receipt:
  commit: 634d9e87767c9f6282b7c23ffe5f8f6760384ceb
  contract_revision: 1
  passed_at: "2026-10-02T11:41:50Z"
  evidence:
    test_proof:
      command: cargo test --test core_basics blackboard_ && cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 2
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Staged diff limited to task blackboard store, core parser bootstrap, and domain tests.
        claims:
          applicable: true
          evidence: Real SQLite schema, CRUD, topic filters, isolation, concurrency, reopen, cascade tested.
        concurrency:
          applicable: true
          evidence: Two independent WAL connections posted 40 messages with busy timeout.
        project_isolation:
          applicable: true
          evidence: Namespace check and task foreign key bind messages to one project task.
        administration:
          applicable: true
          evidence: Index and FK verified, no task files created.
  decision: pass
```

1. **SQLite Schema**:
   - Add `task_blackboard` table in `src/db/tasks_store.rs`:
     `id INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL REFERENCES tasks ON DELETE CASCADE, author TEXT NOT NULL, topic TEXT NOT NULL, payload TEXT NOT NULL, created_at INTEGER NOT NULL`.
   - Add index on `(task_id, created_at)`.
2. **Store API**:
   - `blackboard_post(task_id, author, topic, payload) -> Result<u64>`
   - `blackboard_read(task_id, topic?, limit?) -> Result<Vec<BlackboardMessage>>`
   - `blackboard_clear(task_id) -> Result<usize>` (prunes ephemeral notes for completed task).
3. **Task Isolation & Concurrency**:
   - Ensure WAL transactions with busy timeout handle concurrent posts from parallel worker subagents cleanly.

---

### task: in-store-task-artifacts-and-evidence

```yaml
task_ref: in-store-task-artifacts-and-evidence
target: "Accept structured JSON evidence directly in MCP and CLI task_submit, and require the typed task proof standard without evidence files"
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/core/tasks/gates.rs
  - src/db/tasks_store.rs
  - src/engine/tasks.rs
  - src/mcp/tools.rs
  - src/cli/task.rs
  - tests/core_basics/tasks.rs
  - tests/mcp_context/tasks.rs
status: completed
receipt:
  commit: 980f555fcf4d9c1136eb1e7d5889d4e53e1f9f1a
  contract_revision: 1
  passed_at: "2026-10-02T12:43:35Z"
  evidence:
    test_proof:
      command: cargo test --test core_basics && cargo test --test mcp_context && cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 58
      tests_failed: 0
      log: core_basics 33 passed; mcp_context 25 passed; clippy zero warnings; cargo test --all-targets passed before final review contour repair
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: "Diff matches task 2 scope: typed gate validators, direct JSON CLI/MCP seams, and domain tests."
        claims:
          applicable: true
          evidence: "Real SQLite tests verify typed contract, test, and review proofs; bare forms and field aliases are rejected."
        concurrency:
          applicable: true
          evidence: "Independent reviewer inspected final strict-proof diff and ran the real SQLite reject-path test."
        project_isolation:
          applicable: true
          evidence: "Evidence binds task identity, claim revision, worker and worktree to the task row."
        administration:
          applicable: true
          evidence: "CLI and MCP accept inline JSON; no evidence files are read or written; clippy passed with zero warnings."
  decision: pass
```

1. **Direct In-Memory Evidence Submissions**:
   - Accept a raw JSON object as `evidence` in MCP and CLI `task_submit`; reject file-based evidence references.
   - Validate and store the proof payload directly in `task_gates.evidence` without reading an evidence file from disk.
2. **Standardized Proof Schemas**:
   - Require typed proof payloads: `test_proof` (command, exit code, test count, failures, optional bounded inline log), `review_proof` (5 contours with inline evidence), and `contract_proof` (seam test reference, red exit code). Reject older unwrapped proof forms.
3. **In-Store Proof Storage**:
   - Store all gate proofs, test logs, review contours, and commit metadata directly inside SQLite tables (`task_gates`, `task_blackboard`, `task_submissions`).

---

### task: task-agent-metadata-and-workflow-guidance

```yaml
task_ref: task-agent-metadata-and-workflow-guidance
target: "Add agent_type metadata to task specs, configurable guidance path in forge-mcp.yaml with missing file fallback, and dynamic workflow_guidance in claim and inspect responses"
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/core/tasks/mod.rs
  - src/engine/tasks/workspaces.rs
  - src/engine/tasks.rs
  - src/db/tasks_store.rs
  - tests/core_basics/tasks.rs
status: completed
receipt:
  commit: ec913ac22f1ab69ec9784f87b1c5041f65ae2f06
  contract_revision: 1
  passed_at: "2026-10-02T12:58:45Z"
  evidence:
    test_proof:
      command: cargo test --test core_basics && cargo test --test mcp_context && cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 59
      tests_failed: 0
      log: core_basics 34 passed; mcp_context 25 passed; clippy zero warnings; focused guidance test passed after final fallback tweak
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Change is limited to task guidance engine/workspace logic and one domain seam test.
        claims:
          applicable: true
          evidence: Real engine and SQLite test verifies agent_type, root and linked guidance paths, stage-specific claim and inspect guidance, and missing-file fallback.
        concurrency:
          applicable: true
          evidence: Guidance names the accepted builder worker ID and requires a distinct independent review identity.
        project_isolation:
          applicable: true
          evidence: Configured guidance paths are confined to each workspace root and linked workspace identity.
        administration:
          applicable: true
          evidence: No old agents_md alias; canonical agents_guidance only. Core, MCP, and clippy checks passed.
  decision: pass
```

1. **Flexible `agent_type` Metadata**:
   - Extend `TaskSpec` with optional `agent_type: Option<String>`.
   - Accept named agent roles (`worker`, `reviewer`, `architect`, `planner`), model tier aliases (`pro`, `flash`, `flash_lite`), or exact model identifiers (e.g. `gpt-6-sol`, `claude-3-7-sonnet`, `gemini-2.5-pro`).
   - Parse `agent_type` from milestone YAML task blocks and expose in `task_claim` / `task_descriptor`.
2. **Configurable Guidance in `forge-mcp.yaml`**:
   - Support `agents_guidance: <path>` (e.g. `docs/runbooks/acdd.md` or custom workflow file) in `forge-mcp.yaml` and `linked_workspaces`.
   - If omitted, default to `AGENTS.md`.
3. **Missing Guidance Fallback & GitHub Pointer**:
   - If the configured or default guidance file does not exist on disk, handle missing files gracefully:
     - Return a structured warning in `workflow_guidance.warning`.
     - Provide concise inline starter steps so the agent can still make progress.
     - Include a canonical GitHub documentation link to `docs/reference/acdd.md` in ContextUnity Forge MCP explaining why the file is needed and how to scaffold it.
4. **Dynamic `workflow_guidance` in Task Envelope**:
   - In `task_claim` and `task_manage(inspect)`, compute and return dynamic `workflow_guidance`:
     - Active stage;
     - Prescribed `agent_type` and `subagent_role`;
     - Discrete actionable steps for the current gate (contract authoring, independent review, build, deliver);
     - Independence rules requiring distinct reviewer identity from the accepted builder.

---

### task: task-to-milestone-context-rollup

```yaml
task_ref: task-to-milestone-context-rollup
target: Automatically rollup durable task outcomes into milestone receipts and task descriptors upon completion
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/milestones.rs
- src/core/tasks/mod.rs
- src/core/tasks/gates.rs
- src/db/tasks_store.rs
- tests/core_basics/tasks.rs
- tests/mcp_context/tasks.rs
status: completed
receipt:
  commit: 5694b2b881bca64409f0f49c272e6942d6c072da
  contract_revision: 1
  passed_at: 2026-10-02T13:25:49.598446728+00:00
  evidence:
    test_proof:
      command: cargo test --test core_basics && cargo test --test mcp_context && cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 60
      tests_failed: 0
      log: core_basics 35 passed; mcp_context 25 passed; clippy zero warnings; terminal rollup and crash-retry seams passed
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Change matches admitted task4 scope: task receipt model, terminal gate, milestone YAML writer, store, and integration tests.'
        claims:
          applicable: true
          evidence: Real SQLite and Markdown seam proves generated typed receipt with accepted commit, invariants, five-contour summary, and architectural notes.
        concurrency:
          applicable: true
          evidence: Crash-retry path accepts only exact persisted receipt and keeps claim and blackboard on mismatch.
        project_isolation:
          applicable: true
          evidence: Blackboard pruning deletes only completed task messages; sibling task messages remain.
        administration:
          applicable: true
          evidence: Core 35 and MCP 25 tests passed; clippy zero warnings; no manual prewritten receipt required.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-TASK-BLACKBOARD: Subagents communicate task context and delivery state asynchronously through an in-memory/SQLite task blackboard rather than workspace-littering files or conversational context bloat.'
    - 'INV-EPHEMERAL-SCRATCHPAD: Scratchpad blackboard messages are strictly task-scoped and pruned upon task completion, preventing context contamination across tasks.'
    - 'INV-IN-STORE-ARTIFACTS: Task specifications, red test proofs, review contours, and receipts are stored directly inside SQLite tables (task_gates, task_blackboard, task_submissions) rather than arbitrary disk YAML/MD files.'
    - 'INV-TASK-MILESTONE-ROLLUP: Completing a task automatically rolls up its verified commit SHA, invariant proofs, and durable architectural outcomes into the milestone descriptor and Markdown receipt, preserving vital context while discarding transient scratchpad noise.'
    - 'INV-TASK-AGENT-METADATA: Tasks declare an optional agent_type metadata field, allowing milestone authors to prescribe the required agent specialization (e.g. worker, reviewer, pro, flash) for execution.'
    - 'INV-CONFIGURABLE-WORKFLOW-GUIDANCE: Task claim and inspect responses dynamically provide actionable workflow guidance derived from the configured instructions file in forge-mcp.yaml (defaulting to AGENTS.md or docs/reference/acdd.md), enabling custom repositories to plug in their own instructions.'
    - 'INV-PEER-REVIEWED-CONTRACT: A task contract must be verified and approved by an independent reviewer agent on the blackboard before implementation code may be claimed or authored.'
    architectural_notes:
    - Task delivery derives a typed receipt from accepted SQLite proofs, writes milestone YAML, and clears only task-scoped blackboard messages. Exact receipt retry recovers an interrupted cross-store commit.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

1. **Rollup Mechanics**:
   - When a task reaches terminal stage (`deliver` / `completed`), extract:
     - Verified commit SHA;
     - Proven architectural invariants (`applicable_invariants`);
     - Summary of review decisions across the 5 contours;
     - Key architectural outcomes / decisions recorded on the blackboard under topic `architectural_notes`.
   - Update the task's entry in the milestone Markdown file (`docs/milestones/<milestone>.md`), writing a typed `receipt:` block containing these durable outcomes.
2. **Ephemeral Context Pruning**:
   - Automatically call `blackboard_clear(task_id)` upon successful task completion, ensuring transient debugging notes and author-reviewer debates are cleaned up while durable findings remain in the milestone receipt.

---

### task: blackboard-tooling-and-agent-routing

```yaml
task_ref: blackboard-tooling-and-agent-routing
target: Expose blackboard operations via MCP tools, author reference and runbook documentation, and update Forge skills
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/mcp/tools.rs
- src/cli/task.rs
- src/db/tasks_store.rs
- src/engine/tasks/workspaces.rs
- README.md
- docs/reference/acdd.md
- docs/runbooks/acdd.md
- docs/reference/tasks.md
- docs/reference/mcp-tools.md
- docs/reference/mcp-setup.md
- docs/reference/cli.md
- docs/reference/configuration.md
- docs/reference/README.md
- docs/README.md
- AGENTS.md
- .agents/skills/contextunity-forge/SKILL.md
- tests/core_basics/tasks.rs
- tests/mcp_context/tasks.rs
status: completed
receipt:
  commit: 6efc4c1bfdf73cc33675e213e0797501fac207a7
  contract_revision: 1
  passed_at: 2026-10-02T13:51:42.782798928+00:00
  evidence:
    test_proof:
      command: cargo test --test core_basics && cargo test --test mcp_context && cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 61
      tests_failed: 0
      log: core_basics 35 passed; mcp_context 26 passed; clippy zero warnings; real MCP stdio and CLI share SQLite blackboard
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Diff matches admitted task5 scope: MCP/CLI/store/guidance, real integration tests, ACDD references, indexes, agent routing.'
        claims:
          applicable: true
          evidence: MCP stdio and CLI post/read share SQLite; completed task rejects late post; typed JSON-only proof and automatic rollup documented.
        concurrency:
          applicable: true
          evidence: Atomic store insert predicate serializes blackboard posts with task delivery; independent code and docs reviewers passed.
        project_isolation:
          applicable: true
          evidence: Store and tools bind task ID to namespace, topic filtering and task-scoped reads; sibling task messages remain isolated.
        administration:
          applicable: true
          evidence: Core 35 and MCP 26 tests pass, clippy zero warnings, docs links and 20-tool catalog verified.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-TASK-BLACKBOARD: Subagents communicate task context and delivery state asynchronously through an in-memory/SQLite task blackboard rather than workspace-littering files or conversational context bloat.'
    - 'INV-EPHEMERAL-SCRATCHPAD: Scratchpad blackboard messages are strictly task-scoped and pruned upon task completion, preventing context contamination across tasks.'
    - 'INV-IN-STORE-ARTIFACTS: Task specifications, red test proofs, review contours, and receipts are stored directly inside SQLite tables (task_gates, task_blackboard, task_submissions) rather than arbitrary disk YAML/MD files.'
    - 'INV-TASK-MILESTONE-ROLLUP: Completing a task automatically rolls up its verified commit SHA, invariant proofs, and durable architectural outcomes into the milestone descriptor and Markdown receipt, preserving vital context while discarding transient scratchpad noise.'
    - 'INV-TASK-AGENT-METADATA: Tasks declare an optional agent_type metadata field, allowing milestone authors to prescribe the required agent specialization (e.g. worker, reviewer, pro, flash) for execution.'
    - 'INV-CONFIGURABLE-WORKFLOW-GUIDANCE: Task claim and inspect responses dynamically provide actionable workflow guidance derived from the configured instructions file in forge-mcp.yaml (defaulting to AGENTS.md or docs/reference/acdd.md), enabling custom repositories to plug in their own instructions.'
    - 'INV-PEER-REVIEWED-CONTRACT: A task contract must be verified and approved by an independent reviewer agent on the blackboard before implementation code may be claimed or authored.'
    architectural_notes:
    - MCP and CLI task_blackboard share the task SQLite store; an atomic completed-state predicate prevents late ephemeral posts. ACDD reference and routing docs reflect the generated receipt lifecycle.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

1. **Dual-Layer Documentation**:
   - **`docs/reference/acdd.md`**: Foundational architectural reference explaining the ACDD philosophy, independent review rationale, task blackboard concept, and the canonical GitHub link for external repositories.
   - **`docs/runbooks/acdd.md`**: Concrete, step-by-step operational runbook detailing the 4 roles (Contract Author, Contract Reviewer, Builder, Delivery Reviewer), gate transitions, and blackboard communication protocols.
2. **MCP Tool Integration**:
   - Add `task_blackboard` tool or action in `task_manage`:
     - `task_blackboard(action: "post"|"read", task_id: string, topic?: string, payload?: string)`
3. **CLI Commands**:
   - `contextunity-forge-mcp task blackboard read <task_id> [--topic <topic>]`
   - `contextunity-forge-mcp task blackboard post <task_id> --topic <topic> --payload <json_or_text>`
4. **Skill Synchronization**:
   - Update `.agents/skills/contextunity-forge/SKILL.md` (and global skill) to document the complete blackboard and subagent dispatch protocol.
   - Ensure Forge FTS indexes both `docs/reference/acdd.md` and `docs/runbooks/acdd.md` for fast retrieval via `get_doc` or `search_docs`.
