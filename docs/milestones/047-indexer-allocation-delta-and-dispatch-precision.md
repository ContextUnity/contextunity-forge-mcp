---
id: m-indexer-allocation-delta-and-dispatch-precision
title: Zero-allocation linker hot paths, bounded delta frontier, and explicit unwrap/polymorphic resolution
doc_type: contract
status: active
repository: forge-mcp
project: forge-mcp
depends_on:
- m-bounded-semantic-resolution-and-language-boundaries
owners:
- src/core/models/compact_graph.rs
- src/core/semantic.rs
- src/core/schema.rs
- src/engine/linker.rs
- src/engine/linker/indexing.rs
- src/engine/linker/resolution.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/value_flow.rs
- src/engine/linker/contracts.rs
- src/engine/languages/rust.rs
- src/engine/languages/rust/linker.rs
- src/engine/languages/rust/value_flow.rs
- src/engine/languages/typescript/value_flow.rs
- src/engine/languages/python.rs
- src/engine/languages/python/value_flow.rs
- src/db/delta.rs
- src/db/ingest.rs
- src/db/writer.rs
- docs/architecture/indexing.md
- docs/architecture/performance.md
- docs/reference/languages.md
- tests/linker/
- tests/incremental/
- tests/languages/
- tests/receivers/
- tests/query_context/
invariants:
- 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
- 'INV-UNCERTAINTY: Candidate evidence (possible_calls, confidence=possible) never becomes an exact calls edge on concrete trait implementors or certifies removal safety; overflow beyond K=8 widens to Unknown/Incomplete while retaining analysis_gaps.'
- 'INV-ZERO-ALLOC-LOOKUP: Qualified symbol lookups across resolution, semantic_context, contracts, and delta reuse scratch String buffers or prefix strips instead of per-iteration format! heap allocations, with zero new Cargo dependencies.'
- 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
related_plans: []
started_at: 2026-10-14T17:05:07+00:00
---

# Zero-allocation linker hot paths, bounded delta frontier, and explicit unwrap/polymorphic resolution

## Outcome and purpose

Following direct code and test-suite verification of commit `3bb9cdbcf7ae4fcbd7c79e24bc78aba68bdc196e` (after Milestone 046), this milestone eliminates the remaining heap-allocation and incremental-delta bottlenecks without adding external Cargo dependencies, while closing three static resolution gaps in a way that preserves all existing negative tests (`tests/receivers/rust_value_flow.rs:521, 564, 1192`, `tests/languages/typescript/grammar.rs:461`, and `tests/query_context/symbol_context.rs:607`):

1. **Eliminate Linker, Value-Flow & Delta Heap Allocations (Zero New Crate Dependencies):**
   - In `src/engine/linker/resolution.rs` (lines 274–276), per-file `aliases`, `external_aliases`, and `type_only_aliases` allocate nested `HashMap<String, HashMap<String, ...>>`, cloning scope and symbol strings on every import and wildcard expansion. Convert these keys to borrowed `&'a str` slices tied to the `Facts` arena, and at line 2110 replace `lookup(&format!("{}.{}", n.qualname, rest))` with `lookup(qualified(&mut lookup_key, &n.qualname, rest))`.
   - In `src/engine/linker/semantic_context.rs` (lines 311, 840, 1382, 1492, 1584) and `src/engine/linker/contracts.rs` (lines 361, 409), lexical scope walks allocate `format!("{scope}.{name}")` on the heap on every step. Replace these with reusable `super::qualified(&mut lookup_key, scope, name)` scratch buffers.
   - In `src/engine/linker/value_flow.rs` (lines 217–223, 372, 399), `Scope.bindings` allocates `String` keys and heap `Vec<Binding<'a>>` even when a parameter or local variable has only a single binding. Borrow `&'a str` keys from `Node` / `ValueFlowFacts` and use a pure-Rust inline enum `BindingList<'a> { One(Binding<'a>), Many(Vec<Binding<'a>>) }` (requiring zero `Cargo.toml` changes).
   - In `src/core/models/compact_graph.rs` (line 142), convert `Coverage.evidence` from `String` to `Cow<'static, str>` so static literal diagnostic strings (`"type guard condition is unproven or shadowed"`, `"callee is shadowed by a parameter or local binding of unknown callable identity"`, etc.) use `Cow::Borrowed` with zero heap allocation.
   - In `src/db/delta.rs` (lines 199–204 and 243–248), replace the 3 per-node `format!("{prefix}.{name}")` allocations in the top-level symbol check with a zero-allocation helper `is_prefixed_qualname(qual, prefix, name)`.

