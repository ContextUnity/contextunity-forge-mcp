---
id: m-repository-task-lifecycle
title: "Repository-owned task lifecycle"
doc_type: contract
status: completed
depends_on: []
owners:
  - src/mcp/
  - src/cli/
  - src/core/
  - src/db/
  - src/engine/
  - tests/
invariants:
  - "INV-TASK-TOOLS: Exactly four task MCP tools expose flat typed arguments."
  - "INV-TASK-AUTHORITY: Git milestone files own task specifications."
  - "INV-TASK-RECEIPT: Accepted handoff requires a matching synchronously read receipt."
  - "INV-TASK-RETENTION: Completed operational tasks have a 14-day grace period."
  - "INV-TASK-MULTI-WORKSPACE: Linked workspaces with enabled tasks expose scoped task discovery, local AGENTS.md guidance, and repository-confined paths."
related_plans:
  - docs/plans/architecture-and-modularity.md
  - docs/plans/tool-performance-and-db-optimization.md
---

# Repository-owned task lifecycle

## Outcome and admission

Implement lightweight task coordination connecting Git milestone specifications,
isolated worktrees, verified delivery gates, and durable completion receipts.
The implementation is admitted by the October 2, 2026 owner request. Registered
interfaces are described in the [MCP reference](../reference/mcp-tools.md).

The owner request admits implementation and verification in the task-lifecycle
worktree. Final receipts and completed status wait for the owner's separate
commit command. This file owns the task workflow; the
[architecture plan](../plans/architecture-and-modularity.md) and
[performance plan](../plans/tool-performance-and-db-optimization.md) retain their
independent commitments.

## Git specifications and evolving commitments

Milestones exist as controlled Git Markdown. Forge reads and indexes their
frontmatter, headings, and task YAML blocks. Humans and authorized agents edit
these files directly. Existing document retrieval supplies milestone read access.

Use numeric filename prefixes for queue order and stable qualified IDs for
repository/project/milestone identity. Read status and dependencies before work.
Roadmap prose supplies macro context; milestone files supply commitments.

Plan 1-5 coherent feature-slice tasks per milestone. File counts and elapsed time
are planning estimates. A slice may span contracts, models, services, interfaces,
tests, and documentation. Admit discoveries serving the same goal and invariants
as additional uniquely identified task blocks in the active milestone.

> [!IMPORTANT]
> Invariant: Each task digest covers its own specification and governing
> constraints. Adding sibling tasks preserves existing contracts and proof;
> receipts and completion status are excluded from specification digests.

Reindex new task blocks and create operational tasks from the indexed specification.
Re-admit changed existing contracts. Close task admission before milestone completion.

Example: “До майлстоуна оптимізації мов додається задача покращення спільного
графа; чинні контракти задач Rust, Python і TypeScript залишаються чинними.”

## Four flat task tools

```text
task_list(milestone_ref?: string,
          status?: "ready" | "in_progress" | "blocked" | "completed" | "all",
          stage?: "build" | "review" | null,
          repository?: string)
task_claim(task_id, stage, worker_id, worktree)
task_submit(task_id, stage, evidence_ref, action: "pass" | "reject", findings?)
task_manage(action: "create" | "sync" | "inspect" | "delete" | "extend_scope",
            task_id?: string, milestone_ref?: string, task_ref?: string,
            paths?: list[string], force: bool = false, workspace?: string)
```

> [!IMPORTANT]
> Invariant: The task MCP surface consists of these four tools. Rust validation
> and advertised schemas enforce flat typed fields and action-specific selectors.

Reject unknown fields and incompatible selector combinations:

| Action | Required fields | Additional constraints |
|---|---|---|
| create | milestone_ref and task_ref | Optional workspace; omit task_id/paths; force=false. |
| sync | milestone_ref or workspace | Omit task_id/task_ref/paths; force=false. |
| inspect | task_id | Omit workspace/milestone/task refs/paths; force=false. |
| delete | task_id or milestone_ref | Exactly one selector; workspace only with milestone_ref; omit task_ref/paths. |
| extend_scope | task_id and nonempty paths | Omit workspace/milestone/task refs; force=false. |

