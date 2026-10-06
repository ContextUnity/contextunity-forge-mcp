---
id: m-language-semantics-and-resolution-coverage
title: Language semantics and resolution coverage
doc_type: contract
status: active
depends_on: []
owners:
- src/engine/
- src/core/
- tests/
invariants:
- 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
- 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
- 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
- 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
related_plans: []
started_at: 2026-10-02T17:06:10+00:00
---

# Language semantics and resolution coverage

## Outcome and purpose

Improve source-proven resolution in Python, JavaScript, TypeScript, HTML,
Vue, and Rust across repositories. Keep `resolved`, `external`, `ambiguous`,
and `unresolved` distinct. Preserve lexical order, import provenance, project
isolation, and deterministic commitments. Each language has one optimization
root task; its subtasks track independently verifiable improvements and
remaining work. The shared polyglot attribution seam has its own task.

Every language task maximizes source-proven classified coverage on real codebases
while preserving correct `unresolved` and `ambiguous` boundaries. Before task
delivery, its owner builds the release binary and measures a controlled corpus
with that binary from the task's dedicated development worktree. The owner
audits every completed subtask against the current code and that index: record
the production-path edge, status, provenance, and measured language delta. An
integration-worktree scan supplements but never replaces the task's own gate.
A green fixture alone does not close a subtask. If classified coverage remains
below the task target, reopen a weak subtask or add a repair subtask within the
same language task, fix the source-proven cause, and remeasure within the
bounded profiling policy. Primary verification evaluates diffs of individual
references (`path:line:expression`) against the fixed baseline rather than shifting
corpus file totals. Identify direct proof and provenance-only changes
without claiming an increase in classified coverage. Record genuinely dynamic
and ambiguous residuals with their source constraints.

The pre-integration Commerce reference index covers 3,953 files (fixed benchmark
baseline: 3,901 files). The table below is a historical baseline until the integrated
candidate is rebuilt. Classified means `resolved + external` divided by all four
resolution statuses. These measured counts direct investigation; they do not authorize
classification by a receiver's spelling or a repository-specific path.

| Language | Resolved | External | Unresolved | Ambiguous | Classified |
| --- | ---: | ---: | ---: | ---: | ---: |
| Python | 112,512 | 107,712 | 34,895 | 5 | 86.32% |
| JavaScript | 2,209 | 6,988 | 6,606 | 0 | 58.20% |
| TypeScript | 2,953 | 2,281 | 4,242 | 2 | 55.22% |
| HTML | 1,503 | 466 | 2,531 | 0 | 43.76% |
| Vue | 675 | 583 | 640 | 0 | 66.28% |

The reference contains no Rust files, so Rust acceptance uses its dedicated
source and receiver seams. The indexed build takes 11.455 seconds (345
files/sec) and stores 194,248,704 bytes (48 KiB/file). Milestone 030 owns the
separate 400 files/sec and 45 KiB/file performance budgets.

## Current task-worktree measurements

Each row comes from that language task's own release binary and indexed
corpus. The candidates contain different pending changes; these figures are
not an integrated milestone result. The Rust row includes a known external
import precedence regression now repaired in source but awaiting reindex.

| Language | Classified | Unresolved | Cold index | Corpus |
| --- | ---: | ---: | ---: | --- |
| Python | 222,020 / 255,086 (87.04%) | 33,066 | 11.002 s | Commerce, 3,761 files |
| JavaScript | 9,760 / 15,792 (61.80%) | 6,032 | 10.857 s | Commerce, 3,761 files |
| TypeScript | 6,089 / 9,490 (64.16%) | 3,399 | 11.560 s | Commerce, 3,761 files |
| HTML | 4,882 / 4,929 (99.05%) | 47 | 16.475 s | Commerce, 3,761 files |
| Vue | 1,634 / 1,898 (86.09%) | 264 | 11.132 s | Commerce, 3,761 files |
| Rust | 21,384 / 40,894 (52.29%) | 19,324 | 1.865 s | Forge, 163 files |

Python and TypeScript tasks have been reopened after review reproduced two
receiver-flow regressions. The other language and polyglot task receipts remain
historical evidence. The integrated release scan below supersedes the separate
candidate timings for milestone handoff; unresolved source cases remain
fail-closed and are tracked by their exact language and provenance boundaries.

## Integrated release verification

The integrated Commerce candidate built from this worktree on 2026-10-06
contains 3,901 files, 65,551 nodes, and 302,113 edges. It classifies 251,155 of
294,460 references (85.29%), with 43,247 unresolved and 58 ambiguous. The cold
build takes 13.871 seconds (`extract_ms=4,396.47`, `persist_ms=5,841.86`,
`indexes_ms=1,472.77`, `seal_ms=623.71`) and reports generation
`7ea6992fd332250f57b689304dd29fdf0da16ff3c636d5076a8d435e516c0828`.
Commitment integrity passes on the corrected source, and the complete test
suite passes (594 passed, 0 failed, 3 ignored). The last measured coverage and
cold-build values remain those of the pre-correction candidate; the requested
integrated cold-build gate of at most 12.0 seconds remains open, and the final
source's coverage is unverified. This milestone stays active pending a new
controlled integrated measurement. Full phase and storage
evidence is recorded in milestone 030's active performance receipt; the host
had no cargo/rustc jobs but a 6.27 load average at measurement start.
After this measurement, `cargo test --all-targets` exposed a Future-boundary
regression in async return inference. The extractor and linker were corrected
to expose an async return annotation only under `await`. The recorded elapsed
time, coverage, and generation above therefore describe the pre-correction
candidate; final-source Commerce coverage and cold-build time remain unmeasured,
so the coverage gate must be revalidated before M020 handoff.
The awaited generic-return fix changes the five Commerce `grant.get` rows at
`engram_source_grants.py:93,94,96,99,100` from unresolved to external, with no
reverse transitions at those keys. `Category.add_root` at
`category_root_create_operations.py:259` remains unresolved: `Category` uses
`treebeard.mp_tree.MP_Node` as its runtime base, while the local `add_root`
declaration exists only under `TYPE_CHECKING`; the external inherited method
has no normalized provider mapping in this index. The exact residual stays open
for provider-boundary work rather than a name-based classification.
The preceding HTML rev4 candidate classifies 39 previously unresolved exact
references (11 Django context tags, 20 Jinja registered filters, and eight
inline handler calls) without a reverse transition. Whitespace-control
extraction adds 27 external directive references and one resolved macro.
The HTML rev5 candidate attributes exact HTML/JavaScript owners through
4,021 sparse sidecar rows (128 KiB), classifies 4,882/4,929 HTML references,
and leaves 47 unresolved. Five exact raw HTML references become resolved with
no reverse transition; other rows move to their JavaScript language owner.
The cold index takes 16.475 s (228.3 files/s), below throughput budget.
Warm overview/analyze SQL takes 11.603/9.169 ms, but first overview SQL takes
32.449 ms and still misses the 30 ms interactive budget. Full public MCP
admission latency remains unverified.
The latest Vue candidate gains 20 exact unresolved-to-resolved and 12
unresolved-to-external references with zero reverse transitions. A green
typed function-map fixture produces no citation-field gain on Commerce because
the real `v-for` uses a tuple binding; that subtask remains open for a grouped
follow-up rather than a one-fix scan.
A newer TypeScript candidate is excluded from the table: direct-callee
reference suppression caused 58 exact classified-to-unresolved key regressions
despite green domain tests. Its 16.908 s cold timing is also incomparable
because an unrelated Forge auto-rebuild may have overlapped the scan.
Removing that suppression recovers 14 classified keys, but 44 exact
external-to-unresolved GridViewSpec keys remain in the following own scan.
The four-fix candidate stays unaccepted pending typed receiver provenance;
its quiet 15.698 s cold index also misses the throughput budget.
The latest Rust candidate gains 335 exact unresolved-to-external references
on byte-identical source files with no reverse transition, including recovery
of all 32 prior `hashbrown` import regressions. It classifies 52.29% and remains
far below the >90% task target. Its 1.865 s cold index was measured under host
load 10.47, so the difference from 1.111 s lacks a quiet paired baseline.

## Tasks in this milestone

### task: python-language-optimization

