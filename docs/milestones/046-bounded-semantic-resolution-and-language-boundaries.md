---
id: m-bounded-semantic-resolution-and-language-boundaries
title: Bounded semantic resolution and language ownership
doc_type: contract
status: active
repository: forge-mcp
project: forge-mcp
depends_on:
- m-test-suite-architecture-and-domain-consolidation
owners:
- src/core/semantic.rs
- src/core/typed_facts.rs
- src/core/models.rs
- src/core/models/
- src/core/commitments/
- src/core/schema.rs
- src/core/response.rs
- src/engine/ast/
- src/engine/linker.rs
- src/engine/linker/
- src/engine/languages/
- src/engine/scanner/
- src/db/
- src/mcp/
- src/cli/
- build.rs
- Cargo.toml
- Cargo.lock
- docs/adr/
- docs/architecture/
- docs/reference/
- docs/testing/
- docs/milestones/
- docs/
- README.md
- TESTS.md
- tests/
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
- 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
- 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
- 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
- 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
- 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
- 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 4). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
- 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
- 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
related_plans: []
started_at: 2026-10-13T04:00:44+00:00
---

# Bounded semantic resolution and language ownership

## Purpose and admission

Deliver a more modular static resolver with bounded candidate evidence, honest uncertainty, compile-time language feature decoupling, and proven sub-11.5s cold build performance on the control repository (`commerce-release-update`).

This milestone follows [043](archive/043-test-suite-architecture-and-domain-consolidation.md). It operates independently of installer distribution packaging (Milestone 044). All language feature isolation and decoupling previously slated for distribution are directly owned by this milestone to ensure clean modularity of language profiles. All skill distribution, packaging, and installation into user home directories (`~/.agents`, `~/.gemini`, `~/.claude`) or repository-local agent directories are strictly owned by Milestone 044; this milestone does not touch or distribute skills.

Owner authorization covers preparation of this manifest. Proposed changes to ADR 0008 are explicit decision work in task 1. Downstream candidate inference in tasks 4, 5, and 6 is strictly conditional on owner approval of that decision. If the decision is declined, the system retains current fail-closed semantics (conditional branches and reassignments truncate inference without generating candidates), and downstream task contracts formalize those fail-closed truncation boundaries.

## Evidence baseline and discovery

Source: ContextUnity/contextunity-forge-mcp. The source checkout was inspected directly.

Before execution, load `contextunity-forge` and [root instructions](../../AGENTS.md). Run `code_map_overview` against the actual worktree. Record workspace, generation, admitted roots, language capabilities, skipped/partial files, and current source revision in task evidence. For every changed symbol use search → inspect → explain → impact(depth=1) → tests → digest-checked snippet. Use the returned generation for pagination. Reconcile baseline drift through contract_revision before claiming a changed contract.

### Existing mechanisms to retain

| Source / symbols | Existing capability | Treatment |
|---|---|---|
| `src/core/semantic.rs`: `TypeExpr`, `ValueExpr`, `BindingFact`, `FieldFact` | Typed local semantic facts | Extend only where admitted; keep this IR |
| `src/core/typed_facts.rs`: `TypedFacts`, `FlowStore` | Typed in-memory facts, compressed durable representation, compatibility views | Preserve hydration and serialization contracts |
| `src/engine/linker/value_flow.rs`: `ValueFlowIndex`, `TypeTarget` | Bounded propagation, tracked reads and epochs, cached receivers | Reuse; introduce no parallel inference engine |
| `src/engine/linker/traits.rs`: `LanguageLinker`, `ImportContext` | Language-specific import and receiver hooks | Strengthen responsibility boundaries |
| `src/engine/languages/mod.rs`: `LanguageProfile` | Grammar, extraction, registry and features | Preserve static registry and extension contract |
| `src/engine/linker/resolution.rs`: `ResolutionInput` | Shared reference linking with language/framework-specific state | Move coherent ownership, preserve scheduling |
| `src/db/delta.rs`, `src/engine/scanner/mod.rs` | Incremental updates and semantics versioning | Extend invalidation only for newly introduced dependencies |
| `src/core/models/compact_graph.rs` | Compact internal tags and public string adapters | Keep navigation projection compact |
| `tests/receivers/`, `tests/languages/`, `tests/incremental/` | Positive and negative semantic fixtures | Extend existing domain suites |

The graph is stored in SQLite; FTS5 supplies text retrieval, not semantic resolution. Preserve that distinction in documentation.

## Architectural constraints and decision delta

[ADR 0008](../adr/0008-bounded-static-value-flow.md) currently ends inference at conditional branches and reassignments. The proposed extension permits finite candidate evidence for explicitly supported, complete local branch joins only. Unknown or unsupported alternatives retain unknown state; they cannot be dropped to manufacture a unique result. Keep arbitrary dynamic dispatch, mutation, and macro expansion outside exact-call inference.

The contract strictly distinguishes:
1. `MAX_VALUE_FLOW_TYPE_DEPTH` (existing AST/expression evaluation depth limit).
2. `MAX_CANDIDATE_BRANCH_JOINS` ($K=8$, maximum candidate set cardinality across local joins). Overflow beyond $K$ widens explicitly to `Unknown/Incomplete` rather than truncating silently.

[ADR 0018](../adr/0018-layer-dependency-boundaries-and-core-leaf-isolation.md) explicitly permits cohesive multi-pass algorithms to remain intact. Split by responsibility rather than file length. Preserve existing core/engine/db boundaries and avoid generic plugin infrastructure without a concrete consumer.

[ADR 0010](../adr/0010-incremental-delta-transitive-dirtying.md) and [ADR 0012](../adr/0012-zero-regression-serialization-and-cold-build-latency.md) govern incremental consistency, lean projections, compressed facts, and evidence-led performance. Keep unchanged outputs stable; version intentional changes and reject incompatible indexes through existing admission/rebuild paths.

> [!IMPORTANT]
> Invariant: `LanguageLinker` returns a distinction between not-applicable, blocked/unknown, unique and multiple-candidate outcomes. A blocked result cannot fall through to a weaker name match and become exact.

> [!IMPORTANT]
> Invariant: `ValueFlowIndex` retains unknown alternatives and deterministic exhaustion. Candidate evidence assists navigation and conservative impact analysis; it never certifies removal safety.

> [!IMPORTANT]
> Invariant: Cold index build on a clean, isolated scratch copy of the control repository (`commerce-release-update` at commit `cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd`) with scratch adapter pointing to clean isolated copies of linked workspaces (`traverse` at commit `39a1b8065b3d2356727e06fee47c1b5c8c7f04e3`, `grid-view-spec` at commit `d5f8f43e63c012c78af680af3391d04ea4855be7`) must complete within 11.5 seconds wall-clock time on a pre-built release binary (`target/release/contextunity-forge-mcp build <isolated-copy>`) on a lightly loaded host (CPU load < 4) without throughput regression. The owner clarified on 2026-10-13 that `<4` supersedes the earlier `<1.0` threshold.

## Execution order and shared scope

Execute `contract-and-quality-oracle` → `language-feature-decoupling-and-isolated-compilation` → `language-resolution-ownership` → `bounded-candidate-evidence` → `dispatch-and-opacity-navigation` → `integration-and-flavor-parity`.

