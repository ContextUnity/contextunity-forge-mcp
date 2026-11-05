---
title: "ACDD Gate Engine Bugs and Coverage Audit"
doc_type: plan
status: audit
date: 2026-11-03
---

# ACDD Gate Engine Bugs and Coverage Audit

## Audit authority, scope, and evidence

> [!IMPORTANT]
> **Historical Disposition & Current Authority**
> This document is a historical audit artifact capturing snapshot evidence at commit `12d8df87516b5c2c57ba3d0348cfd138dc30c5af`.
> - **Current Implementation Authority**: Governed by [ADR 0020](../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md), [ADR 0021](../adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md), and [ACDD Lifecycle Segregation Plan](acdd-lifecycle-segregation-fallback-removal-and-verification.md).
> - **Architectural Resolution Status**:
>   - *Reopened or still incomplete*: F01–F04, F06, F07, F10–F20, F22–F24, F31–F32. Round 4 found that partial repairs do not prove the full boundaries; the bindings below identify the current findings.
>   - *Verified boundaries*: literal, NUL-delimited scoped index preservation (F26); rejection of a missing or mismatched seam path; live conditional skip selection; and validation of the 18 profile examples. These do not establish the surrounding lifecycle as complete.
>   - *Superseded*: Proposed synthetic `decision_required` / rigid `TASK_DECISION_MISSING` SQLite gates are superseded by ADR 0020 universal typed transitions (`when`, `pass`, `fail`, `loop`) and red seam verification. Blackboard recording operates as cognitive agent guidance, not an artificially enforced blocking SQLite gate.
>   - *Planned for AC-003*: Headless orchestrator runtime, dedicated native CLI commands for Workflows and Plans (`workflow run`, `plan run`).

### Round 4 evidence bindings

The Sol Round 4 report (`/tmp/forge-round4-audit-1793786772/ROUND-4-REPORT.md`) audited the worktree at HEAD `12d8df87516b5c2c57ba3d0348cfd138dc30c5af`; its evidence applies to that snapshot, not a later repair. Reopenings remain pending fresh production-seam proof:

| Original findings | Round 4 binding |
| --- | --- |
| F01–F04, F06, F07 | R4-05, R4-07, R4-15: root execution, authority, tracks, templates, and remaining compatibility paths. F02's native Workflow and Plan execution is planned AC-003 work. |
| F10–F14, F31 | R4-07–R4-09 and R4-14: graph identities, candidate provenance, conditional recovery, and rejection routing. |
| F15–F20, F24 | R4-03, R4-05, R4-06, R4-15, R4-19 and R4-20: verification authority, transitions, recovery, and proof provenance. |
| F22–F23 | R4-01–R4-02: typed command and contract proofs can bypass observed execution. |
| F32 | R4-01–R4-20: passing existing checks do not cover the reproduced negative cases. |

The result and reproductions below are the original snapshot, not current closure claims. F21's proposed blocking decision-table gate remains superseded.

This is an audit and proposed repair specification. No production code, profiles, tests, Git history, or task authority is changed by this document. Findings describe the inspected working tree, including pre-existing uncommitted changes, at HEAD `12d8df87516b5c2c57ba3d0348cfd138dc30c5af`. Paths and line numbers refer to that working-tree snapshot. The corresponding modules now live under `crates/forge-tasks/src/tasks/profile/`, `crates/forge-tasks/src/engine/gates/`, `crates/forge-tasks/src/milestones/handoff/`, and `crates/forge-tasks/src/store/recovery/`.

The governing sources are [ADR 0020](../adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md), [ADR 0021](../adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md), the [lifecycle segregation and verification plan](acdd-lifecycle-segregation-fallback-removal-and-verification.md), and the official [Workflow](../../skills/contextunity-forge/references/acdd-profile-examples/profile-workflow.example.yaml), [Plan](../../skills/contextunity-forge/references/acdd-profile-examples/profile-plan.example.yaml), and [Milestone](../../skills/contextunity-forge/references/acdd-profile-examples/profile-milestone.example.yaml) profiles. ADR 0021 supplies the later model-separation and fallback-removal decision. Architecture and ADR indexes, repository/source/test owner instructions, [TESTS.md](../../TESTS.md), roadmap, and milestone guidance supply the surrounding boundaries.

The audit follows parsing and normalization through admission, cursor selection, claim/next, command execution, proof compilation, typed submission, pass/reject, snapshot/commit, receipt production, recovery, milestone verification, and handoff. Source searches use Forge structural discovery and AST search, followed by direct source reads. The code map has incomplete coverage, so negative graph results are not treated as proof of absence. Supporting storage, Git, command, subtask, blackboard, and verification modules are included where they affect those paths. Tests under `tests/acdd` are inspected as evidence of behavior, not as substitutes for runtime source inspection.

Severity means: **P0** permits acceptance without required proof or damages user state; **P1** blocks an official lifecycle, breaks transition/recovery integrity, or omits a required guard; **P2** weakens diagnostics, portability, or coverage. “Reproduced” means an executed test/probe demonstrated the result. “Source-confirmed” means the cited production path establishes the defect. “Support gap” means admitted architecture has no complete runtime implementation; it is not represented as a regression in a previously supported model.

## Result

The current engine does not cleanly implement any of the three official models end to end. The defects share a root cause: profile validation, task flattening, store indexing, recovery, receipts, and milestone verification use different definitions of an executable gate and different transition rules.

| Model | Required behavior | Current result |
| --- | --- | --- |
| Workflow | Root gate graph; no task IDs, task rows, or manifests; default persistence does nothing | Official example fails validation. Removing its forward rejection still fails the terminal-delivery rule. Public execution remains task/milestone-bound. |
| Plan | Only `tasks:`; `doc_type: plan`; in-place execution; snapshots; no automatic commits or Level 0 gates | Official profile parses, but inherits hidden milestone verification. Plan-native admission/execution is absent, and snapshot delivery retains the legacy automatic-commit flag. |
| Milestone | Level 0 synthesis once, task DAG, Level 0 engine verification, ordinary rejection to polish, independent review and handoff | Official example fails validation. Root gates leak into tasks; skipped snapshots break delivery; verification remains a separate fallback machine and can be bypassed by the external-evidence handoff path. |

There are **32 findings** below. The highest-priority repairs are the external verification bypass (F18), unobserved mechanical proof acceptance (F22), and unrelated-index destruction (F26). Fixing only the root-flattening loop does not repair the other lifecycle consumers.

## Executed verification and reproductions

| Command/check | Observed result |
| --- | --- |
| `cargo test --package forge-tasks --lib` | Exit 0; **15 passed, 0 failed**. |
| `cargo test --test acdd -- --list` | Exit 0; **167 tests** discovered. |
| `cargo test --test acdd acdd::gates_ast:: -- --nocapture` | Exit 0; **5 passed, 0 failed**. |
| `cargo test --test acdd acdd::milestones::milestone_verification -- --nocapture` | Exit 0; **3 passed, 0 failed**; these assert the existing fallback model. |
| `cargo test --test acdd` | Exit 101; **166 passed, 1 failed**, 0 ignored. Failure: `acdd::tasks::workspaces::scoped_commit_commits_staged_deletion_and_preserves_unrelated_staged_work`. |
| Isolated rerun of that failing test with `--exact --nocapture` | Exit 101; **0 passed, 1 failed**. Assertion at `tests/acdd/tasks/workspaces.rs:3550`: `real index must retain unrelated staged README.md: ?? README.md`. |
| Current built CLI validates official Workflow profile | Invalid: `gate 'verify' reject_to must reference a strictly prior gate in gates`. |
| Current built CLI validates official Plan profile | Valid: one root entry, three executable leaves; the effective profile still has inherited milestone verification. |
| Current built CLI validates official Milestone profile | Invalid: `gates must contain one terminal delivery gate when delivery is declared`. |

A disposable Rust harness in `/tmp` linked against the current `forge-tasks` library and exercised public task admission/claim/submission against real temporary Git workspaces. It did not modify repository source. Its executed results are:

```text
root-leak admitted-stage="contract_synthesis"
root-leak-claim: OK String("contract_synthesis")
multiple-tracks-claim: ERROR TASK_GATE_CURSOR_AMBIGUOUS ... 'build' is not unique
first-false-condition-claim: ERROR TASK_STAGE_INVALID ... 'build' ... active stage 'docs'
skipped-seam-next="deliver"
skipped-seam-delivery: ERROR TASK_SNAPSHOT_NOT_FOUND ... no accepted candidate snapshot
unobserved-contract-with-exit-2: OK String("build")
unobserved-command-actual-exit-9: OK String("deliver")
workflow-without-reject-edge: ERROR ... one terminal delivery gate
```

The contract probe declared expected exit 101 and `assertion failed`, supplied exit 2 with a nonexistent seam reference, posted no architectural decision, and still advanced. The command probe registered a command that exits 9, supplied passing JSON without executing it, and still advanced. These prove acceptance defects; they do not claim that the stricter zero-argument proof compiler has the same bypass internally. The bypass is the alternative typed-submission path.

Later, an expanded harness invocation was blocked by the shell tool allowlist and was not executed. No results are attributed to those additional cases. Strict Clippy, commitment-integrity tests, and the full all-targets repository suite were not run as part of this documentation-only audit. The ACDD result above is not a passing repository verification claim.

## Gate and transition coverage ledger