Create synchronously reads the Git task block, mapping target to goal and scope to
allowed_write_scope. Stable identity is repository/project/milestone_id:task_ref.
Repeated create returns the same descriptor and status. Changed admitted
specifications require contract re-admission. After operational cleanup, completed
archived task blocks return their durable receipt; new execution uses a new task_ref.
Inspect returns retained attempts, gates, findings, and audit history.

## Worktrees, claims, and configuration

The tracked configuration uses `tasks_db: .forge/tasks.sqlite`; omission of the
key has the same default. Open absolute paths directly and normalize relative
paths from the config directory. Configure a shared path locally for agents
coordinating across worktrees. Keep this authoritative
store separate from rebuildable code indexes and define schema recovery.

Task readiness returns unclaimed nonterminal tasks with satisfied prerequisites.
Null stage includes design, contract, and handoff. Independent worktrees isolate
files; scope declarations guide implementation and review. Git merges branches.

Claim atomically records worker_id, worktree, claimed_at, and a monotonically
increasing claim_revision, setting in_progress. Claim collisions return
TASK_ALREADY_CLAIMED immediately. Different tasks may share file scopes in their
isolated worktrees. Persist revision counters across reset and task deletion.

Return a concentrated task envelope: goal, applicable invariants, scope,
proof_policy, contract revision, gates, source references, and claim revision.
Bind submit evidence to that revision, owner, worktree, and admitted candidate.
Successful submission, reset, deletion, and contract revision end old ownership.
Reject stale submissions. Identical submit retries return the accepted receipt.

Human-administered recovery uses CLI:

```sh
contextunity-forge-mcp task reset <task-id>
```

Clear worker/worktree, increment claim_revision, abandon the attempt, set the
current gate pending, preserve passed predecessors, and return the task ready.
The administrator stops the old worker before recovery. Reset invalidates its
submission authority; it leaves process management with the administrator.

Database migration utilities also belong to CLI:

```sh
contextunity-forge-mcp migrate preview
contextunity-forge-mcp migrate apply
contextunity-forge-mcp migrate verify
```

Preview reports conflicts, apply imports approved manifests idempotently, and
verify reconciles identities and evidence. Freeze schemas and error contracts
before implementation.

## Architecture admission and scope changes

Accepted ADRs govern global decisions; architecture pages describe current
topology; milestones specify concrete transitions. Applicable owner rules and
live source establish implementation authority.

During design/v1, inspect impact, callers, config, persistence, and tests through
Forge and exact source reads. Apply clear ADR/architecture rules autonomously and
cite the governing source. Escalate direct task/ADR contradictions and irreversible
database/DDL or data-loss choices through structured consequence-based questions.

During contract/v1, admit machine types/schemas and sensitive seam-test proof.
Behavior-changing tests demonstrate failure on the current implementation;
existing-behavior work uses controlled mutation or admitted sensitivity proof.
Documentation-only work uses static integrity proof. Start build after admission.

Minor adjustments use `task_manage` with action="extend_scope" before editing new paths.
Freeze normalized base roots from the original task scope: file entries use
`Path::parent`; directory entries root themselves. Deduplicate roots and preserve
them across extensions. Normalize paths and reject traversal and symlink escapes.
Require paths inside the same frozen module roots and atomically append unique
paths, returning the updated perimeter. A new sibling file remains admissible
when the original scope named a point file within that directory.

AUTHORITY_GAP covers schema changes, public contracts, governing invariant changes,
and ADR conflicts. Stop affected work, prepare an approved Git milestone amendment,
increase affected contract_revision, invalidate claim revisions, and reset build
and review pending. Invalidate obsolete handoff evidence and re-admit design/contract.
Ordinary agents refer ADR changes to the human decision owner.