The tasks deliberately share implementation and documentation files. Local `depends_on` edges serialize those owners; claim a successor only after its predecessor completes. Reuse completed-scope release rather than bypassing locks. Keep one claimed task per overlapping perimeter. An unforeseen file requires admitted extend_scope or contract revision before editing. Record related discoveries as subtasks and unrelated work as deferred findings.

Follow the active pinned profile and returned workflow_guidance; do not change `.forge/acdd/` or hardcode role models. A milestone manifest is retrieved through task/milestone operations, not through doc_search. Copy durable delivered invariants into the indexed documents in each task's scope.

## Tasks

### task: contract-and-quality-oracle

```yaml
task_ref: contract-and-quality-oracle
target: Record the admitted semantic boundary, baseline performance on control repository copy, and characterize existing resolution through real indexing seams
proof_policy: direct-proof
contract_revision: 4
scope:
- docs/adr/
- docs/architecture/modularity.md
- docs/architecture/indexing.md
- docs/reference/languages.md
- docs/testing/README.md
- tests/AGENTS.md
- tests/common/
- tests/linker/
- tests/receivers/
- tests/languages/
- tests/incremental/
depends_on: []
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
- 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
- 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
- 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
status: completed
subtasks:
- subtask_ref: contract-and-quality-oracle-1
  title: Reconcile existing facts and accepted architecture
  status: completed
  evidence: Reconciled live TypeTarget/merge and resolution statuses, existing language parser depth, alias depth, and bounded reducer iteration. Source confirms MAX_VALUE_FLOW_TYPE_DEPTH=8 and SemanticLimits::alias_depth defaults to 64; no independent four-hop factory-return cap is asserted. Current TypeTarget has no candidate variant; merge retains identical known identities, lets Unknown dominate, and widens unequal known targets to Ambiguous. Four direct production-seam tests passed, 0 failed.
- subtask_ref: contract-and-quality-oracle-2
  title: Record owner decision and bounded semantics in ADR 0008
  status: completed
  evidence: 'Recorded the owner-approved K=8 decision in ADR 0008: stable declaration identity, complete-path enumeration, unknown/incomplete alternatives preserved, overflow widens to Unknown/Incomplete, and candidate evidence remains separate from exact calls. Added the versioned Python/Rust/TypeScript/mixed-framework contract matrix and CLI/MCP compatibility/feature-isolation constraints. Direct proof command passed 4 focused tests (0 failures).'
- subtask_ref: contract-and-quality-oracle-3
  title: Establish public-seam ground truth and baseline performance on isolated copy of control repository
  status: completed
  evidence: 'Task 1 baseline complete. Corrected isolated comparison exited 0 at pre-run load 0.95 (2026-10-13T06:21:57.965812681Z) on pinned clean scratch copies: commerce-release-update cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd, traverse 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec d5f8f43e63c012c78af680af3391d04ea4855be7. Receipt: /tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013/comparison.json. Task worktree release binary SHA-256 4d6f52fcdf30a75df55582a96e536e6c904c745a25f8301869189c5f3d0ba8e2. Comparison cold builds wall/CPU/peak RSS: Forge 10,995.749 ms / 48,665.935 ms / 1,378,116 KiB; Codebase Memory 25,738.266 ms / 112,234.788 ms / 4,110,080 KiB (startup 4,203.668 ms). Flattening all raw request runs by backend and process state gives medians (wall/CPU): Forge cold 100.792/147.559 ms (85 samples), warm 3.120/3.024 ms (85 samples); Codebase Memory cold 77.761795/41.511023 ms (90 samples), warm 86.205175/39.824425 ms (90 samples). Maximum sampled request RSS: Forge 64,496 KiB, Codebase Memory 1,113,128 KiB.'
receipt:
  commit: 5a619aee2a8a4c5a47b267fd3fead051399e13c8
  contract_revision: 4
  passed_at: 2026-10-13T09:27:19.590298396+00:00
  evidence:
    command_proof:
      command: cargo test --test linker
      exit_code: 0
      tests_passed: 93
      tests_failed: 0
      log: 'Repaired Task 1 request-median values against all 90 raw Codebase Memory samples per process state. Verified corrected receipt medians (wall/CPU): Forge cold 100.792486/147.559061 ms, warm 3.120345/3.023837 ms; Codebase Memory cold 77.761795/41.511023 ms, warm 86.205175/39.824425 ms. ADR 0008 and subtask-3 evidence now match. Earlier focused verification on unchanged code: cargo test --test linker 93 passed, 0 failed; cargo test --all-targets 661 passed, 0 failed, 1 ignored; strict Clippy passed. Current docs verification: target/release/contextunity-forge-mcp build . exit 0, 288 files, 5171 nodes, 275 doc sections; docs search ''Codebase Memory cold 77.762'' --doc-type adr found one section; docs get updated ADR baseline section returned one match; git diff --check clean. Baseline receipts remain /tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013/comparison.json and standalone-build-measurement.json. Standalone metrics and owner threshold evidence unchanged.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Fresh independent audit confirms the frozen candidate 340e991 contains only the six admitted documentation files; candidate scope matches the Task 1 allowlist, git diff --check is clean, and the pre-existing untracked milestone document is excluded and untouched.
        claims:
          applicable: true
          evidence: 'ADR 0008 and subtask-3 evidence match all raw metrics in /tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013/comparison.json, including corrected Codebase Memory medians: cold 77.761795/41.511023 ms and warm 86.205175/39.824425 ms wall/CPU. Forge cold/warm medians are 100.792486/147.559061 and 3.120345/3.023837 ms. The standalone receipt records exit 0, wall 10.589928868 s, child CPU 48.634931 s, max RSS 1,358,348 KiB, and matching task-release-binary SHA-256 4d6f52fcdf30a75df55582a96e536e6c904c745a25f8301869189c5f3d0ba8e2. Python stdlib subprocess/monotonic/resource is documented as the alternate timer. Owner blackboard message 137 supersedes the imported <1.0 load text with <4; measured load 1.86 complies. Current-vs-future semantic wording and named production-seam fixtures were checked.'
        concurrency:
          applicable: false
          evidence: Documentation-only candidate changes no concurrent runtime, task-state, receipt, or locking behavior.
        project_isolation:
          applicable: true
          evidence: Independent audit confirms benchmark inputs, build database/cache, and linked workspace paths use isolated /tmp scratch copies; the scratch adapter points only to those copies. Binary SHA and pinned repository SHAs are recorded. Scratch copies lack Git metadata, so repository SHAs could not be independently rederived from those copies.
        administration:
          applicable: false
          evidence: Candidate changes no schema, migration, profile, privileged delivery, or repository administration behavior.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Reconcile live `TypeTarget`, resolution statuses, candidate handling, flow limits, feature isolation, and archived milestone 045 with this baseline.
2. Record an owner-approved ADR update for the narrow change to ADR 0008; keep historical rationale and link affected contracts. Clearly distinguish `MAX_VALUE_FLOW_TYPE_DEPTH` from candidate set limit `MAX_CANDIDATE_BRANCH_JOINS` ($K=8$), merge/widening rules, fallback precedence, output location, and public compatibility policy. Explicitly condition downstream branch-inference implementation in tasks 4, 5, and 6 on owner approval of this decision.
3. Establish baseline performance measurements on clean, isolated scratch copies of `commerce-release-update` (worktree `../../worktrees/commerce-release-update` at commit `cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd`) and clean copies of its linked workspaces (`traverse` at commit `39a1b8065b3d2356727e06fee47c1b5c8c7f04e3`, `grid-view-spec` at commit `d5f8f43e63c012c78af680af3391d04ea4855be7`):
   - Rewrite `forge-mcp.yaml` in the scratch `commerce-release-update` directory so that `linked_workspaces` point directly to the isolated clean scratch copies of `traverse` and `gridviewspec` rather than primary repository trees.
   - Compile release binary first via `cargo build --release`.
   - Measure cold index build wall-clock time using the pre-compiled binary: `/usr/bin/time -v target/release/contextunity-forge-mcp build <isolated-copy>` on a host with CPU load < 1.0; verify cold build completes in $\le 11.5$s wall-clock time.
   - Run tool comparison benchmark via `python benchmarks/run_benchmarks.py --profile benchmarks/profiles/commerce-release-update.json --mode comparison --workspace <isolated-copy>`; record the commit SHAs of all three repositories (`commerce-release-update: cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd`, `traverse: 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3`, `grid-view-spec: d5f8f43e63c012c78af680af3391d04ea4855be7`) and baseline median wall times (warm and cold process), CPU times, and peak RSS across scenarios in task evidence.
4. Define a versioned table of exact targets, possible targets, opaque/unknown cases, and forbidden false positives across Python, Rust, TS, and mixed frameworks.
5. Capture current results through scanner → linker → SQLite → public queries with direct-proof.

Verification: unfiltered `linker`, `languages`, `incremental` targets for changed suites; documentation metadata/links and retrieval checks. Use direct-proof for this characterization/decision slice.

---

### task: language-feature-decoupling-and-isolated-compilation

```yaml
task_ref: language-feature-decoupling-and-isolated-compilation
target: Isolate cross-language linker and AST dependencies behind proper cfg-gates so single-language feature sets compile cleanly
proof_policy: direct-proof
contract_revision: 4
scope:
- src/engine/ast/mod.rs
- src/engine/linker.rs
- src/engine/linker/resolution.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/value_flow.rs
- src/engine/languages/
- Cargo.toml
- tests/languages/
depends_on:
- contract-and-quality-oracle
invariants:
- 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
status: completed
subtasks:
- subtask_ref: cfg-gate-typescript-dom-bindings
  title: Enclose typed_dom_property, typed_dom_receiver, and prototype_visible_at in src/engine/linker/resolution.rs, value_flow.rs, and linker.rs under cfg(feature = 'lang-typescript') with fallback no-op stubs
  status: completed
  evidence: 'Completed: linker.rs now exposes lang-typescript-only typed DOM wrappers with None fallback stubs; resolution.rs and value_flow.rs route through those wrappers. prototype_visible_at has an always-available no-TypeScript fallback returning true. Verified `cargo check --no-default-features --features lang-python` exits 0.'