| Surface | Inspected owner and branches | Findings |
| --- | --- | --- |
| Profile | `tasks/profile.rs`: schema, defaults, merge, normalization, executable leaves, hierarchical/legacy validation, references, conditions, templates, commands, persistence | F01, F04–F08, F13, F17, F20, F24, F31 |
| Task admission | `tasks/mod.rs`, `engine/lifecycle.rs`, `engine/workspaces.rs`, `store/mod.rs`: manifest authority, pins, first/completed stage, task shape and scope | F02–F04, F07, F09, F15, F29 |
| Claim and dispatch | `engine/claim.rs`, `engine/context.rs`, `engine/lifecycle.rs`, `store/lifecycle.rs`: explicit/implicit stage, aliases, conditions, independence, next, park/resume/reset | F01, F08–F09, F13–F14, F24–F25, F27, F29–F30 |
| Run and verify | `milestones/command_gate.rs`, `engine/submit.rs`, `engine/scheduler.rs`, `verification.rs`: scoped command plan, timeout/spawn/error, traces, proof compilation | F08, F16, F19–F23, F29 |
| Submit/pass | `engine/mod.rs`, `engine/gates.rs`, `tasks/gates.rs`: claim CAS, idempotence, proof kind/schema, snapshots, scope/signatures, receipts, stage advance | F01, F08–F11, F15, F21–F28, F31 |
| Reject | `engine/gates.rs`, `engine/submit.rs`, `engine/workspaces.rs`, `store/recovery.rs`: default destination, scope/symbol overrides, findings, worker reuse, replay | F05–F06, F12–F16, F28 |
| Delivery | `engine/git_snapshot.rs`, `git.rs`, `engine/gates.rs`, `store/mod.rs`, `store/subtasks.rs`: candidate, immutable review, subtask readiness, Git/index rollback, receipt | F10–F11, F24–F27, F29, F31 |
| Recovery | `store/recovery.rs`, `store/trace.rs`, `store/schema.rs`, `milestones/recovery.rs`: transaction journal, restoration, checkpoint/transition, execution attempts, migrations | F09–F10, F12–F14, F21, F25, F27–F28 |
| Blackboard | `blackboard.rs`, `engine/blackboard.rs`, `store/blackboard.rs`: task/milestone scope, topics, recipients, status, decision use | F03, F21 |
| Milestone | `milestones/mod.rs`, `synthesis.rs`, `contract.rs`, `handoff.rs`, `integration.rs`: initialization, DAG/matrix, verification, repair/review/delivery, binding, archive/retry/prune | F02–F06, F10, F17–F20, F27–F30 |
| Tests | Every `tests/acdd` module, including task domain modules, parser/runner/gate/milestone/blackboard/persistence/MCP/support fixtures | F32 and the per-finding requirements below |

Existing claim-revision checks, contract-revision checks, immutable candidate validation, scoped candidate commits, receipt binding, authority/path confinement, and recovery journals are valuable safeguards. Retain them while fixing the inconsistent lifecycle interpretation. No finding authorizes relaxing those boundaries.

## Findings

### F01 — P1: Root gates are still executed as task gates

**Reproduced.** `collect_cursors` (`crates/forge-tasks/src/engine/claim.rs#L23`), lines 78–98, walks both `gate.gates` and `gate.tasks.gates`, then turns a root leaf into a task cursor. The inner collector repeats the mixed traversal at lines 37–40. A root `contract_synthesis` therefore becomes the first stage of every admitted task. A root post-verification graph would also leak if validation admitted it. Profiles without `tasks:` are treated as a task pipeline rather than a distinct execution subject.

`GateProfile::executable_gates` (`crates/forge-tasks/src/tasks/profile.rs#L1121`) independently repeats the same semantic flattening. Updating only the engine collector leaves store admission, lookup, recovery, and receipt code inconsistent.

**Required repair:** task operations select an admitted task track and extract only its `tasks.gates`. Root gates belong to a Workflow or Milestone subject. Task operations on a Workflow fail before admission/claim with a model-specific diagnostic. Route every consumer through the same compiled gate projection. See D1 and D2.

**Proof:** assert task claim/run/submit never visits either Level 0 boundary; separately execute each boundary exactly once per milestone authority revision. Cover no `tasks:`, an empty block, and multiple blocks explicitly.

### F02 — P1: Workflow and Plan lack complete execution subjects

**Support gap.** Public operations in `engine/lifecycle.rs` (`crates/forge-tasks/src/engine/lifecycle.rs`) operate on task IDs and milestone manifests. `validate_authority` (`crates/forge-tasks/src/engine/mod.rs#L881`) expects milestone authority. Sync at `engine/lifecycle.rs:731–788` parses that authority. `init` (`crates/forge-tasks/src/milestones/mod.rs#L324`) converts plan material into a milestone contract rather than admitting a `doc_type: plan` lifecycle directly. There is no complete manifest-free Workflow runner or in-place Plan runner exposed by this crate.

**Required repair:** introduce subject selection around the existing gate engine, not fake tasks for Workflow and not automatic plan-to-milestone conversion. Workflow may use generic execution/checkpoint storage but creates zero rows in task tables. Plan admission accepts plan authority and binds steps to its task container, without synthesis, milestone activation, isolated worktree requirements, archival, or handoff. ADR 0020 described some orchestration as future work; ADR 0021 now makes these first-class models. Report this as missing implementation rather than pretending parser acceptance supplies it.

**Proof:** public API plus CLI/MCP lifecycle tests for all three models; assert database row counts and Git HEAD behavior, not just JSON shape.

### F03 — P1: Milestone preflight does not execute or enforce the Level 0 contract gate

**Source-confirmed.** `preflight` (`crates/forge-tasks/src/milestones/synthesis.rs#L224`), lines 224–374, validates repository skills, DAG/matrix information, and task admission, and posts initialization context. It does not claim and accept the root `contract_synthesis` gate with its runtime-red proof and architectural decision. Task claims check task authority without requiring accepted Level 0 synthesis.

Removing F01 without adding the correct milestone execution path removes the accidental per-task synthesis but leaves no actual synthesis gate. The plan's `synthesis_review` diagram is also not present in the supplied Milestone example; do not silently invent that extra gate.

**Required repair:** persist the root cursor on milestone initialization; execute declared pre-task gates once, validate their proofs, and unlock task waves only afterward. Preserve the existing structural preflight checks as prerequisites, not proof substitutes. Revisions invalidate the relevant boundary proof. Only execute a synthesis review when the admitted profile declares it.

**Proof:** root synthesis rejection, missing decision, failed red proof, duplicate initialization, concurrent starts, restart, and stale-authority cases must leave tasks locked.

### F04 — P1: Two milestone profile authorities and implicit inheritance contaminate models

**Reproduced/source-confirmed.** `GateProfile` (`crates/forge-tasks/src/tasks/profile.rs#L1079`) has a separate top-level `milestone_verification` field. Parsing at lines 2191–2206 also accepts a compatibility container through policy settings. The new official example puts the container inside `gates`, but `crates/forge-tasks/src/milestones/handoff.rs#L1055` reads the separate field. These are different execution authorities.

Profile merge at `profile.rs:2021–2084` retains the embedded verification container when an override omits it. `merge_profile_document` (`crates/forge-tasks/src/tasks/profile.rs#L2277`) only clears it through a special empty-command path. The official Plan thus inherits milestone fallback behavior it never declared. Embedded default gates at `acdd.default.yaml:289` (`crates/forge-tasks/src/tasks/acdd.default.yaml#L289`) are flat task gates; ADR 0021 classifies a profile without `tasks:` as Workflow.

**Required repair:** use one root graph as execution authority. Explicit `gates:` replaces inherited execution topology; merge role/command/guidance catalogs separately. Remove the top-level verification and policy-settings execution side channels. Update the embedded default to an explicit task container/model-consistent graph. Reject obsolete execution shapes with an actionable diagnostic rather than silently keeping both meanings.

**Proof:** validate and resolve each official example against embedded defaults; inspect the effective topology. Plan and Workflow must contain no inherited milestone gate.

### F05 — P1: Hierarchical validation rejects the official Workflow and Milestone

**Reproduced.** `validate_hierarchical` (`crates/forge-tasks/src/tasks/profile.rs#L1529`) validates root gates with task-terminal requirements (`true` at line 1536). `validate_gate_sequence` (`crates/forge-tasks/src/tasks/profile.rs#L1703`) requires one terminal delivery when delivery is present, looking only one level through containers. A milestone closing root follows its task delivery, so the official Milestone is rejected. A pure Workflow without a delivery gate also fails terminal validation.

Lines 1766–1803 apply the same strictly-prior lexical rule to `reject_to` and `independent_from`. `verify → reject_to: polish` is a forward edge and is rejected. Rejection destination resolution at lines 1669–1701 can count template gates or gates from another subject, which does not establish a valid executable destination.

**Required repair:** validate graph shape by subject. Task tracks require their own valid terminal receipt behavior; Workflow does not require task delivery; milestone roots permit preflight, task expansion, and closure. Resolve rejection edges to unique executable nodes in the same subject even when forward/cyclic. Validate independence against actual accepted predecessor evidence; do not conflate it with array position. See D2.

**Proof:** all three examples validate; missing/ambiguous/cross-subject targets fail; forward repair edges and legal cycles pass; unsafe independence references fail.

### F06 — P1: The declared repair graph has no success-edge semantics

**Source-confirmed; profile/design gap.** `advance_gate_cursor_for_task` (`crates/forge-tasks/src/engine/claim.rs#L311`) advances to the next applicable leaf. Applied to `verify, polish, review`, a successful verify still executes polish. After polish, there is no normal pass edge back to verify. Deleting the fallback enum cannot by itself implement the plan's required `verify pass → review`, `verify fail → polish`, `polish pass → verify` graph.

The official examples also make review independent from `polish`, although a clean pass should skip polish entirely. Independence from a node that never ran is not a complete reviewer-independence policy.

**Required repair:** provide declarative success routing in the shared graph. D2 proposes an explicit `pass_to` field; this is a proposed schema addition, not a claim that it exists today. Use lexical next only as the default sequence behavior. Bind reviewers independently from all actual builders of the candidate, including polish when executed. Keep ordinary attempt limits if admitted; do not recreate a verification-specific state machine under new names.

**Proof:** clean pass skips polish; failure enters polish; successful repair reruns verify; review rejection enters the same repair graph; clean-path independence works without invented polish evidence.

### F07 — P1: Multiple task blocks and task types are not modeled

**Reproduced/source-confirmed.** `TaskGateBlock` (`crates/forge-tasks/src/tasks/profile.rs#L605`) only has `gates` and lacks `deny_unknown_fields`; a `tasks.when` discriminator is silently discarded. `TaskSpec` (`crates/forge-tasks/src/tasks/mod.rs#L619`) denies unknown fields but has no ADR 0020 task `type` field or declared per-task command override. Two anonymous tracks with `build` yield `TASK_GATE_CURSOR_AMBIGUOUS`; tracks with unique names are concatenated instead of selected.

**Required repair:** parse the admitted task type shape, normalize its aliases, and support an explicit task-block selector. Require exactly one applicable track at admission, with clear zero-match/overlap errors. Preserve block identity in canonical paths. Do not concatenate unrelated code/doc/database tracks, discard selector fields, or infer admission type from a mutable file list. Reject unsupported fields until implemented. See D1.