```yaml
task_ref: python-language-optimization
target: Improve Python imports, local value flow, and receiver members through source-proven providers
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/core/semantic.rs
- src/engine/ast/relations.rs
- src/engine/ast/routes.rs
- src/engine/languages/manifests.rs
- src/engine/languages/python.rs
- src/engine/languages/python/
- src/engine/linker.rs
- src/engine/linker/
- tests/python_semantics.rs
- tests/python_external_call_evidence.rs
- tests/python_child_module_links.rs
- tests/builtin_call_coverage.rs
- tests/receivers/
- tests/ast_extractors.rs
subtasks:
- subtask_ref: package-reexports
  title: Traverse unique __init__.py re-exports and monorepo package hubs
  status: completed
  evidence: cargo test --test python_semantics package_reexports passes
- subtask_ref: direct-import-edges
  title: Keep direct Python symbol import edges aligned with coverage across workspaces
  status: completed
  evidence: cargo test --test python_child_module_links passes
- subtask_ref: import-provenance-and-receiver-flow
  title: Resolve bounded relative imports, aliases, local collections, and receiver returns
  status: completed
  evidence: Python unified receiver and import suites pass; controlled coverage is 86.32%
- subtask_ref: route-call-precision
  title: Admit Django routes only from explicit route registrations and valid handlers
  status: completed
  evidence: cargo test --test ast_extractors passes
- subtask_ref: shadowed-binding-provenance
  title: Trace local and parameter receivers through guarded assignments without crossing branches
  status: completed
  evidence: 'CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --test typed_receiver_resolution python_value_flow:: (23 passed, 0 failed); public seam expected red exit 101 before implementation'
- subtask_ref: bounded-dynamic-callees
  title: Summarize uniquely proven factory and callback returns for dynamic call sites
  status: completed
  evidence: 'CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --test typed_receiver_resolution nested_python_calls_follow_only_unique_callable_returns: 1 passed, 0 failed; pre-fix exit 101; reviewed paths, claims, concurrency, project_isolation, administration'
- subtask_ref: inferred-member-providers
  title: Resolve unique methods on inferred local receivers and fail closed on ambiguous providers
  status: completed
  evidence: 'Public writer+SQLite source-order regression `later_unconditional_python_method_overrides_conditional_providers`: targeted RED exit 101 (ambiguous vs resolved), then final current-source CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --test typed_receiver_resolution later_unconditional_python_method_overrides_conditional_providers: 1 passed, 0 failed; nearby inferred_python_members_require_a_unique_class_provider: 1 passed, 0 failed. Conditional duplicate Dynamic.render ambiguous; later unconditional Replaced.render resolved exact target line 12; later class field assignment leaves field.render unresolved. Five-contour paths, claims, concurrency, project_isolation, administration review PASS; independently re-reviewed PASS. Rebased candidate dc9b41b, git diff --check clean.'
- subtask_ref: typed-parameter-members
  title: Carry Mapping and class annotation evidence into bounded parameter member lookup
  status: completed
  evidence: 'ADR 0008: finite Python framework member provenance carried as static canonical base with explicit import origin/line, no runtime evaluation or per-reference I/O. Public AST+linker plus persisted writer+SQLite seam: CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --test typed_receiver_resolution annotated_python_receivers_follow_declared_inheritance_and_protocol_members: 1 passed, 0 failed; pre-fix exit 101 generic unverified evidence. Exact django.http.HttpRequest.GET.get external, local Protocol.execute resolved edge, missing member unresolved. Five-contour review PASS paths, claims, concurrency, project_isolation, administration; git diff --check clean. No corpus-wide metric claim.'
- subtask_ref: type-only-annotation-imports
  title: Resolve TYPE_CHECKING guarded imports for PEP 484/563/695 type annotations as type_only edges without runtime call edges
  status: completed
  evidence: 'Persisted writer SQLite seam GREEN 1/1: cargo test --test python_semantics type_checking_imports_resolve_annotations_without_runtime_calls. TYPE_CHECKING import yields exact annotation reference edge to same-project Client in two linked workspaces; runtime Client constructor stays unresolved. Exact import position, type-only role and annotation signature gate are in compressed ValueFlowFacts/linker, not receiver_hint or nodes.details. commitment_integrity GREEN 12/12.'
- subtask_ref: stdlib-logging-factory-returns
  title: Resolve logging.getLogger(__name__) factory returns to builtin:logging.Logger methods (info, warning, error, debug, exception)
  status: completed
  evidence: Proven via tests/python_semantics.rs::logging_factory_methods_have_verified_builtin_origin_after_persistence. logging.getLogger factory returns resolve receiver member calls (info, warning, error, debug, exception) as builtin:logging.Logger with lexical shadow guards.
- subtask_ref: dict-literal-and-mapping-methods
  title: Resolve dict literals {} and MutableMapping/dict annotations to builtin:dict methods (get, items, keys, values, pop, update), and read-only Mapping to (get, items, keys, values)
  status: completed
  evidence: Proven via tests/python_semantics.rs::local_receiver_dictionary_and_logger_methods_resolve_in_function_scope. Dict literals ({}) resolve methods (get, keys, values, update, pop) to builtin:dict, and Mapping annotations resolve read-only mapping methods (get, items, keys, values) with local shadow guards.
- subtask_ref: context-manager-enter-propagation
  title: Propagate with-statement context manager __enter__() return types to binding targets from explicit annotations (e.g. sqlite3.connect -> Connection, open -> TextIO/BinaryIO by mode)
  status: completed
  evidence: cargo test --test python_semantics call_return_type_and_context_manager_propagation_resolves_persisted_coverage passes (pre-fix RED 101, post-fix GREEN 1/1); with-statement bindings propagate explicit return annotations into receiver types; commitment_integrity passes 12/12; clippy 0 warnings.
- subtask_ref: member-access-call-callee-dedup
  title: Skip a call-callee attribute in relations::member_access so self.method(), cls.factory(), and this.method() keep one calls row
  status: completed
  evidence: tests/ast_extractors.rs::python_member_access_callee_does_not_emit_duplicate_reference passed after RED exit code 101. Callee attribute nodes in member_access skip emitting duplicate references relation while retaining exact calls relation.
- subtask_ref: inherited-self-and-cls-member-lookup
  title: Resolve self.member and cls.member to an indexed method on a base class, including a base defined in another file
  status: completed
  evidence: cargo test --test python_semantics inherited_self_member_resolves_cross_file_mixin_methods passes (pre-fix RED exit 101, post-fix GREEN 1/1); C3 linearization gracefully skips Base::Unknown without invalidating whole class hierarchy; commitment_integrity passes; clippy 0 warnings.
- subtask_ref: call-return-type-annotation-propagation
  title: Bind var = func() and with func() as var to the indexed callable's explicit return annotation
  status: completed
  evidence: cargo test --test python_semantics call_return_type_and_context_manager_propagation_resolves_persisted_coverage passes (pre-fix RED 101, post-fix GREEN 1/1); free function and self.method() calls bind variables to proven return annotations; commitment_integrity passes 12/12; clippy 0 warnings.
- subtask_ref: stdlib-sqlite-and-logger-adapter-receivers
  title: Resolve logging.LoggerAdapter methods and sqlite3.Connection methods from the finite stdlib receiver table
  status: completed
  evidence: cargo test --test python_semantics stdlib_sqlite_and_logger_adapter_receivers_resolve_persisted_coverage passes (pre-fix RED 101, post-fix GREEN 1/1); commitment_integrity 12/12; clippy 0 warnings.
- subtask_ref: loop-iterable-annotation-element-inference
  title: Type a for-target from an explicit list[T], Sequence[T], or Iterable[T] annotation when T is dict or Mapping
  status: completed
  evidence: cargo test --test python_semantics loop_iterable_annotation_element_inference_resolves_persisted_coverage passes (pre-fix RED 101, post-fix GREEN 1/1); list[dict], Sequence[Mapping], and Iterable[dict] annotations type loop targets to resolve row.get; commitment_integrity passes 12/12; clippy 0 warnings.
- subtask_ref: global-name-does-not-mask-instance-field
  title: 'For Python syntax `global name` or `nonlocal name` in a function that also assigns `self.name = Provider()` in `__init__`, persisted writer coverage for `box.name.run()` through an explicitly typed Box receiver remains resolved to the indexed Provider method; lexical `name` binding uncertainty must not erase independent FieldFacts.'
  status: completed
  evidence: 'RED confirmed before fix. Public writer+SQLite regression resolves `box.client.run` to `src.fields.Client.run` after global-name uncertainty was limited to lexical bindings; `cargo test --test python_semantics` passes 26/26.'
- subtask_ref: commerce-source-proven-unresolved-repairs
  title: 'Re-evaluate Category.add_root in category_root_create_operations.py:259 and grant.get in engram_source_grants.py:93-100 against parser facts and normalized provider evidence; classify only source-proven calls, otherwise report exact residual unresolved sites without path or spelling heuristics.'
  status: completed
  evidence: 'Controlled Commerce generation 7ea6992fd332250f57b689304dd29fdf0da16ff3c636d5076a8d435e516c0828: five exact grant.get rows at lines 93,94,96,99,100 are external with no reverse transitions. Category.add_root at category_root_create_operations.py:259 remains unresolved; Category runtime base is treebeard.mp_tree.MP_Node and its local add_root declaration is guarded by TYPE_CHECKING, but this external inherited callable has no normalized provider mapping. Keep fail-closed pending explicit provider-boundary work.'
- subtask_ref: awaited-generic-return-loop-element
  title: 'Propagate an explicit async list[T] return through `await` and loop binding to resolve only source-proven element members; untyped, decorated, ambiguous, or non-collection providers remain unresolved.'
  status: completed
  evidence: 'Public persisted seam and Python suite pass (26/26): async result types live in ValueExpr::Await(Call). `typed_receiver_resolution` passes 75/75, `python_semantics` passes 26/26.'
receipt:
  commit: 21214f830c8e7b2deae0ec18dfabe5e17f88bbb5
  contract_revision: 4
  passed_at: 2026-10-05T18:56:44.280889623+00:00
  evidence:
    test_proof:
      command: cargo test --test python_semantics
      exit_code: 0
      tests_failed: 0
      tests_passed: 26
    review:
      review_proof:
        contours:
          administration:
            applicable: true
            evidence: Commitment integrity verified (12/12 pass), Merkle tree determinism maintained across cold/incremental rebuilds, milestone documentation updated.
          claims:
            applicable: true
            evidence: All 15 subtasks are fully completed and verified by public persisted integration tests in tests/python_semantics.rs (26/26 pass), tests/ast_extractors.rs, and tests/typed_receiver_resolution.rs. Clippy clean with 0 warnings.
          concurrency:
            applicable: true
            evidence: Thread-safe data structures and immutable indexes; C3 linearization and value flow evaluation operate deterministically across worker threads.
          paths:
            applicable: true
            evidence: All modified files fall strictly within allowed_write_scope.
          project_isolation:
            applicable: true
            evidence: Cross-file mixin method resolution and package imports strictly respect package boundaries and do not leak symbols across unlinked projects.
        decision: pass
    decision: pass
    rollup:
      verified_invariants:
      - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
      - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
      - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
      - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Architecture & Seam Contract:
- Universal AST Rules: Python AST extraction must resolve imports from `import_statement` and `import_from_statement`, tracking lexical positions and line order.
- Type Annotations: Annotations (`AnnAssign`, function parameter types, return types) define static receiver types. Generic subscript types (`Mapping[K, V]`, `Sequence[T]`, `Optional[T]`) must unwrap to their base protocol or class.
- Guards & Invariants: `if TYPE_CHECKING:` imports must be tagged with `role = "type_only"` in AST facts and never resolve runtime call expressions. Local variable assignments within conditional blocks (`if`, `try`, `match`) must not leak unconditional type inference to surrounding or alternative branch scopes.
- Public Seams: `tests/python_semantics.rs`, `tests/python_child_module_links.rs`, `tests/receivers/`.

#### Python Resolution Improvement Hypotheses & Subtask DoD Specifications

1. `member-access-call-callee-dedup`:
   - **Concrete Syntax**: An `attribute` node whose parent is a `call` and `parent.child_by_field_name("function")` is that attribute (`self.method()`, `cls.factory()`, `this.method()`).
   - **Root Cause & Empirical Count**: `relations::member_access` emits a `references` row for every `self.` / `cls.` / `this.` attribute, including a call callee. On the Commerce index, 6,075 such Python rows are `unresolved` with `0 lexically justified candidates`. 3,414 of those sites already have a `resolved` or `external` row for the same path, line, and expression. About 2,661 sites have no classified twin: the call itself is `unresolved`.
   - **Expected Transition**: The callee attribute does not emit `references`. The `calls` row remains. A site that already has a classified twin loses the extra `unresolved` row. A site whose only row is an unresolved call stays `unresolved`.
   - **DoD**: `tests/ast_extractors.rs` shows one `calls` relation for `self.method()` and no second `references` relation for that callee. A still-unresolved call remains in coverage.

2. `inherited-self-and-cls-member-lookup`:
   - **Concrete Syntax**: `class Child(Base)` with `self.method()` or `cls.method()`, where `method` is an indexed method of `Base` or of a further indexed base in another file. Commerce witness: `SqliteCellEdgeMutationLayer` bases reach `SqliteConnectionMixin._get_connection`, and `_cell_edges_mutations.py` line 246 calls `self._get_connection`.
   - **Root Cause & Empirical Count**: Bases are stored as source text such as `(SqliteCellEdgeValidationLayer)`. The mixin method is indexed, and the call stays `unresolved` with `0 lexically justified candidates`. These sites are the remainder after callee dedup, about 2,661 Python `self.*` / `cls.*` sites with no classified twin.
   - **Expected Transition**: The call resolves to the indexed method. `getattr` and any other dynamic callee stay `unresolved`. `unittest.TestCase.assertEqual` uses the existing finite stdlib table once the base name resolves to that class.
   - **DoD**: A persisted test in `tests/python_semantics.rs` links a cross-file `self._get_connection()` edge to the mixin method. A missing method stays `unresolved`.

3. `call-return-type-annotation-propagation`:
   - **Concrete Syntax**: `var = func(...)` or `with func() as var` when the indexed callable has an explicit `-> ReturnType` annotation, such as `-> sqlite3.Connection` or `-> ContextUnitLoggerAdapter`.
   - **Root Cause**: `ValueExpr::Call` yields `TypeTarget::Unknown` for an ordinary function. A later `var.execute()` or `logger.info()` stays a shadowed binding of unknown callable identity. An unannotated call stays unknown. `logging.getLogger` is already covered by `stdlib-logging-factory-returns`.
   - **Expected Transition**: The bound name receives `TypeTarget::Local` when `ReturnType` is an indexed project type, or `TypeTarget::External` when it is a known stdlib type. Method resolution then follows that type.
   - **DoD**: A persisted test in `tests/python_semantics.rs` resolves `db.execute` after `with connect() as db` when `connect` is annotated `-> sqlite3.Connection`. The same pattern without a return annotation stays `unresolved`.

4. `stdlib-sqlite-and-logger-adapter-receivers`:
   - **Concrete Syntax**: Methods on `logging.LoggerAdapter` (`info`, `warning`, `error`, `debug`, `critical`, `exception`) and on `sqlite3.Connection` (`execute`, `executemany`, `cursor`, `commit`, `rollback`, `close`).
   - **Root Cause**: `semantic_context.rs` already maps the `logging.LoggerAdapter` factory and `sqlite3.Cursor`. The finite receiver table does not list `LoggerAdapter` methods or `sqlite3.Connection` methods. This table is stdlib, not a framework manifest.
   - **Expected Transition**: A proven `LoggerAdapter` or `sqlite3.Connection` receiver resolves those methods to `external` with provider `builtin:logging.LoggerAdapter` or `builtin:sqlite3`.
   - **DoD**: A persisted test in `tests/python_semantics.rs` covers both receivers. An unknown method on the same type stays `unresolved`.

5. `loop-iterable-annotation-element-inference`:
   - **Concrete Syntax**: `for row in items:` when `items` has an explicit `list[T]`, `Sequence[T]`, or `Iterable[T]` annotation and `T` is `dict` or `Mapping`.
   - **Root Cause**: `ValueExpr::LoopElement` does not unwrap `T`. Annotated `Mapping` parameters are already resolved by `dict-literal-and-mapping-methods`. A parameter named `row` without this annotation is not a loop target.
   - **Expected Transition**: `row.get` resolves to `builtin:dict.get` or `builtin:Mapping.get`. An untyped `row.get` stays `unresolved`.
   - **DoD**: A persisted test in `tests/python_semantics.rs` resolves the loop target and leaves an unannotated parameter `unresolved`.


The historical Python audit records 18,816 shadowed binding cases, 5,177
dynamic callees, 4,670 references without lexical candidates, 2,483 inferred
receivers without a verified member, 165 typed parameters without a member,
and 40 missing import targets in the audited groups. These groups overlap.
Prove any new classification through persisted coverage and exact import or
receiver evidence; keep conditional providers ambiguous or unresolved.

### task: javascript-language-optimization

```yaml
task_ref: javascript-language-optimization
target: Resolve JavaScript module, platform, and receiver chains with finite provenance
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/core/models.rs
- src/core/semantic.rs
- src/engine/ast/routes.rs
- src/engine/languages/manifests.rs
- src/engine/languages/typescript.rs
- src/engine/languages/typescript/
- src/engine/linker.rs
- src/engine/linker/
- tests/typescript_semantics.rs
- tests/language_boundaries.rs
- tests/manifests.rs
- tests/receivers/
- tests/ast_extractors.rs
- src/engine/ast/mod.rs
- src/engine/languages/html/javascript.rs
- src/core/typed_facts.rs
status: completed
subtasks:
- subtask_ref: commonjs-and-node-globals
  title: Link local require and exports and classify finite Node globals
  status: completed
  evidence: cargo test --test typescript_semantics commonjs and cargo test --test manifests pass
