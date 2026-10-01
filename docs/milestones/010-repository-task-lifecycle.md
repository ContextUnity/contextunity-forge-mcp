---
id: m-repository-task-lifecycle
title: "Repository-owned task lifecycle"
doc_type: contract
status: deferred
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
related_plans:
  - docs/plans/architecture-and-modularity.md
  - docs/plans/tool-performance-and-db-optimization.md
---

# Repository-owned task lifecycle

## Outcome and admission

Implement lightweight task coordination connecting Git milestone specifications,
isolated worktrees, verified delivery gates, and durable completion receipts.
This is a deferred target contract. Current registered interfaces remain described
in the [MCP reference](../reference/mcp-tools.md).

Activate after the owner accepts prerequisite tool readiness under the
[architecture plan](../plans/architecture-and-modularity.md) and
[performance plan](../plans/tool-performance-and-db-optimization.md), then admits
the concrete Rust implementation contract and proof. Verify current source before
choosing module boundaries. This file owns the planned task workflow.

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
task_ready(stage: "build" | "review" | null)
task_claim(task_id, stage, worker_id, worktree)
task_submit(task_id, stage, evidence_ref, action: "pass" | "reject", findings?)
task_manage(action: "create" | "inspect" | "delete" | "extend_scope",
            task_id?: string, milestone_ref?: string, task_ref?: string,
            paths?: list[string], force: bool = false)
```

> [!IMPORTANT]
> Invariant: The task MCP surface consists of these four tools. Rust validation
> and advertised schemas enforce flat typed fields and action-specific selectors.

Reject unknown fields and incompatible selector combinations:

| Action | Required fields | Additional constraints |
|---|---|---|
| create | Indexed milestone_ref and task_ref | Omit task_id/paths; force=false. |
| inspect | task_id | Omit milestone/task refs/paths; force=false. |
| delete | task_id or milestone_ref | Exactly one selector; omit task_ref/paths. |
| extend_scope | task_id and nonempty paths | Omit milestone/task refs; force=false. |

Create reads the indexed task block, mapping target to goal and scope to
allowed_write_scope. Stable identity is repository/project/milestone_id:task_ref.
Repeated create returns the same descriptor and status. Changed admitted
specifications require contract re-admission. After operational cleanup, completed
archived task blocks return their durable receipt; new execution uses a new task_ref.
Inspect returns retained attempts, gates, findings, and audit history.

## Worktrees, claims, and configuration

Configure the shared tasks.sqlite path explicitly in forge-mcp.yaml. Open absolute
paths directly; normalize relative paths from the config directory. Every worktree
instance points to the same absolute operational store. Keep this authoritative
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

These target specifications remain deferred with the milestone. Resolve concrete
module boundaries and evidence in design/contract before claiming build.

### task: specifications-and-store

```yaml
task_ref: specifications-and-store
target: "Реалізувати індексацію специфікацій та незалежне сховище задач"
proof_policy: seam-test-first
scope: [src/core/, src/db/, src/engine/, tests/]
```

Admit typed milestone/task parsing, qualified identities, per-task digests,
explicit database configuration, persistence/recovery, and idempotent creation.
Prove duplicate handling, malformed metadata rejection, sibling-task isolation,
and task-state survival across index rebuilds.

### task: task-tools-and-coordination

```yaml
task_ref: task-tools-and-coordination
target: "Реалізувати чотири MCP-тулзи, claims та розширення скоупу"
proof_policy: seam-test-first
scope: [src/mcp/, src/core/, src/db/, src/engine/, tests/]
```

Admit the flat schema, task envelopes, atomic claims/revisions, ready/inspect,
and minor scope extensions. Prove collisions, stale ownership, same-root sibling
paths, traversal rejection, and schema enforcement through public seams.

### task: gates-and-receipts

```yaml
task_ref: gates-and-receipts
target: "Реалізувати ворота здачі, незалежне рев'ю та синхронний handoff"
proof_policy: seam-test-first
scope: [src/mcp/, src/core/, src/db/, src/engine/, tests/]
```

Admit all five gates, evidence binding, review/remediation, contract invalidation,
and synchronous disk receipt validation. Prove immediate write-then-handoff,
mismatched proof rejection, and accepted completion persistence.

### task: cleanup-and-administration

```yaml
task_ref: cleanup-and-administration
target: "Реалізувати delete, TTL, адміністративний reset та CLI-міграції"
proof_policy: seam-test-first
scope: [src/cli/, src/mcp/, src/core/, src/db/, tests/]
```

Admit relational deletion, 14-day cleanup, outcome preservation, force semantics,
human CLI reset, and preview/apply/verify recovery. Prove boundary times, cascade
integrity, missing milestone cleanup, revision monotonicity, and import retries.

### task: pilot-acceptance-and-docs

```yaml
task_ref: pilot-acceptance-and-docs
target: "Перевірити пілот у worktree та узгодити документацію з реалізацією"
proof_policy: deferred-final-test
scope: [docs/, AGENTS.md, forge-mcp.yaml, tests/]
```

Exercise the complete lifecycle in independent worktrees, retrieval of milestone
sections, archival after final handoff, index filtering, and recovery. Align
current reference/agent guidance after capability admission. Record exact
commands, candidates, retrieval results, and lifecycle proof before activation.
