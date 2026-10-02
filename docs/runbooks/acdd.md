---
title: ACDD execution runbook
doc_type: runbook
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

1. Read the milestone contract and relevant [architecture](../architecture/README.md)
   and [decisions](../adr/README.md). Check scope and dependencies before
   assigning agents.
2. Run `contextunity-forge-mcp task sync docs/milestones/<number>-<slug>.md`.
3. Run `contextunity-forge-mcp task list --status ready` and select a task whose prerequisites are complete.
4. Keep work on the current branch, with one atomic commit per task containing
   source, tests, and its generated receipt. Keep the history linear.
5. Use distinct `worker_id` values for the builder and the review and delivery
   workers. Read `workflow_guidance`, `agent_type`, and the configured
   instruction path before claiming a stage.

## Deliver one task

If `design/v1` is active, claim it and submit a non-null design proof before the
contract stage. Use the stage shown by claim or inspect rather than skipping an
open gate.

| Gate | Worker action | Required proof |
| --- | --- | --- |
| `contract/v1` | Trace the proposed behavior from producer to consumer, owner, state, failure path, and observable proof. Write a red test through a public CLI, MCP, engine, or SQLite seam; verify its intended failure. Have another agent check the contract against live code and governing docs when using subagents. | `proof.contract_proof` names the seam test and its nonzero `red_exit_code`. |
| `build/v1` | Implement inside the allowed scope; turn the seam test green; run the affected domain suite and Clippy. Create the candidate task commit. | `proof.test_proof` records the exact test command, exit code 0, at least one passing test, and zero failures. |
| `review/v1` | Use a worker different from the accepted builder. Review the stable candidate diff against the task contract, then run focused verification. | `proof.review_proof` records `decision: "pass"` and evidence for all five review contours. |
| `deliver/v1` | Use a worker different from the builder. Submit the accepted commit for task delivery. | Forge validates the task and review proof, writes the receipt and context rollup, marks the task completed, and clears its blackboard. |

Claim before submitting each gate. Pass a direct JSON `evidence` object; do not
create evidence files. Each object includes the active `task_id`, exact
versioned `stage`, `claim_revision`, `contract_revision`, `worker_id`, absolute
claimed `worktree`, candidate `commit`, and non-null `proof`. Contract, build,
and review use typed proof. Claim output supplies the revisions and worktree.
A rejected review or delivery returns the
task to build remediation; read findings before rebuilding. See
[proof schemas and CLI forms](../reference/tasks.md#gates-and-evidence).

### Independent review

The reviewer checks the five contours against the current contract and the
candidate diff. Findings must cite an admitted requirement or verified
runtime defect. Route requests outside this task's scope to the owning task;
do not expand the contract during review.

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

### Keep one task commit

Use real callers and SQLite in contract and review tests. Keep tests in an
existing domain suite, share fixtures, and use table-driven cases for related
boundaries. When replacing tests, preserve each unique observable scenario.
Follow [test placement rules](../../tests/AGENTS.md).

The reviewed candidate commit contains source, tests, and documentation. The
delivery gate generates the receipt after accepting that candidate. Amend the
**same** task commit with the receipt, then inspect the final diff and linear
history before starting another task. The receipt names the reviewed candidate
SHA; the amended commit has a different SHA because a commit cannot contain
its own hash. Do not create a separate receipt commit or a merge commit.

## Share task context

The task blackboard is temporary, task-scoped SQLite state for agent handovers.
The specification already lives in the queue, so post concise findings rather
than duplicating the whole contract.

| Topic | Typical author | Use |
| --- | --- | --- |
| `contract_draft` | Contract author | Red test path, command, observed failure, and proposed public seam. |
| `contract_findings` | Contract reviewer | Unsupported assumptions and required contract repairs. |
| `build_proof` | Builder | Candidate SHA, focused test result, and Clippy result. |
| `architectural_notes` | Any worker | Decisions or trade-offs that must survive task delivery. |

Read relevant messages before taking over a gate. Blackboard messages do not
replace gate evidence. On successful task delivery, Forge copies
`architectural_notes` into the milestone receipt and clears that task's
messages. Other topics are temporary.

```sh
contextunity-forge-mcp task blackboard post TASK_ID --topic architectural_notes --payload "Decision and reason"
contextunity-forge-mcp task blackboard read TASK_ID --topic architectural_notes
```

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
2. Add a deferred end-to-end test that crosses real public callers and proves
   the milestone invariants across the completed tasks.
3. Run `cargo test --all-targets`,
   `cargo clippy --all-targets --all-features -- -D warnings`, and
   `cargo test --test commitment_integrity`. Record the actual test counts.
4. Commit the deferred test, then run:

```sh
contextunity-forge-mcp milestone handoff <number> --verification-command "cargo test --all-targets" --tests-passed <count> --tests-failed 0
```

The handoff command records verification, duration, and commit, marks the
milestone completed, moves it under `docs/milestones/archive/`, and updates
task references. Inspect the result, then commit the archived document.

## Guidance configuration

Set `agents_guidance` in the root `forge-mcp.yaml`, or `linked_workspaces[].tasks.agents_guidance` for a linked repository. Each path is confined to its repository and defaults to `AGENTS.md`. If the file is missing, claim and inspect return inline steps, `TASK_GUIDANCE_MISSING`, and the [canonical ACDD reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md).
