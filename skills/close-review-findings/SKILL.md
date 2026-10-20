---
name: close-review-findings
description: Diagnose and fix any defect, and classify, remediate, or evidence-close findings from Pi reviews, pull-request reviews, audits, or repeated review rounds. Use for every defect fix and when asked to analyze review findings, fix everything found, close a review, explain why findings reappeared, verify remediation completeness, or rerun review after fixes.
---

# Close Review Findings

## Repository Routing

Read the nearest repository `AGENTS.md` before using this skill. It selects the
owning review launcher, architecture mapper, required gates, and evidence store.
Load `review-authority` before classifying or repairing findings. That skill owns
the universal authority boundary and finding routing; this skill owns structural
attribution, invariant remediation, proof, and closure evidence.
After loading this base skill, load
`.agents/skills/close-review-findings/ADDON.md` when the repository routes to
one. The add-on may require more evidence or define local commands; it must not
weaken this skill's attribution, invariant, regression, or closure requirements.

Do not load a similarly named local `SKILL.md`: that can shadow this global base
skill and break composition.

**IMPORTANT: THE GOAL IS NOT TO CLOSE FINDINGS. THE GOAL IS TO PROVIDE A
COMPREHENSIVE FIX.**

A finding ID going quiet is not success. Success is one restored owner-cluster
invariant across every equivalent mutation, backend, caller, and persistence
seam — with Code Map / ast-grep / live call-site evidence and a mutation matrix
in chat. Prefer leaving findings `unresolved` over a premature
`confirmed-fixed`. Prefer STOPPING a review-loop clinic over another green
local patch. Closing the ledger without closing the invariant is a process
failure.

Convert an untrusted review ledger into verified invariant closures. Preserve raw
evidence, distinguish old defects from new regressions, and never equate green
package gates with complete remediation.

For every defect fix, complete the ordered closure map in section 2 before the
first production edit. If any boundary is missing, stop implementation and fill
it; a local symptom patch is not a fix. Apply sections 3 through 7 even when the
defect did not originate in a review.

## 1. Establish authority and scope

1. Read the nearest `AGENTS.md`, the implementation skill, the governing plan,
   and the review skill that produced the ledger.
2. Determine whether the request authorizes analysis only, implementation,
   repeated review, or lifecycle closure. Do not broaden one permission into the
   others.
3. Record the review ID, artifact directory, reviewed revision or worktree
   snapshot, review parameters, and current repository state.
4. Read the raw findings and reviewer evidence. Preserve malformed or unparsed
   output as unresolved evidence; never silently discard it or infer a clean
   result.
5. Require terminal review status before reporting a final ledger or closing the
   review. A live status snapshot is provisional.
6. Before reporting any count or result, read the current lifecycle status with
   its coverage and terminal report. A raw ledger may lead lifecycle state: if
   the phase is live, report only `provisional` raw count plus
   returned/required coverage; never call it the review result.

Do not treat reviewer wording, severity, reachability, proposed fixes, or claims
about causation as established facts.

### Authority stack (before any remediation edit)

Higher wins. Review never overrides a higher layer:

1. Explicit user decision in this session
2. Active frozen Contract (current non-superseded Plan subtasks + acceptance /
   writes / forbidden effects)
3. Named owning / deferred plans that still own the surface (e.g. a later
   telemetry cutover plan)
4. Review findings and proposed fixes

If a finding demands deleting, relocating, or weakening a symbol/path/API that
the active Contract still requires, or that a named deferred plan still owns
until a separate cutover: classify `invalid` or `plan-deferred` (disposition
`dismissed-with-evidence` / `plan-deferred-with-guard`). Do **not** implement
it. Say the finding is contract-conflicting or out-of-scope and keep the
invariant.

**Hard stop before delete/rename of a public or production path:** quote the
Contract clause or plan that still requires it, or get an explicit user
decision to change scope via `contract-subtask`. “Review said remove it” is
not authority.

## 2. Attribute every finding

Compare the reviewed snapshot, the pre-remediation snapshot, and current source.
Assign exactly one origin:

| Origin | Meaning |
|---|---|
| `introduced-regression` | The defective condition first appears in the remediation diff. |
| `pre-existing-missed` | The same condition existed before remediation but earlier review or tests missed it. |
| `incomplete-remediation` | The fix narrowed the original defect but left the owning invariant false. |
| `duplicate` | Another finding already represents the same reachable invariant failure. |
| `invalid` | The claim is unreachable, contradicted by source/runtime evidence, or based on an incorrect contract. |
| `plan-deferred` | The behavior is explicitly assigned to a later task and remains safely fail-closed. |

