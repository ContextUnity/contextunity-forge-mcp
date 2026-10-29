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
