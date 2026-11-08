---
title: "Test Suite Hardening — Reality Reset"
doc_type: plan
---

# Test Suite Hardening — Reality Reset

Replace the previous T1–T17 diet plan. That plan optimized for vanity line
targets, invented absolute speed promises, and pushed agents to move plumbing
into ever-larger harnesses. This plan starts from measured branch state and
keeps only changes that reduce cost, duplication, or diagnosis time.

## 1. Current branch state (`diet/base`)

Measured on `diet/base` on 2026-11-07 after freezing the current edits. Line
counts are `.rs` files under each path; the direct `tests/*.rs` files are listed
as one row.

| Area | Lines of `.rs` | Notes |
| --- | ---: | --- |
| `tests/` total | 105,649 | Direct `tests/*.rs` files: 3,020 lines |
| `tests/acdd/` | 35,498 | `mcp.rs` ~5.3k; `tasks/lifecycle.rs` ~3.7k |
| `tests/languages/` | 26,997 | |
| `tests/mcp/` | 14,415 | Direct `tests/mcp.rs`: 56 lines |
| `tests/framework_manifests/` | 7,494 | |
| `tests/query_context/` | 4,664 | |
| `tests/receivers/` | 3,292 | |
| `tests/core/` | 2,852 | |
| `tests/linker/` | 2,764 | |
| `tests/incremental/` | 2,540 | |
| `tests/common/` | 1,391 | Shared substrate exists; underused |
| `tests/commitment_integrity/` | 660 | |
| `tests/mcp_context/` | 62 | |
| `TaskHarness` | 1,358 / 96 methods | `tests/acdd/tasks/harness.rs` |
| `MilestoneHarness` (+ submodules) | 2,258 | `tests/acdd/milestones/` harness files |

The suite contains 1,286 literal `#[test]` declarations (`rg -c '#\[test\]'`;
this count does not include differently-spelled test attributes).

Focused command baselines below were measured with `time cargo test --test …`;
wall time includes any Cargo compilation performed by that invocation and is a
same-machine observation, not a benchmark claim.

| Command | Passed / failed | Wall time |
| --- | ---: | ---: |
| `cargo test --test acdd` | 335 / 0 | 2m 35.990s |
| `cargo test --test mcp` | 164 / 0 | 34.698s |
| `cargo test --test languages` | 403 / 0 | 2m 20.860s |
| `cargo test --test framework_manifests` | 40 / 0 | 54.494s |

One-caller inventory (Forge call edges, cross-checked against source; call-edge
counts are provisional where source/AST finds indirect calls):

- `TaskHarness`: 96 methods; graph reports 18 with no test edge, 38 with one,
  and 40 with two or more. Source inspection confirms no direct test caller for
  `git_output`, `persisted_gate_proof`, `persisted_submission_proof`,
  `persisted_submission_evidence`, `schema_object_count`, `schema_objects`,
  `schema_version`, `tombstone_count`, and `submit`.
- `MilestoneHarness` family: 171 methods/functions; graph reports 51 with no
  direct test edge, 46 with one, and 74 with two or more. Source inspection
  identified these single-test facade candidates: `manage_at`, `create_task`,
  `scan_paths`, `extend_scope`, `active_path`, `read_task_messages`,
  `store_for_task`, and `handoff_context_at`.

Useful work already landed (keep and finish):

- Domain splits under `tests/acdd/{milestones,tasks}/`, `tests/mcp/`, languages.
- In-process engine paths for many milestone/task flows; CLI/stdio no longer the
  default matrix driver.
- `MilestoneFixtureBuilder` and fixture templates instead of raw heredoc copies.
- Shared seed / snapshot patterns in some rejection matrices.
- `McpTestHarness` / in-process tool calls for most MCP tool tests; `StdioClient`
  mostly confined to transport smoke.
- In-memory extraction helpers for parts of languages / frameworks.
- Some one-caller helpers returned to tests (recent “return one-caller” commits).

Failed or harmful plan effects (stop repeating):

- Absolute line budgets (`milestones < 2k`, `mcp < 3.5k`, `languages < 8k`).
  Agents satisfied them by relocating code into harnesses; total suite grew.
- Absolute wall-time slogans (“sub-second loop”, “L1 < 5s for cargo test”).
- Treating `#[ignore]` / env filters as a substitute for shared setup.
- Premature `PlanHarness` / `WorkflowHarness` expansion ahead of AC-003 runtime
  (thin Markdown extract fixtures are enough until the engine exists).
- Duplicating the same three-layer essay across plan, architecture, `tests/AGENTS.md`,
  and the global `test-suite-refactor` skill.

## 2. Goals (measurable, non-vanity)

1. **Cost**: reduce default-lane wall time for the suites agents actually run
   during task work (`acdd`, `mcp`, focused owner binaries). Record before/after
   on the same machine; do not invent absolute targets from the old audit table.
2. **Duplication**: unique `#[test]` count for a cluster must not rise unless an
   `added` row names a new observable dimension. Prefer delete/collapse over wrap.
3. **Diagnosis**: one scenario per `#[test]` (or one table of input variations);
   assertions stay in the test; helpers must have ≥2 callers.
4. **Harness size**: shrink god-harnesses by inlining one-caller methods and
   promoting true cross-suite plumbing to `tests/common/`. Harness line count
   falling while scenario files stay huge is a failure mode, not success.
5. **Seams**: keep real production paths. Do not replace engine/Git/SQLite seams
   with mockist unit doubles to chase speed.

Non-goals:

- Matching arbitrary historical line counts.
- Building orchestrator plan/workflow execution harnesses before AC-003 admits them.
- Hiding slow genuine Git/history/scale cases behind silent filters without an
  explicit lane and a documented reason.

## 3. Standing rules (carry into code, not essays)

These are the only durable process rules. Detail lives in
`docs/architecture/test-harness.md` and `tests/AGENTS.md`.

1. **Replace then delete** — never keep the weak duplicate beside the stronger seam.
2. **Two-caller rule** — one caller ⇒ scenario code in the test.
3. **Assertions in the test** — harness returns values / `Result`; it does not
   `assert!` the contract.
4. **Shared seed, single mutation** — rejection/validation matrices build the
   expensive base once, then mutate one field per row.
5. **In-process by default** — `milestone_engine::*`, `tasks::*`, in-process MCP
   handlers. Reserve `Command` / `StdioClient` / `mkfifo` for dedicated smoke tests.
6. **In-memory when the seam allows** — pure store/state/AST cases must not open
   tempdirs or Git repos.
7. **Real lanes only** — keep registered scenarios in the default lane. Add an
   opt-in lane only when a code-backed gate protects a genuine expensive seam
   after shared setup; never reference an unimplemented environment switch.
8. **No harness theater** — do not extract a method to hit a line-count story.

## 4. Sequential work plan

Execute phases in order. Do not open a later phase while the previous exit
criteria fail. Stop Codex agents that are still chasing T11–T17 line budgets;
retarget them to the active phase below.

### Phase A — Freeze and rebaseline (½–1 day)

1. Stop new “diet to N lines” tasks.
2. Capture a dated baseline on `diet/base` (recorded above): line totals by
   top-level suite, literal test declaration count, wall times for the ACDD/MCP
   owner binaries and focused languages/frameworks binaries, and a Forge/source
   one-caller inventory for `TaskHarness` and `MilestoneHarness`.
3. Write the baseline numbers into this plan’s status table (replace estimates).
4. Pause worktrees whose only acceptance criterion is a vanity line target;
   merge only changes that already satisfy Phase B/C rules.

**Exit**: baseline numbers committed into this plan; agents pointed here.

### Phase B — Harness surgery before more scenario moves (2–4 days)

