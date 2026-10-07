---
id: m-task-hierarchy-visibility-and-blackboard-coordination
title: Task hierarchy visibility, milestone status lifecycle, and pinpoint blackboard coordination
doc_type: contract
status: active
depends_on:
- m-unified-agent-context-and-task-lifecycle:completed
owners:
- src/core/tasks/
- src/db/tasks_store.rs
- src/engine/tasks/
- src/engine/milestones.rs
- src/engine/scanner.rs
- src/mcp/
- src/cli/
- docs/
- tests/
invariants:
- 'INV-MILESTONE-STATUS-TAXONOMY: Milestones support strictly four lifecycle states: active, planned, completed, cancelled. Manifests without an explicit status default to planned in root and completed in archive; manifests with unparseable frontmatter or unknown status fail closed. Cancelled milestones require a non-empty closure.reason frontmatter field. Cancelling a milestone archives its rationale and prunes its tasks and their outgoing dependencies from SQLite during sync, purges its blackboard messages (DELETE FROM task_blackboard WHERE milestone_ref = ?1), while strictly preserving unsatisfied incoming dependency blocks (task_dependencies WHERE dependency_id = ?1) on remaining tasks. Contract revision bump in task sync preserves existing subtask execution states and evidence.'
- 'INV-TASK-VISIBILITY-ACTIVE-DEFAULT: task list and task_list surface only tasks belonging to active milestones by default. Tasks from planned or completed milestones are accessible via mutually exclusive milestone_status filters (active [default], planned, completed, all) or targeted milestone_ref. Pre-sync tasks of cancelled milestones are hidden from active views and visible under all or targeted milestone_ref until pruned.'
- 'INV-COMPACT-TASK-LISTING: Task listings omit subtask descriptions/titles and evidence blobs by default, emitting lean subtask_ref and status. Full titles and verification evidence are emitted strictly on task inspect / task_inspect, when detail: "full" (MCP) / --full (CLI) is requested, or via dedicated subtask query operations.'
- 'INV-PINPOINT-BLACKBOARD-DISCOVERY: Task blackboard supports a 3-tier hierarchy (milestone, task, subtask) with schema columns milestone_ref TEXT NOT NULL, nullable task_id TEXT REFERENCES tasks ON DELETE CASCADE, and nullable subtask_ref TEXT. Operations support an explicit scope selector (scope: "milestone" | "task" | "subtask"). When scope is "milestone", operations strictly target milestone-level coordination entries (task_id IS NULL) even when an in_progress task is active; if milestone_ref is omitted, resolves to the single active milestone or fails closed if zero or multiple active milestones exist. When scope is "task", operations target task-level entries (matching task_id); if task_id is omitted, resolves to the single in_progress task or fails closed (TASK_BLACKBOARD_NO_ACTIVE_TASK or TASK_BLACKBOARD_AMBIGUOUS_TASK). When scope is "subtask", operations target subtask-level entries (matching task_id and subtask_ref); if keys are omitted, resolves via unique in_progress task and unique in_progress subtask or fails closed (TASK_BLACKBOARD_SUBTASK_REQUIRED). When scope and keys are omitted, auto-resolution follows a strict ladder: (1) if exactly one task is in_progress, resolves to task scope; (2) if more than one task is in_progress, fails closed with TASK_BLACKBOARD_AMBIGUOUS_TASK; (3) only when zero tasks are in_progress, falls back to active milestone scope — resolving if exactly one milestone is active, or failing closed with TASK_BLACKBOARD_AMBIGUOUS_MILESTONE or TASK_BLACKBOARD_NO_ACTIVE_CONTEXT. Listing with scope "milestone" returns strictly milestone-level messages (WHERE milestone_ref = ?1 AND task_id IS NULL). Listings enforce a default page size of 10 messages and maximum page size of 50 (ordered by created_at DESC, id DESC), emit pagination metadata, omit message payload by default, and provide direct inspection access by message_id.'
- 'INV-MANIFEST-RESOLUTION-FIDELITY: Milestone references by ID, prefix, stem, or path propagate resolution errors strictly, give active milestones precedence over archived files during prefix lookup without guessing, and validate YAML frontmatter schemas.'
- 'INV-README-DOCUMENTATION-INDEXING: Overview README.md files within milestone and plan directories (docs/milestones/README.md, docs/plans/README.md) are admitted to the doc_search index, while individual contract manifests remain isolated.'
started_at: 2026-10-07T02:45:00+00:00
---