**Proof:** one block, disjoint multiple blocks, overlapping selectors, no match, repeated leaf names in different tracks, scalar/list task types, and absent-type default behavior.

### F08 — P1: Conditions fail open and lose inherited predicates

**Source-confirmed.** `claim.rs:32` uses child condition **or** parent condition, so a child `when: code` replaces an inherited `when: seam`; both constraints are not enforced. `task_matches_gate_condition` (`crates/forge-tasks/src/engine/claim.rs#L252`) handles four string cases and returns `true` for everything else. Condition aliases accepted by `normalize_gate_condition` (`crates/forge-tasks/src/tasks/profile.rs#L1009`) are not applied during gate normalization. Thus accepted aliases and `public_api` can execute unconditionally.

`code` and `doc` are guessed from scope extensions; mixed code/document scopes and scope extension alter the result. `db_migration` is a substring guess. `seam` checks only concurrency mode, not incoming/outgoing dependency topology. Parallel tasks with no dependency are treated as seams; related sequential tasks are always skipped.

**Authority conflict:** ADR 0020's condition description ties seam applicability to DAG relationships, while its sequential-flow discussion and the later plan explicitly require sequential tasks to skip the coordinated seam barrier. Resolve this by distinguishing dependency reconciliation from a concurrent delivery barrier; do not reintroduce sequential deadlock. A Boolean based only on concurrency cannot express the distinction.

**Required repair:** normalize typed predicates once, reject unsupported values, AND inherited predicates, and evaluate against admitted type, public-interface facts, and the applicable DAG/concurrency policy. Persist the applicability decision or bind it to immutable authority. See D1.

**Proof:** every accepted predicate and alias; unknown predicates fail closed; nested parent/child conjunction; mixed scopes; dependency/concurrency/cancellation combinations; no implicit type change after scope extension.

### F09 — P1: Admission, reset, claim, and next disagree on the active gate

**Reproduced/source-confirmed.** Admission at `store/mod.rs:1209` (`crates/forge-tasks/src/store/mod.rs#L1209`) takes the first unfiltered executable gate and uses root `gates.len()` to locate the completed stage. Reset at `store/lifecycle.rs:624` (`crates/forge-tasks/src/store/lifecycle.rs#L624`) similarly restores the unfiltered first stage. Engine claim chooses the filtered stage, but `ClaimContext::claim` (`crates/forge-tasks/src/store/lifecycle.rs#L974`) compares it to the old stored stage at line 1047. A false first condition makes a newly admitted task unclaimable.

`next` (`crates/forge-tasks/src/store/lifecycle.rs#L525`) claims the stored stage at line 615 before the engine resolves its applicable cursor (`engine/lifecycle.rs:712–727`). Requirements are checked before filtered selection in explicit claim (`:149`). These paths can demand skipped-gate inputs or persist a claim before later cursor resolution fails.

**Required repair:** resolve and persist the applicable current node inside the admission/reset/claim/next transaction. Use the same projection for completed-state indexing. Validate requirements for that node, then claim it; release or roll back every failed activation/context path. Keep explicit-stage mismatch diagnostics and claim CAS.

**Proof:** false first/middle/last gate; all gates skipped; reset; completed sync; next versus explicit claim parity; context failure must leave no live claim.

### F10 — P1: Skipped gates remain mandatory snapshot and handoff prerequisites

**Reproduced/source-confirmed.** `build_snapshot_candidate` (`crates/forge-tasks/src/store/mod.rs#L666`), lines 666–741, chooses the latest preceding gate marked for snapshots without checking whether it was applicable and accepted. The same assumption appears in `engine/mod.rs:1068–1074` and `engine/gates.rs:2244–2294`. A sequential task skips `seam.integrate`, reaches deliver, then fails because that skipped gate has no snapshot, despite an accepted build snapshot.

`accepted_gate_evidence` (`crates/forge-tasks/src/milestones/handoff.rs#L229`), lines 229–353, requires every flattened gate. `flattened_profile` (`crates/forge-tasks/src/engine/claim.rs#L119`) includes conditional leaves irrespective of the task, so a successfully skipped gate later blocks handoff. Accepted boundary-caller lookup has the same unfiltered predecessor risk.

**Required repair:** use accepted execution history for the current task/contract revision and applicable path. Choose the last accepted candidate-producing transition, not the last syntactic snapshot declaration. Require proof only for nodes actually required/executed on that path; skipped-node records must not be forged as passing evidence.

**Proof:** complete actual snapshot-backed sequential delivery and milestone handoff with skipped seam gates; include conditional contracts, reset/reject cycles, and stale accepted snapshots.

### F11 — P1: Seam integration can invalidate the only delivery review source

**Source-confirmed.** The repository profile snapshots `seam.integrate` at lines 217–237 (`.forge/acdd/profile.yaml#L217`), after the ordinary review. Delivery at lines 239–251 includes only `review` in `review_sources` and is independent only from `build`. The delivery validator at `engine/gates.rs:2280` (`crates/forge-tasks/src/engine/gates.rs#L2280`) requires that source review's commit to equal the latest candidate. A meaningful seam integration changes that candidate, so ordinary review evidence no longer matches it. The seam reviewer is not used as the final source.

**Required repair:** specify and enforce final-candidate review sources for the executed path. Include the seam review when integration runs, and verify its independence from the integration builder. Preserve the original task review's coverage without requiring its old candidate hash to equal a later integrated tree. Receipt provenance must distinguish those reviewed candidates. Do not relax immutable delivery merely to make hashes match.

**Proof:** modify a tracked file during seam integration, run its review, then deliver; stale/no seam review must fail. Also complete the skipped-seam path.

### F12 — P1: Recovery replays a different lifecycle and can index out of bounds

**Source-confirmed.** `store/recovery.rs:1849–1874` (`crates/forge-tasks/src/store/recovery.rs#L1849`) calculates a flattened position but checks root gates as prerequisites. At lines 1953–1956 (`crates/forge-tasks/src/store/recovery.rs#L1953`), it uses the flattened position directly as `submitted_profile.gates[position]`. A nested leaf index can exceed the number of root entries and panic. Lines 1966–1974 compute pass/completion from root length and reject only from `reject_to`, ignoring conditional skips and resolved `reject_routing`.

**Required repair:** resolve canonical nodes and replay the same persisted transition decisions used at runtime. No root-array indexing by leaf offset. Validate accepted history, applicability, target, subject, and authority revision through the shared resolver. Return typed corruption errors instead of panicking.

**Proof:** restore/checkpoint replay at every nested stage, after skipped gates and symbol-routed rejection, after delivery, and with malformed/stale paths. Inject the out-of-range case and assert an error without panic or partial restoration.

### F13 — P1: Templates and stage names are validated differently from execution

**Source-confirmed.** `validate_template_cycles` (`crates/forge-tasks/src/tasks/profile.rs#L1968`) validates named templates, but task cursor construction and `executable_gates` do not instantiate `use_template`. A named template-use leaf can be treated as the leaf itself, omitting its body; an anonymous use lacks an executable ID. Validating a template is not executing it.

Generic retry policy is similarly decorative: `GateDef.max_attempts` at `profile.rs:923` and `GateTemplate.max_attempts` at `:621` are accepted, but executable transition paths do not enforce these limits. AST access checks find only template validation and fallback-specific limit consumption. Ordinary gate limits cannot be claimed as working protection against repair cycles.

`index_of` (`crates/forge-tasks/src/tasks/profile.rs#L1147`) and `by_stage` (`crates/forge-tasks/src/tasks/profile.rs#L1158`) choose a matching leaf/suffix, while `cursor_for_path` has separate ambiguity logic. A wrong fully qualified path can fall through to a leaf match. Parent/leaf path prefixing and the special `milestone_verification` normalization exception further split identity rules.

**Required repair:** expand templates once into the subject's compiled graph, with deterministic names and inherited command/guidance/condition semantics. Validate and enforce any declared ordinary attempt limit against durable attempts. Use that graph for all lookups. Fully qualified names require exact matches; permit a short alias only when unique in the selected subject/track. Validate references after expansion.

**Proof:** actual execution of nested templates, use-site overrides, cycles, zero/valid/exhausted ordinary attempt limits, repeated leaf names, invalid qualified names, and aliases through claim, submit, receipts, and recovery.

### F14 — P1: Rejection routing depends on prose and loses the selected destination

**Source-confirmed.** `engine/gates.rs:2452–2528` (`crates/forge-tasks/src/engine/gates.rs#L2452`) scans strings and shallow object values for `symbol`, `interface`, and sometimes `signature`. `no symbol changes` routes to the architectural gate; a typed signature finding nested differently can route to scope. The separate `evaluate_reject_routing` (`crates/forge-tasks/src/engine/submit.rs#L466`) helper uses different classification rules and is not the production decision used here.

The chosen target is not consistently consumed downstream: `rejected_workflow_context` (`crates/forge-tasks/src/engine/workspaces.rs#L395`) reconstructs from `reject_to`, so a `reject_routing.symbols` return to contract can lose its findings/reuse context. Recovery also reconstructs the wrong destination (F12). The target is resolved without checking its task applicability.

**Required repair:** normalize structured finding kinds once; define deterministic precedence (symbol expansion before implementation/scope, then default rejection). Persist the actual canonical destination and classified findings in the transition. Context, worker reuse, replay, and review retry consume that record. Validate target applicability and same-subject routing. Legacy prose can remain narrative, not a mechanical classifier.

**Proof:** scope/symbol/signature/mixed findings, negated prose, nested payload rejection or normalization, absent routing, forward routing, conditional target, and complete context/recovery after symbol rejection.

### F15 — P1: Admitted public symbols are not mechanically enforced

**Source-confirmed.** `TaskSpec.symbols` and `extended_symbols` exist, and profiles declare `reject_routing.symbols`, but the submission guard does not compare changed/new public symbols with that admitted set. `validate_public_signature_scope` (`crates/forge-tasks/src/engine/gates.rs#L1687`) checks baseline callable changes and caller path coverage. It does not consume the task's admitted symbols. Covering caller paths is a different requirement from authorizing an interface expansion.

**Required repair:** compare the indexed public-interface delta against admitted plus explicitly extended symbols, alongside the existing caller-safety check. Fail with a structured symbol finding routed to contract/synthesis as appropriate. Preserve current baseline/caller and retirement protections. New public symbols, aliases, trait items, and visibility changes require deliberate handling; do not use text substrings as symbol identity.