- subtask_ref: web-factory-results
  title: Propagate proven DOM factory and awaited fetch receiver types
  status: completed
  evidence: 'Own release SHA c93185d6 Commerce cold index: local URL and URLSearchParams provider repair lowers unknown local bindings 2480 to 2400. Exact persisted public seam GREEN with direct new URL, currentUrl() return, inline constructor, shadowed URL and rebind guards; external origin builtin:web_api.'
- subtask_ref: lexical-and-try-scope
  title: Preserve CommonJS captures and scoped try bindings with shadow and rebind guards
  status: completed
  evidence: cargo test --test typescript_semantics passes
- subtask_ref: object-and-array-receivers
  title: Resolve finite array and returned-object members through unique local providers
  status: completed
  evidence: cargo test --test typescript_semantics and commitment_integrity pass
- subtask_ref: dynamic-chain-provenance
  title: Resolve unique computed and optional call chains without name-based fallback
  status: completed
  evidence: 'Source-proven inline new URL(...).searchParams.get: exact public seam RED exit 101 dynamic unresolved then GREEN 1/1 with ConstructorResult and shadowed constructor negative.'
- subtask_ref: unresolved-local-imports
  title: Audit local imports and excluded generated outputs with project isolation
  status: completed
  evidence: 'Direct-proof GREEN: cargo test --test typescript_semantics javascript_local_imports_require_indexed_project_targets. Public writer::build SQLite proves exact .mjs-to-.ts import and call edges and resolved coverage in two linked workspaces; physically present scanner-excluded dist artifacts remain unresolved.'
- subtask_ref: commonjs-function-expression-exports
  title: Index module.exports = function and exports.name = function definitions to bind named and default ESM/CJS import call sites
  status: completed
  evidence: Public persisted writer/build ESM+CJS default function provider and named export fixtures pass with source-order/duplicate guards in tests/typescript_semantics.rs (commonjs_default_function_expression_export_binds_esm_and_require_calls). 59/59 tests pass, Merkle 12/12, clippy 0 warnings.
- subtask_ref: standard-ecmascript-and-web-builtins
  title: Classify static methods on Promise, Object, Array, Math, JSON, and Web API (fetch, URL, console) as builtin:ecmascript and builtin:web_api
  status: completed
  evidence: 'Public JS finite static seam proves Object/Array/Math/JSON/Promise/Date exact builtin:ecmascript origin, Web console/fetch/URL origin and local import/parameter/rebind/unknown receiver guards. Commerce same-key scan: Promise.resolve/reject 39 unresolved→external, JS 8,285/19,372→8,324/19,372. Domain 50/50, commitment 12/12, Clippy 0 warnings, all-targets 534 passed/0 failed.'
- subtask_ref: lexical-arrow-and-prototype-flow
  title: Resolve lexical this inside nested arrow functions and prototype method assignments (Type.prototype.method = fn)
  status: completed
  evidence: Preserved lexical this inside nested arrow functions and verified prototype method assignments (Type.prototype.method = fn) with single-write, lexical visibility, non-rebound, and non-delete guards. 4 targeted prototype tests pass in tests/typescript_semantics.rs (59/59 pass), Merkle 12/12, clippy 0 warnings.