# Task hierarchy visibility, milestone status lifecycle, and pinpoint blackboard coordination

## Outcome and purpose

In multi-agent coordination and large milestone queues, agents experienced severe token overflows and operational ambiguities when listing tasks and interacting with the blackboard:

1. **Massive Output Overflow in Task Listings**: When calling `task_list`, responses grew past 80 KiB because every task emitted all subtasks with full titles and lengthy verification evidence strings, and included tasks from all historical, archived, or planned milestones.
2. **Stale Cancelled Milestone Tasks**: Cancelled or rejected milestones (such as `051-optical-token-compression`) remained lingering in the SQLite tasks database because `cancelled` was not part of the milestone status taxonomy and `task sync` lacked automatic pruning for cancelled milestones.
3. **Fragile Milestone Reference Resolution**: When resolving milestone references by prefix, stem, or ID, errors were swallowed in `task_list` leading to silent empty results, and prefix lookups hit false ambiguous collisions between active and archived manifests.
4. **Context Flooding from Blackboard Messages**: Reading the blackboard without tight parameters returned massive multi-kilobyte log payloads across irrelevant tasks, quickly overflowing agent context windows.
5. **Inaccessible Milestone Overviews in Doc Search**: Scanner isolation blocked all markdown files under `docs/milestones/`, preventing agents from searching high-level overview guides like `docs/milestones/README.md`.

This milestone resolves these issues across six focused, domain-bounded tasks:
- Dynamic milestone reference resolution with active manifest priority, strict path component matching, and error propagation.
- Milestone status lifecycle taxonomy (`active`, `planned`, `completed`, `cancelled`), mandatory `closure.reason` for cancelled manifests, automatic cancelled task and blackboard pruning while strictly preserving dependency blocks on remaining tasks, and subtask progress retention upon contract revision sync.
- Active-by-default task visibility, mutually exclusive CLI flags (`--planned`, `--completed`, `--all` shorthand for `--milestone-status`) and typed MCP enum (`milestone_status`: `active`, `planned`, `completed`, `all`), compact subtask listings (`subtask_ref` + `status` only, with `title` and `evidence` preserved under `detail: full` or inspection), verified against core and MCP test suites.
- 3-tier pinpoint blackboard coordination with schema migration (`milestone_ref`, nullable `task_id`, `subtask_ref`), explicit scope selector (`scope: "milestone" | "task" | "subtask"`) with fail-closed key resolution, deterministic ladder auto-resolution when scope is omitted (single in_progress task -> task scope; zero in_progress tasks -> active milestone scope; fails closed on ambiguous or missing contexts), milestone-level isolation (scope "milestone" returns strictly `task_id IS NULL` rows), subtask isolation, bounded pagination (default page size 10 messages, max 50, ordered by `created_at DESC, id DESC` with pagination metadata), payload omission by default, and single-message inspection by `message_id`, verified against core and MCP test suites.
- Overview README.md scanner admission for `docs/milestones/README.md` and `docs/plans/README.md`, verified with doc_search tests.
- ADR 0014 documentation and canonical documentation alignment across milestone READMEs and reference docs, executed after all implementation tasks.

---

## Tasks in this milestone

### task: milestone-reference-resolution-and-error-propagation

```yaml
task_ref: milestone-reference-resolution-and-error-propagation
target: Resolve milestone references dynamically by ID, prefix, stem, and path with active manifest precedence over archive using path components, and strictly propagate resolution errors in task list and management operations
agent_type: worker
proof_policy: direct-proof
contract_revision: 3
scope:
- src/engine/tasks/workspaces.rs
- src/engine/tasks.rs
- src/engine/milestones.rs
- tests/core_basics/tasks.rs
- tests/mcp_context/tasks.rs
```

