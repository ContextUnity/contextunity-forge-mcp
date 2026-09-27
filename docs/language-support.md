# Language support: current behavior and limits

See [language profiles and extension guide](language-profiles.md) for the current
pipeline, automatic registration, supported extensions and each profile's limits.

## Implemented reliability and cost improvements

- Language rules belong to immutable `LanguageProfile` implementations. The
  scanner and linker select profiles through the generated registry. Adding a
  provider requires no central dispatch edit. Cargo features select compiled
  grammars; the default is Python, Rust, TS/JS, Vue and Proto. Other modes,
  including Go, are optional.
- Exact imports require complete module namespace, language family and workspace
  evidence. Suffix matching remains a discovery aid rather than dependency proof.
  Missing or ambiguous aliases cannot fall through to unrelated declarations.
- Module conventions are language-specific: Python preserves `index.py` and
  `mod.py`, while package `__init__` has Python package semantics.
- The compiled profile fingerprint participates in index semantics. Changes to
  providers, shared profile support, grammar dependencies or enabled features invalidate stored
  analysis before it is reused.
- Builtins require an unshadowed language-level binding. Unknown receiver types,
  function-pointer targets, bare Ruby invocations and recognized implicit or
  constructor forms retain unresolved evidence where identity is unproved.
- TypeScript prepares default-export bindings once per parsed file. A hash set
  tracks symbol IDs, and AST identities bind inline and parenthesized handlers.
- Vue uses Tree-sitter absolute byte/point offsets across script blocks.
- Proto RPC references participate in persisted dependencies and removal checks.
- Cycle analysis uses integer node keys and serializes selected output. It
  retains cross-file cycles with a 500,000-edge computation limit, two-second
  budget and 8 MiB output bound.
- Delta records both endpoint dependencies for every edge kind. Provider changes,
  including an ambiguous new provider without the imported symbol, invalidate
  consumers. Replaced endpoints are checked before commit.
- Component ownership changes are computed from old and new module owners.
  A body edit no longer refreshes every component representative. Delta reports
  parsed files, loaded Facts, rewritten files and phase times so remaining
  costs can be measured separately.
- Documentary suffix lookup is constructed only when needed. Exact code links
  retain complete namespace checks; reusable local lookup buffers reduce
  temporary string allocation.
- MCP validates source inventory before reads. Its compact SQL projections,
  bounded pages, source previews and final 64 KiB response ceiling protect
  context use. See [context protection](context-protection-plan.md).

## Implemented language boundaries

The engine supports detailed language semantics for the core supported languages:

- **Go**: Package-level module naming (`package <name>` participating in qualname) and sibling file visibility (`sibling_accessible`), allowing free functions across sibling files within the same package directory to link without explicit imports.
- **JS/TS**: Extension support for `.mjs`, `.cjs`, `.mts`, and `.cts`. Workspace root-aware `package.json#exports` resolution (supporting subpath exports and monorepo packages/crates/libs layout).
- **Vue**: Template bindings parser extracting component references (`<Component>`), event callback calls (`@click="handler"`), prop bindings (`:prop="val"`), and template interpolations (`{{ expr }}`).
- **Rust**: Basic macro tracking (`macro_rules!` definitions and calls with `"macro"` kind and `"macro"` prefix), standard built-in macros (`vec`, `println`, `assert`, etc.), and Cargo workspace-aware `crate::` / member crate import path resolution.
- **Proto**: Package-qualified types via `package <name>;` declaration parsing and message field type references extracted as relationships for non-primitive types.

## Remaining language boundaries

| Area | Current limitation |
| --- | --- |
| Module roots | Adapter scan roots do not establish runtime package roots. |
| Go | Build tags, CGo directives and external `_test` packages across directories are not modeled. |
| JS/TS | `tsconfig.json` path mappings and baseUrl are not resolved. |
| Vue | Complex embedded JavaScript expressions inside `<script setup>` macros are not fully resolved. |
| Rust | Procedural macro expansion, `include!` topology and conditional compilation (`#[cfg(...)]`) are not evaluated. |
| Proto | Public imports (`import public "..."`) are not transitively exposed to downstream consumers. |
| Java/C#/Kotlin | Classpath/assembly imports, overloads and inferred receivers remain conservative. |
| PHP/Ruby | Autoloading, metaprogramming and implicit dispatch exceed syntax-only resolution. |
| C/C++ | Preprocessing, compiler include paths, templates and operator dispatch need external build evidence. |