2. **Explicit Unwrapping for `Result<T, E>?` / `Option<T>?` (Rust), `await Promise<T>` (TS/JS), and already-extracted `Optional[T]` / `Union[T, None]` (Python), Plus Bounded `dyn Trait` Dispatch:**
   - **Why implicit unwrapping in `resolve_applied` is forbidden by existing tests**: `tests/receivers/rust_value_flow.rs:521, 1192` explicitly assert that calling `optional.work()` directly on `optional: Option<Service>` or `result: Result<Service, Error>` stays `"unresolved"` (while `optional.is_some()` and `result.is_ok()` resolve as `"external"` std builtins), because `Option` and `Result` do not implement `Deref`. Similarly, `tests/languages/typescript/grammar.rs:461` and `tests/receivers/rust_value_flow.rs:564` assert that calling an `async` factory *without* `await` (`const client = make(); client.work()`) stays `"unresolved"`.
   - **Explicit Unwrapping Solution Across Languages**:
     - **Rust `?` (`try_expression`) and `.unwrap()` / `.expect(...)`**: Keep un-unwrapped `Result<T, E>` and `Option<T>` as `Builtin("Result")` / `Builtin("Option")`. `src/engine/languages/rust/value_flow.rs` emits `ValueExpr::Project` for `try_expression` and `.unwrap()` / `.expect(...)`. `src/engine/languages/rust.rs` (`call_result_hint`) stores that projection in the existing `ReceiverHint::CallResult { callee: format!("value:{json}"), member }` without editing `src/core/models.rs`. The linker unwraps `Result<T, E>`, `anyhow::Result<T>`, and `Option<T>` to `args[0]`.
     - **TypeScript / JavaScript `await Promise<T>` (`src/engine/languages/typescript/value_flow.rs`, `src/engine/linker/value_flow.rs`)**: When a TS/JS function is `async` or returns `Promise<T>`, populate `facts.async_return_type = Some(T)` while keeping `facts.return_type = Some(TypeExpr::Unknown)` (so un-awaited calls stay unresolved), emit `ValueExpr::Await` for `await_expression`, and populate `index.async_returns` for TypeScript/JavaScript alongside Python. Unwrapping applies on explicit `await` for calls (`(await make()).work()`), parameters (`(await p).work()`), and local aliases (`const a = p; (await a).work()`). Un-awaited receivers stay unresolved.
     - **Python `Optional[T]` / `Union[T, None]` and PEP 604 `T | None` unwrapping**: `src/engine/languages/python/value_flow.rs` parses PEP 604 binary union expressions (`T | None`, `None | T`) and forward-ref strings into `TypeExpr::Applied { base: "Union", args: [...] }`. `resolve_payload` / `explicit_payload` in linker value flow unwrap one non-null payload from `Optional[T]`, `Union[T, None]`, and `T | None` without requiring `typing.Union` in scope. A `Union` with two or more non-null types, or referencing `NoneType`, stays `unresolved`. ABC dispatch is deferred.
   - **Bounded `dyn Trait` Dispatch (`src/engine/linker/resolution.rs`)**:
     - Preserve `trait_object_dispatch` in `graph.coverage` and never emit exact `calls` edges to concrete implementors.
     - Emit an `"inferred"` `calls` edge to `Trait::method` and bounded `possible_calls` (`confidence = "possible"`, up to `MAX_CANDIDATE_BRANCH_JOINS = 8`). Overflow widens to `Unknown/Incomplete`.
     - Python ABC dispatch is deferred with the Python parser work.
     - Cross-file dirtying of callers when trait implementors are added, changed, or removed is owned by Task 3 in `src/db/delta.rs`.
   - **Rust `macro_rules!` Transcription References (`src/engine/languages/rust.rs`)**:
     - Extract static call/type identifier references inside `macro_definition` transcription bodies as `kind = "references"` owned by the `macro` node while preserving the `macro_expansion_not_analyzed` gap on invocation sites (`tests/query_context/symbol_context.rs:701`).

3. **Deterministic Resolution-Identity Digest & Indexed Delta Frontier (`src/db/delta.rs`, `src/engine/linker/contracts.rs`):**
   - Add `Contracts::resolution_digest(&self, facts: &Facts) -> BTreeSet<[u8; 16]>` using the existing `sha2` dependency (no new crate and no XOR fold) over `(node.id, node.kind, node.name, node.qualname, node.path, node.is_test, node.language, node.generated, self.details(node))` for non-component nodes. `resolution_identity_changed` compares those digest sets. `details` remains a JSON value; the identity set stops being `BTreeSet<String>` built by `serde_json::to_string`.
   - Invariant `INV-DELTA-NO-SOURCE-IO`: `src/db/delta.rs` must not perform file I/O (`std::fs`), string parsing, or regex on source files. Hop decisions read indexed facts only.
   - Re-exports are the module `details.exports` array already persisted from `ExportBinding` (`name`, `module`, `star`) by `src/engine/ast/mod.rs`. This milestone does not add `ImportKind`, `Facts.reexports`, or parser changes.
   - `import_frontier` includes `"rust"` with `"python" | "javascript" | "typescript" | "vue"`. Adding, changing, or removing an `impl Trait` dirties files that call `Trait::method`. ABC implementor dirtying is deferred with ABC dispatch.
   - Hop 1 (direct importers of `changed_resolution`) is always dirtied. Hop 2+ is dirtied only when the owner's indexed `details.exports` re-exports that upstream provider (`module` names it, or `star` is true) or `details.value_flow.return_contract` exposes the provider, including a qualified `namespace.Type`. A missing export list or an unresolvable import expands fail-safe so the Merkle root still matches a cold build.

---

## Tasks in this milestone

### task: zero-alloc-linker-and-value-flow-buffers

