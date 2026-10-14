---
title: "Milestone instructions"
doc_type: guide
---

# Milestone Instructions

Read [milestone taxonomy and naming groups](README.md#milestone-taxonomy-and-naming-groups) before authoring, naming, or admitting milestone commitments.

## Task design and documentation scope

When authoring task contracts that introduce or modify user-facing behavior, CLI/MCP tools, lifecycle diagnostics, or system invariants:
- Include the affected documentation files (under `docs/`) directly in the task's initial `scope:`.
- Ensure code changes and their corresponding documentation evolve together within the task boundary rather than deferring documentation to subsequent milestones.

## Milestone handoff and archival

During the milestone handoff stage:
- Execute the required milestone verification gate from [`TESTS.md`](../../TESTS.md#milestone-handoff-gate).
- Run `contextunity-forge-mcp milestone handoff <id> ...` to archive the manifest and rewrite landed task receipts.
- Stage the archived manifest in `archive/` and stage the removal of the active working manifest.
- Ensure any obsolete or untracked pre-archive working manifest files (such as `docs/milestones/<id>-...md`) are unlinked so that branch integration and fast-forward landing into `main` remain completely clean without untracked duplicates.
