---
title: "Milestones"
doc_type: guide
---

# Milestones

Milestone commitment files in this directory (`0XX-*.md`) define the repository's execution queue and lifecycle commitments.

## Ownership and Truth

- **Single Source of Truth:** Each milestone file (`docs/milestones/0XX-*.md`) owns its own status (`active`, `planned`, `deferred`, `completed`), invariants, task breakdown, and acceptance outcomes.
- **Queue Order:** Numeric filename prefixes (`010-*.md`, `020-*.md`, ...) supply the queue execution order.
- **Archive:** Completed milestones are moved to [archive/](archive/README.md) along with their proof receipts.

To find the next active commitment, discover the lowest-numbered file with `status: active` in this directory. A `planned` milestone whose `depends_on` predecessor is still active stays blocked. The current execution focus is milestone 020.
