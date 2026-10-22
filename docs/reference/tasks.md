---
title: "Repository task operations"
doc_type: api
---

# Repository task operations

Git milestone frontmatter and fenced YAML task blocks own specifications.
`src/engine/tasks/mod.rs` serves CLI and MCP through the independent SQLite store
in `src/db/tasks_store/mod.rs`. Code-index rebuilds preserve operational task state.
For the lifecycle, load the `contextunity-forge` skill; for worktree and Git rules,
follow root [AGENTS.md](../../AGENTS.md#worktrees-and-git-permissions).

## Configuration and identity

`tasks_db` defaults to `.forge/tasks.sqlite` when omitted from `forge-mcp.yaml`.
When running within a git worktree, relative paths (including the default) resolve
against the primary worktree root discovered via `git rev-parse --git-common-dir`,
ensuring all linked worktrees share the same central tasks database without requiring
machine-specific absolute paths in tracked configuration. In non-git or standalone
workspaces, relative paths resolve directly from the configuration directory.
Absolute paths open directly.

All MCP tools, including task tools, use the [worktree selection rules](mcp-tools.md#worktree-selection). An omitted selector always keeps the canonical server root, independent of active claims. Explicit selection loads branch-local adapter/configuration and milestone manifests while Git-linked worktrees retain the shared task database; standalone repositories remain independent unless configured to share a store.

The human-friendly alias cascade belongs to MCP requests. CLI task claim/next operations require an existing `--worktree` directory path; CLI query commands select their workspace with `--root`. Do not treat MCP name aliases as CLI path aliases.
Connections use WAL, a 5000 ms busy timeout, NORMAL
synchronization, foreign keys, and a 256 MiB mmap ceiling. Transactional changes
increment the `meta` generation; requests retain no cached task connection.

`task_repository` defaults to `forge-mcp`; `task_project` defaults to that
repository. Omitted milestone identity fields use the configured pair.
Matching milestone frontmatter may declare `repository` and
`project`. IDs are `project/milestone_id:task_ref`. The repository selects the Git checkout and is omitted from the task id. A legacy id `repository/project/milestone_id:task_ref` still resolves when that repository and project match the workspace. Operations and
cleanup remain restricted to the configured project, including forced deletion.
Unknown schemas and populated non-task databases fail before schema mutation.
Revision counters survive operational deletion.

For subproject milestones, explicit `project` frontmatter wins over the longest
configured `owners` directory prefix, then path inference, then `task_project`.
Apply a configured `aliases` mapping once to that selected name. Sync, list,
claim, submit, delivery receipts, and milestone handoff retain the resulting
canonical task namespace. See [workspace configuration](configuration.md#task-storage)
for mappings and examples.

Milestone frontmatter `status: in_progress` is read as `active`.

```yaml
tasks_db: .forge/tasks.sqlite
task_repository: forge-mcp
task_project: forge-mcp
agents_guidance: AGENTS.md
record_task_commits: true # or false to disable writing task commit hashes into Markdown receipts
# commit_tracking: milestone_only # optional alternative: all (default) | milestone_only | off
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
directory discovery scans numbered Markdown files, including archives. Batch
synchronization silently skips archive paths and tombstoned milestones; explicit
synchronization of either fails with `STORE_SYNC_FAILED` to prevent resurrection.
Store sync and create recheck the tombstone after acquiring the SQLite writer
lock, so a handoff that commits while synchronization waits remains authoritative.
Tombstone keys include the canonical repository, project, and milestone ID;
identical milestone IDs in other projects remain independent.
Unavailable linked roots are skipped. Duplicate names, roots, or task namespaces
are rejected before mutation.

The linked configuration's `name` selects its workspace. Its repository identity
defaults to that name, while its own `forge-mcp.yaml` can override `task_repository`
and `task_project`. All operations through the primary server use the primary
`tasks_db`; a linked repository's independent store is not used by this server.
Synchronize linked specifications explicitly before querying their queue.

`task_list` uses the workspace selected by the CLI root or current MCP request;
it omits linked tasks unless `repository: "all"` or a configured workspace name
selects them. Status defaults to `ready` in the unique active milestone. With no
active milestone it returns an empty page; with multiple active milestones it
requires `milestone_ref` or an explicit `milestone_status`. Each list entry is a compact card with `task_id`, `target`,
`status`, `stage`, `owner`, `agent_type`, and `rev` (the contract revision).
The single-workspace response puts `workspace_root`, `agents_guidance`, and
`workspace` beside `tasks`. With `repository: "all"`, `workspaces` maps each
`repository/project` namespace to those paths and its workspace name.
Use `task_manage` with `action: "inspect"` to read the full task specification
and attempts. Inspect Markdown uses `## Task record`, separate from the claim
context's `## Contract`, and renders equal scope, target, proof policy, and task
invariant projections once. An extended write scope shows its initial paths and
added paths separately. Distinct authority values and milestone invariants remain
visible; inspect retains revisions, receipts, gates, attempts, and findings while
omitting the contract digest. Existing task IDs route claim, submit, inspect, reset, delete,
and scope extension to their owning namespace. Inspect and claim include absolute
`workspace_root`, absolute `agents_guidance`, the milestone's local invariants,
and stage-specific `workflow_guidance`. The latter gives `active_stage`,
`role`, `mode` (`subagent` or `inline`), `role_guide` (advisory role guidance and recommended models, adapted for inline execution when `mode: inline`), and `steps`. At review and
delivery it also gives the independence rule and accepted builder identity.
After a `reject_to` rewind, `rejection` gives
`rejected_from`, the prior builder `worker_id` when reuse is enabled, and the
persisted findings; it is null on clean forward runs. Reuse guidance adds
`Return task to builder '{worker_id}' with review rejection findings to repair defects.`
If the instruction file is missing, does not name the `contextunity-forge` skill, that skill is not installed globally or in the repository, or `.gitignore` lacks the canonical protective rules, the response includes `TASK_GUIDANCE_MISSING` and inline steps.

Scope paths are checked in both their owner's root and the task's claimed isolated
worktree. Traversal and symlinks into another repository are rejected. Nested
configured repositories have distinct perimeters: parent scopes cannot include
the child root, and the child can claim work inside its own root. The most specific
configured root determines ownership. A worktree inside another repository's
perimeter cannot claim a foreign task.

### Scope Management and Co-Evolution Planning

Every task contract declares an explicit file perimeter (`scope`) bounding allowable writes. Forge guards these write boundaries in SQLite (`task_scope_paths` table):

- **Exclusive Scope Locks and Sibling Conflicts**: Declared scope paths are registered with `frozen = 1`. During sync, overlapping task scopes create predecessor-to-successor dependencies in task declaration order. These SQLite edges preserve `TaskSpec.depends_on` and authority digests. `wave_index` is zero without dependencies, otherwise one plus the greatest predecessor wave. An active sibling with exact, ancestor, or descendant overlap blocks a claim with `TASK_SCOPE_CONFLICT`.
- **Dynamic Extension (`extend_scope`)**: A claimed task extends its perimeter within admitted base roots via `task extend-scope <task_id> <path>...` or MCP `task_manage(action: "extend_scope", ...)`. If an active sibling holds the requested scope, one transaction admits the paths, adds holder dependencies, publishes a `blockers` note, ends the claim, and returns `status: "parked"` with `blocked_by`. The descriptor retains `parked_worker_id` and `parked_worktree`; the same worker resumes in that worktree after the holders complete. Conflicts with other nonterminal owners remain fail-closed.
- **Terminal Task Scope Release**: Completed and cancelled tasks retain their scope history without holding active scope locks. Cancellation leaves dependencies unsatisfied; a dependent task still requires a completed predecessor.
- **Protected System Scopes and Dirty File Prohibition**: System configuration and workflow profile files (`.forge/acdd/**` and profile definitions) are protected system paths (`TASK_SCOPE_PROTECTED`). Regular tasks cannot claim or extend into these paths via `extend_scope`. Furthermore, during `task_claim`, gate execution, and `task_submit`, the compiled binary inspects the worktree git status: if any file under `.forge/acdd/**` is dirty (modified, unstaged, staged, or untracked) and is not explicitly admitted in `task.spec.scope`, Forge fails closed with `TASK_SCOPE_VIOLATION`. This guarantees that agents cannot mutate workflow profiles, weaken review contours, or bypass test gates on the fly. Legitimate profile updates are performed either directly outside task cycles or within dedicated configuration tasks whose explicit scope admits `.forge/acdd/profile.yaml`.
- **Documentation Co-Evolution Planning**: Shared documentation (such as `docs/reference/tasks.md`, `README.md`, or architecture documents) frequently evolves across multiple milestone tasks. When multiple tasks in a milestone must update the same documentation or common shared modules, dependencies must be sequenced via `depends_on`. Once the predecessor task completes, the successor task gains lawful access to extend into and evolve the shared documentation. Alternatively, documentation changes can be planned alongside code changes within the owning task's build gate, ensuring documentation and implementation remain synchronized without scope friction.
- **Task Dependencies and Cross-Milestone Sequencing (`depends_on`)**: Task contracts declare local dependencies (`task_ref`), cross-milestone task dependencies (`milestone_id:task_ref`), or external project dependencies (`project/milestone_id:task_ref`). Dependent tasks remain in `blocked` status until all predecessor tasks are completed, preventing premature claims (`TASK_NOT_READY`).

## Five flat MCP tools

`task_list` accepts `limit` (1–100, default 30), `offset` (default 0), and
`generation`; nonzero offsets require the previous page's token. Pagination
returns `total`, `limit`, `offset`, actual `returned`, optional `next_offset`,
`has_more`, the task-view `generation`, and `byte_omitted`. The generation binds
the selected canonical workspace and namespace, milestone, filters, and complete
task/subtask state. It does not use the code-index generation. Changed task or
subtask state and changed filters reject a continuation with a restart hint;
index-only changes do not invalidate it. Byte trimming preserves intact task
records and every nested subtask, advances by the actual returned count, and
keeps the next offset absent at the final page. Continuation hints retain the
effective workspace and filters. A single record or required page metadata that
cannot fit returns a bounded actionable error without splitting a record.

`task_list` accepts `milestone` as an alias for `milestone_ref`, with the same
milestone resolution and status defaults. It resolves safe logical spellings such
as `UX-001`, `UX-001.md`, `docs/milestones/UX-001`, and
`milestones/UX-001.md` to the matching manifest, including those spellings with
one leading slash. Existing IDs, numeric prefixes, complete slugs, configured
paths, and archive paths retain their normal resolution. Traversal, arbitrary
absolute paths, foreign roots, ambiguity, and invalid existing manifests fail
closed. Supply one name per request.

When no milestone matches, `task_list` also accepts a full task ID or a unique
task slug in the selected workspace set. The response contains one `resolution`
object with `kind: "task_parent"`, the original `input`, canonical `task_id`,
parent `milestone_id`, and canonical `milestone_ref`. Markdown displays one
`[auto-resolved task '<input>' to parent milestone '<milestone_id>']` note.
Direct milestone matches take precedence, including milestone IDs that resemble
task slugs. Ambiguous or missing task selectors remain errors.

The CLI accepts `task show` for `task inspect`, `--task TASK_ID` in place of
the positional task ID on inspect/show and submit, and `--file PATH` in place
of the positional milestone reference on sync. `task list --detail compact|full`
selects subtask detail explicitly; `--full` selects full detail. Selector forms
and the two detail flags are mutually exclusive within one invocation. See the
[CLI reference](cli.md#repository-tasks) for examples.

| Tool | Arguments and behavior |
| --- | --- |
| `task_list` | Optional `repository`, `milestone_ref`, `milestone_status`, `status`, `stage`, `detail`, `limit`, `offset`, and `generation`. Task status defaults to `ready`; values are `ready`, `in_progress`, `blocked`, `parked`, `completed`, `cancelled`, and `all`. `parked` selects tasks waiting for a scope owner with retained worker/worktree affinity; `all` includes them and retained cancelled tasks. Milestone status defaults to `active`; values are `active`, `planned`, `completed`, and `all`. A targeted `milestone_ref` defaults milestone status to `all`. With no explicit milestone under the active default, one unique active milestone is selected, zero yields an empty list, and multiple candidates fail with an ambiguity hint. `limit` accepts 1–100 (default 30); nonzero `offset` requires the task-view generation. `stage` matches a gate ID declared by the active profile; null matches every gate. Subtask detail defaults to compact references and statuses; `full` includes titles and verification evidence. Each task summary includes its declared `depends_on` references. |
| `task_claim` | Required `task_id`, `stage`, `worker_id`, and `worktree`; optional `bundle` (boolean, defaults to true). Claims the current gate atomically, returning context tailored to the active gate (see below). Set `bundle: false` for minimal details. Collisions return typed `TASK_ALREADY_CLAIMED`. |
| `task_submit` | Required `task_id`, `stage`, JSON object `evidence`, and `action` (`pass` or `reject`); optional JSON `findings`. Reject requires findings. |
| `task_manage` | Required `action`; optional `workspace`, `task_id`, `milestone_ref`, `task_ref`, `paths`, `force` (default false), `subtask_ref`, `title`, `subtask_status`, and `evidence`. `next` additionally requires `worker_id` and a `worktree` path in CLI/engine action input. MCP accepts `worktree` as a routing selector for every action and requires it for `next`. See the action table below. |
| `task_blackboard` | `action` is `post`, `read`, or `inspect`. Optional `scope` selects `milestone`, `task`, or `subtask`; `milestone_ref`, `task_id`, and `subtask_ref` identify or constrain that context. Post requires canonical `topic` and `payload`, accepts `author` and optional active-profile `gate`, and returns an ID. Read accepts `topic`, optional active-profile `gate`, `limit` (default 10, maximum 50), and `offset`; it returns newest-first summaries and pagination metadata without payload. Inspect with `message_id` returns that message including payload and gate, ignoring optional `scope`, `task_id`, and `milestone_ref`; without `message_id`, inspect returns the scoped read page. |

For MCP calls, every task tool accepts `worktree` as a routing selector;
`task_claim` and `task_manage(action="next")` require it, while `task_submit`
accepts it optionally. Canonicalize claim and next selectors before engine
execution and persistence. For other `task_manage` actions, remove the selector
before engine validation; submit routing never replaces claim-bound evidence
identity.

Unknown fields are rejected. Ready tasks are unclaimed and nonterminal, with
satisfied local prerequisites. Missing or amended authority blocks open work.
Null stage matches all gates.

Task tools and CLI task commands default to Markdown. Use MCP `format: "json"`
or global CLI `--format json` to parse the semantic payload. The lifecycle
layouts preserve actual status, dependencies, requested subtask detail, claim
guidance, inspect history, findings, candidate bindings, and receipts. See the
[task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle)
for blocker callouts and message recovery.

| Manage action | Required CLI/engine action fields | Other constraints |
| --- | --- | --- |
| `next` | `worker_id`, `worktree` path | MCP also requires its routing selector; on other MCP actions this optional selector is removed before engine validation. Optional workspace; omit task/milestone/subtask selectors, paths, evidence, and force. Returns a claim with context bundle, or `status: "idle"` when no task is eligible. |
| `create` | `milestone_ref`, `task_ref` | Optional workspace; omit task_id/paths; force=false. |
| `sync` | `milestone_ref` or `workspace` | Optional explicit file within workspace; omit task_id/task_ref/paths; force=false. |
| `inspect` | `task_id` | Omit workspace/milestone_ref/task_ref/paths; force=false. |
| `context` | `task_id` | Omit workspace/milestone_ref/task_ref/paths; returns zero-shot context bundle without claiming. |
| `delete` | Exactly one of `task_id`, `milestone_ref` | Workspace is allowed only with milestone_ref; omit task_ref/paths. |
| `extend_scope` | `task_id`, nonempty `paths` | Omit workspace/milestone_ref/task_ref; force=false. |
| `subtask_add` | `task_id`, `subtask_ref`, `title` | Optional workspace; omit milestone_ref/paths; force=false. |
| `subtask_update` | `task_id`, `subtask_ref`, `subtask_status` | Optional `evidence`, `workspace`; status is `ready`, `in_progress`, `completed`, `cancelled`, or `blocked`; legacy `pending` aliases `ready`. |
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

`task_manage(action: "next")` selects and writes one claim under an immediate
SQLite transaction. Priority is independent delivery, independent review,
unblocked parked work belonging to the requesting worker, then ordinary ready
contract/build work. Ordinary work sorts by `wave_index`, then `task_id`.
Review and delivery exclude the accepted builder; all candidates require satisfied
dependencies and no active scope overlap. Parked work uses its retained worktree
even when the dispatcher receives another path. Authority and worktree validation
run before the claim is persisted; activation or context failures release it.

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

Tasks and subtasks share `ready`, `in_progress`, `completed`, `cancelled`, and
`blocked`. Only tasks support `parked`, retaining worktree affinity during scope
conflicts. Subtasks record progress; they do not participate in task dispatch or
satisfy task dependencies.

Task specification `status: cancelled` is admitted through `task sync`. It ends
an active claim, increments its revision, and retains the task, attempts, gates,
subtasks, and evidence. Cancelled tasks appear in `--status cancelled` and `all`,
cannot be claimed or dispatched, and leave dependent tasks blocked. Sync does
not revive them merely because the manifest returns to `ready`: use reset/reopen
after restoring the specification, or admit a higher contract revision. Sync
rejects cancellation of an accepted completed task and rejects completion receipts
on cancelled specifications. Milestone-level cancellation retains its separate
pruning behavior; see [milestone lifecycle](#milestone-cli-lifecycle).

Milestone handoff retains cancelled tasks when their manifest and SQLite identity,
contract digest, revision, status and milestone location agree. Cancellation
supplies no completion receipt, accepted builder, dependency satisfaction or
implementation scope permission. Other incomplete tasks still block handoff.

Complex domain tasks often contain finer-grained milestones, discovered edge cases,
or sub-component checklists (e.g. testing specific UI fixtures, verifying individual DOM
queries, or step-by-step refactoring). Instead of proliferating root-level milestone tasks
that inflate the queue and require a full configured gate lifecycle,
agents should deepen the active task using **subtasks**.

- **Schema**: Each subtask contains `subtask_ref` (alphanumeric identity slug), `title` (goal description),
  `status` (`ready`, `in_progress`, `completed`, `cancelled`, or `blocked`), and optional `evidence` (verification command or test notes). Omitted status defaults to `ready`; legacy `pending` is accepted in manifests, stored descriptors, SQLite rows, CLI and MCP inputs, and returned as `ready`.
- **Canonical State**: During active execution, SQLite (`task_subtasks` table
  and `Task.spec.subtasks` descriptor) is the canonical operational state. Upon task completion at the profile's terminal delivery gate,
  Forge serializes all subtasks directly into the milestone Markdown fenced YAML block alongside the delivery receipt.
  The milestone document is the durable canonical record that survives SQLite clearing, rebuilds, and milestone handoff.
- **Post-Delivery Immutability**: Subtasks are strictly operational execution artifacts. Once a task passes its terminal delivery gate
  (`status: completed`), its subtasks and receipt are frozen. Calling `subtask_add` or `subtask_update` on a completed or cancelled task
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

The embedded profile assigns `contract_author`, `builder`, and
`independent_reviewer` subagents. The contract author posts a blackboard `draft`
targeted to `build`; the independent reviewer continues in the same worker
session through delivery. Claim guidance includes the `NO_PREFLIGHT_SUBAGENTS`
rule and `workflow_guidance.expected_evidence_template`, which is bound to the
active task, stage, claim and contract revisions, worker, and worktree.

### Context bundle matrix by proof taxonomy

When claiming a gate with `task_claim`, Forge returns tailored context bundles based on the gate's closed `proof` taxonomy:

| Proof Kind | Claim Context Bundle Content | Primary Role Guidance |
|---|---|---|
| `contract` | Admitted target, invariants, declared scope paths, proof policy, seam test template/reference, indexed scope symbols, external callers, covering tests, and ADR summaries. | Contract Author: write red public-seam test, post a build-targeted draft, submit non-zero red exit code. |
| `command` | Target, invariants, admitted scope paths, verified contract proof, indexed scope symbols, external callers, covering tests, ADR summaries, scoped commands, and evidence template. | Implementer: implement within scope, make seam test pass, and submit passing command proof. |
| `review` | Target, invariants, scope paths, candidate snapshot SHA (from predecessor `sha_snapshot: true` gate), git diff against baseline HEAD, contour checklist with operational criteria from `gate.contours`. | Independent Reviewer: verify candidate diff against contour checklist, submit review proof with decision. |
| `delivery` | Target, invariants, verified candidate snapshot SHA, all passing review proofs from `review_sources`, completed subtasks summary, receipt template. | Deliverer: verify rollup proofs and invariant checks, finalize receipt. |
| `none` | Target, invariants, admitted scope paths, informational guidance. | Worker: perform non-verified informational step. |
| `scheme` | Target, invariants, admitted scope paths, recursive JSON schema definition for expected proof payload. | Schema Worker: submit structured payload matching configured recursive schema. |

For contract and build proofs, the context bundle adds `scope_symbols`,
`external_callers`, and `covering_tests`. External callers are indexed one-hop
`calls` or `imports` edges from production symbols in scope to caller files
outside the admitted scope. Covering tests include direct and two-hop indexed
test dependencies. Each ADR context item includes up to five `invariants_summary`
lines. `workflow_guidance.scoped_commands` combines registered command prefixes
with the contract seam or indexed covering-test targets, while
`expected_evidence_template` supplies the claim-bound evidence shape.

`evidence` is a direct JSON object bound to the active claim. The `commit` field can be omitted; Forge automatically captures the scoped candidate snapshot SHA on predecessor gates with `sha_snapshot: true` (e.g. `contract` and `build`), and inherits it on downstream gates (`review` and `deliver`). For example, a passing build submission contains:

```json
{
  "task_id": "forge-mcp/m-example:implementation",
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
is retained in `task_gates.evidence` without reading an evidence file. Supplied SHA values may be 7-to-40-character hexadecimal prefixes of the expected commit; Forge resolves them to the full canonical SHA before comparison and persistence. Task API responses retain full SHAs in receipt and snapshot fields, while `inspect_cmd` may use a short display form.

When a code map is available, passing contract and build submissions also
enforce indexed task boundaries. Contract proof returns
`TASK_SEAM_DISCONNECTED` unless its exact indexed test symbol has a current
one- or two-hop path to a non-test production symbol inside admitted scope.
Supported witness edges are `calls`, `imports`, `possible_calls`, `implements`,
and `overrides`. Forge checks each hop's source and target identities, current
typed outgoing references, and applicable import bindings against source parsed
from the claimed worktree. It uses that worktree's code map when present;
otherwise it uses the workspace's read-only code map. Missing or stale indexed
evidence does not prove a seam, so refresh the claimed-worktree code map after
changing a test, helper, import, or binding used by the witness.

Rust integration tests can import the root library through its Cargo crate name,
including the underscore form of a hyphenated package name. An explicit
`[lib].name` supplies the library name instead. Forge normalizes these imports
against the indexed root `src/lib.rs`, using the admitted Cargo manifest snapshot
for the owning workspace. Ordinary imports, aliases, module-qualified calls and
glob imports provide the same seam evidence as internal `crate::` imports.
Missing or malformed package metadata and unrelated crate names do not establish
a local library identity. Refresh the index after changing the Cargo identity;
existing manifest-digest admission invalidates its previous bindings.
This mapping covers the root `src/lib.rs` layout. A different `[lib].path` or
`autolib = false` without an explicit library target does not establish that
identity, even when an unused `src/lib.rs` exists in the index.

For greenfield features with uncreated scope files, create minimal compiling
production stubs and module/import declarations in admitted scope, then refresh
the index. Changing proof policy does not bypass seam reachability validation.
A changed indexed public
function or method signature with a caller outside admitted scope returns
`TASK_SCOPE_INCOMPLETE`, allowing the task to admit that caller or preserve the
signature. Rust trait methods are implicitly callable at the visibility of
their containing visible trait, including methods with default bodies and
declarations without bodies.

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
verified invariants and review summary. Task blackboard `decisions` and `deferred`
are stored as topic-labelled strings in the existing `architectural_notes` vector
(for example, `decisions: <payload>`), preserving historical receipt bytes and
the original message payload.
The task becomes completed in SQLite and its blackboard messages are cleared.
The milestone document is the durable context after task delivery. When `auto_commit` is active,
Forge has already committed the deliverable; when `auto_commit: false`, commit the task's scoped source,
tests, and generated receipt in its claimed task worktree under standing permissions.
Merge the delivered task branch into the milestone branch and delete the
temporary task branch. Final handoff validates immutable task proofs and the
[integration evidence](#milestone-cli-lifecycle), refreshes optional navigation
pointers, and archives the document. Commit the archive afterward. Completed tasks
can be reopened for actual task repairs with `contextunity-forge-mcp task reopen
<task-id>`; branch history rewrites alone do not require reopening them.

Zero runtime backward compatibility applies strictly to SQLite runtime storage and active milestone manifests. Historical receipts archived in `docs/milestones/archive/` are read as-is and exempt from active profile receipt schema validation. Filtering by `--stage <id>` matches directly against the persistent `task.stage` string ID in SQLite; CLI argument validation checks that the stage ID is defined in the active profile of the workspace or open tasks. If a task references a stage missing from the active profile's `gates`, Forge fails closed with `TASK_STAGE_UNKNOWN`, naming mismatched gates and suggesting either migrating task records or pinning the intended profile via `acdd_profile: <path>:<sha256_prefix>`.

## Task lifecycle diagnostics and error ergonomics

Claim, submit, manage, and reset errors use one single-line format:
`{CODE}: {task_id}. {facts}. Do: {action}`. Operations without a task selector use
the milestone reference or `unselected` in that position. The engine owns this
text and MCP propagates it verbatim.

### Structured Authority Diagnostics (`AUTHORITY_GAP` and `TASK_AUTHORITY_MISSING`)

Contract digests bind `repository`, `project`, `id`, `invariants`, `owners`, `depends_on`, optional `acdd_profile`, and the task body (excluding mutable runtime fields `receipt`, `status`, `subtasks`). When the file specification diverges from the database contract:
- **Field Attribution**: Digest differences are decomposed into qualified field names: milestone frontmatter (`milestone.invariants`, `milestone.owners`, `milestone.depends_on`, `milestone.acdd_profile`, `milestone.id`) versus task YAML (`task.target`, `task.proof_policy`, `task.scope`, `task.scope_roots`, `task.invariants`, `task.depends_on`, etc.).
- **Revision Comparisons**: Errors state whether `file revision <= stored` or `file revision > stored (unsynced)` and output the exact command to resolve divergence:
  - If frontmatter diverged: revert the mutated milestone frontmatter fields (or declare task-level invariants inside `task.invariants`).
  - If task YAML diverged: increment `contract_revision` in the task specification and run `contextunity-forge-mcp task sync`.
- **Milestone ID Protection**: Pre-detects if `milestone.id` changed relative to stored tasks before syncing or claiming:
  `AUTHORITY_GAP: milestone '{milestone_ref}'. milestone id changed from '{stored_id}' to '{file_id}'. Do: revert milestone id in {milestone_ref}. Task ids are {project}/{id}:{task_ref}; task cannot be resolved.`
- **`TASK_AUTHORITY_MISSING`**: Emits distinct actionable errors when the entire milestone file is missing versus when a specific task block is absent from the file.

### Lifecycle Error Codes

- `TASK_CANDIDATE_MISMATCH`: The candidate snapshot from the build gate does not match current worktree state. Restore the worktree to the candidate snapshot before retrying delivery or submitting rejection.
- `TASK_NOT_READY`: The task is blocked by unsatisfied dependencies or parent milestone state.
- `TASK_ALREADY_CLAIMED`: The task gate is currently held under an active claim revision by another worker.
- `TASK_STALE_SUBMISSION`: Submission evidence does not match the active gate, claim revision, or candidate snapshot baseline.
- `TASK_REVIEW_NOT_INDEPENDENT`: Independent review contour requires a `worker_id` distinct from the accepted builder.
- `TASK_SELECTOR_INVALID`: Unknown action or invalid field combination in `task_manage` or CLI commands.
- `TASK_STALE_CLAIM`: Active claim revision diverged or open claim row is missing; release or reclaim the task.
- `TASK_NOT_FOUND`: The requested task ID is not found in SQLite.
- `TASK_SCOPE_INVALID`: Specified path falls outside admitted base roots (or traversal detected).
- `TASK_SCOPE_CONFLICT`: Specified path belongs to an active, uncompleted sibling task in the milestone.
- `TASK_SCOPE_PROTECTED`: Configuration paths under `.forge/acdd/**` require explicit admission in initial task scope.
- `TASK_SCOPE_VIOLATION`: Dirty configuration or profile files detected in git status during claim or submission.

### In-Context Guidance Guardrails

Upon every `task_claim`, Forge injects standard operational rules directly into `workflow_guidance.rules` so agents receive runtime guardrails without relying on external documents:
1. `PROTECTED`: Milestone `id`, `invariants`, `owners`, `depends_on`, and `acdd_profile` are immutable while tasks are active.
2. `ISOLATION`: Develop exclusively inside the assigned dedicated worktree.
3. `IMMUTABILITY`: Keep admitted scope unchanged after review pass and during deliver.
4. `INDEPENDENCE`: Review stage requires a worker_id distinct from the accepted builder.
5. `NEVER_EDIT_RECEIPTS`: Never edit `receipt:` or `status:` blocks in `docs/milestones/*.md` manually. `task_submit(stage=deliver)` writes and commits receipts automatically.

## Task blackboard

`task_blackboard` stores messages in the configured task SQLite store with a
`milestone_ref`, optional `task_id`, optional `subtask_ref`, optional `gate`,
`author`, `topic`, `payload`, and `created_at`. The task-store schema is version 5;
the one-time v4→v5 migration adds the nullable gate column and gate query indexes,
then maps `contract_draft`→`draft`, `contract_findings`→`findings`,
`build_proof`→`notes`, and `architectural_notes`→`decisions`. Existing canonical
topics stay unchanged and unknown historical topics become `notes`. These aliases
are accepted only by that migration; runtime calls must use one of the six
canonical topics: `draft` for contract drafts, `notes` for build observations and
general context, `findings` for review findings, `blockers` for current blockers,
`decisions` for decisions retained in the delivery receipt, and `deferred` for
deferred findings retained in the delivery receipt. Post requires a canonical
topic and payload. An omitted author
uses the task owner when the resolved context has one, or the current transport
(`mcp` or `cli`). Post rejects read-only pagination fields; read rejects
`author` and `payload`. Post and read use the resolved task project. Inspect with `message_id` searches configured task workspaces and returns the first match. In this form, `scope`, `task_id`, `milestone_ref`, and `subtask_ref` are ignored so callers can reuse scoped request objects. Without `message_id`, inspect resolves the requested hierarchy scope and returns the same payload-free page as read, including topic, gate, limit, and offset filters.
Post and read accept optional `gate`, which must name a gate in the active task
profile. A gate on a post targets that message; a gate on a read filters for that
gate. Without a read gate, the requested scope returns messages from all gates.

Use `scope: "milestone"` for milestone-level messages. It resolves the explicit
milestone or the unique active milestone and never includes task-scoped entries.
An explicit milestone scope accepts the same safe logical aliases and unique
task-parent fallback as `task_list`; fallback returns the same structured
`resolution` provenance and one Markdown correction note. Reads and message
inspection keep their existing message IDs and scope columns; a fallback post
inserts exactly one message at the canonical parent milestone scope, which is
the `milestone_ref` reported by the blackboard resolution. Task and
subtask scopes use `milestone_ref` only as a direct milestone filter and do not
reinterpret task selectors.
Use `scope: "task"` or `scope: "subtask"` to select those levels. Omitted task
and subtask keys resolve only through unique in-progress contexts; missing or
ambiguous contexts fail closed. When scope and keys are all omitted, Forge uses
the unique in-progress task, fails if several are in progress, and falls back to
the unique active milestone only when no task is in progress.

Read returns one page of payload-free summaries, ordered by `created_at DESC`
and `id DESC`. The default `limit` is 10, the maximum is 50, and `offset` selects
the next page using the returned pagination metadata. Use `action: "inspect"`
with a `message_id` from a post or read result to retrieve the full message and
payload. Without `message_id`, inspect behaves like a scoped read. A claim or context bundle includes untagged messages and messages
targeted to its active gate. Gate-targeted messages also appear under
`workflow_guidance.blackboard_messages`; compact `blackboard_info` reports the
active gate, message ids, count, and topic counts. Invalid topics or gate ids
return the canonical topic descriptions, active profile gate ids, and a request
example so the request can be corrected.

Markdown read output groups messages by their actual topic and preserves the
newest-first order inside each group. A blocker renders as a caution callout with
its message ID, author, scope, gate, timestamp, and available body. Payload-free
read summaries include the exact `task_blackboard(action: "inspect", message_id: <id>)`
recovery request. Other topics remain separate from blocker
severity. Grouping does not infer reply relationships or whether a blocker is
resolved.

At delivery, messages under `decisions` and `deferred` are copied into the
receipt's existing `architectural_notes` vector as `decisions: <payload>` or
`deferred: <payload>`, in `created_at`, then `id` order, before task blackboard
messages are cleared.

> [!IMPORTANT]
> Invariant: A blackboard operation resolves to exactly one hierarchy scope; reads do not expose payloads, and only `inspect` returns payload.

## Milestone CLI lifecycle

Use the CLI milestone commands to create, inspect, and close repository
contracts:

```sh
contextunity-forge-mcp milestone init --plan docs/plans/proposal.md [--dir docs/milestones] [--num 011] [--slug short-name] [--title "Title"] [--active]
contextunity-forge-mcp milestone list [--archive] [--status planned|active|completed|cancelled|all]
contextunity-forge-mcp milestone show <id-or-number> [--full]
contextunity-forge-mcp milestone handoff-context <id-or-number> --target-base <git-selector>
contextunity-forge-mcp milestone handoff <id-or-number> [--commit <git-selector>] --evidence-file <external-review.json>
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

Commit the final implementation and follow the instruction-level documentation
scan in root [AGENTS.md](../../AGENTS.md#documentation-scan-before-handoff).
Run `milestone handoff-context <id> --target-base main` in the milestone worktree.
The command validates the exact manifest and SQLite task set: completed tasks
require their accepted proofs, and identically cancelled tasks require no
completion receipt. It binds cancelled authority separately from completed
proofs, retains required
Git proof objects, and returns `binding`, resolved `target_base`, and diagnostic
`commit`. Obtain an independent review of the combined delta against this base
and run the repository verification from [TESTS.md](../../TESTS.md). The reviewer
must differ from every worker listed in `binding.builders`.

Handoff validates the manifest's original task scope separately from operational
scope extensions. The stored effective scope must equal the original scope plus
the admitted extension ledger, within its frozen boundary roots; unrecorded or
out-of-bound scope changes reject with `TASK_AUTHORITY_CHANGED`. Accepted gate
proofs, contract revisions and receipts remain unchanged.

Supply a JSON `IntegrationEvidence` record through `--evidence-file`:

| Field | Required value |
| --- | --- |
| `binding` | Copy the complete generated binding unchanged: version, repository, project, milestone, authority/content/base digests, and builders. |
| `target_base` | The context's resolved target-base commit. Selectors are resolved again and their content is checked. |
| `review.worker_id` | Independent integration reviewer identity. |
| `review.decision`, `review.summary` | `pass` and a nonempty account of the combined review. |
| `review.scope_findings`, `review.contract_findings` | Empty arrays after resolving unadmitted changes and contract/invariant findings. |
| `verification.command`, `verification.exit_code` | Exact completed repository verification command and `0`. |
| `verification.tests_passed`, `verification.tests_failed` | Recorded counts and `0` failures. |

Keep the evidence file outside tracked implementation content, for example under
`/tmp/`. Forge reads the record; the coordinator runs verification and launches
review/documentation agents according to repository instructions.

> [!IMPORTANT]
> Invariant: Final evidence binds current task contracts and accepted gate proofs,
> the selected target-base content, and every tracked implementation path, mode,
> addition, deletion and file content. The versioned projection removes only
> renderer-owned milestone frontmatter `status`, `started_at`, `handoff`, task
> `status`/`receipt`, and generated subtask `status`/`evidence`; contract and prose
> bytes remain bound. Unfamiliar YAML spellings stay bound. Dirty files, untracked
> deliverables, foreign/stale evidence, failed verification, non-independent review,
> incomplete tasks, and paths outside admitted task scopes reject before publication.

Rebase, cherry-pick, merge, amend, and multi-task squash may change branch SHAs
without invalidating unchanged content evidence. Upstream integration and conflict
resolutions are assessed in the final combined review. Changed implementation,
target-base content, configuration, documentation, or authority needs fresh final
review and verification; it does not reopen every completed task. Accepted task
proofs and decisions remain unchanged. Original baselines need not occur on the
current first-parent history.

Task `receipt.commit` is navigation metadata: handoff retains known accepted
ancestor pointers or refreshes a unique scope-equivalent rewritten landing;
unresolved pointers are omitted. `record_task_commits: false` and `commit_tracking:
off|milestone_only` omit individual navigation pointers in both SQLite and Markdown.
Accepted proof objects remain reachable under `refs/forge/evidence/` after branch
rewrites, reflog expiry and handoff; preparing context pins available historical
proofs. Restore missing immutable proof objects rather than recreating accepted
gate evidence.

Handoff archives the supplied integration record and verification result in
frontmatter, records the selected final implementation commit (default `HEAD`),
sets `status: completed`, and by default prunes completed and cancelled tasks
and their claims, gates, findings, scopes, submissions, subtasks, revisions, and
blackboard messages from SQLite. The transaction rechecks the original milestone
location and complete task state, satisfies existing incoming dependencies on
completed tasks and the terminal milestone, and records a tombstone with completed
task IDs. The archive preserves task receipts and terminal subtask evidence;
cancelled tasks receive no completion receipt. Set the project-wide
`handoff_pruning: false` in `forge-mcp.yaml` to retain task history with references
relocated to `archive/`, without creating a pruning tombstone. See
[configuration](configuration.md#declarative-acdd-profile-and-role-configuration).
Duration starts at `started_at` or the earliest
claim timestamp. Commit the archive afterward, staging its addition and the
working manifest's removal; this later commit SHA is not part of its own receipt.
Historical archived receipts remain readable without rewriting them.

Build validation treats a missing Git-enumerated scoped source file as deleted,
including an unstaged deletion. Other read failures still reject the submission.
Removed public symbols retain their accepted baseline caller checks, even after
reindexing: admit each affected caller's update or deletion in the task scope.
An unadmitted baseline caller still blocks retirement; deleting its working copy
does not remove it from the scoped candidate snapshot.

Before contract acceptance, a retired baseline symbol must still exist in the
index so its callers can be established. A refreshed index missing that symbol
does not prove zero callers: restore the baseline symbol and refresh the index,
then accept the contract before retirement. Accepted caller evidence remains
available to build validation after subsequent reindexing.
Older accepted captures with no callers and no target-presence confirmation
require the baseline symbol to remain indexed. If it is missing, restore it,
refresh the index, and accept the contract again before retirement. Previously
captured nonempty caller sets continue to protect their caller paths.


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
contextunity-forge-mcp task blackboard post [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] [--gate GATE] --topic TOPIC --payload TEXT [--author WORKER]
contextunity-forge-mcp task blackboard read [TASK_ID] [--scope milestone|task|subtask] [--milestone-ref REF] [--subtask-ref REF] [--gate GATE] [--topic TOPIC] [--limit N] [--offset N]
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

Blackboard `--scope` selects milestone, task, or subtask messages. Optional
`--gate` targets a post or filters a read using the active task profile. Topics
must be one of `draft`, `notes`, `findings`, `blockers`, `decisions`, or `deferred`.
If scope and keys are omitted, the command resolves one in-progress task, otherwise one
active milestone; ambiguous contexts fail closed. Read pages default to 10 and
cap at 50, omit payload, and return pagination metadata. `inspect MESSAGE_ID`
returns the payload for one message.

Reset (or `task reopen`, also available via MCP `task_manage` with `action: "reset"` or `"reopen"`)
abandons ownership, increments claim revision, and marks the task ready. For in-progress tasks,
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

To achieve zero-shot agent orientation and eliminate exploratory tool-call loops, Forge aggregates task context by default upon MCP `task_claim` and CLI `task claim`, as well as through `task_manage(action: "context")` and CLI `task context <task-id>`. The MCP `task_claim` argument `bundle: false` returns minimal details; CLI `task claim` always returns the context bundle. The shared lifecycle formatter preserves all fields each mode supplies; see the [task lifecycle Markdown contract](mcp-markdown-contracts.md#task-lifecycle).

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
10. **`blackboard` and `blackboard_info`** (All stages): Up to 15 recent untagged messages and messages targeted to the active gate from the task, parent milestone, and sibling tasks. Each message includes origin metadata (`scope: "milestone" | "task" | "subtask" | "sibling"`), `task_id`, `subtask_ref`, and optional `gate`. `blackboard_info` summarizes the active gate, message ids, total count, targeted count, and topic counts. Gate-targeted messages also appear in `workflow_guidance.blackboard_messages`.

9. **`governing_invariants`** (contract, command, and review proof gates): Indexed documentation rules with at most five direct entries and five IDs found through one-hop callers or callees, omitted counts, and source references. This graph projection supplements `contract.invariants`; it never caps or replaces task authority, profile guidance, decisions, commands, snapshots, or proof receipts. The compatibility `adrs` field includes indexed ADR context only when a verified document or governing edge ties it to the task scope; matching a path token or prose word alone does not establish governance. Indexed `Decision:` lines remain available as source context without becoming accepted task authority.

### Response Bounding and Anti-Bloat Invariants

To guarantee responses remain well below the 96 KiB ceiling of [ADR 0011](../adr/0011-bounded-mcp-response-budgets.md):
- **Raw table pruning**: Raw SQLite dumps (`gates` test logs, `attempts`, `findings`, raw `spec`, and root `receipt`) are pruned from context responses, while `depends_on` is preserved in `contract` and at envelope root, and completion `receipt` is preserved inside `context_bundle` for completed tasks. Detailed historical archives remain accessible on demand via `task_manage(action: "inspect")` or CLI `task inspect <id>`.
- **Minimal claim mode (`bundle: false`)**: Delivers lean task metadata (`task_id`, `stage`, `status`, `allowed_write_scope`, `subtasks`, `depends_on`, `workflow_guidance`) strictly pruned of heavy `gates`, `attempts`, `findings`, `receipt`, and raw `spec` archives, ensuring minimal claim mode never exhausts response budgets.
- **Zero root duplication**: Substructures are scoped to `context_bundle` without duplicating `adrs`, `scope_symbols`, or `covering_tests` at root.
- **Bounded blackboard**: Aggregates the 15 most recent chronological messages; payloads exceeding 500 Unicode characters are safely truncated along character boundaries (use `task_blackboard(action: "inspect", message_id: <id>)` for full text).
- **Size budget**: Claim (including `bundle: false`), next, and context responses are bounded to 24 KiB of serialized JSON, leaving room for MCP text escaping and protocol metadata under the 96 KiB ceiling. Oversized responses omit whole fields, prioritizing claim identity and revisions. `byte_limited`, `omitted_fields` (up to 16 JSON pointers), `omitted_field_count`, and `retrieval_hint` identify incomplete context. Read the milestone file for full authority and CLI `task inspect <task-id>` for stored details before relying on omitted information. Stored contracts and evidence are unchanged.
- **Sync summaries**: Create and sync return compact task records with identity (including `spec.task_ref`), milestone reference, status, stage, revisions, and wave index. `total` counts all synchronized tasks, while `returned` counts the bounded prefix in `tasks`. `byte_limited: true` means additional summaries were omitted; all selected tasks were synchronized. Use `task_list` or CLI `task list` to retrieve task summaries.
- **Initialized SQLite stores**: Version 6 opens apply connection-local settings without DDL or write transactions, allowing task readers to open while another connection holds the writer lock. Version 5 migrates once inside `BEGIN IMMEDIATE`, creating `milestone_tombstones` and updating the version marker. New dedicated stores initialize version 6 in WAL mode; unsupported versions remain rejected.

### Context Bundle Example

Below is an illustrative payload returned upon claiming a task (`task_claim` or `task context <task-id>`):

```json
{
  "task_id": "forge-mcp/m-sample:language-feature",
  "status": "ready",
  "stage": "contract",
  "allowed_write_scope": ["src/engine/languages/html.rs", "tests/languages/html/syntax.rs"],
  "context_bundle": {
    "contract": {
      "task_id": "forge-mcp/m-sample:language-feature",
      "target": "Add HTML template island extraction",
      "stage": "contract",
      "status": "ready",
      "proof_policy": "seam-test-first",
      "allowed_scope": ["src/engine/languages/html.rs", "tests/languages/html/syntax.rs"],
      "subtasks": [
        {
          "subtask_ref": "wire-syntax",
          "title": "Parse embedded expressions",
          "status": "ready",
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
      "blackboard_messages": [
        {
          "id": 101,
          "gate": "contract",
          "topic": "draft",
          "payload": "HTML extractor misses template attributes; targeted seam in languages/html/syntax.rs."
        }
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
        "topic": "draft",
        "gate": "contract",
        "author": "builder-1",
        "payload": "HTML extractor misses template attributes; targeted seam in languages/html/syntax.rs."
      }
    ],
    "blackboard_info": {
      "active_gate": "contract",
      "message_count": 1,
      "targeted_count": 1,
      "topic_counts": {"draft": 1},
      "message_ids": [101]
    }
  }
}
```
