---
title: ACDD execution runbook
doc_type: guide
repository_id: contextunity-forge-mcp
project_id: forge-mcp
---

# ACDD execution runbook

Use this runbook to deliver a [milestone](../milestones/README.md). Run commands
from the repository root. The shared [contextunity-forge skill](../../skills/contextunity-forge/SKILL.md)
owns generic Forge tool guidance and standing Git permissions. The active task
profile and claim guidance own gate sequence, proof policy, roles, models, and
review contours. The [task reference](../reference/tasks.md) owns API and
evidence schemas; this page owns repository operations and verification.

## Two completion levels

| Level | Outcome | Current operation | Durable record |
| --- | --- | --- | --- |
| Task delivery | One task is completed | `task_submit` at the configured delivery stage | Milestone task receipt and SQLite status |
| Milestone handoff | All tasks and the deferred final test are complete | `milestone handoff` | Archived milestone and verification receipt |

> [!IMPORTANT]
> Invariant: Use **delivery** for the task outcome and **handoff** for the
> milestone outcome. The configured delivery stage completes one task; `milestone handoff`
> archives the completed milestone.

The milestone Markdown owns target, scope, invariants, dependencies, and
durable receipts. SQLite owns claims, gates, evidence, and temporary task
messages. A code-index rebuild does not reset task state.

## Prepare the queue

1. Create and enter a dedicated worktree for the milestone:
   `git worktree add .worktrees/<milestone-prefix>-<slug> -b <branch-name>`
   Run integration and milestone closure here; run each task's claims and tests
   in its claimed worktree.
2. Read the milestone contract and relevant [architecture](../architecture/README.md)
   and [decisions](../adr/README.md). Check scope and dependencies before
   assigning agents.
3. Bring any legacy task receipts in the milestone document into alignment with the
   current typed receipt schema before syncing, preserving engine parsers in `src/` intact.
4. Run `contextunity-forge-mcp task sync docs/milestones/<number>-<slug>.md`.
5. Run `contextunity-forge-mcp task list --milestone docs/milestones/<number>-<slug>.md --status ready` and select a task whose prerequisites are complete.
6. Read `workflow_guidance`, `agent_type`, the configured instruction path,
   and worker separation requirements before claiming a stage.
7. Read the active task's proof policy and gate guidance from its contract and
   claim response. Follow the task profile instead of assuming a fixed gate
   sequence or role assignment.
8. **Targeted verification**: Run the task's affected test suite and record
   its command and counts. Run final integration checks in the milestone worktree.
