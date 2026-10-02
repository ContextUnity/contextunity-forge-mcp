---
title: "Milestone index"
doc_type: guide
---

# Milestones

## Admitted target contracts

- [Repository task lifecycle](010-repository-task-lifecycle.md): in-progress Rust
  implementation of repository-owned task operations and durable receipts;
  final acceptance waits for the owner's commit command.

## Commitment ownership

Milestones are Git Markdown commitments with stable IDs, status, owners,
dependencies, invariants, and acceptance outcomes. Ordered filename prefixes
supply queue order; check lifecycle and dependencies before selecting work.
Plans retain research and design rationale and link to admitted commitments.

Completed commitments live in [archive/](archive/README.md). Preserve their
IDs and proof receipts through moves. The task lifecycle milestone owns
future task admission and completion rules.
