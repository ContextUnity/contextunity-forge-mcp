---
title: "Universal Multi-Worktree Resolution & Zero-Jump Architecture"
doc_type: plan
status: approved
---

# Universal Multi-Worktree Resolution & Zero-Jump Architecture

## Executive Summary

This source plan preserves the implementation requirements for universal MCP
worktree routing and stable server roots. The accepted
[milestone 056 contract](../milestones/archive/056-universal-multi-worktree-resolution.md#outcome-and-authority)
owns scope and authority; handoff moves its manifest to that archive path.
This source plan retains selector and proof requirements.

The supplied baseline design identified three defects addressed by this plan:
1. **Asymmetric routing**: task tools were dispatched from the server root while code-navigation tools could select worktrees.
2. **Claim-driven default roots**: an omitted selector could follow an active task claim rather than retain the server root.
3. **Restricted matching**: exact names and trailing suffixes did not cover numeric milestone aliases or middle slugs.

These are baseline notes, not current runtime claims. See the [MCP worktree reference](../reference/mcp-tools.md#worktree-selection) for the live contract.

This plan establishes a universal multi-worktree MCP interface in which every
tool can target a worktree per request without claim-driven root changes.

---

## Architecture Specification

```
                               ┌────────────────────────────────────────────────────┐
                               │ Forge MCP server: canonical server root + DB       │
                               └─────────────────────────┬──────────────────────────┘
                                                         │
                Any MCP tool (worktree: "023" or "control-member-repair")
                                                         │
                                                         ▼
                                 ┌──────────────────────────────────────────────┐
                                 │ Shared worktree resolver                    │
                                 │ "." / explicit path; exact name; prefix or   │
                                 │ zero-normalized number; suffix; substring    │
                                 └─────────────────────┬────────────────────────┘
                                                       │
                         Same request-local routing for all 20 MCP tools
                                                       │
                                                       ▼
                                  Selected worktree: .worktrees/023-sql-...
                    ┌─────────────────────────────────────────────────────────────┐
                    │ Branch-local: adapter/config, code index, milestone files,  │
                    │ and checkpoints; Git-linked worktrees share the task DB.   │
                    └─────────────────────────────────────────────────────────────┘
```

---

## Detailed Implementation Steps

### Step 1: Flexible Worktree Matching in `src/mcp/server.rs`

Implement the shared resolver for explicit `worktree` selectors:

#### Matching Cascade Order:
1. **Direct root or path**: `"."` selects the server root. An absolute path or a path relative to the server root containing `/` or `\` selects that existing directory directly; a missing explicit path returns `WORKTREE_NOT_FOUND`.
2. **Collect candidates**: inspect Git-registered worktrees and immediate `.worktrees/` children, canonicalizing paths and deduplicating candidates.
3. **Exact name**: match the entire candidate directory name.
4. **Prefix / number**: match a literal prefix. If the selector is all digits, compare the candidate's milestone-number prefix after removing leading zeroes from both digit strings; do not parse through a bounded integer type. Thus `"23"` and `"023"` select the same `023-*` candidate.
5. **Suffix**: match a literal suffix.
6. **Substring**: match a literal substring.
7. **Ambiguity and missing results**: stop at the first nonempty tier. One candidate resolves; multiple candidates return `WORKTREE_AMBIGUOUS` with candidates and do not fall through; no candidate, including an empty selector, returns `WORKTREE_NOT_FOUND`.

---

### Step 2: Keep the default root stable in `resolve_target_root_and_db`

When `worktree` is omitted, retain the canonical server root and its configured
index database. Do not inspect active task claims to select another root.
For an explicit selector, resolve the canonical selected root and use its
branch-local adapter/configuration, milestone manifests, and default code index.
Selecting the server root preserves a custom server `--db` path. Git-linked
worktrees continue to share the task database through existing task-store
resolution; standalone repositories stay independent unless explicitly
configured to share a store. Do not migrate task storage.

---

### Step 3: Remove tool-specific routing in `src/mcp/tools.rs`

Route every MCP tool through the same resolver and request-local server,
including all `code_map_*` and task tools, `forge_guide`, and
`session_checkpoint`. Remove `is_task_tool` and any task-only dispatch bypass.
Keep invalid-format checks before mutations and retain strict unknown-field
rejection.

---

### Step 4: Add routing context to MCP tool inputs

Expose `worktree` on all 20 MCP tools, including `TaskBlackboardInput`,
`TaskListInput`, `TaskClaimInput`, `TaskSubmitInput`, `TaskManageInput`,
`forge_guide`, and `session_checkpoint`.

- `task_claim` and `task_manage(action="next")` require a selector. Resolve it
  before engine execution so the claim stores the canonical selected path.
- Other task-management actions accept `worktree` only as routing context; keep
  their action-specific engine validation unchanged.
- `task_submit` accepts an optional routing selector without changing claim,
  candidate, authority, reviewer-independence, or evidence-identity validation.
- Route task manifests and adapter configuration from the selected branch while
  preserving the shared task database for Git-linked worktrees.
- Keep code indexes and checkpoints local to the selected branch. Keep all
  unknown-selector rejection and response-budget rules intact.

---

### Step 5: Strengthen existing MCP contract seams

Extend the existing production stdio seams in `tests/mcp/context.rs` with
parameterized cases rather than duplicate standalone test functions:

1. **Numeric prefixes**: aliases with and without leading zeros, including
   `"023"`/`"23"` and `"060"`.
2. **Slug matching**: literal prefix, middle substring, and suffix selectors.
3. **Precedence and errors**: exact-name precedence, explicit paths, ambiguous
   candidates, missing selectors, and empty selectors.
4. **Universal task routing**: branch-only milestone manifests through task
   sync/list/blackboard, with task state in the shared Git task database.
5. **Stable defaults and affinity**: omitted selectors remain at the server root
   before and after active-claim transitions; claim and next persist canonical
   selected paths; submit keeps existing identity validation.
6. **Tool coverage and locality**: all 20 schemas accept `worktree`; checkpoints
   and indexes remain worktree-local.
7. **ACDD fixtures**: use Git-linked worktrees or explicit shared task-database
   configuration where independent repositories need a shared store.

---

## Acceptance Criteria

- `worktree: "023"` and `"23"` resolve the corresponding zero-padded numeric
  milestone prefix without requiring a full path; `"060"` resolves similarly.
- Literal prefixes, suffixes, and substrings follow the documented cascade,
  with exact names taking precedence and ambiguous tiers rejected.
- Explicit paths and `"."` select directly; missing and empty selectors fail
  with `WORKTREE_NOT_FOUND`.
- Every MCP tool routes through the selected root; task manifests/configuration
  come from that branch while Git-linked worktrees share the task database.
- Omitted `worktree` always retains the server root regardless of task claims.
- Task claim and next persist canonical paths; submit retains existing evidence
  and claim identity validation.
- Branch-local indexes and checkpoints remain isolated; unknown selectors and
  invalid formats fail before mutation.

## Supplied historical verification (not candidate evidence)

The following test counts and observations were supplied with the source plan.
They are retained as historical input only, were not independently verified
for this candidate, and do not establish its acceptance or build proof. No
candidate verification is recorded here.

The existing MCP worktree seams cover numeric aliases with and without leading
zeros, prefixes, suffixes, substrings, explicit paths, exact-name precedence,
ambiguous and missing selectors, claim-independent defaults, and branch-only
task manifests. ACDD lifecycle fixtures use Git-linked worktrees or explicit
shared task-database configuration when selecting independent repositories.

- `cargo test --test mcp`: 90 passed, 0 failed.
- `cargo clippy --all-targets --all-features -- -D warnings`: exit 0.
- `cargo test --test acdd`: 115 passed, 1 failed. The unrelated, already-modified
  `skills/contextunity-forge/references/acdd_profile.yaml.example` contains an
  unsupported `version` field; its contents remain unchanged by this implementation.
- `cargo test --all-targets -- --skip declarative_profile_compiles_defaults_and_respects_role_overrides`:
  814 passed, 0 failed, 1 ignored, 1 filtered. This supplementary run excludes
  only the profile-example failure above and is not an unfiltered full-suite pass.
- Live MCP stdio checks verify stable defaults with active claims, aliases,
  branch-only task sync/list/blackboard operations, canonical persisted
  claim/next worktree paths, checkpoint isolation, and all 20 tool schemas.