Run disjoint harness owners in parallel: `tests/acdd/tasks/harness.rs`,
`tests/acdd/milestones/harness*`, and `tests/mcp/harness*` /
`tests/common/mcp_harness.rs`. Keep their caller edits inside their own suite
directories. Refactor `tests/acdd/support.rs` after the two ACDD harness owners
finish, because it can intersect both caller migrations. Phase C stays closed
until every Phase B owner passes its exit criteria.

For each harness method:

| Finding | Action |
| --- | --- |
| One distinct test caller | Inline body into that test; delete method |
| Two+ callers, same operation, different flag | One parameterized method |
| Cross-suite workspace / DB / MCP transport | Move to `tests/common/` |
| Named after a single test / mirrors a scenario | Inline or rename to a domain verb |
| Contains `assert!` on contract outcomes | Return value; move assert to test |

Do **not** add new scenario wrappers. Do **not** create parallel harnesses.

**Exit**:

- Material drop in method count on `TaskHarness` / `MilestoneHarness`
- No new one-caller helpers introduced in the same diff
- Owner suite still green (`cargo test --test acdd …` scoped as needed)

### Phase C — Delete and collapse the fattest scenario files (ongoing, one owner per file cluster)

Current priority by post-Phase-B size and duplication smell:

1. `tests/acdd/mcp.rs` (5,267 lines; 23 `#[test]` declarations)
2. `tests/acdd/tasks/lifecycle.rs` (3,932 lines; 86 declarations)
3. `tests/mcp/context.rs` (3,740 lines; 39 declarations)
4. `tests/mcp/tool_evolution/mcp_boundaries.rs` (2,774 lines; 22 declarations)
5. `tests/acdd/runner.rs` (2,328 lines; 46 declarations)
6. Remaining large `milestones/{discovery,handoff,lifecycle,history}.rs`
7. Large languages/query_context files only after ACDD/MCP cost is under control

Per file cluster:

1. Inventory unique observables (KEEP / REWRITE / REMOVE / UNCERTAIN).
2. Keep the strongest production-seam test; table-drive input variations.
3. Delete subsumed tests in the **same** diff.
4. Prefer shared seed + one mutation over N full lifecycles.
5. Split a file only after deletion, and only on a real domain boundary.

Run separate Phase C owners concurrently only when their write paths are
disjoint. Each owner handles one file cluster; for clusters sharing a Cargo
test binary, wait until the active edits settle before running the owner suite.
The 2026-11-07 Phase C starting counts above are each file's literal `#[test]`
declaration baseline; do not increase them without an `added` row naming a new
observable dimension.

**Exit per cluster**: unique tests ≤ baseline (except justified `added` rows);
owner suite wall time not worse; replacement map recorded in the delivery notes.

### Phase D — Finish transport and fixture discipline (after C starts paying off)

1. MCP: keep OS stdio / `mkfifo` exclusively under dedicated transport smoke
   (`tests/mcp/transport.rs` and true special-file probes). Tool contract tests
   stay on `McpTestHarness` / in-process handlers.
2. ACDD: finish eliminating residual `Command::new` from domain matrices; leave
   1–2 CLI smoke paths.
3. Fixtures: no new multi-line milestone/plan heredocs in tests; use templates /
   builders already in tree.
4. Languages / frameworks: share parse/index setup across table rows; do not
   rebuild full workspaces for pure AST cases.

**Exit**: smell counts for `StdioClient` / `Command::new` / tempdir in domain
suites are smoke-only; default agent loops no longer spawn child servers for
ordinary tool assertions.

### Phase E — Lanes for what remains legitimately expensive

Only after Phases B–D remove redundant setup:

1. Re-measure the slowest remaining tests.
2. Keep default lane on in-process + shared seed paths.
3. Keep registered scenarios in the default lane unless a genuine expensive
   history, crash/journal, or large-scale seam remains after setup reduction and
   has a code-backed opt-in gate plus a fast L1 production-seam proof.
4. Document only lane commands implemented in code; state what each lane proves
   and record passed, failed, and ignored counts.

**Exit**: default lane free of known redundant full-lifecycle matrices; gated
tests named and justified.

### Phase F — Documentation and skill sync (parallel, keep thin)

1. Keep `docs/architecture/test-harness.md` as the short rule sheet (not a second
   plan, not a catalog of aspirational harnesses).
2. Keep `tests/AGENTS.md` as placement + invariants; link the architecture page
   instead of restating it.
3. Keep `test-suite-refactor` focused on inventory → replace/delete → split →
   verify. Do not embed repository line budgets or ASCII layer propaganda.
4. Do not expand Plan/Workflow harnesses beyond current Markdown/profile extract
   fixtures until AC-003 admits execution seams.

## 5. What to ignore from the old plan

| Old claim | Disposition |
| --- | --- |
| T11 milestones `< 2,000` lines | Drop. Measure cost and harness callers instead. |
| T12 tasks `< 5,000` lines | Drop. Collapse lifecycle/mcp duplication first. |
| T13 mcp `< 3,500` lines | Drop. Finish in-process + delete duplicate tool scenarios. |
| T14 languages `< 8,000` / `< 20s` | Drop absolute numbers; keep in-memory AST discipline. |
| T15 frameworks `< 10s` | Re-measure after shared setup; no slogan target. |
| T16 Plan/Workflow skeletons for AC-003 | Keep minimal fixtures only; no execution harness sprawl. |
| T17 “universal profile traversal” as a mega-task | Keep `advance_to` / session isolation where they already help; do not rewrite the suite around it. |
| Old 15–20s inventory table as current truth | Historical only; rebaseline in Phase A. |
| Fake gating via `.filter` / ignore | Remains forbidden. |
| Mechanical 500/800-line cap on single-domain files | Drop blanket line-count splits. Wave 5 decomposes ONLY multi-subsystem monoliths (`tests/acdd/mcp.rs`, `tests/mcp/context.rs`, `tests/mcp/tool_evolution/mcp_boundaries.rs`), shared fixture infrastructure (`tests/acdd/support/*`), and `tests/commitment_integrity/*`, while preserving cohesive single-domain test files (`test_read.rs`, `freshness/*`, `languages/*`, `query_context/*`, `linker/*`, `core/*`) as single files without mechanical line-count splitting. |

## 6. Status

| Phase | Status |
| --- | --- |
| A Freeze and rebaseline | **Complete — baseline captured 2026-11-07** |
| B Harness surgery | **Complete — all four owner suites passed on 2026-11-07** |
| C Delete/collapse fat files | **Complete — all declared priority clusters and recorded follow-up candidates have dispositions and passing owner-suite evidence** |
| D Transport/fixture discipline | **Complete — repo-wide process/transport hits were classified; domain MCP calls use in-process handlers** |
| E Real lanes | **Complete — final `cargo test --all-targets` passed 1,153 / 0 / 0 ignored after C/D work** |
| F Docs/skills thin sync | **Complete — architecture and test guidance aligned; docs check passed** |

Phase B owner results (2026-11-07): TaskHarness **96→61 methods / 1,358→1,011
lines**, with 150/0 task tests; MilestoneHarness **171→167 methods**, with
83/0 milestone tests; ACDD support **85→79 declarations / 1,925→1,853 lines**,
with the full ACDD suite at 335/0; MCP harness surgery removed 31 one-test
helpers, with the MCP suite at 164/0. No tests were added or removed in these
Phase B slices. Each owner reported `git diff --check` exit 0. The MCP owner also
reported target-harness `rustfmt --check` passing; the full caller formatting
check still has pre-existing differences in dirty files.

Phase C results (2026-11-07; eighteen completed clusters, including one
additional verification slice):