- subtask_ref: cfg-gate-commonjs-callables
  title: Enclose commonjs_default_callable in src/engine/ast/mod.rs under cfg(feature = 'lang-typescript') with fallback no-op stub
  status: completed
  evidence: 'Completed: ast/mod.rs routes CommonJS callable detection through a lang-typescript-gated helper with a false fallback when TypeScript is disabled. Verified isolated Python check exits 0.'
- subtask_ref: verify-isolated-language-compilation
  title: 'Verify clean compilation across isolated feature sets: lang-python, lang-rust, lang-typescript, lang-vue (--no-default-features), and default'
  status: completed
  evidence: 'Completed: all pass (exit 0): `cargo check --no-default-features --features lang-python`; `cargo check --no-default-features --features lang-rust`; `cargo check --no-default-features --features lang-typescript`; `cargo check --no-default-features --features lang-vue`; `cargo check`.'
- subtask_ref: cfg-gate-typescript-members-context
  title: Enclose TypeScriptMembers in src/engine/linker/semantic_context.rs, resolution.rs, and linker.rs under cfg(feature = 'lang-typescript')
  status: completed
  evidence: 'Completed: TypeScriptMembers context declaration/call paths were already guarded by lang-typescript cfg at semantic_context.rs, resolution.rs, and linker.rs; verified in the isolated Python, Rust, Vue and default compilation matrix.'
receipt:
  commit: 479b49c380629ba35f9230f68bf5f5d5ca923cc2
  contract_revision: 4
  passed_at: 2026-10-13T10:08:39.633869372+00:00
  evidence:
    command_proof:
      command: cargo test --test languages
      exit_code: 0
      tests_passed: 253
      tests_failed: 0
      log: 'Also passed: cargo check --no-default-features --features lang-python; lang-rust; lang-typescript; lang-vue; cargo check; cargo clippy --all-targets --all-features -- -D warnings; cargo fmt -- --check; git diff --check (all exit 0).'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Compared the worktree status and candidate snapshot: only the five modified files are src/engine/ast/mod.rs, src/engine/linker.rs, src/engine/linker/resolution.rs, src/engine/linker/semantic_context.rs, and src/engine/linker/value_flow.rs, all admitted by task scope. No staged or untracked files, Cargo.toml change, profile/configuration change, or neighboring-project path change. Every path in snapshot 5433c5f blob-hash-matches the review worktree.'
        claims:
          applicable: true
          evidence: 'The TypeScript CommonJS helper is called only through a lang-typescript-gated wrapper with false fallback; DOM property/receiver helpers route through wrappers with None fallbacks; prototype visibility has a no-TypeScript true fallback. TypeScriptMembers storage/setup paths are cfg-gated, and the Rust Self::/Self. branch is now wholly lang-rust-gated. Cargo declares optional TypeScript and JavaScript grammars under lang-typescript and lang-vue depends on lang-typescript. Accepted build proof is bound to snapshot 5433c5f and records Python, Rust, TypeScript, Vue, and default checks plus strict all-feature Clippy, all exit 0. Reviewer reran the complete target: cargo test --test languages, exit 0, 253 passed, 0 failed. Coverage limitation: fallback semantics are compile-checked by the feature matrix; no new standalone test asserts the false/None stub values.'
        concurrency:
          applicable: true
          evidence: No task store, transaction, lock, receipt, or shared-state code changed. Forge records the accepted build at claim revision 6 against baseline head 5a619aee2a8a4c5a47b267fd3fead051399e13c8 and candidate 5433c5f; this review is independently claimed at revision 7. Candidate file hashes still match the snapshot after review verification, so no stale candidate or partial publication was introduced.
        project_isolation:
          applicable: true
          evidence: The task ID and milestone resolve to repository forge-mcp, and the review claim uses its declared isolated task worktree. All modified paths are within that task's allowed scope; no linked workspace, task namespace, profile path, symlink, traversal, or repository-root handling was changed.
        administration:
          applicable: true
          evidence: No profile, migration, hook configuration, build script, or generated registry was modified; the protected .forge/acdd profile is clean. The active claim returned the compiled standard five-contour review policy, and the build receipt preserves the validated candidate baseline. Hook execution and the landed task SHA are delivery-stage checks; this review makes no claim that a delivery receipt already exists.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Enclose DOM property and receiver lookup in `src/engine/linker/resolution.rs` and `src/engine/linker/value_flow.rs` under `cfg(feature = "lang-typescript")` with no-op fallback stubs when disabled.
