---
id: m-universal-multi-worktree-resolution
title: Universal Multi-Worktree Resolution and Stable MCP Roots
doc_type: contract
status: active
started_at: 2026-10-21T12:00:21.034253225+00:00
owners:
  - src/mcp/
  - src/engine/tasks/mod.rs
  - tests/mcp/
  - tests/acdd/
  - docs/reference/
  - skills/contextunity-forge/
invariants:
  - 'INV-STABLE-SERVER-ROOT: An omitted worktree always retains the canonical server root and configured index database, independent of active task claims, task settings, or claim transitions.'
  - 'INV-UNIVERSAL-WORKTREE-ROUTING: Every MCP tool uses the same explicit worktree resolver; task manifests and adapters come from the selected root while Git-linked worktrees retain their shared task store.'
  - 'INV-WORKTREE-MATCH-PRECEDENCE: Explicit paths and dot select directly; otherwise match exact name, prefix or zero-normalized milestone number, suffix, then substring. The first nonempty tier must have exactly one canonical candidate; missing and ambiguous selectors fail closed.'
  - 'INV-CANONICAL-CLAIM-AFFINITY: Task claim and next persist the canonical selected path, never an unresolved numeric or slug selector; submit retains existing claim, candidate, authority, and reviewer independence checks.'
related_plans:
  - docs/plans/universal-multi-worktree-resolution.md
---

# Universal Multi-Worktree Resolution and Stable MCP Roots

## Outcome and authority

Implement [Universal Multi-Worktree Resolution](../plans/universal-multi-worktree-resolution.md).
The detailed implementation steps own matching precedence where the overview diagram differs.
The source plan's existing verification section is historical input, not proof of this candidate.

## Executable architecture

MCP request `worktree` → `Server::call_tool` → `resolve_target_root_and_db` →
`resolve_explicit_worktree` → request-local server/adapter/index → existing engine
operation → selected branch manifests and shared Git task SQLite → MCP result.

Without a selector, use the server root and its configured database. With a selector,
use the canonical selected root and its own default code index. Keep `.` explicit,
absolute and relative paths, registered external siblings, `.worktrees` candidates,
canonical deduplication, custom server database paths, and linked-server defaults.
Pure digit aliases compare milestone prefixes with leading zeros removed. Match
non-numeric prefixes, suffixes, and substrings literally. Reject empty, missing,
and ambiguous selectors without invoking the operation or mutating task state.

Route `task_list`, `task_blackboard`, `task_manage`, `task_claim`, and `task_submit`
identically to navigation tools. Keep required claim/next affinity semantics;
canonicalize their selectors before engine execution. A selected branch-only
manifest must be discoverable without copying it into main. Preserve task store
ownership, gate proofs, response budgets, strict unknown argument rejection,
configuration validation, and checkpoint isolation. Remove obsolete implicit-claim
routing helpers/state when no consumer remains. No storage migration, new tool,
implicit claim fallback, retry policy, or cross-repository behavior expansion.

## Proof and execution

Strengthen the existing `tests/mcp/context.rs` production stdio seams with
parameterized cases instead of duplicate standalone tests. Include `023`/`23`,
`060`, prefix, middle slug, suffix, exact precedence, ambiguity/missing/empty,
explicit paths, primary/linked stable defaults before and after claim transitions,
branch-only task sync/list/blackboard, canonical claim/next paths, submit, and
schema support. Adapt existing ACDD fixtures only where universal dispatch exposes
their assumed root/store relationship; retain their behavioral assertions.

Contract proof must compile and fail at runtime (101) against baseline routing.
Sensitivity: baseline implicit-claim selection or task routing bypass must fail
these same seams; disabling numeric/prefix/substring matching must fail selector
cases. Reuse the observed baseline red run where it proves sensitivity.

Build and review run the complete affected MCP and ACDD targets, focused contract
proof, and strict Clippy. A throwaway live MCP stdio scenario must observe selected
roots, branch-only manifests, canonical claim paths, and stable defaults. Remove
throwaway harnesses after recording evidence. Before handoff, independently scan
admitted docs, review the final combined content against main, and run the exact
compound gate in `TESTS.md` without filters. No benchmark execution.

## Tasks in this milestone

### task: universal-worktree-hub

```yaml
task_ref: universal-worktree-hub
target: Implement universal explicit MCP worktree routing, flexible selectors, stable defaults, canonical task affinity, and co-evolving documentation
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/mcp/server.rs
  - src/mcp/tools.rs
  - src/mcp/tools/inputs.rs
  - src/engine/tasks/mod.rs
  - tests/mcp/context.rs
  - tests/mcp/protocol.rs
  - tests/acdd/
  - README.md
  - docs/reference/mcp-tools.md
  - docs/reference/tasks.md
  - docs/reference/mcp-setup.md
  - docs/reference/cli.md
  - docs/runbooks/limits-and-freshness.md
  - docs/plans/universal-multi-worktree-resolution.md
  - skills/contextunity-forge/SKILL.md
  - skills/contextunity-forge/references/
status: ready
```

## Deferred and out-of-scope defects

```yaml
deferred_defects: []
```
