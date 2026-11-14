---
id: AR-001-borrowed-data-and-composable-execution
title: "Borrowed data paths and composable Rust execution"
doc_type: contract
status: planned
depends_on: []
guidance:
  hitl: pro
owners:
  - src/cli/mod.rs
  - src/mcp/tools.rs
  - src/engine/pipeline.rs
  - crates/forge-tasks/src/engine/orchestrator/task_runner.rs
  - tests/
  - docs/reference/
  - docs/architecture/
  - docs/milestones/AR-001-borrowed-data-and-composable-execution.md
invariants:
  - 'INV-SCOPE-INTEGRITY: Exclude every Cargo.toml, Cargo.lock, build.rs, .forge/**, adapter/profile configuration, and dependency change from implementation scopes and extensions; this goal authorizes no protected-file exception.'
  - 'INV-CORE-LEAF: Preserve existing crate dependency directions, core leaf isolation, and the frozen engine/database boundary; add no cross-layer imports or shared execution framework.'
  - 'INV-SURFACE-COMPATIBILITY: Preserve tool registration, CLI syntax, public Rust signatures, output defaults, errors, debug-log contents and success/error timing, serialized response bounds, and continuation identities.'
  - 'INV-OWNERSHIP-AT-BOUNDARIES: Borrow immutable policy and role configuration while their owners live; move consumed candidates and CLI results; allocate owned copies only where transport, retained debug data, or durable DTO lifetimes require ownership.'
  - 'INV-ORDERED-SINGLE-DISPATCH: Execute each admitted stage once in its existing order, short-circuit on its existing failure, and preserve special recovery paths; use local typed helpers and FnOnce composition without a parallel registry or dynamic middleware chain.'
  - 'INV-INDEX-PUBLICATION: Preserve source snapshot verification, extraction cache identity, source-buffer reuse, permit release, cancellation and worker joining, bounded spooling, deterministic graph/commitment bytes, and verified atomic publication.'
  - 'INV-TASK-AUTHORITY: Preserve profile-driven gate selection, genuine worker/session independence, active claims, retained rejection sessions, command observations, immutable candidates, and receipt-backed delivery; runner process success never substitutes for an accepted submission.'
  - 'INV-PROOF-BOUNDARY: Prove compatibility through real production callers and deterministic sensitivity mutations; distinguish supplemental ownership evidence from end-to-end proof and make no unmeasured latency, throughput, RSS, or whole-operation zero-allocation claim.'
required_skills:
  - contextunity-forge
  - contract-planning
  - agent-instructions
related_plans: []
---

# Borrowed data paths and composable Rust execution

## Outcome and admission

Optimize three observed ownership boundaries: MCP/CLI dispatch and presentation,
cold extraction, and task gate execution. Keep the current engine owners and
public contracts. Use ordinary borrowed references, moves, and small typed
functions or closures to express the existing stage sequence. Introduce a helper
only when it removes repeated policy/control flow or makes an ownership boundary
explicit; leave cohesive algorithms readable in place.

`AR` owns architecture and composition. The inspected active and archive filename
inventory contains no `AR-NNN` milestone; `AR-001` is the next unused sequence.
This contract is planned and writes no runtime implementation during authoring.
Its first accepted task claim activates execution.

The planned [IN-002 rename](IN-002-contextunity-acdd-one-pass-rename.md) overlaps
these source owners. Neither contract is a semantic prerequisite for the other.
The coordinator serializes their execution; they must not run concurrently.
This contract targets the current Forge paths. If IN-002 lands first, re-ground
and revise this contract's paths, identities, test commands, and citations before
claiming any task. Do not introduce old/new-name compatibility scaffolding here.

The planning add-on references ADR 0024, but the inspected ADR inventory contains
0001–0021 and no 0024. Its contents are unavailable authority. Existing ADRs and
verified code govern this work; no lifecycle redesign depends on that reference.
No repository-local lesson file is present in the inspected planning add-on paths.

## Architectural viability and negative boundaries

