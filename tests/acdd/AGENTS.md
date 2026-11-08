# ACDD Test Suite — Router and Module Invariants

This document defines the architectural boundaries and test authoring invariants for the ACDD test suite (`tests/acdd.rs`, `tests/acdd/`).

## Sub-domain Routes

- **Blackboard Coordination**: [blackboard.rs](blackboard.rs) — Targeted routing, topics, RFC message delivery, and payload size bounds (`INV-PAYLOAD-OVERFLOW-GUARD`).
- **MCP Tool Contracts**: [mcp.rs](mcp.rs) — JSON-RPC tool contracts (`task_claim`, `task_submit`, `task_manage`, `task_blackboard`, `task_list`).
- **Milestone Lifecycles**: [milestones/AGENTS.md](milestones/AGENTS.md) — Milestone initialization, synthesis, DAG verification, handoff, and recovery.
- **Subtask Lifecycle**: [subtasks.rs](subtasks.rs) — Subtask state transitions, evidence validation, and dependency ordering.
- **Task Engine & Lifecycles**: [tasks/AGENTS.md](tasks/AGENTS.md) — Task state machine, seam reachability, signature boundaries, workspaces, and retirement.
- **Shared Test Harnesses**: [support.rs](support.rs) and [tests/common/](../common/) — Reusable workspace builders, temp stores, and fixture factories.

## Non-Negotiable Invariants

### 1. Domain Cohesion over Monolithic God-Files
- Organize test files strictly along distinct behavioral domains.
- Never dump unrelated test concerns into an existing suite simply because it touches tasks or milestones. When a new behavioral domain or subsystem emerges, create a dedicated domain file or directory.

### 2. Shared Harness Reuse (`INV-TEST-HARNESS-REUSE`)
- **Zero Boilerplate Copy-Paste**: Do not inline repetitive file tree construction (e.g. `root.write("src/api.rs", ...)`).
- Use `tests/acdd/support.rs` and `tests/common/` factories (`acdd_workspace`, `Workspace`, and fixture builders). Common setup must be parameterized, not duplicated.

### 3. Table-Driven Permutations (`INV-TABLE-DRIVEN-PERMUTATION`)
- When testing permutations across languages (TypeScript, Python, JavaScript), status codes, or error states, use structured test case tables:
  ```rust
  for (lang, ext, code) in cases {
      assert_boundary(lang, ext, code);
  }
  ```
- Subsume duplicate test functions into a single table-driven function.

### 4. Positive Seams and Runtime-Red Contracts
- Extended capabilities must strengthen existing domain seams additively.
- Contract gate tests must compile cleanly and produce runtime assertion failures (exit code 101). Never introduce compilation errors.
