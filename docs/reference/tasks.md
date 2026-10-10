---
title: "Repository task operations"
doc_type: api
---

# Repository task operations

Git milestone frontmatter and fenced YAML task blocks own specifications.
`src/engine/tasks.rs` serves CLI and MCP through the independent SQLite store
in `src/db/tasks_store.rs`. Code-index rebuilds preserve operational task state.
For the lifecycle, load the `contextunity-forge` skill; for the delivery sequence,
follow the [execution runbook](../runbooks/acdd.md).

## Configuration and identity

`tasks_db` defaults to `.forge/tasks.sqlite` when omitted from `forge-mcp.yaml`.
When running within a git worktree, relative paths (including the default) resolve
against the primary worktree root discovered via `git rev-parse --git-common-dir`,
ensuring all linked worktrees share the same central tasks database without requiring
machine-specific absolute paths in tracked configuration. In non-git or standalone
workspaces, relative paths resolve directly from the configuration directory.
Absolute paths open directly.
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
When a role is configured, `role_spec` gives its effective `mode` and
`reuse_on_reject`, ordered `models` (where the first model is primary), and
recommendation. After a `reject_to` rewind, `rejection` gives
`rejected_from`, the prior builder `worker_id` when reuse is enabled, and the
persisted findings; it is null on clean forward runs. Reuse guidance adds
`Return task to builder '{worker_id}' with review rejection findings to repair defects.`
If the instruction file is missing, does not name the `contextunity-forge` skill, or that skill is not installed globally or in the repository, the response includes `TASK_GUIDANCE_MISSING`,
inline steps.

Scope paths are checked in both their owner's root and the task's claimed isolated
worktree. Traversal and symlinks into another repository are rejected. Nested
configured repositories have distinct perimeters: parent scopes cannot include
the child root, and the child can claim work inside its own root. The most specific
configured root determines ownership. A worktree inside another repository's
perimeter cannot claim a foreign task.

### Scope Management and Co-Evolution Planning

Every task contract declares an explicit file perimeter (`scope`) bounding allowable writes. Forge guards these write boundaries in SQLite (`task_scope_paths` table):

- **Exclusive Scope Locks and Sibling Conflicts**: Declared task scope paths are registered as frozen write locks (`frozen = 1`) upon milestone initialization and synchronization. At `task_claim`, Forge compares a task's paths with sibling locks in the same milestone. Exact, ancestor, or descendant overlaps with a non-completed sibling fail closed with `TASK_SCOPE_CONFLICT`. Dynamic extensions use the same milestone lock registry and reject paths owned by active siblings.
- **Dynamic Extension (`extend_scope`)**: During an open task, a claimed task can dynamically extend its write perimeter to unowned files or tests (`frozen = 0`) via `task extend-scope <task_id> <path>...` (or MCP `task_manage(action: "extend_scope", ...)`).
- **Completed Task Scope Release**: Once a task reaches `completed`, both `task_claim` and `extend_scope` exclude its rows from scope-conflict checks (`json_extract(t.descriptor, '$.status') != 'completed'`). Forge retains the completed task's `task_scope_paths` history while allowing successor tasks to claim or extend into those paths.
- **Protected System Scopes and Dirty File Prohibition**: System configuration and workflow profile files (`.forge/acdd/**` and profile definitions) are protected system paths (`TASK_SCOPE_PROTECTED`). Regular tasks cannot claim or extend into these paths via `extend_scope`. Furthermore, during `task_claim`, gate execution, and `task_submit`, the compiled binary inspects the worktree git status: if any file under `.forge/acdd/**` is dirty (modified, unstaged, staged, or untracked) and is not explicitly admitted in `task.spec.scope`, Forge fails closed with `TASK_SCOPE_VIOLATION`. This guarantees that agents cannot mutate workflow profiles, weaken review contours, or bypass test gates on the fly. Legitimate profile updates are performed either directly outside task cycles or within dedicated configuration tasks whose explicit scope admits `.forge/acdd/profile.yaml`.
- **Documentation Co-Evolution Planning**: Shared documentation (such as `docs/reference/tasks.md`, `README.md`, or architecture documents) frequently evolves across multiple milestone tasks. When multiple tasks in a milestone must update the same documentation or common shared modules, dependencies must be sequenced via `depends_on`. Once the predecessor task completes, the successor task gains lawful access to extend into and evolve the shared documentation. Alternatively, documentation changes can be planned alongside code changes within the owning task's build gate, ensuring documentation and implementation remain synchronized without scope friction.