The existing `responding_with_presentation` closure boundary, bounded response
renderer, cold producer/consumer pipeline, and canonical task executor already
provide the required structure. Extend those compositions locally. A universal
service trait, Tower stack, boxed future chain, plug-in API, secondary dispatcher,
or request-wide context object spanning all three domains adds no admitted value.

Preserve all public function signatures and DTO ownership. `symbols: []` is a
closed public-interface boundary, not permission to add exported helpers. Private
helpers stay in their scoped files; no new production module registration is
needed. Do not relocate presentation into the leaf crate or move task effects
into pure models. Preserve existing re-exports and import directions.

Exclude new dependencies, unsafe production code, lifetime erasure, leaked/static
buffers, interior-mutability workarounds, reader pools, concurrency changes,
automatic retries, storage migrations, cache/version changes, feature changes,
language/linker algorithm changes, and changes to the workflow/profile schema.
Preserve allocator trims, compression, bounded queues, source limits, and existing
publication ordering. Do not run benchmarks, change host settings, or claim
numerical performance improvements without separately authorized measurement.
Normal engine-owned task receipts/checkpoints are governed by the existing
workflow; they do not authorize editing protected infrastructure.

## Grounded symbols and current ownership

Inspected with `code_map_inspect` against canonical graph generation
`e78987e59d95103e03cc39fe93c89dfcc1f4339523333714f5bef0f560da6d85`.
Complete source reads confirm the following opportunities; line numbers describe
this inspected baseline and are not permanent identities.

| Verified declaration | Current path and contract | Admitted change |
| --- | --- | --- |
| [`call_tool`](../../src/mcp/tools.rs), lines 1195–1340 | Consumes the RMCP request; validates format/fields, selects a worktree and adapter, dispatches through the existing router, enforces response policy, then logs when enabled. | Keep the ordering and router. Retain one request adapter snapshot, borrow its response policy during finalization, and remove redundant policy copies where owner lifetimes permit. |
| [`responding_with_presentation`](../../src/mcp/tools.rs), lines 379–439 | Takes `run: impl FnOnce(&scanner::Adapter, &ResponsePolicy) -> anyhow::Result<Value>` and decorates Markdown before bounded rendering. | Reuse this existing middleware-style boundary; keep handlers and presentation metadata typed and local. |
| [`run_selected`](../../src/cli/mod.rs), lines 1330–1941 | Owns CLI dispatch; bounded `task_list`/`forge_read` presentation passes `Ok(result.clone())` and retains the original for optional debug logging. | Move the result on the debug-disabled bounded path. Preserve an original snapshot only when enabled logging requires it; keep logging after successful rendering and preserve write/error behavior. |
| [`result_for_presented`](../../src/mcp/response.rs), lines 80–246 | Existing owning bounded-renderer API shared by CLI/MCP; preserves raw continuation data while trying compact/truncated renderings. | Reuse unchanged. Its internal raw/candidate copies are outside the optimization claim; removing them without equivalent continuation proof is forbidden. |
| [`extract_candidate`](../../src/engine/pipeline.rs), lines 424–497 | Borrows a `BuildCandidate`, checks file metadata around a bounded read, then clones the candidate before `into_entry`. | Consume the candidate after its checks/cache-key use and move its path/language/digest into the existing `FileEntry`. Change only this private signature and its caller. |
| [`extraction_worker`](../../src/engine/pipeline.rs), lines 700–795 | Owns `PipelineWork`, reuses one source string, prepares facts/storage, and transfers its `MemoryPermit` into the result item. | Destructure owned work and compose extraction/preparation without duplicating candidate strings or changing permit/cancellation ownership. |
| [`run_task_lifecycle`](../../crates/forge-tasks/src/engine/orchestrator/task_runner.rs), lines 32–248 | `pub fn run_task_lifecycle(root: &Path, task_id: &str, options: &TaskRunOptions, observer: &mut dyn FnMut(OrchestratorEvent)) -> Result<Value>`; clones a selected role into a tuple despite retaining the owning profile. | Borrow the selected role and role ID during preparation/execution; allocate only at existing owned invocation/claim/event boundaries. Keep the public signature. |
| [`render_role_prompt`](../../crates/forge-tasks/src/engine/orchestrator/spawner.rs), lines 77–111 | `pub fn render_role_prompt(claim: &Value) -> Result<String, RunnerError>` rejects incomplete/truncated authority. | Preserve and call this canonical borrowed producer unchanged. |