```yaml
task_ref: zero-alloc-linker-and-value-flow-buffers
target: Eliminate per-file and per-lookup String/Vec heap allocations across resolution.rs, semantic_context.rs, value_flow.rs, contracts.rs, delta.rs, and compact_graph.rs without adding external dependencies
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/core/models/compact_graph.rs
- src/engine/linker.rs
- src/engine/linker/indexing.rs
- src/engine/linker/resolution.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/value_flow.rs
- src/engine/linker/contracts.rs
- src/db/delta.rs
- docs/architecture/performance.md
- tests/linker/optimizations.rs
depends_on: []
invariants:
- 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
- 'INV-ZERO-ALLOC-LOOKUP: Qualified symbol lookups across resolution, semantic_context, contracts, and delta reuse scratch String buffers or prefix strips instead of per-iteration format! heap allocations, with zero new Cargo dependencies.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
status: completed
subtasks:
- subtask_ref: borrowed-alias-maps-in-resolution
  title: Convert aliases, external_aliases, and type_only_aliases in src/engine/linker/resolution.rs (lines 274-276) from owned String keys to borrowed &'a str slices and replace format! at line 2110 with qualified(&mut lookup_key, ...)
  status: completed
  evidence: 'Borrowed alias names and owner scopes replace per-import String clones; external provenance payload remains owned. Synthetic file_module fallback scopes retain borrowed local storage with inferred key lifetime. Imported implementation member lookup uses qualified scratch. Focused `cargo test --test linker linker_borrows_arena_keys_and_reuses_qualified_lookup_storage -- --nocapture`: exit 0, 1 passed / 0 failed / 97 filtered. Full `cargo test --test linker`: exit 0, 98 passed / 0 failed. Strict Clippy exit 0.'
- subtask_ref: scratch-qualified-lookup-buffers
  title: Replace format!("{scope}.{name}") heap allocations in semantic_context.rs (lines 311, 840, 1382, 1492, 1584), contracts.rs (lines 361, 409), and delta.rs (lines 199-204, 243-248) with scratch buffers and prefix checks
  status: completed
  evidence: Lexical and member lookup sites in semantic_context and contracts reuse 128-byte String scratch through super::qualified; resolution imported and class-field lookups reuse the file scratch. Delta top-level checks use exact prefix/dot strips. Focused seam exit 0, 1 passed / 0 failed; full linker target exit 0, 98 passed / 0 failed; `cargo clippy --all-targets --all-features -- -D warnings` exit 0.
- subtask_ref: inline-binding-list-and-cow-coverage-evidence
  title: Use borrowed &'a str keys and inline BindingList<'a> (One | Many) in value_flow::Scope::bindings (lines 217-223) and Cow<'static, str> for static evidence in compact_graph::Coverage (line 142)
  status: completed
  evidence: Scope keys borrow typed facts or original node JSON; One stores a single Binding inline and Many promotion retains order and mutation indexing. Compact evidence is Cow<static,str>, diagnostic literals borrow, public conversion uses into_owned. Mechanical Cow consumers in framework_relations, dictionary, ingest, and writer fixture are converted. Focused seam exit 0, 1 passed / 0 failed, including parameter shadow evidence and single/multiple receiver write cases; full linker target exit 0, 98 passed / 0 failed; strict Clippy exit 0.
receipt:
  commit: e4c22c8ebf21a5a9f37d4dd5103b766c51cf3045
  contract_revision: 1
  passed_at: 2026-10-14T17:46:36.913022270+00:00
  evidence:
    command_proof:
      command: cargo test --test linker linker_borrows_arena_keys_and_reuses_qualified_lookup_storage -- --nocapture
      exit_code: 0
      tests_passed: 1
      tests_failed: 0
      log: 'Final focused seam: exit 0; 1 passed, 0 failed, 97 filtered. Full affected-target gate: cargo test --test linker; exit 0; 98 passed, 0 failed, 0 filtered; finished in 2.81s. Rust gate: cargo clippy --all-targets --all-features -- -D warnings; exit 0; no diagnostics; finished in 18.23s; contributes no tests. Standalone rustfmt --edition 2021 over changed Rust source/fixture files and git diff --check exit 0. All three subtasks are completed with evidence. No Cargo dependencies added. No benchmarks run, so whole-repository latency/peak-allocation invariant is not measured here. Implementation preserves synthetic missing-owner file_module fallback with inferred borrowed scope-key lifetime; actual owner scopes and alias names borrow Facts. Accepted contract red runtime seam evidence is blackboard 141 (1 failed/97 filtered, exit 101). The first build compile attempt found two Cow consumers and the first Clippy found the fixture/type alias/extra borrow; all were repaired within Forge-approved scope expansions before the final green commands.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate ce9e7727898c5e11c1d6fec0a08baa07c93b2c08 vs baseline cac453e3b62cccb5796a290a9a9ad2bd92563518 has exactly14 files in admitted scope, including approved Cow consumer extensions. All candidate-scoped files match current worktree (scoped git diff empty). Each implementation change serves borrowed maps, qualified buffers, BindingList or Cow acceptance; indexing.rs is unchanged but included by scoped snapshot. Only other worktree difference is previously known Forge milestone-start status/started_at/YAML frontmatter normalization. No dependency/config/credential/neighbor changes.
        claims:
          applicable: true
          evidence: 'All five concrete acceptance items verified in actual candidate diff/source: borrowed alias/external/type-only names and owner scopes preserve missing-owner file_module fallback; imported implementation/class-field candidates use qualified scratch; semantic lexical/import/member and contract lookup sites reuse128-byte buffers; delta prefix+dot strips equal previous exact qualification checks. BindingList One->Many preserves order, slice mutation and replay event indices; fallback names borrow original successfully decoded JSON, typed names borrow FlowState. Compact Coverage Cow borrows static diagnostics; public into_owned and dictionary/ingest as_ref preserve text and persistence keys. Core leaf imports unchanged. Named seam exercises actual AST+linker; original intended red alias1/param1/scope4 budgets; submitted final focused1/0, complete linker98/0, strict all-targets/all-features Clippy exit0. Docs truthfully limit allocation measurements. Whole-repository cold11.5s/delta50ms/peak heap invariant remains unmeasured pending authorized benchmark verification, explicitly not certified by this review; limitation recorded in blackboard142.'
        concurrency:
          applicable: true
          evidence: Maps/scratch remain local to existing per-file parallel resolution closure or resolver invocation; Facts/typed arenas remain immutable, lifetime-checked borrows. Inline history promotion retains ordered writes and unchanged epoch/replay logic. No shared mutable cache, locks, transactions, revision fencing or publication path change. Test allocator delegates System with const thread-local counter and RAII reset; private one-thread Rayon pool excludes other test threads. Submitted full linker98/0.
        project_isolation:
          applicable: true
          evidence: Borrowing preserves string key content and all existing path/workspace filters. Delta exact prefix-dot-name strips preserve prior comparison including empty/Unicode components. Cow adapters preserve per-owner dictionary and persistence namespaces. No repository-root/path admission, task identifiers, linked-workspace profiles, filesystem, symlink or traversal handling changes.
        administration:
          applicable: false
          evidence: No migration/schema/profile pin/lifecycle/credentials/privileged delivery change. Extensions admitted by Forge current scope. Current HEAD equals accepted baseline cac453e3b62cccb5796a290a9a9ad2bd92563518; candidate scoped tree unchanged. Hooks, landed SHA and durable receipt remain governed by unchanged deliver gate; delivery not performed by this review.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
    - 'INV-UNCERTAINTY: Candidate evidence (possible_calls, confidence=possible) never becomes an exact calls edge on concrete trait implementors or certifies removal safety; overflow beyond K=8 widens to Unknown/Incomplete while retaining analysis_gaps.'
    - 'INV-ZERO-ALLOC-LOOKUP: Qualified symbol lookups across resolution, semantic_context, contracts, and delta reuse scratch String buffers or prefix strips instead of per-iteration format! heap allocations, with zero new Cargo dependencies.'
    - 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-ZERO-ALLOC-LOOKUP: Qualified symbol lookups across resolution, semantic_context, contracts, and delta reuse scratch String buffers or prefix strips instead of per-iteration format! heap allocations, with zero new Cargo dependencies.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. In `src/engine/linker/resolution.rs` (lines 274–276), replace owned `String` keys in per-file import maps with borrowed `&'a str` slices tied to the `Facts` arena:
   ```rust
   let mut aliases: HashMap<&'a str, HashMap<&'a str, Vec<&'a Node>>> = HashMap::new();
   let mut external_aliases: HashMap<&'a str, HashMap<&'a str, Option<(String, usize)>>> = HashMap::new();
   let mut type_only_aliases: HashMap<&'a str, HashSet<&'a str>> = HashMap::new();
   ```
   and at line 2110 use `qualified(&mut lookup_key, &n.qualname, rest)` instead of `format!("{}.{}", n.qualname, rest)`.