## Five flat MCP tools

| Tool | Arguments and behavior |
| --- | --- |
| `task_list` | Optional `repository`, `milestone_ref`, `milestone_status`, `status`, `stage`, and `detail`. Task status defaults to `ready`; values are `ready`, `in_progress`, `blocked`, `completed`, and `all`. Milestone status defaults to `active`; values are `active`, `planned`, `completed`, and `all`. A targeted `milestone_ref` defaults milestone status to `all`. `stage` matches a gate ID declared by the active profile; null matches every gate. Subtask detail defaults to compact references and statuses; `full` includes titles and verification evidence. |
| `task_claim` | Required `task_id`, `stage`, `worker_id`, and `worktree`; optional `bundle` (boolean, defaults to true). Claims the current gate atomically, returning context tailored to the active gate (see below). Set `bundle: false` for minimal details. Collisions return typed `TASK_ALREADY_CLAIMED`. |
| `task_submit` | Required `task_id`, `stage`, JSON object `evidence`, and `action` (`pass` or `reject`); optional JSON `findings`. Reject requires findings. |
| `task_manage` | Required `action`; optional `workspace`, `task_id`, `milestone_ref`, `task_ref`, `paths`, `force` (default false), `subtask_ref`, `title`, `subtask_status`, and `evidence`. Selectors follow the table below. |
| `task_blackboard` | `action` is `post`, `read`, or `inspect`. Optional `scope` selects `milestone`, `task`, or `subtask`; `milestone_ref`, `task_id`, and `subtask_ref` identify or constrain that context. Post requires `topic` and `payload`, accepts `author`, and returns an ID. Read accepts `topic`, `limit` (default 10, maximum 50), and `offset`; it returns newest-first summaries and pagination metadata without payload. Inspect requires `message_id` and returns that message including payload. |

Unknown fields are rejected. Ready tasks are unclaimed and nonterminal, with
satisfied local prerequisites. Missing or amended authority blocks open work.
Null stage matches all gates.

| Manage action | Required fields | Other constraints |
| --- | --- | --- |
| `create` | `milestone_ref`, `task_ref` | Optional workspace; omit task_id/paths; force=false. |
| `sync` | `milestone_ref` or `workspace` | Optional explicit file within workspace; omit task_id/task_ref/paths; force=false. |
| `inspect` | `task_id` | Omit workspace/milestone_ref/task_ref/paths; force=false. |
| `context` | `task_id` | Omit workspace/milestone_ref/task_ref/paths; returns zero-shot context bundle without claiming. |
| `delete` | Exactly one of `task_id`, `milestone_ref` | Workspace is allowed only with milestone_ref; omit task_ref/paths. |
| `extend_scope` | `task_id`, nonempty `paths` | Omit workspace/milestone_ref/task_ref; force=false. |
| `subtask_add` | `task_id`, `subtask_ref`, `title` | Optional workspace; omit milestone_ref/paths; force=false. |
| `subtask_update` | `task_id`, `subtask_ref`, `subtask_status` | Optional `evidence`, `workspace`; status must be `pending`, `in_progress`, or `completed`. |
| `subtask_list` | `task_id` | Optional workspace; omit milestone_ref/task_ref/paths; force=false. |
| `reset` | `task_id` | Returns an open task to ready at its current gate; a completed task restarts at the first gate in its active profile. |
| `reopen` | `task_id` | Alias for reset. |

Completed task reset reads the owning milestone document and atomically replaces
its Markdown receipt before resetting SQLite to the first gate in the active profile. Missing or
unwritable milestone documents fail before SQLite changes. A returned SQLite
error restores the original Markdown. The two stores are updated in sequence;
an interrupted process between updates requires manual reconciliation.
Reset and task delivery serialize receipt edits through one advisory file lock
beside the task database, so concurrent tasks cannot overwrite each other's
milestone receipts.

Create reads the selected task; sync reads all task blocks atomically per file. Repeated
creation preserves state. Changed specifications require a larger Git task
`contract_revision`; sync revokes claims and restarts contract. Digests
cover task fields, identity, owners, dependencies, and invariants, excluding
sibling tasks, completion status, and receipts.