| File | Start lines / raw `#[test]` tokens | Current lines / raw `#[test]` tokens |
| --- | ---: | ---: |
| `tests/acdd/mcp.rs` | 5,267 / 23 | 4,967 / 22 |
| `tests/acdd/tasks/lifecycle.rs` | 3,932 / 86 | 3,875 / 75 |
| `tests/mcp/context.rs` | 3,740 / 39 | 3,734 / 38 |
| `tests/mcp/tool_evolution/mcp_boundaries.rs` | 2,774 / 22 | 2,782 / 21 |
| `tests/acdd/runner.rs` | 2,328 / 46 | 2,300 / 41 |
| `tests/acdd/milestones/discovery.rs` | 1,098 / 33 | 1,081 / 28 |
| `tests/acdd/milestones/handoff.rs` | 1,074 / 10 | 1,054 / 9 |
| `tests/acdd/milestones/lifecycle.rs` | 980 / 22 | 924 / 18 |
| `tests/acdd/milestones/verification.rs` (additional wave slice) | 411 / 4 | 405 / 3 |
| `tests/acdd/milestones/history.rs` | 811 / 2 | 811 / 2 |
| `tests/query_context/ranking_and_coverage.rs` | 1,582 / 14 | 1,493 / 11 |
| `tests/query_context/symbol_context.rs` | 1,264 / 14 | 1,244 / 12 |
| `tests/query_context/diagnostics_and_source.rs` | 1,266 / 9 | 1,228 / 9 |
| `tests/languages/python/modules.rs` | 1,508 / 27 | 1,504 / 22 |
| `tests/languages/typescript/modules.rs` | 1,114 / 22 | 1,099 / 21 |
| `tests/languages/typescript/grammar.rs` | 1,042 / 30 | 1,037 / 27 |
| `tests/languages/typescript/dom.rs` | 986 / 22 | 960 / 22 |
| HTML syntax/assets/HTMX/classic-wire files | 1,958 / 26 | 1,957 / 26 |

The 17 slices with frozen Phase C starting counts now net **674 lines and 43
raw `#[test]` tokens** removed. In `runner.rs`, six tokens are fixture-string
text; Forge AST confirms 35 actual declarations now, down from 40. The
additional verification slice reduced its wave-entry count from **411 / 4** to
**405 / 3**. These edits retain the inventory's distinct observables. The
latest `cargo test --test acdd` passed **307/0** in 134.87s test time; Phase A
logged **335/0** in 2m35.990s wall time, so those timing measures are not
directly comparable. `cargo test --test mcp` passed **162/0** in 30.61s, versus
**164/0** in 34.698s. `cargo test --test query_context` passed **39/0**
in 9.571s wall / 6.39s test time; Phase A did not record a query-context wall
baseline. `cargo test --test languages` passed **394/0** in 2m16.548s wall /
130.59s test time, versus the Phase A baseline of **403/0** in 2m20.860s.
The task-lifecycle follow-up removed five duplicate declarations and retains
all distinct session, status/history, and submission invariants. The milestone
lifecycle follow-up collapsed two same-seam activation variants while keeping
the CRLF input, guidance, active status, and timestamp-recency checks. The
verification follow-up reused one fixture/snapshot across command success and
failure while keeping both assertion sets. The MCP context follow-up removed
one repeated path assertion; its cold comparisons remain because each
exercises a distinct persisted transition. The delivery and receipts milestone
slices were audited and left unchanged because their assertions cover distinct
handoff and receipt seams. `history.rs` remains unchanged because all 11 and 16
matrix rows execute and assert separate invariants. The mutation-payload case
retains all six actions;
its `sync_many` case now crosses the response cap at 63 persisted tasks, and its
focused run passes in 19.76s with the 1,200-repeat invariant payload. Strict
Clippy (`cargo clippy --all-targets --all-features -- -D warnings`) and
`git diff --check` pass. The language slice is complete.

Phase D results (2026-11-07; audited slices):

| File / seam | Start lines / declarations | Current lines / declarations | Result |
| --- | ---: | ---: | --- |
| `tests/acdd/mcp.rs` | 4,991 / 22 | 4,502 / 22 | −489 lines; tool assertions use `McpTestHarness` / engine paths; one environment-session stdio smoke remains |
| `tests/acdd/tasks/workspaces/process.rs` | 334 / 0 | 325 / 0 | Rewind recovery uses a real ref-lock fault and in-process `db-recover`; delivery SIGKILL and child-only Git-GC process cases remain |
| `tests/framework_manifests/finite_operations/callable_imports.rs` | 613 / 1 | 637 / 1 | Both descriptor variants share one persisted workspace; the second uses production delta and cold-build parity |

The context FIFO/directory/symlink probe remains in `tests/mcp/context.rs`:
`McpTestHarness` dispatches directly to the production server handler, and this
case proves special-file path behavior rather than transport. Forge AST search
finds `StdioClient` only in `tests/mcp/transport.rs` and the single ACDD
environment-session smoke; the only ACDD `Command::new(env!(...))` calls are
that smoke and the delivery process-boundary path. The remaining Git, rustc,
FIFO, `setsid`, and `kill` calls are confined to snapshot, compile-proof, or
dedicated process tests. Language syntax cases use `ExtractorHarness`; persisted
HTML relation tests retain disk because their contract is the production
filesystem/index seam.

This wave's separate `cargo test --test acdd` run passed **307/0** in 134.87s
test time, and `cargo test --test mcp` passed **162/0** in 30.61s. Strict Clippy
passed. `cargo test --all-targets` exited 0; its per-target summaries total
**1,188 passed, 0 failed, 0 ignored**. `git diff --check` passed. The framework
suite passed **40/0** in 55.43s
test time / 57.877s wall; Phase A recorded 54.494s wall for that suite, so this
single observation does not establish a runtime improvement despite removing
one repeated persisted-workspace setup. Re-measure with the next Phase E lane
pass before claiming a timing gain. Full-file rustfmt checks on the already-dirty
`history.rs`, `handoff.rs`, `verification.rs`, and MCP boundaries file reported
formatting drift in untouched regions; those files were not globally reformatted.

Additional Phase C follow-up slices (2026-11-07):

| File | Wave entry: lines / raw `#[test]` tokens | Current: lines / raw `#[test]` tokens | Result |
| --- | ---: | ---: | --- |
| `tests/acdd/mcp.rs` | 4,488 / 21 | 4,485 / 21 | Removed one status inequality already implied by the table's exact `cancelled` row. |
| `tests/mcp/context.rs` | 3,734 / 38 | 3,735 / 38 | Canonicalized each fixture root once before both selector requests; retained isolated FIFO, directory, and symlink cases. |
| `tests/mcp/tool_evolution/mcp_boundaries.rs` | 2,786 / 21 | 2,780 / 21 | Folded duplicate write-SQL inputs into the safety matrix and checked persisted SQLite state for each; removed a repeated multi-statement row. |
| `tests/acdd/runner.rs` | 2,270 / 40 | 2,270 / 40 | No safe deletion: the two Cargo identity rejection predicates retain separate tests and diagnostics. |

These four bounded passes reduced the files by a net **8 lines** and kept all
120 literal `#[test]` tokens. The context slice removes repeated filesystem
canonicalization rather than source lines. Follow-up verification passed:
`cargo test --test acdd` **299/0** in 147.10s test time,
`cargo test --test mcp` **156/0** in 32.21s,
`cargo test --test commitment_integrity` **17/0** in 27.70s, and strict Clippy
in 7.21s. `cargo test --all-targets` passed **1,172/0**; the separate
`cargo test -p forge-tasks --lib` gate passed **34/0** in 15.14s. These are
observed command times, not a benchmark or a speedup claim.

Active Codex worktrees under `.worktrees/diet-*` must be retargeted to the
current phase or paused. Do not merge further “diet to N lines” acceptance
criteria.

### Additional Phase C follow-up — 2026-11-07

Wave-entry counts below were measured from the live worktree immediately before assignment; earlier frozen baselines above remain unchanged.