2. Enclose `commonjs_default_callable` in `src/engine/ast/mod.rs` under `cfg(feature = "lang-typescript")` with fallback stub.
3. Enclose `TypeScriptMembers` references and initialization in `src/engine/linker/semantic_context.rs`, `src/engine/linker/resolution.rs`, and `src/engine/linker.rs` under `cfg(feature = "lang-typescript")`.
4. Prove that compiling with `--no-default-features --features lang-python` or `--features lang-rust` compiles cleanly without missing symbols or pulling TypeScript grammar/linker code.
5. Verify clean compilation for `--no-default-features --features lang-typescript` and dependent profile `--no-default-features --features lang-vue`.
6. Preserve all existing tests and resolutions under default features.

Verification: direct-proof via `cargo check --no-default-features --features lang-python`, `cargo check --no-default-features --features lang-rust`, `cargo check --no-default-features --features lang-typescript`, `cargo check --no-default-features --features lang-vue`, and full default `cargo test --test languages`.

---

### task: language-resolution-ownership

```yaml
task_ref: language-resolution-ownership
target: Move language and framework decisions behind explicit ownership while preserving observable graph behavior
proof_policy: direct-proof
contract_revision: 4
scope:
- src/core/semantic.rs
- src/core/typed_facts.rs
- src/engine/linker.rs
- src/engine/linker/
- src/engine/languages/
- src/engine/ast/
- tests/linker/
- tests/receivers/
- tests/languages/
- tests/core/
- tests/incremental/
- tests/common/
- docs/architecture/modularity.md
- docs/architecture/indexing.md
- docs/reference/languages.md
- docs/testing/README.md
depends_on:
- language-feature-decoupling-and-isolated-compilation
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
- 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
status: completed
subtasks:
- subtask_ref: language-resolution-ownership-1
  title: Encapsulate language resolution state
  status: completed
  evidence: Borrowed LanguageMemberLookup replaces per-language member-index fields in ResolutionInput; cargo test --test linker passed 93 tests.
- subtask_ref: language-resolution-ownership-2
  title: Separate cross-language framework passes
  status: completed
  evidence: framework_relations::compose owns route/HTMX/render/template/context-provider graph joins; cargo test --test linker passed 93 and cargo test --test languages passed 253, preserving final deterministic sort.
- subtask_ref: language-resolution-ownership-3
  title: Prove output and feature parity
  status: completed
  evidence: 'Parity proof passed: linker 93, languages 253, core 31 (1 ignored), incremental 23, commitment_integrity 14; feature cargo check matrix passed for Python/Rust/TypeScript/Vue with no defaults and default; strict Clippy, fmt, and diff checks passed.'
receipt:
  commit: c575b6284f35b1bcbad54592983f7c620119156c
  contract_revision: 4
  passed_at: 2026-10-13T10:53:37.241895758+00:00
  evidence:
    command_proof:
      command: cargo test --test linker && cargo test --test languages && cargo test --test core && cargo test --test incremental && cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check && git diff --check && cargo check --no-default-features --features lang-python && cargo check --no-default-features --features lang-rust && cargo check --no-default-features --features lang-typescript && cargo check --no-default-features --features lang-vue && cargo check
      exit_code: 0
      tests_passed: 414
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Candidate snapshot acef4e4 changes only docs/architecture/indexing.md, docs/architecture/modularity.md, docs/reference/languages.md, docs/testing/README.md, src/engine/linker.rs, src/engine/linker/resolution.rs, src/engine/linker/semantic_context.rs, and adds src/engine/linker/framework_relations.rs; all are within the admitted task scope. Worktree/snapshot blob hashes match, git diff --check passes, and no profile, repository configuration, credential, or neighboring-project path changed. Documentation governance conflict #136 is recorded at milestone scope: docs/AGENTS.md asks for skill/example updates, while 046 excludes those paths; the recorded disposition is to keep this task in scope and reconcile at handoff or a separate docs task.'
        claims:
          applicable: true
          evidence: 'ResolutionInput no longer stores PythonReceivers, RustMembers, TypeScriptMembers, or RouteIndex. LanguageMemberLookup borrows the already-built Context indexes and directly forwards the same lookup arguments; its methods create no Vec/String or clone and preserve the previous member outcome and candidate precedence. framework_relations::compose owns calls_endpoint, HTMX, renders, template-reference, and context_provider output; its moved branches retain external/resolved/unresolved/ambiguous outcomes. linker.rs merges the composition graph before the unchanged sort_graph validation/order. HTML code remains lang-html-gated; Rust/TypeScript façade methods remain feature-gated. Existing public linker/language production tests cover receiver precedence, endpoint routes, HTMX, template targets, and ambiguous providers. Reviewer verification passed: cargo test --test linker (93 passed) and cargo test --test languages (253 passed), both exit 0.'
        concurrency:
          applicable: true
          evidence: The patch changes no task store, transaction, locking, or receipt code. Forge accepted the build at claim revision 6 for candidate acef4e4 with candidate_baseline_head 479b49c380629ba35f9230f68bf5f5d5ca923cc2; the review is a separate worker claim at revision 7. Worktree HEAD remains that validated baseline and all captured snapshot file hashes still match after review tests, so no stale candidate or partial state was introduced.
        project_isolation:
          applicable: true
          evidence: The task ID resolves to forge-mcp and the review ran from its declared isolated worktree. Every changed path is inside the task perimeter. framework_relations consumes the existing indexed facts, owner set, profile map, and nodes; it adds no filesystem/path resolution or linked-workspace access. No symlinks, traversal segments, repository roots, or task namespaces changed.
        administration:
          applicable: true
          evidence: 'No migration, profile, protected .forge configuration, build script, or gate definition changed. Forge returned the configured review contour set and accepted build proof bound to the candidate baseline above. The active review precedes delivery, so Git hook execution and the landed task SHA remain delivery-gate checks; this proof does not assert a landed receipt exists yet. The skill/example omission is the already-deferred milestone finding #136, not scope added here.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Reproduce the accepted contract seam before editing; where output is intentionally unchanged, use an approved existing-boundary direct proof.
2. Encapsulate Rust/Python/TypeScript member indexes within their language resolution ownership. Common scheduling consumes a minimal read-only lookup/result interface rather than one field per language. Preserve borrowed data and avoid per-reference SQL, disk I/O, or rebuilding indexes.
3. Distinguish not-applicable from blocked/unknown outcomes. Preserve resolution precedence, overload/stub rules, declaration scope, and fail-closed ambiguity.
4. Give cross-language route/template relations a composition pass with explicit input facts and output edges. Preserve phase ordering so framework edges do not depend on arbitrary registry iteration.
5. Move language-specific literal interpretation and builtin guards to extraction/language ownership where semantics can be preserved.
6. Preserve public JSON, graph ordering, and commitment bytes for unchanged inputs.

Verification: direct-proof via `cargo test --test linker`, `cargo test --test languages`, `cargo test --test core`, `cargo test --test incremental`, and `cargo test --test commitment_integrity`. Negative fixtures demonstrate preserved shadowing, mutation, ambiguous imports, and unresolved receivers.

---

### task: bounded-candidate-evidence

```yaml
task_ref: bounded-candidate-evidence
target: Retain finite candidate evidence or formalize fail-closed branch truncation based on ADR 0008 decision
proof_policy: seam-test-first
contract_revision: 6
scope:
- src/core/semantic.rs
- src/core/typed_facts.rs
- src/core/models.rs
- src/core/models/
- src/core/commitments/
- src/engine/linker.rs
- src/engine/linker/
- src/engine/languages/
- src/engine/scanner/
- src/core/schema.rs
- src/db/writer.rs
- src/db/reader.rs
- src/db/reader/
- src/db/delta.rs
- src/db/ingest.rs
- src/db/symbols/search.rs
- src/db/symbols/test_discovery.rs
- tests/linker/
- tests/receivers/
- tests/languages/
- tests/core/
- tests/incremental/
- tests/commitment_integrity/
- tests/commitment_integrity.rs
- tests/common/
- docs/architecture/modularity.md
- docs/architecture/indexing.md
- docs/reference/languages.md
- docs/testing/README.md
- docs/milestones/046-bounded-semantic-resolution-and-language-boundaries.md
depends_on:
- language-resolution-ownership
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
- 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
- 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
status: completed
subtasks:
- subtask_ref: bounded-candidate-evidence-1
  title: Implement bounded candidate domain and joins (or formalize fail-closed branch truncation)
  status: completed
  evidence: 'cargo test --test linker: 97 passed. Complete Python branch joins preserve source BindingFact cardinality, stable candidate order/identity, Unknown alternatives, K=8 accepted as ambiguous, K=9 widened to Unknown/Incomplete with no partial candidate prefix; later alias reads receive possible_calls only, never exact calls.'