**Proof:** admitted/unadmitted new symbol, signature expansion, private-only change, trait/public alias cases, symbol extension with revision invalidation, and routing to the architectural owner.

### F16 — P1: Command failure does not traverse ordinary rejection edges

**Source-confirmed.** `execute_command_gate_with_timeout` (`crates/forge-tasks/src/milestones/command_gate.rs#L31`) persists a failed trace and returns `TASK_COMMAND_FAILED` at lines 226–237. It does not apply the gate's `reject_to` or end the claim through a rejection transition. `submit_zero_arg` (`crates/forge-tasks/src/engine/submit.rs#L149`) explicitly refuses reject actions; `verify_task` at lines 312–384 reports verification error metadata rather than routing a failed command gate. The graph cannot autonomously move from verify to polish through this task mechanism.

**Required repair:** route engine-observed command failure through the shared transition operation, preserving logs and closing the claim atomically. Recognize expected runtime-red contract outcomes as successful contract proof; timeout, spawn failure, compilation/setup failure, and unexpected exit are separate failure classes. Do not route every nonzero exit identically.

**Proof:** success, assertion-red success, unexpected exit, spawn failure, timeout, malformed counts, and repeated failure all produce the correct next node, claim state, and durable trace.

### F17 — P1: Fallback is required throughout schema, configuration, and runtime

**Source-confirmed.** `GateFallback` (`crates/forge-tasks/src/tasks/profile.rs#L627`), `GateDef.fallback` at line 917, fallback field validation at lines 1937–1964, and cycle traversal at lines 1984–2001 remain. `validate_milestone_verification` (`crates/forge-tasks/src/tasks/profile.rs#L1561`) hard-codes exactly verify/review and requires `build_review_cycle`, `check_full_repo`, and two fallback attempts. A clean zero-fallback milestone profile cannot satisfy it.

`MilestoneVerificationPhase` (`crates/forge-tasks/src/milestones/handoff.rs#L888`), `FallbackBuildClaim`, and `MilestoneVerificationState` retain seven phases, `fallback_attempts`, `fallback_build_claim`, and `fallback_builder`. Configuration, routing, execution, build/review/delivery functions at lines 1020–1821 contain the fallback action machine. Both `acdd.default.yaml:340–365` (`crates/forge-tasks/src/tasks/acdd.default.yaml#L340`) and repository profile:253–278 (`.forge/acdd/profile.yaml#L253`) still configure it.

**Required repair:** remove the entire subsystem and its parsing side channels, not just the enum or YAML keyword. Drive milestone closure through the shared root graph. Preserve useful observed-command, binding, journal, review, receipt, and archive primitives. Recognize persisted legacy state explicitly and supply a safe recovery diagnostic/migration; do not silently interpret a half-fallback checkpoint as a passed generic gate. D3 enumerates deletion sites.

**Proof:** reject obsolete fallback configuration; normal verify/polish/review transitions and restart use no fallback phase, field, action, or hard-coded two-attempt policy. Refactor the three existing fallback tests rather than retaining their obsolete contract.

### F18 — P0: External JSON can bypass autonomous milestone verification

**Source-confirmed.** `handoff` (`crates/forge-tasks/src/milestones/handoff.rs#L2644`) validates client `IntegrationEvidence` and archives without requiring an accepted engine verification checkpoint. `IntegrationEvidence::validate` (`crates/forge-tasks/src/milestones/integration.rs#L104`) only requires a nonempty command, zero exit code, and zero failures; it does not establish execution, the mandatory command, or positive required counts.

Telemetry at `handoff.rs:2417–2428` conditionally observes a project command only for particular source polish. `observe_or_reuse_project_command` (`crates/forge-tasks/src/milestones/integration.rs#L148`) treats matching client command text and HEAD as sufficient reuse, returning no stdout/stderr digests. Clean trees, test-only polish excluded from the source flag (`handoff.rs:1877–1879`), and self-reported passing JSON can therefore avoid the required engine run.

**Required repair:** handoff accepts mechanical verification only from the engine's accepted root verify transition bound to the current candidate, target base, authority, profile, and command plan. Execute the mandatory registered `check_full_repo` when no valid bound run exists. External JSON may convey independent cognitive review; it cannot mint command proof, counts, or a reuse decision. Apply the guard to retry/journal paths too. See D4.

**Proof:** failing full-repo command plus forged passing JSON must prevent archive, prune, receipt/note publication, and handoff success. Cover clean/no-polish, test-only, source-polish, explicit commit, alternate target base, and every retry path.

### F19 — P1: Verification uses the wrong profile and insufficient cache identity

**Source-confirmed.** Milestone verification loads the workspace profile (`handoff.rs:1305–1482`), whereas synthesis honors the milestone's `acdd_profile` (`synthesis.rs:272`). Cached review-required state is reused on content binding/HEAD without binding the resolved profile and command/output policy. A changed command can leave an old run acceptable for new requirements.

Delivered-state accommodation at `handoff.rs:1178–1237` allows broad documentation paths/Markdown changes. Such files may include executable examples, fixtures, or command inputs. A file extension alone does not prove that a previous command covered the current candidate.

**Required repair:** resolve the same admitted profile for every stage; bind proof to its normalized execution digest, command plan, output rules, authority revision, candidate tree/content binding, and target base. Preserve deliberate content-based proof reuse across navigation-only history rewrites. Treat archival outputs as narrowly declared outputs; reverify execution-affecting changes.

**Proof:** pinned versus workspace profile, changed command/count policy, documentation fixture change, target-base change, content-identical rewrite, and archive-output-only retry.

### F20 — P1: Milestone command dispatch and count acceptance are unsound

**Source-confirmed.** `run_milestone_check` (`crates/forge-tasks/src/milestones/integration.rs#L250`) ignores declared output parsing and uses Rust-specific `test_counts` (`crates/forge-tasks/src/milestones/integration.rs#L501`). `VerificationCheck::passed` (`crates/forge-tasks/src/milestones/integration.rs#L244`) checks only completed/exit 0, so zero required counts or a log reporting failures can still pass. The mandated composite command runs commitment integrity independently and again under all-targets; summing every Rust summary double-counts that standalone segment contrary to [TESTS.md](../../TESTS.md).

For pattern commands, `CommandDef::command_for_root` (`crates/forge-tasks/src/tasks/profile.rs#L778`) scans only five levels, suppresses walk errors, selects one matching command, and can fall back to the first configured command. A polyglot repository is not fully verified by selecting one language. The older `observe_command` (`crates/forge-tasks/src/milestones/integration.rs#L188`) uses unbounded process output execution rather than the bounded runner.

**Required repair:** reuse the declared command-plan and output-parser machinery; run all applicable required commands, fail unmatched required paths/errors, enforce output count policy and zero failures, and avoid duplicate segment counting. Record each command's timeout, exit, digests, and counts. Use the bounded process runner for all execution paths.

**Proof:** non-Rust output, no counts, reported failures with exit 0, timeout, nested/polyglot paths, overlapping globs, unmatched paths, unreadable traversal, and exact composite-command count aggregation.

### F21 — P1: Contract submission has no mechanical decision guard or supporting revision schema

**Reproduced/source-confirmed.** `compile_evidence_envelope` (`crates/forge-tasks/src/engine/submit.rs#L21`) and `submit_with_pre_persist` (`crates/forge-tasks/src/engine/gates.rs#L2022`) do not require `topic = 'decisions'` before accepting a contract. The existing submission path can advance with no blackboard record. A late blackboard rollup used for delivery is not this guard.

The suggested SQL in the lifecycle plan cannot simply be pasted in: `task_blackboard` is created at `store/mod.rs:550` (`crates/forge-tasks/src/store/mod.rs#L550`) without `claim_revision`. `schema::migrate` (`crates/forge-tasks/src/store/schema.rs#L6`) migrates version 6 to 7 by adding recipient/status only. Blackboard posts do not bind architectural payload to the active claim revision.

**Required repair:** add a versioned additive schema migration, server-stamped claim/contract identity, and structured payload validation for nonempty `blueprint`, `data_flow`, and `invariants`. Require the record for the active canonical contract gate/subject, worker, claim, and contract revision. Perform the authoritative guard inside submission's transaction and use it for typed, inferred, direct-store, and root-gate submissions. Level 0 synthesis uses milestone scope, not a fabricated task ID. Return `TASK_DECISION_MISSING` for missing/stale decisions; reject malformed decisions distinctly. See D5.

**Proof:** absent, malformed, empty-section, wrong topic/task/milestone/gate/worker, old claim, old contract, superseded/deleted decision, valid decision, transaction race, and migrated database cases.

### F22 — P0: Typed submissions can invent contract, command, and seam execution

**Reproduced/source-confirmed.** `validate_contract` (`crates/forge-tasks/src/tasks/gates.rs#L105`) accepts any positive red exit for normal proof policies; it does not enforce the gate's expected exit/signature. `validate_command` (`crates/forge-tasks/src/tasks/gates.rs#L129`) accepts any registered command or command-prefix extension, not necessarily the gate's selected command, with caller-supplied counts. `validate_seam_test` (`crates/forge-tasks/src/tasks/gates.rs#L170`) accepts references/counts without observed execution.

`submit_with_pre_persist` (`crates/forge-tasks/src/engine/gates.rs#L2152`) applies these payload validators without requiring claim-bound execution traces. The zero-argument compiler has stronger candidate/run checks, but clients can choose typed evidence instead. Optional code-map reachability checks do not prove that a seam test actually ran. The successful exit-2 and unexecuted exit-9 probes demonstrate this split.

**Required repair:** every mechanical proof must resolve an engine-observed, completed run for the current subject, canonical gate, claim/contract revision, candidate fingerprint, selected command plan, and output policy. Typed payloads reference or agree with those facts; they do not create them. Cognitive review remains reviewer-supplied. Validate expected runtime-red signatures and actual executed seam identities. Do not remove the existing reachability/signature guards. See D6.

**Proof:** reject forged counts, wrong command/prefix, nonexistent/unexecuted seam, compilation error presented as runtime-red, stale/incomplete run, changed candidate, reused other-claim traces, and typed-versus-zero-argument parity.

### F23 — P1: Red-to-green verification is not part of accepting the build gate