9. **Parallel writers**: Create a dedicated child worktree for each concurrent
   task, branched from the milestone. Claim and edit in that task worktree:
   `git worktree add .worktrees/<milestone-prefix>-<task-slug> -b <milestone-prefix>-<task-slug>`.
   After delivery and its task commit, merge the child branch into the milestone
   branch, delete the temporary task branch, and remove the child worktree.
   Root `AGENTS.md` grants that merge and branch deletion. The commands are in
   [Test placement and task integration](#test-placement-and-task-integration).
10. **Subtasks**: Record discoveries under the active task with `task subtask add`.
    Do not add a root milestone task for them. The schema is in the
    [task reference](../reference/tasks.md#subtasks-and-iterative-deepening).

Subtask status and evidence follow the task operations reference. Use the
shared skill's test procedure to add subtask cases to the admitted seam.

## Deliver one task

Claim the active task stage before submitting proof. Follow the gate identifiers,
stage instructions, role policy, and proof requirements returned by the active
task profile and claim. Use the [task reference](../reference/tasks.md#gates-and-evidence)
for evidence fields and CLI forms. Use `TESTS.md` for verification commands and
record the exact command and observed counts in build evidence.

### Independent review

The reviewer checks the contours specified by the active profile against the
current contract and candidate diff. Cite an admitted requirement, a baseline
regression, or a verified defect on the reference corpus. Route sibling-owned
defects to their owning task; reopen it when completed. Admit unowned paths
through `contextunity-forge-mcp task extend-scope <task-id> <path>` before
edits. Apply the [scope contract](../reference/tasks.md#five-flat-mcp-tools).
Record deferred findings in milestone `deferred_defects` or a linked successor
contract before delivery clears temporary blackboard discussion.

> [!IMPORTANT]
> Invariant: Review findings cite the admitted contract or a verified regression.
> Record proposals beyond that authority in a durable deferred destination.

For a valid finding, repair the underlying invariant and prove the result
through its public seam. Consolidate related findings into one bounded repair
pass, then recheck the affected contours. Mark inapplicable contours with a
reason rather than leaving them empty.

### Test placement and task integration

Use real callers and SQLite in contract and review tests. Keep tests in an
existing domain suite, share fixtures, and follow ACDD task taxonomy:
- **Feature Tasks**: Define exactly one root seam test at the contract stage selected by the active profile. Subtasks must not author separate unit test functions or binaries.
- **Scope Tasks**: Operate across broad domains/subsystems via a single parameterized table-driven test harness, where subtasks add test cases.
When replacing tests, preserve each unique observable scenario.
Follow [test placement rules](../../tests/AGENTS.md). Keep MCP tests under
`tests/mcp/` and register them from `tests/mcp.rs` in the existing MCP test
executable.

Follow the [shared skill's Git permission rules](../../skills/contextunity-forge/SKILL.md#git-permissions-and-milestone-close)
to determine whether delivery already created the task commit or requires an
agent commit. Avoid duplicate commits.

Merge the delivered task branch into the milestone branch and delete the
temporary task branch. From the milestone worktree, remove the child worktree:

```bash
git worktree remove .worktrees/<milestone-prefix>-<task-slug>
```

Root `AGENTS.md` grants the scoped task-branch merge, branch deletion, and
worktree removal permission. To repair a completed task before handoff,
run `contextunity-forge-mcp task reopen <task-id>` and repeat its gates.

## Share task context

Use `task_blackboard` for temporary milestone, task, and subtask coordination.
Read its active tool schema for supported topics and message operations. Move
accepted decisions and deferred findings to the milestone contract or a linked
successor before delivery clears temporary messages.

## Verify at the right scope

Use the narrowest command that proves the active contract. For tasks in this
repository's ACDD domain, follow the existing ACDD test target and the proof
policy selected by the active profile:

```sh
cargo test --test acdd tasks::TEST_NAME
cargo test --test acdd
cargo clippy --all-targets --all-features -- -D warnings
```

Confirm the filtered command actually runs the named test: Cargo can exit 0
when the filter matches zero tests. If the active proof policy requires a red
run, inspect its intended failure and nonzero exit code. Record the green
command's pass count and zero failures.

Use the affected suite for another domain. Run
`cargo test --test commitment_integrity` when a task touches commitments or
their storage invariants. Record the exact focused command and counts in build
evidence. Reuse green evidence in review only while the candidate and test
environment remain unchanged; rerun the affected check after repairs.

Reserve `cargo test --all-targets` for the milestone's deferred final test.
It compiles and links all test executables; repeating it at every task gate
adds cost without improving the focused contract proof. The
[testing guide](../testing/README.md) owns the repository-wide cadence.

## Coordinate subagents without busy polling

Give each agent a bounded role and an explicit artifact to return: contract
findings, a candidate diff, five-contour review, or deferred final test. Keep
one writer per file scope and review a stable diff. While independent agents
work, complete local work that does not depend on their results.

Use host notifications when available. If an explicit wait is needed, choose
the longest `wait_agent` timeout compatible with the host's user update
cadence: up to 60 seconds when progress must be reported each minute, or
several minutes when updates can be delivered asynchronously. Do not add
`sleep` after `wait_agent` or poll individual agents in a tight loop. Read
completed messages and resolve findings before follow-up work.

## Close the milestone

1. Confirm every milestone task is `completed` in SQLite and has a durable
   receipt. Documentation and instruction updates belong to each task's `build/v1`;
   verify changed relative links and metadata.
2. Run the milestone handoff gate in [`TESTS.md`](../../TESTS.md#milestone-handoff-gate)
   and prove milestone invariants through real callers.
3. Run `milestone handoff` with that gate as `--verification-command` and the
   counts specified in `TESTS.md`.
4. After `milestone handoff` succeeds, commit the archived milestone and merge
   the completed milestone branch into its target branch under the standing
   ACDD merge permission in root `AGENTS.md`.
5. Install the updated release binary:

   ```sh
   cargo install --path . --root ~/.local --force
   ```

## Guidance configuration

Set `agents_guidance` in the root `forge-mcp.yaml`, or `linked_workspaces[].tasks.agents_guidance` for a linked repository. Each path is confined to its repository and defaults to `AGENTS.md`. If the file is missing, claim and inspect return inline steps and `TASK_GUIDANCE_MISSING`. Use the [ACDD navigation](../reference/acdd.md) for links to the shared skill, this runbook, and task operations.