- subtask_ref: bounded-candidate-evidence-2
  title: Persist compact provenance and compatible facts
  status: completed
  evidence: 'cargo test --test linker: 97 passed, including receiver production-path persistence/retrieval coverage showing possible_calls provenance persists while exact calls remain absent. cargo test --test commitment_integrity: 15 passed; logical FTS postings and configuration are committed and tampering/query-only verification are covered.'
- subtask_ref: bounded-candidate-evidence-3
  title: Prove deterministic exhaustion and delta parity
  status: completed
  evidence: 'cargo test --test incremental: 24 passed, including branch-join cold/delta parity and deterministic exhaustion diagnostics. ADR 0004 bit-identical root parity is preserved; final combined five-target suite passed 420, failed 0, ignored 1.'
receipt:
  commit: e75639c3437b65f2badcfd8626d906599f31a866
  contract_revision: 6
  passed_at: 2026-10-13T13:02:01.816263128+00:00
  evidence:
    command_proof:
      command: cargo test --test linker --test languages --test core --test incremental --test commitment_integrity --quiet
      exit_code: 0
      tests_passed: 421
      tests_failed: 0
      log: 'Targets: linker 97; languages 253; core 31 passed, 1 ignored; incremental 25; commitment_integrity 15. cargo clippy --all-targets --all-features -- -D warnings passed; cargo fmt --check passed; git diff --check passed. No benchmarks run.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: The 21 changed paths are within the admitted scope, including src/core/commitments.rs and tests/incremental/doc_parity.rs. No staged or untracked source/config files were present. git diff --check passed, and explicit git diff --exit-code ed89a45 -- <all 21 changed paths> returned clean, confirming the reviewed contents match candidate snapshot ed89a45.
        claims:
          applicable: true
          evidence: 'Python branch joins remain separate transfer facts, preserve source binding cardinality, deduplicate local declaration identities deterministically, retain unknown alternatives, accept K=8, and widen K>8 to Unknown/Incomplete without a partial prefix. Resolution persists candidates only as possible_calls. FTS v2 hashes node postings by nodes.node_hash and document postings by doc_sections.doc_id, checks postings have owners, and validates active docsize token counts. tests/incremental/doc_parity.rs now edits an earlier document while a later section retains the maximum rowid and asserts delta/cold output roots match. Reviewer command `cargo test --test linker --test languages --test core --test incremental --test commitment_integrity --test query_context --quiet` exited 0: 461 passed, 0 failed, 1 ignored. No benchmarks run, as instructed.'
        concurrency:
          applicable: true
          evidence: FTS vocabulary views remain connection-local; snapshot hashing uses separate read-only connections, restores query_only after temporary view creation, and installs commitments transactionally. The fix adds only deterministic owner joins during digesting and does not alter task claim or shared task-state operations. No concurrency defect found.
        project_isolation:
          applicable: true
          evidence: All changed paths are in this repository's admitted source, test, and documentation scope. FTS vocabulary queries are explicitly bound to main.node_search/main.doc_search and map postings to owner identities in the same database. No workspace adapter, linked-workspace path, or project configuration changed.
        administration:
          applicable: true
          evidence: The logical FTS format remains explicitly versioned as forge-owner-sha256-v2. No task store, gate profile, credentials, hooks, or delivery/migration behavior changed. No administration-specific defect found.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Implementation is conditional on ADR 0008 update approval in task 1:
   - **If ADR 0008 is approved**:
     - Implement finite candidate evidence semantics accepted in task 1; reuse `TypeExpr`, `ValueExpr`, typed facts, and bounded evaluator; represent finite possible declarations separately from unknown/opaque alternatives.
     - Sort/deduplicate candidates by stable identity; bound set cardinality to $K=8$ (`MAX_CANDIDATE_BRANCH_JOINS`), distinct from `MAX_VALUE_FLOW_TYPE_DEPTH`. Overflow beyond $K$ widens explicitly to `Unknown/Incomplete` state. Report budget exhaustion rather than presenting a partial set as exhaustive.
     - First implement complete local branch joins in one explicitly supported syntax family, then equivalent admitted cases for other supported languages. Preserve unknown writes, alias-cycle termination, scope visibility, and existing annotation precedence.
     - Keep declaration resolution distinct from candidate dispatch. Store compact navigational provenance/reason and bounded target references; keep full flow facts in the compressed store, outside `nodes.details`/FTS duplication.
     - Prove cold/delta equivalence after changing a factory return, exported type, branch alternative, or candidate member; deletion removes stale candidates.
   - **If ADR 0008 is declined**:
     - Preserve and formalize strict fail-closed truncation at conditional branches and reassignments; branch joins unconditionally truncate inference and widen to `Unknown/Incomplete` without candidate generation.
     - Do not create candidate relations or persist $K=8$ candidate sets; record explicit truncation in provenance.
     - Formalize branch-exhaustion boundaries and revise contract through contract_revision to declare branch-truncation invariants as permanent design.
2. Preserve unchanged encodings. Version intentionally changed semantic facts/index behavior via existing version checks and rebuild admission.
3. Verification: red/green public seam in existing `linker` suite; unfiltered `linker`, `languages`, `core`, `incremental`, `commitment_integrity` for affected code:
   - If ADR 0008 approved: test cases include one candidate, two candidates, exactly $K$, $K+1$, unknown alternative, reassignment, and cyclic alias.
   - If ADR 0008 declined: test cases prove immediate fail-closed widening to Unknown on conditional branch joins and reassignments without manufacturing candidate edges.
   - Existing exact fixture results remain unchanged in either case.