Scope extensions preserve admitted base roots: tasks may declare optional
`scope_roots` to define the pre-admitted boundary roots within which `extend_scope`
may add paths. When `scope_roots` is omitted, directories freeze themselves;
files in subdirectories freeze their parent directory; repository-root files
freeze only their own path. Declare `scope_roots` for a wider admitted boundary.
Documentation co-evolution scope planning: tasks altering observable contracts,
interfaces, workflows, configuration, or operational assumptions must include their
governing documentation files (e.g. `docs/reference/tasks.md`, `TESTS.md`, `tests/AGENTS.md`)
in `scope` at task planning or add them via `extend_scope` while the task is open. File scope
isolation is enforced within the same repository/project and milestone via
`TASK_SCOPE_CONFLICT`. During claim, a sibling with unsatisfied dependencies is not yet an
eligible lock owner; `extend_scope` checks declared paths for every non-completed sibling.
An active task holds exclusive ownership over its admitted scope to protect candidate
snapshots from concurrent clobbering and Git merge conflicts. When a task completes
(`status: completed`), its scope lock is released while its historical path rows remain,
allowing subsequent sibling tasks (such as those ordered via `depends_on`) to claim or
extend into and update those files. Linked projects use separate task namespaces, even when
they share a relative milestone path. Prefer domain-granular documentation files over
shared monoliths when structuring parallel tasks to avoid active scope contention.
Traversal, paths outside admitted roots, and symlinks leaving the repository or the
admitted root fail before any extension is added. Scope extension within admitted
roots preserves `contract_revision`, while altering `scope_roots` requires contract revision.
Claim requires an existing directory; an unavailable worktree reports
`WORKTREE_NOT_FOUND` before ownership is recorded.
The first accepted claim for a planned milestone activates its worktree document
and records `started_at` from the first claim timestamp. Planned milestones
retain an empty `started_at` while they wait in the queue.

## Subtasks and iterative deepening

Complex domain tasks often contain finer-grained milestones, discovered edge cases,
or sub-component checklists (e.g. testing specific UI fixtures, verifying individual DOM
queries, or step-by-step refactoring). Instead of proliferating root-level milestone tasks
that inflate the queue and require a full configured gate lifecycle,
agents should deepen the active task using **subtasks**.

- **Schema**: Each subtask contains `subtask_ref` (alphanumeric identity slug), `title` (goal description),
  `status` (`pending`, `in_progress`, or `completed`), and optional `evidence` (verification command or test notes).
- **Canonical State**: During active execution, SQLite (`task_subtasks` table
  and `Task.spec.subtasks` descriptor) is the canonical operational state. Upon task completion at the profile's terminal delivery gate,
  Forge serializes all subtasks directly into the milestone Markdown fenced YAML block alongside the delivery receipt.
  The milestone document is the durable canonical record that survives SQLite clearing, rebuilds, and milestone handoff.
- **Post-Delivery Immutability**: Subtasks are strictly operational execution artifacts. Once a task passes its terminal delivery gate
  (`status: completed`), its subtasks and receipt are frozen. Calling `subtask_add` or `subtask_update` on a completed task
  fails closed with `TASK_TERMINAL`.
- **Digest Independence**: Contract digests exclude `spec.subtasks`. Adding, updating, or completing subtasks
  never triggers `AUTHORITY_GAP` or forces contract re-admission.
- **Sync behavior**: When the task contract digest is unchanged, `task sync` updates subtask titles from Markdown
  while preserving `status` and `evidence` recorded in SQLite; subtasks created via CLI or MCP remain preserved.
  On successful re-admission with a higher `contract_revision`, sync preserves status and evidence for retained `subtask_ref` values, updates their titles, and removes references omitted from the new specification. Reassess retained evidence against the amended contract.

### Subtask guidance field

`context_bundle.guidance.subtask_dod` returns the advisory criteria when `bundle` is true, the default. `bundle: false` omits `context_bundle`. `subtask_update` records the caller-provided status and evidence and does not score the criteria. The `contextunity-forge` skill lists the criteria.

```yaml
task_ref: language-profile
target: Implement profile
proof_policy: seam-test-first
scope: [src/]
subtasks:
  - subtask_ref: dom-methods
    title: Extract query methods
    status: completed
    evidence: "tests/languages/html/syntax.rs passed"
  - subtask_ref: alpine-attrs
    title: Support Alpine x-directives
    status: in_progress
```