2. In `src/engine/linker/semantic_context.rs` (`unshadowed_global` line 311, `member` line 840, `resolve_symbol` lines 1382, 1492, 1584) and `src/engine/linker/contracts.rs` (`local` line 361, `builtin_unshadowed` line 409), replace `format!("{scope}.{name}")` with a stack-allocated `String::with_capacity(128)` scratch buffer reused across the loop via `super::qualified(&mut key, scope, name)`.
3. In `src/engine/linker/value_flow.rs` (lines 217–223, 372, 399), replace `bindings: HashMap<String, Vec<Binding<'a>>>` with borrowed `&'a str` keys and a zero-dependency inline enum:
   ```rust
   enum BindingList<'a> {
       One(Binding<'a>),
       Many(Vec<Binding<'a>>),
   }
   ```
4. In `src/core/models/compact_graph.rs` (line 142), change `Coverage.evidence` to `Cow<'static, str>` so all literal diagnostic messages use `Cow::Borrowed` with zero heap allocation.
5. In `src/db/delta.rs` (lines 199–204 and 243–248), replace `qual == format!("{prefix}.{name}")` with `qual.strip_prefix(prefix).and_then(|s| s.strip_prefix('.')) == Some(name)`.

---

### task: explicit-unwrap-and-polymorphic-dispatch-edges

```yaml
task_ref: explicit-unwrap-and-polymorphic-dispatch-edges
target: Unwrap Result/Option on explicit ?/unwrap/expect in Rust, Promise<T> on explicit await for calls/parameters/aliases in TS/JS, and Optional[T]/Union[T, None] / PEP 604 binary union expressions (T | None) in Python value flow with fail-closed multi-type unions while preserving negative cases, emit bounded possible_calls (K<=8) for dyn Trait receivers, and extract macro_rules! body references
proof_policy: seam-test-first
contract_revision: 6
scope:
- src/core/semantic.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/value_flow.rs
- src/engine/linker/resolution.rs
- src/engine/languages/rust.rs
- src/engine/languages/rust/linker.rs
- src/engine/languages/rust/value_flow.rs
- src/engine/languages/typescript/value_flow.rs
- src/engine/languages/python.rs
- src/engine/languages/python/value_flow.rs
- docs/architecture/indexing.md
- docs/reference/languages.md
- tests/linker/method_semantics.rs
- tests/receivers/rust_value_flow.rs
- tests/languages/typescript/grammar.rs
- tests/query_context/symbol_context.rs
depends_on:
- zero-alloc-linker-and-value-flow-buffers
invariants:
- 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
- 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
- 'INV-UNCERTAINTY: Candidate evidence (possible_calls, confidence=possible) never becomes an exact calls edge on concrete trait implementors or certifies removal safety; overflow beyond K=8 widens to Unknown/Incomplete while retaining analysis_gaps.'
- 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage; cross-file dirtying of callers on addition/removal of concrete trait implementors is governed and delivered by Task 3.'
status: completed
subtasks:
- subtask_ref: explicit-rust-try-unwrap-and-ts-await-promise
  title: Unwrap Result<T, E>/Option<T> in Rust on explicit try_expression (?) or .unwrap()/.expect(), and unwrap Promise<T> in TS/JS on explicit await_expression for calls, parameters, and local aliases while preserving un-awaited unresolved cases
  status: completed
  evidence: Completed against frozen candidate e41826d. Production-path linker regressions cover Rust explicit ?, unwrap/expect payloads and TS/JS awaited Promise payloads, params, and aliases; `cargo test --lib` 44/44, `cargo test --test linker` 101/101, and `cargo test --test languages` 253/253 passed; strict `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: trait-and-interface-bounded-possible-calls
  title: In src/engine/linker/resolution.rs (lines 846-880), link dyn Trait calls to Trait::method (inferred) and emit bounded possible_calls edges (K<=8) to indexed implementors, widening overflow to Unknown/Incomplete while retaining trait_object_dispatch in analysis_gaps
  status: completed
  evidence: Completed against frozen candidate e41826d. Linker tests cover bounded trait dispatch and K<=8/overflow behavior; persisted query-context test verifies scoped possible_calls and analysis-gap output. `cargo test --test linker` 101/101 and `cargo test --test query_context` 42/42 passed; strict Clippy passed. Cross-file dirtying when implementors change remains Task 3-owned under INV-DELTA-PARITY.
