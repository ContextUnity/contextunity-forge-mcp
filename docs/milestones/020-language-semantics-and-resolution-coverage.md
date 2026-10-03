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
related_plans:
- docs/plans/tool-performance-and-db-optimization.md
- docs/plans/architecture-and-modularity.md
started_at: 2026-10-02T17:06:10+00:00
---

# Language semantics and resolution coverage

## Outcome and purpose

Improve source-proven resolution in Python, JavaScript, TypeScript, HTML,
Vue, and Rust across repositories. Keep `resolved`, `external`, `ambiguous`,
and `unresolved` distinct. Preserve lexical order, import provenance, project
isolation, and deterministic commitments. Each language has one root task;
its subtasks track independently verifiable optimizations and remaining work.

The controlled Commerce reference index covers 3,953 files. Classified means
`resolved + external` divided by all four resolution statuses. These measured
counts direct investigation; they do not authorize classification by a
receiver's spelling or a repository-specific path.

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

## Tasks in this milestone

### task: python-language-optimization

```yaml
task_ref: python-language-optimization
target: Improve Python imports, local value flow, and receiver members through source-proven providers
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/engine/ast/relations.rs
- src/engine/ast/routes.rs
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
status: planned
subtasks:
- subtask_ref: mapping-and-logger-methods
  title: Resolve proven dict and logging.Logger methods on local receivers
  status: completed
  evidence: cargo test --test python_semantics and typed_receiver_resolution pass
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
  status: pending
- subtask_ref: bounded-dynamic-callees
  title: Summarize uniquely proven factory and callback returns for dynamic call sites
  status: pending
- subtask_ref: inferred-member-providers
  title: Resolve unique methods on inferred local receivers and fail closed on ambiguous providers
  status: pending
- subtask_ref: lexical-candidate-gaps
  title: Classify source-proven standard modules and declarations with no current lexical candidate
  status: pending
- subtask_ref: remaining-import-targets
  title: Resolve remaining package and re-export targets with import-line and project evidence
  status: pending
- subtask_ref: typed-parameter-members
  title: Carry Mapping and class annotation evidence into bounded parameter member lookup
  status: pending
```

The controlled Python index retains 18,816 shadowed binding cases, 5,177
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
contract_revision: 2
scope:
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
status: planned
subtasks:
- subtask_ref: commonjs-and-node-globals
  title: Link local require and exports and classify finite Node globals
  status: completed
  evidence: cargo test --test typescript_semantics commonjs and cargo test --test manifests pass
- subtask_ref: playwright-imported-returns
  title: Carry proven Playwright imports through bounded browser and page return chains
  status: completed
  evidence: cargo test --test typescript_semantics playwright_imported_return_chains passes
- subtask_ref: playwright-callback-parameters
  title: Type direct Playwright callback parameters from proven page calls
  status: completed
  evidence: cargo test --test typescript_semantics playwright_callback_parameters passes
- subtask_ref: alpine-object-members
  title: Resolve unique same-file Alpine.data object members and local this references
  status: completed
  evidence: cargo test --test typescript_semantics alpine_data_object_members passes
- subtask_ref: web-factory-results
  title: Propagate proven DOM factory and awaited fetch receiver types
  status: completed
  evidence: cargo test --test language_boundaries javascript_web_factory_results_keep_proven_receivers passes
- subtask_ref: lexical-and-try-scope
  title: Preserve CommonJS captures and scoped try bindings with shadow and rebind guards
  status: completed
  evidence: cargo test --test typescript_semantics playwright_imported_return_chains passes
- subtask_ref: object-and-array-receivers
  title: Resolve finite array and returned-object members through unique local providers
  status: completed
  evidence: cargo test --test typescript_semantics and commitment_integrity pass
- subtask_ref: route-call-precision
  title: Admit JavaScript routes only from explicit registrations and valid paths
  status: completed
  evidence: cargo test --test ast_extractors passes
- subtask_ref: local-receiver-provenance
  title: Trace remaining local and shadowed receivers across bounded calls and callbacks
  status: pending
- subtask_ref: dynamic-chain-provenance
  title: Resolve unique computed and optional call chains without name-based fallback
  status: pending
- subtask_ref: missing-lexical-candidates
  title: Add finite platform or local providers only when imports and source declarations prove them
  status: pending
- subtask_ref: object-member-providers
  title: Propagate unique object literal and factory member targets across bounded local scopes
  status: pending
- subtask_ref: typed-receiver-methods
  title: Verify finite Playwright and Web API method transitions on inferred receivers
  status: pending
- subtask_ref: unresolved-local-imports
  title: Resolve the remaining seven local or generated module targets with project isolation
  status: pending
```

The controlled JavaScript index retains 2,820 local or shadowed bindings,
2,055 dynamic callees, 932 references without a lexical candidate, 417 object
members without a unique provider, 375 inferred receivers without a verified
member, and 7 missing imports. The language target above 60% remains open at
58.20%; unknown `window.Alpine` plugins and arbitrary dynamic calls stay
unresolved until their provider is proven.

### task: typescript-language-optimization

```yaml
task_ref: typescript-language-optimization
target: Resolve TypeScript package and typed platform members from declarations and imports
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/languages/manifests.rs
- src/engine/languages/typescript.rs
- src/engine/languages/typescript/
- src/engine/linker.rs
- src/engine/linker/
- tests/typescript_semantics.rs
- tests/manifests.rs
- tests/language_boundaries.rs
status: planned
subtasks:
- subtask_ref: web-and-testing-builtins
  title: Classify finite DOM, Fetch, Playwright, and collection methods with origin
  status: completed
  evidence: cargo test --test typescript_semantics web_and_dom_builtins passes