### task: milestone-status-lifecycle-and-cancelled-pruning

```yaml
task_ref: milestone-status-lifecycle-and-cancelled-pruning
target: Formalize milestone status taxonomy (active, planned, completed, cancelled), require closure.reason for cancelled manifests, automatically prune tasks and outgoing dependencies of cancelled milestones from SQLite and purge milestone blackboard rows during sync while preserving unsatisfied incoming dependency blocks (task_dependencies WHERE dependency_id = ?1), and preserve subtask progress and evidence during contract revision sync
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/core/tasks/mod.rs
- src/engine/milestones.rs
- src/engine/tasks.rs
- src/db/tasks_store.rs
- docs/milestones/archive/051-optical-token-compression.md
- tests/core_basics/tasks.rs
```

### task: task-visibility-and-compact-subtask-listing

```yaml
task_ref: task-visibility-and-compact-subtask-listing
target: 'Filter task listings by milestone status with active milestones by default, implement primary CLI argument --milestone-status <active|planned|completed|all> with mutually exclusive shorthand flags (--planned, --completed, --all) and typed MCP milestone_status enum (active, planned, completed, all), strip subtask evidence, and omit subtask titles/descriptions by default in task listings while retaining them on detail: full and inspection'
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/tasks.rs
- src/cli/task.rs
- src/mcp/tools.rs
- tests/core_basics/tasks.rs
- tests/mcp_context/tasks.rs
```

### task: pinpoint-blackboard-and-payload-omission

```yaml
task_ref: pinpoint-blackboard-and-payload-omission
target: 'Support 3-tier blackboard hierarchy (milestone, task, subtask) with milestone_ref column, nullable task_id, and subtask_ref column, provide explicit scope selector (scope: "milestone" | "task" | "subtask") with deterministic key resolution rules (omitted keys resolve to unique active/in_progress context or fail closed), ensure scope "milestone" listings isolate milestone-level messages (task_id IS NULL), implement deterministic ladder auto-resolution when scope and keys are omitted (single in_progress task -> task scope; zero in_progress tasks -> active milestone scope; fail closed on ambiguous or missing contexts), implement subtask filtering, cap listings with default page size 10 and max 50 (created_at DESC, id DESC) with pagination metadata, omit payload from list output by default, and provide direct inspection access by message_id across MCP and CLI'
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/db/tasks_store.rs
- src/engine/tasks.rs
- src/cli/task.rs
- src/mcp/tools.rs
- tests/core_basics/tasks.rs
- tests/mcp_context/tasks.rs
```

### task: milestone-and-plan-readme-indexing

```yaml
task_ref: milestone-and-plan-readme-indexing
target: Permit overview README.md files under docs/milestones/ and docs/plans/ to be scanned and indexed in doc_search while preserving strict isolation of individual milestone and plan contract markdown files
agent_type: worker
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/scanner.rs
- forge-mcp.yaml
- tests/core_basics/tasks.rs
```

### task: adr-0014-and-documentation-alignment

```yaml
task_ref: adr-0014-and-documentation-alignment
target: Author ADR 0014, clean up deferred relics in docs/milestones/README.md, document archive rationale in docs/milestones/archive/README.md, and align documentation in docs/reference/tasks.md, docs/reference/cli.md, and docs/reference/mcp-tools.md
agent_type: worker
proof_policy: direct-proof
contract_revision: 1
depends_on:
- milestone-reference-resolution-and-error-propagation
- milestone-status-lifecycle-and-cancelled-pruning
- task-visibility-and-compact-subtask-listing
- pinpoint-blackboard-and-payload-omission
- milestone-and-plan-readme-indexing
scope:
- docs/adr/0014-milestone-task-subtask-hierarchy-and-visibility.md
- docs/milestones/README.md
- docs/milestones/archive/README.md
- docs/reference/tasks.md
- docs/reference/cli.md
- docs/reference/mcp-tools.md
```
