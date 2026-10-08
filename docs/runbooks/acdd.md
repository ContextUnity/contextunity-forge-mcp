---
title: ACDD execution runbook
doc_type: guide
repository_id: contextunity-forge-mcp
project_id: forge-mcp
---

# ACDD execution runbook

Use this runbook to deliver a [milestone](../milestones/README.md) through the
[ACDD lifecycle](../reference/acdd.md). Run commands from the repository root.
The [task reference](../reference/tasks.md) owns API and evidence schemas; this
page owns the order of work and verification.

## Two completion levels

| Level | Outcome | Current operation | Durable record |
| --- | --- | --- | --- |
| Task delivery | One task is completed | `task_submit` at `deliver/v1` | Milestone task receipt and SQLite status |
| Milestone handoff | All tasks and the deferred final test are complete | `milestone handoff` | Archived milestone and verification receipt |

> [!IMPORTANT]
> Invariant: Use **delivery** for the task outcome and **handoff** for the
> milestone outcome. `deliver/v1` completes one task; `milestone handoff`
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
6. Use distinct `worker_id` values for the builder and the review and delivery
   workers. Read `workflow_guidance`, `agent_type`, and the configured
   instruction path before claiming a stage.
7. **Pre-existing code reconciliation**: Use `proof_policy: direct-proof` to
   submit existing passing seam evidence at `contract/v1`. Complete build
   verification before independent review; all four gates apply.
8. **Targeted verification**: Run the task's affected test suite and record
   its command and counts. Run final integration checks in the milestone worktree.
9. **Parallel writers**: Create a dedicated child worktree for each concurrent
   task, branched from the milestone. Claim and edit in that task worktree:
   `git worktree add .worktrees/<milestone-prefix>-<task-slug> -b <milestone-prefix>-<task-slug>`.
   Integrate its delivered task commit into the milestone branch under the
   repository's Git permissions, then prune the completed child worktree.
10. **Subtasks for iterative deepening**: When new discoveries, component variations (e.g. Playwright fixtures,
    Alpine directives, DOM query methods), or sub-checklists arise during implementation, record them as **subtasks**
    under the active task (`task subtask add <task_id> <subtask_ref> <title>`). Do NOT spawn new root-level milestone tasks
    for internal sub-discoveries, which would inflate the milestone and trigger full four-gate lifecycles (`contract/v1` through `deliver/v1`)
    for minor items. Subtasks maintain fine-grained status and evidence directly without breaking the parent contract digest.
    During execution, SQLite is the canonical state; upon passing `deliver/v1`, Forge flushes all subtasks into the durable
    milestone Markdown document alongside the delivery receipt. Completed tasks freeze subtask modification (`TASK_TERMINAL`).

## Deliver one task

Claim and advance tasks through the 4 gates in order (`contract/v1` -> `build/v1` -> `review/v1` -> `deliver/v1`).
Use the stage shown by claim or inspect rather than skipping an open gate.

| Gate | Worker action | Required proof |
| --- | --- | --- |
| `contract/v1` | Trace the proposed behavior from producer to consumer, owner, state, failure path, and observable proof. For greenfield tasks, write a red test through a public seam and verify its intended failure. For direct proof, verify the existing seam. | `proof.contract_proof` names the seam test and its `red_exit_code` (or 0 for direct proof). |
| `build/v1` | Implement inside the allowed scope; turn the seam test green; run the affected domain suite and Clippy. Forge captures the candidate snapshot. | `proof.test_proof` records the exact test command, exit code 0, at least one passing test, and zero failures. |
| `review/v1` | Use a worker different from the accepted builder. Review the candidate diff of scoped files against the task contract. Ignore unrelated dirty files outside this task's scope. | `proof.review_proof` records `decision: "pass"` and evidence for all five review contours. |
| `deliver/v1` | Use a worker different from the builder. Submit delivery, then commit the delivered task under the repository's Git permissions. | Forge validates the task and review proof, writes the receipt and context rollup, marks the task completed, and clears its blackboard. |

