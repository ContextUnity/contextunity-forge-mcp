# ContextUnity Forge MCP — Agent Router

## Read before work

- Read [documentation instructions](docs/AGENTS.md) before documentation changes.
- Read [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md) before structural changes.
- Read [roadmap](docs/roadmap.md) and [milestones](docs/milestones/README.md) for admitted direction and commitments.
- Read [tests/AGENTS.md](tests/AGENTS.md) for test placement and authoring boundaries.

## Routes

- Runtime behavior and setup: [README.md](README.md) and [docs](docs/).
- Architecture and performance: [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md).
- Interfaces: [configuration and tools](docs/reference/README.md), [task operations](docs/reference/tasks.md), [ACDD navigation](docs/reference/acdd.md), and [CLI commands](docs/reference/cli.md).
- Operations: [runbooks](docs/runbooks/README.md) and the [ACDD execution runbook](docs/runbooks/acdd.md).
- Planning: [roadmap](docs/roadmap.md), [milestones](docs/milestones/README.md), and [plans](docs/plans/README.md).
- Verification: [required gates](TESTS.md) and the [testing guide](docs/testing/README.md).
- Forge tool discovery and task operations: load the shared `contextunity-forge` skill.

Use Forge MCP tools or the `contextunity-forge-mcp` CLI on the surface that provides the needed operation. Follow the active task profile and the `workflow_guidance` returned by task claim for gate sequence, proof policy, roles, models, review contours, and profile-specific behavior. Task builds run the affected test target and `cargo clippy --all-targets --all-features -- -D warnings`. The `TESTS.md` milestone gate is `cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`.

## Git permissions

Forge ACDD grants standing permission inside an active milestone workflow:

- With `auto_commit: true`, Forge commits the candidate tree and milestone receipt during delivery. Do not create a duplicate task commit.
- With `auto_commit: false`, create one scoped task commit containing task changes and the generated receipt after delivery.
- Merge each delivered task branch into the milestone branch and remove its temporary branch and worktree.
- After all milestone tasks are delivered and verified, run `milestone handoff`, then commit the archived milestone and merge the completed milestone branch into its target.

Load the shared `commit-workflow` skill before authorized manual task or archive commits. Stage only scoped task changes and generated receipts. Require explicit approval for unrelated commits, pushes, publication, and repository-level cleanup.

Do not run benchmarks without explicit user approval. Read [benchmark instructions](benchmarks/AGENTS.md) before benchmarking.