- subtask_ref: lockfile-dependencies
  title: Attribute npm, pnpm, and yarn dependencies and scoped packages to lockfiles
  status: completed
  evidence: cargo test --test manifests passes
- subtask_ref: typed-dom-receivers
  title: Resolve verified DOM members on annotated platform receiver types
  status: completed
  evidence: cargo test --test typescript_semantics typed_dom_receiver_members passes
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
  status: pending
- subtask_ref: package-export-maps
  title: Follow package exports, TypeScript path aliases, and workspace package boundaries
  status: pending
- subtask_ref: typed-callback-and-return-flow
  title: Carry unique generic callback and function return types into member resolution
  status: pending
- subtask_ref: residual-cause-audit
  title: Classify remaining TypeScript unresolved by evidence before adding finite providers
  status: pending
```

The reference has 4,242 unresolved TypeScript references and 55.22%
classified coverage against the >80% target. The pending audit must separate
unsupported dynamic expressions from repairable source-proven providers.

### task: html-language-optimization

```yaml
task_ref: html-language-optimization
target: Resolve HTML template directives and symbols through verified framework registries
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/languages/html.rs
- src/engine/linker.rs
- src/engine/linker/
- tests/html_profile.rs
- tests/language_boundaries.rs
status: planned
subtasks:
- subtask_ref: django-jinja-builtins
  title: Classify standard Django and Jinja template tags and filters as framework builtins
  status: completed
  evidence: cargo test --test html_profile passes
- subtask_ref: custom-tag-filter-registries
  title: Link custom tags and filters to imported or registered template libraries
  status: pending
- subtask_ref: template-include-imports
  title: Resolve local include, extends, and macro imports with project-local paths
  status: pending
- subtask_ref: template-expression-flow
  title: Carry proven template context and macro bindings into expression lookup
  status: pending
- subtask_ref: residual-cause-audit
  title: Classify remaining HTML unresolved by parser evidence and framework dialect
  status: pending
```

The reference has 2,531 unresolved HTML references and 43.76% classified
coverage. Custom registration requires a verified provider; unknown filters
remain unresolved.

### task: vue-language-optimization

```yaml
task_ref: vue-language-optimization
target: Resolve Vue SFC templates and setup receivers through same-file and registered providers
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/core/models.rs
- src/engine/languages/vue.rs
- src/engine/languages/vue/
- src/engine/languages/manifests.rs
- src/engine/languages/typescript/value_flow.rs
- src/engine/linker.rs
- src/engine/linker/
- tests/language_boundaries.rs
- tests/incremental_scope.rs
status: planned
subtasks:
- subtask_ref: setup-compiler-macros
  title: Admit Vue compiler macros only in script setup and honor lexical shadowing
  status: completed
  evidence: cargo test --test language_boundaries vue_compiler_macros passes
- subtask_ref: setup-template-bridge
  title: Link same-file script setup bindings and component imports into templates
  status: completed
  evidence: cargo test --test language_boundaries vue_sfc_template_bridge passes
- subtask_ref: setup-callable-provenance
  title: Resolve verified defineEmits and useI18n callable results
  status: completed
  evidence: cargo test --test language_boundaries passes
- subtask_ref: typed-defineprops
  title: Resolve direct same-file typed defineProps members in template expressions
  status: completed
  evidence: cargo test --test language_boundaries vue_typed_defineprops_template_members passes
- subtask_ref: pinia-setup-actions
  title: Link direct setup-store actions from proven Pinia and Nuxt registration
  status: completed
  evidence: cargo test --test language_boundaries vue_pinia_setup_store_direct_actions passes
- subtask_ref: nested-props-members
  title: Carry verified nested and optional defineProps member types into templates
  status: pending
- subtask_ref: auto-registered-components
  title: Resolve components only from source-proven local imports or framework registration
  status: pending
- subtask_ref: composable-return-members
  title: Propagate unique composable return members and setup aliases into templates
  status: pending
- subtask_ref: residual-cause-audit
  title: Classify remaining Vue unresolved with SFC scope and registration evidence
  status: pending
```

The reference has 640 unresolved Vue references and 66.28% classified
coverage against the >75% target. Nested or optional `props` members and
registration-driven component names require additional source evidence.

### task: rust-language-optimization

```yaml
task_ref: rust-language-optimization
target: Keep Rust AST references precise and resolve unique standard and local receiver semantics
proof_policy: seam-test-first
contract_revision: 1
scope:
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
status: planned
subtasks:
- subtask_ref: non-symbol-noise
  title: Filter wildcards, lifetimes, and numeric tuple indexes from references
  status: completed
  evidence: 'cargo test --test typed_receiver_resolution rust_value_flow:: passes'
- subtask_ref: standard-library-provenance
  title: Attribute finite Rust prelude and imported stdlib calls with local shadow precedence
  status: completed
  evidence: cargo test --test builtin_call_coverage and commitment_integrity pass
- subtask_ref: constructor-receiver-flow
  title: Infer standard constructors and local Self or Workspace return receivers
  status: completed
  evidence: 'cargo test --test typed_receiver_resolution rust_value_flow:: passes'
- subtask_ref: macro-persistence
  title: Preserve external stdlib macro and indexed local macro identities
  status: completed
  evidence: cargo test --test language_boundaries rust_macros_and_cargo_workspace passes
- subtask_ref: trait-method-provenance
  title: Resolve uniquely selected trait methods through imports and impl evidence
  status: pending
- subtask_ref: associated-call-chains
  title: Carry bounded generic and associated return types into local member calls
  status: pending
- subtask_ref: residual-cause-audit
  title: Measure Rust unresolved on a representative Rust workspace and rank proven repairs
  status: pending
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
