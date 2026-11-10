---
title: "Augmented Context-Driven Delivery"
doc_type: guide
---

# Augmented Context-Driven Delivery

ACDD (Augmented Context-Driven Delivery) is a system that injects tailored context into verified gates. Rather than relying on open-ended or unstructured agent coding loops, ACDD dynamically serves each agent the exact context it needs — admitted write scopes, governing architectural invariants, in-scope symbol skeletons, covering test seams, and reviewer rejection findings — bound to atomic, verifiable gates.
People keep the contract in Git (`docs/milestones/*.md`). Forge syncs tasks into `.forge/tasks.sqlite` and serves them to agents through `task_list`, `task_claim`, and `task_submit`. A code-index rebuild does not reset that queue.

The active profile names the gates, their order, the proof each gate requires, the roles, and the review contours. The profile compiled into Forge uses `contract`, `build`, `review`, and `deliver`. A repository file at `.forge/acdd/profile.yaml`, or a pin on a milestone or task, replaces that shape. Writing one is covered in [ACDD profiles](profiles.md).

Claiming a gate returns two kinds of context. `workflow_guidance` names the role, the steps, and the tools that fit the gate. The context bundle carries the material for that role: scope and symbols on contract and build, the candidate snapshot on review, the receipt rules on deliver. Recent blackboard notes ride along. Historical gate logs stay in SQLite until someone asks `task inspect` for them. [Task operations](tasks.md) specifies the arguments, evidence objects, and receipts. [ADR 0016](../adr/0016-lean-stage-tailored-task-context-bundles.md) records why the bundle changes by gate.

MCP task tools and CLI task commands present results as Markdown by default;
use MCP `format: "json"` or CLI `--format json` when parsing values. The shared
layout preserves stage guidance and evidence and gives embedded blockers a
recoverable caution callout. See the
[task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle).

The embedded profile assigns contract author, builder, and independent reviewer roles. The contract author posts the draft as a blackboard `draft` targeted to `build`; the reviewer continues from review through delivery in the same worker session. Workflow guidance includes `NO_PREFLIGHT_SUBAGENTS`, the registered combined Rust command, and scoped test commands. Contract and build context includes indexed scope symbols, external callers, covering tests, ADR invariant summaries, and a task-bound expected evidence template. The task engine rejects disconnected contract seams and public signature changes whose indexed callers fall outside admitted scope.

For any gate declaring `independent_from`, claim and submission compare its
`worker_id` with the accepted worker of that predecessor. Session metadata is
optional and does not decide gate independence. Lifecycle metadata uses explicit
in-process identity or `FORGE_AGENT_SESSION_ID`, without legacy host scraping.
Configured runners retain distinct real session fingerprints, isolated process
groups and rejection resumption. Claim, worktree, candidate and proof bindings
continue to apply.

Submission to an independent next gate returns a structured `dispatch` derived
from the actual profile role and accepted prior worker. The same object and exact
`[INDEPENDENT GATE DISPATCH REQUIRED]` directive appear in workflow guidance,
with suggested worker `{task_slug}-{gate}`. The coordinator performs that dispatch;
reading guidance launches no process. See [gate dispatch and evidence](tasks.md#gates-and-evidence).

## Hierarchical gate cursors and seam proofs

A profile gate can contain child gates. [`claim_nested_gate`](../../crates/forge-tasks/src/engine/claim.rs) resolves the persisted stage to its active leaf gate, applies that leaf's role and proof requirements, and accepts qualified child stage IDs. Passing a child advances the cursor to the next child. Claims expose the active gate path and breadcrumb in `workflow_guidance`; task-specific RFCs and gate-targeted blackboard messages are injected there alongside current RFC broadcasts, while the general `blackboard` field retains task, sibling, and milestone context.

[`validate_seam_reachability`](../../crates/forge-tasks/src/engine/gates/seam.rs) checks the current test source against indexed call and import evidence, follows module imports and public re-exports, and permits up to four test-helper hops before requiring an admitted production target. When source drift invalidates an indexed route, the gate reports `TASK_SEAM_STALE` with an index-refresh action. A disconnected seam reports discovered calls, coverage, reached targets, and the reason traversal stopped.

The shared [contextunity-forge skill](../../skills/contextunity-forge/SKILL.md#acdd-profiles-and-task-operations) is what an agent loads for tool choice and delivery. The claim's `workflow_guidance` is what that agent follows for the current gate. [CLI commands](cli.md) lists the terminal forms, including milestone commands, which stay CLI-only.

## Headless task, milestone, plan and workflow execution

The [CLI orchestration commands](cli.md#structured-plans) share canonical profiles,
gate cursors, proof validation and process supervision. Task runners advance the
existing persisted task lifecycle. Milestone runners validate dependency waves,
dispatch that lifecycle serially in-place or through owned isolated worktrees,
and integrate accepted deliveries before the next task. Root gate observations
stay in existing transition records; profile, authority and content bindings
control restart reuse. No separate task or gate registry is introduced.

[`run_standalone_workflow`](../../crates/forge-tasks/src/engine/orchestrator/workflow.rs)
executes a workflow's taskless root graph with ephemeral observations and
`save: none`. Commands reuse the existing output parser and supervised process
executor. Agent roles receive actual independent sessions and must return a
structured decision with typed proof. Rejections carry structured findings to
the retained target session. Exit status alone cannot supply role acceptance.
Task containers and persistent receipts are refused for these workflows.

[`run_plan_workflow`](../../crates/forge-tasks/src/engine/orchestrator/plan.rs)
projects production-indexed `doc_type: plan` sections to source-owned steps.
Validated dependency order controls execution; the plan's source status and
evidence remain its authority. Snapshot persistence preserves HEAD and staged
content. Explicit profiles must already be task-only and snapshot-only.
Plan delivery calls the same subtask completion/evidence guard as task delivery;
immutable gates preserve their accepted candidate. Completed source steps with
evidence are not replayed, and concurrent source edits fail closed.

Milestone verification executes the exact current root command cursor. Autonomous
handoff consumes a genuine independent role decision and Forge-observed command
results bound to final authority, implementation and explicit target-base
content. The existing handoff validator remains authoritative. A route without
independent review cannot invent evidence to complete handoff. See
[milestone commands](cli.md#repository-milestones) for recovery and failure effects,
and [global options](cli.md#global-options) for immutable configuration selection
across CLI commands and the server lifetime.

## Documentation and instructions co-evolution

A task that changes observable behavior, public interfaces, workflows, configuration, or operational assumptions updates the matching documentation and agent instructions during its build stage. Follow the [shared skill's task guidance](../../skills/contextunity-forge/SKILL.md#acdd-profiles-and-task-operations).
