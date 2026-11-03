---
title: "Augmented Context-Driven Development"
doc_type: guide
---

# Augmented Context-Driven Development

ACDD is how Forge runs milestone work. People keep the contract in Git (`docs/milestones/*.md`). Forge syncs tasks into `.forge/tasks.sqlite` and serves them to agents through `task_list`, `task_claim`, and `task_submit`. A code-index rebuild does not reset that queue.

The active profile names the gates, their order, the proof each gate requires, the roles, and the review contours. The profile compiled into Forge uses `contract`, `build`, `review`, and `deliver`. A repository file at `.forge/acdd/profile.yaml`, or a pin on a milestone or task, replaces that shape. Writing one is covered in [ACDD profiles](profiles.md).

Claiming a gate returns two kinds of context. `workflow_guidance` names the role, the steps, and the tools that fit the gate. The context bundle carries the material for that role: scope and symbols on contract and build, the candidate snapshot on review, the receipt rules on deliver. Recent blackboard notes ride along. Historical gate logs stay in SQLite until someone asks `task inspect` for them. [Task operations](tasks.md) specifies the arguments, evidence objects, and receipts. [ADR 0016](../adr/0016-lean-stage-tailored-task-context-bundles.md) records why the bundle changes by gate.

MCP task tools and CLI task commands present results as Markdown by default;
use MCP `format: "json"` or CLI `--format json` when parsing values. The shared
layout preserves stage guidance and evidence and gives embedded blockers a
recoverable caution callout. See the
[task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle).

The embedded profile assigns contract author, builder, and independent reviewer roles. The contract author posts the draft as a blackboard `draft` targeted to `build`; the reviewer continues from review through delivery in the same worker session. Workflow guidance includes `NO_PREFLIGHT_SUBAGENTS`, the registered combined Rust command, and scoped test commands. Contract and build context includes indexed scope symbols, external callers, covering tests, ADR invariant summaries, and a task-bound expected evidence template. The task engine rejects disconnected contract seams and public signature changes whose indexed callers fall outside admitted scope.

## Hierarchical gate cursors and seam proofs

A profile gate can contain child gates. [`claim_nested_gate`](../../crates/forge-tasks/src/engine/claim.rs) resolves the persisted stage to its active leaf gate, applies that leaf's role and proof requirements, and accepts qualified child stage IDs. Passing a child advances the cursor to the next child. Claims expose the active gate path and breadcrumb in `workflow_guidance`; task-specific RFCs and gate-targeted blackboard messages are injected there alongside current RFC broadcasts, while the general `blackboard` field retains task, sibling, and milestone context.

[`validate_seam_reachability`](../../crates/forge-tasks/src/engine/gates.rs) checks the current test source against indexed call and import evidence, follows module imports and public re-exports, and permits up to four test-helper hops before requiring an admitted production target. When source drift invalidates an indexed route, the gate reports `TASK_SEAM_STALE` with an index-refresh action. A disconnected seam reports discovered calls, coverage, reached targets, and the reason traversal stopped.

The shared [contextunity-forge skill](../../skills/contextunity-forge/SKILL.md#acdd-authority-and-scope) is what an agent loads for tool choice and delivery. The claim's `workflow_guidance` is what that agent follows for the current gate. [CLI commands](cli.md) lists the terminal forms, including milestone commands, which stay CLI-only.

## Documentation and instructions co-evolution

A task that changes observable behavior, public interfaces, workflows, configuration, or operational assumptions updates the matching documentation and agent instructions during its build stage. Follow the [shared skill's co-evolution procedure](../../skills/contextunity-forge/SKILL.md#acdd-authority-and-scope).