| File | Wave entry: lines / raw `#[test]` tokens | Current: lines / raw `#[test]` tokens | Change |
| --- | ---: | ---: | --- |
| `tests/acdd/milestones/discovery.rs` | 1,081 / 28 | 1,067 / 28 | Removed an in-process malformed-evidence row already covered by the CLI smoke's diagnostic and exit-code assertion. |
| `tests/acdd/milestones/handoff.rs` | 1,054 / 9 | 1,044 / 9 | Removed the combined status-swap row; single-status mutations retain both rejection predicates. |
| `tests/acdd/tasks/lifecycle.rs` | 3,878 / 73 | 3,872 / 73 | Replaced a raw gate-evidence query with the existing typed harness reader; retained all distinct lifecycle cases. |
| `tests/query_context/ranking_and_coverage.rs` | 1,582 / 15 | 1,506 / 14 | Removed one subsumed wildcard query and combined equivalent compact-coverage checks; kept distinct ranking contracts separate. |
| `tests/query_context/diagnostics_and_source.rs` | 1,225 / 9 | 1,236 / 9 | Reused one persisted resolver seed and removed 214 redundant coverage-row inserts (328 → 114); retained all three summary results. |
| `tests/languages/python/modules.rs` | 1,504 / 22 | 1,546 / 22 | Shared one in-memory link across six isolated provider variants and hoisted duplicate syntax extraction; the setup grows while avoiding repeated linker passes. |

The affected owner suites passed: `cargo test --test acdd` **299/0** (148.73s test time), `cargo test --test query_context` **41/0** (6.54s), and `cargo test --test languages` **393/0** (139.94s). Strict Clippy (`cargo clippy --all-targets --all-features -- -D warnings`) and `git diff --check` passed. This wave records test-runner times, not wall-time measurements; it makes no speedup claim.

The slowest ACDD history case was separately remeasured with `time cargo test --test acdd acdd::milestones::history::handoff_preserves_proofs_across_reviewed_history_rewrites -- --exact`: **1/0**, 31.01s test time and 31.283s wall time. It already uses one delivered baseline and same-workspace `FixtureSnapshot`; the rows remain in the default lane, with no remaining repeated lifecycle setup identified in this pass.

### Additional Phase C follow-up — 2026-11-07 (parallel wave)

Counts are the live worktree at assignment and after the edit. Declaration counts came from Forge AST (including `#[tokio::test]` where present).

| File | Wave entry: lines / test declarations | Current: lines / test declarations | Change |
| --- | ---: | ---: | --- |
| `tests/acdd/mcp.rs` | 4,480 / 19 | 4,468 / 19 | Removed an unpaginated all-repositories request; compare CLI sync IDs against paginated MCP responses instead. |
| `tests/acdd/tasks/lifecycle.rs` | 3,872 / 73 | 3,879 / 73 | Reused one harness lifecycle for contract, command, and review rejection checks; removed repeated resets and a replayed gate pass. |
| `tests/mcp/context.rs` | 3,735 / 37 | 3,732 / 36 | Combined adapter response-policy bounds on one live-reload fixture; retained all eight invalid settings. |
| `tests/mcp/tool_evolution/mcp_boundaries.rs` | 2,780 / 21 | 2,758 / 21 | Moved duplicate invalid-pattern and traversal inputs into the existing validation table. |
| `tests/acdd/tasks/guidance.rs` | 1,601 / 11 | 1,606 / 10 | One claim now checks the shared task, milestone, and cross-gate blackboard bundle; review notes remain excluded. |
| `tests/mcp/test_read.rs` | 1,134 / 21 | 1,112 / 20 | Folded the indexed enclosing-symbol mapping into the repeated-method MCP/source selection test. |

These six clusters net **47 lines and 3 test declarations removed**. The shared-workspace milestone topology matrix initially exposed two missing-database snapshot assumptions: the fixture now creates and closes the empty SQLite store before capture, and `FixtureSnapshot::restore` recreates a scenario-deleted store from its captured bytes after validating the path. The matrix still runs every row in the same workspace.

The final affected-suite runs passed: `cargo test --test acdd` **298/0** in **143.88s test time / 2m28.620s wall**, and `cargo test --test mcp` **154/0** in **33.49s test time / 38.845s wall**. Strict Clippy passed in **5.089s wall**; `git diff --check` passed. No tests were ignored or filtered. These are observed run times, not a benchmark or a speedup claim.

### Additional Phase C follow-up — 2026-11-07 (task/MCP/core wave)

Counts are the live worktree immediately before assignment and after the edits; test declarations are Forge AST counts.

| File | Wave entry: lines / declarations | Current: lines / declarations | Change |
| --- | ---: | ---: | --- |
| `tests/acdd/tasks/boundaries.rs` | 846 / 7 | 845 / 7 | Table-drove 13 commit-scope inputs with labels and retained each expected predicate. |
| `tests/acdd/tasks/seams.rs` | 1,039 / 7 | 1,034 / 7 | Two stale-file mutations reuse one claimed worktree; each still rebuilds its local index and checks rejection. |
| `tests/acdd/tasks/subtasks.rs` | 857 / 8 | 857 / 8 | No new change; the overlapping invalid-status assertion was already removed in the inherited diff. |
| `tests/acdd/tasks/workspaces/snapshot_recovery.rs` | 1,360 / 4 | 1,359 / 4 | Removed an overwritten no-op commit assignment; inherited changes already combine same-seed recovery checks. |
| `tests/mcp/freshness/freshness_and_admission.rs` | 890 / 22 | 887 / 21 | Source edit and add/rename/delete freshness checks share one workspace/server and cache refresh. |
| `tests/core/storage_batches.rs` | 1,065 / 6 | 1,061 / 6 | Bundled-manifest digest setup now uses the in-memory registry API, dropping the temporary workspace and scanner. |

These six assignments net **14 lines and 1 test declaration removed**. The unaffected subtask file was left intact. The owner suites passed: `cargo test --test acdd` **298/0** in **139.32s test time / 2m25.799s wall**; `cargo test --test mcp` **153/0** in **34.45s test time / 38.841s wall**; and `cargo test --test core` **49/0** in **8.49s test time / 11.039s wall**. Strict Clippy passed in **3.388s wall** and `git diff --check` passed. No tests were ignored or filtered. These are observed runs, not benchmarks or speedup claims.

### Additional Phase C / D follow-up — 2026-11-07 (ACDD MCP, runner, transport audit)

| File | Wave entry: lines / Forge AST tests | Current: lines / Forge AST tests | Change |
| --- | ---: | ---: | --- |
| `tests/acdd/mcp.rs` | 4,468 / 19 | 4,460 / 18 | Merged malformed evidence rows into the cached command verification/submission flow, retaining all five errors and the no-trace assertion. |
| `tests/acdd/runner.rs` | 2,270 / 34 | 2,269 / 34 | Removed one duplicate count-only `printf` input; kept the stronger precompiled-runner existence assertion and separate Cargo identity predicates. |
| `tests/mcp/freshness/stdio_and_response.rs` | 1,675 / 13 | 1,675 / 13 | No new edit: all tool contracts already use `McpTestHarness` direct mode; no stdio client, process, FIFO, or duplex transport call remains in this file. |

The ACDD change netted **9 lines and 1 test declaration removed**; the stdio audit found no remaining migration in that file. `cargo test --test acdd` passed **297/0** in **141.67s test time / 2m28.576s wall**. Strict Clippy passed in **4.242s wall**, and `git diff --check` passed. The wall time varied from the previous ACDD run, so this is recorded without a speedup claim. No tests were ignored or filtered.

### Additional Phase C follow-up — 2026-11-07 (remaining ACDD cases)

| File | Wave entry: lines / Forge AST tests | Current: lines / Forge AST tests | Change |
| --- | ---: | ---: | --- |
| `tests/acdd/gates_ast.rs` | 1,314 / 17 | 1,310 / 17 | Removed a parse-only profile row subsumed by the stronger document validation of the same example. |
| `tests/acdd/blackboard.rs` | 725 / 8 | 725 / 8 | No-active and ambiguous-milestone resolution now reuse one workspace while retaining both errors. |
| `tests/acdd/tasks/workspaces/confinement.rs` | 772 / 5 | 762 / 5 | Table-drove 15 path-to-project results and three non-alias checks; all six sync identity assertions remain. |

