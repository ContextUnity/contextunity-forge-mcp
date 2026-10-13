---
title: "ADR 0008: Bounded Static Value Flow and Receiver Resolution"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0008: Bounded Static Value Flow and Receiver Resolution

## Status
Accepted

## Context
Accurate method call linking requires receiver type inference (e.g. knowing that `user.save()` calls `User.save`). Dynamic evaluation (e.g. running code in an interpreter) introduces arbitrary execution risks and severe latency overheads. Unbounded static analysis risks infinite loops on cyclic alias chains or path explosions.

## Decision
1. **Purely Static Data-Flow Analysis**:
   - Infer receiver types through lexical AST facts: assignment positions, explicit type annotations, constructor invocations, and straight-line factory returns.
   - Prohibit runtime evaluation or process sandboxing.
2. **Deterministic Termination & Bounded Traversal**:
   - Alias resolution and return-summary propagation use the reducer's existing bounded limits; no separate four-hop factory-return cap is asserted here.
   - Variable reassignments outside an admitted complete local join and cyclic aliases terminate type inference immediately, falling back to unresolved status or opaque dispatch. Conditional branches remain fail-closed unless they meet the amendment below.
3. **No Interprocedural Side-Effect Speculation**:
   - Receiver inference operates within proven lexical scope boundaries. Global or mutated variables without deterministic local initialization remain unresolved.

## Consequences
- Deterministic linker execution through bounded, fail-closed traversal.
- Zero security risk from executing arbitrary codebase scripts.
- Confident call graph edges with explicit provenance for resolved members.

## Verified baseline at milestone 046 admission

The archived [milestone 045](../milestones/archive/045-source-modularity-and-god-file-decomposition.md) requires `INV-ZERO-SEMANTIC-REGRESSION`: existing language extraction, static value flow, overload ambiguity, and selector disambiguation keep their current results. At this amendment, `TypeTarget` has local, callable, external, Rust dereference, builtin, object, ambiguous, and unknown variants; it has no finite candidate-set variant. The current merge keeps identical known identities, lets `Unknown` dominate, and widens other unequal pairs to `Ambiguous`. Alias resolution follows its existing alias-depth bound and fails closed on cycles or exhaustion.

The public `Coverage` record carries path, line, expression, status, and evidence. A definite method call is represented by a `calls` edge; ambiguous or unresolved evidence does not create that edge. Current receiver fixtures establish these boundaries:

- `tests/receivers/python_value_flow.rs::guarded_python_bindings_resolve_only_inside_their_branch` resolves the guarded branch-local call, leaves the call on an unknown alternative unresolved, and leaves the post-join call unresolved.
- `tests/receivers/python_value_flow.rs::later_unconditional_python_method_overrides_conditional_providers` builds and reads the SQLite index: conditional duplicate methods remain `ambiguous`; a later unconditional definition is `resolved` and its persisted call targets that definition; a later non-callable field assignment remains `unresolved`.
- `tests/linker/receivers/typed.rs::ambiguous_receiver_type_never_selects_one_imported_class` preserves `ambiguous` coverage and emits no call edge for two imported declarations with the same name.
- `tests/receivers/rust_value_flow.rs::conditional_and_reassigned_rust_bindings_do_not_keep_an_unconditional_proof` records conditional writes and unknown reassignments in extracted facts rather than treating them as unconditional proofs.

This baseline is the direct-proof starting point for the approved candidate addition. The implementation tasks must preserve these exact and negative cases outside admitted complete joins.

## Owner-approved amendment: bounded local branch joins (2026-10-13)

The owner approved retaining finite candidate evidence for complete local branch joins: **allow at most `K = 8` distinct targets; overflow widens to `Unknown/Incomplete`.** This amendment replaces only the conditional-branch clause in Decision 2. Reassignments outside an admitted complete join, cyclic aliases, dynamic values, and unproven side effects remain fail-closed. Tasks 4, 5, and 6 of milestone 046 may implement candidate inference and navigation only under this decision.

### Join and fallback contract

- Keep the existing `MAX_VALUE_FLOW_TYPE_DEPTH = 8` guard on recursive language type-expression AST parsing and `SemanticLimits::alias_depth` (default 64) as separate bounds. Return-summary propagation follows the reducer's existing bounded iteration limits; this contract does not assert a separate four-hop factory-return cap. Define `MAX_CANDIDATE_BRANCH_JOINS = 8` as a separate cardinality bound over distinct declaration identities; candidate count must not change any traversal bound.
- Admit a join only when the language resolver can enumerate every local exit path. Include an implicit fall-through or incoming value when the syntax has one. Deduplicate targets by stable declaration identity and order them deterministically.
- If every path proves the same target, retain the existing exact result. If paths prove two through eight distinct targets, retain them as possible targets. An opaque or unknown alternative remains explicit and makes the result incomplete; it cannot be dropped to manufacture an exact or exhaustive result.
- If a join would exceed eight distinct targets, widen to `Unknown/Incomplete` and report exhaustion. Do not expose a truncated prefix as an exhaustive set. Unknown writes, alias cycles, unsupported control flow, and opaque expansion keep their existing fail-closed precedence.
- Preserve source-order assignment effects. A later proven unconditional write replaces an earlier join; an unknown write does not preserve a stale exact proof. Explicit return annotations continue to take precedence over bounded return summaries.