Include stale architecture/runbook descriptions in the authorized scope, verify
their claims, repair metadata/links, and formalize invariant alerts during delivery.

## Gates, review, and durable handoff

Claim/submit cover design/v1 → contract/v1 → build/v1 → review/v1 → handoff/v1.
Build pass supplies the candidate commit and required test evidence, then yields
ready_for_review. Independent review evaluates five owner-admitted security
contours, recording applicability and evidence. Freeze those contours in the
implementation contract. Review pass yields ready_for_handoff; reject records
findings on the same task and returns it to remediation for another build/review.

After accepted review, write status: completed and a typed receipt inside the
task's own YAML block. Include tested implementation commit, passed_at,
contract_revision, test command/result/artifacts, review proof, and decision.
The receipt may be committed separately from the tested implementation.
Flush buffered output, close the file, and await successful write completion.

> [!IMPORTANT]
> Invariant: Handoff synchronously reads the milestone file in the claimed
> worktree using `std::fs::read_to_string` and parses the exact unique task block.
> Only a receipt matching accepted commit, contract revision, and test/review
> proof permits completed/completed_at in SQLite and starts the retention clock.

Malformed, missing, or mismatched receipts reject handoff. This synchronous path
reads the completed disk write independently of asynchronous indexing events.
Imported completion follows the same proof validation.

Keep receipts isolated under unique task headings. Merge independent task blocks
by identity and validate YAML and IDs; resolve conflicting same-task receipts
explicitly. Verify code integration after branch merge.

Keep the active milestone path until the final admitted task has passed handoff.
Every task must first reach completed in SQLite. Retain the accepted receipts
through cleanup and move the completed file to milestones/archive/ after final
membership closure. Durable receipts preserve earlier completed tasks after TTL.

## Retention, dependencies, and delete

> [!IMPORTANT]
> Invariant: Completed tasks remain operationally available for 14 days from
> completed_at. Automatic cleanup deletes only completed tasks older than 14 days.

Default ready queries omit terminal tasks. Canceled tasks support authorized
manual deletion; automatic canceled-task retention needs separate admission.
Pause cleanup during import/reconciliation.

Dependencies between task IDs stay within one milestone. External dependencies
reference milestone outcomes such as m-XX:completed or public artifacts; use
qualified identities across projects. Materialize satisfied local dependency
outcomes before removing prerequisite rows.

Use task_manage(action="delete", task_id="...") for one task and milestone_ref
for a batch. Standard deletion requires terminal state, no active claims,
dependency integrity, and age greater than 14 days for completed tasks. Batch
eligibility checks every target. State/time/dependency checks are relational;
SQLite deletion performs no Git inspection.

Explicit user-authorized force=true bypasses state, age, milestone-presence, and
claim eligibility for the identified target. Revoke claims and preserve satisfied
dependency outcomes; unresolved removed prerequisites block retained dependents.
Project isolation remains mandatory. Cascade operational rows and graph links
transactionally; return deleted IDs/count, revoked claims, and mode.

Missing milestone authority blocks open work until restored or re-admitted.
Retain IDs for relational cleanup of closed tasks. External artifacts and Git
receipts have their own retention policies. Index rebuild preserves task state.

## Implementation task specifications

The owner admits these feature slices. The shared task service lives in
`src/engine/tasks.rs`; MCP and CLI adapt its typed arguments. Specifications and
receipt models live in `src/core/tasks/`, while `src/db/tasks_store.rs` owns the
independent WAL database. Proof modules extend the existing `core_basics` and
`mcp_context` integration suites.

`task_list` defaults strictly to ready. Evidence references are worktree-relative
YAML or JSON files binding task ID, versioned gate, claim/contract revisions,
worker, canonical worktree, implementation commit, and proof. Accepted retries
return the stored result. An amended task specifies a larger `contract_revision`
before sync resets its gates and revokes ownership. Frozen scope roots remain
unchanged by ordinary extensions. Use the [task reference](../reference/tasks.md)
for wire fields, receipt shape, and administration.

