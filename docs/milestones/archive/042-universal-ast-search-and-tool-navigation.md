---
id: m-universal-ast-search-and-tool-navigation
title: Universal AST search and path-scoped code navigation
doc_type: contract
status: completed
depends_on: []
owners:
- src/engine/ast/
- src/engine/languages/
- src/cli/ast.rs
- src/cli/mod.rs
- src/cli/guide.rs
- src/mcp/
- src/db/reader/selectors.rs
- src/db/reader.rs
- src/db/symbols.rs
- src/db/traversal.rs
- docs/reference/
- tests/
- benchmarks/
invariants:
- 'INV-PROFILE-AST-PATTERN: Shared pattern search has no language-name dispatch; grammar-specific capabilities (complete syntax, declaration body kinds across syntax forms, fragment probes for root-rejected expressions and macros, attributes) are provided through the registered LanguageProfile contract.'
- 'INV-PROBE-WRAPPER-ISOLATION: Removing a synthetic probe wrapper must affect only the probe container node and unwrap to the target AST node; it must not change match semantics of the user pattern or strip nodes of legitimate declaration patterns.'
- 'INV-SOUND-AST-PREFILTER: Candidate narrowing may reject a file only when every required pattern token is represented by the selected index; body-only identifiers and literals rely on sound complete AST candidate fallback and cannot become false negatives.'
- 'INV-SELECTOR-SCOPE: An explicit path constrains candidates before ambiguity resolution and never changes workspace or project boundaries.'
- 'INV-FAIL-CLOSED-AMBIGUITY: Ambiguous selectors return a structured machine-readable JSON outcome with bounded candidates and never choose a target implicitly.'
- 'INV-BOUNDED-NAVIGATION: Search, preview, ambiguity, and continuation outputs stay within the 64 KiB response ceiling; performance values are recommendations unless admitted by the owning milestone.'
- 'INV-NO-SOURCE-MIRROR: Signatures are projected strictly from existing nodes.details without reading source or mutating database schema/projections; previews use the bounded source reader, and indexed storage never gains copied source bodies.'
- 'INV-SOUND-AST-FALLBACK: Function-body token indexing is out of scope (evaluated and rejected in milestone 030 under density limits); pattern search relies on the existing sound bounded AST candidate fallback and never adds a parallel FTS table or drops valid body-literal matches.'
- 'INV-REJECT-UNKNOWN-ARGUMENTS: Unrecognized request fields are rejected before query execution with rejected_arguments validation, preventing malformed or misspelled path arguments from widening search scope.'
- 'INV-PARITY-AND-COORDINATION: Extend completed milestone 050 navigation ergonomics (promoting ambiguous text errors to machine-readable JSON envelopes and consolidating inspect_selector); keep CLI and MCP semantics equivalent across path parameters and depth defaults.'
related_plans:
- docs/plans/042-universal-ast-search-and-tool-navigation.md
- docs/milestones/archive/030-tool-performance-and-storage-compaction.md
- docs/milestones/archive/050-mcp-output-compaction-and-agent-ergonomics.md
started_at: 2026-10-07T22:43:00+00:00
handoff:
  completed_at: 2026-10-08T00:52:32.451630593+00:00
  duration: 2h 9m
  commit: 33aac23573a2e3f48ef5c299b281b86e12373318
  verification:
    command: cargo test --all-targets --quiet
    status: passed
    tests_passed: 640
    tests_failed: 0
---

# Universal AST search and path-scoped code navigation

## Outcome and purpose

Define a language-profile-driven AST search contract and path-aware, bounded MCP and CLI navigation so queries work consistently across current and future language profiles without duplicating parser wrappers or losing source matches to an unsound text prefilter.

## Source plan notes

>
> # Universal AST search and path-scoped code navigation
>
> Keep grammar-specific fragment handling behind the extensible `LanguageProfile`
> capability contract and the shared `src/engine/ast` search pipeline. Carry a caller's
> path through every selector tool before ambiguity resolution, bound structured
> ambiguity and preview responses, and keep search output useful enough to guide a
> precise follow-up call. Reuse the existing indexed symbol metadata (`nodes.details`),
> source access, and the sound AST fallback proven in milestone 030; do not add a
> raw-source mirror or a second language-specific search implementation.
>
> Navigation contracts build directly upon the completed text baseline in milestone 050,
> promoting text-based ambiguity guidance in `tests/mcp_context.rs` to machine-readable JSON
> envelopes and standardizing on `inspect_selector`. CLI commands mirror MCP options and
> defaults for complete parity.
>

## Architecture and compatibility boundary

### AST pattern flow