- subtask_ref: rust-macro-rules-body-references
  title: Extract identifier call/type references from macro_definition transcription bodies in src/engine/languages/rust.rs as references edges owned by the macro node while retaining macro_expansion_not_analyzed
  status: completed
  evidence: Completed against frozen candidate e41826d. Macro transcription reference/invocation-gap regression is in the linker integration suite (`tests/linker/method_semantics.rs`); `cargo test --test linker` 101/101 and `cargo test --test query_context` 42/42 passed; strict Clippy passed. Contract rev5 criterion is identifiers followed by `(` or `::`; implementation also handles `{`.
- subtask_ref: python-optional-union-unwrap
  title: Unwrap one non-null payload from already extracted Optional[T] and Union[T, None] in linker value flow and semantic context, leaving multi-type unions unresolved
  status: completed
  evidence: 'Completed against frozen candidate e41826d. Focused persisted nullable readback seam passed 1/1, including fail-closed multi-nonnull Union cases; `cargo test --test linker` 101/101 and `cargo test --lib` 44/44 passed; `cargo test --test languages` 253/253 and strict Clippy passed. Blocker #163 was resolved by the narrow canonical Optional/Union guard.'
- subtask_ref: python-pep604-binary-union-unwrap
  title: Parse PEP 604 binary union expressions (T | None) in src/engine/languages/python/value_flow.rs and unwrap single non-null payload via linker value flow
  status: completed
  evidence: 'Implemented in src/engine/languages/python/value_flow.rs, src/engine/linker/value_flow.rs, and semantic_context.rs with bracket-depth-aware splitting, parenthesized expression handling, Symbol::Unknown fallback unwrapping, and fail-closed multi-nonnull / NoneType rejection. Verified by tests/linker/method_semantics.rs (102/102 passed) and strict Clippy clean.'
receipt:
  contract_revision: 6
  passed_at: 2026-10-15T12:20:00.000000000+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test linker
      exit_code: 0
      tests_passed: 102
      tests_failed: 0
      log: |-
        Exact registered command passed after submission validation, with CARGO_TARGET_DIR=/tmp/m047-task2-isolated-target supplied through the process environment.
        Focused Python nullable seam: cargo test --test linker method_semantics::python_optional_and_single_none_union_project_one_payload_on_index_readback -- --exact --nocapture; exit 0; 1 passed, 0 failed.
        Unfiltered cargo test --test linker; exit 0; 101 passed, 0 failed.
        Unfiltered cargo test --test languages; exit 0; 253 passed, 0 failed.
        Unfiltered cargo test --test query_context; exit 0; 42 passed, 0 failed.
        Focused contract seam: cargo test --lib engine::linker::value_flow::tests::awaited_promise_parameter_alias_resolves_only_explicit_payload -- --exact --nocapture; exit 0; 1 passed, 0 failed.
        Unfiltered cargo test --lib; exit 0; 44 passed, 0 failed.
        Strict Clippy: cargo clippy --all-targets --all-features -- -D warnings; exit 0; no diagnostics.
        cargo fmt --all and cargo fmt --all -- --check passed; git diff HEAD --check is clean.
        cargo test tests/receivers/rust_value_flow.rs ran zero tests because Cargo treated the path as a name filter; no proof credit. cargo test --test receivers is not a registered target. The complete linker target passed and contains the receiver tests.
        Forge delta refreshed 11 modified source, test, and documentation paths; generation bc3ab37b83c9744a4010c82fdbbfef0a15757d68a42a46215add7044696b3b18.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Independently checked frozen candidate e41826dba9a3cb10b014f8b9205e003e6a811ec7 against its baseline and task scope: candidate file blobs match the worktree snapshot; all candidate paths are admitted task paths. No candidate changes touch src/db/delta.rs or task/profile administration; Task 3 retains implementor-delta dirtying.'
        claims:
          applicable: true
          evidence: 'Candidate inspection and persisted linker integration tests cover Rust explicit ?/.unwrap/.expect payload flow and bounded dyn-trait dispatch, TypeScript awaited Promise payload/parameters and aliases, fail-closed Python Optional/Union parsing, and macro transcription references while leaving invocation expansion opaque. Blocker #163’s multi-nonnull Union cases are unresolved in the focused nullable readback regression; focused test passed 1/1 and the full linker target passed 101/101. No cross-file trait-implementor dirtying claim is attributed to Task 2.'
        concurrency:
          applicable: true
          evidence: 'Reviewed new indexing facts as extraction-scoped and bounded; trait implementation candidates are sorted/deduplicated and overflow beyond the bound fails closed. Strict Clippy and the full required affected targets passed: cargo clippy --all-targets --all-features -- -D warnings; cargo test --lib (44/44); cargo test --test linker (101/101); cargo test --test languages (253/253); cargo test --test query_context (42/42).'
        project_isolation:
          applicable: true
          evidence: The candidate stays within the claimed task worktree and scoped paths. The linker coverage exercises persisted project-local resolution and fail-closed ambiguity; complete linker, language, and query-context targets passed. No linked-workspace configuration or project-boundary relaxation is in this candidate.
        administration:
          applicable: false
          evidence: No task administration, profile/schema, migration, credential, delivery receipt, or lifecycle implementation path is changed by the candidate. This submission only records the authorized review decision; delivery remains outside this review action.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
    - 'INV-UNCERTAINTY: Candidate evidence (possible_calls, confidence=possible) never becomes an exact calls edge on concrete trait implementors or certifies removal safety; overflow beyond K=8 widens to Unknown/Incomplete while retaining analysis_gaps.'
    - 'INV-ZERO-ALLOC-LOOKUP: Qualified symbol lookups across resolution, semantic_context, contracts, and delta reuse scratch String buffers or prefix strips instead of per-iteration format! heap allocations, with zero new Cargo dependencies.'
    - 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-UNCERTAINTY: Candidate evidence (possible_calls, confidence=possible) never becomes an exact calls edge on concrete trait implementors or certifies removal safety; overflow beyond K=8 widens to Unknown/Incomplete while retaining analysis_gaps.'
    - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage; cross-file dirtying of callers on addition/removal of concrete trait implementors is governed and delivered by Task 3.'
    architectural_notes:
    - 'decisions: Blocker #163 (Python Optional/Union legacy fallback) is resolved by the narrow canonical Optional/Union guard that prevents legacy typed-parameter fallback from reclassifying malformed multi-nonnull unions. Evidence: focused persisted nullable readback seam passed 1/1; cargo test --test linker passed 101/101; cargo test --lib passed 44/44; cargo clippy --all-targets --all-features -- -D warnings passed.'
    - 'deferred: INV-DELTA-PARITY cross-file dirtying of callers when concrete trait implementors are added or removed remains owned by Task 3 (src/db/delta.rs). Task 2 does not claim or close that work.'
    - 'decisions: Macro acceptance in contract rev5 (milestone line 265) requires scanning macro transcription identifier tokens followed by `(` or `::`; implementation additionally handles `{` for struct initializers. Broader bare annotation or generic forms such as `Foo<Bar>` are outside this contract and would require a future admitted task.'
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. **Explicit Rust `?` / `.unwrap()` / `.expect()` Unwrapping (preserving `tests/receivers/rust_value_flow.rs:521, 1192`)**:
   - Direct bindings `x: Option<T>` or `x: Result<T, E>` without `?`/`.unwrap()`/`.expect()` remain `Builtin("Option")` / `Builtin("Result")` so `x.is_some()` / `x.is_ok()` resolve as `"external"` and `x.work()` stays `"unresolved"`.
   - In `src/engine/languages/rust.rs` (`call_result_hint`), encode syntax-derived `ValueExpr::Project` expressions into `ReceiverHint::CallResult { callee: format!("value:{json}"), member }` without modifying `src/core/models.rs`.
   - In `src/engine/languages/rust/value_flow.rs` (`value`), `try_expression` and `.unwrap()` / `.expect(...)` calls emit `ValueExpr::Project`. In `src/engine/linker/value_flow.rs`, deserialize `value:` receiver hints within semantic limits and evaluate the projected expression, unwrapping `try_expression` (`expr?`) or `.unwrap()` / `.expect(...)` calls on `Result<T, E>` and `Option<T>` (including 1-arg `anyhow::Result<T>`) to the inner type `T` (`TypeTarget::Local(T)`).