This wave netted **14 lines removed**, with no test declaration changes. `cargo test --test acdd` passed **297/0** in **142.74s test time / 2m28.887s wall**; strict Clippy passed in **3.882s wall**; `git diff --check` passed. No cases were ignored or filtered. Wall time remains variable and no speedup is claimed.

Final full-tree verification after these waves: `cargo test --all-targets` passed **1,169/0/0 ignored** in **7m29.422s wall**. Strict Clippy remained green, and the final `git diff --check` passed.

### Additional Phase C follow-up — 2026-11-07 (parallel ACDD/MCP audit)

Counts are the live worktree immediately before and after this assignment wave; declaration counts came from Forge AST. The ACDD MCP, task lifecycle, guidance, and snapshot-recovery files were audited without new edits because their remaining observables were distinct and inherited diffs already removed proven duplicates.

| File | Wave entry: lines / declarations | Current: lines / declarations | Change |
| --- | ---: | ---: | --- |
| `tests/acdd/mcp.rs` | 4,460 / 18 | 4,460 / 18 | No safe duplicate remained; preserved all distinct MCP, persistence, CLI smoke, session, and concurrency observables. |
| `tests/acdd/tasks/lifecycle.rs` | 3,879 / 73 | 3,879 / 73 | No safe duplicate remained; selector, status/history, pagination, session, submission, persistence, and race cases have distinct observables. |
| `tests/acdd/tasks/guidance.rs` | 1,606 / 10 | 1,606 / 10 | No further duplicate after the inherited claim-context and missing-guidance consolidation. |
| `tests/acdd/tasks/workspaces/snapshot_recovery.rs` | 1,359 / 4 | 1,359 / 4 | No safe duplicate remained; checkpoint ownership is bound to each canonical workspace, so fixtures remain isolated. |
| `tests/mcp/context.rs` | 3,732 / 36 | 3,709 / 36 | Removed a repeated lower-level `target` assertion already proved by the retained in-process MCP query. |
| `tests/mcp/tool_evolution/mcp_boundaries.rs` | 2,758 / 21 | 2,715 / 21 | Removed repeated malformed-pattern and document-rendering assertions, folded the missing-language diagnostic into the AST diagnostic test, and kept the unique document validation seam. |

The two MCP edits net **66 lines removed** with no net change to test declarations. The `mcp` owner suite passed **153/0** in **32.13s test time**; strict Clippy passed in **3.19s**; `git diff --check` passed. No tests were ignored or filtered. These are observed test-runner times, not a benchmark or a speedup claim.

### Additional Phase C follow-up — 2026-11-07 (languages/query-context wave)

Counts are live worktree measurements immediately before and after this wave; declaration counts came from Forge AST.

| File | Wave entry: lines / declarations | Current: lines / declarations | Change |
| --- | ---: | ---: | --- |
| `tests/languages/python/frameworks.rs` | 1,056 / 15 | 1,037 / 11 | Replaced five import-provenance tests with one in-memory linker table while retaining all accepted/rejected cases; persisted alias and delta tests remain. |
| `tests/languages/rust/workspace_factories.rs` | 719 / 15 | 679 / 11 | Consolidated equivalent Cargo policy, tuple-factory, alias, and module-visibility cases; inlined three one-caller fixture helpers. |
| `tests/languages/python/semantics.rs` | 614 / 16 | 661 / 14 | Merged duplicate persisted logging-origin and loop binding cases; other cross-file semantic checks now use the in-memory linker, while one persistence test remains. |
| `tests/languages/html/django_templates/custom_file_rules.rs` | 603 / 1 | 603 / 1 | No safe deletion: the single test proves custom rule discovery and persisted filesystem/index behavior. |
| `tests/languages/html/django_templates/rendered_facts.rs` | 547 / 1 | 544 / 1 | Removed a duplicate imported-alias fixture and a final unchanged-state cold build already covered by parity assertions. |
| `tests/query_context/search_and_paging.rs` | 552 / 7 | 552 / 7 | No edit; kept one `UNCERTAIN` Rust-prefix scenario because the fabricated index value’s production reachability is not established. |

The language/query-context changes net **15 lines and 10 test declarations removed**. The owner suites passed: `cargo test --test languages` **383/0** in **139.69s test time** and `cargo test --test query_context` **41/0** in **6.02s test time**. Strict Clippy passed in **3.38s**, and `git diff --check` passed. No tests were ignored or filtered. These are observed test-runner times, not a benchmark or a speedup claim; the language suite duration was effectively unchanged from its prior observation despite fewer declarations.

### Final verification after the Phase C follow-up waves — 2026-11-07

`cargo test --all-targets` passed **1,159 tests, 0 failed, 0 ignored** (including 18 library unit tests; integration targets: ACDD 297, CLI roots 7, commitment integrity 17, core 49, docs check 9, framework manifests 40, impact context 4, incremental 31, languages 383, linked workspaces 3, linker 107, MCP 153, and query context 41). Strict `cargo clippy --all-targets --all-features -- -D warnings` passed after the implementation waves, and `git diff --check` passed after this entry was written. The all-targets wall duration was not separately measured, so no total runtime or speedup claim is made.

### Additional Phase C / D follow-up — 2026-11-07 (incremental, manifests, commitment audit)

| File | Wave entry: lines / tests | Current: lines / tests | Result |
| --- | ---: | ---: | --- |
| `tests/incremental/directory_slice.rs` | 799 / 3 | 782 / 3 | Removed repeated inbound/outbound module-presence assertions already proved immediately above; kept directory identity, topology, paging, edits, rename, atomicity, and ordering assertions. |
| `tests/framework_manifests/finite_operations/named_binding.rs` | 753 / 1 | 768 / 1 | Moved pure AST extraction of unchanged fixture text out of repeated persisted phases and rename passes; retained persisted graph, resolver, mutation, manifest, path, commitment, and delta checks. |
| `tests/commitment_integrity.rs` plus `tests/commitment_integrity/` | 2,071 / 17 | 2,047 / 14 | Replaced four repeated missing-reference tests with one nine-case seal matrix over a shared in-memory schema and rollback-isolated mutations; the coverage path/expression/evidence cases also share one sealed seed. Removed the obsolete scenario-only mutation helper. |

The three edited clusters net **26 lines and 3 test declarations removed**. The separate in-place history-snapshot audit confirmed that `tests/acdd/milestones/history.rs` already captures each delivered lifecycle once, restores through `FixtureSnapshot` at the original workspace path, and retains all 27 matrix rows and 58 assertion calls. Audits of Python modules, TypeScript modules/grammar/DOM, and HTML assets found no additional safe change: pure extraction/linking already uses RAM, while remaining persisted SQL path, delta, manifest, workspace identity, coverage, or filesystem behavior needs the disk-backed seam. No additional CLI, StdioClient, FIFO, or Git workspace-creation process was found in domain tests; the existing `SeedGitTemplate` uses `OnceLock` for the shared Git seed.

Owner suites passed: `cargo test --test incremental` **31/0** in **26.80s**, `cargo test --test framework_manifests` **40/0** in **57.39s**, and `cargo test --test commitment_integrity` **14/0** in **24.72s**. Strict `cargo clippy --all-targets --all-features -- -D warnings` passed in **2.80s**. The final `cargo test --all-targets` passed **1,156/0/0 ignored**: library 18, main 0, ACDD 297, CLI roots 7, commitment integrity 14, core 49, docs check 9, framework manifests 40, impact context 4, incremental 31, languages 383, linked workspaces 3, linker 107, MCP 153, and query context 41. The final `git diff --check` passed. These are ordinary suite observations, not benchmarks; the total all-targets wall duration was not measured, and no speedup claim is made.