**Source-confirmed.** `verify_red_to_green` (`crates/forge-tasks/src/engine/submit.rs#L388`) is a helper, but `command_proof` (`crates/forge-tasks/src/engine/submit.rs#L620`) does not invoke it to require the accepted red seam to pass in build. The helper also infers a test target from Cargo command syntax. Existing runner tests call the helper directly; they do not establish that an ordinary build submission is blocked when its contract seam remains red.

**Required repair:** couple accepted contract evidence to the subsequent build's observed execution for that contract revision. Persist test identities/results at execution time and require each mandated seam to be present and green. Avoid inferring universal test identity from Rust command text; the current command-proof fact stores only one optional seam reference. Keep proof-policy exceptions explicit and admission-bound.

**Proof:** build passes unrelated tests while contract seam stays red; contract changes; multiple seam refs; custom/non-Rust runner; stale green trace; accepted direct-proof exception; full public run/submit path.

### F24 — P1: Persistence normalization violates model and explicit-hook intent

**Source-confirmed.** `normalize_gate_hierarchy` (`crates/forge-tasks/src/tasks/profile.rs#L2229`) translates persistence into legacy `sha_snapshot` and `auto_commit` flags. Delivery defaults to automatic commit; `post: snapshot` does not disable that flag. An omitted delivery post hook becomes commit, while explicit `auto_commit: false` can erase `post: commit`. `submit_inner` (`crates/forge-tasks/src/engine/mod.rs#L1191`) then decides actual delivery commit from the legacy Boolean.

The official Plan's snapshot delivery therefore does not mechanically guarantee no automatic commit. There is no Plan-level prohibition on commit hooks or a model-aware default policy. `PersistenceAction` supports snapshot/commit; absence represents no hook, but literal `pre/post: none` is not accepted. ADR 0021's shorthand `persistence: none/snapshot` is not the current mapping schema; this spelling needs explicit reconciliation, not an assumed parser capability.

**Required repair:** use one resolved persistence policy as runtime authority. Workflow defaults to no hooks; Plan permits snapshots and forbids automatic commit hooks before/pre/post/delivery; Milestone delivery commits as declared. Explicit snapshot/none must not be overridden by legacy flags. Remove legacy fields or perform a single unambiguous migration at parse time. See D7.

**Proof:** each model × pre/post none/snapshot/commit; omitted defaults; conflicting legacy fields; Plan HEAD unchanged through delivery; snapshots retained; Milestone delivery produces one commit.

### F25 — P1: Generic pre/post commits lack the delivery rollback boundary

**Source-confirmed.** Pre-commit in `finish_claim` (`crates/forge-tasks/src/engine/lifecycle.rs#L204`) writes Git before later snapshot/context/checkpoint work. Failure releases the claim but cannot undo an already created commit. Generic post-commit at `engine/mod.rs:1261–1267` (`crates/forge-tasks/src/engine/mod.rs#L1261`) can move HEAD inside the submission hook without populating the delivery rollback guard used at lines 1272–1294. A later database/checkpoint failure can leave Git and gate state disagreeing.

**Required repair:** extend the existing Git/index rollback and recovery-journal pattern to every authorized persistence hook. Prepare proof and output objects, perform a compare-and-swap transition, and retain enough journal state to recover after process death. On ordinary failure restore only operation-owned Git/index mutations; preserve user changes. Idempotent claim/submit retry must not create extra commits or rerun a pre hook blindly.

**Proof:** fail each step after pre/post commit, fail hooks, fail SQLite commit/checkpoint, retry accepted submission, and restart with pending journal; assert HEAD/index/state consistency.

### F26 — P0: Scoped commit resets the entire user's real index

**Reproduced.** `git_scoped_commit` (`crates/forge-tasks/src/git.rs#L529`) correctly builds a temporary index for scoped paths, then executes real-index `git read-tree HEAD` at lines 624–628 and ignores its status. This clears unrelated staged additions/edits/deletions. The existing isolated test fails with a staged unrelated `README.md` becoming untracked.

The lifecycle plan's “already verified” statement about whole-index synchronization is disproved by this test. Simply removing synchronization is insufficient: scoped paths would remain staged against the old HEAD, recreating ghost changes.

**Required repair:** synchronize only committed scope paths to the new HEAD, preserving unrelated index entries byte-for-byte in meaning, including partially staged content and deletions. Reuse the existing index rollback machinery in `engine/git_snapshot.rs` (`crates/forge-tasks/src/engine/git_snapshot.rs#L118`), and propagate synchronization errors through rollback/recovery. Use literal NUL-delimited pathspecs. See D8.

**Proof:** make the existing failing test pass; add separate staged-deletion, partial-stage, rename, unusual-filename, hook-failure, index-lock, and rollback cases. Never run destructive reset on the user's workspace as an audit workaround.

### F27 — P1: Checkpoint trees and gate paths do not identify accepted snapshots

**Source-confirmed.** Task submission records `HEAD^{tree}` at `engine/gates.rs:2620–2695` (`crates/forge-tasks/src/engine/gates.rs#L2620`), although a snapshot can contain uncommitted candidate changes. Git lookup errors are collapsed through default values. Claim checkpoint construction at `engine/lifecycle.rs:283–315` similarly does not consistently use the captured candidate tree.

Transition `gate_path` uses the cursor's parent container (often `tasks`) at `engine/gates.rs:2648`, while execution traces use a canonical leaf such as `tasks.build`. A checkpoint can therefore claim the wrong tree and lack the node identity needed for deterministic replay.

**Required repair:** record the actual immutable candidate tree selected by the persistence action and the canonical executed leaf path. Distinguish baseline HEAD from candidate tree explicitly. Use the same identity in proof, transition, checkpoint, trace, and breadcrumb; fail closed when required Git identity cannot be resolved.

**Proof:** dirty/untracked/deleted candidate differing from HEAD, nested gates, skipped gate, missing Git object, and checkpoint-to-proof tree/path equality across restart.

### F28 — P1: Milestone state/trace writes have crash-invalid intermediate states

**Source-confirmed.** `save_milestone_verification_state` (`crates/forge-tasks/src/milestones/handoff.rs#L977`) records a transition and then saves a checkpoint in separate operations. Failure routing persists a failure action with a phase that `milestone_verification_action_matches_phase` (`crates/forge-tasks/src/milestones/handoff.rs#L1020`) rejects until a subsequent fallback write completes. The corresponding review rejection has the same intermediate-state problem. A crash can leave an unloadable checkpoint.

Verification attempt IDs are derived from saved run counts at `handoff.rs:1382–1391`, but `run_milestone_check` (`crates/forge-tasks/src/milestones/integration.rs#L337`) persists a unique trace before the new state. A crash between those writes can make retry reuse a trace key. Transactional task submission is stronger than this separate milestone writer.

**Required repair:** replace these writes with the shared subject transition transaction: reserve an attempt before execution, persist completed observations, then atomically CAS proof/transition/checkpoint/claim state. Treat external command execution as potentially repeatable after a crash; do not claim impossible exactly-once process semantics. Recovery must distinguish reserved/running/completed/accepted attempts and safely reuse only completed bound facts.

**Proof:** fault injection at every reservation/trace/transition/checkpoint boundary, concurrent runners, failed command/review routing, and repeated restart without duplicate acceptance or unique-key dead ends.

### F29 — P1: DAG scheduling and seam barriers do not implement declared concurrency semantics

**Source-confirmed/support gap.** `ClaimContext::claim` (`crates/forge-tasks/src/store/lifecycle.rs#L1025`) unconditionally requires dependencies to be completed. `unsatisfied_dependencies` (`crates/forge-tasks/src/store/mod.rs#L1875`) uses terminal task readiness for all modes. Speculative work cannot begin on accepted upstream candidates as contemplated by ADR 0020. `rebase_speculative_dependency` (`crates/forge-tasks/src/engine/scheduler.rs#L6`) exists as a callable helper but is not automatically connected before command/proof execution.

The task delivery path validates its candidate and subtasks but has no corresponding peer seam-wave barrier. Merely enabling `when: seam` is neither dependency integration nor coordinated acceptance. Normal completed-dependency DAG waves are supported; speculative coordination is the missing part.

**Required repair:** preserve ordinary DAG readiness, define accepted-candidate readiness separately for admitted speculative mode, and bind automatic rebase/reverification to the actual dependency candidate revisions. Enforce a concurrent barrier only for applicable live participants; cancelled/delivered/irrelevant tasks must not deadlock it. Resolve the seam semantic conflict in F08 before adding a barrier.

**Proof:** sequential DAG waves, disjoint parallel work, speculative upstream change, rebase conflict, cancellation, peer rejection, stale dependency proof, and no premature delivery or sequential deadlock.

### F30 — P1: Milestone reviewer independence is weaker than task independence

**Source-confirmed.** Task claims have worker and session-fingerprint checks (`store/lifecycle.rs:15–54`, `:1050–1070`). Milestone review at `handoff.rs:1677–1686` (`crates/forge-tasks/src/milestones/handoff.rs#L1677`) and external evidence validation compare worker IDs/builders only. The same agent session can change its worker label and satisfy that weaker check. Repair builder history is represented by the bespoke fallback fields rather than a shared candidate-provenance policy.

**Required repair:** use persisted subject claims and the same session independence enforcement for root reviews. Exclude every actual author of the reviewed candidate, including integration/polish authors, while allowing a clean verify path with no polish author. Bind cognitive review to the current content/target/profile authority.

**Proof:** same worker, different worker same session, genuinely independent session, multiple repair authors, content changes after review, and clean path without polish.

### F31 — P1: Missing review sources produce empty receipts and late handoff failure

**Source-confirmed.** Hierarchical validation checks a referenced `receipt_review` when present but does not require a nonempty applicable `review_sources` set (`profile.rs:1816` onward); legacy validation was stricter. The official Plan delivery and Milestone task delivery omit both an explicit review source list and a derived final-candidate rule. Delivery at `engine/gates.rs:2275–2294` iterates the empty list, then generates an empty `review_sources` rollup at `:2329–2333`. `review_source_proofs` (`crates/forge-tasks/src/tasks/gates.rs#L339`) accepts that empty map when the expected list is empty. `validate_durable_receipt_with_profile` (`crates/forge-tasks/src/tasks/gates.rs#L400`) therefore does not itself prevent this empty-review receipt, while `validate_task_provenance` (`crates/forge-tasks/src/milestones/handoff.rs#L365`) explicitly rejects it at lines 408–410. Parse/delivery acceptance conceals a later handoff failure and loses durable review provenance.

