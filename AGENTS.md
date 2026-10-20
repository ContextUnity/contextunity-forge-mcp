# ContextUnity Forge MCP — Agent Router

## Read before work

- Read [documentation instructions](docs/AGENTS.md) before documentation changes.
- Read [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md) before structural changes.
- Read [src/AGENTS.md](src/AGENTS.md) before source code changes and module decomposition.
- Read [roadmap](docs/roadmap.md) and [milestones](docs/milestones/README.md) for admitted direction and commitments.
- Read [tests/AGENTS.md](tests/AGENTS.md) for test placement and authoring boundaries.

## Routes

- Runtime behavior and setup: [README.md](README.md) and [docs](docs/).
- Architecture and modularity: [src/AGENTS.md](src/AGENTS.md), [architecture](docs/architecture/README.md), and [decisions](docs/adr/README.md).
- Interfaces: [configuration and tools](docs/reference/README.md), [task operations](docs/reference/tasks.md), [ACDD navigation](docs/reference/acdd.md), and [CLI commands](docs/reference/cli.md).
- Operations: [runbooks](docs/runbooks/README.md).
- Planning: [roadmap](docs/roadmap.md), [milestones](docs/milestones/README.md), and [plans](docs/plans/README.md).
- Verification: [required gates](TESTS.md) and the [testing guide](docs/testing/README.md).
- Forge tool discovery and task operations: load the shared `contextunity-forge` skill.
- Follow the [Blackboard procedure](skills/contextunity-forge/SKILL.md#blackboard) for claim context, progress reports, and gate or agent handoffs.

Use Forge MCP tools or the `contextunity-forge-mcp` CLI on the surface that provides the needed operation. Follow the active task profile and the `workflow_guidance` returned by task claim for gate sequence, proof policy, roles, models, review contours, and profile-specific behavior. Task builds run the affected test target and strict Clippy. Follow [TESTS.md](TESTS.md) for the exact task and milestone verification commands and count rules.

The embedded ACDD profile registers `rust-check-and-test` as strict Clippy followed by tests. Use its `workflow_guidance.scoped_commands` entry for an affected test target and submit the task-bound `expected_evidence_template` with completed proof fields.

## Documentation scan before handoff

Before milestone handoff, launch a documentation agent in the milestone worktree.
Give it the manifest, the final diff against the selected target base, the
accepted task evidence, and an explicit admitted documentation write scope.
Require it to load `agent-instructions`, read `docs/AGENTS.md` and affected owner
instructions, and reconcile behavior, CLI/MCP, configuration, workflows,
architecture, invariant descriptions, skills, and examples with live code/tests.
Update affected pages; explain when no update is needed. Preserve task-level
documentation co-evolution. Route source defects to their task owner and
architecture decision changes to owner approval. Post the scanned content binding,
updates, checks, and unresolved findings on the milestone blackboard. Submit the
documentation edits for independent review, then run the final checks in
`TESTS.md`. Repeat the affected scan if implementation changes afterward. The
coordinator launches the agent; this step adds no Forge gate or runtime launcher.

## Worktrees and Git permissions

Worktrees live at `.worktrees/<milestone-prefix>-<slug>` for milestones and `.worktrees/<milestone-prefix>-<task-slug>` for concurrent tasks.

### Permitted Git operations inside active milestone workflows

- Task commits:
  - With `auto_commit: true`, Forge automatically creates the delivery commit and records the receipt in SQLite; do not create duplicate commits.
  - With `auto_commit: false`, create exactly one scoped task commit containing task changes and the generated receipt after delivery.
  - Snapshot immutability: Never modify files during `deliver` stage or after `review`. Any edit triggers `TASK_CANDIDATE_MISMATCH`. If delivery fails, restore the worktree to the candidate snapshot before submitting rejection (`task submit --stage deliver --action reject`).
- History integration: Preserve accepted task proofs while integrating branches. Rebase, cherry-pick, merge, amend, and squash may change navigation SHAs without reopening completed tasks. Before handoff, obtain independent integration review and repository verification bound to the final content and explicit target base; follow the task and CLI references for the evidence file.
- Branch integration: Merge each delivered task branch into the milestone branch, delete the temporary task branch, and remove its worktree (`git worktree remove .worktrees/<milestone-prefix>-<task-slug>`).
- Handoff commit: When every milestone task is completed with accepted proofs or identically cancelled in the manifest and task store without a completion receipt, run `contextunity-forge-mcp milestone handoff` in the milestone worktree. Preserve cancelled history without completion credit. Commit the archived milestone document, along with any out-of-scope adjustments, deferred fixes, and recorded findings, documenting them in the handoff commit message.
- Milestone landing: In the primary worktree (`main`), fast-forward the milestone branch (`git merge --ff-only <milestone-branch>`), remove the milestone worktree (`git worktree remove .worktrees/<milestone-prefix>-<slug>`), and delete the milestone branch (`git branch -d <milestone-branch>`).

### Operations requiring explicit user approval

- Commits on `main`: Do not create Git commits directly on `main` without explicit user approval.
- Dangerous Git operations: History rewrites (`git reset --hard`, destructive rebases, force pushes), branch deletion outside task/milestone cleanup, and repository-level state clearing require explicit user approval.
- Remote publications: `git push`, tagging, releases, and credential publication require explicit user approval.

Do not run benchmarks without explicit user approval. Read [benchmark instructions](benchmarks/AGENTS.md) before benchmarking.

## Launching Codex agents (Antigravity Managed Background Task)

Launch Codex agents (`codex exec` / `codex exec resume`) directly through the native platform tool `run_command` with `WaitMsBeforeAsync: 500`:

```json
{
  "CommandLine": "codex exec resume -c model=\"gpt-6.1-sol\" -c model_reasoning_effort=\"medium\" <SESSION_ID> \"<PROMPT>\"",
  "Cwd": "/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/<milestone-prefix>-<task-slug>",
  "WaitMsBeforeAsync": 500,
  "toolAction": "Starting Codex Sol builder agent",
  "toolSummary": "Start Codex Sol builder for task"
}
```

For a new session without an existing `<SESSION_ID>`, run `codex exec -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" "<PROMPT>"` with the same `WaitMsBeforeAsync: 500` parameter.

### Review execution and findings feedback loop

1. **Review Stage (`model_reasoning_effort="xhigh"`)**:
   - Run the independent reviewer agent with maximum reasoning effort:
     `codex exec -c model="gpt-6.1-sol" -c model_reasoning_effort="xhigh" "<REVIEW_PROMPT>"`
   - Use an independent `worker_id` (e.g. `codex-sol-reviewer`) distinct from the builder worker.
2. **Builder feedback loop (`codex exec resume <BUILDER_SESSION_ID>`)**:
   - Always route review findings, rejection details, and remediation tasks back into the **same builder session**:
     `codex exec resume -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" <BUILDER_SESSION_ID> "<REVIEW_FINDINGS>"`
   - This preserves the builder's complete context, file history, and reasoning continuity without cold restarts.

### Why this execution pattern is required

1. **`WaitMsBeforeAsync: 500` (Managed Background Task)**:
   - The platform spawns the command directly in the Linux shell (`bash -c`).
   - When the process runs longer than 500 ms, the platform kernel automatically promotes it to a Managed Background Task and displays the live status bar (`[HH:MM:SS] codex exec ... running`).
2. **Reactive wakeup (Zero-Polling)**:
   - Do not poll task status in a loop. Stop calling tools at the end of the turn when waiting on the background agent.
   - When the process finishes (success or error), the platform event bus delivers `<SYSTEM_MESSAGE> Task id "task-XXXX" finished with result...` and reactively wakes the coordinator for the next step.
3. **Real-time log streaming**:
   - The platform streams process output in real time to `.system_generated/tasks/task-XXXX.log`, which can be inspected on disk without losing context.