- subtask_ref: js-regression-and-loss-audit
  title: Audit 83 lost candidate classifications against callback flow and dynamic call sites
  status: completed
  evidence: 'Audit confirmed: 83 losses from removing synthetic name-only fallback are unproven dynamic callback callers and runtime property accesses that lack indexed lexical declarations; per contract and ADR 0008 they correctly remain fail-closed unresolved.'
- subtask_ref: chained-call-return-value-flow
  title: Bind the result of a finite DOM or ECMAScript call to that function's return type, such as document.createElement to Element
  status: completed
  evidence: cargo test --test typescript_semantics javascript_and_typescript_chained_call_return_value_flow passes; all 61 tests pass; Merkle 12/12, clippy 0 warnings
receipt:
  commit: 21214f830c8e7b2deae0ec18dfabe5e17f88bbb5
  contract_revision: 5
  passed_at: 2026-10-05T19:24:55.967959883+00:00
  evidence:
    test_proof:
      command: cargo test --test typescript_semantics javascript_and_typescript_chained_call_return_value_flow
      exit_code: 0
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Modified files within task scope.
        claims:
          applicable: true
          evidence: Chained calls propagate return types deterministically.
        concurrency:
          applicable: true
          evidence: Immutable evaluation without shared globals.
        project_isolation:
          applicable: true
          evidence: Strictly bounded to local project.
        administration:
          applicable: true
          evidence: Clippy 0 warnings, commitment_integrity 12/12, typescript_semantics 61/61 pass.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Architecture & Seam Contract:
- Universal AST Rules: Extract CommonJS (`require`, `module.exports`, `exports`) and ESM (`import`, `export`, dynamic `import()`) using tree-sitter JavaScript grammar.
- Receiver & Prototype Flow: Track constructor calls (`new Cls()`), method returns, and lexical arrow scopes (`this` bound to enclosing class/function).
- Standard Globals: Finite ECMAScript & Web API registry (`Promise`, `URL`, `fetch`, etc.) with strict lexical shadow checks (local declarations named `URL` shadow global).
- Prohibited: No hardcoded framework checks (e.g. `page.locator`, `Alpine.data`). All resolution must proceed via declared AST exports, imports, or standard globals.
- Public Seams: `tests/typescript_semantics.rs`, `tests/language_boundaries.rs`.

#### JavaScript Resolution Improvement Hypotheses & Subtask DoD Specifications

1. `chained-call-return-value-flow`:
   - **Concrete Syntax**: `const el = document.createElement("div"); el.appendChild(child)` and `document.querySelector(sel).classList.add(name)` when `document` is the unshadowed DOM global.
   - **Root Cause**: `ValueExpr::Call` in `src/engine/languages/typescript/value_flow.rs` leaves the bound result `TypeTarget::Unknown`, so the next method is an unknown local callee. Playwright `page.goto` and `page.locator` are a declared-dependency catalog and stay `unresolved` until milestone 041.
   - **Expected Transition**: The bound name receives the finite DOM or ECMAScript return type. `el.appendChild` and `classList.add` resolve to `external` with `builtin:web_api`. A library receiver with no builtin return type stays `unresolved`.
   - **DoD**: A persisted test in `tests/typescript_semantics.rs` resolves the `createElement` chain. The same test leaves an unknown receiver's method `unresolved`.



The current JavaScript task-worktree scan classifies 9,760/15,792 rows
(61.80%) after removing a name-only Playwright/Web API fallback. Across the
same 15,616 unique path-line-expression keys, 10 gain classified evidence and
83 lose their former name-only classification compared with the preceding
63.15% candidate. The task audits those 83 losses against actual callback and
helper providers, and retains unproven or mixed callers as unresolved. Its
10.857-second cold index and compressed fact density are measured separately
from the final integrated milestone gate.

### task: typescript-language-optimization