The durable validator also searches only root `profile.gates` for delivery and its candidate snapshot (`tasks/gates.rs:405–415`), but completed-task sync passes the hierarchical task profile directly (`store/mod.rs:1201–1206`). Valid hierarchical completed tasks can fail re-admission with `profile has no delivery gate`. The task projection must be shared here too.

**Required repair:** either declare the sources explicitly in each official example, or define one canonical derivation from accepted applicable review predecessors and validate it before admission. Reconcile conditional seam review as F11 requires. Validate receipt semantics at the task-track level; do not require task receipts from Workflow root review gates.

**Proof:** each official task lifecycle reaches durable receipt and, for Milestone, actual handoff. Missing, skipped, stale, wrong-candidate, non-review, and cross-track sources fail at the earliest valid boundary.

### F32 — P2: Tests protect legacy shortcuts instead of the three complete models

**Source-confirmed.** Parser/gate tests predominantly use older/future-v2 fixtures; accepted condition-alias tests do not execute the conditions. Nested/conditional tests in `tests/acdd/gates_ast.rs#L732` exercise a flat root sequence as if it were task execution. The sequential seam test at lines 878–1071 uses proof-free/snapshot-free gates and stops before real delivery, so it misses F10.

The milestone verification fixture in `milestones.rs:7242–7361` (`tests/acdd/milestones.rs#L7242`) uses the compatibility policy container, fallback, and fake command output. The three passing verification tests assert the legacy two-attempt machine. `tests/acdd/support.rs#L15` builds externally asserted passing handoff verification, which makes that bypass a fixture convention. Runner helper tests do not prove build submission invokes red-to-green validation.

**Required repair:** use the official profiles as input and drive complete lifecycles through public runtime seams, including real snapshot candidates, command observations, receipts, recovery, and handoff. Keep fast unit tests, but stop treating them as evidence for omitted transitions. Add separate descriptive domain tests for distinct workflows; parameterize only variations of the same unit. Preserve parallel test execution. No implementation tests are added by this audit.

## Exact edit proposals

These are unapplied design diffs. Minus lines and edit intervals identify the inspected source exactly; plus lines containing new helpers/types specify proposed interfaces whose implementations and all listed consumers must be supplied together. They are not presented as a standalone compiling patch. Keep `Cargo.toml` and `build.rs` out of the repair scope unless separately admitted. Use existing modules and gate/checkpoint storage rather than a parallel registry or verification-only executor.

### D1 — Task-only projection, selectors, and conjunction

The immediate root-isolation hunk for `engine/claim.rs:78–100` is:

```diff
     let mut cursors = Vec::new();
     for (root_index, gate) in profile.gates.iter().enumerate() {
-        let children = gate
-            .gates
-            .iter()
-            .chain(gate.tasks.iter().flat_map(|tasks| tasks.gates.iter()));
-        let effective_when = gate.when.as_deref();
-        if children.clone().next().is_some() {
-            let mut path = Vec::new();
-            if !gate.id.trim().is_empty() {
-                path.push(gate.id.clone());
-            }
-            let mut nested = gate.gates.clone();
-            if let Some(tasks) = &gate.tasks {
-                nested.extend(tasks.gates.iter().cloned());
-            }
-            collect_sequence(&nested, root_index, &path, effective_when, &mut cursors)?;
-        } else {
-            collect_sequence(std::slice::from_ref(gate), root_index, &[], effective_when, &mut cursors)?;
-        }
+        let Some(tasks) = &gate.tasks else { continue };
+        collect_sequence(
+            &tasks.gates, root_index, &[], gate.when.as_deref(), &mut cursors,
+        )?;
     }
     if cursors.is_empty() {
-        bail!("TASK_GATE_CURSOR_COMPLETE: no executable task gate remains");
+        bail!("TASK_MODEL_INVALID: task operations require a nonempty tasks: container");
     }
```

This hunk demonstrates the precise flattening correction but is **insufficient alone**: multiple tracks, selectors, default profiles, all alternate flatteners, and stores must change in the same repair. A no-task Workflow must remain valid for its own runner; the error applies only to task projection. An empty task block should have its own configuration diagnostic, distinct from a completed execution.

Replace task cursor selection with a task-aware projection rather than repeatedly flattening the whole profile:

```diff
-fn collect_cursors(profile: &GateProfile) -> Result<Vec<GateCursor>> {
+fn collect_cursors(profile: &GateProfile, task: &crate::store::Task) -> Result<Vec<GateCursor>> {
+    let track = profile.select_task_track(&task.spec)?;
+    // Expand only track.gates; root gates are a different subject.
+    // Canonical identities include the selected block and nested container path.
```

`select_task_track` is a proposed method: exactly one admitted block, deterministic identity, normalized type selector, no-match/overlap diagnostics. Pass the task/projection to `gate_sequence`, `flattened_profile`, `cursor_for_stage`, `advance_gate_cursor_for_task`, and their store/receipt/recovery consumers. Do not keep the old profile-only helpers as alternate task authority.

The schema edit at `tasks/profile.rs:602–608` is:

```diff
 #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
-#[serde(default)]
+#[serde(default, deny_unknown_fields)]
 pub struct TaskGateBlock {
+    /// Normalized selector for the admitted task type(s).
+    pub when: Option<String>,
     /// Ordered gate definitions applied to each task.
     pub gates: Vec<GateDef>,
 }
```

Parse/normalize this selector with the existing condition vocabulary, then compile it into typed predicates; the `String` is only the serialized form. Add the ADR's scalar/list task type to `TaskSpec` using its existing custom/default parsing conventions. Keep the task's admitted type stable across scope extension.

Replace the cursor's single inherited condition with a conjunction (proposed compiled representation):

```diff
-    pub when: Option<String>,
+    pub conditions: Vec<String>,
@@
-            let effective_when = gate.when.as_deref().or(inherited_when);
+            let mut effective_conditions = inherited_conditions.to_vec();
+            if let Some(condition) = gate.when.as_deref() {
+                let condition = normalize_gate_condition(condition)
+                    .with_context(|| format!("GATE_INVALID_CONFIGURATION: unknown condition '{condition}'"))?;
+                effective_conditions.push(condition.to_owned());
+            }
@@
-        if task_matches_gate_condition(cursor.when.as_deref(), task) {
+        if cursor.conditions.iter().all(|condition| {
+            task_matches_gate_condition(Some(condition), task)
+        }) {
```

Expose the existing `normalize_gate_condition` helper within the crate, import it at the use site, and preserve its `Option<&'static str>` result until the compiled typed representation replaces it. Change `task_matches_gate_condition` to evaluate a closed typed enum, or return `Result<bool>` with a corresponding fallible conjunction, so `_ => true` disappears. Predicate evaluation requiring DAG/public-interface facts receives those admitted facts, not only a `Task` and guessed filenames.

### D2 — One graph, subject boundaries, and normal routing

Replace `profile.rs:1121–1167`'s independent flatten/first-suffix lookup with canonical graph lookup. Replace the root terminal validation call at `:1536` and strictly-prior rejection validation at `:1766–1803`:

```diff
-        self.validate_gate_sequence(&self.gates, "gates", true)?;
+        let graph = self.compile_graph()?;
+        graph.validate_subject_boundaries()?;
+        graph.validate_references()?;
```

`compile_graph`, `validate_subject_boundaries`, and `validate_references` are proposed interfaces. They must expand templates, select execution subjects, resolve exact paths/unique aliases, normalize conditions, validate required task receipts, and validate same-subject pass/reject edges. This compiled projection replaces duplicate lookup tables; it does not add a second persistent gate registry.

Add an explicit success edge alongside `reject_to` in `GateDef`:

```diff
-    /// Prior gate to rewind to when this gate is rejected.
+    /// Gate entered when this gate is rejected; may be a forward repair node.
     #[serde(default)]
     pub reject_to: Option<String>,
+    /// Explicit success destination; omitted means the next applicable node.
+    #[serde(default)]
+    pub pass_to: Option<String>,
```

Update both official root repair graphs and the repository/default closure graph as follows:

```diff
       - id: verify
         command: check_full_repo
         proof: command
+        pass_to: review
         reject_to: polish
@@
       - id: polish
         role: builder
         command: check_full_repo
         proof: command
+        pass_to: verify
@@
       - id: review
         role: independent_reviewer
         proof: review
-        independent_from: polish
         reject_to: polish
```

Removing that last field requires the shared review policy to reject all actual candidate authors, including polish. It must not disable independence. Resolve this with the existing `independent_worker` policy and persisted author/session provenance; retain explicit references where the predecessor always executes.

Transition selection becomes: validate claim → validate bound proof/decision → resolve pass or classified reject edge → resolve applicability → apply authorized persistence → atomically persist transition/checkpoint/task state. Every admission/reset/next/recovery/receipt path uses this graph and accepted execution history. Completion is a subject terminal state, not `root_gates.len() - 1`.

### D3 — Exhaustive fallback and duplicate-authority removal map

| File and exact inspected interval | Delete/replace operation |
| --- | --- |
| `tasks/profile.rs:624–637` | Delete the complete `GateFallback` declaration. |
| `tasks/profile.rs:915–917` | Delete `GateDef.fallback` and its documentation/serde attribute. |
| `tasks/profile.rs:1079–1081` | Remove the separate top-level milestone-verification execution field; root graph owns closure. |
| `tasks/profile.rs:1561–1623` | Delete the hard-coded two-gate/fallback validator; validate ordinary root graph/required verification policy instead. |
| `tasks/profile.rs:1937–1964` | Remove fallback-specific field validation; keep ordinary command/template/attempt validation where still applicable. |
| `tasks/profile.rs:1984–2001` | Remove fallback template edges from cycle traversal; retain real `use_template` cycle checks. |
| `tasks/profile.rs:2021–2084`, `:2191–2206`, `:2277–2292` | Remove merge/parse compatibility handling that reintroduces the separate verification container; explicit topology replaces topology. |
| `milestones/handoff.rs:882–925` | Delete fallback state constants/types/fields and the phase enum; retain only reusable subject/binding concepts. |
| `milestones/handoff.rs:935–1037` | Replace bespoke load/save/action-phase logic with shared transactional subject checkpoints. |
| `milestones/handoff.rs:1039–1163` | Remove fallback action dispatch and required fallback configuration. Resolve root graph node/command from the admitted profile. |
| `milestones/handoff.rs:1240–1294` | Delete fallback routing/exhaustion; use ordinary reject transition to polish. |
| `milestones/handoff.rs:1305–1821` | Replace verify/fallback-build/fallback-review/delivery phase handlers with generic command, role claim, review, and delivery transitions. Preserve command runner, binding, receipt, and archive operations as reusable operations. |
| `.forge/acdd/profile.yaml:253–278` | Remove separate top-level container; add ordinary root `milestone_verification` graph after `tasks:` with D2 pass/reject edges. |
| `tasks/acdd.default.yaml:289–365` | Put per-task gates inside `tasks:`; remove separate fallback closure; define only model-consistent root closure when appropriate. |
| `tests/acdd/milestones.rs:7242` onward | Replace compatibility/fallback fixtures and phase assertions with official profile and ordinary graph assertions. |