Possible targets are navigation evidence, not call edges. Store full candidate facts in the compressed value-flow fact store, outside `nodes.details` and FTS text. Expose bounded target references, provenance, completeness, and exhaustion through a separately labeled possible-target result on the shared CLI/MCP query surface. Do not encode a multi-target result as an exact `calls` edge or reinterpret existing exact `resolved`, `external`, `ambiguous`, and `unresolved` results. Keep existing response fields and exact results compatible; add candidate fields and relations additively, retain response and pagination limits, and use existing fact/index version checks and rebuild admission for intentional encoding changes.

Candidate inference must follow the source language profile and Cargo feature already enabled for indexing. It does not authorize activating another language grammar or adding a cross-language dependency to an independent profile. The milestone's language-feature-decoupling task owns the cfg-gate implementation and isolated compilation matrix; candidate support must preserve that matrix.

### Branch-join contract matrix v1

A language profile may emit possible targets only for syntax forms its resolver explicitly enumerates. The first implementation admits one complete local branch family; unsupported forms in every row remain unknown or incomplete until that profile is admitted.

| Source domain | Exact result | Possible targets | Unknown or incomplete | Forbidden false positive |
| --- | --- | --- | --- | --- |
| Python | Existing annotations, constructors, and aliases continue to resolve when one declaration is proven on every path. | Complete `if`/`elif`/`else` joins with known local values may retain up to eight distinct declarations. An omitted `else` includes the incoming value. | Unknown factories, unsupported statements, reassignment outside the admitted join, alias cycles, or overflow. | Selecting the first or last method definition, dropping an unassigned path, or resolving from a class name alone. |
| Rust | Existing nominal annotations and constructors continue to resolve when one declaration is proven. | An admitted complete `if`/`else` or finite `match` may retain up to eight known nominal declarations. | Macro expansion, `cfg`-dependent paths, trait objects, unsupported dereference, unknown alternatives, or overflow. | Treating one visible trait implementation as the unique runtime target when other implementations or dispatch paths remain possible. |
| TypeScript and JavaScript | Existing annotations, constructors, and aliases continue to resolve when one declaration is proven. | An admitted complete conditional join with known local declarations may retain up to eight targets. | Dynamic writes, opaque expressions, unresolved `any` or external values, unsupported control flow, or overflow. | Selecting a declaration by member spelling alone or dropping a conditional alternative. |
| Mixed framework composition | Existing explicit composition relations continue to resolve under their owning language and framework rules. | A possible target may cross a language boundary only when an explicit composition pass supplies the source, owner, and bounded target evidence. | Runtime transforms, generated code, dynamic template names, opaque handlers, or missing composition evidence. | Inferring a cross-language call from a shared name or treating a framework registration as proof of a unique runtime route. |

## Isolated task 1 baseline (2026-10-13)

The corrected comparison used a clean scratch copy of `commerce-release-update` at `cbe26dbfcda03f9f42ef92b0cb52680e4727c1bd`; its linked scratch workspaces were `traverse` at `39a1b8065b3d2356727e06fee47c1b5c8c7f04e3` and `grid-view-spec` at `d5f8f43e63c012c78af680af3391d04ea4855be7`. The scratch adapter pointed only to those scratch workspace copies. Raw `/proc/loadavg` immediately before the run was `0.95` at `2026-10-13T06:21:57.965812681Z`. The Forge executable was the task worktree release binary at `/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/046-contract-and-quality-oracle/target/release/contextunity-forge-mcp`, SHA-256 `4d6f52fcdf30a75df55582a96e536e6c904c745a25f8301869189c5f3d0ba8e2`.

Command:

```sh
python3 benchmarks/run_benchmarks.py \
  --profile /tmp/contextunity-forge-m046-baseline/commerce-release-update-verified.json \
  --mode comparison \
  --workspace /tmp/contextunity-forge-m046-baseline/commerce-release-update \
  --output-dir /tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013
```

The command exited 0 and produced 70 metric rows over 19 scenarios, with five repeats per backend/process-state row (cold or warm). The validated receipt is `/tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013/comparison.json`. Cold index-build measurements were:

| Indexer | Wall time | CPU time | Peak RSS | Startup |
| --- | ---: | ---: | ---: | ---: |
| Forge | 10,995.7 ms | 48,665.9 ms | 1,378,116 KiB | included |
| Codebase Memory | 25,738.3 ms | 112,234.8 ms | 4,110,080 KiB | 4,203.7 ms before the index request |

Across all raw request samples by backend and process state (85 Forge samples and 90 Codebase Memory samples per state), median `tools/call` wall and CPU time, plus the maximum request RSS across both states, were:

| Backend | Cold-process median wall / CPU | Warm-process median wall / CPU | Maximum request RSS |
| --- | ---: | ---: | ---: |
| Forge | 100.792 / 147.559 ms | 3.120 / 3.024 ms | 64,496 KiB |
| Codebase Memory | 77.762 / 41.511 ms | 86.205 / 39.824 ms | 1,113,128 KiB |

The measured Forge cold build is 10.9957 seconds, within the 11.5-second ceiling. Subsequent milestone work must avoid regression against this baseline. Request latency is measured after MCP initialization; process startup is reported separately. The comparison uses isolated indexes and does not flush the operating-system page cache.

The separate standalone cold-build check used the same precompiled Forge binary and isolated Commerce copy. At `2026-10-13T09:02:54Z`, pre-run CPU load average was `1.86`, below the owner-corrected `<4` threshold. Python's standard-library `subprocess`, monotonic clock, and `resource.getrusage(RUSAGE_CHILDREN)` measured 10.590 seconds wall, 48.635 seconds child CPU, and 1,358,348 KiB maximum resident set size; the process exited 0. The receipt is `/tmp/contextunity-forge-m046-baseline/results/task1-baseline-verified-20261013/standalone-build-measurement.json`.