Depth-one impact confirms `extract_candidate` is called by `extraction_worker`;
`run_task_lifecycle` feeds wave dispatch and isolated task execution and has
existing lifecycle tests. The bounded renderer has CLI and MCP consumers.
The graph reports partial resolution (30.6% of non-external references), so a
missing call edge is not absence evidence. Read each scoped caller and use AST
search before changing private signatures. `$VALUE.clone()` AST search confirms
policy/adapter copies in MCP; inspect each copy rather than mechanically deleting
`clone`, especially cheap `Arc` handles and externally owned DTO fields.

## Governing invariants and producer-consumer contract

`search_docs` grounds layer isolation in
[ADR 0018](../adr/0018-layer-dependency-boundaries-and-core-leaf-isolation.md#1-core-leaf-isolation),
response limits in [ADR 0011](../adr/0011-bounded-mcp-response-budgets.md#decision),
publication in [indexing](../architecture/indexing.md#documentation-references-and-verification),
and stage ownership in [performance invariants](../architecture/performance.md#model-and-agent-invariants-immutable-guidelines).
Preserve [dual-surface parity](../adr/0001-dual-surface-parity.md),
[staged publication](../adr/0003-two-phase-staged-publication.md),
[lean serialization](../adr/0012-zero-regression-serialization-and-cold-build-latency.md),
[task snapshots](../adr/0015-acdd-lifecycle-snapshots-and-proactive-defect-resolution.md),
and [workflow/task separation](../adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md).
No accepted ADR changes in this milestone.

| Boundary / owner task | Input → output and consumers | State, configuration, failure, and proof |
| --- | --- | --- |
| surfaces | RMCP request or parsed CLI command → existing engine operation → presentation/result | Existing worktree/config/profile selection, module enablement, defaults, debug flag and byte limits. Read callbacks retain their existing lock/transaction fence. Errors short-circuit at the existing stage. CLI and MCP contract tests compare parsed JSON and Markdown semantics, errors, paging and byte limits. |
| indexing | Scanner-owned candidate + shared adapter/cache + reusable source buffer → checked `FileEntry` + facts → storage/catalog → spool and published index | Existing metadata/digest checks and cache-key fields; one owner moves the permit until ingestion/drop. Cold failures cancel/join and preserve the old index. Existing cold/delta, cache and commitment seams read back the published database. |
| task-execution | Resolved profile + persisted task/gate → role or mechanical execution → accepted gate state/events/receipt | Profile, SQL gates, claims, checkpoints and receipts remain authoritative. Keep independent sessions, rejection reuse, timeout and no-progress failures. Existing lifecycle/restart tests inspect persisted state and owned event DTOs. |

Complete paths to preserve:

1. MCP request → `call_tool` format/field checks → worktree/module selection →
   canonical router → handler `read_with_adapter` admission/query fence →
   `responding_with_presentation` → bounded response enforcement → caller.
   CLI root/settings selection → `run_selected` → same engine owner → current
   presentation/renderer → writer; retain guide-init and source-only-read recovery.
2. Cold build → scanned candidate → byte permit/work queue → `extraction_worker`
   → checked extraction/cache → catalog and storage preparation → consumer spool
   → existing seal/verify/WAL restoration/atomic publish → locked query readback.
3. CLI task run or wave executor → `run_task_lifecycle` → canonical claim →
   complete context → role runner or mechanical submit → accepted SQL transition
   → event/receipt readback. Do not synthesize acceptance from observer events or
   process exit status.

All paths retain their current workspace confinement, validation, limits, cleanup,
and rejection semantics. Add no tenancy/configuration dimension, alternate backend,
network dependency, retry loop, or compensation mechanism. Planned consumers are
the same callers under IN-002's rename; any API evolution belongs to a separate
owner-approved contract.

## Waves, proof policy, and harness allocation

| Wave | Tasks | Dependency |
| --- | --- | --- |
| 0 | `freeze-ownership-and-seams` | none |
| 1 | `compose-surface-dispatch`, `move-cold-extraction-inputs`, `borrow-task-execution-context` | each depends only on Wave 0 |

Wave 0 owns shared test registrations and creates minimal valid module stubs.
It registers `tests/mcp/borrowed_dispatch.rs` in `tests/mcp.rs`,
`tests/core/borrowed_pipeline.rs` in `tests/core.rs`, and
`tests/acdd/orchestrator/lifecycle/borrowed_execution.rs` as a child module in
`tests/acdd/orchestrator/lifecycle.rs`. The child inherits the existing Linux
runner lane and can reuse its parent's private `LifecycleHarness` without
exporting a second harness. Preserve established suite-root path conventions.
Downstream tasks never edit these shared indexes or another task's seam file.
Overlaps between bootstrap and later scopes are sequential, never concurrent.

Use `direct-proof`: this is a behavior-preserving refactor and the canonical
[task proof contract](../reference/tasks.md#proof-taxonomy-bundle-generation)
permits an observed exit 0 for that policy. Do not manufacture a red test by
asserting a new public behavior or a private helper's existence. Establish the
compatibility baseline before Build, then prove sensitivity with the mutations
below. Any additional contract that selects expected-red must compile and fail
at runtime with exit 101, never at compilation.

Use the existing `McpTestHarness`, in-process CLI runner, `Workspace`, lifecycle
harness, canonical fixtures, and commitment readback. Do not add a new Cargo
test binary or global allocator. Pointer/lifetime checks and AST ownership review
are supplemental evidence, not substitutes for production-seam compatibility.
No whole-operation allocation ceiling is admitted. Record removed deep copies,
remaining required ownership, test sensitivity and any limitations in each task's
evidence. Benchmark lanes and host-dependent timing thresholds are out of scope.

## Tasks in this milestone

### task: freeze-ownership-and-seams

```yaml
task_ref: freeze-ownership-and-seams
target: "Freeze the ownership/compatibility matrix and disjoint waves, pre-register compiling domain seam stubs, and prove the unchanged manifest parser before parallel refactoring without altering production behavior or protected infrastructure"
proof_policy: direct-proof
contract_revision: 1
depends_on: []
scope:
  - docs/milestones/AR-001-borrowed-data-and-composable-execution.md
  - tests/mcp.rs
  - tests/core.rs
  - tests/acdd/orchestrator/lifecycle.rs
  - tests/mcp/borrowed_dispatch.rs
  - tests/core/borrowed_pipeline.rs
  - tests/acdd/orchestrator/lifecycle/borrowed_execution.rs
symbols: []
status: ready
subtasks:
  - subtask_ref: freeze-stage-and-ownership-matrix
    title: "Confirm stage order, lifetime boundaries, existing errors, protected exclusions, and exact seam allocation in this manifest"
  - subtask_ref: preregister-domain-seams
    title: "Create compiling empty seam modules and register them only in existing suite indexes"
  - subtask_ref: prove-bootstrap-closure
    title: "Validate the real manifest and dependency graph, compile registered stubs, and record the producer-consumer matrix for downstream tasks"
```

Blueprint slice:

- Implementation steps: re-inspect cited owners and the active profile; validate
  exact scopes/DAG/public-symbol closure; publish this matrix; register the three
  stubs; run parser proof and strict Clippy. Keep existing errors and DTOs; shared
  error taxonomy changes and new shared core definitions are unnecessary.
- Symbol signatures: none added or modified. The parser seam is existing
  `snapshot_parses_frontmatter_identity_and_task_blocks` in
  `crates/forge-tasks/src/milestones/contract.rs`.
- Test seam: `cargo test -p forge-tasks --lib snapshot_parses_frontmatter_identity_and_task_blocks`;
  `contextunity-forge-mcp validate docs/milestones/AR-001-borrowed-data-and-composable-execution.md --format json`.
  Strict Clippy proves every registered stub compiles. Parser sensitivity: a
  disposable in-memory manifest fixture with a local dependency cycle must fail
  parsing; never corrupt the committed authority to demonstrate this.
- Incoming seams: current manifest parser, profile and suite indexes.
- Outgoing seams: compiling registered files and the frozen matrix consumed by
  all Wave 1 tasks; stubs carry no fake passing assertions or ignored tests.
- Negative boundaries: no production edit, ADR change, fixture framework,
  generated definitions, new error enum, or post-bootstrap index ownership.

### task: compose-surface-dispatch

```yaml
task_ref: compose-surface-dispatch
target: "Compose existing MCP/CLI dispatch and finalization with borrowed request policy and moved CLI results, proving unchanged validation order, output/debug contracts and bounded continuations through real handlers using existing dependencies"
proof_policy: direct-proof
contract_revision: 1
depends_on:
  - freeze-ownership-and-seams
scope:
  - src/mcp/tools.rs
  - src/cli/mod.rs
  - tests/mcp/borrowed_dispatch.rs
  - docs/reference/mcp-tools.md
  - docs/reference/cli.md
  - docs/reference/mcp-markdown-contracts.md
symbols: []
status: planned
subtasks:
  - subtask_ref: retain-request-policy-owner
    title: "Retain one selected request adapter and borrow final response policy while preserving format/field/worktree/module rejection precedence"
  - subtask_ref: consume-bounded-cli-results
    title: "Move bounded CLI result payloads when debug is disabled and retain an original debug snapshot only when required"
  - subtask_ref: prove-surface-composition
    title: "Exercise CLI/MCP success, short-circuit errors, debug output, paging and response limits and align affected presentation references"
```

Blueprint slice:

- Implementation steps: keep `call_tool` as protocol boundary and the canonical
  router as sole dispatch owner; extract only cohesive private stage helpers;
  retain existing `FnOnce` handler composition; borrow policy from the retained
  adapter or a local default; move CLI `Value` at its last use. Preserve debug
  log payload and timing, including omission on bounded-render errors. Preserve
  guide-init recovery, source-only reads, validation exit and broken-pipe success.
- Symbol signatures: all public APIs and `result_for_presented` stay unchanged;
  private helper/signature changes are confined to the two scoped source files.
- Test seam: proposed `tests/mcp/borrowed_dispatch.rs::surface_dispatch_preserves_results_and_rejections`,
  run with `cargo test --test mcp borrowed_dispatch`; also run existing
  `cargo test --test mcp cli_contract`, `cargo test --test mcp test_read`, and
  `cargo test --test mcp tool_evolution` for bounds and paging.
  Keep distinct debug and continuation workflows in separate named tests.
- Sensitivity: temporarily bypass final byte enforcement, select the primary
  instead of requested worktree, and reorder format versus unsupported-field
  rejection; the corresponding scenario must fail for each mutation. Assert
  continuation generation/offset and retained source digest, Unicode boundaries,
  JSON/Markdown outcomes, and debug payload readback. Ownership source review
  must show removal of the unconditional CLI result clone and selected policy
  clone; remaining `Arc`, tool-name, debug, or transport copies need reasons.
- Incoming seams: parsed CLI/RMCP input, selected adapter, immutable invocation
  settings, canonical handler result, current presentation/response contracts.
- Outgoing seams: unchanged engine calls and bounded renderer invocation; writer
  output and MCP response envelope. No cross-task production interface is added.
- Negative boundaries: do not edit `src/mcp/response.rs`, server admission,
  formatter algorithms, tool schemas, command syntax or logging infrastructure.
  Required raw-value copies in the bounded renderer remain intact. Reconcile
  reference pages with live behavior; do not describe the refactor as new syntax.

### task: move-cold-extraction-inputs

```yaml
task_ref: move-cold-extraction-inputs
target: "Move scanner-owned candidates into checked extraction results and compose worker preparation without redundant candidate strings, proving cache/publication equivalence and cancellation/permit safety within existing bounded indexing stages"
proof_policy: direct-proof
contract_revision: 1
depends_on:
  - freeze-ownership-and-seams
scope:
  - src/engine/pipeline.rs
  - tests/core/borrowed_pipeline.rs
  - docs/architecture/indexing.md
  - docs/architecture/performance.md
symbols: []
status: planned
subtasks:
  - subtask_ref: consume-checked-candidate
    title: "Let private extraction consume BuildCandidate after metadata and cache-key checks, moving path/language/digest into the existing FileEntry"
  - subtask_ref: preserve-worker-stage-ownership
    title: "Compose worker preparation with explicit single ownership of source buffer, extracted facts and MemoryPermit"
  - subtask_ref: prove-index-equivalence-and-cleanup
    title: "Verify warm/cold extraction, source rejection, failed-consumer cleanup, cold/delta parity and deterministic commitments and update architecture descriptions"
```

Blueprint slice:

- Implementation steps: destructure `PipelineWork` without cloning its candidate;
  preserve pre/post-read metadata and size checks and all cache-key fields;
  compute cache inputs before consuming candidate fields; reuse existing
  `BuildCandidate::into_entry` unchanged; compose fact/storage preparation using
  local functions/`Result::and_then` where useful. Keep the required facts map
  and catalog path ownership unless a separately proven replacement is within
  this same file. No claim requires removing every allocation.
- Symbol signatures: private `extract_candidate` changes from borrowed to owned
  `BuildCandidate`; its `extraction_worker` caller changes together. Public build,
  delta, scanner and cache signatures remain unchanged.
- Test seam: proposed `tests/core/borrowed_pipeline.rs::cold_pipeline_preserves_cache_and_published_facts`,
  run with `cargo test --test core borrowed_pipeline`; separate
  `cold_pipeline_failure_preserves_published_snapshot` covers real consumer
  failure and cleanup. Also run `cargo test --test incremental` and
  `cargo test --test commitment_integrity`.
- Sensitivity: bypass candidate metadata rejection for a stale-candidate
  scenario; bypass consumer cancellation for the failure scenario; mutate an
  output fact for the cold/delta commitment comparison. Tests must detect the
  respective rejection, cleanup, or published-root mismatch. Use existing hooks
  and fixtures, not sleeps. Supplemental private tests may check moved string
  buffer identity; source review must show the consumed candidate replaces
  `candidate.clone().into_entry(...)` on both cache-hit and miss paths.
- Incoming seams: scanner candidate, permit, shared registry/cache, source-buffer
  lifetime. Outgoing seams: existing `PipelineItem`, compressed payloads/catalog,
  consumer/spool, sealed persisted facts. Preserve all current queue and batch
  limits, including single oversized admission and release on every exit path.
- Negative boundaries: no scanner, DB writer, cache-format, linker or publication
  algorithm edit; no new per-file full-facts copy or nested parallel iterator;
  no source reread, permit lifetime extension, detached worker or swallowed error.

### task: borrow-task-execution-context

```yaml
task_ref: borrow-task-execution-context
target: "Borrow selected role configuration through task gate preparation/execution and use local ordered stage composition while preserving canonical claims, rejection reuse, observed command proof, process failure handling and durable receipt acceptance"
proof_policy: direct-proof
contract_revision: 1
depends_on:
  - freeze-ownership-and-seams
scope:
  - crates/forge-tasks/src/engine/orchestrator/task_runner.rs
  - tests/acdd/orchestrator/lifecycle/borrowed_execution.rs
  - docs/reference/acdd.md
symbols: []
status: planned
subtasks:
  - subtask_ref: borrow-selected-role
    title: "Retain the owning profile and borrow its selected role/role ID instead of cloning a full role into the runner tuple"
  - subtask_ref: compose-gate-execution
    title: "Separate preparation and execution with private typed functions where they remove repeated logic while retaining exact claim/context/submit ordering"
  - subtask_ref: prove-authoritative-completion
    title: "Exercise profile-driven mechanical/role execution, retained rejection sessions, restart and failure rejection and document unchanged execution authority"
```

Blueprint slice:

- Implementation steps: keep the profile alive through each iteration; borrow
  its selected role until constructing the existing owned invocation; retain
  required ownership in `Claim`, `RunnerInvocationContext`, and events. Keep
  `RunnerConfig::from_role`, context reconstruction, prompt validation, spawner,
  monitor and zero-argument submit as canonical collaborators. Use local typed
  helpers/closures only where the resulting order stays explicit; do not force
  mechanical and role branches through a new generic runtime.
- Symbol signatures: `run_task_lifecycle` and all exported DTO signatures stay
  unchanged. Only private helpers in this source file may be introduced.
- Test seam: proposed `tests/acdd/orchestrator/lifecycle/borrowed_execution.rs::task_dispatch_preserves_profile_authority`,
  run with `cargo test --test acdd acdd::orchestrator::lifecycle::borrowed_execution`;
  also run `cargo test --test acdd acdd::orchestrator::lifecycle` and
  `cargo test -p forge-tasks --lib`. Reuse the canonical lifecycle fixtures and
  existing harness; add no second factory. Keep genuine process tests only for
  the runner-process boundary and preserve their platform constraints.
- Sensitivity: treat a zero-exit runner with no gate submission as success,
  replace the resumed builder session with a fresh session, and replay an
  accepted gate after restart. Separate scenarios must detect each mutation
  through persisted gate/claim/receipt and event readback. Cover mechanical
  gates without role spawning, active foreign claim refusal, timeout/failure,
  attempts exhaustion and incomplete-context rejection through the existing
  scoped lifecycle suite. Source/lifetime review proves the selected full role
  clone is gone; owned event and process records remain valid after local owners
  drop. No wall-clock or total allocation ceiling is inferred.
- Incoming seams: registry/worktree selection, persisted gate and profile,
  invocation options, genuine session and complete context. Outgoing seams:
  unchanged claim/submit APIs, role process, ordered observer events and durable
  accepted task state consumed by wave dispatch and CLI task run.
- Negative boundaries: no SQL/schema/store transaction edits, profile or stage
  semantics change, runner command/template change, new retry/session policy,
  prompt truncation, or synthetic proof/receipt. Keep store drop/reopen boundaries
  around subprocess execution and canonical submission unchanged.

## Verification and delivery

Before each task Build, load `code-quality` and `test-contract-seam`, the relevant
owner instructions, and returned workflow guidance. For automated contract/build
gates use the claim-bound `task verify` command with the named seam, followed by
the normal stage submission. Run the scoped compatibility tests and strict
`cargo clippy --all-targets --all-features -- -D warnings`; record actual command
and test counts. Run new scenarios separately and parameterize only input
variations of the same seam. Do not duplicate or reorganize unrelated suites.

Acceptance requires all three concrete ownership improvements, passing production
compatibility and sensitivity evidence, disjoint Wave 1 scopes, no unadmitted
public API or protected-file changes, and documentation matching the final code.
An ownership optimization that cannot preserve its boundary is returned to the
architect with an explicit authority gap; do not silently broaden scope or
substitute a speculative optimization.

At milestone handoff the coordinator launches the required documentation agent
with the final manifest, accepted evidence and diff against the explicit target
base. Its admitted documentation scope is this manifest and the six reference/
architecture pages named in the implementation tasks. Submit resulting edits for
independent review; repeat affected scans after later implementation changes.
Follow [TESTS.md](../../TESTS.md#milestone-handoff-gate): commitment integrity,
strict Clippy, `cargo test --all-targets`, and the separate
`cargo test -p forge-tasks --lib`. Preserve trace-bound counts and report the
member-library count separately. Bind independent integration review and engine
verification to the final content and selected target base before archival.
This authoring gate does not claim or submit tasks, initialize implementation,
commit, publish, run benchmarks, or perform milestone handoff.
