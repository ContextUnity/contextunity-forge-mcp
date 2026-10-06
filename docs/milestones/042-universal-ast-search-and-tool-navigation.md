---
id: m-universal-ast-search-and-tool-navigation
title: Universal AST search and path-scoped code navigation
doc_type: contract
status: planned
depends_on:
  - m-framework-manifests-and-parser-modularity
  - m-language-semantics-and-resolution-coverage
  - m-tool-performance-and-storage-compaction
owners:
  - src/engine/ast/
  - src/engine/languages/
  - src/cli/ast.rs
  - src/mcp/
  - src/db/reader.rs
  - src/db/symbols.rs
  - docs/reference/
  - tests/
  - benchmarks/
invariants:
  - 'INV-PROFILE-AST-PATTERN: Shared pattern search has no language-name dispatch; grammar-specific fragments are provided through the registered LanguageProfile contract.'
  - 'INV-SOUND-AST-PREFILTER: Candidate narrowing may reject a file only when every required pattern token is represented by the selected index; body-only identifiers and literals cannot become false negatives.'
  - 'INV-SELECTOR-SCOPE: An explicit path constrains candidates before ambiguity resolution and never changes workspace or project boundaries.'
  - 'INV-FAIL-CLOSED-AMBIGUITY: Ambiguous selectors return bounded candidates and never choose a target implicitly.'
  - 'INV-BOUNDED-NAVIGATION: Search, preview, ambiguity, and continuation outputs stay within the 64 KiB response ceiling; performance values are recommendations unless admitted by the owning milestone.'
  - 'INV-NO-SOURCE-MIRROR: Signatures come from the existing node projection and previews use the source reader; indexed storage never gains copied source bodies.'
  - 'INV-MILESTONE-030-OPTIMIZATION: Reuse milestone 030 body-token indexing and performance work; do not add a parallel FTS table or an unbounded fallback scan.'
  - 'INV-PLAN-COORDINATION: Coordinate AST, index, and response changes with milestones 030, 041, 050, and 051 at their existing subsystem boundaries; do not introduce a duplicate parser, linker, or token index.'
  - 'INV-CLI-MCP-PARITY: AST pattern preparation, selector outcomes, and pagination retain equivalent CLI and MCP semantics.'
related_plans:
  - docs/plans/042-universal-ast-search-and-tool-navigation.md
  - docs/milestones/030-tool-performance-and-storage-compaction.md
  - docs/milestones/041-framework-manifests-and-parser-modularity.md
  - docs/milestones/051-optical-token-compression.md
---

# Universal AST search and path-scoped code navigation

## Outcome and purpose

Define a language-profile-driven AST search contract and path-aware, bounded MCP navigation so queries work consistently across current and future language profiles without duplicating parser wrappers or losing source matches to an unsound text prefilter.

## Source plan notes

>
> # Universal AST search and path-scoped code navigation
>
> Keep grammar-specific fragment handling behind the extensible `LanguageProfile`
> contract and the shared `src/engine/ast` search pipeline. Carry a caller's path
> through every selector tool before ambiguity resolution, bound structured
> ambiguity and preview responses, and keep search output useful enough to guide a
> precise follow-up call. Reuse the existing indexed symbol metadata, source
> access, and the separate milestone 030 body-token plan; do not add a raw-source
> mirror or a second language-specific search implementation.
>
> The follow-on text-tool milestone consumes these stable response contracts. The
> optical-token hypothesis in milestone 051 continues to compare the final text
> surface from milestone 050 and does not add image transport for source, diffs, or
> signatures.

## Architecture and compatibility boundary

### AST pattern flow

