---
title: "ADR 0014: Task Hierarchy, Visibility, and Blackboard Coordination"
doc_type: adr
status: accepted
date: 2026-10-07
---

# ADR 0014: Task Hierarchy, Visibility, and Blackboard Coordination

## Status

Accepted by milestone 014.

## Decision

Milestone lifecycle, task visibility, compact subtask summaries, and coordination messages follow one explicit hierarchy. Git milestone manifests remain the authority for milestone and task contracts; SQLite stores operational task state and scoped blackboard messages.

## Current behavior

### Milestone lifecycle

Milestones use exactly four states: `active`, `planned`, `completed`, and `cancelled`. A root manifest without a status defaults to `planned`; an archived manifest without one defaults to `completed`. Invalid frontmatter and unknown statuses fail closed. A cancelled manifest includes a non-empty `closure.reason` and remains in the archive as the durable explanation for cancellation.

> [!IMPORTANT]
> Invariant: `task sync` prunes a cancelled milestone's tasks, their outgoing dependencies, and its blackboard messages from SQLite. It preserves unsatisfied incoming dependency blocks on remaining tasks and preserves subtask execution status and evidence when a task contract revision changes.

### Task visibility and subtask detail

Task listings default to tasks belonging to active milestones. `milestone_status` selects `active`, `planned`, `completed`, or `all`; a targeted `milestone_ref` also selects a specific milestone. Before sync prunes a cancelled milestone, its tasks remain available through `all` or the targeted reference.

> [!IMPORTANT]
> Invariant: Task list entries show subtask references and statuses by default. Titles and verification evidence appear only through task inspection, an explicit full-detail request, or dedicated subtask operations.

### Blackboard hierarchy

Blackboard messages belong to a milestone, task, or subtask. An explicit `scope` selects that level. Milestone scope addresses only milestone-level messages. Task and subtask scopes resolve omitted identifiers only when one active context is unambiguous. With scope and keys omitted, resolution selects the unique in-progress task; multiple in-progress tasks fail closed; with no in-progress task, resolution falls back to the unique active milestone and fails closed if that context is ambiguous or absent.

> [!IMPORTANT]
> Invariant: Blackboard reads return summaries without payload, use pages of 10 messages by default and at most 50, order newest first, and include pagination metadata. `inspect` retrieves one message by `message_id`, including its payload.

### Documentation discovery

Overview `README.md` files at the roots of `docs/milestones/` and `docs/plans/` are indexed. Individual milestone and plan contracts remain outside `doc_search` indexing.

## Consequences

Milestone manifests preserve the durable lifecycle record, while SQLite can discard cancelled operational state and retain blockers on work that remains. Task listings provide a compact active queue, and agents can coordinate at the milestone, task, or subtask level without reading message payloads until they inspect a specific message. The milestone and plan overviews remain discoverable without making every contract a general documentation search result.

## Non-goals

This decision does not add a `deferred` lifecycle state, expose full subtask evidence in compact listings, or make blackboard reads unbounded or payload-inclusive.