`ast_grep_search` and the matching CLI command enter the shared search pipeline in
`src/engine/ast`. The pipeline obtains eligible paths through the existing file
and symbol indexes, asks the registered `LanguageProfile` to prepare the pattern,
parses it with that profile's Tree-sitter grammar, and verifies structural matches
against digest-checked source. Pattern preparation returns a typed result that
distinguishes complete syntax, supported fragments, and invalid syntax.

Rather than cataloging disparate syntax examples or hardcoding language branches in
the shared engine, the contract models AST pattern support through four orthogonal
`LanguageProfile` capabilities:
1. **`complete_syntax`**: Available out-of-the-box for all profiles. Any standalone
   construct that forms a valid, self-contained syntax tree at grammar root without
   synthetic modification (e.g. whole item/type declarations, statements and expressions
   in languages that admit them at file root like Python and TypeScript, or template elements
   like `<div id="$ID">$$$CHILDREN</div>` in HTML). Framework calls in languages with root
   statement support (e.g. `render(...)`, `defineNuxtComponent(...)`) parse directly under
   this capability without dedicated framework branches or milestone 041 dependencies.
   A newly registered profile with only `complete_syntax` is fully valid and functional.
2. **`declaration_without_body`**: The profile advertises its set of body node kinds across
   supported declaration forms (for Rust: `block` for functions/methods, `field_declaration_list`
   for structs, `enum_variant_list` for enums, `declaration_list` for impls; for Python: `block`
   for functions, async functions, and classes; for TypeScript: `statement_block` for functions/methods,
   `class_body` for classes, `interface_body` for interfaces). The shared `search_range` matcher filters
   children against the profile-declared body kinds rather than hardcoding a single `block` kind,
   enabling body-omitted and wildcard declarations (`fn $NAME($$$ARGS) {}`, `def $NAME($$$ARGS):`,
   `function $NAME($$$ARGS) {}`, `async def $NAME($$$ARGS):`, `pub struct $NAME { $$$FIELDS }`,
   `enum $NAME { $$$VARIANTS }`, `impl $TRAIT for $TYPE { $$$BODY }`, `interface $NAME { $$$MEMBERS }`,
   `class $NAME { $$$MEMBERS }`).
3. **`fragment_probe`**: The profile provides a synthetic probe container and an explicit
   unwrap path to the target AST node for isolated expressions, calls, macros, or assignments
   that grammar parsers reject at file root (for example, in Rust, naked instance calls
   `client.fetch($ARG)`, static calls `Type::method($ARG)`, and macro calls `vec![$ARG]` are
   invalid at top level and require a probe container like a synthetic function). Removing a
   probe wrapper must affect only the synthetic probe container node and unwrap to the target
   AST node (`INV-PROBE-WRAPPER-ISOLATION`); it must not alter source match semantics of the
   inner pattern or strip nodes of legitimate user declaration patterns.
4. **`attribute`**: The profile advertises its decoration and annotation syntax
   (`#[derive($$$TRAITS)]`, `#[test]`, `@$DEC\ndef $NAME($$$ARGS):`, `@$DEC\nclass $NAME:`,
   `@$DEC class $NAME {}`, `class $NAME { @$DEC $METHOD($$$ARGS) {} }`)
   layered over the shared declaration matcher.

A syntax failure or query with an undeclared fragment form reports a structured
diagnostic containing the profile language, the exact nonempty input span of the
Tree-sitter `ERROR`, an actionable hint, and valid syntax examples from that profile
(replacing opaque plain-text rejections or misleading empty results). Tool schema
descriptions for `ast_grep_search` and `forge_guide("ast")` explicitly document
these supported profile capabilities and wildcard conventions.

Candidate filtering is sound for the text represented in `node_search`. Function-body
token indexing was evaluated in milestone 030 (`function-body-search-tokens`) and
closed as no-go due to strict storage density headroom limits (~0.277 KiB/file reserve).
Milestone 042 introduces no parallel full-text table, no raw source mirror, and no
unbounded fallback scan. General AST pattern searches rely on the existing sound
bounded AST candidate fallback, as verified by `ast_grep_prefilter_preserves_matches_for_body_only_literals`,
and preserve body-only literal matches without false negatives.

### Selector and search-result flow

Selector input carries an optional workspace-relative `path` to
`src/db/reader/selectors.rs::select_detail` before candidate cardinality is evaluated.
The selector grammar preserves exact IDs, qualified names, `path:name`, `path::name`,
`path:line`, and `path#Lline`. It adds `path:kind:name` only when the middle token
is a registered node kind; `#` remains reserved for line coordinates. Existing
workspace and linked-project guards remain authoritative.

CLI query commands (`inspect`, `explain`, `impact`, `tests`, `remove` in `src/cli/mod.rs`)
mirror this contract by accepting an optional `--path` argument to maintain complete
CLI/MCP parity (`INV-PARITY-AND-COORDINATION`).

