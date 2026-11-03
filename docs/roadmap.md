---
title: "Forge MCP strategic direction"
doc_type: guide
---

# Strategic direction

Forge MCP focuses on precise, evidence-backed code navigation across supported
languages and large workspaces. Development improves extraction and resolution
quality, predictable query latency, compact storage, and reproducible cold and
incremental builds through measured production seams.

Repository-owned documentation and planning connect design intent to those
code relationships. Development strengthens the existing milestone/task
lifecycle, repository onboarding, isolated worktree delivery, and durable
completion receipts across projects.

## Execution Queue and Planned Milestones

- **`LS-002`**: SQL schema and query indexing, deterministic entrypoint classification, 1-hop directory boundary slices, and module topology.
- **`PF-001`**: Cold-build performance and HTML endpoint resolution repair, including splitting the Cargo Workspace into 3 independent crates (`forge-core`, `forge-index`, and `forge-tasks`) to bound cold-build RSS, decouple language analysis from lifecycle governance, and accelerate incremental compilation.
- **`IN-001`**: Seamless installer, multi-flavor distribution, macOS Darwin POSIX parity, and ACDD onboarding.
- **`UX-002`**: Autonomous command gates (`execute_command_gate` wired to zero-arg `task_submit` and `task verify`), lean verification diagnostics with conditional worktree-confined trace logs, profile output schemas (`commands.<id>.output`), write-scope pre/post guards, and unified layered `forge_read`.
- **`AC-003`**: Headless CLI lifecycle orchestrator (`task run` / `milestone run`), subagent process spawning (`role.runner`), rejection routing feedback loops, and autonomous wave execution.