Prove attribution from source snapshots or history. Do not call every finding
reported after a fix a regression.

For each non-duplicate finding, record:

- exact claim and evidence;
- origin classification;
- reachability verdict;
- governing plan or contract clause;
- canonical owner;
- post-fix invariant;
- affected callers, modes, backends, authorities, and persistence boundaries;
- forbidden effects and compatibility constraints.

Before any remediation edit, write and verify this ordered map:

`reachable scenario → canonical owner → public/internal type → parser or compiled
form → persisted/runtime propagation → failure semantics → pre-fix regression
(or explicit not-warranted rationale for trivial maintenance) → mutation check →
live call sites`.

Also record one traceability row per cluster:

`plan/ACDD invariant → canonical owner → production consumer → persisted
evidence → boundary regression`.

Do not skip a boundary because the local symptom appears obvious. Do not make an
architecture decision independently. If the governing user decision or plan does
not already fix the owner, public/internal contract, wire or persisted form,
security, concurrency, migration, multi-backend, or compatibility semantics, stop
and request an explicit decision. Do not encode an assumption in code, tests, a
task, or an amendment.

Treat an isolated helper unit test as characterization only. Closure requires a
production-path regression through the real composition root and every stage the
invariant crosses. For multi-stage recovery or review workflows, prove
`resolution command → persisted recovery → confirmation → downstream consumer →
fresh worker or equivalent restart` when those stages exist. A local symptom
patch that leaves a second error boundary unset is `incomplete-remediation`.

## 3. Confirm reachability structurally

1. Orient to the canonical owner and its boundary before searching callers.
2. Write each ast-grep predicate and validate it against a known positive.
3. Enumerate syntax variants that can express the behavior: constructor, method,
   free function, alias, optional chain, adapter, and composition-root forms.
4. Run the validated predicates across production and tests.
5. Classify every match as owner, caller, adapter, composition root, test seam,
   or forbidden residual; inspect every production match.
6. Use the owning repository's declared impact mapper. Load `contextunity-forge`
   for its discovery workflow and follow the active tool schemas. Otherwise use the
   repository's supported structural analysis plus live call-site inspection.
7. Treat stale, missing, zero-node, or untracked required architecture-map
   results as missing evidence. Supplement them with validated structural search
   and live call-site inspection.

A zero-match result proves only the validated structural predicate. It does not
prove runtime, crash, filesystem, timing, authorization, or persistence safety.

## 4. Cluster by invariant

Group confirmed findings by canonical owner and post-fix invariant, not by file,
reviewer, or symptom. One cluster may contain multiple finding IDs.

Do not open a patch queue of N findings. Open at most one owner-cluster at a
time. For that cluster write exactly one post-fix invariant, then the full
reachable scenario across every owner it touches (for example
`recovery → scheduler`, `repair → capability`, `archive → restart`,
`apply → final persistence`, `source → built entrypoint`). A green helper suite
that never crosses those seams is not remediation evidence.

Before editing, enumerate adjacent dimensions relevant to the owner:

- alternate callers, adapters, build modes, and source execution;
- valid, missing, malformed, corrupt, legacy, and future-version input;
- per-item, aggregate, count, byte, time, retry, and pagination bounds; distinguish
  a page size from a total effect bound for one invocation;
- byte bounds before JSON serialization, canonicalization, and persistence, not
  only at text ingress;
- transaction boundaries, materialization, crash, restart, and reconciliation;
- filesystem topology: same-device and cross-device moves, failure after the
  first physical effect, retry convergence, symlink targets, and shared Git
  metadata;
- concurrency, interleavings, exact-time boundaries, and TOCTOU;
- authorization, fencing, revocation, lifecycle state, and ownership changes;
- cleanup, observer failure, logging failure, and error preservation;
- compatibility surfaces, backends, migrations, and fallback paths.
- release status of every persisted or wire format: internal-only, development,
  released, deprecated, or explicitly supported for migration.

Write the smallest sufficient remediation route from blast radius:

- direct repair for one low-risk owner;
- structured primary repair for one coherent cluster;
- full-cycle workflow for persistence, wire contracts, security, concurrency,
  migration, multi-backend, or multiple independent clusters.

Reject reviewer-proposed fixes that violate the plan or frozen Contract,
contradict a named deferred/owning plan, acknowledge failed work, weaken
fail-closed behavior, add speculative future scope, or repair only a local
symptom. Prefer adjudication over implementation when the finding fights
higher authority.

## 5. Prove the invariant before implementation