## Gates and evidence

Gates are dynamically governed by the active profile (`acdd_profile`), defaulting to `contract`, `build`, `review`, and `deliver`.
The final task gate performs task delivery; `milestone handoff` is the separate
command that closes and archives the whole milestone.
Each accepted submission releases ownership. Review requires a worker different from the
accepted builder and verifying the same candidate commit. Review/delivery rejection records
findings and returns the task to remediation at `gate.reject_to` (defaulting to `build` in standard profile).

### Context bundle matrix by proof taxonomy

When claiming a gate with `task_claim`, Forge returns tailored context bundles based on the gate's closed `proof` taxonomy:

| Proof Kind | Claim Context Bundle Content | Primary Role Guidance |
|---|---|---|
| `contract` | Admitted target, invariants, declared scope paths, proof policy, seam test template/reference. | Contract Author: write red public-seam test, submit non-zero red exit code. |
| `command` | Target, invariants, admitted scope paths, verified contract proof (seam test ref & exit code), command to execute. | Implementer: implement within scope, make seam test pass, submit passing command proof. |
| `review` | Target, invariants, scope paths, candidate snapshot SHA (from predecessor `sha_snapshot: true` gate), git diff against baseline HEAD, contour checklist with operational criteria from `gate.contours`. | Independent Reviewer: verify candidate diff against contour checklist, submit review proof with decision. |
| `delivery` | Target, invariants, verified candidate snapshot SHA, all passing review proofs from `review_sources`, completed subtasks summary, receipt template. | Deliverer: verify rollup proofs and invariant checks, finalize receipt. |
| `none` | Target, invariants, admitted scope paths, informational guidance. | Worker: perform non-verified informational step. |
| `scheme` | Target, invariants, admitted scope paths, recursive JSON schema definition for expected proof payload. | Schema Worker: submit structured payload matching configured recursive schema. |

`evidence` is a direct JSON object bound to the active claim. The `commit` field can be omitted; Forge automatically captures the scoped candidate snapshot SHA on predecessor gates with `sha_snapshot: true` (e.g. `contract` and `build`), and inherits it on downstream gates (`review` and `deliver`). For example, a passing build submission contains:

```json
{
  "task_id": "forge-mcp/forge-mcp/m-example:implementation",
  "stage": "build",
  "claim_revision": 7,
  "contract_revision": 1,
  "worker_id": "builder",
  "worktree": "/absolute/claimed/worktree",
  "proof": {
    "command_proof": {
      "command": "cargo test --test acdd tasks::completed_task_reopen",
      "exit_code": 0,
      "tests_passed": 1,
      "tests_failed": 0
    }
  }
}
```

Contract proof has the form `{"contract_proof":{"seam_test_ref":"tests/...::test_name","red_exit_code":101}}`. `red_exit_code` may be 0 when `proof_policy` is `direct-proof` or `deferred-final-test`.
Build proof is wrapped in `command_proof`; on pass it requires a nonempty command,
exit code 0, at least one passing test, and zero failures. An optional `log` is
limited to 64 KiB. Review proof is wrapped in `review_proof` and requires a
`decision` matching the action plus the review contours configured for the gate
(`gate.contours`, defaulting to `standard` with the five canonical review contours: `paths`, `claims`,
`concurrency`, `project_isolation`, and `administration`). Each contour has a
boolean `applicable` and nonempty `evidence`, including a rationale when
inapplicable. Other gates carry non-null proof. Identical accepted retries return
the stored result; changed or revoked evidence fails closed. The validated JSON
is retained in `task_gates.evidence` without reading an evidence file.

