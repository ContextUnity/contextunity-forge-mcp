---
title: "Repository task operations"
doc_type: api
---

# Repository task operations

Git milestone frontmatter and fenced YAML task blocks own specifications.
`src/engine/tasks.rs` serves CLI and MCP through the independent SQLite store
in `src/db/tasks_store.rs`. Code-index rebuilds preserve operational task state.

## Configuration and identity

`tasks_db` defaults to `.forge/tasks.sqlite` when omitted from `forge-mcp.yaml`.
Relative paths resolve from the configuration directory; absolute paths open
directly. The default gives each worktree its own store. For coordination across
worktrees, explicitly configure the same shared path in each local installation.
Keep machine-specific paths out of portable tracked configuration.
Connections use WAL, a 5000 ms busy timeout, NORMAL
synchronization, foreign keys, and a 256 MiB mmap ceiling. Transactional changes
increment the `meta` generation; requests retain no cached task connection.

`task_repository` defaults to `forge-mcp`; `task_project` defaults to that
repository. Omitted milestone identity fields use the configured pair.
Matching milestone frontmatter may declare `repository` and
`project`. IDs are `repository/project/milestone_id:task_ref`. Operations and
cleanup remain restricted to the configured project, including forced deletion.
Unknown schemas and populated non-task databases fail before schema mutation.
Revision counters survive operational deletion.

```yaml
tasks_db: .forge/tasks.sqlite
task_repository: forge-mcp
task_project: forge-mcp
```

## Linked repository tasks

The primary configuration opts linked repositories into task coordination:

```yaml
linked_workspaces:
  - name: traverse
    path: ../traverse
    tasks:
      enabled: true
      milestones_dir: docs/milestones
      agents_md: AGENTS.md
```

Omitted `tasks`, `tasks.enabled: false`, or workspace `enabled: false` excludes
the entry. The two paths default to the values shown and remain confined to the
linked repository. Missing or empty milestone directories synchronize zero tasks;
directory synchronization scans numbered Markdown files, including archives.
Unavailable linked roots are skipped. Duplicate names, roots, or task namespaces
are rejected before mutation.

The linked configuration's `name` selects its workspace. Its repository identity
defaults to that name, while its own `forge-mcp.yaml` can override `task_repository`
and `task_project`. All operations through the primary server use the primary
`tasks_db`; a linked repository's independent store is not used by this server.
Synchronize linked specifications explicitly before querying their queue.

`task_list` omits linked tasks by default. Set `repository: "all"` to combine
enabled namespaces, or a configured workspace name to select one. Status still
defaults to `ready`. Existing task IDs route claim, submit, inspect, reset, delete,
and scope extension to their owning namespace. Inspect and claim include absolute
`workspace_root`, absolute `agents_guidance`, and the milestone's local invariants.
Guidance is a file reference; its contents remain repository-owned.

Scope paths are checked in both their owner's root and the task's claimed isolated
worktree. Traversal and symlinks into another repository are rejected. Nested
configured repositories have distinct perimeters: parent scopes cannot include
the child root, and the child can claim work inside its own root. The most specific
configured root determines ownership. A worktree inside another repository's
perimeter cannot claim a foreign task.

## Four flat MCP tools

| Tool | Arguments and behavior |
| --- | --- |
| `task_list` | Optional `repository`, `milestone_ref`, `status`, and `stage`. Status defaults strictly to `ready`; explicit values are `ready`, `in_progress`, `blocked`, `completed`, and `all`. Stage is `build`, `review`, or null. |
| `task_claim` | Required `task_id`, `stage`, `worker_id`, and `worktree`. Claims the current gate atomically. Collisions return typed `TASK_ALREADY_CLAIMED`. |
| `task_submit` | Required `task_id`, `stage`, `evidence_ref`, and `action` (`pass` or `reject`); optional JSON `findings`. Reject requires findings. |
| `task_manage` | Required `action`; optional `workspace`, `task_id`, `milestone_ref`, `task_ref`, `paths`, and `force` (default false). Selectors follow the table below. |

Unknown fields are rejected. Ready tasks are unclaimed and nonterminal, with
satisfied local prerequisites. Missing or amended authority blocks open work.
Null stage includes design, contract, and handoff.

| Manage action | Required fields | Other constraints |
| --- | --- | --- |
| `create` | `milestone_ref`, `task_ref` | Optional workspace; omit task_id/paths; force=false. |
| `sync` | `milestone_ref` or `workspace` | Optional explicit file within workspace; omit task_id/task_ref/paths; force=false. |
| `inspect` | `task_id` | Omit workspace/milestone_ref/task_ref/paths; force=false. |
| `delete` | Exactly one of `task_id`, `milestone_ref` | Workspace is allowed only with milestone_ref; omit task_ref/paths. |
| `extend_scope` | `task_id`, nonempty `paths` | Omit workspace/milestone_ref/task_ref; force=false. |

Create reads the selected task; sync reads all task blocks atomically per file. Repeated
creation preserves state. Changed specifications require a larger Git task
`contract_revision`; sync revokes claims and restarts design/contract. Digests
cover task fields, identity, owners, dependencies, and invariants, excluding
sibling tasks, completion status, and receipts.

Scope extensions preserve frozen base roots: files freeze their parent;
directories freeze themselves. Traversal and symlinks leaving the repository
or the frozen module root fail before any extension is added.
Claim requires an existing directory; an unavailable worktree reports
`WORKTREE_NOT_FOUND` before ownership is recorded.
The first accepted claim for a planned milestone activates its worktree document
and records `started_at` from the first claim timestamp. Planned milestones
retain an empty `started_at` while they wait in the queue.

