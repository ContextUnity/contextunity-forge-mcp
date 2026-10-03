---
title: "Repository task operations"
doc_type: api
---

# Repository task operations

Git milestone frontmatter and fenced YAML task blocks own specifications.
`src/engine/tasks.rs` serves CLI and MCP through the independent SQLite store
in `src/db/tasks_store.rs`. Code-index rebuilds preserve operational task state.
For the lifecycle rationale, see [ACDD](acdd.md); for the delivery sequence,
follow the [execution runbook](../runbooks/acdd.md).

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
agents_guidance: AGENTS.md
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
      agents_guidance: AGENTS.md
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
defaults to `ready`. Each list entry is a compact card with `task_id`, `target`,
`status`, `stage`, `owner`, `agent_type`, and `rev` (the contract revision).
The single-workspace response puts `workspace_root`, `agents_guidance`, and
`workspace` beside `tasks`. With `repository: "all"`, `workspaces` maps each
`repository/project` namespace to those paths and its workspace name.
Use `task_manage` with `action: "inspect"` to read the full task specification
and attempts. Existing task IDs route claim, submit, inspect, reset, delete,
and scope extension to their owning namespace. Inspect and claim include absolute
`workspace_root`, absolute `agents_guidance`, the milestone's local invariants,
and stage-specific `workflow_guidance`. The latter gives `active_stage`,
`agent_type`, `subagent_role`, `steps`, and the instruction path. At review and
delivery it also gives the independence rule and accepted builder identity.
If the instruction file is missing, the response includes `TASK_GUIDANCE_MISSING`,
inline steps, and the [canonical ACDD reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md).

Scope paths are checked in both their owner's root and the task's claimed isolated
worktree. Traversal and symlinks into another repository are rejected. Nested
configured repositories have distinct perimeters: parent scopes cannot include
the child root, and the child can claim work inside its own root. The most specific
configured root determines ownership. A worktree inside another repository's
perimeter cannot claim a foreign task.

## Five flat MCP tools

| Tool | Arguments and behavior |
| --- | --- |
| `task_list` | Optional `repository`, `milestone_ref`, `status`, and `stage`. Status defaults strictly to `ready`; explicit values are `ready`, `in_progress`, `blocked`, `completed`, and `all`. Stage is `build`, `review`, `deliver`, or null. |
| `task_claim` | Required `task_id`, `stage`, `worker_id`, and `worktree`. Claims the current gate atomically. Collisions return typed `TASK_ALREADY_CLAIMED`. |
| `task_submit` | Required `task_id`, `stage`, JSON object `evidence`, and `action` (`pass` or `reject`); optional JSON `findings`. Reject requires findings. |
| `task_manage` | Required `action`; optional `workspace`, `task_id`, `milestone_ref`, `task_ref`, `paths`, `force` (default false), `subtask_ref`, `title`, `subtask_status`, and `evidence`. Selectors follow the table below. |
| `task_blackboard` | Required `action` (`post` or `read`) and `task_id`. Post requires `topic` and `payload`, accepts optional `author`, and returns an ID. Read accepts optional `topic` and `limit`, and returns chronological messages. |

Unknown fields are rejected. Ready tasks are unclaimed and nonterminal, with
satisfied local prerequisites. Missing or amended authority blocks open work.
Null stage includes design and contract.

| Manage action | Required fields | Other constraints |
| --- | --- | --- |
| `create` | `milestone_ref`, `task_ref` | Optional workspace; omit task_id/paths; force=false. |
| `sync` | `milestone_ref` or `workspace` | Optional explicit file within workspace; omit task_id/task_ref/paths; force=false. |
| `inspect` | `task_id` | Omit workspace/milestone_ref/task_ref/paths; force=false. |
| `delete` | Exactly one of `task_id`, `milestone_ref` | Workspace is allowed only with milestone_ref; omit task_ref/paths. |
| `extend_scope` | `task_id`, nonempty `paths` | Omit workspace/milestone_ref/task_ref; force=false. |
| `subtask_add` | `task_id`, `subtask_ref`, `title` | Optional workspace; omit milestone_ref/paths; force=false. |
| `subtask_update` | `task_id`, `subtask_ref`, `subtask_status` | Optional `evidence`, `workspace`; status must be `pending`, `in_progress`, or `completed`. |
| `subtask_list` | `task_id` | Optional workspace; omit milestone_ref/task_ref/paths; force=false. |

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

## Subtasks and iterative deepening

Complex domain tasks often contain finer-grained milestones, discovered edge cases,
or sub-component checklists (e.g. testing specific UI fixtures, verifying individual DOM
queries, or step-by-step refactoring). Instead of proliferating root-level milestone tasks
that inflate the queue and require heavyweight five-gate lifecycles (`design/v1` through `deliver/v1`),
agents should deepen the active task using **subtasks**.

- **Schema**: Each subtask contains `subtask_ref` (alphanumeric identity slug), `title` (goal description),
  `status` (`pending`, `in_progress`, or `completed`), and optional `evidence` (verification command or test notes).