Unknown or unrecognized request arguments are rejected before query execution with
a `rejected_arguments` validation error, preventing misspelled parameters (such as
`pah` instead of `path`) from unintentionally broadening a local request to a
workspace-wide scan (`INV-REJECT-UNKNOWN-ARGUMENTS`).

Ambiguity handling evolves the completed milestone 050 text baseline into a structured,
machine-readable `outcome: "ambiguous"` result. It reports `total` and at most eight
candidates with `id`, `path`, `line`, `qualname`, and a character-bounded `signature`
(up to 512 characters). The envelope is a normal bounded JSON result, allowing clients
to narrow the path without parsing long error strings. Existing assertion in
`tests/mcp_context.rs` (`stdio_compact_navigation_preserves_symbol_and_page_contracts`),
which previously asserted `isError: true` with text hints, is evolved to assert this
machine-readable envelope.

Search hits expose their signature directly from the existing `nodes.details` projection
(`signature`, character-bounded to 512 characters) without reading source files from disk
or altering database schema and projections (`INV-NO-SOURCE-MIRROR`). Source previews are
attached only when total hits are at most three or `include_preview: true` is explicitly
requested, capped at no more than three hits, 20 lines per hit, and 12 KiB combined,
fetched strictly through the existing bounded source reader. Each compact hit page
includes the unified `inspect_selector` (established in milestone 050) for ready-to-call
`code_map_inspect` invocations with `show_source: true`.

The default impact depth is one edge in both MCP `code_map_impact` (via a dedicated
`impact_depth` helper) and CLI `query impact` (`default_value_t = 1`), while `code_map_query`
and CLI `query run` preserve their existing `depth: 2` default. An explicitly supplied
depth keeps its current meaning.

### Producers and consumers

| Producer | Current owner | Consumer and observable result |
| --- | --- | --- |
| AST pattern request | `src/mcp/tools.rs`, `src/cli/ast.rs`, `src/engine/ast/` | Registered `LanguageProfile` capability matrix (complete syntax, declaration body kinds, fragment probes, attributes); structural hits for valid fragments and structured span diagnostics for unsupported fragments or errors. |
| Selector plus optional path | `src/mcp/`, `src/cli/mod.rs`, `src/db/reader/selectors.rs`, `src/db/reader.rs` | `select_detail` returns one path-scoped symbol or a machine-readable `outcome: "ambiguous"` envelope without guessing, across both MCP and CLI commands. |
| Search hit metadata and preview | `src/db/symbols.rs`, existing source reader | Search clients receive character-bounded signatures from existing `nodes.details`, previews when total <= 3 or `include_preview: true`, and the unified `inspect_selector`. |
| Body tokens baseline | Milestone 030 no-go outcome | Function-body token indexing is out of scope; AST pattern search relies on sound candidate fallback proven by `ast_grep_prefilter_preserves_matches_for_body_only_literals`. Milestone 042 adds no new index or source mirror. |
| Navigation baseline | Completed milestone 050 | Evolves text ambiguity errors in `tests/mcp_context.rs` into machine-readable JSON envelopes, unifies `inspect_selector`, and sets default impact depth to 1 for MCP and CLI impact. |

Milestone 042 changes grammar and query preparation through `LanguageProfile` capabilities
and the shared AST layer only. Framework call sites are handled as standard expressions,
leaving framework-specific manifest linking independent. Milestone 030 remains the
authoritative owner of storage density and build throughput; no schema, Merkle leaf,
or persisted source format change is admitted here.

## Failure modes and containment

- A valid expression or incomplete declaration can be rejected by a whole-file
  parser: use profile-declared fragment adapters with isolated probe unwrap and report
  exact failure spans when unsupported.
- A function-body literal can be removed by an unsound FTS prefilter: rely on the
  existing sound AST candidate fallback verified by `ast_grep_prefilter_preserves_matches_for_body_only_literals`.
- A dropped or misspelled path argument can turn a local selector into a repository-wide
  ambiguity: fail fast on unknown fields with `rejected_arguments`, pass known paths
  before cardinality checks, and retain the workspace/project boundary across MCP and CLI.
- A large candidate list or preview can exceed the MCP response limit: cap
  candidates (at most 8), 512-character signatures, preview count (at most 3), lines (at most 20),
  and bytes (at most 12 KiB), preserving the 64 KiB protocol ceiling.
- A language-specific adapter can drift from CLI or alter extraction costs:
  share preparation through `LanguageProfile`, verify both public surfaces, and
  keep search-only parsing out of the cold extraction path.

## Proof and performance observations

