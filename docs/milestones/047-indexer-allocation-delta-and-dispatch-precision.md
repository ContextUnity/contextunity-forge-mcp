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

2. **Explicit Unwrapping for `Result<T, E>?` / `Option<T>?` and `await Promise<T>`, Plus Bounded `dyn Trait` `possible_calls`:**
   - **Why implicit unwrapping in `resolve_applied` is forbidden by existing tests**: `tests/receivers/rust_value_flow.rs:521, 1192` explicitly assert that calling `optional.work()` directly on `optional: Option<Service>` or `result: Result<Service, Error>` stays `"unresolved"` (while `optional.is_some()` and `result.is_ok()` resolve as `"external"` std builtins), because `Option` and `Result` do not implement `Deref`. Similarly, `tests/languages/typescript/grammar.rs:461` and `tests/receivers/rust_value_flow.rs:564` assert that calling an `async` factory *without* `await` (`const client = make(); client.work()`) stays `"unresolved"`.
   - **Explicit Unwrapping Solution**:
     - **Rust `?` (`try_expression`) and `.unwrap()` / `.expect(...)` (`src/engine/languages/rust/value_flow.rs:218`, `src/engine/linker/value_flow.rs`, `src/engine/linker/semantic_context.rs`)**: Keep un-unwrapped `Result<T, E>` and `Option<T>` as `Builtin("Result")` / `Builtin("Option")`, but when `rust/value_flow.rs` extracts a `try_expression` (`make_repo()?`) or `.unwrap()` / `.expect(...)` call, unwrap the inner `T` payload (`args[0]`) from `Result<T, E>`, `anyhow::Result<T>`, or `Option<T>`.
     - **TypeScript / JavaScript `await Promise<T>` (`src/engine/languages/typescript/value_flow.rs:370, 502` & `src/engine/linker/value_flow.rs:542`)**: When a TS/JS function is `async` or returns `Promise<T>`, populate `facts.async_return_type = Some(T)` while keeping `facts.return_type = Some(TypeExpr::Unknown)` (so un-awaited calls stay unresolved), emit `ValueExpr::Await` for `await_expression` in `typescript/value_flow.rs:370`, and enable `index.async_returns` population in `value_flow.rs:542` for TypeScript/JavaScript alongside Python.
   - **Bounded `dyn Trait` / Interface Dispatch (`src/engine/linker/resolution.rs:846–880, 2336–2365`)**:
     - Preserve the `trait_object_dispatch` analysis gap in `graph.coverage` and never emit exact `calls` edges to concrete implementing methods (`tests/query_context/symbol_context.rs:668`).
     - Emit an `"inferred"` `calls` edge to the trait/interface method declaration (`Trait::method`) and emit bounded `possible_calls` (`confidence = "possible"`, up to `MAX_CANDIDATE_BRANCH_JOINS = 8`) to workspace methods that `implements` the trait method.
   - **Rust `macro_rules!` Transcription References (`src/engine/languages/rust.rs`)**:
     - Extract static call/type identifier references inside `macro_definition` transcription bodies as `kind = "references"` owned by the `macro` node while preserving the `macro_expansion_not_analyzed` gap on invocation sites (`tests/query_context/symbol_context.rs:701`).