The implementation admits five independent-review security contours: path and
scope confinement, claim fencing, transactional concurrency, project isolation,
and administrative recovery/deletion. Review proof records applicability and
evidence for `paths`, `claims`, `concurrency`, `project_isolation`, and
`administration` before handoff.

### task: specifications-and-store

```yaml
task_ref: specifications-and-store
target: "Implement specification indexing and independent task storage"
proof_policy: seam-test-first
scope: [src/core/, src/db/, src/engine/, tests/]
status: completed
```

Admit typed milestone/task parsing, qualified identities, per-task digests,
explicit database configuration, persistence/recovery, and idempotent creation.
Prove duplicate handling, malformed metadata rejection, sibling-task isolation,
and task-state survival across index rebuilds.

### task: task-tools-and-coordination

```yaml
task_ref: task-tools-and-coordination
target: "Implement four MCP tools, claims, and scope extension"
proof_policy: seam-test-first
scope: [src/mcp/, src/cli/, src/core/, src/db/, src/engine/, tests/]
status: completed
```

Admit the flat schema, task envelopes, atomic claims/revisions, ready/inspect,
and minor scope extensions. Prove collisions, stale ownership, same-root sibling
paths, traversal rejection, and schema enforcement through public seams.

### task: gates-and-receipts

```yaml
task_ref: gates-and-receipts
target: "Implement completion gates, independent review, and synchronous handoff"
proof_policy: seam-test-first
scope: [src/mcp/, src/core/, src/db/, src/engine/, tests/]
status: completed
```

Admit all five gates, evidence binding, review/remediation, contract invalidation,
and synchronous disk receipt validation. Prove immediate write-then-handoff,
mismatched proof rejection, and accepted completion persistence.

### task: cleanup-and-administration

```yaml
task_ref: cleanup-and-administration
target: "Implement delete, TTL retention, administrative reset, and CLI migrations"
proof_policy: seam-test-first
scope: [src/cli/, src/mcp/, src/core/, src/db/, tests/]
status: completed
```

Admit relational deletion, 14-day cleanup, outcome preservation, force semantics,
human CLI reset, and preview/apply/verify recovery. Prove boundary times, cascade
integrity, missing milestone cleanup, revision monotonicity, and import retries.

### task: pilot-acceptance-and-docs

```yaml
task_ref: pilot-acceptance-and-docs
target: "Verify worktree pilot and align documentation with implementation"
proof_policy: deferred-final-test
scope: [docs/, AGENTS.md, forge-mcp.yaml, tests/]
status: completed
```

Exercise the complete lifecycle in independent worktrees, retrieval of milestone
sections, archival after final handoff, index filtering, and recovery. Align
current reference/agent guidance after capability admission. Record exact
commands, candidates, retrieval results, and lifecycle proof before activation.

### task: multi-workspace-and-linked-repository-tasks

```yaml
task_ref: multi-workspace-and-linked-repository-tasks
target: "Support multi-repository tasks (linked_workspaces), local AGENTS.md, CRLF normalization, and relative tasks_db"
proof_policy: seam-test-first
scope:
  - src/engine/tasks.rs
  - src/engine/tasks/
  - src/cli/
  - src/mcp/tools.rs
  - src/db/tasks_store.rs
  - src/mcp/tasks.rs
  - src/core/tasks/
  - forge-mcp.yaml
  - docs/
  - tests/
status: completed
```

1. **Configuration & Resilience**:
   - Update `forge-mcp.yaml` to use relative `tasks_db: .forge/tasks.sqlite` and provide a fallback default in `TaskSettings`.
   - Normalize CRLF line endings (`\r\n` -> `\n`) in `Milestone::parse` to prevent parsing failures on Windows / mixed checkouts.
   - Resolve repository identity dynamically from configuration instead of hardcoding `"forge-mcp"`.