At passing task delivery (`deliver`), Forge validates the current task
specification and accepted build and review proof, then writes
`status: completed` and a receipt into the
milestone task block. When `auto_commit: true` (default in profile), Forge automatically
creates an atomic Git commit directly from the candidate snapshot tree of the nearest predecessor gate with `sha_snapshot: true` (which is `build` in the default profile, whose candidate tree SHA is verified across all `review_sources`)
plus the milestone receipt file, with parent `HEAD` (verifying `git rev-parse HEAD` matches `candidate_baseline_head`, the exact commit SHA of HEAD captured when the candidate snapshot was created).
Standard commit message is derived from task target and completed subtasks, and hooks are enforced.
If the commit fails (e.g. pre-commit hook failure), the SQLite transaction rolls back atomically,
leaving the task uncompleted at `deliver` with an explicit error (`TASK_DELIVERY_COMMIT_FAILED`), allowing the deliverer to reject
back to `reject_to` (e.g. `build`) or retry. The task receipt created at delivery records the created commit SHA from `auto_commit` (or the developer's delivery commit when `auto_commit: false`), contract revision,
RFC3339 `passed_at`, command proof, review proof, and decision. Its rollup retains
verified invariants, review summary, and task blackboard `decisions` and `deferred`.
The task becomes completed in SQLite and its blackboard messages are cleared.
The milestone document is the durable context after task delivery. When `auto_commit` is active,
Forge has already committed the deliverable; when `auto_commit: false`, commit the task's scoped source,
tests, and generated receipt in its claimed task worktree under standing permissions.
Merge the delivered task branch into the milestone branch and delete the
temporary task branch. When `milestone handoff` is executed, Forge verifies all milestone tasks are completed, resolves each task's landed commit in the milestone branch (the commit that landed the task's work into the milestone branch, whether via fast-forward or merge commit), rewrites `receipt.commit` in each task block and SQLite to this landed SHA, records `handoff.commit` (HEAD at handoff) in frontmatter, and archives the document. The subsequent milestone archive commit occurs after handoff and is excluded from `receipt.commit`, avoiding any hash cycle. Prior to final milestone handoff, completed tasks can
be reopened if necessary using `contextunity-forge-mcp task reopen <task-id>`.

Zero runtime backward compatibility applies strictly to SQLite runtime storage and active milestone manifests. Historical receipts archived in `docs/milestones/archive/` are read as-is and exempt from active profile receipt schema validation. Filtering by `--stage <id>` matches directly against the persistent `task.stage` string ID in SQLite; CLI argument validation checks that the stage ID is defined in the active profile of the workspace or open tasks. If a task references a stage missing from the active profile's `gates`, Forge fails closed with `TASK_STAGE_UNKNOWN`, naming mismatched gates and suggesting either migrating task records or pinning the intended profile via `acdd_profile: <path>:<sha256_prefix>`.

## Task blackboard

`task_blackboard` stores messages in the configured task SQLite store with a
`milestone_ref`, optional `task_id`, optional `subtask_ref`, `author`, `topic`,
`payload`, and `created_at`. Post requires a topic and payload. An omitted author
uses the task owner when the resolved context has one, or the current transport
(`mcp` or `cli`). Post rejects read-only pagination fields; read rejects
`author` and `payload`. Post and read use the resolved task project. Inspect
searches configured task workspaces by `message_id` and returns the first match.

Use `scope: "milestone"` for milestone-level messages. It resolves the explicit
milestone or the unique active milestone and never includes task-scoped entries.
Use `scope: "task"` or `scope: "subtask"` to select those levels. Omitted task
and subtask keys resolve only through unique in-progress contexts; missing or
ambiguous contexts fail closed. When scope and keys are all omitted, Forge uses
the unique in-progress task, fails if several are in progress, and falls back to
the unique active milestone only when no task is in progress.

Read returns one page of payload-free summaries, ordered by `created_at DESC`
and `id DESC`. The default `limit` is 10, the maximum is 50, and `offset` selects
the next page using the returned pagination metadata. Use `action: "inspect"`
with a `message_id` from a post or read result to retrieve the full message and
payload. Use `architectural_notes` for decisions that must survive task delivery;
other topics are temporary collaboration context.

> [!IMPORTANT]
> Invariant: A blackboard operation resolves to exactly one hierarchy scope; reads do not expose payloads, and only `inspect` returns payload.

## Milestone CLI lifecycle

Use the CLI milestone commands to create, inspect, and close repository
contracts:

```sh
contextunity-forge-mcp milestone init --plan docs/plans/proposal.md [--dir docs/milestones] [--num 011] [--slug short-name] [--title "Title"] [--active]
contextunity-forge-mcp milestone list [--archive] [--status planned|active|completed|cancelled|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <full-sha>] --verification-command "<verified TESTS.md milestone gate>" --tests-passed <count> --tests-failed 0
```