3. **Streaming Resolution-Identity Digest & Re-Export Gated Delta Frontier (`src/db/delta.rs`):**
   - Replace the JSON-serialization + `BTreeSet<String>` allocation in `resolution_identity_changed` (`src/db/delta.rs:985–1025`) with a deterministic 64-bit order-independent hash over `Contracts::details(node)`.
   - Bound `while !import_frontier.is_empty()` (`src/db/delta.rs:356–376`) so 1-hop direct importers of `changed_resolution` are always included in `affected`, while 2+ hop transitive importers are only added when the intermediate module actually re-exports symbols or exposes the provider in its own resolution contract, maintaining 100% bit-identical Merkle roots (`assert_cold_equivalent`).

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
target: "Unwrap Result/Option on explicit ?/unwrap/expect in Rust and Promise<T> on explicit await in TS/JS while preserving un-unwrapped negative cases, emit bounded possible_calls for dyn Trait receivers, and extract macro_rules! body references"
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/core/semantic.rs
  - src/engine/linker/semantic_context.rs
  - src/engine/linker/value_flow.rs
  - src/engine/linker/resolution.rs
  - src/engine/languages/rust.rs
  - src/engine/languages/rust/linker.rs
  - src/engine/languages/rust/value_flow.rs
  - src/engine/languages/typescript/value_flow.rs
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
  - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
status: planned
subtasks:
  - subtask_ref: explicit-rust-try-unwrap-and-ts-await-promise
    title: "Unwrap Result<T, E>/Option<T> in Rust only on explicit try_expression (?) or .unwrap()/.expect(), and unwrap Promise<T> in TS/JS only on explicit await_expression via async_return_type"
    status: pending
  - subtask_ref: trait-and-interface-bounded-possible-calls
    title: "In src/engine/linker/resolution.rs (lines 846-880), link dyn Trait calls to Trait::method (inferred) and emit bounded possible_calls edges (K<=8) to indexed implementors while retaining trait_object_dispatch in analysis_gaps"
    status: pending
  - subtask_ref: rust-macro-rules-body-references
    title: "Extract identifier call/type references from macro_definition transcription bodies in src/engine/languages/rust.rs as references edges owned by the macro node while retaining macro_expansion_not_analyzed"
    status: pending