2. **Linked Workspaces Task Discovery**:
   - In `forge-mcp.yaml`, allow `linked_workspaces` entries to configure `tasks: { enabled: bool, milestones_dir?: string, agents_md?: string }`.
   - By default, `task_list` returns only tasks for the primary repository. Supplying `repository: "<name>"` or `repository: "all"` queries linked workspaces.
   - Graceful handling: linked workspaces without a `docs/milestones` directory or with `tasks.enabled: false` are safely ignored without errors.
3. **Local Guidance & Scope Confinement**:
   - `task inspect` and `task claim` return `workspace_root` and `agents_guidance` pointing to the task's repository-local `AGENTS.md` (e.g. `traverse/AGENTS.md`).
   - Confine `allowed_write_scope` and `extend_scope` to the root of the task's owning repository.

## Implementation verification and pending acceptance

The implementation remains uncommitted in `.worktrees/task-lifecycle` on branch
`task-lifecycle`, based on `7ca68c316f53f98550c825571f85604ce4527160`.
That base identifies the comparison point; it is not an implementation receipt.

The public integration suites exercise specification parsing, independent WAL
storage surviving a code-index rebuild, default-ready filtering, concurrent
claims, revision fencing, confined scope extension, independent review,
remediation, synchronous receipt validation, retention boundaries, cascading
deletion, project isolation, reset, and idempotent CLI migration. The stdio
lifecycle test uses separate builder and reviewer worktrees and completes all
five gates with fixture evidence. Fixture commit values are test inputs.

The initial live worktree pilot synchronized the first five admitted tasks and exercised
`task list`, `task claim`, `task inspect`, and `task reset`. Reset cleared the
pilot owner and returned the claimed task to ready with a higher revision.
Forge MCP retrieved this milestone's implementation specification and the task
reference's gates/evidence section from the correct worktree; graph metadata
reported zero stored parse errors. Changed documentation has valid frontmatter
keys and resolvable relative file links.

Initial implementation verification commands and local logs:

- `cargo test --all-targets`: 409 passed, 0 failed, 3 ignored;
  `/tmp/forge-task-lifecycle-final-tests.log`.
- `cargo test --test commitment_integrity`: 12 passed;
  `/tmp/forge-task-lifecycle-final-commitment.log`.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed without warnings;
  `/tmp/forge-task-lifecycle-final-clippy.log`.
- `git diff --check`: clean.

The owner separately authorized the existing Vue resolution assertion to select
the call on line 2; its previous failure is reproducible on the base commit.

Final acceptance stays open. A tested implementation commit, actual independent
review evidence, durable per-task receipts, live handoffs, and milestone archival
await the owner's separate commit command. The milestone remains in_progress.

## Review repair verification

The owner-reported configuration, identity, CRLF, and worktree diagnostics
findings are reproducible conditions in the initial implementation. The tracked
configuration uses `.forge/tasks.sqlite`, and `TaskSettings` supplies that path
when the key is omitted. Configured identity governs create/sync and migration;
persisted qualified identity governs authority and handoff across worktrees.
`Milestone::parse` normalizes CRLF before reading frontmatter and task blocks.
Service and store claims validate an existing directory and report
`WORKTREE_NOT_FOUND` before recording ownership.

Public-seam tests demonstrate the default path, LF/CRLF digest equality, a failed
missing-worktree claim preserving ready/unclaimed state, and all five MCP gates
with omitted repository frontmatter, the `contextunity` namespace, independent
builder/reviewer worktrees, CLI migration, and a CRLF completion receipt.
The added regression failed on the initial implementation with missing tasks_db.

Current settled source/test checks:

- `cargo test --all-targets`: 410 passed, 0 failed, 3 ignored;
  `/tmp/forge-task-review-all-targets.log`.
- `cargo test --test commitment_integrity`: 12 passed;
  `/tmp/forge-task-review-commitment.log`.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed without warnings;
  `/tmp/forge-task-review-clippy.log`.
