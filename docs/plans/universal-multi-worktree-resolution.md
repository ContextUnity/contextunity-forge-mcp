---
title: "Universal Multi-Worktree Resolution & Zero-Jump Architecture"
doc_type: plan
status: approved
---

# Universal Multi-Worktree Resolution & Zero-Jump Architecture

## Executive Summary
This document provides a complete, step-by-step implementation plan to resolve root-jumping and worktree split-brain defects in ContextUnity Forge MCP.

Currently, running a single MCP server from the repository root causes:
1. **Asymmetric Routing (`is_task_tool`)**: Task tools (`task_blackboard`, `task_list`, etc.) unconditionally force `target_root = main`, discarding the `worktree` parameter entirely, while code navigation tools (`code_map_*`) dynamically switch to worktrees.
2. **Spooky Root Jumping (`active_claim_worktree`)**: When `worktree` is omitted, the server checks SQLite for any unended task claim and silently re-points code navigation tools to that claim's worktree. When the claim ends or transitions, the root unpredictably snaps back to `main`.
3. **Restricted Worktree Resolution**: Worktree selection only supports exact directory names or trailing suffixes (`-slug`), failing completely on milestone/task numbers (e.g., `023`, `060`) or middle slugs (`sql-schema`).

This plan fixes these flaws by establishing a **Universal Multi-Worktree Hub Architecture** where a single server running at the repository root predictably serves any worktree on demand via number, slug, or explicit path.

---

## Architecture Specification

```
                               ┌──────────────────────────────────────────────────┐
                               │       Forge MCP Server (Root: repository main)   │
                               │      Shared SQLite: .forge/tasks.sqlite          │
                               └────────────────────────┬─────────────────────────┘
                                                        │
                      Incoming Tool Request (worktree: "023" OR "control-member-repair")
                                                        │
                                                        ▼
                                   ┌─────────────────────────────────────────┐
                                   │   Enhanced resolve_explicit_worktree    │
                                   │   1. Number / Prefix match ("023" -> 023-*)  │
                                   │   2. Exact Directory Name Match             │
                                   │   3. Suffix Match ("*-repair")               │
                                   │   4. Slug Substring Match ("*sql-schema*")  │
                                   │   5. Absolute / Relative Path (Last Resort) │
                                   └────────────────────┬────────────────────┘
                                                        │
                         Identical Resolution for ALL Tools (code_map_* AND task_*)
                                                        │
                                                        ▼
                                    Target Worktree: .worktrees/023-sql-...
                        ┌────────────────────────────────────────────────────────┐
                        │ • Code Index: .worktrees/023-.../.forge/code_map.sqlite│
                        │ • Task Manifests: .worktrees/023-.../docs/milestones/  │
                        │ • Blackboard: .worktrees/023-... (branch scoped)       │
                        └────────────────────────────────────────────────────────┘
```

---

## Detailed Implementation Steps

### Step 1: Flexible Worktree Matching in `src/mcp/server.rs`

Update `resolve_explicit_worktree` in `src/mcp/server.rs` around line 559 to prioritize human-friendly numbers and slugs:

#### Matching Cascade Order:
1. **Direct Path Check (Absolute or Explicit Relative with separators)**:
   If `requested` contains `/` or `\` and `path.is_dir()`, resolve canonical path.
2. **Collect Candidates**:
   Scan `.worktrees/*` and `git worktree list --porcelain -z`.
3. **Exact Name Match**:
   `name == requested` (e.g., `023-sql-schema-indexing-entrypoints-and-topology-slices`).
4. **Prefix / Number Match**:
   `name.starts_with(&format!("{requested}-"))` or `name.starts_with(requested)` (e.g., `"023"` or `"23"` matches `023-*`).
5. **Suffix Match**:
   `name.ends_with(&format!("-{requested}"))` or `name.ends_with(requested)` (e.g., `"topology-slices"`).
6. **Unique Substring / Slug Match**:
   `name.contains(requested)` (e.g., `"sql-schema"`).
7. **Ambiguity & Missing Handling**:
   - Exactly 1 match: return `Ok(canonical_path)`.
   - 0 matches: return `Err(WORKTREE_NOT_FOUND: no worktree matching '{requested}')`.
   - > 1 matches: return `Err(WORKTREE_AMBIGUOUS: '{requested}' matches [a, b]. Pass a more specific number or name.)`.

---

### Step 2: Remove Spooky Action-at-a-Distance in `resolve_target_root_and_db`

In `src/mcp/server.rs` around line 484:
1. **Default to Server Root when `requested_worktree` is None**:
   Do NOT query `task_claims` for an arbitrary unended claim to hijack `self.root`.
   When `requested_worktree.is_none()`, the server root remains **stable and deterministic**: `target = context.root.clone()`.
2. **Worktree Isolation**:
   When `requested_worktree` is provided, resolve `target_root = target` and `target_db = target.join(".forge/code_map.sqlite")`.
   If `.forge/code_map.sqlite` does not exist in the worktree, fallback to `cli::default_db(&target)`.

---

### Step 3: Eliminate `is_task_tool` Discrimination in `src/mcp/tools.rs`

In `src/mcp/tools.rs` around line 905:

#### Before:
```rust
let (target_root, target_db) = if is_task_tool(&request.name) {
    (self.root.clone(), self.db.clone())
} else {
    match self.resolve_target_root_and_db(requested_worktree, &primary_adapter) {
        Ok(pair) => pair,
        Err(error) => ...
    }
};
```

#### After:
```rust
let (target_root, target_db) = match self.resolve_target_root_and_db(requested_worktree, &primary_adapter) {
    Ok(pair) => pair,
    Err(error) => {
        return Ok(response::result_for(
            Err(error),
            &ResponsePolicy::default(),
            output_format,
            operation,
            &tool_name,
            None,
        ).into());
    }
};
```
Delete `is_task_tool` helper function completely. All tools dispatch through `request_server = self.with_root_and_db(target_root, target_db, adapter.clone())`.

---

### Step 4: Add `worktree` to Task Input Schemas in `src/mcp/tools/inputs.rs`

In `src/mcp/tools/inputs.rs`, add `pub worktree: Option<String>` to:
1. `TaskBlackboardInput`:
   ```rust
   pub struct TaskBlackboardInput {
       #[serde(default)]
       pub worktree: Option<String>,
       #[serde(default)]
       pub format: Option<OutputFormat>,
       #[serde(flatten)]
       pub request: BlackboardRequest,
   }
   ```
2. `TaskListInput`:
   ```rust
   pub struct TaskListInput {
       #[serde(default)]
       pub worktree: Option<String>,
       #[serde(default)]
       pub format: Option<OutputFormat>,
       #[serde(flatten)]
       pub query: TaskListQuery,
   }
   ```
3. `TaskClaimInput` & `TaskSubmitInput` & `TaskManageInput`:
   Ensure `worktree` is accepted and passed through to the command execution context.
4. Update `rejected_selector_fields` in `src/mcp/tools.rs` to allow `"worktree"` across all task tools.

---

### Step 5: Test Verification Suite

Add dedicated integration tests in `tests/mcp/context.rs`:

1. **`test_worktree_resolution_by_number_prefix`**:
   - Setup a repo with `.worktrees/023-sql-schema` and `.worktrees/060-acdd`.
   - Call `code_map_overview(worktree: "023")` -> resolves to `023-sql-schema`.
   - Call `code_map_overview(worktree: "060")` -> resolves to `060-acdd`.
2. **`test_worktree_resolution_by_slug`**:
   - Call `code_map_search(worktree: "sql-schema", pattern: "...")` -> resolves correctly.
3. **`test_task_blackboard_in_worktree`**:
   - Place a milestone manifest `docs/milestones/023-*.md` with `status: active` only in the worktree branch.
   - Call `task_blackboard(worktree: "023", action: "read")`.
   - Verify it successfully finds the milestone and returns messages without reporting "Milestone not found".
4. **`test_server_root_stability_without_worktree`**:
   - Create an active claim in `.worktrees/022-repair`.
   - Call `code_map_overview` without `worktree`.
   - Verify it returns `primary` workspace metadata and does NOT jump to `022-repair`.
5. **`test_ambiguity_rejection`**:
   - Create `.worktrees/010-foo-bar` and `.worktrees/020-baz-bar`.
   - Call `worktree: "bar"` -> asserts error contains `WORKTREE_AMBIGUOUS`.

---

## Acceptance Criteria
- [x] Passing `worktree: "023"` resolves to `.worktrees/023-*` without needing the full path.
- [x] Passing `worktree: "sql-schema"` resolves to `.worktrees/023-sql-schema-*`.
- [x] `task_blackboard` and `task_list` respect the `worktree` parameter identically to `code_map_*`.
- [x] Omitting `worktree` never jumps roots based on background claims.
- [x] All tests in `tests/mcp/context.rs` compile and pass with strict Clippy checks.

## Implementation verification

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