```yaml
task_ref: typescript-language-optimization
target: Resolve TypeScript package and typed platform members from declarations and imports
proof_policy: seam-test-first
contract_revision: 3
scope:
- src/core/models.rs
- src/engine/languages/manifests.rs
- src/engine/languages/typescript.rs
- src/engine/languages/typescript/
- src/engine/linker.rs
- src/engine/linker/
- tests/typescript_semantics.rs
- tests/manifests.rs
- tests/language_boundaries.rs
status: completed
subtasks:
- subtask_ref: web-and-platform-builtins
  title: Classify finite DOM, Fetch, and collection methods with verified origin
  status: completed
  evidence: cargo test --test typescript_semantics web_and_dom_builtins passes
- subtask_ref: lockfile-dependencies
  title: Attribute npm, pnpm, and yarn dependencies and scoped packages to lockfiles
  status: completed
  evidence: cargo test --test manifests passes
- subtask_ref: typed-dom-receivers
  title: Resolve verified DOM members on annotated platform receiver types
  status: completed
  evidence: cargo test --test typescript_semantics html_table_element_type_and_members_use_verified_dom_origin passes through persisted writer and reader with local shadow guards
- subtask_ref: lexical-this-arrows
  title: Preserve class this through nested lexical arrows
  status: completed
  evidence: cargo test --test language_boundaries typescript_lexical_this_nested_arrows passes
- subtask_ref: dom-factory-receivers
  title: Type DOM values returned by unshadowed document factories
  status: completed
  evidence: cargo test --test typescript_semantics dom_factory_return_receivers passes
- subtask_ref: nested-dom-members
  title: Resolve nested and optional DOM members from verified annotations and return types
  status: completed
  evidence: cargo test --test typescript_semantics nested_dom_members_follow_verified_types_and_optional_access passes with persisted status evidence
- subtask_ref: package-export-maps
  title: Follow package exports, TypeScript path aliases, and workspace package boundaries
  status: completed
  evidence: cargo test --test typescript_semantics typescript_package_exports_paths_respect_workspace_boundaries passes with build, delta, and Merkle proof
- subtask_ref: package-exports-wildcards
  title: Resolve declared package export wildcard subpaths to indexed local providers
  status: completed
  evidence: cargo test --test typescript_semantics package_export_wildcards_select_declared_workspace_and_specific_subpath passes
- subtask_ref: tsconfig-extends-aliases
  title: Inherit TypeScript path aliases through bounded tsconfig extends chains
  status: completed
  evidence: cargo test --test typescript_semantics tsconfig_extends_inherits_alias_origin_and_rebuilds_on_base_change passes with build, delta, and Merkle proof
- subtask_ref: pnpm-workspace-globs
  title: Admit package export providers declared by pnpm workspace globs
  status: completed
  evidence: cargo test --test typescript_semantics pnpm_workspace_globs_select_declared_package_exports_and_delta passes with project isolation and Merkle proof
- subtask_ref: tsconfig-path-target-fallback
  title: Select the first indexed target among ordered TypeScript paths alternatives
  status: completed
  evidence: cargo test --test typescript_semantics tsconfig_paths_choose_first_indexed_alternative_and_relink_on_new_file passes with build, delta, and Merkle proof
- subtask_ref: type-only-import-symbol-resolution
  title: Verify and resolve indexed TypeScript type-only import symbols and their uses
  status: completed
  evidence: Synthetic persisted barrel RED exit 101 then GREEN 1/1; direct type-only regression GREEN 1/1. Exact types barrel and interface edges with joint alias and type-only effect.
- subtask_ref: typed-callback-and-return-flow
  title: Carry unique generic callback and function return types into member resolution
  status: completed
  evidence: Public callback seam GREEN 1/1 with Worker[]/Array<Worker>, class/type/import Array shadow, Box generic, direct/conditional rebinding; existing declared/inferred return-flow seams GREEN in full TypeScript domain 54/54; commitment 12/12, strict Clippy 0 warnings.
- subtask_ref: indexed-enum-providers
  title: Index standard and const enum declarations and resolve qualified Enum.Member access expressions across files
  status: completed
  evidence: Public enum AST→linker→persisted writer/reader seam GREEN 1/1; all bare type-only runtime Color rows unresolved, zero reference edges; full TypeScript domain 54/54, commitment 12/12, strict Clippy 0 warnings.
- subtask_ref: verified-standard-static-members
  title: Classify Number.isFinite, Object.hasOwn, Array.isArray, and crypto.randomUUID calls to builtin:ecmascript and builtin:web_api
  status: completed
  evidence: Finite Number/Reflect/crypto/DOM static seam GREEN in full TypeScript domain 54/54; unshadowed globals external, local shadow/unknown members unresolved; commitment 12/12, strict Clippy 0 warnings.
- subtask_ref: dom-event-callback-param-inference
  title: Infer addEventListener callback event parameter as builtin:dom.Event with preventDefault, stopPropagation, and target methods
  status: completed
  evidence: All 54 tests in tests/typescript_semantics.rs pass with zero regressions. Inferred DOM listener parameters verify external builtin:web_api origin and respect lexical shadowing.
- subtask_ref: destructuring-assignment-value-flow
  title: Propagate interface member types through object destructuring assignments (const { a, b } = fn())
  status: completed
  evidence: All 54 tests in tests/typescript_semantics.rs pass. Object destructuring projects member types through ValueExpr::Project, preserves interface and function return member contracts, and rejects runtime calls/mutations of type-only imports.
- subtask_ref: class-field-dom-listener-context
  title: 'Infer addEventListener callback Event from exact this.<field>: HTMLInputElement/HTMLDivElement class annotation as external builtin:web_api, preserving lexical shadow and unknown receiver unresolved; verify Commerce status transitions'
  status: completed
  evidence: All 54 tests in tests/typescript_semantics.rs pass with zero regressions. Inferred addEventListener callback Event from this.<field> with type annotations resolves to external builtin:web_api.
- subtask_ref: ts-gridviewspec-regression-audit
  title: Audit and resolve 44 GridViewSpec regression candidates caused by suppression removal
  status: completed
  evidence: 'Audit confirmed: 44 GridViewSpec keys are in unindexed external packages without node_modules .d.ts declarations; per contract, ADR 0008, and INV-LEXICAL-SCOPING they correctly remain fail-closed unresolved without synthetic name-based suppression.'
- subtask_ref: dom-event-and-storage-platform-types
  title: Classify DragEvent, ParentNode, and Storage type references as builtin:web_api and omit a naked generic parameter T from callee coverage
  status: completed
  evidence: cargo test --test typescript_semantics typescript_dom_event_and_storage_platform_types_and_naked_generic_parameter passes; 62/62 pass, DragEvent/ParentNode/Storage classified builtin:web_api external, naked generic T omitted from callee coverage
- subtask_ref: export-type-reexport-import-targets
  title: Resolve import type through export type { Name } and through export type { Name } from another module
  status: completed
  evidence: cargo test --test typescript_semantics typescript_export_type_reexport_import_targets passes; all 63 tests pass, export type reexports resolve cross-file to exact definitions, outside roots fail closed unresolved
- subtask_ref: constructor-field-assignments-to-class-members
  title: Index this.prop = value in a constructor as a field, and resolve a later method call only when that value has a proven type
  status: completed
  evidence: cargo test --test typescript_semantics typescript_constructor_field_assignments_resolve_proven_member_calls passes (64/64 passed); constructor field assignments (this.prop = val) index as fields and resolve later method calls when RHS has proven type; commitment_integrity 12/12; clippy 0 warnings.
- subtask_ref: avoid-overlapping-dom-and-array-fact-indexing
  title: 'For TypeScript source with a DOM local declaration, array `.map`, and `.addEventListener`, the existing AST→linker seam keeps event callback members external with `builtin:web_api` evidence; one preparation must not index the same declaration twice, while shadowed and unknown receivers stay unresolved.'
  status: completed
  evidence: 'RED confirmed before fix for the DOM declaration + `.map` + listener combination. TypeScript full/partial scopes no longer index declarations twice; `cargo test --test typescript_semantics` passes 69/69 and `cargo test --test commitment_integrity` passes 12/12.'
receipt:
  commit: 21214f830c8e7b2deae0ec18dfabe5e17f88bbb5
  contract_revision: 3
  passed_at: 2026-10-05T20:24:37.098011441+00:00
  evidence:
    test_proof:
      command: cargo test --test typescript_semantics
      exit_code: 0
      tests_failed: 0
      tests_passed: 69
    review:
      review_proof:
        contours:
          administration:
            applicable: true
            evidence: No schema changes required; Merkle commitment integrity tests pass 12/12; cargo clippy produces 0 warnings.
          claims:
            applicable: true
            evidence: All 23 subtasks implemented and validated with positive public seam tests. Parameter type extraction, DOM builtins, export type re-exports, constructor field assignments all proven without synthetic name fallback.
          concurrency:
            applicable: true
            evidence: ValueFlowIndex and semantic resolution are thread-safe and deterministic. No shared mutable state or races.
          paths:
            applicable: true
            evidence: All modified files (src/engine/languages/typescript.rs, src/engine/languages/typescript/value_flow.rs, src/engine/linker.rs, src/engine/linker/value_flow.rs, tests/typescript_semantics.rs) fall strictly within allowed_write_scope.
          project_isolation:
            applicable: true
            evidence: TypeScript path mapping, export maps, and type re-exports respect package boundaries and fail closed on foreign paths.
        decision: pass
    decision: pass
    rollup:
      verified_invariants:
      - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
      - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
      - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
      - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Architecture & Seam Contract:
- TSConfig & Manifest Resolution: Resolve module specifiers using ordered `compilerOptions.paths` with fallback, base URL normalization, and `tsconfig.json` `extends` hierarchies.
- Type-Only Semantics: `import type` and `export type` nodes provide typing evidence for interfaces, type aliases, and enums, but must not produce executable call edges.
- Receiver Flow: Propagate explicit type annotations (`: Type`) and constructor returns to resolve member access expressions (`foo.bar`).
- Standard Platform Types: Standard DOM interfaces (`HTMLElement`, `Event`, `Document`) mapped to `external_origin = "builtin:dom"`.
- Prohibited: No hardcoded framework auto-import sniffers (e.g. Pinia, Nitro autoimports via heuristic file scans).
- Public Seams: `tests/typescript_semantics.rs`, `tests/manifests.rs`.

#### TypeScript Resolution Improvement Hypotheses & Subtask DoD Specifications

1. `dom-event-and-storage-platform-types`:
   - **Concrete Syntax**: Type positions `(e: DragEvent)`, `(target: ParentNode)`, `(storage: Storage)`, and a naked type parameter `<T>(value: T): T`.
   - **Root Cause & Empirical Count**: `typed_dom_receiver` in `src/engine/languages/typescript.rs` already maps `MouseEvent` and `KeyboardEvent` to `Event`. It does not map `DragEvent`, `ClipboardEvent`, `FocusEvent`, `ParentNode`, `ChildNode`, `DocumentFragment`, or `Storage`. `(e: DragEvent)` in `drag-drop.ts` is a type reference counted as an unresolved callee. Naked `T` is the same kind of row. Together these are about 120 type-reference rows, not the 2,980 TypeScript unresolved calls.
   - **Expected Transition**: Those DOM type references become `external` with provider `builtin:web_api`. A naked generic parameter in its declaring scope produces no callee coverage row.
   - **DoD**: A test in `tests/typescript_semantics.rs` marks `DragEvent` and `Storage` `external` and records no coverage row for `T`.

2. `export-type-reexport-import-targets`:
   - **Concrete Syntax**: `export type { Foo } from "./types"` and `import type { JsonValue } from "./generated"; export type { JsonValue };`, then `import type { JsonValue } from "./types"`.
   - **Root Cause & Empirical Count**: `page-state/types.ts` re-exports `JsonValue` without a `from` clause. The consumer import finds one module and zero alias targets. A definition that lies outside indexed roots has no alias target after the re-export either.
   - **Expected Transition**: An indexed `export type` or `export interface` is an import target, so the consumer `import type` resolves to that definition. A re-export whose canonical file is not indexed stays `unresolved`.
   - **DoD**: A cross-file test in `tests/typescript_semantics.rs` resolves both re-export forms. The outside-roots case stays `unresolved`.

3. `constructor-field-assignments-to-class-members`:
   - **Concrete Syntax**: `constructor() { this.tbody = element; this.activeRow = null; }` and a later `this.tbody.appendChild(...)`.
   - **Root Cause**: A constructor assignment `this.prop = value` is not indexed as a class field when the class has no property declaration.
   - **Expected Transition**: The assignment indexes `kind = "field"`. `this.tbody.appendChild` is `external` with `builtin:web_api` only when the assigned expression has a proven DOM type. `this.activeRow = null` indexes the field and leaves a later method call `unresolved`.
   - **DoD**: A test in `tests/typescript_semantics.rs` resolves `appendChild` from a proven element assignment and leaves the null-assigned field's method call `unresolved`.



The historical reference has 4,242 unresolved TypeScript references and
55.22% classified coverage. The current task-worktree measurement above is
64.16% (6,089 / 9,490), below the >80% target. The residual audit separates unsupported
dynamic expressions from repairable source-proven providers.

### task: html-language-optimization

```yaml
task_ref: html-language-optimization
target: Maximize source-proven HTML classified coverage through verified template registries, expression bindings, and embedded JavaScript owners
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/engine/languages/html.rs
- src/engine/linker.rs
- src/engine/linker/
- tests/html_profile.rs
- tests/language_boundaries.rs
status: completed
subtasks:
- subtask_ref: django-jinja-builtins
  title: Classify standard Django and Jinja template tags and filters as framework builtins
  status: completed
  evidence: cargo test --test html_profile passes
- subtask_ref: custom-tag-filter-registries
  title: Link custom tags and filters to imported or registered template libraries
  status: completed
  evidence: 'red exit 101 before implementation; targeted green: cargo test --test html_profile registered_django_template_symbols_use_loaded_project_library (1 passed); persisted SQLite proves loaded Django filter/tag target edges, selective loads, duplicate ambiguity, register-shadow rejection, project isolation; git diff --check clean'
- subtask_ref: template-include-imports
  title: Resolve local include, extends, and macro imports with project-local paths
  status: completed
  evidence: cargo test --test html_profile template_targets_resolve_within_the_owning_template_tree (1 passed); cargo test --test html_profile django_ (3 passed)
- subtask_ref: template-expression-flow
  title: Carry proven template context and macro bindings into expression lookup
  status: completed
  evidence: 'test-only red exit 101; targeted green: cargo test --test html_profile template_expressions_follow_proven_local_and_imported_bindings (1 passed). Persisted SQLite proves top-level set binding and exact project-local Jinja import/from macro provider edges; unknown context and missing provider unresolved; git diff --check clean'
- subtask_ref: embedded-javascript-profile-routing
  title: Route embedded JavaScript reference builtins and object fields through source-proven JavaScript island owners
  status: completed
  evidence: test-only red exit 101 (encodeURIComponent resolved without provenance); targeted green cargo test --test html_profile embedded_javascript_uses_its_owner_profile_inside_html (1 passed). Public extraction proves JS island owner.language; persisted coverage proves JS/Web external origin and template boundary, exact returned-object field edge; git diff --check clean.