Add a focused regression only when it protects an observable invariant against
a plausible recurrence. Do not add a new test merely to assert a static
constant/allowlist entry, exact source shape, wording, or another implementation
detail; for trivial maintenance, record direct inspection and existing-gate
evidence instead. When a regression is warranted, make it fail before production
changes and name the plausible mutation it must kill.

Match evidence to the boundary:

- use real SQLite for transaction, corruption, migration, and durable authority;
- use concurrent processes for concurrency claims;
- kill a real process and reopen state for crash/restart claims;
- mutate filesystem components during the operation for TOCTOU claims;
- exercise compiled and source execution separately for entrypoint claims;
- exercise the production composition root for wiring claims;
- use malformed durable rows for fail-closed reader claims;
- test exact, below-limit, and over-limit cases for every bound;
- for recovery, test the full candidates × residuals × repair/normalizer outcome
  table; malformed residual bytes with no candidates must not become success;
- for merge or deduplication, state the canonical identity and prove that no
  distinct authoritative identifier is discarded without an explicit contract;
- assert filesystem cleanup against the production-resolved physical path or
  digest, not a logical path retained from an older layout;
- prove process termination by observing the real PID disappear, including a
  signal-resistant fixture when termination promises escalation;
- construct the exact prior supported released schema and prove a failed
  migration preserves both its columns and version; before first release,
  require one clean baseline instead of migrations between development layouts.

An injected exception in the same process does not prove crash convergence.
Sequential clients do not prove concurrency. Mocks do not prove production
composition. Happy paths and implementation-shaped snapshots are insufficient.

Reject or rewrite a regression that locks in a defective contract: it expects
behavior that the governing plan forbids, asserts compiler or snapshot shape
instead of the required invariant, or stays green when the owning invariant is
false. For TOCTOU or check-then-mutate claims, interleave the conflicting
mutation between the real check and the real mutate; creating state only after
commit does not model the race.

## 6. Repair the canonical owner

1. Implement the smallest change that restores the cluster invariant.
2. Put shared contracts in typed owner modules and preserve typed error and
   logging boundaries required by the repository.
3. Separate permissive public input types from fully normalized internal types;
   application ports consume the normalized type, never an optional-field input
   shape that has only been validated by convention.
4. Preserve primary and cleanup failures through typed causes or aggregates.
5. Avoid compatibility fallback or acknowledgment that converts unresolved work
   into apparent success.
6. Do not convert unreleased intermediate schemas into permanent compatibility
   contracts. Squash them into the initial baseline unless preservation was an
   explicit supported requirement.
7. Run the focused regression, when warranted, after each behavior change.
8. Refactor only after the invariant is green; a new behavior change starts a
   new Red cycle.

## 7. Perform closure checks

After local Green:

1. Rerun the identical ast-grep predicates as residual scans.
2. Run diff-impact and inspect every unexpected dependent.
3. Inspect the diff against every affected plan clause and forbidden effect.
4. Run mutation evidence or an equivalent deliberate one-line break.
5. Run the repository's focused and full required gates.
6. Compare current source with the pre-fix snapshot and answer:
   - Did the change create a new path?
   - Did it merely narrow the original vulnerability?
   - Which adjacent invariant could still fail?
7. Confirm or dismiss every raw finding individually.
8. Keep every unsupported dismissal, unresolved malformed output, missing
   runtime receipt, and untested dimension open.

Do not restore a plan task to `done` merely because tests pass.

## 8. Produce closure receipts

Create one closure receipt per invariant cluster:

```text
finding IDs:
origin:
disposition:
canonical owner:
post-fix invariant:
traceability: plan invariant → owner → production consumer → persisted evidence → boundary regression
affected paths and modes:
plan clauses:
regression and mutation evidence:
production-path evidence:
ast-grep residual evidence:
impact and dependent evidence:
focused and full gate receipts:
remaining risks or deferred behavior:
```

Use one disposition:

- `confirmed-fixed`;
- `dismissed-with-evidence`;
- `duplicate-of`;
- `plan-deferred-with-guard`;
- `unresolved`.

Close a cluster only when every source finding has a disposition and no
confirmed residual remains.

### Fail-closed stops (do not soft-skip)

These block `confirmed-fixed` and block launching the next review when the
repository's `AGENTS.md` or review contract requires the capability. Report the
gap and stop; do not substitute a weaker search for the missing evidence.

1. **Stale or failed required architecture map** — the configured impact mapper
   returns stale, missing, or zero-node results. Refresh or repair coverage
   first. Do not close while falling back only to live search.