---

### task: dispatch-and-opacity-navigation

```yaml
task_ref: dispatch-and-opacity-navigation
target: Expose declaration targets, possible dispatch and analysis gaps consistently to coding agents
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/core/models.rs
- src/core/models/
- src/core/response.rs
- src/engine/linker/
- src/engine/languages/rust/
- src/db/reader.rs
- src/db/reader/
- src/db/symbols.rs
- src/db/traversal.rs
- src/mcp/tools.rs
- src/mcp/server.rs
- src/cli/mod.rs
- tests/linker/
- tests/receivers/
- tests/query_context/
- tests/impact_context.rs
- tests/mcp/
- tests/incremental/
- tests/common/
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- docs/architecture/modularity.md
- docs/architecture/indexing.md
- docs/reference/languages.md
- docs/testing/README.md
depends_on:
- bounded-candidate-evidence
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
- 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
status: completed
subtasks:
- subtask_ref: dispatch-and-opacity-navigation-1
  title: Expose trait and macro uncertainty
  status: completed
  evidence: Rust dyn parameter value-flow facts identify the declared trait receiver; resolver emits trait_object_dispatch gap while preserving macro invocation -> declaration calls edge and emits macro_expansion_not_analyzed. Proven by rust_trait_object_and_macro_dispatch_expose_scoped_analysis_gaps in query_context.
- subtask_ref: dispatch-and-opacity-navigation-2
  title: Integrate conservative query/removal semantics
  status: completed
  evidence: possible_calls remains a separate confidence=possible edge. Inbound traversal includes candidate-only callers and removal lists the possible_calls dependency and blocks safety. Proven by explain_pages_possible_calls_as_distinct_navigation_edges and candidate_only_callers_appear_in_impact_and_block_removal.
- subtask_ref: dispatch-and-opacity-navigation-3
  title: Keep CLI MCP and reference documentation aligned
  status: completed
  evidence: Updated MCP/CLI, language, architecture, and testing references for paged analysis_gaps, possible_calls impact/removal behavior, and Rust trait/macro limits; fmt and diff checks pass.
receipt:
  contract_revision: 4
  passed_at: 2026-10-13T14:40:06.551851294+00:00
  evidence:
    command_proof:
      command: cargo test
      exit_code: 0
      tests_passed: 672
      tests_failed: 0
      log: 'Exit 0; 672 passed, 0 failed, 1 ignored across default targets. Affected domain targets independently passed: linker 97, query_context 42, impact_context 4, mcp 78, incremental 25. `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt -- --check`, and `git diff --check` passed. No standalone receivers test target exists; receiver coverage is included in linker.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Inspected candidate snapshot 3e9e5e78c6ce283f1d3c86998223abaface70c29 and the worktree diff. All 13 changed files are within the admitted task scope; no staged or untracked changes, profile/repository configuration, generated state, or neighboring project paths changed. `git diff --check` passes, and the current worktree content matches the candidate snapshot.
        claims:
          applicable: true
          evidence: 'Traced Rust dyn receiver type facts through trait_object_name/type_expr_inner and the Rust resolver: trait-object dispatch emits unresolved `trait_object_dispatch` coverage without selecting concrete impl calls. Resolved macro candidates keep the calls edge to the declaration and emit `macro_expansion_not_analyzed`. `with_analysis_gaps` returns deterministically ordered, generation-fenced paged evidence. `possible_calls` remains a distinct `confidence=possible` edge; inbound traversal includes it and removal assesses it as a blocker. Named tests use temporary workspaces, production indexing, SQLite persistence, and query/traversal readers. Reviewer commands passed: `cargo test --test linker` (97), `cargo test --test query_context` (42), `cargo test --test impact_context` (4), `cargo test --test mcp` (78), and `cargo test --test incremental` (25); total 246 passed, 0 failed.'
        concurrency:
          applicable: true
          evidence: The change adds per-file graph-local coverage and read-only SQL paging through the existing connection/paging generation checks; it introduces no shared mutable state, lock, transaction, task claim, or receipt writes. Resolver lookup uses the already-built immutable value-flow index. No concurrency-sensitive mutation path changed.
        project_isolation:
          applicable: true
          evidence: Gap queries bind the selected indexed node's workspace-relative path and line span as SQL parameters against the current index connection. The new tests create isolated temporary workspaces and exercise the production writer/reader paths. No adapter, linked-workspace configuration, or filesystem path resolution changed.
        administration:
          applicable: true
          evidence: No task-store, migration, profile-pin, Git hook, candidate-delivery, or receipt code/configuration changed. Snapshot and scope checks found no administrative or protected-path modifications.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 1.0). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Implementation of candidate navigation and dispatch relations is conditional on ADR 0008 update approval in task 1:
   - If ADR 0008 is approved: for admitted Rust trait-object fixtures and branch joins, expose resolvable declarations and candidate relations distinctly from exact calls. Inbound impact conservatively accounts for admitted possible references; removal analysis accounts for candidate callers (red seam test on candidate-only caller).
   - If ADR 0008 is declined: do not introduce candidate branch edges; inbound impact and removal analysis maintain fail-closed guarantees; red seam test proves analysis gap exposure for macro invocations and trait-object declarations without candidate dispatch edges.
2. Preserve macro invocation-to-declaration links when available and record expansion-not-analyzed as an analysis gap. Do not execute procedural macros, Cargo build scripts, or target code.
3. Define scope and provenance of each gap. Inspect/explain show relevant gaps; if candidate semantics are approved, inbound impact conservatively accounts for admitted possible references; removal analysis never turns absence of exact callers into unconditional safety when relevant uncertainty remains. If candidate semantics are declined, analysis gaps are reported directly without candidate relations.
4. Public CLI/MCP responses share the same semantics, confidence labels, limits, and pagination. Candidate truncation includes an explicit indicator; preserve the 64 KiB cap and generation fencing.
5. Document the bounded static estimate and runtime/opaque-analysis limits for `code_map_prove_removal` in `docs/reference/mcp-tools.md` and CLI documentation. Skill distribution and installation into global/local agent environments remain strictly owned by Milestone 044.

Verification: `linker`, `query_context`, `impact_context`, `mcp`, `incremental`. Test trait declaration versus implementation, macro gap, overflow, stale generation, and bounded payload through actual production paths. If ADR 0008 is approved, test candidate-only caller and candidate truncation; if declined, verify fail-closed truncation and gap reporting.

---

### task: integration-and-flavor-parity

```yaml
task_ref: integration-and-flavor-parity
target: Verify end-to-end semantic contracts across isolated Cargo feature sets, verify control repository performance, and publish durable indexed guidance
proof_policy: deferred-final-test
contract_revision: 8
scope:
- Cargo.toml
- Cargo.lock
- build.rs
- src/core/commitments.rs
- src/db/ingest.rs
- tests/languages/features.rs
- tests/languages/
- tests/linker/
- tests/receivers/
- tests/incremental/
- tests/mcp/
- tests/core/
- tests/common/
- tests/commitment_integrity/
- tests/commitment_integrity.rs
- docs/architecture/
- docs/reference/languages.md
- docs/reference/mcp-tools.md
- docs/reference/mcp-setup.md
- docs/reference/cli.md
- docs/testing/README.md
- docs/milestones/README.md
- docs/README.md
- README.md
- TESTS.md
- docs/milestones/046-bounded-semantic-resolution-and-language-boundaries.md
depends_on:
- dispatch-and-opacity-navigation
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
- 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
- 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 4). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
- 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
- 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
status: completed
subtasks:
- subtask_ref: integration-and-flavor-parity-1
  title: Prove end-to-end cold/delta/query behavior and Merkle integrity
  status: completed
  evidence: 'Completed: cargo test --test commitment_integrity 16/16, cargo test --test incremental 25/25, and cargo test --all-targets passed (672 passed, 0 failed, 1 ignored). Review approved production-seam implementation at snapshot d808c6f; cold/delta results and commitment integrity are covered.'