`milestone init` chooses the highest current or archived milestone number in its
destination plus ten when `--num` is absent. It accepts a piped description and
task blocks and returns task
sync guidance. `--active` records the creation time as `started_at`; planned
documents gain that timestamp on the first accepted task claim.

`milestone list` reports the frontmatter state and SQLite completion ratio for
each current milestone. Status values are `planned`, `active`, `completed`,
`cancelled`, and `all`; `--archive` includes archived contracts, and completed
or cancelled status filters include archived contracts.
`milestone show` returns frontmatter, expected outcomes, and task metadata;
`--full` includes the complete task descriptions.

`milestone handoff` validates completion of every task belonging to the selected
milestone in SQLite. For each completed task, handoff resolves its landed commit in the milestone branch (the commit by which that task landed in the branch, whether via fast-forward or merge commit) and rewrites `receipt.commit` in each task block and SQLite to this landed SHA. The command records
the supplied implementation commit (default: current HEAD) in a YAML `handoff` block
with `completed_at`, `duration` as `Xh Ym`, and `verification` containing
`command`, `status: passed`, `tests_passed`, and `tests_failed`. Duration starts
at frontmatter `started_at`, or the earliest SQLite claim timestamp for an
older active document. The command sets frontmatter `status: completed`, moves
the document to the selected directory's `archive/`, and updates the stored
`milestone_ref` of its tasks. The subsequent milestone archive commit occurs after handoff and is excluded from `receipt.commit`, avoiding any hash cycle. Follow [milestone closure](../runbooks/acdd.md#close-the-milestone) for the final
verification gate, handoff, and archiving.

### Deferred and out-of-scope defects

Milestone documents maintain a typed block for out-of-scope defects, uncovered edge cases, or deferred review findings directly below the tasks:

```yaml
deferred_defects:
  - id: DEFECT-001
    source: review_findings
    title: "Unshadowed function resolution in edge case"
    path: src/engine/languages/python.rs
    disposition: deferred
    notes: "Follow up in subsequent milestone"
```

Fields:
- `id`: Unique identifier slug (checked via `valid_identity`).
- `source`: Discovery source (`review_findings`, `task_blackboard`, worker ID).
- `title`: Non-empty description of the defect.
- `path`: (Optional) Targeted relative file or directory path.
- `disposition`: `deferred`, `subsequent_milestone`, or `rejected` (defaults to `deferred`).
- `notes`: (Optional) Reproduction steps, context, or deferred milestone reference.

## CLI and administration

```sh
contextunity-forge-mcp task list [--repository NAME|all] [--milestone REF] [--status STATUS] [--stage STAGE] [--milestone-status active|planned|completed|all] [--planned|--completed|--all] [--full]
contextunity-forge-mcp task create MILESTONE_REF TASK_REF [--workspace NAME]
contextunity-forge-mcp task sync [MILESTONE_REF] [--workspace NAME]
contextunity-forge-mcp task inspect TASK_ID
contextunity-forge-mcp task claim TASK_ID --stage STAGE --worker WORKER --worktree PATH
contextunity-forge-mcp task submit TASK_ID --stage STAGE --action pass|reject --evidence '<JSON_OBJECT>' [--findings JSON]
contextunity-forge-mcp task blackboard post [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] --topic TOPIC --payload TEXT [--author WORKER]
contextunity-forge-mcp task blackboard read [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] [--topic TOPIC] [--limit N] [--offset N]
contextunity-forge-mcp task blackboard inspect MESSAGE_ID
contextunity-forge-mcp task extend-scope TASK_ID PATH...
contextunity-forge-mcp task delete TASK_ID [--force]
contextunity-forge-mcp task delete --milestone REF [--workspace NAME] [--force]
contextunity-forge-mcp task reset TASK_ID
contextunity-forge-mcp task reopen TASK_ID
contextunity-forge-mcp task cleanup
contextunity-forge-mcp migrate preview [MILESTONE_REF]
contextunity-forge-mcp migrate apply [MILESTONE_REF]
contextunity-forge-mcp migrate verify [MILESTONE_REF]
```

Task list defaults to active milestones and compact subtask references/statuses.
Use `--milestone-status planned|completed|all` or its mutually exclusive
`--planned`, `--completed`, and `--all` shorthands to select other milestone
sets; `--milestone REF` targets one milestone and defaults that query to all
milestone statuses. Use `--full` to include subtask titles and evidence.

Blackboard `--scope` selects milestone, task, or subtask messages. If scope and
keys are omitted, the command resolves one in-progress task, otherwise one
active milestone; ambiguous contexts fail closed. Read pages default to 10 and
cap at 50, omit payload, and return pagination metadata. `inspect MESSAGE_ID`
returns the payload for one message.

Reset (or `task reopen`, also available via MCP `task_manage` with `action: "reset"` or `"reopen"`)
abandons ownership, increments claim revision, and marks the task pending. For in-progress tasks,
passed predecessor gates are preserved. When executed on a completed task, reset cleanly reopens it:
it clears terminal receipt data from both SQLite and the milestone Markdown document, resets the
active gate to the first gate in the active profile, clears `completed_at`, restores `ready` status, and retains existing subtask
history so new subtasks (e.g. audit or remediation items) can be added and progressed.
Additionally, when a milestone document increments `contract_revision`, `task sync` re-admits and
reopens completed tasks into `ready` at the new contract revision.

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

## Unified Task Context Bundle

To achieve zero-shot agent orientation and eliminate exploratory tool-call loops, Forge aggregates task context by default upon MCP `task_claim` and CLI `task claim`, as well as through `task_manage(action: "context")` and CLI `task context <task-id>`. The MCP `task_claim` argument `bundle: false` returns minimal details; CLI `task claim` always returns the context bundle.

### Proof-Taxonomy Bundle Generation

Per [ADR 0016](../adr/0016-lean-stage-tailored-task-context-bundles.md), the bundle follows the active gate's configured proof kind and provides the material needed for that gate. Gate IDs are profile-defined; the embedded default profile uses `contract`, `build`, `review`, and `deliver`.

1. **`contract`** (All stages): Complete task contract, goal, active stage, status, proof policy, allowed write scope, invariants, subtasks, task dependencies (`depends_on`), worker identity, and revision counters.
2. **`guidance`** (All stages): Role, recommended tools, steps, command registry, policies, review sources, and review contours come from the active profile. A gate with `independent_from` also receives its independence rule and configured review policy.
   - `recommended_tools`: Stage-specific tool recommendations (e.g. `code_map_overview` and ADR reads at contract, `ast_grep_search` and `code_map_inspect` at build, `code_map_impact` and `code_map_prove_removal` at review).
   - `actionable_steps`: Concrete operational steps to advance the gate.
   - `subtask_dod`: Subtask acceptance criteria.
3. **`adrs`** (contract, command, and review proof gates): Scope-to-ADR mapping querying `docs/adr/` and `docs/architecture/`. Documents are included when their path or content matches scope tokens; unrelated documents are omitted.
4. **`scope_symbols` and `covering_tests`** (contract and command proof gates): High-value symbol skeletons and test suites covering declared scope files or domain boundaries. Signatures are safely truncated along Unicode character boundaries at 200 characters.
5. **`contract_seam_test` and `unresolved_review_findings`** (command proof gates): The prior accepted contract seam reference and, when a rejected review or delivery gate routes back to this gate, the structured findings for that rejection.
6. **`candidate_snapshot`** (review proof gates): The nearest prior accepted `sha_snapshot` commit, its captured baseline parent, and `inspect_cmd` (`git show <commit>`) for review against the configured contours.
7. **`proof_schema`** (schema proof gates): The recursive JSON schema the worker must satisfy under `scheme_proof`.
8. **`latest_snapshot` and `milestone_ref`** (delivery proof gates and completed tasks), and **`receipt`** (completed tasks): The nearest accepted candidate snapshot, inspect command, milestone file path, and durable completion receipt.
10. **`blackboard`** (All stages): Active collaboration messages and architectural notes from the full milestone hierarchy: task-scoped (`task_id`), parent milestone-level (`milestone_ref` where `task_id IS NULL`), and milestone sibling tasks. Every message is explicitly annotated with origin metadata (`scope: "milestone" | "task" | "subtask" | "sibling"`), `task_id`, and `subtask_ref`.

### Response Bounding and Anti-Bloat Invariants

To guarantee responses remain well below the 64 KiB ceiling of [ADR 0011](../adr/0011-bounded-mcp-response-budgets.md):
- **Raw table pruning**: Raw SQLite dumps (`gates` test logs, `attempts`, `findings`, raw `spec`, and root `receipt`) are pruned from context responses, while `depends_on` is preserved in `contract` and at envelope root, and completion `receipt` is preserved inside `context_bundle` for completed tasks. Detailed historical archives remain accessible on demand via `task_manage(action: "inspect")` or CLI `task inspect <id>`.
- **Minimal claim mode (`bundle: false`)**: Delivers lean task metadata (`task_id`, `stage`, `status`, `allowed_write_scope`, `subtasks`, `depends_on`, `workflow_guidance`) strictly pruned of heavy `gates`, `attempts`, `findings`, `receipt`, and raw `spec` archives, ensuring minimal claim mode never exhausts response budgets.
- **Zero root duplication**: Substructures are scoped to `context_bundle` without duplicating `adrs`, `scope_symbols`, or `covering_tests` at root.
- **Bounded blackboard**: Aggregates the 15 most recent chronological messages; payloads exceeding 500 Unicode characters are safely truncated along character boundaries (use `task_blackboard(action: "inspect", message_id: <id>)` for full text).
- **Size budget**: Bounded context bundles typically range between 6 KiB and 20 KiB, eliminating response limit exhaustion.

### Context Bundle Example

Below is an illustrative payload returned upon claiming a task (`task_claim` or `task context <task-id>`):

```json
{
  "task_id": "forge-mcp/forge-mcp/m-sample:language-feature",
  "status": "ready",
  "stage": "contract",
  "allowed_write_scope": ["src/engine/languages/html.rs", "tests/languages/html/syntax.rs"],
  "context_bundle": {
    "contract": {
      "task_id": "forge-mcp/forge-mcp/m-sample:language-feature",
      "target": "Add HTML template island extraction",
      "stage": "contract",
      "status": "ready",
      "proof_policy": "seam-test-first",
      "allowed_scope": ["src/engine/languages/html.rs", "tests/languages/html/syntax.rs"],
      "subtasks": [
        {
          "subtask_ref": "wire-syntax",
          "title": "Parse embedded expressions",
          "status": "pending",
          "evidence": null
        }
      ],
      "depends_on": [],
      "invariants": [
        "INV-HTML-ISLANDS: HTML islands maintain durable coverage provenance."
      ],
      "contract_revision": 1,
      "claim_revision": 1,
      "worker_id": "builder-1",
      "worktree": "/path/to/worktree"
    },
    "guidance": {
      "stage": "contract",
      "recommended_tools": [
        "code_map_overview",
        "get_doc",
        "search_docs",
        "code_map_inspect"
      ],
      "actionable_steps": [
        "Author a sensitive failing seam test in the designated test suite (or verify existing seam directly with exit code 0 if proof_policy: direct-proof or deferred-final-test).",
        "Submit contract proof with seam test and red_exit_code."
      ],
      "subtask_dod": [
        "Contract slice resolution: Demonstrably resolves an explicit, bounded slice of the contract without breaking boundaries.",
        "Milestone & ADR alignment: Builds upon existing architectural seams rather than ad-hoc isolated patches.",
        "Production-seam evidence: Validates through real production paths rather than synthetic stubs mocking away system complexity.",
        "Systemic non-regression: Preserves existing behavior and untouched invariants.",
        "Anti-looping invariant: Retain fail-closed boundaries and escalate via task_blackboard topic blockers if repeated attempts stall."
      ]
    },
    "adrs": [
      {
        "path": "docs/adr/0013-data-driven-framework-manifests.md",
        "title": "Data-Driven Framework Manifests",
        "status": "superseded",
        "relevance": "direct"
      }
    ],
    "scope_symbols": [
      {
        "name": "parse_template",
        "kind": "function",
        "path": "src/engine/languages/html.rs",
        "line": 45,
        "signature": "pub fn parse_template(source: &str) -> Result<Template>"
      }
    ],
    "covering_tests": [
      {
        "path": "tests/languages/html/syntax.rs",
        "kind": "scoped_test",
        "match_reason": "declared_in_scope"
      }
    ],
    "blackboard": [
      {
        "id": 101,
        "scope": "task",
        "topic": "hypothesis",
        "author": "builder-1",
        "payload": "HTML extractor misses template attributes; targeted seam in languages/html/syntax.rs."
      }
    ]
  }
}
```