- Task integration modules: 13 passed; `/tmp/forge-task-review-focused.log`.

The added multi-workspace invariant changes the governing digests of the five
previously synchronized tasks. Their live listing remains blocked pending contract
readmission. The sixth task is registered by the linked-workspace pilot below.
The repair preserves the amendments and existing revisions of the first five tasks.

Independent read-only review found no residual defect within these four repairs.
Its coverage combines source inspection of all production parsing and claim
entrypoints with the recorded public-seam tests. The missing-worktree test
exercises the service; direct-store and non-directory handling were inspected
in source. The reviewer worktree fixture has no local configuration. This review
does not accept the sixth task or supply the milestone's final delivery review.

## Linked repository task verification

The owner request admits `multi-workspace-and-linked-repository-tasks`, including
CLI parity and the shared workspace registry in `src/engine/tasks/workspaces.rs`.
The task remains in_progress pending a tested implementation commit and final
receipt acceptance. The [task reference](../reference/tasks.md#linked-repository-tasks)
defines its registered configuration and selectors.

Linked task configuration is opt-in. Synchronization selects a workspace by name,
reads one file or its numbered milestone directory, and writes only that task
namespace in the primary store. Missing/empty directories return zero tasks.
List retains primary-ready defaults and adds explicit repository/all selection.
Inspect and claim carry owner root, local guidance, and milestone invariants.
ID-based operations route to the owning namespace; scope validation checks its root
and the task's claimed isolated worktree. Scope traversal, symlink escape, and
claiming from another configured repository fail closed.

The core integration proof covers shared storage, opt-in filtering, missing and
empty directories, default/custom task identity, custom guidance, scope extension,
and cross-repository boundaries. The MCP/CLI proof covers workspace directory and
file sync, repository selection parity, guidance, all five linked-task gates,
CRLF receipt acceptance, and completion surviving a server restart.

Settled source/test checks:

- `cargo test --test core_basics -- tasks`: 13 passed;
  `/tmp/forge-linked-core.log`.
- `cargo test --test mcp_context -- tasks`: 4 passed;
  `/tmp/forge-linked-mcp.log`.
- `cargo test --test commitment_integrity`: 12 passed;
  `/tmp/forge-linked-commitment.log`.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed without warnings;
  `/tmp/forge-linked-clippy.log`.
- `cargo test --all-targets`: 414 passed, 0 failed, 3 ignored;
  `/tmp/forge-linked-all-targets.log`.

The live worktree pilot created this task, claimed design, inspected its local
root/guidance, and reset ownership with a higher revision. The task is registered
and operationally ready after the pilot. The first five tasks still require
readmission for the added governing invariant; their revisions remain unchanged.
The commerce-release-update configuration was inspected read-only: traverse and
gridviewspec have no task opt-in yet. Their configuration and stores were not
modified. Code scanner, linker, writer, and commitment owners remain unchanged.

Independent review identified owner-root validation during claimed extension,
nested repository perimeters, and omitted-project defaults as remaining gaps.
All three reproduced in public-seam tests before remediation:
`/tmp/forge-linked-review-red.log`. The final implementation validates scope in
both owner and claimed roots, checks configured repository boundaries during
admission/list/claim/submit/extension, and carries repository/project defaults
through sync, authority, handoff, and migration.

The closure matrix covers owner-only and worktree-only symlink escapes with valid
local extensions; foreign nested extension and broad parent directory rejection
with successful child-owned claims; custom local project identity with default
project counterparts, CLI migration, and all lifecycle gates. The child root is
excluded from parent task scopes, while child-owned claims remain valid inside
that root. Nested repositories retain distinct owners and write perimeters.

Independent re-review of the settled repair found no remaining reachable defects
in this task's admitted scope and confirmed the three fixes. It verified exact
source/callsite evidence and the pre-fix failure log; execution results are supplied
by the production-seam and full checks recorded above. This static review and
fixture evidence do not supply the tested commit and live receipts required for
milestone completion.