- subtask_ref: integration-and-flavor-parity-2
  title: Verify clean compilation and test behavior across modular Cargo features
  status: completed
  evidence: 'Completed: cargo check passed for no-default lang-python, lang-rust, lang-typescript, lang-vue and default features; all-target tests passed. Existing feature-off dead-code warnings are disclosed in the build proof.'
- subtask_ref: integration-and-flavor-parity-3
  title: Verify performance on isolated copy of control repository (cold build <= 11.5s) and publish guidance
  status: completed
  evidence: 'Completed: isolated benchmark receipts compare Task 6 with Task 1. Bash builtin time cold build 11.330s at pre-run load 2.64 (<4), exit 0. Forge query medians improved cold 98.38→96.64ms and warm 3.12→2.53ms; max sampled Forge RSS improved cold 60,260→59,924KiB and warm 64,496→63,036KiB. Codebase comparator cold median increased 12.3% and warm decreased 2.5%; disclosed in receipt. No timing reruns.'
- subtask_ref: integration-and-flavor-parity-4
  title: Remediate measured cold-build regression in logical FTS commitment and ingest paths
  status: completed
  evidence: 'Completed: src/core/commitments.rs and src/db/ingest.rs remediate the measured logical FTS commitment/ingest cold-build cost while preserving deterministic commitments; tests/commitment_integrity.rs verifies insertion-order invariance. Approved isolated Bash build was 11.330s (<11.5s).'
receipt:
  contract_revision: 8
  passed_at: 2026-10-13T18:07:48.648317973+00:00
  evidence:
    command_proof:
      command: cargo test --test commitment_integrity; cargo test --test incremental; cargo clippy --all-targets --all-features -- -D warnings; cargo test --all-targets; cargo check --no-default-features --features lang-python; cargo check --no-default-features --features lang-rust; cargo check --no-default-features --features lang-typescript; cargo check --no-default-features --features lang-vue; cargo check; cargo fmt --all -- --check; git diff --check; Bash builtin time precompiled release build on isolated scratch copy
      exit_code: 0
      tests_passed: 672
      tests_failed: 0
      log: 'Task 6 rev8 scope now explicitly admits the active milestone file; the updated candidate snapshot also includes tests/commitment_integrity.rs. This revision adds only task-scope/contract metadata and fixes indentation in the already completed Task 4 YAML block so Forge can parse the manifest; production code and test sources are unchanged from the full rev7 validation. `git diff --check` passes on the exact rev8 worktree. Focused commitment_integrity was rerun after rev7: 16/16; focused incremental: 25/25. Strict Clippy passed. cargo test --all-targets: unit 43; main 0; acdd 74; cli_roots 7; commitment_integrity 16; core 31 passed/1 ignored; impact_context 4; incremental 25; languages 253; linked_workspaces 2; linker 97; mcp 78; query_context 42; total 672 passed, 0 failed, 1 ignored. All five feature checks passed: lang-python, lang-rust, lang-typescript, lang-vue with no defaults, and default cargo check. Feature-off builds emit existing unused/dead-code warnings; default is warning-free. cargo fmt --all -- --check passed. Cold-build receipt is the approved Bash builtin `time` measurement (no external timer): /tmp/contextunity-forge-m046-final/results/task6/hot-loop-cold-build-bash-time.log. Clean isolated Commerce scratch copy; immediate pre-run load 2.64 (<4); exit 0; elapsed 11.330s, within 11.5s by 0.170s. Binary SHA256 d798cd5b7e415e8d5a3ec795e90e275b990c902b7cab023609109878543716d7; output root 2c39820507fa50ab53008442e247b20cd25fbe8a2d077007ed0425a7f4229258. Five-repeat comparison receipt /tmp/contextunity-forge-m046-final/results/task6/hot-loop-comparison/comparison.json against Task1 baseline /tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013/comparison.json: aggregate Forge query medians cold 98.38→96.64ms (-1.8%), warm 3.12→2.53ms (-18.8%); maximum sampled Forge RSS across scenarios improved cold 60,260→59,924 KiB and warm 64,496→63,036 KiB. Individual scenario timings/RSS are preserved in the receipt. Codebase comparator cold median 77.69→87.22ms (+12.3%), warm 83.61→81.51ms (-2.5%); this is the comparator backend, not Forge. Profile-reported Forge index-build measurement is 11,557.6ms wall / 51,378.8ms CPU / 1,418,036KiB RSS; it is disclosed separately from the acceptance''s direct Bash timing. No timing reruns were performed.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Verified against contract rev8 and allowed_write_scope. `git status --short` in the task worktree lists only docs/milestones/046-bounded-semantic-resolution-and-language-boundaries.md, src/core/commitments.rs, src/db/ingest.rs, and tests/commitment_integrity.rs. All four are admitted; `git diff d808c6f -- <these paths>` is empty, so the worktree content matches the frozen candidate. Snapshot d808c6f contains the active milestone file (blob 1aa407241efdd203ffd058b2c8a342685dc3758c) and standalone tests/commitment_integrity.rs (blob c874dc30990a36770bb5efbea6089a6751840a8d). The baseline-to-candidate functional diff is confined to those four admitted paths; `git diff --check` is clean. No profile, credential, generated-state, or neighboring-project path changed.
        claims:
          applicable: true
          evidence: The code keeps ALGORITHM at `forge-owner-sha256-v2`. `append_fts_posting` emits the same 0xff row marker and t/i type tags and payload encoding as `encode_row`; term, owner, column, and offset sort keys preserve the prior logical SQL order while doc_search hashes stable doc_sections.doc_id and node_search hashes nodes.node_hash. The implementation rejects unknown columns/terms, missing FTS documents or owner identities, missing/invalid docsize rows, and token-count mismatches. The admitted production-seam coverage includes tests/commitment_integrity.rs::logical_fts_commitments_ignore_physical_insertion_order, read_only_query_only_verification_uses_temporary_logical_fts_views, logical_fts_commitments_are_stable_and_reject_semantic_tampering, and fts_sealing_survives_valid_rebuild_reindex_and_vacuum; tests/incremental/doc_parity.rs::delta_and_cold_roots_match_after_document_rowid_changes exercises cold/delta parity with changing document rowids. Forge build claim 19 binds to d808c6f and records commitment_integrity 16/16, incremental 25/25, all targets 672 passed/0 failed/1 ignored, strict Clippy, formatting/diff checks, and Python/Rust/TypeScript/Vue no-default plus default Cargo checks. Query comparison receipts report Forge cold median 98.38→96.64 ms, warm 3.12→2.53 ms, and lower maximum sampled query RSS for both states.
        concurrency:
          applicable: true
          evidence: No task-store, revision fencing, receipt, lock, or cross-task code changes are in the candidate. In ingest.rs the code computes stable search_id once and reuses it within the existing transaction/connection path; only the already-owned FTS batch is sorted before flush. Commitment verification remains read-only apart from the existing temporary logical FTS vocabulary views, with query_only restoration covered by the production test. There is no newly introduced shared mutable state or partial publication path.
        project_isolation:
          applicable: true
          evidence: The review claim and worktree identify the owning forge-mcp project and milestone worktree. No workspace or path-routing logic changed. The dedicated timing receipt runs the candidate release binary against `/tmp/contextunity-forge-m046-final/task6-hot-loop-20261013-commerce`; its scratch forge-mcp.yaml points traverse and gridviewspec to isolated `/tmp/contextunity-forge-m046-final/task6-optimized-20261013-*` copies. The comparison receipt records a disposable workspace root and disposable Forge/Codebase indexes; the build proof identifies pinned clean baseline repositories and states primary baseline worktrees were not modified.
        administration:
          applicable: true
          evidence: The active milestone path is now part of contract revision 8 and is present in snapshot d808c6f. Reviewer worker codex-046-integration-flavor-review-r8 is distinct from build worker codex-046-root-build-rev8; this review is claim revision 20 bound to contract revision 8, candidate d808c6f, and baseline eeaa282e83cc0d380b36ea2eafdffa20458aad54. No Cargo feature configuration, profile pin, migration, protected config, credential, Git hook, or task administration source changed. Delivery/landed-SHA receipt remains governed by the subsequent deliver stage.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence never becomes an exact runtime-call claim; unsupported paths, unknown writes, overflow, and opaque expansion retain explicit uncertainty.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-LAYERS: Keep core a leaf and preserve the frozen engine/database dependency boundary in ADR 0018.'
    - 'INV-OWNERSHIP: Language rules belong to language resolvers; cross-language framework relations use explicit composition passes; common scheduling and budgets remain shared.'
    - 'INV-BOUNDS: Candidate sets, inference propagation, retrieval, and serialized responses have deterministic limits and observable exhaustion.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 4). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases, selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-DELTA: Cold build and incremental rebuild of identical admitted inputs expose identical semantic results and valid commitments.'
    - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of independent profiles (lang-python, lang-rust, etc.), while dependent profiles like lang-vue explicitly declare their lang-typescript dependency.'
    - 'INV-PERF-NO-REGRESSION: Indexing and query throughput must not regress. Cold index build on a clean, isolated scratch copy of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) with scratch adapter pointing to clean isolated copies of linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7) must complete within 11.5 seconds wall-clock time using pre-compiled binary target/release/contextunity-forge-mcp build under a lightly loaded host (CPU load < 4). Median tool query latencies and RSS peak across benchmark scenarios must not regress against task 1 baseline.'
    - 'INV-CONTROL-REPO-ISOLATION: Performance evaluations, benchmarks, and multi-workspace verifications must strictly operate on clean, isolated scratch/temporary copies of the control repository (commerce-release-update at commit cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd) and clean copies of all linked workspaces (traverse at commit 39a1b8065b3d2356727e06fee47c1b5c8c7f04e3, grid-view-spec at commit d5f8f43e63c012c78af680af3391d04ea4855be7). The scratch adapter forge-mcp.yaml in the isolated commerce worktree must be rewritten to point linked_workspaces to those isolated clean scratch directories instead of primary repository trees. Never read from or modify primary baseline worktrees.'
    - 'INV-DOCS: Keep milestone contracts outside doc_search; publish delivered semantic contracts in indexed ADR, architecture, and reference documents.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Exercise the combined seam: first index → exact and uncertain lookup → edit/delete an export → incremental refresh → same semantic result as cold rebuild → impact/removal response (candidate-aware if ADR 0008 approved; conservative fail-closed gap reporting if ADR 0008 declined).