The exact schema deletion is:

```diff
-/// Conditional fallback subgraph for a failed verification gate.
-#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
-#[serde(default, deny_unknown_fields)]
-pub struct GateFallback {
-    /// Reusable gate template to invoke.
-    #[serde(rename = "use")]
-    pub use_template: Option<String>,
-    /// Optional command override.
-    pub command: Option<String>,
-    /// Guidance slots attached to the fallback.
-    pub guidance: Vec<String>,
-    /// Maximum fallback attempts.
-    pub max_attempts: Option<u32>,
-}
@@
-    /// Reusable recovery subgraph for a failed gate.
-    #[serde(default)]
-    pub fallback: Option<GateFallback>,
```

Retain generic `max_attempts` only as a normal admitted graph policy. Do not retain `fallback_*` JSON action aliases or silently map their state into success. Diagnose old checkpoint/config formats explicitly. Removal checks target executable Rust, active YAML, generated schemas/help, exports, and fixtures; historical ADRs and this audit may legitimately discuss the removed subsystem.

### D4 — Handoff consumes engine proof, never client verification assertions

At `handoff.rs:2662–2665`, replace the verification authority while retaining binding and independent review checks:

```diff
     let sha = integration::resolve_commit(root, commit.unwrap_or("HEAD"))?;
     let target = integration::resolve_commit(root, &evidence.target_base)?;
     let current = binding(root, &item, &store, &relevant, &sha, &target)?;
-    evidence.validate(&current)?;
+    evidence.validate_review_and_binding(&current)?;
+    let verification = ensure_engine_verification(
+        root, &mut store, &item, &current, &sha, &target,
+    )?;
```

The proposed `ensure_engine_verification` resolves the milestone-pinned profile, executes the declared root `check_full_repo` through the shared command gate when no current accepted proof exists, records stdout/stderr digests and policy-valid counts, and returns only an engine-owned proof reference. On failure it commits the ordinary `reject_to: polish` transition and returns a blocking result before archival. It must not accept an arbitrary client command ID as the mandatory gate.

Replace all three archive/note sources at `handoff.rs:2759`, `:2803–2804`, and telemetry verification construction with the returned proof's command/counts/digests/reference. External evidence can keep a review/binding DTO; remove mechanical verification fields from authoritative input or reject mismatches explicitly. Update `IntegrationEvidence::validate`, `retry_handoff`, journal verification, and the final binding recheck to require the same accepted proof.

At `integration.rs:169–182`, delete the entire client-command reuse branch:

```diff
-    if verification.exit_code == 0
-        && verification.tests_failed == 0
-        && verification.command.trim() == registered_command.trim()
-        && resolve_commit(root, "HEAD")? == sha
-    {
-        return Ok(CommandObservation {
-            selection: selection.to_owned(),
-            command: registered_command.to_owned(),
-            exit_code: 0,
-            stdout_digest: None,
-            stderr_digest: None,
-            reused_verification: Some(true),
-            implementation_digest: binding.implementation_digest.clone(),
-        });
-    }
```

Replace the surrounding helper with lookup of an accepted engine run, or eliminate it when the shared gate owns execution. Never substitute unconditional rerunning of every optional telemetry command for a well-defined mandatory verification gate. Apply timeouts and capture both output digests on every actual execution.

### D5 — Revision-bound architectural decisions

Extend `task_blackboard` through the versioned migration in `store/schema.rs`, preserving version-6-to-7 upgrades and existing messages. From the current version 7, the next migration needs nullable legacy columns plus an index such as:

```sql
ALTER TABLE task_blackboard ADD COLUMN claim_revision INTEGER;
ALTER TABLE task_blackboard ADD COLUMN contract_revision INTEGER;
CREATE INDEX IF NOT EXISTS task_blackboard_decision_claim_idx
ON task_blackboard(task_id, gate, topic, claim_revision, contract_revision, status);
```

Old records remain readable but do not satisfy a current claim. For root subjects, reuse the generic subject/claim identity and milestone scope; a task-only index is not sufficient for Level 0 decisions. Stamp identities from the active claim in `store/blackboard.rs`, not from untrusted payload numbers. Existing free-form messages retain their purpose; an architectural `decisions` record used as proof must deserialize a structured decision with the three required nonempty sections.

At `engine/gates.rs:2152`, after active claim identity checks and before accepting proof/persistence, add the authoritative guard:

```diff
+        if action == "pass" && gate_def.proof.kind() == Some(ProofKind::Contract) {
+            require_current_architectural_decision(&tx, &task, &cursor)?;
+        }
         validate_gate_proof(
```

Also call the read-only guard from `engine/submit.rs` before expensive proof compilation for clear early diagnostics; the transaction guard remains authoritative. The helper is proposed and must enforce scope, canonical gate, worker, current revisions, active status, and structured content. Use the equivalent shared-subject check for milestone synthesis. Do not hold a long-lived write transaction while an agent authors a decision.

### D6 — One proof authority and enforced red-to-green

At `engine/gates.rs:2152–2158`, payload shape validation remains, then mechanical kinds require current observed facts:

```diff
         validate_gate_proof(
             &evidence.proof, &gate_def, &profile,
             action == "pass", &task.spec.proof_policy,
         )?;
+        if action == "pass" && matches!(gate_def.proof.kind(),
+            Some(ProofKind::Contract | ProofKind::Command | ProofKind::SeamTest))
+        {
+            validate_observed_gate_proof(&tx, &task, &cursor, evidence)?;
+        }
```

`validate_observed_gate_proof` is proposed. Factor the existing claim-bound run/plan/candidate validation from `compile_evidence_envelope` into shared code rather than opening another store transaction inside this transaction. Verify exact registered command selection and every completed command, expected runtime-red exit/signature, output/count policy, and observed test identities. A typed command string with additional shell text must not count as execution of a registered command.

Wire accepted red seam refs into build acceptance in `command_proof` and the authoritative store guard. Extend execution facts from a single optional `seam_ref` to the required set of observed test identities/outcomes with an additive migration if needed. Keep declared proof-policy exceptions, but never silently treat “no trace” as an exception.

### D7 — Persistence policy is authoritative

Replace the delivery compatibility branch in `profile.rs:2244–2255`:

```diff
-    if gate.proof.kind() == Some(ProofKind::Delivery) {
-        if !gate.auto_commit {
-            if gate.persistence.post == Some(PersistenceAction::Commit) {
-                gate.persistence.post = None;
-            }
-        } else if gate.persistence.post == Some(PersistenceAction::Commit) {
-            gate.auto_commit = true;
-        } else if gate.persistence.post.is_none() {
-            gate.persistence.post = Some(PersistenceAction::Commit);
-        }
-    } else {
-        gate.auto_commit = false;
-    }
+    // Model-aware defaults are resolved once before execution.
+    // Compatibility flags may mirror the resolved hook, never override it.
+    gate.auto_commit = gate.persistence.post == Some(PersistenceAction::Commit);
```

Resolve omission by subject before this mirror operation, then remove the mirror when legacy consumers migrate. Reject Plan commit hooks rather than quietly changing them to snapshots. Decide and document the literal `none`/model shorthand grammar; absence already expresses no hook. At `engine/mod.rs:1191–1192`, determine delivery commit from the resolved post hook/model, not `gate.auto_commit`. Use one rollback-aware persistence executor for pre and post actions, with actual snapshot tree/baseline fields.

### D8 — Synchronize only scoped index paths

Replace `git.rs:624–628` exactly:

```diff
-    // Synchronize the worktree's default index with HEAD so git status remains clean
-    let _ = Command::new("git")
-        .args(["read-tree", "HEAD"])
-        .current_dir(worktree)
-        .output();
+    // Preserve unrelated staged entries; update only paths owned by this commit.
+    synchronize_committed_scope_index(worktree, scope, &head)?;
```

The proposed helper should reuse the existing index snapshot/rollback support, resolve literal scoped paths including deleted paths, and update only those entries against the new HEAD using NUL-delimited pathspec input. A suitable Git primitive is `git --literal-pathspecs reset --quiet <new-head> --pathspec-from-file=- --pathspec-file-nul`, with pathspec bytes supplied on stdin and checked status. Use existing confined/literal scope expansion; do not interpret glob metacharacters in filenames. Do not reset HEAD or the working tree. A directory scope includes its owned tracked/untracked/deleted entries, not unrelated directories.

Wrap commit, proof-ref retention, and real-index synchronization in the existing rollback/recovery boundary so any later error restores the previous HEAD/index or records a recoverable pending operation. Do not declare success after ignoring index-update errors.

### D9 — Remaining exact consumer changes