`ast_grep_search` and the matching CLI command enter the shared search pipeline in
`src/engine/ast`. The pipeline obtains eligible paths through the existing file
and symbol indexes, asks the registered `LanguageProfile` to prepare the pattern,
parses it with that profile's Tree-sitter grammar, and verifies structural matches
against digest-checked source. Pattern preparation returns a typed result that
distinguishes complete syntax, supported fragments, and invalid syntax. The
shared layer owns wrappers, partial-body comparison, diagnostics, and candidate
filter safety; profiles provide grammar-specific examples and optional fragment
parsers through one defaultable extension point. A newly registered language
profile can use complete syntax immediately and add fragment forms without adding
branches to `search_range` or copies in individual language modules.

Function, class, struct, and `impl` patterns may omit a body or use an empty/body
wildcard when that fragment kind is supported by the profile. Removing a probe
wrapper must affect only the probe node; it must not change which source nodes
the user pattern can match. A syntax failure reports the language, the input
span of the Tree-sitter `ERROR`, and one valid example from that profile. Unsupported
fragment kinds fail with that diagnostic instead of returning a misleading empty
result.

Candidate filtering is sound for the text represented in `node_search`. The
already planned `function-body-search-tokens` task in milestone 030 owns
deduplicated identifier and string-literal indexing on existing function and
method documents. Milestone 042 consumes that index when available and adds no
second index. Until the indexed tokens can prove a query candidate, the pipeline
must use an existing bounded path strategy or return a typed budget outcome; it
must not silently filter out a valid body match, scan without a bound, or store
raw source in SQLite.

### Selector and search-result flow

Selector tool input carries an optional workspace-relative `path` to
`src/db/reader::select_detail` before candidate cardinality is evaluated. The
selector grammar preserves exact IDs, qualified names, `path:name`, `path::name`,
`path:line`, and `path#Lline`. It adds `path:kind:name` only when the middle token
is a registered node kind; `#` remains reserved for line coordinates. Existing
workspace and linked-project guards remain authoritative. Unknown arguments are
reported in `ignored_arguments` and cause the request to stop before query
execution, so a misspelled `path` cannot broaden a local request.

Ambiguity is a machine-readable `outcome: ambiguous` result, not a selected node.
It reports `total` and at most eight candidates with `id`, `path`, `line`,
`qualname`, and a byte-bounded `signature`. The envelope is a normal bounded JSON
result, so clients can narrow the path without parsing a long error string.
Search hits add the projected signature from `nodes.details`. At most three hits
may contain source previews; previews contain no more than 20 lines per hit and
12 KiB combined, and are fetched only through the existing bounded source reader.
Each compact hit page includes one ready-to-call `code_map_inspect` invocation
using an exact hit ID and `show_source: true`. The default impact depth is one;
an explicitly supplied depth keeps its current meaning.

### Producers and consumers

| Producer | Current owner | Consumer and observable result |
| --- | --- | --- |
| AST pattern request | `src/mcp/tools.rs`, `src/cli/ast.rs`, `src/engine/ast/` | Registered `LanguageProfile`; valid fragments return structural hits and invalid syntax returns a span diagnostic. |
| Selector plus optional path | `src/mcp/`, `src/db/reader.rs` | `select_detail` returns one path-scoped symbol or a bounded ambiguity envelope without guessing. |
| Search hit metadata and preview | `src/db/symbols.rs`, existing source reader | Search clients receive indexed signatures, optional bounded source, and an exact next-call suggestion. |
| Planned body tokens | Blocked milestone 030 `function-body-search-tokens` task | May support AST candidate prefilter only after density headroom and syntax coverage are proven; current general-pattern searches use the bounded indexed-file set and preserve body-only matches. Milestone 042 adds no second index. |
| Text output baseline | Completed milestone 050, then milestone 051 | Milestone 050 owns the stable text response contract; milestone 051 continues measuring that finalized text output. |