Use public CLI/MCP and persisted-reader seams. Parser tests are table-driven over
the capability matrix for registered language profiles (Rust, Python, TypeScript/JavaScript,
HTML, and Vue):
- **Rust**: `complete_syntax` (`fn yes() -> bool { true }`), `declaration_without_body` with profile body kinds (`fn $NAME($$$ARGS) {}` [`block`], `pub struct Inspect { $$$FIELDS }` [`field_declaration_list`], `enum $NAME { $$$VARIANTS }` [`enum_variant_list`], `impl $TRAIT for $TYPE { $$$BODY }` [`declaration_list`]), `attribute` (`#[derive($$$TRAITS)] struct $NAME { $$$FIELDS }`, `#[test] fn $NAME($$$ARGS) {}`), `fragment_probe` for root-rejected expressions and macros (`client.fetch($ARG)`, `Type::method($ARG)`, `vec![$ARG]`).
- **Python**: `complete_syntax` for root-valid statements and expressions (`def yes(): return True`, `client.fetch($ARG)`, `$X = $Y`), `declaration_without_body` with `block` (`def $NAME($$$ARGS):`, `async def $NAME($$$ARGS):`, `class $NAME:`), `attribute` (`@$DEC\ndef $NAME($$$ARGS):`, `@$DEC\nclass $NAME:`).
- **TypeScript**: `complete_syntax` for root-valid statements and declarations (`function yes(): boolean { return true; }`, `client.fetch($ARG)`, `let $X = $Y;`, `const $X = $Y;`), `declaration_without_body` with profile body kinds (`function $NAME($$$ARGS) {}` [`statement_block`], `class $NAME { $$$MEMBERS }` [`class_body`], `interface $NAME { $$$MEMBERS }` [`interface_body`]), `attribute` (`@$DEC class $NAME {}`, `class $NAME { @$DEC $METHOD($$$ARGS) {} }`).
- **HTML / Vue**: verifies that profiles with only `complete_syntax` (`<div id="$ID">$$$CHILDREN</div>` for HTML, `<template>$$$NODES</template>` for Vue) succeed without fragment extensions, changes, or language-name branches in `search_range`.
- **Negative diagnostic**: an undeclared fragment form or invalid grammar syntax returns a structured diagnostic carrying profile language, exact nonempty Tree-sitter `ERROR` span, actionable hint, and valid pattern examples.

Include positive structural cases, an exact pattern case, body-only literal verification
(`ast_grep_prefilter_preserves_matches_for_body_only_literals`), same-name symbols under
two paths, rejected unknown arguments, bounded ambiguity envelopes in MCP and CLI,
maximum-size previews, and explicit/default impact depth. Mutating path propagation
to use an unscoped selector must make the two-directory disambiguation assertion fail.

On the fixed Commerce workspace (3,901 files), create one cold index with the
release CLI and a fresh temporary SQLite database. Record process monotonic wall
time, the CLI `elapsed_ms` receipt, indexed file and node counts, and `output_root`.
Keep the database under `/tmp` so the reference workspace remains unchanged. This
is an observational cold-index timing, not a latency acceptance gate. Do not run
the warm AST or MCP comparison benchmark. Milestone 030 owns active build-throughput
gates. Every result, including eight 512-character signatures and maximum previews,
remains within ADR 0011's 64 KiB ceiling.

Run the focused public-seam suites, `cargo test --test commitment_integrity`,
strict Clippy, and `cargo test --all-targets` before handoff.

## Tasks in this milestone

### task: profile-driven-ast-fragment-search