### Post-run audit of tests reported over 60 seconds — 2026-11-07

The final full-run output reported three individual long-running cases. `handoff_preserves_proofs_across_reviewed_history_rewrites` keeps its 11 history/commit-policy rows and restores `FixtureSnapshot` at the same workspace path. `vue_destructured_composable_call_uses_indexed_return_member` retains all 24 cold/delta comparisons because each distinct provider/configuration mutation needs its own persisted edge/coverage parity; a trial that removed 23 comparisons was reverted after confirming the final-state comparison did not prove intermediate parity. `configured_router_prefixes_are_static_unique_and_recursive` retains all 15 cold builds paired with delta states because each mutation has a distinct filesystem/manifest state. All three remain in the default lane with no ignored or filtered rows; the measured Git/build work is required by these seams.

### Additional Phase C / D follow-up — 2026-11-07 (ACDD harness plumbing)

| File cluster | Change |
| --- | --- |
| `tests/acdd/mcp.rs` / `tests/acdd/support.rs` | Replaced 14 repeated `finish()` + `with_session()` restart sequences across three tests with the shared `restart_mcp_session` harness wrapper. Worker/session choices and every assertion stay in the tests; no test count or observable changed. Current `mcp.rs` is 4,445 lines with 18 Forge-indexed tests. |
| `tests/acdd/tasks/workspaces/snapshot_recovery.rs` / `recovery_fixture.rs` | Reused the existing `TaskHarness::verified_revision_text` for snapshot-ref and HEAD reads. The test remains at 4 declarations; snapshot, recovery, rollback, delivery, and handoff assertions remain. Existing shared fixture/method call counts are above two. |
| `tests/acdd/runner.rs` | No setup extraction was safe: the largest Task3 runner test already uses `CommandGateHarness::execute_in_process` for all 11 command executions and its custom profile/process fixture is unique. Corrected the assertion name/message to match its existing `log_path.is_none()` condition; production only persists a sidecar path for oversized failed diagnostics. |

The post-edit gates passed: `cargo clippy --all-targets --all-features -- -D warnings` in **4.74s**, `cargo test --all-targets` **1,156 passed / 0 failed / 0 ignored**, and `git diff --check`. Per-target results in this run: library 18 (3.05s), main 0, ACDD 297 (144.79s), CLI roots 7 (2.76s), commitment integrity 14 (23.73s), core 49 (8.29s), docs check 9 (0.10s), framework manifests 40 (54.83s), impact context 4 (0.49s), incremental 31 (26.72s), languages 383 (137.05s), linked workspaces 3 (0.38s), linker 107 (8.21s), MCP 153 (31.71s), and query context 41 (5.83s). Total all-targets wall time was not measured; these suite runs are not benchmarks and no speedup is claimed.

### Additional Phase C follow-up — 2026-11-07 (parallel milestone and MCP pass)

| File | Wave-entry / current test declarations | Result |
| --- | ---: | --- |
| `tests/acdd/milestones/delivery.rs` | 5 / 5 | Replaced four manual gate/worker claim sequences with the existing profile-driven `TaskHarness::claim_next()`; the helper has 16 indexed callers. All delivery, receipt, candidate, and cleanup assertions remain in their tests. |
| `tests/acdd/milestones/handoff.rs` | 9 / 9 | Routed workspace-owned foreign-file removal through the existing `Workspace::remove`; no contract observable changed. |
| `tests/mcp/test_read.rs` | 20 / 19 | Merged the valid-range aliases and malformed-range bounds cases into the existing MCP seam test, retaining all accepted, reversed, malformed, and extreme-bound assertions while removing one repeated workspace/MCP setup. At wave entry this file was 1,112 lines; it is now 1,060. |
| `tests/acdd/milestones/lifecycle.rs` | 18 / 18 | No additional safe edit: the remaining project-binding routes are distinct and already use the canonical milestone/task harnesses. |
| `tests/acdd/milestones/receipts.rs` | 4 / 4 | No additional edit: invalid receipt mutations already share a delivered baseline and each test retains a distinct receipt/history contract. |
| `tests/framework_manifests/finite_operations/callable_imports.rs` | 1 / 1 | No additional edit: the existing persisted delta matrix checks distinct provider/configuration transitions and cold parity. |
| `tests/incremental/doc_parity.rs` | 4 / 4 | An attempted build consolidation moved reciprocal document links into the source-only delta phase and failed parity. Restored the original state order and retained the second build required by that distinct document-link seam; the focused test passes. |

Focused checks passed after the edits: `cargo test --test acdd milestones::delivery::` **5/0**, `cargo test --test mcp forge_read_` **18/0**, and `cargo test --test incremental source_delta_relinks_affected_docs_with_bare_suffix_references` **1/0**. Final `cargo test --all-targets` passed **1,155/0/0 ignored** (ACDD 297, CLI roots 7, commitment integrity 14, core 49, docs check 9, framework manifests 40, impact context 4, incremental 31, languages 383, linked workspaces 3, linker 107, MCP 152, query context 41, and library 18; main 0). Strict Clippy passed in **4.37s**. No independent review was run, as requested. These checks are not a benchmark or a speedup claim.

### Additional Phase C follow-up — 2026-11-07 (milestone snapshot and lifecycle pass)

Counts are live measurements at this wave's entry and exit. Test counts are
literal `#[test]` attributes; Forge AST confirmed the discovery count.

| File | Wave entry: lines / tests | Current: lines / tests | Result |
| --- | ---: | ---: | --- |
| `tests/acdd/mcp.rs` | 4,445 / 18 | 4,444 / 18 | Removed unused archive-directory creation; no assertion or test count changed. |
| `tests/acdd/tasks/lifecycle.rs` | 3,879 / 73 | 3,876 / 73 | Inlined the single-caller SQLite busy-handler callback; contention assertions remain. |
| `tests/acdd/milestones/discovery.rs` | 1,067 / 28 | 1,065 / 27 | Shared one workspace for same-unit planned/active `init` checks and table-drove rejected directory inputs; malformed-frontmatter operations remain separate. |
| `tests/acdd/milestones/handoff.rs` | 1,046 / 9 | 1,046 / 9 | Kept preflight rejection and successful archive/pruning as separate contract tests. |
| `tests/acdd/milestones/lifecycle.rs` | 929 / 18 | 919 / 18 | Removed a redundant `planned` sync row and reused owner-alias assertions in the same alias lifecycle test; longest-prefix stays separate. |
| `tests/acdd/milestones/history.rs` | 811 / 2 | 811 / 2 | No further safe deletion; the 11 history and 16 landing rows remain. |
| `tests/acdd/milestones/verification.rs` | 405 / 3 | 422 / 3 | Kept all 24 evidence scenarios. Snapshot restore now follows shared-state mutations only: 12 restores on the Unix path instead of restoring before every row. The existing non-Unix mode guard remains. |

The edits net **1 additional source line** and **one fewer test declaration**;
the verification change adds a small restore-state flag while avoiding redundant
Git/SQLite snapshot restoration for evidence-only rows. No benchmark or speedup
claim is made. `cargo test --test acdd` passed **296/0** in **141.73s**;
`cargo clippy --all-targets --all-features -- -D warnings` passed in **5.73s**;
`cargo test --all-targets` passed **1,154/0/0 ignored** (library 18, ACDD 296,
CLI roots 7, commitment integrity 14, core 49, docs check 9, framework manifests
40, impact context 4, incremental 31, languages 383, linked workspaces 3, linker
107, MCP 152, and query context 41; main 0). Total all-targets wall time was not
measured. Independent review was not run, as requested.