Milestone 041 keeps framework-specific rules in manifests consumed by
`LanguageLinker`; milestone 042 changes grammar and query preparation through
`LanguageProfile` and the shared AST layer only. It does not move framework rules
into parser adapters. Milestone 030 remains the owner of build throughput,
storage density, BM25 search, and function-body token indexing. No schema,
Merkle leaf, or persisted source format change is admitted here. The 050 text
response contract remains the baseline consumed by 051; AST search adds no image
transport or rasterized source content.

## Failure modes and containment

- A valid expression or incomplete declaration can be rejected by a whole-file
  parser: use a profile-declared fragment adapter and report its exact failure
  span when unsupported.
- A function-body literal can be removed by an unsound FTS prefilter: test the
  literal through the public AST search seam and only narrow when the relevant
  tokens are in the existing index.
- A dropped path argument can turn a local selector into a repository-wide
  ambiguity: stop on unknown fields, pass known paths before cardinality checks,
  and retain the workspace/project boundary.
- A large candidate list or preview can exceed the MCP response limit: cap
  candidates, signature bytes, preview count/lines/bytes, and preserve pagination.
- A language-specific adapter can drift from CLI or alter extraction costs:
  share preparation through `LanguageProfile`, verify both public surfaces, and
  keep search-only parsing out of the cold extraction path.

## Proof and performance observations

Use public CLI/MCP and persisted-reader seams. Parser tests are table-driven over
the currently registered Python, TypeScript/JavaScript, Rust, HTML, and Vue
profiles; a profile advertises only fragment kinds its grammar can prove. Include
positive structural cases, an exact pattern case, a body-only literal, malformed
syntax with its input span/example, same-name symbols under two paths, unknown
arguments, bounded ambiguity, maximum-size previews, and explicit/default impact
depth. Mutating the body-token candidate filter to omit a known literal must make
the public body-literal assertion fail; mutating path propagation to use an
unscoped selector must make the two-directory disambiguation assertion fail.

On the fixed Commerce workspace (3,901 files), run
`python3 benchmarks/mcp_tool_comparison_benchmark.py --bench --profile benchmarks/profiles/commerce-release-update.json --repeats 30 --scenario ast_pattern_search --output /tmp/m042-ast-pattern-search.json`.
The runner makes one unmeasured warmup call in the persistent warm process, then
30 measured calls; use its warm-process `wall_ms` samples and report median and
p95, excluding server startup and the warmup. Around 30 ms p95 is a useful AST
search comparison value, not an acceptance gate for this milestone. For each
affected selector/search tool, use the same warm-process method over 30 calls
with a fixed indexed target and record the result. For comparison, recommended
p95 values are around 10 ms for exact/prefix search, 30 ms for full-text search,
25 ms for inspect/explain, 50 ms for impact/tests, and 30 ms for prove_removal;
milestone 030 owns any active latency gates. A
  deliberate benchmark run with candidate narrowing disabled records the
  candidate-parse and p95 delta for comparison; this observation is not a gate.
  Every result, including eight
long signatures and maximum previews, remains within ADR 0011's 64 KiB ceiling.

Run the focused public-seam suites, `cargo test --test commitment_integrity`,
strict Clippy, and `cargo test --all-targets` before handoff. Record the same
Commerce tool-latency procedure and compare cold-build throughput and database
density with milestone 030's latest integrated measurement. Treat those values
as comparison context; this milestone does not inherit or change milestone
030's gates.

## Tasks in this milestone

### task: profile-driven-ast-fragment-search