```yaml
task_ref: profile-driven-ast-fragment-search
target: Prepare and search AST patterns through a shared language-profile capability adapter, with sound candidate filtering and diagnosable fragment syntax
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/engine/ast/
- src/engine/languages/
- src/cli/ast.rs
- src/mcp/tools.rs
- tests/ast_extractors.rs
- tests/language_profiles.rs
- tests/html_profile.rs
subtasks:
- subtask_ref: body-literal-prefilter
  title: A Rust function containing the literal "coverage_owner_language" is returned by public ast_grep_search for that string; candidate filtering cannot exclude it because the text occurs only in the function body, maintaining the sound AST fallback proven by ast_grep_prefilter_preserves_matches_for_body_only_literals
  status: completed
  evidence: 'Rust body-only literal production seam passed via tests/language_profiles.rs::public_ast_search_keeps_rust_body_only_literal_candidates. Public MCP route regression passed: `cargo test --test tool_evolution ast_grep_prefilter_preserves_matches_for_body_only_literals -- --nocapture` (1 passed); Server::ast_grep_search delegates to cli::ast::search_paged.'
- subtask_ref: ast-search-latency
  title: On the fixed Commerce workspace (3,901 files), create one cold index with the release CLI and a fresh temporary SQLite database; record process monotonic wall time, CLI elapsed_ms, indexed file and node counts, and output_root, without running a warm AST or MCP comparison benchmark
  status: completed
  evidence: 'Cold release CLI index of the fixed Commerce workspace succeeded in one run using a fresh /tmp database. Process monotonic wall=12.037313s; CLI elapsed_ms=11691.920358; 3,901 files, 65,551 nodes; output_root=ba749789214ea93ba08e5109e7db8276871ba3f3e4f137bea8bcc09d92d20f1b. Command: target/release/contextunity-forge-mcp --root /home/oleksii/ContextUnity/worktrees/commerce-release-update --db /tmp/m042-commerce-cold-index-run1.sqlite build. MCP latency benchmark omitted per user instruction.'
- subtask_ref: profile-capability-matrix
  title: 'Table-driven test proves the capability matrix across profiles: Rust, Python, TypeScript, and HTML/Vue against their declared capabilities (complete_syntax for root-valid definitions and expressions, declaration_without_body with profile-defined body kinds across functions, structs, enums, impls, classes, and interfaces, fragment_probe with isolated unwrap path for root-rejected expressions and macros in Rust, and attribute); newly registered profiles with only complete_syntax succeed without search_range branches'
  status: completed
  evidence: '`cargo test --test ast_extractors ast_search -- --nocapture`: 4 passed; table covers Rust/Python/TypeScript/HTML/Vue, declaration body omission/completion, Rust expression+macro probes, attributes, and default complete-syntax-only TOML profile.'
- subtask_ref: unsupported-fragment-diagnostics
  title: An AST search query with an undeclared fragment form or invalid grammar syntax returns a structured diagnostic containing the profile language, exact nonempty Tree-sitter ERROR span, actionable hint, and valid pattern examples instead of a misleading empty result
  status: completed
  evidence: '`cargo test --test language_profiles public_ast_search_ -- --nocapture` proves the structured diagnostic fields and exact nonempty Tree-sitter ERROR span. `cargo test --test tool_evolution mcp_ -- --nocapture` passed 11/11; invalid MCP AST syntax carries the same JSON diagnostic with isError=true, while valid AST matches remain successful.'
status: completed
receipt:
  commit: 7ece6c439212f315f046444a535bba8d9eac8532
  contract_revision: 5
  passed_at: 2026-10-08T00:35:22.511741731+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets --quiet
      exit_code: 0
      tests_passed: 640
      tests_failed: 0
      log: 'All targets passed: 640 passed, 0 failed, 3 ignored. Two ignored tests are existing opt-in cases; the cold workspace benchmark test requires FORGE_BENCH_ROOT and was not run. Strict cargo clippy --all-targets --all-features -- -D warnings passed. cargo test --test commitment_integrity passed 14/14. Changed Rust files passed rustfmt --check and git diff --check.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed src/mcp/tools.rs::Server::ast_grep_search, which is within task1 scope. The only repair in this slice converts a returned diagnostic value to MCP error status while preserving its JSON; tests and unrelated modules were not changed for this repair.
        claims:
          applicable: true
          evidence: Structured unsupported_ast_pattern fields remain provided by cli::ast::search_paged; MCP boundary suite passes 11/11 and all targets pass 640/640, with strict Clippy clean.
        concurrency:
          applicable: true
          evidence: No concurrent agents were used. The handler change is isolated from Task2 response-envelope changes in src/mcp/response.rs and Task3 search/navigation implementation.
        project_isolation:
          applicable: true
          evidence: Work and verification stayed in the dedicated 042 worktree; test workspaces are temporary. Commerce was read-only for the single authorized cold-index measurement.
        administration:
          applicable: true
          evidence: Task1 was reopened to address the adjacent regression in its declared MCP handler scope, then contract/build/review claims were made under distinct worker IDs. No commit or push was performed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-PROFILE-AST-PATTERN: Shared pattern search has no language-name dispatch; grammar-specific capabilities (complete syntax, declaration body kinds across syntax forms, fragment probes for root-rejected expressions and macros, attributes) are provided through the registered LanguageProfile contract.'
    - 'INV-PROBE-WRAPPER-ISOLATION: Removing a synthetic probe wrapper must affect only the probe container node and unwrap to the target AST node; it must not change match semantics of the user pattern or strip nodes of legitimate declaration patterns.'
    - 'INV-SOUND-AST-PREFILTER: Candidate narrowing may reject a file only when every required pattern token is represented by the selected index; body-only identifiers and literals rely on sound complete AST candidate fallback and cannot become false negatives.'
    - 'INV-SELECTOR-SCOPE: An explicit path constrains candidates before ambiguity resolution and never changes workspace or project boundaries.'
    - 'INV-FAIL-CLOSED-AMBIGUITY: Ambiguous selectors return a structured machine-readable JSON outcome with bounded candidates and never choose a target implicitly.'
    - 'INV-BOUNDED-NAVIGATION: Search, preview, ambiguity, and continuation outputs stay within the 64 KiB response ceiling; performance values are recommendations unless admitted by the owning milestone.'
    - 'INV-NO-SOURCE-MIRROR: Signatures are projected strictly from existing nodes.details without reading source or mutating database schema/projections; previews use the bounded source reader, and indexed storage never gains copied source bodies.'
    - 'INV-SOUND-AST-FALLBACK: Function-body token indexing is out of scope (evaluated and rejected in milestone 030 under density limits); pattern search relies on the existing sound bounded AST candidate fallback and never adds a parallel FTS table or drops valid body-literal matches.'
    - 'INV-REJECT-UNKNOWN-ARGUMENTS: Unrecognized request fields are rejected before query execution with rejected_arguments validation, preventing malformed or misspelled path arguments from widening search scope.'
    - 'INV-PARITY-AND-COORDINATION: Extend completed milestone 050 navigation ergonomics (promoting ambiguous text errors to machine-readable JSON envelopes and consolidating inspect_selector); keep CLI and MCP semantics equivalent across path parameters and depth defaults.'
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

### task: path-scoped-selector-resolution

```yaml
task_ref: path-scoped-selector-resolution
target: Carry explicit path scope through every selector tool before ambiguity resolution and return bounded structured ambiguity without selecting a target
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/mcp/
- src/cli/mod.rs
- src/db/reader/selectors.rs
- src/db/reader.rs
- src/db/symbols.rs
- src/db/traversal.rs
- tests/mcp_context.rs
- tests/mcp_context/
- tests/query_context/
subtasks:
- subtask_ref: selector-path-contract
  title: Public MCP inspect, explain, snippet, impact, tests, and prove_removal requests and CLI inspect, explain, impact, tests, and remove commands accept optional path; path filtering occurs inside select_detail before ambiguity checks, and path:name, path::name, path:known_kind:name, path:line, and path#Lline resolve only within the selected workspace path
  status: completed
  evidence: '`cargo test --test mcp_context -- --nocapture` passed (34/34). Public MCP requests and CLI `query inspect/explain/impact/tests/remove --path` resolved `target` only inside `other.py`; in-band path, kind, line, and hash-line selectors resolved `service.py`.'