| Source interval | Required replacement |
| --- | --- |
| `store/mod.rs:1209–1211`; `store/lifecycle.rs:624–660` | Resolve first/terminal applicable node from selected track/history; remove root-length arithmetic. |
| `engine/lifecycle.rs:149–157`; `store/lifecycle.rs:1034–1049` | Resolve/validate applicable stage in the claim transaction before equality checks and persist that stage atomically. |
| `store/lifecycle.rs:559–615`; `engine/lifecycle.rs:712–727` | Dispatch by the same resolved node; do not claim one stage and build context for another. |
| `store/mod.rs:666–741`; `engine/mod.rs:1068–1074`; `engine/gates.rs:2244–2294` | Select latest accepted candidate on the actual applicable history, not latest declared snapshot gate. |
| `milestones/handoff.rs:229–353`, `:2680–2696` | Validate required accepted task history and final receipt sources; do not demand skipped gates or root boundaries as task evidence. |
| `store/recovery.rs:1849–1874`, `:1950–1975` | Replay canonical transition history with the shared graph; remove root-index dereference and reconstructed lexical next/reject. |
| `engine/gates.rs:2452–2528`; `engine/submit.rs:466–481` | One typed rejection classifier/resolver; persist its selected edge. |
| `engine/workspaces.rs:395–456` | Load actual rejected transition and destination for findings/reuse context. |
| `engine/gates.rs:1687–1795`, `:2191–2198` | Add admitted-symbol delta check alongside existing caller safety, using task symbols/extended symbols. |
| `milestones/command_gate.rs:226–237` | On unexpected failure commit a shared rejection transition after preserving observed command facts. |
| `milestones/integration.rs:244–246`, `:258–275`, `:308`, `:501–523` | Enforce declared output/count rules, complete command plan, bounded execution, and nonduplicated composite totals. |
| `engine/lifecycle.rs:204–218`; `engine/mod.rs:1261–1294` | Give generic hooks the same Git/index rollback and journal protection as delivery. |
| `engine/lifecycle.rs:283–315`; `engine/gates.rs:2620–2695` | Record actual candidate tree and canonical leaf, reject Git identity errors, persist checkpoint with transition. |
| `store/trace.rs`; milestone verification replacement | Atomically reserve attempts and accept transition/checkpoint; enforce restart/CAS semantics and no duplicate trace key reuse. |
| `store/mod.rs:1875–1938`; `store/lifecycle.rs:1025–1033`; `engine/scheduler.rs:6–127` | Connect admitted dependency readiness, automatic speculative rebase/reverification, and applicable seam barrier. |
| `milestones/handoff.rs:1677–1686`; `milestones/integration.rs:108–112` | Apply shared worker/session/candidate-author independence to root review. |
| `profile.rs:1816` onward; `tasks/gates.rs:339–366`, `:400–425`; official Plan/Milestone delivery blocks; `.forge/acdd/profile.yaml:244–246` | Validate/declare applicable final-candidate review sources, including executed seam review; reject empty rollups and consume the selected task projection during durable receipt validation. |

## Test coverage requirements

Place new coverage in existing domain modules under `tests/acdd` or narrowly split domain submodules with their owner instructions. Keep one root `tests/acdd.rs` harness. Each row describes a separate descriptively named test when it is a distinct workflow. Use parameter tables only for input variants of the same unit. Contract-gate red proof compiles cleanly and fails at runtime with the admitted assertion exit (the repository task contract uses 101), not a compiler/setup failure.

| Area / findings | Required production-seam cases and assertions |
| --- | --- |
| Model parsing (F01–F07, F13, F17, F24, F31) | Load the three official files through the real merged loader. Assert exact effective roots/tracks, no hidden verification, legal forward reject/pass edges, template expansion, strict fields, ordinary attempt-limit validation/enforcement, and model persistence. Reject obsolete fallback syntax. |
| Workflow (F02, F05–F06, F16, F24) | Run manifest-free verify→review clean path and verify→polish→verify→review repair path. Assert zero task rows/claims, no task ID requirement, no automatic commits, persisted generic cursor, and restart behavior. |
| Plan (F02, F04, F09, F24, F31) | Admit `doc_type: plan`, execute actual build/review/deliver in the active worktree, preserve HEAD and unrelated index entries, retain snapshots and step state, and never call synthesis/handoff/archive. Reject any commit hook. |
| Milestone boundaries (F01, F03–F04) | Synthesis runs once for the milestone and blocks all task claims/next until accepted. Tasks start at their track's contract/build, never root synthesis/closure. Root closure runs only after the task DAG is accepted/completed. |
| Multiple tracks (F07, F13) | Select code/doc/database/mixed admitted types; zero/overlap errors; repeated leaf names across tracks; exact canonical names and unambiguous short aliases. Wrong fully qualified paths never fall back to leaf matching. |
| Conditions (F08–F10) | Test every supported predicate and alias at runtime; unknown condition error; parent AND child; false first/middle/last; all skipped; stable type after scope extension; applicable DAG/concurrency combinations. |
| Claim/next/reset (F09, F25) | Match returned/persisted/claimed cursor in each entry point. Concurrent claims retain CAS. Park/resume affinity survives; activation/context/pre-hook failure releases or recovers the claim. Reset and completed sync use correct leaf positions. |
| Snapshot delivery (F10–F11, F27, F31) | Complete sequential skipped-seam lifecycle with real dirty candidate snapshots and handoff. Complete integrated seam lifecycle with changed files and final seam review. Reject stale/absent/wrong-candidate review and snapshot. |
| Rejection (F05–F06, F12, F14, F16) | Scope routes build; symbol routes contract; mixed finding precedence; negated prose cannot reclassify; default reject; conditional/invalid/cross-subject target; forward repair; findings/reuse context and recovery retain actual target. |
| Symbol boundaries (F15) | Admitted/unadmitted public delta, new public symbol, visibility/trait/alias/signature changes, private-only changes, admitted extension, stale revision, and caller safety/retirement regression cases. |
| Contract decision (F03, F21) | Missing, stale, malformed, wrong scope/gate/author/revision/topic and empty sections fail; valid current decision passes; both typed/inferred submits and direct store/root submit enforce it; migration retains legacy messages without granting current proof. |
| Mechanical proof (F16, F20, F22–F23) | Real registered command execution plus completed traces required. Forged JSON, wrong command/prefix, exit 2 versus expected 101, compilation failure, nonexistent seam, zero counts, stale candidate/claim, incomplete run all fail before advance. |
| Red-to-green (F23) | Contract seam genuinely runtime-red, then build executes that same seam green. Passing unrelated tests cannot advance. Cover multiple seam identities, changed contract revision, non-Rust runner, and admitted proof-policy exceptions. |
| Persistence (F24–F26) | Model × pre/post action matrix; explicit policy overrides no hidden defaults; exact commit count; snapshot/no-op leaves branch HEAD unchanged; hook failure and post-hook database failure recover both Git and store. |
| Index integrity (F26) | Existing failing staged-deletion test; separate partial-stage, unrelated staged addition/edit/delete/rename, whitespace/newline/metacharacter path, directory scope, deleted scoped path, hook failure, locked index, and synchronization failure. |
| Recovery (F12–F13, F25, F27–F28) | Export/recover each nested stage, skipped node, symbol rejection, receipt, and completed state. No panic from flat/root mismatch. Assert accepted tree/leaf/transition equality. Fault injection around every persistent boundary. |
| Autonomous closure (F17–F20) | Clean tree and source/test-only polish always require engine `check_full_repo`. Forged external success with real failing command cannot archive/prune. Clean pass skips repair; failure/review rejection use ordinary polish graph; restart does not recreate fallback fields. |
| Verification identity/counts (F19–F20) | Pinned profile, changed command/output policy/target base/candidate invalidate proof; content-identical history rewrite preserves permitted reuse. Correct stdout/stderr hashes, polyglot command plan, timeout, and exact totals without counting standalone commitment twice. |
| Reviewer independence (F11, F30) | Exclude every actual builder/integrator/polisher and same-session aliases; allow independent reviewer with no polish execution; content/profile change invalidates review; repeated review requires correct claim and candidate. |
| DAG and concurrency (F08, F29) | Ordinary waves, disjoint parallel tasks, speculative accepted upstream candidate, automatic rebase and failure, cancellation/peer rejection, stale upstream revision, applicable barrier, and no sequential barrier deadlock. |
| Handoff retry (F18, F27–F28) | Inject failures before note/archive/unlink/prune/ref cleanup; retry requires the same accepted engine proof and binding. Preserve accepted task proofs through legal history integration. No client JSON restores missing engine verification. |
| Public surface (F02, F16, F18, F21, F32) | API/CLI/MCP parity for model selection, canonical stage, decision failure, observed run, rejection, receipt, and handoff. Handler diagnostics match the documented schema; no alternate evidence-file or direct-store escape. |

Fast parser/unit coverage should remain separate from process-backed lifecycle coverage. Replace fake passing proof fixtures where the subject under test is verification integrity; retain explicitly labeled fixtures for unrelated receipt serialization tests. Assert no mutation on rejected operations: task descriptor, active claim, gate rows, checkpoint, manifest/archive, HEAD, refs, and unrelated index state as applicable.

## Repair order and acceptance

1. **Freeze authority and prove current defects.** Admit a bounded repair contract and source/docs/test ownership. Preserve all pre-existing working-tree changes. Add meaningful runtime-red seams for the acceptance defects; use the already failing index test as current evidence, not a newly invented claim of green status.
2. **Unify graph interpretation.** Repair model selection, root/task isolation, templates, selectors, conditions, success/reject edges, admission/claim/next/reset, and all lookup/recovery/receipt consumers together. Keep the official profiles executable as the acceptance fixtures.
3. **Unify proof and persistence boundaries.** Add revision-bound decisions and observed mechanical proof validation; enforce red-to-green and admitted symbols; repair snapshot selection, final review provenance, Git/index protection, and transactional checkpoint identity.
4. **Replace milestone fallback with the shared root graph.** Preserve reusable command/binding/archive machinery, enforce engine verification on every handoff/retry path, bind profile/command/count policy, and migrate or explicitly diagnose old persisted state. Do not leave a temporary external-JSON bypass while removing fallback.
5. **Close model-level tests and documentation.** Implement the missing Workflow/Plan execution paths, complete declared DAG/concurrency semantics within admitted scope, and reconcile profile examples, shared skill, reference/API/CLI/MCP docs, architecture, runbooks, and root handoff instructions. The current root procedure's externally supplied verification fields must agree with the new engine-owned proof contract. Correct the source plan's disproved index-preservation claim.

Use task-scoped commands from the admitted profile during implementation. At final repository acceptance, follow [TESTS.md](../../TESTS.md): commitment integrity, strict Clippy, and all-targets tests, with an exact passed/failed count using its composite-count rules. Rerun the complete ACDD suite, including the existing index failure. Validate the three official profiles through the actual loader and execute their complete lifecycles. A passing parser result, unit-only suite, or review JSON is insufficient evidence.

This document proposes repairs and coverage; it does not admit milestone tasks, change ADR decisions, certify a bug-free engine, or claim any production remediation is complete. The observed baseline is **15/15 crate unit tests and 166/167 ACDD tests passing**, with multiple separately reproduced lifecycle/proof defects that current tests do not cover.