2. **Explicit TypeScript / JavaScript `await Promise<T>` Unwrapping (preserving `tests/languages/typescript/grammar.rs:461`)**:
   - In `src/engine/languages/typescript/value_flow.rs` (`extract_with_return`), when a function is `async` (not a generator), keep `return_type: Some(TypeExpr::Unknown)` (so un-awaited calls `const client = make(); client.work()` remain unresolved), and extract `async_return_type`: if the return annotation is `Applied { base: "Promise", args: [inner] }`, set `async_return_type = Some(inner)`; if the annotation is `Named` or inferred from `return new Worker()`, set `async_return_type` to that unwrapped return type.
   - In `src/engine/languages/typescript/value_flow.rs` (`value`), emit `ValueExpr::Await { value: Box::new(value(expression, source)) }` for non-`fetch` `await_expression` nodes, and in `src/engine/linker/value_flow.rs` populate `index.async_returns` for `"typescript" | "javascript" | "vue"` alongside `"python"`.
   - Unwrapping applies across call sites, parameters, and local aliases:
     - Direct awaited call: `(await make()).work()` unwraps `async_return_type`.
     - Explicit parameter unwrap: `p: Promise<Service>` awaited as `(await p).work()` unwraps the inner `Service` type via `resolve_payload` / `explicit_payload`.
     - Local alias unwrap: `const a = p; const s = await a; s.work()` unwraps the inner `Service` type.
     - Negative invariance: Un-awaited member calls (`p.work()`, `const client = make(); client.work()`, `a.work()`) remain strictly `unresolved`.
3. **Bounded `dyn Trait` Dispatch (preserving `tests/query_context/symbol_context.rs:607`)**:
   - In `src/engine/linker/resolution.rs` (lines 846–880), retain the `Coverage` entry with `expression: "trait_object_dispatch"` and `evidence: "analysis gap: ..."` so `analysis_gaps` counts remain unchanged.
   - When the receiver resolves to a workspace `trait` node `trait_node`, emit an `"inferred"` `calls` edge to the trait method declaration `Trait::method` (never to concrete implementor methods as `calls`), and if $1 \le \text{implementors} \le \text{MAX\_CANDIDATE\_BRANCH\_JOINS}$ ($8$), emit `possible_calls` (`confidence = "possible"`) edges to each concrete implementing method.
   - If implementors exceed 8 ($K > 8$), widen candidate evidence to `Unknown/Incomplete` while retaining the `trait_object_dispatch` analysis gap.
   - Invalidation note: Cross-file dirtying of callers when new trait implementors are added or removed is owned by Task 3 (`src/db/delta.rs`), satisfying milestone-wide `INV-DELTA-PARITY`.
4. **Rust `macro_rules!` Transcription References**:
   - In `src/engine/languages/rust.rs`, inside `macro_definition` nodes, scan right-hand transcription `token_tree` nodes for identifier tokens followed by `(` or `::` and emit `Reference { kind: "references", .. }` owned by the `macro` node, while keeping `macro_expansion_not_analyzed` on invocation sites.
5. **Python `Optional[T]` / `Union[T, None]` Unwrapping in Linker Value Flow**:
   - In `src/engine/linker/value_flow.rs` and `src/engine/linker/semantic_context.rs`, unwrap the single non-null inner type `T` from already-extracted `Optional[T]` and `Union[T, None]` / `Union[None, T]` (`TypeExpr::Applied`) via `resolve_payload` / `explicit_payload` so member calls on `Optional` receivers (`client: Optional[Client]` -> `client.work()`) resolve to `Client::work` while preserving unresolved semantics for unannotated/untyped receivers.
   - Fail-closed union resolution: When a `Union` has two or more non-null types (e.g. `Union[ServiceA, ServiceB]`), resolution is ambiguous and remains `unresolved` (fail-closed, no speculative calls).
   - Python PEP 604 binary union expressions (`T | None`, `None | T`) and forward-ref string forms are parsed in `src/engine/languages/python/value_flow.rs` into `TypeExpr::Applied { base: "Union", args: [...] }` and unwrapped by the linker value flow without requiring `typing.Union` in scope. Multi-nonnull unions, `NoneType`, and ABC dispatch remain unresolved.
   - Seam proof: `tests/linker/method_semantics.rs::python_optional_union_unwrap_resolves_single_payload_and_rejects_multi_type_unions`.