- subtask_ref: bounded-ambiguity-envelope
  title: Two same-named symbols in different directories produce a machine-readable JSON outcome=ambiguous with total and at most eight candidates carrying id, path, line, qualname, and character-bounded signature (up to 512 characters); no candidate is selected implicitly and the response remains below 64 KiB, evolving milestone 050 error strings and updating tests/mcp_context.rs into structured envelopes
  status: completed
  evidence: The public stdio MCP test returns outcome=ambiguous with total and bounded candidate records without selecting a target; response now also carries a bounded 'message' containing the stable phrase 'ambiguous selector'. `cargo test --test tool_evolution mcp_ -- --nocapture` passed 11/11, and the directory path ambiguity test passed 1/1.
- subtask_ref: rejected-arguments-and-query-scope
  title: An unrecognized request field is rejected before query execution with rejected_arguments validation, preventing misspelled path parameters from broadening a query; valid path-scoped requests use indexed candidates with no global scans
  status: completed
  evidence: The public stdio MCP test rejected misspelled `pah` for both inspect and search with `error.code=rejected_arguments` before lookup; scoped exact symbol search returned only `other.py`. Full mcp_context suite passed 34/34.
status: completed
receipt:
  commit: 1cab24bee3558ec5972984241437e980ee06fb75
  contract_revision: 4
  passed_at: 2026-10-08T00:35:22.570961495+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets --quiet
      exit_code: 0
      tests_passed: 640
      tests_failed: 0
      log: 'All targets passed: 640 passed, 0 failed, 3 ignored. Two ignored tests are existing opt-in cases; the cold workspace benchmark test requires FORGE_BENCH_ROOT and was not run. Strict cargo clippy --all-targets --all-features -- -D warnings passed. cargo test --test commitment_integrity passed 14/14. Changed Rust files passed rustfmt --check and git diff --check.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed src/db/reader.rs::SelectorError::AmbiguousPaths and src/mcp/response.rs ambiguity serialization; both paths are in Task2 scope. No test source changes were made.
        claims:
          applicable: true
          evidence: The typed ambiguity remains fail-closed with at most eight candidates and bounded fields; compatibility message restores 'ambiguous selector'. Directory slice ambiguity and 11 MCP boundary tests pass; full suite passes 640/640.
        concurrency:
          applicable: true
          evidence: No concurrent agents were used. Task2 repairs are distinct from Task1 AST diagnostic handling and Task3 bounded search navigation, despite sharing the permitted src/mcp directory.
        project_isolation:
          applicable: true
          evidence: All changes and verification ran in the dedicated 042 worktree. The Commerce source tree remained read-only and the cold index database resides in /tmp.
        administration:
          applicable: true
          evidence: Task2 was reopened before modifying its declared files; review worker differs from its repair builder. Strict Clippy and commitment integrity pass; no commit or push was performed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-PROFILE-AST-PATTERN: Shared pattern search has no language-name dispatch; grammar-specific capabilities (complete syntax, declaration body kinds across syntax forms, fragment probes for root-rejected expressions and macros, attributes) are provided through the registered LanguageProfile contract.'
    - 'INV-PROBE-WRAPPER-ISOLATION: Removing a synthetic probe wrapper must affect only the probe container node and unwrap to the target AST node; it must not change match semantics of the user pattern or strip nodes of legitimate declaration patterns.'
    - 'INV-SOUND-AST-PREFILTER: Candidate narrowing may reject a file only when every required pattern token is represented by the selected index; body-only identifiers and literals rely on sound complete AST candidate fallback and cannot become false negatives.'
    - 'INV-SELECTOR-SCOPE: An explicit path constrains candidates before ambiguity resolution and never changes workspace or project boundaries.'
    - 'INV-FAIL-CLOSED-AMBIGUITY: Ambiguous selectors return a structured machine-readable JSON outcome with bounded candidates and never choose a target implicitly.'
    - 'INV-BOUNDED-NAVIGATION: Search, preview, ambiguity, and continuation outputs stay within the 64 KiB response ceiling; performance values are recommendations unless admitted by the owning milestone.'
    - 'INV-NO-SOURCE-MIRROR: Signatures are projected strictly from existing nodes.details without reading source or mutating database schema/projections; previews use the bounded source reader, and indexed storage never gains copied source bodies.'
    - 'INV-SOUND-AST-FALLBACK: Function-body token indexing is out of scope (evaluated and rejected in milestone 030 under density limits); pattern search relies on the existing sound bounded AST candidate fallback and never adds a parallel FTS table or drops valid body-literal matches.'
    - 'INV-REJECT-UNKNOWN-ARGUMENTS: Unrecognized request fields are rejected before query execution with rejected_arguments validation, preventing malformed or misspelled path arguments from widening search scope.'
    - 'INV-PARITY-AND-COORDINATION: Extend completed milestone 050 navigation ergonomics (promoting ambiguous text errors to machine-readable JSON envelopes and consolidating inspect_selector); keep CLI and MCP semantics equivalent across path parameters and depth defaults.'
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