Claim before submitting each gate. Pass a direct JSON `evidence` object; do not
create evidence files. Each object includes the active `task_id`, exact
versioned `stage`, `claim_revision`, `contract_revision`, `worker_id`, absolute
claimed `worktree`, and non-null `proof`. Forge supplies a scoped snapshot
when contract/build evidence omits `commit`; review/delivery bind the accepted
build candidate. Contract, build,
and review use typed proof. Claim output supplies the revisions and worktree.
A rejected review or delivery returns the
task to build remediation; read findings before rebuilding. See
[proof schemas and CLI forms](../reference/tasks.md#gates-and-evidence).

### Independent review

The reviewer checks the five contours against the current contract and the
candidate diff. Cite an admitted requirement, a baseline regression, or a
verified defect on the reference corpus. Route sibling-owned defects to their
owning task; reopen it when completed. Admit unowned paths through
`contextunity-forge-mcp task extend-scope <task-id> <path>` before edits.
Apply the [scope contract](../reference/tasks.md#five-flat-mcp-tools).
Record deferred findings in milestone `deferred_defects` or a linked successor
contract before delivery clears temporary blackboard discussion.

> [!IMPORTANT]
> Invariant: Review findings cite the admitted contract or a verified regression.
> Record proposals beyond that authority in a durable deferred destination.

| Contour | Review question |
| --- | --- |
| `paths` | Are edits inside the authorized scope? |
| `claims` | Do code and tests prove specified behavior without invented requirements? |
| `concurrency` | Are thread safety, SQLite transactions, and claim races handled where applicable? |
| `project_isolation` | Are repository and project boundaries preserved? |
| `administration` | Are configuration defaults, schema, and operational commands sound? |

For a valid finding, repair the underlying invariant and prove the result
through its public seam. Consolidate related findings into one bounded repair
pass, then recheck the affected contours. Mark inapplicable contours with a
reason rather than leaving them empty.

### Keep one task commit and observe task taxonomy

Use real callers and SQLite in contract and review tests. Keep tests in an
existing domain suite, share fixtures, and follow ACDD task taxonomy:
- **Feature Tasks**: Define exactly 1 root seam test (`contract/v1`). Subtasks must not author separate unit test functions or binaries.
- **Scope Tasks**: Operate across broad domains/subsystems via a single parameterized table-driven test harness, where subtasks add test cases.
When replacing tests, preserve each unique observable scenario.
Follow [test placement rules](../../tests/AGENTS.md).

After successful delivery and explicit Git authorization, commit the task's
source, tests, and generated milestone receipt together:
```bash
git add <scoped-files> <test-files> <milestone-doc>
git commit -m "feat(<scope>): <task summary>"
```
Preserve the generated snapshot SHA in the receipt. Integrate delivered task
commits into the milestone branch. To repair a completed task before handoff,
run `contextunity-forge-mcp task reopen <task-id>` and repeat its gates.

## Share task context

The blackboard holds temporary milestone, task, and subtask coordination.
Read newest-first summary pages, then inspect relevant message IDs to obtain
their payloads. Select explicit context keys when active work is ambiguous.
The specification already lives in the queue, so post concise findings rather
than duplicating the whole contract.

| Topic | Typical author | Use |
| --- | --- | --- |
| `contract_draft` | Contract author | Red test path, command, observed failure, and proposed public seam. |
| `contract_findings` | Contract reviewer | Unsupported assumptions and required contract repairs. |
| `build_proof` | Builder | Candidate SHA, focused test result, and Clippy result. |
| `architectural_notes` | Any worker | Decisions or trade-offs that must survive task delivery. |

Read summaries and inspect relevant payloads when claiming a task, resuming a
turn, starting a subtask, and taking over a gate. Submit gate
evidence through `task_submit`. Before delivery, give each accepted deferred
finding a durable destination in `deferred_defects` or a linked successor contract. On successful task delivery, Forge copies
`architectural_notes` into the milestone receipt and clears that task's
messages. Other topics are temporary.

```sh
contextunity-forge-mcp task blackboard post TASK_ID --topic architectural_notes --payload "Decision and reason"
contextunity-forge-mcp task blackboard read TASK_ID --topic architectural_notes
```

### Investigation checkpoints

Post `hypothesis` before implementation, `architectural_seam` with the relevant
ADR and pipeline changes, and `measured_delta` before completing a subtask.
After two unsuccessful repair iterations or a performance/Merkle regression,
record reproducible evidence under `blockers` and escalate the architectural
blocker to the coordinator. Retain the current fail-closed boundary.
Promote durable findings before delivery clears task messages.

## Verify at the right scope

Use the narrowest command that proves the active gate. For tasks in this
repository's task domain, run the red and green seam test with a filter, then
the domain suite before task delivery:

```sh
cargo test --test core_basics tasks::TEST_NAME
cargo test --test core_basics
cargo clippy --all-targets --all-features -- -D warnings
```

Confirm the filtered command actually runs the named test: Cargo can exit 0
when the filter matches zero tests. For the red run, inspect the intended
failure and nonzero exit code; for the green run, record at least one passed
test and zero failures.

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
   receipt. Align the affected reference, architecture, runbook, root agent
   guidance, and README pages with changed CLI, MCP, configuration, and schema
   behavior. Validate changed metadata and relative links.
2. Run `test-suite-refactor` for the admitted final verification scope and prove
   milestone invariants across real callers. Route required code/test changes
   through their owning task and delivery gates before final verification.
3. Run `cargo test --all-targets`,
   `cargo clippy --all-targets --all-features -- -D warnings`, and
   `cargo test --test commitment_integrity`. Record the actual test counts.
4. Commit final test changes. Record task IDs and their full commit SHAs in
   `## Task commits`, including any final test commit. Run:

```sh
contextunity-forge-mcp milestone handoff <number> --verification-command "cargo test --all-targets" --tests-passed <count> --tests-failed 0
```

5. Commit the archived milestone as the handoff commit; leave its own SHA
   unrecorded. Merge under the repository's Git permissions.
After merge, install the updated release binary with `cargo install --path . --root ~/.local --force`.

## Guidance configuration

Set `agents_guidance` in the root `forge-mcp.yaml`, or `linked_workspaces[].tasks.agents_guidance` for a linked repository. Each path is confined to its repository and defaults to `AGENTS.md`. If the file is missing, claim and inspect return inline steps, `TASK_GUIDANCE_MISSING`, and the [canonical ACDD reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md).
