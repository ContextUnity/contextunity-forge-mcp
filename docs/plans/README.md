---
title: "Plans"
doc_type: guide
---

# Plans

The `docs/plans/` directory holds active design documents, architectural proposals, research notes, and pre-implementation drafts.

## Lifecycle and Ownership

- **Proposals and Drafts**: New technical designs, comparative audits, and subsystem proposals originate here as plans.
- **Admission to Milestones**: Once a plan reaches consensus and satisfies architectural review, its commitments and tasks are admitted into [`docs/milestones/`](../milestones/README.md).
- **Durable Knowledge**: Architectural invariants and verified topologies migrate to [`docs/architecture/`](../architecture/README.md), while accepted structural choices are recorded in [`docs/adr/`](../adr/README.md).
- **Index Scope**: Plans remain outside the default active MCP code index to preserve high signal-to-noise ratio in code search, while remaining accessible through direct file reads.