### task: bounded-search-result-navigation

```yaml
task_ref: bounded-search-result-navigation
target: Return indexed signatures and bounded source previews with a precise next call while retaining the MCP response ceiling
proof_policy: seam-test-first
contract_revision: 5
scope:
- src/mcp/
- src/cli/mod.rs
- src/cli/guide.rs
- src/db/symbols.rs
- docs/reference/
- tests/mcp_context.rs
- tests/query_context/
depends_on:
- path-scoped-selector-resolution
status: completed
subtasks:
- subtask_ref: indexed-signature-and-preview
  title: Each code_map_search hit exposes its signature from the existing nodes.details projection without source reads or schema modifications; source preview is attached only when total is at most three or include_preview=true, with no more than three hits, 20 lines per hit, and 12 KiB combined preview bytes
  status: completed
  evidence: '`cargo test --test mcp_context stdio_search_previews_and_impact_depth_defaults_are_bounded -- --nocapture` passed. Four matching functions expose signatures from `nodes.details` and inspect arguments; previews are omitted by default above three hits, and an explicit request attaches exactly three previews capped at 20 lines and at most 12 KiB combined. The handler performs one batched metadata lookup, no schema change, and uses the digest-verified source reader only for permitted previews.'
- subtask_ref: exact-next-call-and-selector-grammar
  title: Every compact search result includes the unified inspect_selector for a direct code_map_inspect call with show_source=true; selector field descriptions document exact-id, path:name, path:known_kind:name, path:line, and path#Lline forms; ast_grep_search tool schema and forge_guide("ast") explicitly document the profile capability matrix (complete_syntax, declaration_without_body, fragment_probe, attribute) and wildcards ($NAME, $$$SEQ)
  status: completed
  evidence: '`cargo test --test mcp_context stdio_search_previews_and_impact_depth_defaults_are_bounded -- --nocapture` passed. MCP tool-list schema asserts selector grammar and AST capability text; public `forge_guide(topic=ast)` returns complete_syntax, declaration_without_body, fragment_probe, attribute, the profile matrix, `$NAME`, and `$$$SEQ`. `stdio_compact_navigation_preserves_symbol_and_page_contracts` passes with direct `code_map_inspect` arguments in each hit.'
- subtask_ref: impact-depth-default-and-budgets
  title: Omitted MCP code_map_impact depth and CLI query impact default to one edge via a dedicated impact_depth helper (CLI default_value_t = 1) while code_map_query and CLI query run preserve their depth=2 default; keep every response within the 64 KiB protocol ceiling. Use the milestone cold-index receipt as its sole timing observation; collect no warm latency samples and run no benchmark.
  status: completed
  evidence: '`cargo test --test mcp_context -- --nocapture` passed (35/35) and `cargo test --test query_context -- --nocapture` passed (40/40); strict Clippy passed with -D warnings. Public MCP and CLI tests prove dedicated impact defaults to depth 1 while generic query and query run retain depth 2, including their observable caller results. MCP framing enforces MAX_OUTPUT_BYTES. No warm latency or benchmark ran; only the task1 cold-index receipt is retained.'
receipt:
  commit: 33aac23573a2e3f48ef5c299b281b86e12373318
  contract_revision: 5
  passed_at: 2026-10-08T00:52:07.920178239+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets --quiet
      exit_code: 0
      tests_passed: 640
      tests_failed: 0
      log: 'All targets passed: 640 passed, 0 failed, 3 ignored. Strict cargo clippy --all-targets --all-features -- -D warnings passed. commitment_integrity passed 14/14. Task3 seams passed: mcp_context 35/35 and query_context 40/40 (earlier targeted run); full suite includes these tests. No benchmark or warm latency sample was run.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed Task3 changes in src/db/symbols.rs, src/mcp/tools.rs and inputs.rs, src/cli/mod.rs, src/cli/guide.rs, and docs/reference; these are all within the declared scope. Separate Task1/Task2 compatibility repairs were reviewed under their owners.
        claims:
          applicable: true
          evidence: Task3 subtask evidence covers batched indexed signatures, direct inspect_selector, bounded verified previews, AST guide/schema, impact defaults, and 64 KiB framing. Full suite passed 640/640; strict Clippy and commitment_integrity passed.
        concurrency:
          applicable: true
          evidence: No subagents ran. Task1, Task2, and Task3 changes use their declared domains; shared MCP handler edits were reconciled with adjacent task owners.
        project_isolation:
          applicable: true
          evidence: Changes and verification remained in the dedicated 042 worktree; cold Commerce index was measured once and source remained read-only.
        administration:
          applicable: true
          evidence: Reviewer worker differs from builder; contract revision 5 matches. No warm benchmark ran and no commit or push was performed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-PROFILE-AST-PATTERN: Shared pattern search has no language-name dispatch; grammar-specific capabilities (complete syntax, declaration body kinds across syntax forms, fragment probes for root-rejected expressions and macros, attributes) are provided through the registered LanguageProfile contract.'
    - 'INV-PROBE-WRAPPER-ISOLATION: Removing a synthetic probe wrapper must affect only the probe container node and unwrap to the target AST node; it must not change match semantics of the user pattern or strip nodes of legitimate declaration patterns.'
    - 'INV-SOUND-AST-PREFILTER: Candidate narrowing may reject a file only when every required pattern token is represented by the selected index; body-only identifiers and literals rely on sound complete AST candidate fallback and cannot become false negatives.'
    - 'INV-SELECTOR-SCOPE: An explicit path constrains candidates before ambiguity resolution and never changes workspace or project boundaries.'
    - 'INV-FAIL-CLOSED-AMBIGUITY: Ambiguous selectors return a structured machine-readable JSON outcome with bounded candidates and never choose a target implicitly.'
    - 'INV-BOUNDED-NAVIGATION: Search, preview, ambiguity, and continuation outputs stay within the 64 KiB response ceiling; performance values are recommendations unless admitted by the owning milestone.'
    - 'INV-NO-SOURCE-MIRROR: Signatures are projected strictly from existing nodes.details without reading source or mutating database schema/projections; previews use the bounded source reader, and indexed storage never gains copied source bodies.'
    - 'INV-SOUND-AST-FALLBACK: Function-body token indexing is out of scope (evaluated and rejected in milestone 030 under density limits); pattern search relies on the existing sound bounded AST candidate fallback and never adds a parallel FTS table or drops valid body-literal matches.'
    - 'INV-REJECT-UNKNOWN-ARGUMENTS: Unrecognized request fields are rejected before query execution with rejected_arguments validation, preventing malformed or misspelled path arguments from widening search scope.'
    - 'INV-PARITY-AND-COORDINATION: Extend completed milestone 050 navigation ergonomics (promoting ambiguous text errors to machine-readable JSON envelopes and consolidating inspect_selector); keep CLI and MCP semantics equivalent across path parameters and depth defaults.'
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