The same standard run reported three individual tests over 60 seconds:
`handoff_preserves_proofs_across_reviewed_history_rewrites`,
`configured_router_prefixes_are_static_unique_and_recursive`, and
`vue_destructured_composable_call_uses_indexed_return_member`. Their distinct
history, filesystem/manifest, and provider/configuration states remain necessary
for their production seams, so they stay in the default lane. The run reported
zero ignored tests. `cargo test -p forge-tasks --lib` also passed **34/0** in
**14.56s**. After this plan update, `cargo test --test docs_check` passed
**9/0/0 ignored**; the final repository-wide `git diff --check` also passed.

### Additional Phase C follow-up — 2026-11-07 (fresh candidate wave)

Counts are live measurements at assignment and after the edits. The final
`gates_ast.rs` line count includes removal of one unused import found by Clippy.

| File | Wave entry: lines / test declarations | Current: lines / test declarations | Result |
| --- | ---: | ---: | --- |
| `tests/acdd/tasks/guidance.rs` | 1,606 / 10 | 1,605 / 10 | Removed one broad object-type assertion already implied by the field assertions. |
| `tests/acdd/gates_ast.rs` | 1,310 / 17 | 1,297 / 16 | Removed one subsumed specialized-track test after carrying its persisted returned-stage assertions into the stronger rejection/recovery test; table-drove same-unit guidance inputs. |
| `tests/mcp/freshness/stdio_and_response.rs` | 1,675 / 13 | 1,677 / 13 | Consolidated equivalent reference/adapter cases and split a large governance scenario into three distinct contract tests using shared setup. |
| `tests/linker/method_semantics.rs` | 1,072 / 4 | 1,083 / 4 | Reused one persisted workspace for four method-call edge checks; each method retains its own assertion. |
| `tests/acdd/support.rs` | 1,862 / 12 | 1,862 / 12 | Audited candidate helpers; all source-confirmed shared helpers have at least two callers, so no safe extraction or deletion was made. |

The wave changed four clusters and left the support file unchanged after audit.
Across the five files it removed **1 line** and **one test declaration** net.
The final `cargo test --all-targets` passed **1,153 / 0 / 0 ignored** before the
unused-import-only cleanup in `gates_ast.rs`; strict Clippy then passed in
**4.74s** with that cleanup applied. The cleanup did not change test behavior.
No independent review was run, per the active instruction. Total all-targets
wall time was not recorded, and no speedup claim is made. The three previously
audited tests over 60 seconds remain in the default lane because their Git,
provider/configuration, and filesystem/manifest states are distinct.

The latest live size inventory confirms the original priority files remain the
largest test clusters (`acdd/mcp.rs` 4,444 lines, task lifecycle 3,876, MCP
context 3,709, MCP boundaries 2,715, and ACDD runner 2,269). Each has a recorded
Phase C edit or an explicit no-safe-duplicate disposition above. The remaining
largest files in the candidate set were likewise handled in the recorded
follow-up waves. Phase C is complete for the priority clusters declared in this
plan; this does not establish an arbitrary suite-wide line-count target. Focused
owner-suite command wall times on the same machine remained below their
baselines: `acdd` 295 / 2:10.962 vs 335 / 2:35.990, `mcp` 152 / 32.111s vs
164 / 34.698s, `languages` 383 / 2:12.548 vs 403 / 2:20.860, and
`framework_manifests` 40 / 51.368s vs 40 / 54.494s. `query_context` passed
41 / 0 in 5.779s wall; no baseline is recorded for that binary. These are
normal test runs, not benchmarks.

### Phase D transport and fixture inventory — 2026-11-07

Forge AST searches across `tests/` found **20** `Command::new` expressions,
**8** `StdioClient` identifier occurrences, **3** `mkfifo` literals, **0**
`TempDir::new` expressions, and **0** `tempfile::tempdir` expressions. Seven
`std::env::temp_dir()` calls belong to isolated filesystem fixtures. The
remaining process uses are confined to real CLI behavior checks (`cli_roots`,
`docs_check`, and CLI memory-budget precedence), dedicated MCP stdio transport
smoke (`tests/mcp/transport.rs` and the ACDD environment-session smoke), real
SIGKILL/crash-recovery fixtures (`tests/acdd/tasks/workspaces/process.rs`), and
Git/rustc fixture setup (`tests/acdd/snapshot.rs`,
`tests/acdd/command_gate_harness.rs`). All three FIFO probes test nonblocking
special-file rejection or process rendezvous. No ordinary MCP tool-contract
test launches a child server. `Workspace::new()` in the language tests is used
by the persisted HTML relationship harness; framework manifest tests expose an
in-memory source registry alongside a separate persisted workspace helper.

### Final Phase E verification — 2026-11-07

On the frozen Phase C candidate set, `time cargo test --all-targets` passed
**1,153 / 0 / 0 ignored** in **7m04.996s wall**. Counts by target: library 18,
main 0, ACDD 295, CLI roots 7, commitment integrity 14, core 49, docs check 9,
framework manifests 40, impact context 4, incremental 31, languages 383, linked
workspaces 3, linker 107, MCP 152, and query context 41. Strict
`cargo clippy --all-targets --all-features -- -D warnings` passed in **4.74s**
after the final source edit; later changes only updated this plan. The three
previously audited long-running production-seam tests remain in the default
lane. `git diff --check` and `cargo test --test docs_check` were rerun after this
entry was recorded.

### SOL review remediation and safe cold-reference reuse — 2026-11-07

- Fixed P1/P2 production regressions uncovered on `main`:
  - `crates/forge-tasks/src/store/lifecycle.rs` & `session_tests.rs`: enforced fail-closed session independence (`TASK_REVIEW_SESSION_UNKNOWN`, `TASK_REVIEW_NOT_INDEPENDENT`) when session fingerprints are missing, unknown, or shared with the prerequisite claim.
  - `crates/forge-tasks/src/engine/evidence_compiler.rs` & `crates/forge-tasks/src/tasks/gates.rs`: enforced complete command-plan trace validation (`validate_execution_plan`), required test counts (`tests_passed > 0`, `tests_failed == 0`), and compiler-error rejection on red seam tests.
  - `crates/forge-tasks/src/milestones/{integration.rs,command_gate.rs,handoff/verification.rs}`: rejected zero-exit verification checks that report `tests_failed > 0` or `output_invalid`.
  - `crates/forge-core/src/typed_facts/*` & `crates/forge-index/src/db/commitments.rs`: hardened binary/cursor bounds checks and restored `PRAGMA query_only` around temporary `fts5vocab` table creation.

---

## 7. Clean-Slate Audit & Remediation Plan (2026-11-08)

A clean-slate audit of `diet/base` (commits `1beb27517..6e51bb958` plus the working-tree state) against `main` (`cefb6bde6`) shows that while production check fixes and `OnceLock` Git/runner seeding (`SeedGitTemplate`, `PRECOMPILED_RUNNER`) are valuable, `tests/` still carries **+12,791 net lines over `main`** (`+54,439 / -41,648`) and multiple multi-subsystem monoliths requiring domain decomposition (while preserving cohesive single-domain test files).

### 7.1 Root Causes Identified

1. **Mechanical 1-Caller Inlining & Empty `*Observation` Shells (`tests/commitment_integrity`)**:
   - Single-caller helpers were pasted back into `tests/commitment_integrity.rs` (1,494 lines) inside `(|| -> anyhow::Result<_> { ... })()` IIFE closures and `{ ... }` block expressions, leaving 14 zero-method `*Observation` structs stranded in `tests/commitment_integrity/harness.rs`, `disk_index.rs` (59 lines of structs only), and `parallel_sealing.rs` (26 lines of structs only).