```

#### Acceptance

1. **Explicit Rust `?` / `.unwrap()` / `.expect()` Unwrapping (preserving `tests/receivers/rust_value_flow.rs:521, 1192`)**:
   - Direct bindings `x: Option<T>` or `x: Result<T, E>` without `?`/`.unwrap()`/`.expect()` remain `Builtin("Option")` / `Builtin("Result")` so `x.is_some()` / `x.is_ok()` resolve as `"external"` and `x.work()` stays `"unresolved"`.
   - In `src/engine/languages/rust/value_flow.rs` (`value`, line 218) and `src/engine/linker/value_flow.rs`, when an initializer or receiver is a `try_expression` (`expr?`) or a `.unwrap()` / `.expect(...)` call on a function/variable whose `TypeExpr` is `Applied { base: "Result" | "Option", args }` (including 1-arg `anyhow::Result<T>`), resolve `args[0]` to the inner type `T` (`TypeTarget::Local(T)`).
2. **Explicit TypeScript / JavaScript `await Promise<T>` Unwrapping (preserving `tests/languages/typescript/grammar.rs:461`)**:
   - In `src/engine/languages/typescript/value_flow.rs` (`extract_with_return`, lines 502–515), when a function is `async` (not a generator), keep `return_type: Some(TypeExpr::Unknown)` (so un-awaited calls `const client = make(); client.work()` remain unresolved), and extract `async_return_type`: if the return annotation is `Applied { base: "Promise", args: [inner] }`, set `async_return_type = Some(inner)`; if the annotation is `Named` or inferred from `return new Worker()`, set `async_return_type` to that unwrapped return type.
   - In `src/engine/languages/typescript/value_flow.rs` (`value`, lines 370–387), emit `ValueExpr::Await { value: Box::new(value(expression, source)) }` for non-`fetch` `await_expression` nodes, and in `src/engine/linker/value_flow.rs` (line 542) populate `index.async_returns` for `"typescript" | "javascript" | "vue"` alongside `"python"`.
3. **Bounded `dyn Trait` Dispatch (preserving `tests/query_context/symbol_context.rs:607`)**:
   - In `src/engine/linker/resolution.rs` (lines 846–880), retain the `Coverage` entry with `expression: "trait_object_dispatch"` and `evidence: "analysis gap: ..."` so `analysis_gaps` counts remain unchanged.
   - When the receiver resolves to a workspace `trait` node `trait_node`, emit an `"inferred"` `calls` edge to the trait method declaration `Trait::method` (never to concrete implementor methods as `calls`), and if $1 \le \text{implementors} \le \text{MAX\_CANDIDATE\_BRANCH\_JOINS}$ ($8$), emit `possible_calls` (`confidence = "possible"`) edges to each concrete implementing method.
4. **Rust `macro_rules!` Transcription References**:
   - In `src/engine/languages/rust.rs`, inside `macro_definition` nodes, scan right-hand transcription `token_tree` nodes for identifier tokens followed by `(` or `::` and emit `Reference { kind: "references", .. }` owned by the `macro` node, while keeping `macro_expansion_not_analyzed` on invocation sites.

---

### task: fast-contract-hash-and-bounded-delta-frontier

```yaml
task_ref: fast-contract-hash-and-bounded-delta-frontier
target: "Accelerate incremental delta builds by replacing serde_json::Value + BTreeSet<String> in resolution_identity_changed with streaming 64-bit contract hashing and bounding transitive import_frontier expansion in src/db/delta.rs"
proof_policy: seam-test-first
contract_revision: 1
scope:
  - src/engine/linker/contracts.rs
  - src/db/delta.rs
  - docs/adr/0010-incremental-delta-transitive-dirtying.md
  - docs/architecture/indexing.md
  - docs/architecture/performance.md
  - tests/incremental/resolution_identity.rs
  - tests/incremental/scope.rs
  - tests/commitment_integrity/
depends_on:
  - explicit-unwrap-and-polymorphic-dispatch-edges
invariants:
  - 'INV-DELTA-PARITY: Cold build and incremental delta rebuild over identical admitted inputs produce bit-identical Merkle roots (forge-owner-sha256-v2) and identical graph edges/coverage.'
  - 'INV-PERF-NO-REGRESSION: Cold build on isolated control repository copy (commerce-release-update) completes within 11.5s wall-clock time with reduced peak heap allocations and sub-50ms single-file body-edit delta latency.'
status: planned
subtasks:
  - subtask_ref: streaming-resolution-identity-digest
    title: "Add Contracts::resolution_digest in src/engine/linker/contracts.rs and use it in src/db/delta.rs (lines 985-1025) instead of serializing every node to serde_json::Value and BTreeSet<String>"
    status: pending
  - subtask_ref: reexport-gated-import-frontier
    title: "Bound while !import_frontier.is_empty() in src/db/delta.rs (lines 337-377) so 1-hop importers always relink but 2+ hop importers only expand through modules with re-exports or exported return contracts"
    status: pending
```

#### Acceptance

1. In `src/engine/linker/contracts.rs`, add `pub(crate) fn resolution_digest(&self, facts: &Facts) -> u128` (or order-independent XOR/sum of 128-bit SHA-256 node identity digests over `(node.id, node.kind, node.name, node.qualname, node.path, node.is_test, node.language, node.generated, self.details(node))` for `node.kind != "component"`), and use it in `resolution_identity_changed` (`src/db/delta.rs:985–1025`) to avoid constructing `serde_json::Value` maps and `BTreeSet<String>` for every node.
2. In `src/db/delta.rs` (lines 337–377), refine the `import_frontier` loop:
   - **Hop 1 (direct importers of `changed_resolution`)**: always inserted into `affected`.
   - **Hop 2+ (importers of `owner`)**: only added to `next` frontier if `owner` may transitively propagate symbols from `changed_resolution` (i.e., `owner` is in `changed_resolution`, or `owner` contains wildcard imports, `exports`, `__all__`, `reexport`, or callable `return_contract` / `alias_type` facts), preventing repo-wide hydration when a leaf module changes while preserving 100% parity on `cargo test --test incremental` and `cargo test --test commitment_integrity`.