```yaml
task_ref: profile-driven-ast-fragment-search
target: "Prepare and search AST patterns through a shared language-profile adapter, with sound candidate filtering and diagnosable fragment syntax"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/engine/ast/
  - src/engine/languages/
  - src/cli/ast.rs
  - src/mcp/tools.rs
  - tests/ast_extractors.rs
  - tests/language_profiles.rs
status: blocked
subtasks:
  - subtask_ref: rust-declaration-wildcards
    title: 'Through public ast_grep_search, `fn $NAME($$$ARGS) {}` structurally matches Rust function declarations and `pub struct Inspect { $$$FIELDS }` matches struct declarations; `fn yes() -> bool { true }` still returns its exact single match'
    status: pending
  - subtask_ref: body-literal-prefilter
    title: 'A Rust function containing the literal `"coverage_owner_language"` is returned by public ast_grep_search for that string, and candidate filtering cannot exclude it because the text occurs only in the function body'
    status: pending
  - subtask_ref: profile-fragment-diagnostics
    title: 'In a public ast_grep_search fixture with one target per language, `client.fetch($ARG)` returns exactly one structural match in Python, TypeScript, and Rust; `def $NAME($$$ARGS):`, `function $NAME($$$ARGS) {}`, and `fn $NAME($$$ARGS) {}` match their declarations while ignoring bodies, and `pub struct Inspect { $$$FIELDS }` matches its Rust fields; malformed input reports the profile language, exact nonempty ERROR span, and one valid example. LanguageProfile metadata supplies these fragment capabilities so registering a future profile adds no language-name branch to shared search_range.'
    status: pending
  - subtask_ref: ast-search-latency
    title: 'On commerce-release-update, record public ast_grep_search median and p95 for the registered ast_pattern_search scenario from 30 warm-process calls after one unmeasured warmup; report warm-process tool wall_ms and exclude server startup without treating the result as a milestone 042 pass/fail gate'
    status: pending
```

### task: path-scoped-selector-resolution

```yaml
task_ref: path-scoped-selector-resolution
target: "Carry explicit path scope through every selector tool before ambiguity resolution and return bounded structured ambiguity without selecting a target"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/mcp/
  - src/db/reader.rs
  - src/db/symbols.rs
  - src/db/traversal.rs
  - tests/mcp_context/
  - tests/query_context/
status: blocked
subtasks:
  - subtask_ref: selector-path-contract
    title: 'Public inspect, explain, snippet, impact, tests, and prove_removal requests accept optional path; path filtering occurs inside select_detail before ambiguity checks, and path:name, path::name, path:known_kind:name, path:line, and path#Lline resolve only within the selected workspace path'
    status: pending
  - subtask_ref: bounded-ambiguity-envelope
    title: 'Two same-named symbols in different directories produce a successful JSON outcome=ambiguous with total and at most eight candidates carrying id, path, line, qualname, and bounded signature; no candidate is selected and the response remains below 64 KiB'
    status: pending
  - subtask_ref: ignored-argument-and-query-scope
    title: 'An unrecognized request field is listed in ignored_arguments and causes no query execution; a valid path-scoped request uses indexed target candidates and issues no workspace-wide scan'
    status: pending
```

### task: bounded-search-result-navigation

```yaml
task_ref: bounded-search-result-navigation
target: "Return indexed signatures and bounded source previews with a precise next call while retaining the MCP response ceiling and recording performance observations"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/mcp/
  - src/db/symbols.rs
  - src/cli/guide.rs
  - docs/reference/
  - tests/query_context/
depends_on:
  - path-scoped-selector-resolution
status: blocked
subtasks:
  - subtask_ref: indexed-signature-and-preview
    title: 'Each code_map_search hit exposes its signature from nodes.details without source reads; source preview is attached only when total is at most three or include_preview=true, with no more than three hits, 20 lines per hit, and 12 KiB combined preview bytes'
    status: pending
  - subtask_ref: exact-next-call-and-selector-grammar
    title: 'Every compact search result includes one ready code_map_inspect call using an exact hit id and show_source=true; selector field descriptions document exact-id, path:name, path:known_kind:name, path:line, and path#Lline forms'
    status: pending
  - subtask_ref: impact-depth-default-and-budgets
    title: 'Omitted code_map_impact depth traverses one edge while explicit depth is unchanged; record public search, inspect/explain, impact/tests, and prove_removal latency samples for comparison, and keep every response within the 64 KiB protocol ceiling'
    status: pending
```