2. **Static Namespace Dumping & Type Aliasing (`tests/mcp/harness.rs`)**:
   - 17 of 22 methods on `impl McpTestHarness` in `tests/mcp/harness.rs` are static functions unrelated to MCP (`seed_git`, `create_git_tags`, `stage_git_files`, `commit_git`, `run_git`, `stored_node_count`, `function_entrypoint`, `command_registration_count`, `edge_rows`, `edge_query_plan`, `unresolved_coverage_rows`, `edge_occurrence_snapshot`, `mark_legacy_index_semantics`).
   - Aliasing `McpTestHarness as Client` (`tests/mcp/context.rs`, `tests/mcp/test_read.rs`, `tests/acdd/mcp.rs`) and `as Mcp` (`tests/mcp/tool_evolution.rs`) broke static call-graph edges (`code_map_query` reports 0 callers).
3. **Defeated `Weak<Workspace>` Cache & Missing Shared Fixture Reuse**:
   - `intern_workspace` (`tests/mcp/harness.rs:12-40`) stores `Weak<Workspace>` while `McpTestHarness` holds the only `Arc<Workspace>`. Dropping the harness at the end of a test immediately deletes the workspace and invalidates the cache. Meanwhile, `AnalysisFixture::indexed()` (`tests/mcp/harness/analysis.rs:23`) is uncached and rebuilds the same 3-file workspace 4 times in `tests/mcp/tool_evolution/mcp_boundaries.rs`.
4. **Unoptimized Disk/Memory/CPU Loops**:
   - `persisted_state(w)` (`tests/mcp/harness/analysis.rs:49-93`) dumps every SQLite table (up to 100k rows) and runs full `commitments::verify` **19 times** inside one loop in `tests/mcp/tool_evolution/mcp_boundaries.rs:1997-2108`.
   - `fresh_cold_database(w)` (`tests/languages/support.rs:15-24`) allocates a new `.forge/cold-{seq}.sqlite` and an empty `.forge/cold-cache-{seq}` directory on every cold parity check without cleanup, forcing full Tree-sitter re-parsing.
   - `vue_destructured_composable_call_uses_indexed_return_member` (`tests/languages/typescript/frameworks/vue_runtime/destructured_call.rs:354-477`) adds 19 independent `init*`/`bad*` store/consumer file pairs one-by-one, causing 19 full directory walks with 0% cache hits in `ColdFilesystemReferenceCache` and 19 cold builds of a ~45-file workspace.
   - `DependencyRegistry::load_standard_framework_manifests()` (`crates/forge-index/src/languages/dependency_registry.rs:580-611`) re-parses bundled TOML/YAML framework manifests on every `DependencyRegistry` construction instead of cloning a `OnceLock` map.
5. **Meta-Test Bloat & Duplicate Support Modules**:
   - Over 1,100 lines of meta-tests test test-only helpers (`workspace_fixture_tests` in `tests/acdd/support.rs:1397-1862`, `tests/mcp/harness_contract.rs`, `tests/acdd/tasks/workspaces/snapshots.rs`).
   - `tests/framework_manifests.rs` mounts `#[path = "languages/support.rs"] mod support;` alongside `tests/framework_manifests/support.rs`, duplicating SQLite query helpers.
   - `tests/acdd/mcp.rs` cross-mounts `#[path = "../mcp/cli.rs"]` while `tests/acdd/milestones/harness.rs` builds its own per-call Tokio runtime for CLI execution.
6. **Weak / Silent-Fallback Checks**:
   - `fixture_integration_evidence` (`tests/acdd/support.rs:431-460`) silently fabricates `"unavailable"` digests on `handoff_context` failure, and `observe_fixture_evidence` (`tests/acdd/support.rs:957`) accepts any `TASK_COMMAND_FAILED` prefix without checking for runtime-red exit code 101.

### 7.2 Remediation Execution Waves

| Wave | Scope | Concrete Actions | Verification |
| --- | --- | --- | --- |
| **Wave 1: CPU, Disk & Loop De-bloat** | `crates/forge-index/src/languages/dependency_registry.rs`, `tests/languages/support.rs`, `tests/languages/harness.rs`, `tests/languages/typescript/frameworks/vue_runtime/destructured_call.rs`, `tests/mcp/harness/analysis.rs`, `tests/mcp/tool_evolution/mcp_boundaries.rs` | 1. Cache parsed standard framework manifests in a `OnceLock<BTreeMap<String, FrameworkManifest>>`.<br>2. In `fresh_cold_database`, reuse or clean up temporary cold DBs/caches after observation.<br>3. Batch disjoint `init0..1` and `bad0..16` file-pair writes in `destructured_call.rs` into one delta + cold parity check while keeping sequential deltas for same-path state transitions.<br>4. Cache `AnalysisFixture` and replace the 19x `persisted_state` loop dump in `mcp_boundaries.rs` with before/after full dumps plus per-row `PRAGMA data_version` + `output_root` checks. | `cargo test --test languages`, `cargo test --test mcp`, `cargo test --test framework_manifests` |
| **Wave 2: Harness Architecture Cleanup** | `tests/common/{mod,workspace,sqlite,cli,mcp_harness}.rs`, `tests/mcp/harness.rs`, `tests/framework_manifests.rs`, `tests/acdd/{support,mcp}.rs`, `tests/acdd/milestones/harness.rs` | 1. Remove `Client` / `Mcp` type aliases for `McpTestHarness`.<br>2. Move static Git helpers from `McpTestHarness` to `Workspace` and SQL helpers to `tests/common/sqlite.rs`; inline 1-caller queries.<br>3. Replace the broken `Weak<Workspace>` `intern_workspace` cache with direct `Workspace` construction (and explicit `OnceLock` only for fixtures shared by ≥2 tests).<br>4. Promote `tests/mcp/cli.rs` to `tests/common/cli.rs` and deduplicate `tests/framework_manifests` vs `tests/languages` support modules. | `cargo test --test mcp`, `cargo test --test acdd`, `cargo test --test framework_manifests` |
| **Wave 3: `commitment_integrity` De-IIFE & Module Restoration** | `tests/commitment_integrity.rs`, `tests/commitment_integrity/{harness,disk_index,parallel_sealing,cold_sealing,coverage}.rs` | 1. Delete the 14 one-off `*Observation` struct shells and unwrap the inlined IIFE blocks.<br>2. Provide composable `CommitmentHarness` primitives (`seal`, `verify`, `owner_leaf`, `fts_postings`, `with_savepoint`) in `harness.rs`.<br>3. Move disk-index, parallel-sealing, and cold-sealing tests into their respective submodules so every file is `< 500` lines. | `cargo test --test commitment_integrity` |
| **Wave 4: Assertion Hardening & Meta-Test Diet** | `tests/acdd/support.rs`, `tests/acdd/mcp.rs`, `tests/mcp/context.rs`, `tests/mcp/harness_contract.rs` | 1. Split `fixture_integration_evidence` (strict, no `"unavailable"` fallback) from `malformed_integration_evidence`.<br>2. Tighten `observe_fixture_evidence` to require exit code 101 on contract red runs.<br>3. Replace string-contains checks in `tests/mcp/context.rs` and generic error checks in `tests/acdd/mcp.rs` with structured JSON/error-code assertions.<br>4. Trim the 465-line `workspace_fixture_tests` in `tests/acdd/support.rs` and `tests/mcp/harness_contract.rs` to concise smoke checks. | `cargo test --test acdd`, `cargo test --test mcp` |
| **Wave 5: Decompose Multi-Subsystem Monoliths by Domain Cohesion** | `tests/acdd/mcp.rs` (4.4k), `tests/mcp/context.rs` (3.6k), `tests/mcp/tool_evolution/mcp_boundaries.rs` (2.6k), `tests/acdd/support/*`, `tests/commitment_integrity/*` | Decompose ONLY multi-subsystem monoliths mixing unrelated contracts and shared fixture infrastructure into focused domain submodules (`tests/commitment_integrity/*` < 500 lines). Cohesive single-domain test files (`test_read.rs`, `freshness/*`, `languages/*`, `query_context/*`, `linker/*`, `core/*`) are preserved as single files without mechanical line-count splitting. | Full milestone handoff gate (`TESTS.md`) |
