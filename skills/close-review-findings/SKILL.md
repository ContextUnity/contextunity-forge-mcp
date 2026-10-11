---
name: close-review-findings
description: Diagnose and fix any defect, and classify, remediate, or evidence-close findings from Pi reviews, pull-request reviews, audits, or repeated review rounds.
---

# Close Review Findings

## Repository Routing

Read the nearest repository `AGENTS.md` before using this skill. It selects the
owning review launcher, architecture mapper, required gates, and evidence store.

**IMPORTANT: THE GOAL IS NOT TO CLOSE FINDINGS. THE GOAL IS TO PROVIDE A
COMPREHENSIVE FIX.**

A finding ID going quiet is not success. Success is one restored owner-cluster
invariant across every equivalent mutation, backend, caller, and persistence
seam — with Code Map / ast-grep / live call-site evidence.

## 1. Establish Authority and Scope

1. Read the nearest `AGENTS.md`, the implementation skill, the governing plan,
   and the review skill that produced the ledger.
2. Determine whether the request authorizes analysis only, implementation,
   repeated review, or lifecycle closure.
3. Review findings never override user decisions or frozen milestone contracts.

## 2. Attribute Every Finding

Assign exactly one origin: `introduced-regression`, `pre-existing-missed`,
`incomplete-remediation`, `duplicate`, `invalid`, or `plan-deferred`.

## 3. Confirm Reachability Structurally

1. Orient to the canonical owner and its boundary before searching callers.
2. Write ast-grep predicates and run across production and tests.
3. Use Forge MCP tools (`code_map_impact`, `code_map_inspect`) to trace callers.

## 4. Cluster By Invariant

Group confirmed findings by canonical owner and post-fix invariant, not by file
or symptom.

## 5. Prove and Repair

1. Add a focused regression test that fails before production changes.
2. Implement the smallest change that restores the cluster invariant.
3. Verify with strict checks and residual scans.
