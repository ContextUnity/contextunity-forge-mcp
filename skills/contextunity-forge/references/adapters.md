# Workspace adapter defaults

Use the [adapter example](forge-mcp.yaml.example) for a starter `forge-mcp.yaml`. `contextunity-forge-mcp guide init` generates a starter file.

Each worktree's adapter lives at its repository root and is loaded from the selected MCP root. Linked workspace adapters live in sibling repositories listed by `linked_workspaces` and may supply their own roots and task metadata.

Without an adapter file, Forge scans `.` respecting `.gitignore`, excludes common build/cache directories (`target`, `node_modules`, `.git`, `.venv`, `__pycache__`), and writes `.forge/code-map.sqlite`. The task database defaults to `.forge/tasks.sqlite`; `task_repository` and `task_project` default to `forge-mcp`, `agents_guidance` to `AGENTS.md`, and the milestone directory to `docs/milestones/`.

For Git-linked worktrees, relative `tasks_db` paths resolve from the primary worktree root and share task state. Standalone repositories remain independent unless explicitly configured to share a store. Worktree selector precedence for MCP is described in the [tool catalog](tool-catalog.md#discovery-and-search).

The starter adapter demonstrates roots, documentation/milestone/plan paths, ignores, optional linked workspaces and linked task guidance, task identity, response limits, and the optional ACDD profile path. Its sample linked workspace uses sibling path `../shared-core`, with `src` roots and task milestone/guidance paths.
