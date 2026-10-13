# ContextUnity Forge MCP — Agent Router

## Read before work

- Read [documentation instructions](docs/AGENTS.md) before documentation changes.
- Read [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md) before structural changes.
- Read [src/AGENTS.md](src/AGENTS.md) before source code changes and module decomposition.
- Read [roadmap](docs/roadmap.md) and [milestones](docs/milestones/README.md) for admitted direction and commitments.
- Read [tests/AGENTS.md](tests/AGENTS.md) for test placement and authoring boundaries.

## Routes

- Runtime behavior and setup: [README.md](README.md) and [docs](docs/).
- Architecture and modularity: [src/AGENTS.md](src/AGENTS.md), [architecture](docs/architecture/README.md), and [decisions](docs/adr/README.md).
- Interfaces: [configuration and tools](docs/reference/README.md), [task operations](docs/reference/tasks.md), [ACDD navigation](docs/reference/acdd.md), and [CLI commands](docs/reference/cli.md).
- Operations: [runbooks](docs/runbooks/README.md).
- Planning: [roadmap](docs/roadmap.md), [milestones](docs/milestones/README.md), and [plans](docs/plans/README.md).
- Verification: [required gates](TESTS.md) and the [testing guide](docs/testing/README.md).
- Forge tool discovery and task operations: load the shared `contextunity-forge` skill.

Use Forge MCP tools or the `contextunity-forge-mcp` CLI on the surface that provides the needed operation. Follow the active task profile and the `workflow_guidance` returned by task claim for gate sequence, proof policy, roles, models, review contours, and profile-specific behavior. Task builds run the affected test target and strict Clippy. Follow [TESTS.md](TESTS.md) for the exact task and milestone verification commands and count rules.

## Worktrees and Git permissions

Worktrees live at `.worktrees/<milestone-prefix>-<slug>` for milestones and `.worktrees/<milestone-prefix>-<task-slug>` for concurrent tasks.

### Permitted Git operations inside active milestone workflows

- Task commits:
  - With `auto_commit: true`, Forge automatically creates the delivery commit and records the receipt in SQLite; do not create duplicate commits.
  - With `auto_commit: false`, create exactly one scoped task commit containing task changes and the generated receipt after delivery.
  - Snapshot immutability: Never modify files during `deliver` stage or after `review`. Any edit triggers `TASK_CANDIDATE_MISMATCH`. If delivery fails, restore the worktree to the candidate snapshot before submitting rejection (`task submit --stage deliver --action reject`).
- Task repacking: Before milestone handoff, ensure exactly one commit per task, squashing intermediate commits if any. Record actual task commit SHAs in the milestone document.
- Branch integration: Merge each delivered task branch into the milestone branch, delete the temporary task branch, and remove its worktree (`git worktree remove .worktrees/<milestone-prefix>-<task-slug>`).
- Handoff commit: When all milestone tasks are delivered and verified, run `contextunity-forge-mcp milestone handoff` in the milestone worktree. Commit the archived milestone document, along with any out-of-scope adjustments, deferred fixes, and recorded findings, documenting them in the handoff commit message.
- Milestone landing: In the primary worktree (`main`), fast-forward the milestone branch (`git merge --ff-only <milestone-branch>`), clean up untracked or obsolete pre-archive milestone working files (e.g. `docs/milestones/<id>-...md` archived to `docs/milestones/archive/`), remove the milestone worktree (`git worktree remove .worktrees/<milestone-prefix>-<slug>`), and delete the milestone branch (`git branch -d <milestone-branch>`).

### Operations requiring explicit user approval

- Commits on `main`: Do not create Git commits directly on `main` without explicit user approval.
- Dangerous Git operations: History rewrites (`git reset --hard`, destructive rebases, force pushes), branch deletion outside task/milestone cleanup, and repository-level state clearing require explicit user approval.
- Remote publications: `git push`, tagging, releases, and credential publication require explicit user approval.

Do not run benchmarks without explicit user approval. Read [benchmark instructions](benchmarks/AGENTS.md) before benchmarking.