2. Check default, all-languages, and targeted single-feature builds (`cargo test --no-default-features --features lang-python`, etc.). Confirm unsupported languages appear as capability limits rather than successful empty analysis.
3. Record ground-truth counts before/after on the characterization corpus (distinguishing exact versus candidate precision and recall if ADR 0008 approved; or exact resolution versus explicit analysis gaps if declined). Do not count heuristic/candidate edges as exact improvements.
4. Measure cold build and query throughput against a clean, isolated scratch copy of the control repository (`commerce-release-update` at commit `cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd`) with scratch adapter pointing to clean isolated copies of linked workspaces (`traverse` at commit `39a1b8065b3d2356727e06fee47c1b5c8c7f04e3`, `grid-view-spec` at commit `d5f8f43e63c012c78af680af3391d04ea4855be7`):
   - Measure cold-build elapsed, user, and system time with Bash's built-in timer: `TIMEFORMAT='elapsed=%3R user=%3U system=%3S'; time target/release/contextunity-forge-mcp build <isolated-copy>`. Record pre-run `/proc/loadavg`; the host load average must be <4 and the cold build must complete in $\le 11.5$s wall-clock time. Compare elapsed time with task 1's isolated baseline and retain the timer output as diagnostic evidence.
   - Run tool comparison benchmark via `python benchmarks/run_benchmarks.py --profile benchmarks/profiles/commerce-release-update.json --mode comparison --workspace <isolated-copy>`; median tool latencies (cold and warm process wall time) and median RSS peak across profile scenarios must not regress against the baseline recorded in task 1. Measurements must strictly use isolated workspace copies with rewritten scratch adapter paths, leaving primary baseline checkouts untouched.
5. If the measured cold build exceeds 11.5 seconds, profile and remediate the hot path within `src/core/commitments.rs` and `src/db/ingest.rs`. Preserve the versioned logical FTS commitment, tamper/query-only validation, stable document identity, and bit-identical cold/delta roots; add production-seam regression coverage in `tests/commitment_integrity.rs` or `tests/incremental/`, then repeat the isolated cold-build measurement and comparison benchmark.
6. Reconcile MCP schemas and reference documentation. Skill distribution to home directories remains deferred to Milestone 044.
7. Validate frontmatter, relative links, backticked symbol linkage, and invariant callouts. Verify updated indexed architecture/reference sections are retrievable.

Verification: relevant unfiltered domain targets, strict Clippy, and final handoff gate:
```bash
cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
```

---

## Verification and completion

Follow [TESTS.md](../../TESTS.md). Tasks declaring `seam-test-first` record a real failing contract seam at contract gate and passing build; tasks declaring `direct-proof` validate existing structural or compilation invariants directly with exit code 0; task declaring `deferred-final-test` validates full end-to-end integration and non-regression benchmarks. Verify that a filtered test executed at least one test. Run `cargo clippy --all-targets --all-features -- -D warnings` for Rust builds. Reviewer evidence uses the candidate snapshot and actual commands/results; do not manufacture receipts or reuse results from another tree.

The final handoff command records this verification string only after it has actually run:

```bash
cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
```

Count passes from the final all-targets run according to TESTS.md. Code-map regeneration never resets tasks.sqlite. Execute gates, receipt generation, and authorized task/worktree operations through Forge.