## Gates and evidence

Gates are `design/v1`, `contract/v1`, `build/v1`, `review/v1`, and `handoff/v1`.
Claim/submit arguments also accept unversioned gate names. Each accepted
submission releases ownership. Review requires a worker different from the
accepted builder and the same candidate commit. Review/handoff rejection records
findings and returns the task to build remediation.

`evidence_ref` is a worktree-relative YAML or JSON file:

```yaml
task_id: forge-mcp/forge-mcp/m-example:implementation
stage: build/v1
claim_revision: 7
contract_revision: 1
worker_id: builder
worktree: /absolute/claimed/worktree
commit: 0123456789abcdef0123456789abcdef01234567
proof:
  command: cargo test --all-targets
  result: passed
  artifacts: []
```

Build proof requires a command, passing result, and artifacts list. Review proof
requires `decision: pass`, nonempty `evidence_ref`, and exactly five `contours`:
`paths`, `claims`, `concurrency`, `project_isolation`, and `administration`.
Each contour records boolean `applicable` and nonempty `evidence`, including the
rationale when inapplicable. Other gates carry non-null proof. Identical accepted
retries return the stored result; changed or revoked evidence fails closed.

After review, write `status: completed` and `receipt` inside the task YAML block.
Receipt fields are `commit`, `contract_revision`, RFC3339 `passed_at`, `evidence`
equal to accepted build proof, `review` equal to accepted review proof, and
`decision: pass`. Handoff synchronously reads the claimed worktree milestone with
`std::fs::read_to_string` at the path formed from the active
`task_claims.worktree` and relative `milestone_ref`. Matching specification and proof are required before
SQLite completion and its retention clock begin.

## Milestone CLI lifecycle

Use the CLI milestone commands to create, inspect, and close repository
contracts:

```sh
contextunity-forge-mcp milestone init --plan docs/plans/proposal.md [--num 011] [--slug short-name] [--title "Title"] [--active]
contextunity-forge-mcp milestone list [--archive] [--status planned|active|completed|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <full-sha>] --verification-command "cargo test --all-targets" --tests-passed <count> --tests-failed 0
```

`milestone init` chooses the highest current or archived milestone number plus ten when
`--num` is absent, accepts a piped description and task blocks, and returns task
sync guidance. `--active` records the creation time as `started_at`; planned
documents gain that timestamp on the first accepted task claim.

`milestone list` reports the frontmatter state and SQLite completion ratio for
each current milestone. Archive and status options include archived contracts.
`milestone show` returns frontmatter, expected outcomes, and task metadata;
`--full` includes the complete task descriptions.

`milestone handoff` validates completion of every task belonging to the selected
milestone in SQLite. It records the full commit SHA and a YAML `handoff` block
with `completed_at`, `duration` as `Xh Ym`, and `verification` containing
`command`, `status: passed`, `tests_passed`, and `tests_failed`. Duration starts
at frontmatter `started_at`, or the earliest SQLite claim timestamp for an
older active document. The command sets frontmatter `status: completed`, moves
the document to `docs/milestones/archive/`, and updates the stored
`milestone_ref` of its tasks. The [CLI reference](cli.md) lists every flag.

## CLI and administration

```sh
contextunity-forge-mcp task list [--repository NAME|all] [--milestone REF] [--status STATUS] [--stage build|review]
contextunity-forge-mcp task create MILESTONE_REF TASK_REF [--workspace NAME]
contextunity-forge-mcp task sync [MILESTONE_REF] [--workspace NAME]
contextunity-forge-mcp task inspect TASK_ID
contextunity-forge-mcp task claim TASK_ID --stage STAGE --worker WORKER --worktree PATH
contextunity-forge-mcp task submit TASK_ID --stage STAGE --action pass|reject --evidence REF [--findings JSON]
contextunity-forge-mcp task extend-scope TASK_ID PATH...
contextunity-forge-mcp task delete TASK_ID [--force]
contextunity-forge-mcp task delete --milestone REF [--workspace NAME] [--force]
contextunity-forge-mcp task reset TASK_ID
contextunity-forge-mcp task cleanup
contextunity-forge-mcp migrate preview [MILESTONE_REF]
contextunity-forge-mcp migrate apply [MILESTONE_REF]
contextunity-forge-mcp migrate verify [MILESTONE_REF]
```

Reset abandons ownership, increments claim revision, preserves passed predecessors,
and marks the current gate pending. Stop the old worker before administrative
reset; process management remains outside task operations.

Standard deletion requires completed state, no active claim, and age strictly
greater than 14 days. Batch eligibility is checked before cascade deletion.
Force bypasses those eligibility checks within the selected project. Completed
dependency outcomes are preserved; removed unsatisfied prerequisites block their
dependents. Results contain deleted IDs, count, revoked claims, and mode. List
requests perform eligible cleanup. Git receipts remain after operational cleanup.

Migration preview reads state without import. Apply idempotently syncs the Git
manifest and validates durable completion proof. Verify reconciles digests and
receipts. Cleanup does not run during import/reconciliation. After retention,
completed task blocks return their durable descriptor without reopening execution.
Without a reference, migration selects numbered Markdown milestones under
`docs/milestones/`, including the archive, in filename order.