---

### task: fast-contract-hash-and-bounded-delta-frontier

```yaml
task_ref: fast-contract-hash-and-bounded-delta-frontier
target: Accelerate incremental delta builds by replacing serde_json::Value + BTreeSet<String> in resolution_identity_changed with streaming deterministic contract hashing, dirtying callers of changed trait implementors, and bounding transitive import_frontier expansion in src/db/delta.rs using indexed Facts with zero source I/O
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/engine/linker/contracts.rs
- src/db/delta.rs
- docs/architecture/indexing.md
- docs/architecture/performance.md
- tests/incremental/
depends_on:
- explicit-unwrap-and-polymorphic-dispatch-edges
invariants:
- 'INV-DELTA-NO-SOURCE-IO: src/db/delta.rs must not perform file I/O (std::fs), string parsing, or regex on source files; all decisions must rely on indexed Facts and Contracts.'
- 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
- 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
status: completed
subtasks:
- subtask_ref: streaming-resolution-identity-digest
  title: Add Contracts::resolution_digest in src/engine/linker/contracts.rs and use exact sorted node digest set equality in src/db/delta.rs (lines 1002-1042) instead of serializing every node to serde_json::Value and BTreeSet<String>
  status: completed
  evidence: 'Strict `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test --lib` 49/49 includes `resolution_digest_is_order_stable_ignores_positions_and_tracks_details`; `cargo test --test incremental` 30/30 includes `resolution_digest_matches_exact_contract_changes`: body-only edit => affected_owners=1, loaded_fact_files=0; First→Second return annotation => affected_owners=2, loaded_fact_files=1; cold parity verified at both states.'
- subtask_ref: universal-reexport-gated-import-frontier
  title: Bound import_frontier in src/db/delta.rs so hop 1 always relinks and hop 2+ expands only from indexed details.exports or value_flow.return_contract, with missing exports and unknown imports fail-safe
  status: completed
  evidence: Strict Clippy passed; `cargo test --lib` 49/49 includes `return_contract_and_known_private_import_bound_the_python_frontier` for positive return-contract, known stop, and conservative missing-contract behavior; `cargo test --test incremental` 30/30 includes `indexed_reexport_frontier_matches_cold_build`, semicolon/continuation import facts, and missing-export fail-safe cold parity.
- subtask_ref: rust-import-frontier-and-trait-invalidation
  title: Include Rust in src/db/delta.rs import_frontier and dirty callers of changed trait implementors guaranteeing identical graph edges to cold builds
  status: completed
  evidence: Strict Clippy passed; `cargo test --test incremental` 30/30 includes `dyn_trait_dispatch_add_change_and_remove_impls_match_cold_builds` for add/change/remove with cold parity; `cargo test --lib` 49/49 includes `added_trait_implementor_refreshes_unchanged_caller_candidates`.
receipt:
  contract_revision: 5
  passed_at: 2026-10-15T04:46:41.897982824+00:00
  evidence:
    command_proof:
      command: cargo test --test incremental resolution_identity::resolution_digest_matches_exact_contract_changes -- --exact && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test incremental && cargo test --lib && cargo fmt --all -- --check && git diff --check
      exit_code: 0
      tests_passed: 80
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate 06a14e76fadb0bc9a33b1be3ad1d2671082377b7 matches the worktree snapshot. The six changed paths are src/db/delta.rs, src/engine/linker/contracts.rs, docs/architecture/indexing.md, docs/architecture/performance.md, tests/incremental/scope.rs, and tests/incremental/resolution_identity.rs; all are within the declared scope, with no config, credential, generated, or neighboring-project changes.
        claims:
          applicable: true
          evidence: 'R3 is closed by tests/incremental/resolution_identity.rs::resolution_digest_matches_exact_contract_changes: a body-only edit asserts affected_owners=1 and loaded_fact_files=0; changing build()''s return annotation First→Second asserts affected_owners=2 and loaded_fact_files=1; assert_cold_equivalent runs after both states. Independent `cargo test --test incremental` passed 30/30. Prior findings are closed: src/db/delta.rs gates hop 2+ only on module details.exports or Contracts::details(...).value_flow.return_contract, not details.bindings; `return_contract_and_known_private_import_bound_the_python_frontier` covers positive, known-stop, and conservative helper behavior; `indexed_reexport_frontier_matches_cold_build` covers indexed re-export parity. No production source reads, parsing, or regex occur in the delta frontier; R2/#168 is honored. Digest set equality uses ordered 16-byte hashes. Strict Clippy and `cargo test --lib` (49/49) passed.'
        concurrency:
          applicable: true
          evidence: The resolution digest is a sorted BTreeSet of fixed-width hashes, and candidate/frontier operations are deterministically ordered. No shared mutable concurrent state or task lifecycle implementation was added. Forge fenced this review to claim rev26 and the frozen candidate snapshot; the incremental target passed 30/30.
        project_isolation:
          applicable: true
          evidence: The candidate introduces no project-boundary or linked-workspace configuration changes. The full incremental target passed, including linked_body_edit_preserves_cross_workspace_consumers; delta decisions remain based on the active database's indexed facts.
        administration:
          applicable: false
          evidence: No task administration, profile, schema, migration, credential, or privileged delivery code changed. Delivery is a separate authorized Forge gate.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-CORE-LEAF: src/core imports no module from src/engine, src/db, src/mcp, or src/cli.'
    - 'INV-STATIC: Analyze admitted source facts without executing target code, macros, build scripts, interpreters, or language servers.'
    - 'INV-PRESERVE: Preserve existing exact resolutions, negative cases (un-unwrapped Option/Result member calls and un-awaited async/Promise factories remain unresolved), selectors, response budgets, workspace isolation, and deterministic ordering outside explicitly admitted semantic changes.'
    - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
    - 'INV-UNCERTAINTY: Candidate evidence (possible_calls, confidence=possible) never becomes an exact calls edge on concrete trait implementors or certifies removal safety; overflow beyond K=8 widens to Unknown/Incomplete while retaining analysis_gaps.'
    - 'INV-ZERO-ALLOC-LOOKUP: Qualified symbol lookups across resolution, semantic_context, contracts, and delta reuse scratch String buffers or prefix strips instead of per-iteration format! heap allocations, with zero new Cargo dependencies.'
    - 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
    - 'INV-DELTA-NO-SOURCE-IO: src/db/delta.rs must not perform file I/O (std::fs), string parsing, or regex on source files; all decisions must rely on indexed Facts and Contracts.'
    - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
    - 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
    architectural_notes:
    - 'decisions: M047-T3-R2 authority boundary: The parity failure is admitted, but source-text parsing or file reads in src/db/delta.rs are not. Milestone INV-DELTA-NO-SOURCE-IO and the rev5 Task 3 contract require hop decisions from indexed facts only. Preserve valid Python semicolon/continuation imports through extracted Facts; when import/export kind or source facts are unavailable/ambiguous, expand conservatively for cold/delta parity. Do not reintroduce the review packet''s source parser/read-line approach.'
    - 'decisions: {"id":"M047-T3-R3-closed","message":"The final frozen candidate 06a14e76fadb0bc9a33b1be3ad1d2671082377b7 closes R3. The required tests/incremental/resolution_identity.rs::resolution_digest_matches_exact_contract_changes integration seam proves a body-only edit keeps affected_owners=1 and loaded_fact_files=0, then a First→Second return-contract change yields affected_owners=2 and loaded_fact_files=1; assert_cold_equivalent passes after both states. Prior R1/R2 findings are closed: hop gates use only details.exports or Contracts::details(...).value_flow.return_contract, never details.bindings; the positive return-contract helper seam and indexed_reexport_frontier_matches_cold_build test are present. R2/#168 is honored: delta hop decisions use indexed facts/contracts, with no source parsing, reads, or regex. Independent evidence: strict Clippy passed, cargo test --lib 49/49, cargo test --test incremental 30/30."}'
    - 'deferred: {"id":"M047-T3-performance-measurement-deferred","message":"INV-PERF-NO-REGRESSION timing and peak-allocation claims were not benchmarked in this task because no explicit benchmark authorization was given. Do not claim the 11.5s cold-build, reduced peak allocations, or sub-50ms delta measurements as verified; benchmark evidence remains deferred."}'
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. **Deterministic Exact Contract Digest & Identity (`INV-DELTA-PARITY`)**:
   - In `src/engine/linker/contracts.rs`, add `pub(crate) fn resolution_digest(&self, facts: &Facts) -> std::collections::BTreeSet<[u8; 16]>` (sorted 16-byte SHA-256 digests from the existing sha2 crate over `(node.id, node.kind, node.name, node.qualname, node.path, node.is_test, node.language, node.generated, self.details(node))` for non-component nodes).
   - In `src/db/delta.rs` (`resolution_identity_changed`, lines 1002–1042), replace `serde_json::to_value` and `BTreeSet<String>` with exact set comparison of `old_digests != new_digests`. The identity set is the digest set, not `BTreeSet<String>` from `serde_json::to_string`. `details(node)` may still be JSON. Use existing `sha2` for the 16-byte digest. Do not add a crate and do not XOR the digests together.
2. **Rust Delta Inclusion & Trait Implementor Invalidation**:
   - In `src/db/delta.rs:354-393`, include `"rust"` alongside `"python" | "javascript" | "typescript" | "vue"` in `import_frontier` so Rust imports and dependencies participate in incremental delta tracking.
   - Implement trait implementor invalidation in `src/db/delta.rs`: when an `impl Trait` is added, modified, or removed, mark files containing call sites to `Trait::method` as affected, guaranteeing identical graph edges and Merkle roots to cold builds (`INV-DELTA-PARITY`).
3. **Indexed export frontier (`INV-DELTA-NO-SOURCE-IO`, `INV-DELTA-PARITY`)**:
   - Read hop gates only from indexed module `details.exports` (`ExportBinding.module`, `ExportBinding.star`, already written by `src/engine/ast/mod.rs`) and from `Contracts::details(node)["value_flow"]["return_contract"]`. Do not read source files and do not add `ImportKind` or parser changes.
   - **Hop 1 (direct importers of `changed_resolution`)**: always inserted into `affected`.
   - **Hop 2+ (importers of intermediate `owner`)**: inserted only when an export binding re-exports that upstream provider (`module` names it, or `star` is true) or `return_contract` exposes the provider, including a qualified `namespace.Type`.
   - **Fail-safe**: a missing `details.exports` array or an unresolvable import expands the frontier so the Merkle root still matches a cold build.
   - No new persisted fields are required. If a field is added, it decodes legacy `facts_blob` with `#[serde(default)]`.
4. **Verification Proofs**:
   - `tests/incremental/scope.rs::dyn_trait_dispatch_add_change_and_remove_impls_match_cold_builds` proves that adding, changing, and removing trait implementors matches cold-build Merkle roots and graph edges.
   - `tests/incremental/scope.rs::indexed_reexport_frontier_matches_cold_build` proves hop 1 always relinks and hop 2+ relinks only for an indexed re-export or return contract, including the missing-export fail-safe case.
   - `tests/incremental/resolution_identity.rs::resolution_digest_matches_exact_contract_changes` proves a changed contract changes the digest set and an unchanged contract does not, without a `BTreeSet<String>` of `serde_json::to_string` identities.