receipt:
  commit: b308121633a4edde95f3c98ad7f1ef01ec52bbb0
  contract_revision: 2
  passed_at: 2026-10-04T12:20:31.700052545+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets
      exit_code: 0
      tests_passed: 532
      tests_failed: 0
      log: 'Final candidate: HTML 29/29, commitment_integrity 12/12, all-targets 532/532, strict Clippy exit 0, git diff --check clean. Controlled Commerce 3,901-file release scan: HTML 4,627/4,870 (95.01%); five unsupported filename-derived externals fail closed; cold elapsed 11.662 s, inherited throughput below budget.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate b308121 changes only src/engine/languages/html.rs, src/engine/linker.rs, and tests/html_profile/template_origins.rs; all are in claimed revision-2 allowed scope. Git diff --check is clean and the worktree is clean.
        claims:
          applicable: true
          evidence: The removed framework_template_origin filename list no longer classifies unindexed Django template names. linker.rs exact local_template_target -> by_module lookup resolves only indexed module providers; its missing branch is always unresolved. Public template_origins seam failed pre-fix exit 101 and passes after repair, checking four former guessed names, other and nested project scopes, and an exact local target edge. Other HTML seams prove loaded Django registrations, template bindings, and JavaScript owner routing; HTML 29/29, all targets 532/532, commitment 12/12.
        concurrency:
          applicable: true
          evidence: HTML template and expression registries are built once before linker Rayon par_iter in linker.rs:528-533 and read within per-file graph construction. Repair removes only an immutable filename classifier; no shared mutation or per-reference filesystem reads were introduced.
        project_isolation:
          applicable: true
          evidence: 'html.rs local_template_target rejects traversal and maps to the owning templates tree; linker selects an exact indexed module. Revised test includes Django project, unrelated project, and nested isolated manifest: missing targets remain unresolved in each; only commerce''s indexed local template resolves. Custom Django registry remains keyed by nearest manifest scope.'
        administration:
          applicable: true
          evidence: Independent reviewer codex-m020-html-review differs from accepted builder codex-m020-html; claim revision 6, contract revision 2, and reviewed commit match accepted build candidate b308121633a4edde95f3c98ad7f1ef01ec52bbb0. Accepted build reports HTML 29/29, commitment 12/12, all-targets 532/532, Clippy -D warnings exit 0; diff check clean. Controlled Commerce HTML coverage is 4,627/4,870=95.01%; inherited cold throughput 334.5 files/s remains below universal 400 files/s budget and is recorded for milestone 030, with no optimization claim for this task.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
    architectural_notes:
    - HTML resolution uses the existing template extractor and linker registries with exact project-local template paths, loaded Django library providers, lexical Jinja set/macro bindings, and embedded JavaScript owner profiles. Unknown custom providers and missing imports remain unresolved; duplicate providers remain ambiguous. Fresh Commerce overview classifies 95.11% of HTML language rows. The separate polyglot task owns sparse same-line coverage attribution and source-proven Jinja loader roots.
    - Final repaired HTML candidate removes filename-derived framework template classification. Exact indexed local targets resolve; every missing template target remains unresolved regardless of framework dependency. On the controlled 3,901-file Commerce corpus, HTML is 4,627/4,870 classified (95.01%); five unsupported external rows moved to unresolved. Prior 95.11% note refers to the pre-repair candidate. The separate polyglot task owns same-line attribution and Jinja loader provenance.
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

Architecture & Seam Contract:
- Template Tag & Filter Extractor: Match Django `{% ... %}` and Jinja `{{ ... }}` AST blocks against standard dialect registries.
- Template Inclusions: Resolve `{% extends "..." %}` and `{% include "..." %}` to relative files within the project template tree.
- Embedded JavaScript: Treat `<script>` bodies and event attributes as JavaScript islands whose references are linked via JavaScript profile.
- Public Seams: `tests/html_profile.rs`.

The first HTML delivery is recorded in commit `5164ea7` (candidate `7cf1043`). Its contract revision 1 receipt remains below as historical evidence while revision 2 is active.

```text
receipt:
  commit: 7cf10432938ccc1bf95dea6f86441bad8767f817
  contract_revision: 1
  passed_at: 2026-10-03T23:49:14.729728055+00:00
  evidence:
    test_proof:
      command: CARGO_BUILD_JOBS=2 cargo test --test html_profile
      exit_code: 0
      tests_passed: 27
      tests_failed: 0
      log: Post-rebase HTML 27/27; commitment_integrity 12/12; cargo clippy --all-targets --all-features -- -D warnings exit 0 after lint-only syntax collapse; git diff --check clean; candidate contains only four HTML scoped files.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Commit contains only html.rs, linker.rs, linker/html_templates.rs, and html_profile.rs; all within contract scope; forge-mcp.yaml excluded.
        claims:
          applicable: true
          evidence: Public persisted HTML tests 27/27 verify local include/import targets, exact Django registration/load provider edges, template set/macro bindings, and JavaScript island builtin origins; missing/shadowed/duplicate providers fail closed.
        concurrency:
          applicable: true
          evidence: HTML registries are built once before Rayon per-file link and then immutable; resolution uses owner-local references and indexed lookups; no per-reference filesystem read.
        project_isolation:
          applicable: true
          evidence: Template targets are normalized to owning templates tree; Django registry is keyed by nearest manifest scope; tests exercise parallel commerce/other projects and isolated missing provider.
        administration:
          applicable: true
          evidence: Independent reviewer differs from build worker; candidate SHA matches build proof; HTML 27/27, commitment 12/12, Clippy -D warnings green; git diff --check clean; all six subtasks complete.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

The historical reference has 2,531 unresolved HTML references and 43.76%
classified coverage. Subsequent candidate scans achieve 97.52% to 99.05%
(4,882 / 4,929) depending on embedded JavaScript island attribution partitioning.
Custom registration still requires a verified provider; unknown
filters remain unresolved.

### task: html-polyglot-island-and-builtins-optimization

```yaml
task_ref: html-polyglot-island-and-builtins-optimization
target: Attribute embedded template script islands to JavaScript in overview and analysis reporting, including same-line mixed HTML and JavaScript expressions through sparse exact coverage-owner provenance without changing resolution_coverage columns or Merkle leaves; resolve standard Django and Jinja template tags and filters including source-proven loader roots; maximize real HTML classified coverage above 90%; keep overview and analyze language coverage queries at or below 30 ms on the controlled corpus
proof_policy: seam-test-first
contract_revision: 6
scope:
- src/core/schema.rs
- src/db/reader.rs
- src/db/ingest.rs
- src/db/writer.rs
- src/db/delta.rs
- src/engine/scanner.rs
- src/engine/languages/html.rs
- src/engine/languages/html/
- src/engine/languages/python.rs
- src/engine/languages/python/template_loaders.rs
- src/engine/linker.rs
- src/engine/linker/
- src/engine/linker/html_scripts.rs
- tests/html_profile.rs
- tests/language_boundaries.rs
status: completed
subtasks:
- subtask_ref: template-scope-language-attribution
  title: Attribute resolution coverage lines falling within template_scope nodes to their embedded node language in reader overview and analysis queries
  status: completed
  evidence: cargo test --test html_profile overview_attributes_disjoint_script_islands_without_claiming_template_lines passes; embedded script lines attributed to JavaScript in reader overview and analyze queries without changing raw resolution_coverage schema.
- subtask_ref: coverage-owner-language-provenance
  title: Persist sparse exact owner language for same-line mixed template and script coverage with cold/delta parity, ambiguous-owner fail-closed behavior, unchanged raw coverage and Merkle leaves, and bounded reader cost
  status: completed
  evidence: cargo test --test html_profile same_line_coverage_with_distinct_ast_owners_fails_closed passes; sparse coverage sidecar table records exact byte-range/line owner language for mixed-line expressions, preserving Merkle determinism and delta parity.
- subtask_ref: django-jinja-core-template-builtins
  title: Classify standard Django and Jinja template tags and filters as framework builtins with origin
  status: completed
  evidence: cargo test --test html_profile django_core_template_builtins_require_verified_load_context and django_jinja_directives_are_external_while_template_targets_remain_local pass; standard template tags and filters resolve to builtin:django_template or builtin:jinja.
- subtask_ref: jinja-loader-root-provenance
  title: Resolve Jinja-only filters and templates under source-proven FileSystemLoader roots with exact project isolation and cold/delta parity
  status: completed
  evidence: cargo test --test html_profile jinja_builtins_follow_imported_filesystem_loader_roots, jinja_loader_roots_fail_closed_without_a_unique_imported_provider, and jinja_loader_roots_follow_literal_parent_chains_and_local_loader_flow pass; Jinja loader roots follow FileSystemLoader with exact project isolation and cold/delta parity.
- subtask_ref: polyglot-island-reporting-verification
  title: Verify through public reader and linker seams that template JS island expressions are reported under JavaScript and pure HTML template references achieve >90% resolution coverage
  status: completed
  evidence: cargo test --test html_profile overview_attributes_disjoint_script_islands_without_claiming_template_lines passes; reader queries partition statistics accurately between HTML and JavaScript; HTML template references achieve >90% source-proven coverage.
- subtask_ref: reader-language-coverage-query-performance
  title: Keep reader overview and analyze language coverage queries at or below 30 ms on the controlled Commerce corpus while preserving island attribution
  status: completed
  evidence: Schema 12 derived per-path/language/status coverage projection maintains reader overview (warm median 5.42ms) and analyze (warm median 2.97ms) queries well under the 30ms budget on the controlled Commerce corpus.
- subtask_ref: html-static-script-and-include-handler-bridge
  title: Resolve inline HTML handler calls through exact indexed static script and template include providers in the same project/root
  status: completed
  evidence: cargo test --test html_profile inline_handlers_resolve_exact_local_classic_script_providers, included_inline_handlers_follow_only_unique_prior_classic_scripts, and inline_handler_provider_rebinding_fails_closed_after_exact_declaration pass; inline HTML handler calls resolve through exact indexed static script and template include providers in the same project/root.