- **Canonical State**: During active execution (`design/v1` through `review/v1`), SQLite (`task_subtasks` table
  and `Task.spec.subtasks` descriptor) is the canonical operational state. Upon task completion at `deliver/v1`,
  Forge serializes all subtasks directly into the milestone Markdown fenced YAML block alongside the delivery receipt.
  The milestone document is the durable canonical record that survives SQLite clearing, rebuilds, and milestone handoff.
- **Post-Delivery Immutability**: Subtasks are strictly operational execution artifacts. Once a task reaches `deliver/v1`
  (`status: completed`), its subtasks and receipt are frozen. Calling `subtask_add` or `subtask_update` on a completed task
  fails closed with `TASK_TERMINAL`.
- **Digest Independence**: Contract digests exclude `spec.subtasks`. Adding, updating, or completing subtasks
  never triggers `AUTHORITY_GAP` or forces contract re-admission.
- **Sync Preservation**: Running `task sync` updates subtask titles if modified in Markdown, but never overwrites
  in-progress or completed `status` or `evidence` recorded in SQLite. Subtasks created via CLI or MCP are preserved
  in SQLite and the task descriptor across sync operations.

```yaml
task_ref: language-profile
target: Implement profile
proof_policy: seam-test-first
scope: [src/]
subtasks:
  - subtask_ref: dom-methods
    title: Extract query methods
    status: completed
    evidence: "tests/html_profile.rs passed"
  - subtask_ref: alpine-attrs
    title: Support Alpine x-directives
    status: in_progress
```

## Gates and evidence

Gates are `design/v1`, `contract/v1`, `build/v1`, `review/v1`, and `deliver/v1`.
The final task gate performs task delivery; `milestone handoff` is the separate
command that closes and archives the whole milestone.
Claim/submit arguments also accept unversioned gate names. Each accepted
submission releases ownership. Review requires a worker different from the
accepted builder and the same candidate commit. Review/delivery rejection records
findings and returns the task to build remediation.

`evidence` is a direct JSON object bound to the active claim. For example, a
passing build submission contains:

```json
{
  "task_id": "forge-mcp/forge-mcp/m-example:implementation",
  "stage": "build/v1",
  "claim_revision": 7,
  "contract_revision": 1,
  "worker_id": "builder",
  "worktree": "/absolute/claimed/worktree",
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "proof": {
    "test_proof": {
      "command": "cargo test --test core_basics",
      "exit_code": 0,
      "tests_passed": 1,
      "tests_failed": 0
    }
  }
}
```

Contract proof has the form `{"contract_proof":{"seam_test_ref":"tests/...::test_name","red_exit_code":101}}` (or `red_exit_code: 0` when `proof_policy: direct-proof`).
Build proof is wrapped in `test_proof`; on pass it requires a nonempty command,
exit code 0, at least one passing test, and zero failures. An optional `log` is
limited to 64 KiB. Review proof is wrapped in `review_proof` and requires a
`decision` matching the action plus exactly five `contours`: `paths`, `claims`,
`concurrency`, `project_isolation`, and `administration`. Each contour has a
boolean `applicable` and nonempty `evidence`, including a rationale when
inapplicable. Other gates carry non-null proof. Identical accepted retries return
the stored result; changed or revoked evidence fails closed. The validated JSON
is retained in `task_gates.evidence` without reading an evidence file.

At passing task delivery (`deliver/v1`), Forge validates the current task
specification and accepted build and review proof, then writes
`status: completed` and a receipt into the
milestone task block. The receipt carries the accepted commit, contract revision,
RFC3339 `passed_at`, build proof, review proof, and decision. Its rollup retains
verified invariants, review summary, and task blackboard `architectural_notes`.
The task becomes completed in SQLite and its blackboard messages are cleared.
The milestone document is the durable context after task delivery. The
receipt records the reviewed candidate SHA; amend the task commit with it so
the final history contains one commit for the task. The amended commit has a
different SHA because a commit cannot contain its own hash.

## Task blackboard

`task_blackboard` stores messages with `id`, `task_id`, `author`, `topic`, `payload`,
and `created_at` in the task SQLite store. Post requires a topic and payload.
Read returns messages in chronological order, optionally filtered by topic or
bounded by `limit`; no limit reads all matching messages. An omitted author is
the current claim worker, or `mcp`/`cli` when no claim is active. Post rejects a
read-only `limit`; read rejects `author` and `payload`. The store confines access
to the task's configured project. Use `architectural_notes` for decisions that
must survive task delivery; other topics are temporary collaboration context.

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
contextunity-forge-mcp task list [--repository NAME|all] [--milestone REF] [--status STATUS] [--stage build|review|deliver]
contextunity-forge-mcp task create MILESTONE_REF TASK_REF [--workspace NAME]
contextunity-forge-mcp task sync [MILESTONE_REF] [--workspace NAME]
contextunity-forge-mcp task inspect TASK_ID
contextunity-forge-mcp task claim TASK_ID --stage STAGE --worker WORKER --worktree PATH
contextunity-forge-mcp task submit TASK_ID --stage STAGE --action pass|reject --evidence '<JSON_OBJECT>' [--findings JSON]
contextunity-forge-mcp task blackboard post TASK_ID --topic architectural_notes --payload "Decision and reason" [--author WORKER]
contextunity-forge-mcp task blackboard read TASK_ID [--topic TOPIC] [--limit N]
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