A successful parse does not establish complete runtime dependency coverage.
Unknown references limit removal and test-mapping conclusions.

## Verification boundary

`tests/language_profiles.rs` exercises import identity, automatic registry
contracts, persisted new-language fixtures, unresolved invocations and delta
versus a full rebuild. `tests/profile_review_regressions.rs` covers owner-module
priority, Ruby expression roles, C/C++ declarators, PHP receiver identity and
Java method/value namespaces through persisted queries and removal checks.
Existing `tests/audit_regressions.rs` protects admission,
removal evidence, Rust aliases, decorator scope, cycles, routes, Vue coordinates
and Proto RPC dependencies. These checks do not establish complete grammar
coverage, compiler-equivalent inference or crash recovery.

## Language-refactor release measurements

Measured on 2026-09-27 with four Rayon threads, one warmup and five alternating
baseline/candidate samples. Each ordinary corpus has 100 files and 1,000 simple
definitions; `large_ts` has one file with 4,000 functions. The baseline and candidate
run on identical paths and inputs. No other task builds run during measurement.

Median complete-build times in milliseconds:

| Corpus | Baseline | Profiles and seven new grammars | Change |
| --- | ---: | ---: | ---: |
| python | 109.23 | 112.61 | +3.1% |
| typescript | 125.83 | 136.52 | +8.5% |
| javascript | 140.96 | 135.11 | -4.2% |
| rust | 129.08 | 109.91 | -14.9% |
| go | 100.06 | 108.86 | +8.8% |
| proto | 127.99 | 129.64 | +1.3% |
| vue | 183.08 | 136.76 | -25.3% |
| large_ts | 3674.35 | 694.96 | -81.1% |

Large-file TypeScript extraction alone changes from 3266.22 ms
to 256.22 ms. Small-file complete-build times vary across runs. Before the final
C++-only correction, one Go series measured +12.1%; an eleven-sample follow-up
measured -5.5%. Those earlier receipts remain available alongside the final +8.8%
series. These fixtures do not establish a universal zero-overhead claim or a
consistent Go slowdown. Parsing/extraction improves in every final corpus;
complete-build times also include persistence and filesystem costs.
Nodes, edges and resolution status match across both binaries on all eight
corpora. The richer 11-file compatibility fixture also matches symbol metadata,
52 nodes, 65 edges and 28 coverage rows.

The measured language-refactor executable grows from 12.58 MiB to
28.76 MiB with every additional grammar compiled. The current default feature
selection excludes the optional grammars; this older comparison describes the
full-language configuration before the incremental/context changes.
This is a distribution-size cost; the registry creates parsers only for selected
files. The measurements do not constitute memory-use profiling.

[Raw samples and binary digests](/tmp/forge-language-refactor/performance/results.json).
The session runner is `/tmp/forge-language-refactor/compare_releases.py`.

## Incremental and feature-build measurements

The incremental fixture contains 500 independent components, 500 Python files
and 51,000 nodes. Every file declares 100 uniquely named functions; the delta
changes one function body without introducing cross-file consumers. Measurements
on 2026-09-27 use four Rayon threads, one warmup and three alternating samples.

| Measure | Previous full-language binary | Current default binary |
| --- | ---: | ---: |
| Median complete build | 7.10 s | 7.15 s |
| Median one-file delta | 11.44 s | 1.06 s |
| Affected owners | 500 | 1 |
| Reparsed files | 1 | 1 |

The candidate loads zero persisted Facts files and rewrites one file. Its final
sample reports 3.1 ms extraction, 53.3 ms hydration, 0.7 ms linking, 116.6 ms
persistence, 129.8 ms sealing and 137.8 ms verification. The complete delta also
includes database admission and other work; phase times are not its entire
elapsed time. This fixture measures changes without cross-file consumers;
changes with many real dependents require a separate measurement. Complete-build
medians differ by about 0.8% in this sample.

Every incremental sample matches a clean build's owned nodes, local Facts,
edge occurrences, coverage and per-file commitments. Physical full-text-search
table layouts can differ, so complete database output hashes need not match.
Each database is independently admitted through commitment verification.

The current stripped release binaries measure **12.77 MiB** for the default
Python/Rust/TS+JS/Vue/Proto set and **29.13 MiB** for `all-languages`. The values
report executable file sizes. Resident memory is not profiled in this fixture.

[Raw measurements and binary hashes](/tmp/forge-mcp-evolution-20260927/matched-benchmark.json)
and [build/test matrix](/tmp/forge-mcp-evolution-20260927/gates.json).