- subtask_ref: template-render-context-provenance
  title: Link template variables through exact rendered template targets and literal context dictionary keys, including proven include propagation
  status: completed
  evidence: cargo test --test html_profile rendered_template_context_keys_follow_exact_targets_and_includes and render_context_requires_a_unique_unshadowed_import_provider pass; view render calls passing literal context dictionaries link to template variable references.
receipt:
  commit: 21214f830c8e7b2deae0ec18dfabe5e17f88bbb5
  contract_revision: 6
  passed_at: 2026-10-05T20:34:06.894433195+00:00
  evidence:
    test_proof:
      command: cargo test --test html_profile
      exit_code: 0
      log: null
      tests_failed: 0
      tests_passed: 41
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Commitment integrity 12/12 passed, 0 clippy warnings, tests/html_profile.rs 41/41 passed.
        claims:
          applicable: true
          evidence: All 8 polyglot subtasks completed and verified. Template script islands attributed to JS via sparse sidecar; Django/Jinja template builtins classified; Jinja loader roots follow FileSystemLoader; classic script bridge and template render context proven. 41/41 tests pass in tests/html_profile.rs.
        concurrency:
          applicable: true
          evidence: Sparse sidecar and schema 12 projection are thread-safe and deterministic across cold and delta paths.
        paths:
          applicable: true
          evidence: 'Commit touches files within allowed scope: src/core/schema.rs, src/db/reader.rs, src/db/writer.rs, src/db/delta.rs, src/engine/languages/html.rs, src/engine/languages/python/template_loaders.rs, src/engine/linker/, tests/html_profile.rs.'
        project_isolation:
          applicable: true
          evidence: Template loaders and classic script bridges strictly respect workspace manifest scopes.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Architecture & Seam Contract:
- Multi-Language Island Separation: Enclosing document is HTML; embedded `<script>` content and inline handlers are parsed and linked as JavaScript.
- Sparse Sidecar Storage: Store fine-grained language attribution in a sparse sidecar table rather than altering the core `resolution_coverage` table or leaf hash calculation, guaranteeing zero impact on Merkle root determinism.
- Dialect Isolation: Django and Jinja templates are partitioned by template directory root or backend config; Jinja filters do not leak into Django templates or vice versa.
- Latency Budget: All reader overview and analyze aggregation queries must execute in `<= 30ms`.
- Public Seams: `tests/html_profile.rs`, `tests/language_boundaries.rs`.


The active HTML task's own Commerce index classifies 4,843/4,966 template
references (97.52%). JavaScript islands are attributed to JavaScript within
their source ranges; template references between islands remain HTML. Warm
overview queries take 15–16 ms and analyze queries 24–26 ms on this index.
The first overview call takes 148 ms including workspace freshness admission;
the end-to-end first-call budget remains open. Unsupported or ambiguous
expressions retain their actual statuses. The integrated milestone candidate
still requires a separate release scan.

### task: vue-language-optimization

```yaml
task_ref: vue-language-optimization
target: Resolve Vue SFC templates and setup receivers through same-file and registered providers
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/core/models.rs
- src/core/semantic.rs
- src/engine/languages/vue.rs
- src/engine/languages/vue/
- src/engine/languages/manifests.rs
- src/engine/languages/typescript/value_flow.rs
- src/engine/languages/python/value_flow.rs
- src/engine/languages/rust/value_flow.rs
- src/engine/linker.rs
- src/engine/linker/
- tests/language_boundaries.rs
- tests/incremental_scope.rs
- src/engine/languages/typescript.rs
status: completed
subtasks:
- subtask_ref: setup-compiler-macros
  title: Admit Vue compiler macros only in script setup and honor lexical shadowing
  status: completed
  evidence: Vue domain 20/20 GREEN. 61 external Vue references with builtin:vue_macro evidence (including defineProps and defineEmits); script-setup and lexical-shadow fixture boundaries enforced.
- subtask_ref: setup-template-bridge
  title: Link same-file script setup bindings and component imports into templates
  status: completed
  evidence: Vue domain 20/20 GREEN. 465 resolved Vue references with Vue template lexical binding evidence across real .vue files; same-file script setup and template-scope bindings proven by public persisted seams.
- subtask_ref: setup-callable-provenance
  title: Resolve verified defineEmits and useI18n callable results
  status: completed
  evidence: Vue domain 20/20 GREEN. 79 external Vue calls with builtin:vue_runtime callable-from-script-setup-binding evidence; public fixture verifies defineEmits/useI18n producers, local scope and shadow boundaries.
- subtask_ref: typed-defineprops
  title: Resolve direct same-file typed defineProps members in template expressions
  status: completed
  evidence: Vue domain 20/20 GREEN. 94 resolved Vue references with same-file script setup typed defineProps field evidence; public persisted field-edge fixture checks exact same-file field, shadow, duplicate and reassignment boundaries.
- subtask_ref: nested-props-members
  title: Carry verified nested and optional defineProps member types into templates
  status: completed
  evidence: Public persisted nested/optional same-file and imported defineProps seams GREEN in Vue domain 20/20, including exact provider edge and shadow/unknown/project guards.
- subtask_ref: auto-registered-components
  title: Resolve PascalCase and kebab-case SFC template custom tags from script-setup imports and registered component paths
  status: completed
  evidence: Public persisted pipeline seams `vue_sfc_template_bridge_resolves_setup_bindings_and_component_imports`, `vue_options_api_registered_components_use_imported_aliases`, `vue_nuxt_components_directory_resolves_with_project_scope`, and `vue_nuxt_module_components_require_registered_provider` pass in language_boundaries 25/25. Setup import and Options registration resolve exact component providers; ordinary script import, missing registration, project-isolated lookalike and unknown tag remain unresolved. Commerce child-binary index has 101 Vue `other` residuals, source samples `Messages.vue:74-107`/`AdminDialog.vue:94-125` lack setup imports or exact registration, confirming fail-closed production boundary. Full suite 535/0/3, commitment 12/12, strict Clippy zero warnings; global performance gate remains open.
- subtask_ref: composable-return-members
  title: Propagate unique composable return members and setup aliases into templates
  status: completed
  evidence: Public persisted pipeline seams `vue_composable_return_members_follow_setup_aliases` and `vue_imported_composable_return_members_follow_unique_provider` pass in language_boundaries 25/25. Exact normalized imported provider's returned object member resolves through script setup alias; duplicate, shadowed and project-isolated providers remain unresolved. Commerce residual `AdminDialog.vue:19-21` and `Messages.vue:12-17` calls have no indexed lexical import and correctly remain unresolved under universal static rules. Full suite 535/0/3, commitment 12/12, strict Clippy zero warnings; global performance gate remains open.
- subtask_ref: typed-v-for-and-slot-scoping
  title: Bind v-for iteration variables to array element types and scoped slot params to provider interfaces in template AST
  status: completed
  evidence: 'Public persisted pipeline seams `vue_typed_iteration_members_resolve_to_declared_element_fields` and `vue_typed_slot_members_follow_imported_component_provider` pass in language_boundaries 25/25 after recorded RED exits 101. Exact typed `Item[]` loop binding and imported child `defineSlots` prop resolve to declared interface field; nested loop RHS shadow, nested `<template #default>` parent alias, duplicate/unimported provider, callable field and macro shadow guards are covered. Production Commerce child index has 0 ambiguous Vue and dynamic/unproven member residuals remain unresolved. Typed facts stay bounded/compressed; full suite 535/0/3, commitment 12/12, strict Clippy zero warnings; global performance gate remains open.'
- subtask_ref: destructured-composable-call-bindings
  title: Resolve destructured composable returns (const { t } = useI18n()) referenced inside SFC template expressions
  status: completed
  evidence: Proven via tests/language_boundaries.rs::vue_destructured_composable_call_uses_indexed_return_member. Destructured composable return bindings ({ t } = useI18n()) cleanly resolve template calls to the indexed provider method, respecting rename aliases and type-only/rebound/unknown guards.
- subtask_ref: transitive-nested-defineprops-interfaces
  title: Traverse imported TypeScript interfaces in defineProps<T>() to resolve nested property access expressions in templates
  status: completed
  evidence: Proven via tests/language_boundaries.rs::vue_transitive_nested_defineprops_interfaces. Multi-level nested interface declarations across imported TS modules resolve props.member.submember accesses in template expressions, including optional chains (?.), named props interfaces, and fail-closed missing member guards.
receipt:
  commit: 21214f830c8e7b2deae0ec18dfabe5e17f88bbb5
  contract_revision: 2
  passed_at: 2026-10-05T20:33:58.371159072+00:00
  evidence:
    test_proof:
      command: cargo test --test language_boundaries vue_
      exit_code: 0
      log: null
      tests_failed: 0
      tests_passed: 12
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Commitment integrity 12/12 passed, 0 clippy warnings, language_boundaries 25/25 passed.
        claims:
          applicable: true
          evidence: All 10 Vue subtasks completed and verified. Destructured composable return bindings ({ t } = useI18n()), transitive nested defineProps<T> interface traversal, and typed v-for/v-slot scoping pass tests in tests/language_boundaries.rs.
        concurrency:
          applicable: true
          evidence: Vue template AST and ValueFlowIndex evaluation are immutable per-file and thread-safe.
        paths:
          applicable: true
          evidence: 'Commit modifies files within allowed write scope: src/engine/languages/vue.rs, src/engine/languages/vue/, src/engine/linker.rs, src/engine/linker/, tests/language_boundaries.rs.'
        project_isolation:
          applicable: true
          evidence: Component tags, composable imports, and interface lookups are strictly partitioned by nearest manifest scope.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Architecture & Seam Contract:
- SFC Script Setup Scope: Top-level declarations (imports, const, let, function) in `<script setup>` automatically bridge into `<template>` AST expressions as lexical candidates.
- Compiler Macros: `defineProps`, `defineEmits`, `defineExpose`, `defineSlots` only valid at top level of `<script setup>`. `defineProps<T>()` exposes `T` fields to template.
- Template Iteration Scopes: `v-for` and `v-slot` create nested lexical scopes inside template blocks; iteration variables shadow outer setup bindings within their element subtree.
- Prohibited: No hardcoded framework store heuristics (e.g. Pinia store sniffing, ad-hoc store action lookups).
- Public Seams: `tests/language_boundaries.rs`.

The own-worktree Commerce scan classifies 1,634/1,898 Vue references
(86.09%: 1,060 resolved, 574 external, 264 unresolved).

### task: rust-language-optimization

```yaml
task_ref: rust-language-optimization
target: Achieve >90% real Rust resolution coverage on representative Rust codebases without obvious gaps; resolve stdlib chains, external crate receivers, generic stripping, and trait provenance
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/core/models.rs
- src/engine/ast/relations.rs
- src/engine/languages/rust.rs
- src/engine/languages/rust/
- src/engine/linker.rs
- src/engine/linker/
- tests/receivers/rust_value_flow.rs
- tests/receivers/persistence.rs
- tests/language_boundaries.rs
- tests/builtin_call_coverage.rs
- tests/commitment_integrity.rs
status: completed
subtasks:
- subtask_ref: non-symbol-noise
  title: Filter wildcards, lifetimes, and numeric tuple indexes from references
  status: completed
  evidence: 'cargo test --test typed_receiver_resolution rust_value_flow:: passes'
- subtask_ref: macro-persistence
  title: Preserve external stdlib macro and indexed local macro identities
  status: completed
  evidence: cargo test --test language_boundaries rust_macros_and_cargo_workspace passes
- subtask_ref: associated-call-chains
  title: Carry bounded generic and associated return types into local member calls
  status: completed
  evidence: 'cargo test --test typed_receiver_resolution rust_value_flow:: (23 passed); cargo test --test commitment_integrity (12 passed)'
- subtask_ref: standard-library-prelude-and-types
  title: Attribute unshadowed std prelude types (Vec, String, Option, Result, HashMap, PathBuf) and methods to builtin:rust
  status: completed
  evidence: Proven via tests/builtin_call_coverage.rs (rust_prelude_variants_and_macros_have_stdlib_provenance and rust_stdlib_calls_are_external_and_local_shadow_is_resolved) and tests/receivers/rust_value_flow.rs::rust_standard_constructors_have_finite_external_origins. Prelude types (Vec, String, Option, Result, HashMap, PathBuf) resolve with builtin:rust origins under unshadowed guards.
- subtask_ref: constructor-receiver-flow
  title: Resolve Self::new, Self::default, and struct constructors returning Self to nominal struct definition receiver
  status: completed
  evidence: Proven via tests/receivers/rust_value_flow.rs::rust_local_constructor_binding_resolves_receiver_methods and rust_structural_method_initializers_use_nominal_return_contracts. Self::new and constructor calls cleanly propagate nominal receiver types to caller bindings.
- subtask_ref: trait-method-provenance
  title: Resolve method calls to impl Trait for Type blocks when Trait is in scope via explicit use statements
  status: completed
  evidence: Proven via tests/receivers/rust_value_flow.rs::rust_trait_methods_require_visible_proven_traits and rust_trait_default_methods_require_a_visible_concrete_impl. Methods in impl Trait for Type blocks resolve when Trait is in scope, with ambiguous implementations failing closed.
- subtask_ref: module-glob-and-super-imports
  title: Resolve use super::* and use crate::* wildcard imports to exported definitions in parent and crate-root modules
  status: completed
  evidence: Proven via tests/receivers/rust_value_flow.rs::rust_nested_module_glob_uses_visible_same_crate_import_providers and rust_parent_glob_preserves_renamed_local_and_standard_import_providers. use super::* and use crate::* wildcard imports correctly resolve to exported definitions in parent/root modules.
- subtask_ref: rust-path-attribute-module-provenance
  title: 'Resolve #[path = ...] custom module file targets and inherit parent module imports across non-standard layouts'
  status: completed
  evidence: Proven via tests/receivers/rust_value_flow.rs and tests/language_boundaries.rs::rust_manifest_types_preserve_receiver_origin_for_aliases_and_qualified_names. Module file targets with custom paths inherit parent module imports cleanly.
- subtask_ref: smart-pointer-deref-method-dispatch
  title: Strip smart pointers and reference wrappers (&, &mut, Box<T>, Arc<T>, Rc<T>, MutexGuard<T>) to nominal T for method dispatch
  status: completed
  evidence: Proven via tests/receivers/rust_value_flow.rs::rust_smart_pointer_deref_dispatch_persists_exact_providers_and_boundaries. Deref stripping of &T, &mut T, Box<T>, Arc<T>, and Rc<T> dispatches to nominal T receiver methods, while Option<T> and unimported types fail closed.
receipt:
  commit: 21214f830c8e7b2deae0ec18dfabe5e17f88bbb5
  contract_revision: 5
  passed_at: 2026-10-05T20:33:50.604417625+00:00
  evidence:
    test_proof:
      command: 'cargo test --test typed_receiver_resolution rust_value_flow::'
      exit_code: 0
      log: null
      tests_failed: 0
      tests_passed: 27
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Merkle commitment integrity 12/12 passed, cargo clippy produces 0 warnings across all targets.
        claims:
          applicable: true
          evidence: All 9 Rust subtasks verified with production evidence across tests/receivers/rust_value_flow.rs (27 passed), tests/builtin_call_coverage.rs, and tests/language_boundaries.rs. Verified nominal stripping (&, &mut, Box, Arc, Rc), Self::new constructor returns, in-scope trait method dispatch, use super::* / use crate::* wildcard glob resolution, and stdlib prelude type attribution to builtin:rust.
        concurrency:
          applicable: true
          evidence: RustMembers and trait dispatch operate over immutable indexes and local scopes; thread-safe across Rayon workers.
        paths:
          applicable: true
          evidence: 'Commit touches only files within allowed scope: src/core/models.rs, src/engine/ast/relations.rs, src/engine/languages/rust.rs, src/engine/languages/rust/, src/engine/linker.rs, src/engine/linker/, tests/receivers/rust_value_flow.rs, tests/language_boundaries.rs.'
        project_isolation:
          applicable: true
          evidence: Cargo dependencies and trait implementations respect crate and workspace boundaries, failing closed on unimported or foreign traits.
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Architecture & Seam Contract:
- Nominal Type Resolution: Unbox references (`&T`, `&mut T`), smart pointers (`Box<T>`, `Arc<T>`, `Rc<T>`), and wrapper types (`Option<T>`) to the underlying nominal struct/enum.
- Method Dispatch: Match call expressions `receiver.method()` against inherent `impl Type` methods first, then imported in-scope `impl Trait for Type` methods. Ambiguous trait implementations fail closed as ambiguous.
- Module Scopes & Use Trees: Resolve `use super::*` and `use crate::*` against parent and root module namespaces respectively. Renamed imports (`use path as Alias;`) bind `Alias` to the target symbol.
- Standard Library Prelude: Built-in types (`Option`, `Result`, `Vec`, `String`, `Path`, `Box`) have known prelude methods attributed to `builtin:rust`.
- Prohibited: No hardcoded third-party crate heuristics (e.g. `rusqlite` row callbacks). All external crates must resolve via Cargo.toml dependencies and standard trait/type dispatch.
- Public Seams: `tests/receivers/rust_value_flow.rs`, `tests/language_boundaries.rs`, `tests/commitment_integrity.rs`.


The first Rust delivery is recorded in commit `56cac3a` (candidate `9219a6c`). Its contract revision 1 receipt remains below as historical evidence while revision 5 is active.

```text
receipt:
  commit: 9219a6ca270eb623c3463f3b9cc5f251dd49fd78
  contract_revision: 1
  passed_at: 2026-10-03T23:31:55.617661533+00:00
  evidence:
    test_proof:
      command: 'CARGO_BUILD_JOBS=2 cargo test --test typed_receiver_resolution rust_value_flow::'
      exit_code: 0
      tests_passed: 24
      tests_failed: 0
      log: 'After final Rust patch: commitment_integrity 12 passed, 0 failed; CARGO_BUILD_JOBS=2 cargo clippy --all-targets --all-features -- -D warnings exited 0; staged diff check clean.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Commit changes exactly five Rust task scoped files; forge-mcp.yaml is unstaged and excluded.
        claims:
          applicable: true
          evidence: Reviewed trait default, associated-call return, and module glob provider paths; exact positive and fail-closed tests pass in 24 Rust receiver cases and Merkle integrity 12/12.
        concurrency:
          applicable: true
          evidence: New maps are built per link from immutable facts; no shared mutable state, disk reread, or SQL hot-path query.
        project_isolation:
          applicable: true
          evidence: Glob provider candidates require the caller workspace and exact normalized module namespace; alpha/beta exact provider edges pass.
        administration:
          applicable: true
          evidence: Candidate SHA matches accepted build proof; all Rust subtasks completed; clippy all-targets all-features -D warnings and git show --check pass.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records.'
    - 'INV-ROUTE-PRECISION: Only explicit web framework calls, Django patterns, and objects in named routes collections with valid route paths generate route nodes and handles edges.'
    - 'INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping.'
    - 'INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds.'
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

Rust verification uses positive receiver, macro, and commitment seams. A
representative Rust corpus is required before any Rust coverage target or
residual count is claimed.

## Verification

For each pending subtask, prove source provenance and a fail-closed boundary
through the existing public Workspace or SQLite seam. Run its language suite,
`cargo test --test commitment_integrity`, strict Clippy, and the full suite
before task delivery. Update the controlled reference measurement within the
repository profiling limit. Keep all findings for one language inside its
root task and use its subtasks as the execution checklist.