2. **Missing mutation matrix** — for each owner-cluster (especially schema /
   catalog / RLS / index / FK / defaults / policies), the chat must list the
   equivalent mutations the regression must kill (e.g. name, definition,
   physical flags, defaults, policy attrs). One green example test is not
   enough for `confirmed-fixed`.
3. **Incomplete-remediation-dominant round** — if a later review’s unique
   findings are mostly `incomplete-remediation` on the same owners, do **not**
   start another review. Reopen those clusters, finish the matrix + residual
   path, or ask the user before any further patch/review wave.

Mandatory orientation before editing a cluster remains: the repository's
configured architecture mapper → structural search/call graph → live call sites
→ coverage path → mutation matrix → after edits: impact analysis + residual scan.

## 9. Repeat review without losing comparability

After a remediation wave that claims to close review findings, **launch a new
independent review** of the same scope before reporting closure. Local green,
closure receipts, or checklist edits are not a substitute. Skip only when the
user explicitly forbids another review round.

Reuse the same configured pool, repository scope, Overview, and review
parameters unless the user explicitly changes them. Record unavailable
reviewers and coverage degradation.

### Repeat-review admission gate

Do not start another review until one cumulative closure ledger proves all of
the following:

1. Every raw finding ID from every prior round in the lineage has exactly one
   evidence-backed origin and one disposition. A missing item remains open even
   when a later report omits it.
2. Findings are grouped into owner-clusters. Each cluster names one post-fix
   invariant and lists every cross-owner seam it crosses. Patch-per-finding
   ledgers are invalid admission evidence.
3. Each non-trivial canonical-owner cluster has at least one boundary regression
   that failed on the pre-fix state and a named mutation or equivalent deliberate
   break that the regression rejects. For trivial maintenance, record why a new
   regression would only lock in an implementation detail. Any regression must
   exercise the full reachable seam, not only the local helper that was patched.
4. A migration regression executes the next real canonical mutation against the
   migrated state; schema inspection or row reads alone are insufficient.
5. A codec or persisted-format regression inventories and exercises every
   production parser, reader, replay path, and importer that accepts that
   format, including malformed, non-canonical, oversized, and trailing input.
6. The residual legacy scan covers the complete declared repository surface:
   production source, tests outside conventional test directories, generated or
   rendered contracts, configuration, and public documentation. A scan limited
   to the edited owner subtree is not closure evidence.
7. The rerun binds one verified commit, one repository root, one unchanged scope,
   and the same review parameters, and explicitly compares its output with the
   cumulative ledger. Do not launch a closure review from a changing worktree.
8. Full expected reviewer coverage is present. An unavailable reviewer or scope
   drift makes the result `PARTIAL` and keeps closure open. Preserve malformed
   output and give it to the confirmation reviewer; it prevents `PASS` only
   when confirmation cannot establish complete scope and a complete ledger.
9. Package-gate green counts (`N/N` tests) are recorded only as supporting
   evidence. They never satisfy items 2–6 by themselves.

Record this admission evidence before launch. Publish the ledger path in the
same message that requests or starts the next review. A review invocation is
invalid when any item is absent; do not use the new review to discover whether
prior remediation was complete.

### Hard stop against review-loop clinic

Do **not** open `contract-subtask` + `contract-verify` for each finding. Bugs
inside the active write union: fix, note in the task conclusion, retest. New
Plan scope: propose once, then one append + one verify after user OK.

Stop remediation and stop launching reviews when any of the following is true:

- a later round mostly rediscovers `incomplete-remediation` on the same owners
  (**fail-closed stop #3** above — no “one more review”);
- Code Map is stale/failed or the cluster has no mutation matrix in-chat
  (**fail-closed stops #1–#2**);
- successive rounds add findings caused by the previous remediation patches
  without a new owner-cluster invariant;
- the agent proposes “one more review after local green” without an admission
  ledger.

In those cases report the open clusters, missing seam regressions, and the
governing plan clauses. Ask for an explicit user decision before another review
or another patch wave.

After terminal results:

1. Compare the new ledger with both the prior ledger and pre-fix snapshot.
2. Classify each new item by origin before editing.
3. Reopen the relevant invariant cluster for a true residual or regression.
4. Treat a pre-existing missed defect as a coverage failure in the earlier
   closure, not as a new regression.
5. When requested to update instructions retrospectively, add the missing
   reusable dimension to the nearest owning skill; do not append session history
   or product-specific detail to a workspace router.

Never claim closure from an active review, partial coverage, raw reviewer count,
or green local gates alone.
